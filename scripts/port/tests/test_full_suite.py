"""Tests for scripts/port/full_suite.sh (TP3): crates in concurrent lanes, heavy crates first, the
summary.txt format (crate lines in list order, per-binary times slowest first).

A fake main checkout (a git repo holding a copy of full_suite.sh and a `rust-port` branch) gets a
fake scripts/port/cargo.sh that prints libtest-like output and `cargo.sh: bin-time` lines for the
crate it is asked to test, after sleeping per crate; it logs start/end times so the tests can check
how many crates ran at once. full_suite.sh creates its worktree (POB_FULLSUITE_WT) from that branch.
"""

import os
import re
import shutil
import subprocess
import tempfile
import unittest

import _util

FULL_SUITE = os.path.join(_util.PORT, "full_suite.sh")

FAKE_CARGO_SH = r"""#!/usr/bin/env bash
# fake cargo.sh: test -p <crate> --no-fail-fast
c="$3"
echo "start $c $(date +%s.%N) POB_ORACLE_REQUIRE=$POB_ORACLE_REQUIRE POB_FULL_SUITE=$POB_FULL_SUITE" >> "$FAKE_EVENTS"
sleep "$(eval echo "\${FAKE_SLEEP_${c//-/_}:-0.2}")"
echo "     Running tests/a.rs (target/debug/deps/a-0123456789abcdef)"
echo
echo "running 2 tests"
echo "test a_ok ... ok"
if [ "$c" = crate-bad ]; then
    echo "test a_bad ... FAILED"
    echo
    echo "failures:"
    echo "    a_bad"
    echo
    echo "test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s"
    echo "cargo.sh: bin-time -p $c --test a 3.5s rss=120MB rc=101"
    rc=101
else
    echo "test a_two ... ok"
    echo
    echo "test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s"
    echo "cargo.sh: bin-time -p $c --test a ${#c}.0s rss=${#c}0MB rc=0"
    echo "cargo.sh: bin-time -p $c --lib 0.1s rss=20MB rc=0"
    rc=0
fi
echo "end $c $(date +%s.%N)" >> "$FAKE_EVENTS"
exit $rc
"""


class FullSuiteTest(unittest.TestCase):
    def setUp(self):
        self.tmp = os.path.realpath(tempfile.mkdtemp(prefix="p4-fullsuite-"))
        self.main = os.path.join(self.tmp, "main")
        os.makedirs(os.path.join(self.main, "scripts", "port"))
        shutil.copy2(FULL_SUITE, os.path.join(self.main, "scripts", "port", "full_suite.sh"))
        p = os.path.join(self.main, "scripts", "port", "cargo.sh")
        with open(p, "w") as fh:
            fh.write(FAKE_CARGO_SH)
        os.chmod(p, 0o755)
        for c in ("heavy-a", "heavy-b", "small1", "small2", "crate-bad"):
            os.makedirs(os.path.join(self.main, "crates", c))
            open(os.path.join(self.main, "crates", c, "keep"), "w").close()
        self.git("init", "-q")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "init")
        self.git("branch", "rust-port")
        os.makedirs(os.path.join(self.main, ".PathOfBuilding"))
        self.events = os.path.join(self.tmp, "events.txt")

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def git(self, *args):
        env = dict(os.environ, GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1")
        subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main",
                        "-c", "commit.gpgsign=false"] + list(args), cwd=self.main, env=env, check=True,
                       capture_output=True)

    def run_suite(self, **extra):
        env = dict(os.environ, POB_FULLSUITE_WT=os.path.join(self.tmp, "wt"), FAKE_EVENTS=self.events,
                   POB_FULLSUITE_CRATES="small1 heavy-a crate-bad missing heavy-b small2",
                   POB_FULLSUITE_HEAVY="heavy-a heavy-b", FAKE_SLEEP_heavy_a="1.5", FAKE_SLEEP_heavy_b="1.0")
        env.update(extra)
        r = subprocess.run([os.path.join(self.main, "scripts", "port", "full_suite.sh")], env=env, capture_output=True,
                           text=True, timeout=120)
        self.assertEqual(r.returncode, 0, r.stderr)
        latest = os.path.join(self.main, ".parity", "fullsuite", "latest")
        with open(os.path.join(latest, "summary.txt")) as fh:
            return fh.read(), latest

    def events_list(self):
        with open(self.events) as fh:
            rows = [line.split() for line in fh]
        os.remove(self.events)
        return rows

    @staticmethod
    def overlap(rows):
        cur = best = 0
        for kind, *_ in sorted(rows, key=lambda r: (float(r[2]), r[0] == "start")):
            cur += 1 if kind == "start" else -1
            best = max(best, cur)
        return best

    def test_summary_format_and_lanes(self):
        summary, latest = self.run_suite()
        lines = summary.splitlines()
        self.assertRegex(lines[0], r"^full suite at rust-port [0-9a-f]+, started \d{4}-\d\d-\d\d \d\d:\d\d:\d\d$")
        # One line per existing crate, in POB_FULLSUITE_CRATES order (missing crates skipped).
        crate_lines = lines[1:6]
        self.assertEqual([l.split(":")[0] for l in crate_lines], ["small1", "heavy-a", "crate-bad", "heavy-b", "small2"])
        self.assertRegex(crate_lines[0], r"^small1: rc=0 \d+s; 2 passed, 0 failed, 1 ignored$")
        self.assertRegex(crate_lines[2], r"^crate-bad: rc=101 \d+s; 1 passed, 1 failed, 0 ignored; FAILED: a_bad $")
        self.assertEqual(lines[6], "per-binary times (wall, peak RSS, exit status), slowest first:")
        self.assertEqual(lines[7:12], [
            "  7.0s 70MB rc=0 heavy-a --test a",
            "  7.0s 70MB rc=0 heavy-b --test a",
            "  6.0s 60MB rc=0 small1 --test a",
            "  6.0s 60MB rc=0 small2 --test a",
            "  3.5s 120MB rc=101 crate-bad --test a",
        ])
        self.assertEqual(len([l for l in lines if l.endswith("--lib")]), 4)
        self.assertRegex(lines[-1], r"^finished \d{4}-")
        self.assertFalse([n for n in os.listdir(latest) if n.startswith(".line.") or n.endswith(".new")])
        for c in ("small1", "heavy-a", "crate-bad", "heavy-b", "small2"):
            self.assertTrue(os.path.exists(os.path.join(latest, c + ".log")))
        rows = self.events_list()
        self.assertTrue(all("POB_ORACLE_REQUIRE=1" in " ".join(r) and "POB_FULL_SUITE=1" in " ".join(r)
                            for r in rows if r[0] == "start"))
        # Three lanes, started together: both heavy crates and the first small one.
        self.assertEqual(self.overlap(rows), 3)
        starts = [r[1] for r in sorted(rows, key=lambda r: float(r[2])) if r[0] == "start"]
        self.assertEqual(sorted(starts[:3]), ["heavy-a", "heavy-b", "small1"])

    def test_one_lane_is_serial(self):
        summary, _ = self.run_suite(POB_FULLSUITE_LANES="1", FAKE_SLEEP_heavy_a="0.1", FAKE_SLEEP_heavy_b="0.1")
        rows = self.events_list()
        self.assertEqual(self.overlap(rows), 1)
        # The queue: heavy crates first, then the rest in list order.
        self.assertEqual([r[1] for r in rows if r[0] == "start"], ["heavy-a", "heavy-b", "small1", "crate-bad", "small2"])
        self.assertEqual([l.split(":")[0] for l in summary.splitlines()[1:6]],
                         ["small1", "heavy-a", "crate-bad", "heavy-b", "small2"])


if __name__ == "__main__":
    unittest.main()
