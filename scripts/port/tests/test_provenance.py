import contextlib
import hashlib
import io
import os
import shutil
import subprocess
import tempfile
import unittest

import _util
import provenance

FOO = _util.fixture("prov", "src", "Modules", "Foo.lua")
FOO_V2 = _util.fixture("prov", "src", "Modules", "Foo_v2.lua")


def run(argv):
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        rc = provenance.main(argv)
    return rc, buf.getvalue()


def h(lines_file, a, b):
    with open(lines_file, "rb") as fh:
        return provenance.range_hash_lines(provenance.split_lines(fh.read()), a, b)


class ProvenanceBase(unittest.TestCase):
    def setUp(self):
        provenance._FILE_CACHE.clear()
        provenance._CODE_LINES.clear()
        self.tmp = tempfile.mkdtemp(prefix="p4-prov-")
        self.pob = os.path.join(self.tmp, "pob")
        os.makedirs(os.path.join(self.pob, "src", "Modules"))
        shutil.copy(FOO, os.path.join(self.pob, "src", "Modules", "Foo.lua"))
        self.rs = os.path.join(self.tmp, "rs")
        os.makedirs(self.rs)

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def write_rs(self, name, text):
        p = os.path.join(self.rs, name)
        with open(p, "w") as fh:
            fh.write(text)
        return p

    def good_rs(self, pin="16de4b82d"):
        return self.write_rs("foo.rs", "\n".join([
            "//! @pob-file Modules/Foo.lua @%s" % pin,
            "/// @pob Modules/Foo.lua:4-6 h=%s" % h(FOO, 4, 6),
            "pub fn one() {}",
            "/// @pob Modules/Foo.lua:10-12 h=%s" % h(FOO, 10, 12),
            "pub fn two() {}",
            "/// @pob Modules/Foo.lua:14-16 h=%s" % h(FOO, 14, 16),
            "pub fn three() {}",
            '// @pob-skip Modules/Foo.lua:18-18 reason="module return"',
            "",
        ]))


class HashTest(unittest.TestCase):
    def test_rule(self):
        # calc doc §3.1 rule 5: sha1 over the lines, trailing whitespace stripped, "\n"-joined
        with open(FOO, "rb") as fh:
            lines = fh.read().split(b"\n")
        expect = hashlib.sha1(b"\n".join(l.rstrip() for l in lines[3:6])).hexdigest()[:8]
        self.assertEqual(h(FOO, 4, 6), expect)
        self.assertTrue(lines[3].endswith(b"   \t"))  # line 4 really has trailing blanks
        self.assertNotEqual(expect, hashlib.sha1(b"\n".join(lines[3:6])).hexdigest()[:8])

    def test_crlf_equals_lf(self):
        a = provenance.range_hash_lines([b"x = 1\r", b"y = 2  \r"], 1, 2)
        b = provenance.range_hash_lines([b"x = 1", b"y = 2"], 1, 2)
        self.assertEqual(a, b)

    @_util.needs_pob
    def test_known_pin_hash(self):
        rc, out = run(["marker", "Modules/CalcOffence.lua:348-361"])
        self.assertEqual(out.strip(), "/// @pob Modules/CalcOffence.lua:348-361 h=9365b42f")


class CheckTest(ProvenanceBase):
    def test_good(self):
        self.good_rs()
        rc, out = run(["--pob", self.pob, "check", self.rs])
        self.assertEqual(rc, 0, out)
        self.assertIn("ok: 3 @pob, 1 @pob-skip, 1 @pob-file markers, 0 error(s)", out)

    def test_bad(self):
        self.write_rs("bad.rs", "\n".join([
            "//! @pob-file Modules/Foo.lua @deadbeef1",
            "/// @pob Modules/Foo.lua:4-6 h=00000000",
            "/// @pob Modules/Foo.lua:10-12",
            "/// @pob Modules/Foo.lua:10-99 h=%s" % h(FOO, 10, 12),
            "/// @pob Modules/Nope.lua:1-2 h=00000000",
            "// @pob-skip Modules/Foo.lua:18-18",
            "let s = \"@pob Modules/Foo.lua:1-2 not a comment\";",
        ]))
        rc, out = run(["--pob", self.pob, "check", self.rs])
        self.assertEqual(rc, 1)
        self.assertIn("is not the pin", out)
        self.assertIn("h=00000000, Lua hashes to %s" % h(FOO, 4, 6), out)
        self.assertIn("has no hash (expected h=%s)" % h(FOO, 10, 12), out)
        self.assertIn("outside the file", out)
        self.assertIn("Nope.lua not found", out)
        self.assertIn("without reason", out)
        self.assertIn("6 error(s)", out)


class CoverageTest(ProvenanceBase):
    def scope(self, text):
        p = os.path.join(self.tmp, "scope.toml")
        with open(p, "w") as fh:
            fh.write(text)
        return p

    def test_uncovered_and_double(self):
        self.good_rs()
        self.write_rs("dup.rs", "/// @pob Modules/Foo.lua:11-11 h=%s\n" % h(FOO, 11, 11))
        sc = self.scope('[[file]]\npath = "Modules/Foo.lua"\nranges = ["1-18"]\n')
        rc, out = run(["--pob", self.pob, "coverage", "--scope", sc, self.rs])
        self.assertEqual(rc, 1)
        self.assertIn("uncovered: 2\n", out)          # `local M = {}`; comments/blank lines ignored
        self.assertIn("covered twice: 11", out)
        self.assertIn("11 in-scope lines, 1 uncovered, 1 covered twice", out)

    def test_strict_counts_blank_lines(self):
        self.good_rs()
        sc = self.scope('[[file]]\npath = "Modules/Foo.lua"\nranges = ["1-18"]\n')
        rc, out = run(["--pob", self.pob, "coverage", "--strict", "--scope", sc, self.rs])
        self.assertIn("uncovered: 1-3, 7-9, 13, 17", out)

    def test_full_coverage_with_range(self):
        self.good_rs()
        rc, out = run(["--pob", self.pob, "coverage", "--range", "Modules/Foo.lua:4-18", self.rs])
        self.assertEqual(rc, 0, out)
        self.assertIn("ok: scope --range; 10 in-scope lines, 0 uncovered", out)

    def test_missing_scope_file(self):
        rc, out = run(["--pob", self.pob, "coverage", "--scope", os.path.join(self.tmp, "nope.toml"), self.rs])
        self.assertNotEqual(rc, 0)


class BumpTest(ProvenanceBase):
    def git(self, *args):
        return subprocess.run(["git", "-C", self.pob] + list(args), check=True, capture_output=True,
                              text=True).stdout.strip()

    def setUp(self):
        super().setUp()
        self.git("init", "-q")
        self.git("config", "user.email", "t@example.com")
        self.git("config", "user.name", "t")
        self.git("add", "-A")
        self.git("-c", "commit.gpgsign=false", "commit", "-q", "--no-verify", "-m", "v1")
        self.v1 = self.git("rev-parse", "HEAD")
        shutil.copy(FOO_V2, os.path.join(self.pob, "src", "Modules", "Foo.lua"))
        self.git("-c", "commit.gpgsign=false", "commit", "-q", "--no-verify", "-am", "v2")
        self.v2 = self.git("rev-parse", "HEAD")

    def test_dry_run_reports(self):
        self.good_rs()
        rc, out = run(["--pob", self.pob, "bump", "--from", self.v1, "--to", self.v2, self.rs])
        self.assertEqual(rc, 1)  # one work item
        self.assertIn("3 clean (3 moved, 0 in place), 1 work items", out)
        self.assertIn("Modules/Foo.lua:4-6 -> 5-7", out)
        self.assertIn("Modules/Foo.lua:14-16 -> 15-17", out)
        self.assertIn("Modules/Foo.lua:18-18 -> 19-19", out)
        self.assertIn("`Modules/Foo.lua:10-12` (@pob)", out)
        self.assertIn("-\treturn b * 2", out)
        self.assertIn("+\treturn b * 20", out)
        with open(os.path.join(self.rs, "foo.rs")) as fh:
            self.assertIn(":4-6 ", fh.read())  # dry run leaves files alone

    def test_apply_moves_clean_markers(self):
        self.good_rs()
        rep = os.path.join(self.tmp, "work.md")
        rc, out = run(["--pob", self.pob, "bump", "--from", self.v1, "--to", self.v2, "--apply",
                       "--report", rep, self.rs])
        with open(os.path.join(self.rs, "foo.rs")) as fh:
            text = fh.read()
        self.assertIn("/// @pob Modules/Foo.lua:5-7 h=%s" % h(FOO, 4, 6), text)
        self.assertIn("/// @pob Modules/Foo.lua:10-12 h=", text)       # work item: untouched
        self.assertIn("/// @pob Modules/Foo.lua:15-17 h=", text)
        self.assertIn('// @pob-skip Modules/Foo.lua:19-19 reason="module return"', text)
        self.assertIn("@pob-file Modules/Foo.lua @16de4b82d", text)   # file not clean: pin kept
        self.assertTrue(os.path.exists(rep))
        # Moved markers verify against the new checkout.
        provenance._FILE_CACHE.clear()
        rc2, out2 = run(["--pob", self.pob, "check", os.path.join(self.rs, "foo.rs")])
        self.assertIn("h=", out2)
        # The work item's hash no longer matches, and its file is still pinned to the old commit.
        self.assertIn("2 error(s)", out2)
        self.assertIn("@pob-file pin @16de4b82d is not the pin %s" % self.v2[:9], out2)

    def test_check_after_bump_uses_checkout_head(self):
        # A clean file bumped to v2 verifies against the v2 checkout without editing pobpaths.PIN.
        self.write_rs("clean.rs", "\n".join([
            "//! @pob-file Modules/Foo.lua @16de4b82d",
            "/// @pob Modules/Foo.lua:14-16 h=%s" % h(FOO, 14, 16),
            "",
        ]))
        self.git("checkout", "-q", self.v1)
        rc, out = run(["--pob", self.pob, "bump", "--from", self.v1, "--to", self.v2, "--apply", self.rs])
        self.assertEqual(rc, 0, out)
        self.git("checkout", "-q", self.v2)
        provenance._FILE_CACHE.clear()
        rc, out = run(["--pob", self.pob, "check", self.rs])
        self.assertEqual(rc, 0, out)
        self.assertIn("note: checkout is at %s" % self.v2[:9], out)
        rc, out = run(["--pob", self.pob, "check", "--pin", self.v1, self.rs])
        self.assertEqual(rc, 1, out)

    def test_bump_from_defaults_to_checkout_head(self):
        self.good_rs()
        self.git("checkout", "-q", self.v1)
        rc, out = run(["--pob", self.pob, "bump", "--to", self.v2, self.rs])
        self.assertIn("# provenance bump %s -> %s" % (self.v1[:9], self.v2[:9]), out)
        self.assertIn("Modules/Foo.lua:4-6 -> 5-7", out)
        self.git("checkout", "-q", self.v2)
        rc, out = run(["--pob", self.pob, "bump", "--to", self.v2, self.rs])
        self.assertEqual(rc, 2)
        self.assertIn("--from <old pin>", out)

    def test_apply_bumps_pin_of_clean_file(self):
        self.write_rs("clean.rs", "\n".join([
            "//! @pob-file Modules/Foo.lua @16de4b82d",
            "/// @pob Modules/Foo.lua:14-16 h=%s" % h(FOO, 14, 16),
            "",
        ]))
        rc, out = run(["--pob", self.pob, "bump", "--from", self.v1, "--to", self.v2, "--apply", self.rs])
        self.assertEqual(rc, 0, out)
        with open(os.path.join(self.rs, "clean.rs")) as fh:
            text = fh.read()
        self.assertIn("@pob-file Modules/Foo.lua @%s" % self.v2[:9], text)
        self.assertIn("Modules/Foo.lua:15-17", text)


class MapLineTest(unittest.TestCase):
    def test_map(self):
        hunks = [provenance.Hunk(1, 0, 2, 1, []), provenance.Hunk(11, 1, 12, 1, []), provenance.Hunk(20, 2, 21, 0, [])]
        self.assertEqual(provenance.map_line(hunks, 1), 1)
        self.assertEqual(provenance.map_line(hunks, 4), 5)
        self.assertEqual(provenance.map_line(hunks, 11), 12)
        self.assertEqual(provenance.map_line(hunks, 25), 24)
        self.assertTrue(hunks[1].touches(10, 12))
        self.assertFalse(hunks[0].touches(2, 5))
        self.assertTrue(provenance.Hunk(3, 0, 4, 2, []).touches(2, 5))


if __name__ == "__main__":
    unittest.main()
