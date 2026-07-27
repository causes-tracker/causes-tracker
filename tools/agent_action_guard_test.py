"""Tests for the PreToolUse action guard (tools/agent_action_guard.py)."""

import os
import shlex
import unittest

from agent_action_guard import decide, evaluate


def bash(command):
    return {"tool_name": "Bash", "tool_input": {"command": command}}


def write(path, content=""):
    return {"tool_name": "Write", "tool_input": {"file_path": path, "content": content}}


def edit(path, content=""):
    return {"tool_name": "Edit", "tool_input": {"file_path": path, "new_string": content}}


class NativeDirectTest(unittest.TestCase):
    def test_bare_native_tools_are_denied(self):
        for cmd in (
            "cargo build",
            "cargo",
            "rustc --version",
            "psql -c 'select 1'",
            "tofu apply",
            "terraform plan",
            "yamllint .",
            "/usr/bin/cargo build",
            "FOO=1 cargo test",
            "ls && cargo build",
        ):
            self.assertIsNotNone(decide(bash(cmd)), f"should deny: {cmd}")

    def test_all_repo_hermetic_tools_are_denied(self):
        # Every tool the repo provides hermetically, invoked natively.
        for cmd in (
            "sqlx migrate run",
            "aws s3 ls",
            "protoc --version",
            "buf lint",
            "go build ./...",
            "gazelle",
            "node x.js",
            "npm install",
            "npx tsc",
            "pnpm install",
            "taplo fmt",
            "shfmt -w x.sh",
            "yamlfmt .",
            "ruff format x.py",
            "buildifier BUILD.bazel",
            "shellcheck x.sh",
            "pymarkdown scan x.md",
            "java -jar x.jar",
            "llvm-cov report",
            "llvm-profdata merge",
            "pip install x",
            "pip3 install y",
            # PostgreSQL binaries with no wrapper — not permitted.
            "pg_dump db",
            "pg_isready",
            "initdb -D x",
            "pg_ctl start",
            "postgres -D x",
            "createdb foo",
            "protoc-gen-doc",
        ):
            self.assertIsNotNone(decide(bash(cmd)), f"should deny: {cmd}")

    def test_bazel_wrappers_are_allowed(self):
        for cmd in (
            "bazel run //tools:cargo -- generate-lockfile",
            "bazel build //...",
            "bazel run //infra:tofu -- plan",
            "bazel run //infra/postgres:psql -- -c 'select 1'",
            "bazel test //...",
        ):
            self.assertIsNone(decide(bash(cmd)), f"should allow: {cmd}")

    def test_mentions_not_invocations_are_allowed(self):
        for cmd in (
            "echo cargo build",
            "grep cargo tools/check.sh",
            "git commit -m 'use cargo not native'",
            "cat Cargo.toml",
            "./tools/cargo.sh metadata",
            'echo "a && cargo b"',
        ):
            self.assertIsNone(decide(bash(cmd)), f"should allow: {cmd}")


class ShellDashCTest(unittest.TestCase):
    def test_shell_c_native_is_denied(self):
        for cmd in (
            "sh -c 'cargo build'",
            'bash -c "cargo build"',
            'bash -c "echo hi && cargo build"',
            "bash -lc 'cargo build'",
            "sh -c \"bash -c 'cargo'\"",
            "bash --norc -c 'rustc x.rs'",
        ):
            self.assertIsNotNone(decide(bash(cmd)), f"should deny: {cmd}")

    def test_shell_c_mention_is_allowed(self):
        for cmd in (
            "bash -c 'echo cargo'",
            "bash -c 'grep cargo file'",
            "bash deploy.sh",
        ):
            self.assertIsNone(decide(bash(cmd)), f"should allow: {cmd}")

    def test_lockfile_destruction_via_shell_c_is_denied(self):
        self.assertIsNotNone(decide(bash("bash -c 'rm Cargo.lock'")))
        self.assertIsNotNone(decide(bash('sh -c "echo x > MODULE.bazel.lock"')))


class WrapperTest(unittest.TestCase):
    def test_wrapped_native_is_denied(self):
        for cmd in (
            "xargs cargo",
            "xargs -n1 cargo build",
            "echo x | xargs cargo build",
            "env cargo test",
            "nohup cargo build",
            "sudo cargo",
            "timeout 30 cargo build",
            "time cargo build",
            "find . -name '*.rs' -exec cargo fmt {} +",
        ):
            self.assertIsNotNone(decide(bash(cmd)), f"should deny: {cmd}")

    def test_wrapped_non_native_is_allowed(self):
        for cmd in (
            "xargs grep cargo",
            "timeout 30 grep cargo",
            "find . -exec grep cargo {} ;",
        ):
            self.assertIsNone(decide(bash(cmd)), f"should allow: {cmd}")


class PythonInlineTest(unittest.TestCase):
    def test_inline_and_stdin_python_denied(self):
        for cmd in (
            "python3 -c 'import os'",
            "python -c 'x'",
            "python3 -",
            "python3",
            "python3 -B -c 'y'",
        ):
            self.assertIsNotNone(decide(bash(cmd)), f"should deny: {cmd}")

    def test_python_script_or_module_allowed(self):
        for cmd in (
            "python3 tools/agent_action_guard.py",
            "python3 script.py",
            "python3 -B tools/x.py",
            "python3 -m json.tool",
        ):
            self.assertIsNone(decide(bash(cmd)), f"should allow: {cmd}")


class BazelIndirectionTest(unittest.TestCase):
    def test_bazel_indirect_to_native_is_denied(self):
        for cmd in (
            "bazel run //x -- bash -c 'cargo build'",
            "bazel run --run_under='bash -c cargo' //x",
            "bazel run //x -- sh -c 'rm Cargo.lock'",
        ):
            self.assertIsNotNone(decide(bash(cmd)), f"should deny: {cmd}")

    def test_legitimate_bazel_is_allowed(self):
        for cmd in (
            "bazel run //tools:cargo -- generate-lockfile",
            "bazel run //tools:cargo -- build",
            "bazel build //...",
            "bazel run //infra/postgres:psql -- -c 'select 1'",
            # tofu takes the config dir as a post-`--` arg; basename is "terraform".
            "bazel run //infra:tofu -- infra/terraform apply",
            "bazel --quiet run //infra:tofu -- infra/terraform output -raw x",
        ):
            self.assertIsNone(decide(bash(cmd)), f"should allow: {cmd}")


class LockfileTest(unittest.TestCase):
    def test_direct_lockfile_destruction_denied(self):
        for cmd in (
            "rm MODULE.bazel.lock",
            "rm -f Cargo.lock",
            "truncate -s0 MODULE.bazel.lock",
            "echo x > Cargo.lock",
        ):
            self.assertIsNotNone(decide(bash(cmd)), f"should deny: {cmd}")

    def test_non_lockfile_rm_allowed(self):
        self.assertIsNone(decide(bash("rm -rf .coverage-green")))
        self.assertIsNone(decide(bash("echo x >> notes.txt")))


class DepthTest(unittest.TestCase):
    def test_deeply_nested_indirection_is_denied(self):
        nested = "cargo build"
        for _ in range(8):
            nested = "sh -c " + shlex.quote(nested)
        self.assertIsNotNone(decide(bash(nested)))


class ModRsTest(unittest.TestCase):
    def test_writing_mod_rs_is_denied(self):
        self.assertIsNotNone(decide(write("services/causes_api/src/foo/mod.rs")))
        self.assertIsNotNone(decide(edit("lib/rust/api_db/src/store/mod.rs")))

    def test_other_writes_allowed(self):
        self.assertIsNone(decide(write("services/causes_api/src/foo.rs")))
        self.assertIsNone(decide(write(".claude/settings.json")))
        self.assertIsNone(decide(write("README.md")))


class EvaluateFailsClosedTest(unittest.TestCase):
    def test_unparseable_payload_is_denied(self):
        self.assertIsNotNone(evaluate("this is not json"))
        self.assertIsNotNone(evaluate(""))

    def test_unparseable_command_is_denied(self):
        # Unbalanced quote → shlex raises → fail closed.
        self.assertIsNotNone(
            evaluate('{"tool_name": "Bash", "tool_input": {"command": "bash -c \\"oops"}}')
        )

    def test_parseable_allowed_action_is_silent(self):
        self.assertIsNone(
            evaluate('{"tool_name": "Bash", "tool_input": {"command": "bazel build //..."}}')
        )

    def test_unguarded_tool_is_allowed(self):
        self.assertIsNone(decide({"tool_name": "Read", "tool_input": {"file_path": "x"}}))
        self.assertIsNone(decide({}))


_RUNFILES_WRAPPER = (
    "#!/usr/bin/env bash\n"
    'tool="$(rlocation rust_host_tools/bin/cargo)"\n'
    'cd "${BUILD_WORKSPACE_DIRECTORY}"\n'
    'exec "$tool" "$@"\n'
)


class ContentScanTest(unittest.TestCase):
    def test_written_shell_with_bare_native_is_denied(self):
        for body in (
            "cargo build --release",
            "if true; then cargo test; fi",
            "V=$(cargo --version)",
            "X=`rustc --version`",
            "/usr/bin/cargo build",
            "rm Cargo.lock",
            'psql -c "select 1"',
            "bash -c 'cargo build'",
        ):
            script = "#!/bin/bash\n" + body + "\n"
            self.assertIsNotNone(decide(write("tools/new.sh", script)), f"deny: {body}")

    def test_runfiles_located_tool_is_allowed(self):
        self.assertIsNone(decide(write("tools/new.sh", _RUNFILES_WRAPPER)))

    def test_runfiles_path_ending_in_tool_name_is_allowed(self):
        # psql.sh runs `exec "$(rlocation …)/bin/psql"`: the command word's
        # basename is `psql`, but the `$` marks it as a runfiles path.
        script = (
            "#!/bin/bash\n"
            'exec "$(rlocation _main/infra/postgres/postgres_extracted)/bin/psql" "$@"\n'
        )
        self.assertIsNone(decide(write("infra/postgres/psql.sh", script)))

    def test_env_var_bin_path_is_allowed(self):
        # testfixture.sh runs `"$PGBIN/initdb"`; the `$` marks the path hermetic.
        script = (
            '#!/bin/bash\nPGBIN="$(rlocation x)/bin"\n"$PGBIN/initdb" -D d\n"$PGBIN/pg_ctl" start\n'
        )
        self.assertIsNone(decide(write("infra/postgres/testfixture.sh", script)))

    def test_inline_python_heredoc_in_script_is_allowed(self):
        # Reviewed build-script Python is fine; only the interactive surface denies it.
        script = "#!/bin/bash\npython3 - foo bar <<'PYEOF'\nprint(1)\nPYEOF\n"
        self.assertIsNone(decide(write("tools/new.sh", script)))

    def test_backticks_in_comment_are_not_invocations(self):
        script = "#!/bin/bash\n# invoke for `cargo metadata` and `cargo check`\ntrue\n"
        self.assertIsNone(decide(write("tools/new.sh", script)))

    def test_non_shell_files_are_not_scanned(self):
        self.assertIsNone(decide(write("README.md", "Run `cargo build` — forbidden.\n")))
        self.assertIsNone(decide(write("x.py", "cargo = 5\nsubprocess.run(['cargo'])\n")))

    def test_edit_hunk_adding_bare_cargo_is_denied(self):
        self.assertIsNotNone(decide(edit("tools/x.sh", "    cargo build\n")))


class HelperScriptsTest(unittest.TestCase):
    """Every current tools/*.sh wrapper must pass (they locate tools via runfiles)."""

    HELPERS = (
        "cargo.sh",
        "rustc.sh",
        "rustfmt.sh",
        "proto_gen_impl.sh",
        "sqlx_prepare_check.sh",
        "sqlx_prepare_update.sh",
        "toolchain_versions_test.sh",
        "sqlx-cli/sqlx.sh",
    )

    def test_helpers_are_not_blocked(self):
        here = os.path.dirname(os.path.abspath(__file__))
        for name in self.HELPERS:
            path = os.path.normpath(os.path.join(here, name))
            with open(path, encoding="utf-8") as handle:
                content = handle.read()
            self.assertIsNone(decide(write(path, content)), f"helper should pass: {name}")


if __name__ == "__main__":
    unittest.main()
