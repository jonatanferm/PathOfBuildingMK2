"""Tests for scripts/port/bump_version.sh (BRAND, user decision 2026-11-12): VERSION is YYYY.MM.DD.N,
N the per-day release counter (same day -> N+1, new day -> 1). Every test points POB_VERSION_FILE
at a temporary file and fixes POB_VERSION_TODAY, so the checked-in VERSION is never touched.
The checked-in VERSION itself must hold a well-formed version."""

import os
import re
import subprocess
import tempfile
import unittest

import _util

BUMP = os.path.join(_util.PORT, "bump_version.sh")
REPO_VERSION = os.path.join(os.path.dirname(os.path.dirname(_util.PORT)), "VERSION")
FORMAT = re.compile(r"^\d{4}\.\d{2}\.\d{2}\.[1-9]\d*$")


class BumpVersion(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.file = os.path.join(self.tmp.name, "VERSION")

    def tearDown(self):
        self.tmp.cleanup()

    def bump(self, today, check=True):
        env = dict(os.environ, POB_VERSION_FILE=self.file, POB_VERSION_TODAY=today)
        return subprocess.run(
            ["bash", BUMP], env=env, capture_output=True, text=True, check=check
        )

    def write(self, text):
        with open(self.file, "w") as f:
            f.write(text)

    def read(self):
        with open(self.file) as f:
            return f.read()

    def test_same_day_increments(self):
        self.write("2026.11.12.1\n")
        r = self.bump("2026.11.12")
        self.assertEqual(r.stdout, "2026.11.12.2\n")
        self.assertEqual(self.read(), "2026.11.12.2\n")
        self.assertEqual(self.bump("2026.11.12").stdout, "2026.11.12.3\n")

    def test_counter_past_nine(self):
        self.write("2026.11.12.9\n")
        self.assertEqual(self.bump("2026.11.12").stdout, "2026.11.12.10\n")
        self.assertEqual(self.bump("2026.11.12").stdout, "2026.11.12.11\n")

    def test_new_day_resets_to_one(self):
        self.write("2026.11.12.4\n")
        self.assertEqual(self.bump("2026.11.13").stdout, "2026.11.13.1\n")
        self.assertEqual(self.read(), "2026.11.13.1\n")

    def test_missing_or_empty_file_starts_at_one(self):
        self.assertEqual(self.bump("2026.11.12").stdout, "2026.11.12.1\n")
        self.write("")
        self.assertEqual(self.bump("2026.11.12").stdout, "2026.11.12.1\n")

    def test_no_trailing_newline(self):
        self.write("2026.11.12.1")
        self.assertEqual(self.bump("2026.11.12").stdout, "2026.11.12.2\n")

    def test_malformed_file_is_refused_untouched(self):
        for bad in ("2.67.2\n", "2026.11.12\n", "2026.11.12.0\n", "garbage\n"):
            self.write(bad)
            r = self.bump("2026.11.12", check=False)
            self.assertEqual(r.returncode, 2, bad)
            self.assertEqual(self.read(), bad)

    def test_bad_date_is_refused(self):
        self.write("2026.11.12.1\n")
        r = self.bump("2026-11-12", check=False)
        self.assertEqual(r.returncode, 2)
        self.assertEqual(self.read(), "2026.11.12.1\n")

    def test_default_date_is_today(self):
        self.write("2000.01.01.3\n")
        env = dict(os.environ, POB_VERSION_FILE=self.file)
        env.pop("POB_VERSION_TODAY", None)
        today = subprocess.run(["date", "+%Y.%m.%d"], capture_output=True, text=True).stdout.strip()
        r = subprocess.run(["bash", BUMP], env=env, capture_output=True, text=True, check=True)
        self.assertEqual(r.stdout, f"{today}.1\n")

    def test_checked_in_version_is_well_formed(self):
        with open(REPO_VERSION) as f:
            text = f.read()
        self.assertTrue(text.endswith("\n") and text.count("\n") == 1, repr(text))
        self.assertRegex(text.strip(), FORMAT)


if __name__ == "__main__":
    unittest.main()
