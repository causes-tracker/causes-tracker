#!/usr/bin/env python3
"""Authorise an agent edit to a quality-gate file for the current hour.

The gate files (see `--list`) define the rules the agent might want to bypass.
Editing one requires a token the agent cannot mint without leaning deep into
'bad' behaviour: the token name is a hash over the file's parent-commit content,
the UTC hour, its tree-relative path, and a key derived from the user's passphrase.
The verifier recomputes the name from the stored key and checks that the token exists.
The store, the key file, and the authorise command are denied to the agent's tools.

    buck2 run //tools:authorise -- --init         create the key file (prompts twice)
    buck2 run //tools:authorise -- PATH [PATH ...]  mint a token for each PATH (prompts once)
    buck2 run //tools:authorise -- --verify PATH  exit 0 if PATH is authorised this hour
    buck2 run //tools:authorise -- --match PATH   exit 0 if PATH is a controlled file
    buck2 run //tools:authorise -- --list         print the controlled patterns
    buck2 run //tools:authorise -- --hint         print the authorise command template
"""

import datetime as dt
import fnmatch
import getpass
import hashlib
import os
import secrets
import subprocess
import sys


class AuthoriseError(Exception):
    pass


def controlled_patterns() -> list[str]:
    """Files whose edits need a token: those that define or enforce a quality gate."""
    return [
        ".bazelignore",
        ".buckconfig",
        ".claude/settings.json",
        ".clippy.toml",
        ".github/workflows/*",
        ".shellcheckrc",
        ".yamlfmt",
        "rustfmt.toml",
        "tools/agent_action_guard.py",
        "tools/authorise.py",
        "tools/check.sh",
    ]


def controlled_path(path: str) -> str | None:
    """The tree-relative suffix of `path` naming a controlled file, or None.

    The longest trailing component sequence matching a controlled pattern wins,
    so an absolute path in any checkout and a `./`-prefixed path both resolve.
    """
    patterns = controlled_patterns()
    parts = os.path.normpath(path).split(os.sep)
    for i in range(len(parts)):
        rel = "/".join(parts[i:])
        if any(fnmatch.fnmatchcase(rel, pattern) for pattern in patterns):
            return rel
    return None


def hint(path: str) -> str:
    """What the user runs (from the buck2 workspace) to authorise an edit."""
    return f"buck2 run //tools:authorise -- {path}"


def hour_of(when: dt.datetime) -> str:
    return when.astimezone(dt.timezone.utc).strftime("%Y-%m-%dT%H")


def base_content(path: str) -> bytes:
    """The file as the parent commit has it; empty for a new file or without jj."""
    try:
        proc = subprocess.run(
            ["jj", "file", "show", "--ignore-working-copy", "-r", "@-", path],
            capture_output=True,
            check=False,
        )
        return proc.stdout if proc.returncode == 0 else b""
    except FileNotFoundError:
        return b""


def token_name(key: bytes, path: str, base: bytes, hour: str) -> str:
    return hashlib.sha256(base + hour.encode() + path.encode() + key).hexdigest()


def derive(passphrase: str, salt: bytes) -> bytes:
    return hashlib.pbkdf2_hmac("sha256", passphrase.encode(), salt, 600_000)


def init(passphrase: str, store: str | None = None) -> str:
    """Create the key file; returns its path."""
    store = store or os.path.expanduser("~/.cache/causes-development/authorisations")
    key_file = os.path.join(store, "authoriser")
    if os.path.exists(key_file):
        raise AuthoriseError(f"{key_file} exists; remove it to re-initialise")
    salt = secrets.token_bytes(16)
    os.makedirs(store, mode=0o700, exist_ok=True)
    fd = os.open(key_file, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with open(fd, "w", encoding="utf-8") as f:
        f.write(f"{salt.hex()} {derive(passphrase, salt).hex()}\n")
    return key_file


def authorise(
    paths: list[str],
    passphrase: str,
    store: str | None = None,
    now: dt.datetime | None = None,
    base: bytes | None = None,
) -> list[tuple[str, str]]:
    """Mint this hour's token for each path; returns the tree-relative paths and the hour."""
    store = store or os.path.expanduser("~/.cache/causes-development/authorisations")
    with open(os.path.join(store, "authoriser"), encoding="utf-8") as f:
        salt, stored = f.read().split()
    key = derive(passphrase, bytes.fromhex(salt))
    if key != bytes.fromhex(stored):
        raise AuthoriseError("wrong passphrase")
    hour = hour_of(now or dt.datetime.now(dt.timezone.utc))
    minted = []
    for path in paths:
        rel = controlled_path(path)
        if rel is None:
            raise AuthoriseError(f"{path} is not a controlled file")
        name = token_name(key, rel, base if base is not None else base_content(rel), hour)
        with open(os.path.join(store, name), "w", encoding="utf-8"):
            pass
        minted.append((rel, hour))
    return minted


def verify(
    path: str,
    store: str | None = None,
    now: dt.datetime | None = None,
    base: bytes | None = None,
) -> bool:
    """Whether a token for `path` exists for the current hour."""
    store = store or os.path.expanduser("~/.cache/causes-development/authorisations")
    try:
        with open(os.path.join(store, "authoriser"), encoding="utf-8") as f:
            key = bytes.fromhex(f.read().split()[1])
    except FileNotFoundError:
        return False
    hour = hour_of(now or dt.datetime.now(dt.timezone.utc))
    if base is None:
        base = base_content(path)
    return os.path.exists(os.path.join(store, token_name(key, path, base, hour)))


def _passphrase(prompt: str) -> str:
    if sys.stdin.isatty():
        return getpass.getpass(prompt)
    return sys.stdin.readline().rstrip("\n")


def main(argv: list[str]) -> None:
    try:
        if argv == ["--init"]:
            first = _passphrase("passphrase: ")
            if first != _passphrase("again: "):
                raise AuthoriseError("passphrases differ")
            print(f"created {init(first)}")
        elif len(argv) == 2 and argv[0] == "--verify":
            sys.exit(0 if verify(argv[1]) else 1)
        elif len(argv) == 2 and argv[0] == "--match":
            sys.exit(0 if controlled_path(argv[1]) is not None else 1)
        elif argv == ["--list"]:
            print("\n".join(controlled_patterns()))
        elif argv == ["--hint"]:
            print(hint("{path}"))
        elif argv and not any(a.startswith("-") for a in argv):
            for rel, hour in authorise(argv, _passphrase("passphrase: ")):
                print(f"authorised {rel} for the hour {hour}Z")
        else:
            sys.exit(__doc__)
    except AuthoriseError as e:
        sys.exit(str(e))


if __name__ == "__main__":
    main(sys.argv[1:])
