#!/usr/bin/env python3
"""PreToolUse guardrail: deny actions this repo forbids, at the moment of action.

Wired into `.claude/settings.json` as a PreToolUse hook over Bash/Write/Edit.
Reads the hook payload as JSON on stdin; on a denied action it prints the
PreToolUse deny envelope and exits 0, otherwise it stays silent (allow).

What it denies:

- native build/infra/lint tooling (the `NATIVE_TOOLS` set: rust, db, infra,
  proto, go, js, formatters, linters) instead of the hermetic Bazel wrappers;
- destroying a lockfile (rm/truncate of MODULE.bazel.lock, Cargo.lock,
  requirements_lock.txt);
- creating a `mod.rs` instead of the `<dir>.rs` module layout.

Bash is scanned *recursively* so indirection can't launder a forbidden call:
the checks apply to the leading word of each `;`/`&&`/`||`/`|`/`&` segment and
also through `sh -c`/`bash -c` strings, `xargs`/`find -exec`/`env`/`sudo`/
`timeout`/... wrappers, and the post-`--` / `--run_under` arguments of `bazel`.
Inline or stdin Python (`python -c`, `python -`, a bare REPL) is denied
outright: arbitrary inline code can shell out and can't be verified.

Written shell scripts (a Write/Edit of a `.sh` file or a shell shebang) are
scanned the same way for native tooling and lockfile destruction, so a script
that invokes native tooling by bare name is caught. A Bazel-hosted script that
locates its tool via runfiles (`tool="$(rlocation …/cargo)"; exec "$tool"`)
passes, because the tool name is never in command position — only an `rlocation`
path argument. (The inline-Python deny is interactive-only: a heredoc
`python3 - <<EOF` in a reviewed build script is a normal idiom, not a bypass.)

Fails closed: a hook payload that can't be parsed as JSON, or an interactive
command with a segment that can't be tokenized, is denied. Valid payloads and
commands always parse, so this never fires in normal operation. When scanning
written content, a segment that defeats the tokenizer is skipped instead, so a
complex but legitimate script is not blocked by a weak parser.


"""

import json
import re
import shlex
import sys

# Native binaries that have a hermetic equivalent in this repo's Bazel
# definitions and so must go through it instead.  Maps the binary name to the
# wrapper to suggest in the denial reason.  Covers every toolchain/tool the repo
# provides hermetically (rust, db, infra, proto, go, js, formatters, linters) —
# not just the most common reaches.
NATIVE_TOOLS = {
    # Rust toolchain (@rust_host_tools).
    "cargo": "bazel run //tools:cargo -- … (or `bazel build` for compilation)",
    "rustc": "bazel run //tools:rustc -- …",
    "rustfmt": "bazel run //:format (or //tools:rustfmt)",
    # Database.
    "psql": "bazel run //infra/postgres:psql -- …",
    "sqlx": "bazel run //tools:sqlx -- …",
    # Infrastructure.
    "tofu": "bazel run //infra:tofu -- …",
    "terraform": "bazel run //infra:tofu -- … (the repo uses OpenTofu)",
    "aws": "bazel run //infra:aws -- …",
    # Proto.
    "protoc": "bazel run //tools:proto_gen (codegen via rules_proto)",
    "buf": "bazel test //... (buf runs as the proto lint aspect)",
    # Go (rules_go + gazelle).
    "go": "bazel build (Go runs under rules_go; no native go on PATH)",
    "gazelle": "bazel run //:gazelle",
    # JavaScript (aspect_rules_js).
    "node": "bazel build (JS runs under aspect_rules_js)",
    "npm": "edit .devcontainer/pnpm-lock.yaml; JS deps run under aspect_rules_js",
    "npx": "bazel build (JS tooling runs under aspect_rules_js)",
    "pnpm": "edit .devcontainer/pnpm-lock.yaml; deps run under aspect_rules_js",
    # Formatters (the //:format multirun).
    "taplo": "bazel run //:format",
    "shfmt": "bazel run //:format",
    "yamlfmt": "bazel run //:format",
    "ruff": "bazel run //:format",
    "buildifier": "bazel run //:format",
    # Linters (run as aspects / test targets).
    "yamllint": "bazel test //... (yamllint runs as a lint aspect)",
    "shellcheck": "bazel test //... (shellcheck runs as a lint aspect)",
    "pymarkdown": "bazel test //... (markdown lint runs via a test target)",
    # Java / TLA+ (hermetic Temurin JRE).
    "java": "TLC/TLA+ tooling runs on the hermetic JRE (see //infra/tla)",
    # Coverage tooling (@llvm).
    "llvm-cov": "coverage runs via `bazel coverage` / tools/coverage.sh (llvm-cov is @llvm)",
    "llvm-profdata": "coverage runs via `bazel coverage` / tools/coverage.sh (llvm-profdata is @llvm)",
    # Python deps (rules_python).
    "pip": "add deps to requirements.in and run `bazel run //:requirements.update`",
    "pip3": "add deps to requirements.in and run `bazel run //:requirements.update`",
}

# PostgreSQL binaries from the hermetic tarball (infra/postgres).  Only psql has
# a run wrapper; the rest have none, so they are not permitted — a missing
# wrapper means the tool is off-limits, not exempt.
_PG_NOT_PERMITTED = (
    "//infra/postgres:psql for SQL — the other PostgreSQL binaries have no run "
    "wrapper and are not permitted"
)
NATIVE_TOOLS.update(
    dict.fromkeys(
        (
            "clusterdb",
            "createdb",
            "createuser",
            "dropdb",
            "dropuser",
            "ecpg",
            "initdb",
            "oid2name",
            "pg_amcheck",
            "pg_archivecleanup",
            "pg_basebackup",
            "pgbench",
            "pg_checksums",
            "pg_combinebackup",
            "pg_config",
            "pg_controldata",
            "pg_createsubscriber",
            "pg_ctl",
            "pg_dump",
            "pg_dumpall",
            "pg_isready",
            "pg_receivewal",
            "pg_recvlogical",
            "pg_resetwal",
            "pg_restore",
            "pg_rewind",
            "pg_test_fsync",
            "pg_test_timing",
            "pg_upgrade",
            "pg_verifybackup",
            "pg_waldump",
            "pg_walsummary",
            "postgres",
            "reindexdb",
            "vacuumdb",
            "vacuumlo",
        ),
        _PG_NOT_PERMITTED,
    )
)

# protoc-gen-doc: a protoc plugin (docs), no standalone wrapper — not permitted.
NATIVE_TOOLS["protoc-gen-doc"] = (
    "the //docs target — protoc-gen-doc is a plugin, not permitted as a standalone CLI"
)

# Lockfiles whose destruction is forbidden — regenerate them in place.
LOCKFILES = {
    "MODULE.bazel.lock": "bazel mod deps --lockfile_mode=update",
    "Cargo.lock": "bazel run //tools:cargo -- generate-lockfile",
    "requirements_lock.txt": "the requirements lockfile workflow",
}

# Shells whose `-c <string>` argument is itself a command to recurse into.
_SHELLS = {"sh", "bash", "dash", "zsh", "ksh", "ash", "mksh"}

# Verbs that destroy a file's contents.
_DESTRUCTIVE_VERBS = {"rm", "unlink", "shred", "truncate"}

# Commands that run a trailing (post-option) argument as a subcommand.
_COMMAND_WRAPPERS = {
    "env",
    "sudo",
    "doas",
    "nohup",
    "setsid",
    "nice",
    "ionice",
    "stdbuf",
    "time",
    "command",
    "exec",
    "builtin",
    "xargs",
    "timeout",
    "watch",
}

# python / python3 / python3.12 / python2 — matched on the basename.
_PYTHON_RE = re.compile(r"^python[0-9.]*$")

# A single `>` (truncate), not `>>` (append), pointing at a lockfile.
_TRUNCATE_REDIRECT = re.compile(
    r"(?<!>)>(?!>)\s*['\"]?(" + "|".join(re.escape(n) for n in LOCKFILES) + r")\b"
)

_MAX_DEPTH = 6

_PYTHON_REASON = (
    "Refusing inline/stdin Python (`-c`, `-`, or a bare REPL): it can shell out to "
    "native tooling and can't be verified. Put the logic in a script file run "
    "through the hermetic toolchain, or use the appropriate `bazel run` target."
)

_TOO_DEEP_REASON = "Refusing: command nests subcommands too deeply to verify; denying."

_MOD_RS_REASON = (
    "Refusing to create a `mod.rs`. This project uses the `<dir>.rs` module layout: "
    "put the module in a sibling file (e.g. `foo.rs` with `foo/` for its submodules), "
    "never `foo/mod.rs`."
)

# Written files scanned as shell: by extension, or by a shell shebang.
_SHELL_EXTS = (".sh", ".bash", ".ksh", ".zsh", ".dash")

# Shell keywords that introduce a command (skipped to reach the real command).
_SHELL_KEYWORDS = {"if", "then", "elif", "else", "while", "until", "do", "!"}


def _basename(token):
    return token.rsplit("/", 1)[-1]


def _is_assignment(token):
    return re.match(r"^\w+=", token) is not None


def _native_reason(base):
    return (
        f"Refusing native `{base}`. This repo is Bazel-only and native toolchains "
        f"are not on PATH (results would be non-hermetic). Use: {NATIVE_TOOLS[base]}"
    )


def _lockfile_reason(name, deleting):
    verb = "delete" if deleting else "truncate"
    return (
        f"Refusing to {verb} {name}. Regenerate it in place with {LOCKFILES[name]}; "
        f"destroying it drops extension facts CI computes but this container cannot, "
        f"so CI then rejects the lockfile."
    )


def _consume_inside_quote(command, i, quote):
    """Inside an open quote: advance past one literal char, escape, or the closing quote."""
    c = command[i]
    if c == "\\" and quote == '"' and i + 1 < len(command):
        return 2, quote
    if c == quote:
        return 1, None
    return 1, quote


def _consume_unquoted_special(command, i):
    """Outside quotes: if `i` opens an escape or quote, advance past it; else None."""
    c = command[i]
    n = len(command)
    if c == "\\" and i + 1 < n:
        return 2, None
    if c in ("'", '"'):
        return 1, c
    return None


def _consume_quote_or_escape(command, i, quote):
    """Advance past a quoted or escaped run at `i`; None if `i` is an ordinary char.

    Shared by `_split_segments` and `_strip_comments` so the quote state machine
    isn't duplicated.
    """
    if quote:
        return _consume_inside_quote(command, i, quote)
    return _consume_unquoted_special(command, i)


def _split_segments(command):
    """Split a command on shell operators (; && || | & newline) outside quotes."""
    segments = []
    current = []
    quote = None
    i = 0
    n = len(command)
    while i < n:
        consumed = _consume_quote_or_escape(command, i, quote)
        if consumed is not None:
            length, quote = consumed
            current.append(command[i : i + length])
            i += length
            continue
        if command[i : i + 2] in ("&&", "||"):
            segments.append("".join(current))
            current = []
            i += 2
            continue
        c = command[i]
        if c in (";", "|", "&", "\n"):
            segments.append("".join(current))
            current = []
            i += 1
            continue
        current.append(c)
        i += 1
    segments.append("".join(current))
    return segments


def _shell_c_arg(rest):
    """Return the argument to a shell `-c` (including clusters like `-lc`), or None."""
    for k, t in enumerate(rest):
        if t == "--":
            return None
        if t.startswith("-") and not t.startswith("--") and "c" in t:
            return rest[k + 1] if k + 1 < len(rest) else None
    return None


def _python_reason(rest):
    """Deny inline (`-c`), stdin (`-`), or bare Python; allow a script or `-m`."""
    skip_value = False
    for t in rest:
        if skip_value:
            return None  # the `-m module` or first positional — a real program
        if t in ("-c", "-"):
            return _PYTHON_REASON
        if t == "-m":
            skip_value = True
            continue
        if t.startswith("-"):
            continue
        return None  # a script file
    return _PYTHON_REASON  # nothing to run but stdin → a REPL


def _resolve_wrapped(wrapper, rest):
    """The subcommand a wrapper runs: skip its options (+ a duration for timeout)."""
    i = 0
    while i < len(rest) and (_is_assignment(rest[i]) or rest[i].startswith("-")):
        i += 1
    if wrapper == "timeout" and i < len(rest):
        i += 1  # the DURATION positional
    return rest[i:]


def _consume_exec_args(tokens):
    """Tokens of an -exec command body, up to its terminating ; or +."""
    out = []
    for x in tokens:
        if x in (";", "\\;", "+"):
            break
        if x == "{}":
            continue
        out.append(x)
    return out


def _find_exec_tokens(rest):
    """The command run by `find ... -exec/-execdir CMD … ;/+`, or None."""
    for k, t in enumerate(rest):
        if t in ("-exec", "-execdir"):
            return _consume_exec_args(rest[k + 1 :])
    return None


def _bazel_indirections(rest):
    """Token-lists that bazel will hand to a shell: post-`--` args and `--run_under`."""
    out = []
    for k, t in enumerate(rest):
        if t == "--":
            after = rest[k + 1 :]
            # Recurse only when bazel hands its args to a shell, e.g.
            # `bazel run //x -- bash -c 'cargo'`.  Otherwise post-`--` tokens are
            # arguments to the run target, not a command — e.g.
            # `bazel run //infra:tofu -- infra/terraform output` passes the
            # config dir to tofu; `infra/terraform`'s basename is not a tool.
            if after and _basename(after[0]) in _SHELLS:
                out.append(after)
            break
        if t.startswith("--run_under="):
            out.append(shlex.split(t.split("=", 1)[1], posix=True))
        elif t == "--run_under" and k + 1 < len(rest):
            out.append(shlex.split(rest[k + 1], posix=True))
    return out


def _strip_token_prefix(tokens):
    """Drop leading `VAR=value` assignments and shell keywords (`if`, `then`, …)."""
    i = 0
    while i < len(tokens) and (_is_assignment(tokens[i]) or tokens[i] in _SHELL_KEYWORDS):
        i += 1
    return tokens[i:]


def _check_lockfile_args(rest):
    for arg in rest:
        if _basename(arg) in LOCKFILES:
            return _lockfile_reason(_basename(arg), deleting=True)
    return None


def _check_shell(rest, depth, interactive):
    inner = _shell_c_arg(rest)
    return _check_command(inner, depth + 1, interactive) if inner is not None else None


def _check_find(rest, depth, interactive):
    exec_tokens = _find_exec_tokens(rest)
    return _check_tokens(exec_tokens, depth + 1, interactive) if exec_tokens else None


def _check_bazel(rest, depth, interactive):
    for sub in _bazel_indirections(rest):
        reason = _check_tokens(sub, depth + 1, interactive) if sub else None
        if reason:
            return reason
    return None


def _check_wrapper(base, rest, depth, interactive):
    wrapped = _resolve_wrapped(base, rest)
    return _check_tokens(wrapped, depth + 1, interactive) if wrapped else None


def _dispatch_base(base, rest, depth, interactive):
    """Per-basename handler dispatch, or None when nothing further applies."""
    if base in _DESTRUCTIVE_VERBS:
        return _check_lockfile_args(rest)
    if base in _SHELLS:
        return _check_shell(rest, depth, interactive)
    if base == "find":
        return _check_find(rest, depth, interactive)
    if base == "bazel":
        return _check_bazel(rest, depth, interactive)
    if base in _COMMAND_WRAPPERS:
        return _check_wrapper(base, rest, depth, interactive)
    return None


def _check_tokens(tokens, depth, interactive):
    """Check one already-tokenized command (the command word + its args).

    `interactive` is true for the interactive command surface and false when
    scanning written script content. It governs two leniencies for content: an
    ad-hoc `python -c` is denied only when interactive (a heredoc in a reviewed
    build script is fine), and a segment that defeats the tokenizer fails closed
    only when interactive (content skips it, to not block a complex script).
    """
    if depth > _MAX_DEPTH:
        return _TOO_DEEP_REASON
    head = _strip_token_prefix(tokens)
    if not head:
        return None
    word = head[0]
    base = _basename(word)
    rest = head[1:]

    # A command word containing `$` is dynamically resolved — a variable or a
    # `$(rlocation …)` runfiles path — i.e. the hermetic-wrapper pattern, not a
    # bare native invocation. psql.sh runs `exec "$(rlocation …)/bin/psql"`,
    # whose basename is `psql`; the `$` is what marks it hermetic.
    if "$" not in word and base in NATIVE_TOOLS:
        return _native_reason(base)

    if interactive and _PYTHON_RE.match(base):
        return _python_reason(rest)

    return _dispatch_base(base, rest, depth, interactive)


def _skip_to_eol(command, i):
    n = len(command)
    while i < n and command[i] != "\n":
        i += 1
    return i


def _strip_comments(command):
    """Remove unquoted shell comments (`#` to end of line), quote-aware.

    A comment can't invoke anything, and markdown-style backticks inside comments
    (`# uses `cargo metadata``) would otherwise look like backtick substitutions.
    """
    out = []
    quote = None
    prev = "\n"
    i = 0
    n = len(command)
    while i < n:
        consumed = _consume_quote_or_escape(command, i, quote)
        if consumed is not None:
            length, quote = consumed
            chunk = command[i : i + length]
            out.append(chunk)
            prev = chunk[-1]
            i += length
            continue
        c = command[i]
        if c == "#" and prev in " \t\n;|&(":
            i = _skip_to_eol(command, i)
            continue
        out.append(c)
        prev = c
        i += 1
    return "".join(out)


def _find_paren_close(command, start):
    """End-of-`$(...)` index (exclusive) starting just after `$(`, or None if unbalanced."""
    depth = 1
    j = start
    n = len(command)
    while j < n and depth > 0:
        c = command[j]
        if c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
        j += 1
    return j if depth == 0 else None


def _consume_dollar_paren(command, i):
    """If `i` opens `$(...)`, return `(end_index, inner_text)`; else None."""
    n = len(command)
    if i + 1 >= n or command[i + 1] != "(":
        return None
    end = _find_paren_close(command, i + 2)
    if end is None:
        return None
    return end, command[i + 2 : end - 1]


def _consume_backtick(command, i):
    """If `i` opens `` `...` ``, return `(end_index, inner_text)`; else None."""
    j = command.find("`", i + 1)
    if j == -1:
        return None
    return j + 1, command[i + 1 : j]


def _command_substitutions(command):
    """Inner text of every $(...) and `...` substitution, for recursion."""
    subs = []
    i = 0
    n = len(command)
    while i < n:
        c = command[i]
        consumed = None
        if c == "$":
            consumed = _consume_dollar_paren(command, i)
        elif c == "`":
            consumed = _consume_backtick(command, i)
        if consumed is not None:
            end, inner = consumed
            subs.append(inner)
            i = end
            continue
        i += 1
    return subs


def _tokenize_segment(segment, interactive):
    """Tokenize a segment; on tokenizer failure raise (interactive) or return None (content)."""
    try:
        return shlex.split(segment, posix=True)
    except ValueError:
        if interactive:
            raise  # an interactive command we can't tokenize → fail closed
        return None  # written content: skip a gnarly segment, don't block legit scripts


def _check_substitutions(command, depth, interactive):
    for inner in _command_substitutions(command):
        reason = _check_command(inner, depth + 1, interactive)
        if reason:
            return reason
    return None


def _check_segments(command, depth, interactive):
    for segment in _split_segments(command):
        if not segment.strip():
            continue
        tokens = _tokenize_segment(segment, interactive)
        if not tokens:
            continue
        reason = _check_tokens(tokens, depth, interactive)
        if reason:
            return reason
    return None


def _check_command(command, depth, interactive=True):
    """Check a whole command string: redirects, $() bodies, then each segment."""
    if depth > _MAX_DEPTH:
        return _TOO_DEEP_REASON
    command = _strip_comments(command)
    match = _TRUNCATE_REDIRECT.search(command)
    if match:
        return _lockfile_reason(match.group(1), deleting=False)
    return _check_substitutions(command, depth, interactive) or _check_segments(
        command, depth, interactive
    )


def _looks_like_shell(file_path, content):
    """Whether written content should be scanned as a shell script."""
    if _basename(file_path).endswith(_SHELL_EXTS):
        return True
    head = content.lstrip()[:200].split("\n", 1)[0]
    return head.startswith("#!") and re.search(r"\b(sh|bash|dash|ksh|zsh)\b", head) is not None


def _check_write(file_path, content):
    if _basename(file_path) == "mod.rs":
        return _MOD_RS_REASON
    if content and _looks_like_shell(file_path, content):
        reason = _check_command(content, 0, interactive=False)
        if reason:
            return (
                f"In the shell script being written ({file_path}): {reason} "
                f"Bazel-hosted scripts locate tools via runfiles (e.g. "
                f"`rlocation rust_host_tools/bin/cargo`) and run the resolved path, "
                f"like tools/cargo.sh — not a bare invocation."
            )
    return None


def decide(payload):
    """Return a denial reason for a forbidden action, or None to allow."""
    tool = payload.get("tool_name", "")
    tool_input = payload.get("tool_input") or {}
    if tool == "Bash":
        return _check_command(tool_input.get("command") or "", 0)
    if tool in ("Write", "Edit"):
        content = tool_input.get("content") or tool_input.get("new_string") or ""
        return _check_write(tool_input.get("file_path") or "", content)
    return None


def evaluate(raw_stdin):
    """Return a denial reason for the payload, or None to allow.

    Fails closed at the payload boundary: if the JSON payload can't be parsed
    (or decide() raises), deny. A guard that can't read the action at all must
    not permit it. Valid hook payloads always parse, so this never fires in
    normal operation.
    """
    try:
        return decide(json.loads(raw_stdin))
    except Exception:
        return "agent_action_guard could not parse the action; denying"


def main():
    reason = evaluate(sys.stdin.read())
    if reason is None:
        return
    print(
        json.dumps(
            {
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "deny",
                    "permissionDecisionReason": reason,
                }
            }
        )
    )


if __name__ == "__main__":
    main()
