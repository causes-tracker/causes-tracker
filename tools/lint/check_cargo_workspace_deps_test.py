"""Tests for the workspace-dependency lint (check_cargo_workspace_deps.py)."""

import contextlib
import io
from pathlib import Path
import tempfile
import tomllib
import unittest

from check_cargo_workspace_deps import check_manifest, dep_violation, main


class DepViolationTest(unittest.TestCase):
    def test_workspace_reference_is_allowed(self):
        self.assertIsNone(dep_violation({"workspace": True}))

    def test_workspace_reference_with_features_is_allowed(self):
        self.assertIsNone(dep_violation({"workspace": True, "features": ["json"]}))

    def test_path_dependency_is_allowed(self):
        self.assertIsNone(dep_violation({"path": "../db_pool"}))

    def test_string_shorthand_is_flagged(self):
        self.assertIsNotNone(dep_violation("0.16"))

    def test_version_key_is_flagged(self):
        self.assertIsNotNone(dep_violation({"version": "1", "features": ["full"]}))

    def test_path_with_version_is_flagged(self):
        self.assertIsNotNone(dep_violation({"path": "../db_pool", "version": "0.1"}))

    def test_git_dependency_is_flagged(self):
        self.assertIsNotNone(dep_violation({"git": "https://example.com/x.git"}))


class CheckManifestTest(unittest.TestCase):
    def test_clean_manifest_has_no_violations(self):
        manifest = tomllib.loads(
            '[dependencies]\ntokio = { workspace = true }\ndb_pool = { path = "../db_pool" }\n'
        )
        self.assertEqual(check_manifest(manifest), [])

    def test_every_dependency_table_is_scanned(self):
        manifest = tomllib.loads(
            '[dependencies]\na = "1"\n'
            '[dev-dependencies]\nb = "1"\n'
            '[build-dependencies]\nc = "1"\n'
            "[target.'cfg(unix)'.dependencies]\n"
            'd = "1"\n'
        )
        joined = "\n".join(check_manifest(manifest))
        self.assertEqual(len(check_manifest(manifest)), 4)
        self.assertIn("dependencies: a", joined)
        self.assertIn("dev-dependencies: b", joined)
        self.assertIn("build-dependencies: c", joined)
        self.assertIn("target.'cfg(unix)'.dependencies: d", joined)


ROOT_MANIFEST = '[workspace]\nmembers = ["crates/a"]\n[workspace.dependencies]\nserde = "1"\n'
CLEAN_MEMBER = '[package]\nname = "a"\n[dependencies]\nserde = { workspace = true }\n'
DIRTY_MEMBER = '[package]\nname = "a"\n[dependencies]\nserde = "1"\n'


class MainTest(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)
        self.stderr = io.StringIO()

    def write(self, rel, text):
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        return str(path)

    def run_main(self, args):
        with (
            contextlib.redirect_stderr(self.stderr),
            contextlib.redirect_stdout(io.StringIO()),
        ):
            return main(args)

    def test_clean_workspace_passes(self):
        args = [
            self.write("Cargo.toml", ROOT_MANIFEST),
            self.write("crates/a/Cargo.toml", CLEAN_MEMBER),
            self.write("rustfmt.toml", "edition = '2024'\n"),
        ]
        self.assertEqual(self.run_main(args), 0)

    def test_member_violation_fails_and_is_named(self):
        args = [
            self.write("Cargo.toml", ROOT_MANIFEST),
            self.write("crates/a/Cargo.toml", DIRTY_MEMBER),
        ]
        self.assertEqual(self.run_main(args), 1)
        self.assertIn("crates/a/Cargo.toml", self.stderr.getvalue())
        self.assertIn("serde", self.stderr.getvalue())

    def test_missing_member_manifest_is_a_wiring_error(self):
        self.write("crates/a/Cargo.toml", CLEAN_MEMBER)
        args = [self.write("Cargo.toml", ROOT_MANIFEST)]
        self.assertEqual(self.run_main(args), 1)
        self.assertIn("crates/a", self.stderr.getvalue())
        self.assertIn("wiring", self.stderr.getvalue())

    def test_missing_root_manifest_is_a_wiring_error(self):
        args = [self.write("crates/a/Cargo.toml", CLEAN_MEMBER)]
        self.assertEqual(self.run_main(args), 1)
        self.assertIn("wiring", self.stderr.getvalue())


if __name__ == "__main__":
    unittest.main()
