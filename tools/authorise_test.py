"""Unit tests for tools/authorise.py.

Invoked as a buck2 cached_check: authorise_test.py authorise.py <out>; the stamp is written
only when every test passes.
"""

import datetime as dt
import os
import sys
import tempfile
import unittest

import authorise

T10 = dt.datetime(2026, 1, 1, 10, 30, tzinfo=dt.timezone.utc)
BASE = b"on: push\n"


class ControlledTest(unittest.TestCase):
    def test_exact_and_prefix_patterns(self):
        self.assertEqual(authorise.controlled_path("tools/check.sh"), "tools/check.sh")
        self.assertEqual(authorise.controlled_path(".buckconfig"), ".buckconfig")
        self.assertEqual(
            authorise.controlled_path(".github/workflows/build.yml"), ".github/workflows/build.yml"
        )
        self.assertIsNone(authorise.controlled_path("tools/foo.sh"))
        self.assertIsNone(authorise.controlled_path(".github/dependabot.yml"))

    def test_suffix_matches_in_any_checkout(self):
        self.assertEqual(
            authorise.controlled_path("/home/x/anything/tools/check.sh"), "tools/check.sh"
        )
        self.assertEqual(authorise.controlled_path("./tools/check.sh"), "tools/check.sh")
        self.assertEqual(
            authorise.controlled_path("/tmp/other/.github/workflows/ci.yml"),
            ".github/workflows/ci.yml",
        )

    def test_the_list_controls_itself(self):
        for path in ("tools/authorise.py", "tools/agent_action_guard.py", ".claude/settings.json"):
            self.assertEqual(authorise.controlled_path(path), path)


class HourTest(unittest.TestCase):
    def test_hour_is_utc(self):
        local = dt.datetime(2026, 1, 1, 12, 0, tzinfo=dt.timezone(dt.timedelta(hours=2)))
        self.assertEqual(authorise.hour_of(local), "2026-01-01T10")


class StoreTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.store = os.path.join(self.tmp.name, "store")
        self.addCleanup(self.tmp.cleanup)

    def verify(self, path, **kw):
        return authorise.verify(path, store=self.store, now=kw.pop("now", T10), base=BASE, **kw)

    def test_verify_without_key_file_is_false(self):
        self.assertFalse(self.verify("tools/check.sh"))

    def test_init_writes_the_key_file_once(self):
        key_file = authorise.init("pw", store=self.store)
        self.assertEqual(key_file, os.path.join(self.store, "authoriser"))
        self.assertEqual(os.stat(key_file).st_mode & 0o777, 0o600)
        with self.assertRaises(authorise.AuthoriseError):
            authorise.init("pw", store=self.store)

    def test_authorise_rejects_uncontrolled_path(self):
        authorise.init("pw", store=self.store)
        with self.assertRaises(authorise.AuthoriseError):
            authorise.authorise(["tools/foo.sh"], "pw", store=self.store, now=T10, base=BASE)

    def test_authorise_rejects_wrong_passphrase(self):
        authorise.init("pw", store=self.store)
        with self.assertRaises(authorise.AuthoriseError):
            authorise.authorise(["tools/check.sh"], "nope", store=self.store, now=T10, base=BASE)
        self.assertFalse(self.verify("tools/check.sh"))

    def test_token_covers_only_the_hour_it_was_minted_in(self):
        authorise.init("pw", store=self.store)
        minted = authorise.authorise(["tools/check.sh"], "pw", store=self.store, now=T10, base=BASE)
        self.assertEqual(minted, [("tools/check.sh", "2026-01-01T10")])
        self.assertTrue(self.verify("tools/check.sh"))
        self.assertTrue(self.verify("tools/check.sh", now=T10 + dt.timedelta(minutes=29)))
        self.assertFalse(self.verify("tools/check.sh", now=T10 + dt.timedelta(minutes=30)))
        self.assertFalse(self.verify("tools/check.sh", now=T10 - dt.timedelta(minutes=31)))

    def test_token_is_minted_for_the_tree_relative_path(self):
        authorise.init("pw", store=self.store)
        [(rel, _)] = authorise.authorise(
            ["./tools/check.sh"], "pw", store=self.store, now=T10, base=BASE
        )
        self.assertEqual(rel, "tools/check.sh")
        self.assertTrue(self.verify("tools/check.sh"))

    def test_one_call_mints_several_paths(self):
        authorise.init("pw", store=self.store)
        minted = authorise.authorise(
            ["tools/check.sh", ".shellcheckrc"], "pw", store=self.store, now=T10, base=BASE
        )
        self.assertEqual([rel for rel, _ in minted], ["tools/check.sh", ".shellcheckrc"])
        self.assertTrue(self.verify("tools/check.sh"))
        self.assertTrue(self.verify(".shellcheckrc"))

    def test_token_is_bound_to_path_and_base(self):
        authorise.init("pw", store=self.store)
        authorise.authorise(["tools/check.sh"], "pw", store=self.store, now=T10, base=BASE)
        self.assertFalse(self.verify(".shellcheckrc"))
        self.assertFalse(
            authorise.verify("tools/check.sh", store=self.store, now=T10, base=b"other")
        )


if __name__ == "__main__":
    out = sys.argv.pop()
    del sys.argv[1:]
    result = unittest.main(exit=False).result
    if result.wasSuccessful():
        with open(out, "w", encoding="utf-8"):
            pass
    sys.exit(0 if result.wasSuccessful() else 1)
