"""Tests for scripts/port/testrun.py, the parallel test-binary runner behind `cargo.sh test` (TP3).

Most tests use a fake `cargo` (FAKE_CARGO below) that speaks cargo's side of the protocol: for each
binary of a JSON spec it prints cargo's `Running ...` line and starts the configured target runner
(testrun.py record) with the binary, then prints a doctest section. The fake binaries live under
<tmp>/target/debug/deps/<name>-<16 hex>, print libtest-like output, sleep, log their start/end times
and exit with the spec's code. Every test uses its own lock dir, spool dir and history file.

RealCargoTest runs real cargo on a tiny dependency-free crate and compares the runner's output, exit
status and the binaries' environment, argv and cwd with plain `cargo test` (skipped without cargo).

Behaviour covered:
  * output in cargo's order whatever the finish order, identical for --jobs 1 and 3 (determinism);
  * bounded concurrency (--jobs), extra run slots (none while another run waits for one);
  * --no-fail-fast: every binary runs, cargo's `error: N targets failed:` list (doctests included);
  * fail-fast: binaries after the first failing one (cargo order) are cancelled/killed, none printed;
  * per-binary `cargo.sh: bin-time` lines, the run summary, the wall-time history (longest first);
  * `record` execs non-cargo binaries (rustdoc's doctest executables) unchanged;
  * SIGTERM stops the running binaries.
"""

import fcntl
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest

import _util

TESTRUN = os.path.join(_util.PORT, "testrun.py")

FAKE_CARGO = r'''#!/usr/bin/env python3
import json, os, subprocess, sys
args = sys.argv[1:]
cfg = args[args.index("--config") + 1]
runner = json.loads(cfg.split("runner=", 1)[1])
post = args[args.index("--") + 1:] if "--" in args else []
spec = json.load(open(os.environ["FAKE_SPEC"]))
deps = os.environ["FAKE_DEPS"]
sys.stderr.write("    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.00s\n"); sys.stderr.flush()
for b in spec["bins"]:
    path = os.path.join(deps, b["file"])
    sys.stderr.write("     Running %s (target/debug/deps/%s)\n" % (b["header"], b["file"])); sys.stderr.flush()
    env = dict(os.environ, CARGO_PKG_NAME="fakepkg", FAKE_FROM_CARGO="1")
    rc = subprocess.call(runner + [path] + post, env=env, cwd=os.environ["FAKE_PKG_DIR"])
    if rc != 0:
        sys.exit(rc)
if spec.get("doc", True):
    sys.stderr.write("   Doc-tests fakepkg\n"); sys.stderr.flush()
    sys.stdout.write("\nrunning 1 test\ntest src/lib.rs - f (line 1) ... %s\n\n" % ("FAILED" if spec.get("doc_rc") else "ok"))
    sys.stdout.flush()
    if spec.get("doc_rc"):
        sys.stderr.write("error: doctest failed, to rerun pass `-p fakepkg --doc`\n")
        if "--no-fail-fast" in args:
            sys.stderr.write("error: 1 target failed:\n    `-p fakepkg --doc`\n")
        sys.exit(101)
sys.exit(0)
'''

FAKE_BIN = r'''#!/usr/bin/env python3
import json, os, sys, time
me = os.path.basename(sys.argv[0])
spec = json.load(open(os.environ["FAKE_SPEC"]))
b = [x for x in spec["bins"] if x["file"] == me][0]
log = os.environ["FAKE_EVENTS"]
def ev(what):
    with open(log, "a") as fh:
        fh.write("%s %s %.3f %d\n" % (what, b["name"], time.time(), os.getpid()))
ev("start")
print("\nrunning 1 test")
print("args=%s cwd=%s from_cargo=%s" % (" ".join(sys.argv[1:]), os.getcwd(), os.environ.get("FAKE_FROM_CARGO")))
# Like libtest: the test line is written in two parts, around the test's run time.
sys.stdout.write("test %s ... " % b["name"])
sys.stdout.flush()
time.sleep(b.get("sleep", 0))
rc = b.get("rc", 0)
print("FAILED" if rc else "ok")
print("\ntest result: %s. %d passed; %d failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n"
      % ("FAILED" if rc else "ok", 0 if rc else 1, 1 if rc else 0))
ev("end")
sys.exit(rc)
'''


def write_exe(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as fh:
        fh.write(text)
    os.chmod(path, 0o755)


BIN_TIME = re.compile(r"(?m)^cargo\.sh: bin-time .*\n")
SUMMARY = re.compile(r"(?m)^cargo\.sh: test binaries: .*\n")


def normalise(out):
    return SUMMARY.sub("", BIN_TIME.sub("", out))


class FakeCargoTest(unittest.TestCase):
    def setUp(self):
        self.tmp = os.path.realpath(tempfile.mkdtemp(prefix="p4-testrun-"))
        self.bin = os.path.join(self.tmp, "bin")
        write_exe(os.path.join(self.bin, "cargo"), FAKE_CARGO)
        self.deps = os.path.join(self.tmp, "target", "debug", "deps")
        os.makedirs(self.deps)
        self.pkg = os.path.join(self.tmp, "pkg")
        os.makedirs(self.pkg)
        self.locks = os.path.join(self.tmp, "locks")
        os.makedirs(self.locks)
        self.events = os.path.join(self.tmp, "events.txt")
        self.history = os.path.join(self.tmp, "hist", "times.json")
        self.held = []

    def tearDown(self):
        for fh in self.held:
            fh.close()
        shutil.rmtree(self.tmp)

    def spec(self, bins, **kw):
        """bins: [(name, kind, sleep, rc)]; kind 'lib', 'bin' or 'test'."""
        out = []
        for i, (name, kind, sleep, rc) in enumerate(bins):
            f = "%s-%016x" % (name.replace("-", "_"), 0xabc0 + i)
            header = {"lib": "unittests src/lib.rs", "bin": "unittests src/bin/%s.rs" % name}.get(kind, "tests/%s.rs" % name)
            write_exe(os.path.join(self.deps, f), FAKE_BIN)
            out.append({"name": name, "file": f, "header": header, "sleep": sleep, "rc": rc})
        spec = dict(kw, bins=out)
        p = os.path.join(self.tmp, "spec.json")
        with open(p, "w") as fh:
            json.dump(spec, fh)
        return p

    def run_tr(self, *cargo_args, jobs=3, run_slots=3, timeout=60, popen=False, merged=False):
        env = dict(os.environ, PATH=self.bin + os.pathsep + os.environ["PATH"], FAKE_SPEC=os.path.join(self.tmp, "spec.json"),
                   FAKE_DEPS=self.deps, FAKE_PKG_DIR=self.pkg, FAKE_EVENTS=self.events)
        env.pop("POB_QUICK_CORPUS", None)
        cmd = [sys.executable, TESTRUN, "run", "--jobs", str(jobs), "--run-slots", str(run_slots), "--lock-dir", self.locks,
               "--scope", "0", "--spool", os.path.join(self.tmp, "spool"), "--history", self.history, "--"] + list(cargo_args)
        if popen:
            return subprocess.Popen(cmd, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        if merged:     # one stream, as in a `> log 2>&1` (full_suite.sh)
            return subprocess.run(cmd, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
                                  timeout=timeout)
        return subprocess.run(cmd, env=env, capture_output=True, text=True, timeout=timeout)

    def test_progress_lines_never_split_a_line(self):
        # A (head, streamed live) is mid-line (`test A ... `) when B and C finish and log progress on
        # stderr: in the merged stream the test lines stay whole.
        self.spec([("A", "test", 1.0, 1), ("B", "test", 0.1, 0), ("C", "test", 0.2, 0)])
        r = self.run_tr("test", "-p", "fakepkg", "--no-fail-fast", merged=True)
        self.assertEqual(r.returncode, 101)
        self.assertEqual(re.findall(r"(?m)^test (\S+) \.\.\. (\S+)$", r.stdout), [("A", "FAILED"), ("B", "ok"), ("C", "ok")])
        self.assertEqual(len(re.findall(r"(?m)^cargo\.sh: \[\d/3\] ", r.stdout)), 3, r.stdout)

    def read_events(self):
        evs = []
        try:
            with open(self.events) as fh:
                for line in fh:
                    what, name, t, pid = line.split()
                    evs.append((what, name, float(t), int(pid)))
        except FileNotFoundError:
            pass
        os.path.exists(self.events) and os.remove(self.events)
        return evs

    @staticmethod
    def max_overlap(evs):
        cur = best = 0
        for what, *_ in sorted(evs, key=lambda e: (e[2], e[0] == "start")):
            cur += 1 if what == "start" else -1
            best = max(best, cur)
        return best

    def hold(self, name):
        fh = open(os.path.join(self.locks, name), "a")
        fcntl.flock(fh, fcntl.LOCK_EX | fcntl.LOCK_NB)
        self.held.append(fh)

    def order(self, out):
        return re.findall(r"(?m)^test (\S+).* \.\.\. ", out)

    # --- ordering, determinism, concurrency ------------------------------------------------------

    def test_output_in_cargo_order_and_parallel(self):
        self.spec([("lib", "lib", 0.8, 0), ("tool", "bin", 0.1, 0), ("A", "test", 0.6, 0), ("b-c", "test", 0.1, 0)])
        t0 = time.time()
        r = self.run_tr("test", "-p", "fakepkg", jobs=3)
        wall = time.time() - t0
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.order(r.stdout), ["lib", "tool", "A", "b-c", "src/lib.rs"])
        self.assertEqual(self.max_overlap(self.read_events()), 3)
        self.assertLess(wall, 1.6 + 1.0, "1.6 s of binaries with 3 jobs")  # 1 s collection window at most
        # Each block: cargo's Running line, the binary's output, then its bin-time line.
        self.assertIn("     Running tests/A.rs (target/debug/deps/A-000000000000abc2)\n\nrunning 1 test\n", r.stdout)
        self.assertRegex(r.stdout, r"(?m)^cargo\.sh: bin-time -p fakepkg --lib \d+\.\ds rss=\d+MB rc=0$")
        self.assertRegex(r.stdout, r"(?m)^cargo\.sh: bin-time -p fakepkg --bin tool ")
        self.assertRegex(r.stdout, r"(?m)^cargo\.sh: bin-time -p fakepkg --test b-c ")
        self.assertRegex(r.stdout, r"(?m)^cargo\.sh: test binaries: 4 run, 0 failed, 0 cancelled; .* up to 3 at once")
        # cargo's environment and cwd reach the binaries.
        self.assertIn("cwd=%s from_cargo=1" % self.pkg, r.stdout)
        # Finished line first, doctests last.
        self.assertTrue(r.stdout.startswith("    Finished `test` profile"), r.stdout[:200])
        self.assertLess(r.stdout.index("b-c ... ok"), r.stdout.index("Doc-tests fakepkg"))

    def test_jobs_1_is_serial_and_prints_the_same(self):
        self.spec([("lib", "lib", 0.3, 0), ("A", "test", 0.1, 1), ("B", "test", 0.2, 0)])
        r1 = self.run_tr("test", "-p", "fakepkg", "--no-fail-fast", jobs=1)
        self.assertEqual(self.max_overlap(self.read_events()), 1)
        r3 = self.run_tr("test", "-p", "fakepkg", "--no-fail-fast", jobs=3)
        self.assertGreater(self.max_overlap(self.read_events()), 1)
        self.assertEqual((r1.returncode, r3.returncode), (101, 101))
        self.assertEqual(normalise(r1.stdout), normalise(r3.stdout))

    def test_args_after_double_dash_reach_every_binary(self):
        self.spec([("A", "test", 0, 0), ("B", "test", 0, 0)])
        r = self.run_tr("test", "-p", "fakepkg", "--", "--exact", "x")
        self.assertEqual(r.stdout.count("args=--exact x cwd="), 2, r.stdout)

    # --- failures --------------------------------------------------------------------------------

    def test_no_fail_fast_runs_all_and_lists_failures_in_cargo_order(self):
        # B fails first in time, A later: the list is still in cargo's order (A, B), doctests last.
        self.spec([("A", "test", 0.6, 1), ("B", "test", 0.0, 1), ("C", "test", 0.2, 0)], doc_rc=101)
        r = self.run_tr("test", "-p", "fakepkg", "--no-fail-fast")
        self.assertEqual(r.returncode, 101)
        self.assertEqual(self.order(r.stdout), ["A", "B", "C", "src/lib.rs"])
        self.assertIn("error: test failed, to rerun pass `-p fakepkg --test A`\n", r.stdout)
        self.assertIn("error: test failed, to rerun pass `-p fakepkg --test B`\n", r.stdout)
        self.assertTrue(r.stdout.split("cargo.sh: test binaries:")[0].endswith(
            "error: 3 targets failed:\n    `-p fakepkg --test A`\n    `-p fakepkg --test B`\n    `-p fakepkg --doc`\n"),
            r.stdout)
        self.assertEqual(r.stdout.count("targets failed"), 1)        # cargo's own list is replaced

    def test_doctest_failure_alone_keeps_cargo_status(self):
        self.spec([("A", "test", 0, 0)], doc_rc=101)
        r = self.run_tr("test", "-p", "fakepkg", "--no-fail-fast")
        self.assertEqual(r.returncode, 101)
        self.assertIn("error: 1 target failed:\n    `-p fakepkg --doc`\n", r.stdout)

    def test_fail_fast_cancels_later_binaries(self):
        # Cargo order A, B, C, D. B fails at once; D (after B) is a long sleeper and is killed; A
        # (before B) still runs to the end; C is never printed. Nothing after B's block.
        self.spec([("A", "test", 0.5, 0), ("B", "test", 0.0, 1), ("C", "test", 0.2, 0), ("D", "test", 30, 0)])
        t0 = time.time()
        r = self.run_tr("test", "-p", "fakepkg", jobs=4)
        self.assertLess(time.time() - t0, 10)
        self.assertEqual(r.returncode, 101)
        self.assertEqual(self.order(r.stdout), ["A", "B"])
        body = r.stdout.split("cargo.sh: test binaries:")[0]
        self.assertTrue(body.endswith("error: test failed, to rerun pass `-p fakepkg --test B`\n"
                                      + re.search(r"cargo\.sh: bin-time -p fakepkg --test B .*\n", body).group(0)), body)
        self.assertNotIn("targets failed", r.stdout)
        self.assertNotIn("Doc-tests", r.stdout)
        self.assertRegex(r.stdout, r"test binaries: 2 run, 1 failed, 2 cancelled")
        ended = {e[1] for e in self.read_events() if e[0] == "end"}
        self.assertIn("A", ended)
        self.assertNotIn("D", ended)

    def test_killed_binary_is_reported_like_cargo(self):
        self.spec([("A", "test", 0, -9)])
        # A negative rc cannot be an exit status: the fake binary kills itself instead.
        p = os.path.join(self.deps, json.load(open(os.path.join(self.tmp, "spec.json")))["bins"][0]["file"])
        write_exe(p, "#!/bin/sh\nkill -9 $$\n")
        r = self.run_tr("test", "-p", "fakepkg", "--no-fail-fast")
        self.assertEqual(r.returncode, 101)
        self.assertIn("error: test failed, to rerun pass `-p fakepkg --test A`\n\nCaused by:\n"
                      "  process didn't exit successfully: `%s` (signal: 9, SIGKILL: kill)\n" % p, r.stdout)
        self.assertRegex(r.stdout, r"bin-time -p fakepkg --test A .* rc=-9")

    # --- run slots -------------------------------------------------------------------------------

    def test_extra_jobs_need_free_run_slots(self):
        bins = [("A", "test", 0.4, 0), ("B", "test", 0.4, 0), ("C", "test", 0.4, 0)]
        self.spec(bins)
        # cargo.sh holds run.lock (the base slot); run.lock.1 is busy elsewhere: one extra slot.
        self.hold("run.lock")
        self.hold("run.lock.1")
        r = self.run_tr("test", "-p", "fakepkg", jobs=3)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.max_overlap(self.read_events()), 2)
        # The extra slot is free again afterwards.
        fh = open(os.path.join(self.locks, "run.lock.2"), "a")
        fcntl.flock(fh, fcntl.LOCK_EX | fcntl.LOCK_NB)
        fh.close()

    def test_no_extra_slots_while_another_run_waits(self):
        self.spec([("A", "test", 0.3, 0), ("B", "test", 0.3, 0)])
        self.hold("run.lock")
        open(os.path.join(self.locks, "run.wait.%d" % os.getpid()), "w").close()   # a live waiter
        open(os.path.join(self.locks, "run.wait.999999999"), "w").close()         # a stale one
        r = self.run_tr("test", "-p", "fakepkg", jobs=3)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.max_overlap(self.read_events()), 1)
        self.assertFalse(os.path.exists(os.path.join(self.locks, "run.wait.999999999")))

    # --- history ---------------------------------------------------------------------------------

    def test_history_starts_the_longest_first(self):
        # Clearly different times (C > A > B), so the order taken from the history is fixed: with A
        # and B equally fast, whichever the first run happened to time as longer would go first.
        self.spec([("A", "test", 0.3, 0), ("B", "test", 0.0, 0), ("C", "test", 0.7, 0)])
        self.run_tr("test", "-p", "fakepkg", jobs=1)
        with open(self.history) as fh:
            h = json.load(fh)
        self.assertEqual(sorted(h), ["full -p fakepkg --test A", "full -p fakepkg --test B", "full -p fakepkg --test C"])
        self.assertGreater(h["full -p fakepkg --test C"], 0.5)
        self.assertGreater(h["full -p fakepkg --test C"], h["full -p fakepkg --test A"])
        self.assertGreater(h["full -p fakepkg --test A"], h["full -p fakepkg --test B"])
        self.read_events()
        r = self.run_tr("test", "-p", "fakepkg", jobs=2)
        # Two jobs: C (longest) and A start together, B only when A is done (A, 0.3 s, ends before
        # C, 0.7 s). In cargo's order (no history) A and B would start together and C last.
        starts = [e[1] for e in sorted(self.read_events(), key=lambda e: e[2]) if e[0] == "start"]
        self.assertEqual(sorted(starts[:2]), ["A", "C"])
        self.assertEqual(starts[2], "B")
        self.assertEqual(self.order(r.stdout)[:3], ["A", "B", "C"])     # printed in cargo order still
        # Filtered runs do not touch the history.
        before = json.load(open(self.history))
        self.spec([("A", "test", 0.3, 0), ("B", "test", 0.0, 0), ("C", "test", 0.0, 0)])
        self.run_tr("test", "-p", "fakepkg", "--", "only_this")
        self.assertEqual(json.load(open(self.history)), before)

    # --- record mode, signals --------------------------------------------------------------------

    def test_record_execs_non_cargo_binaries(self):
        # rustdoc runs doctest executables through the target runner: they run unchanged.
        exe = os.path.join(self.tmp, "rustdoctest", "rust_out")
        write_exe(exe, "#!/bin/sh\necho \"doctest ran with $*\"\nexit 7\n")
        r = subprocess.run([sys.executable, TESTRUN, "record", "abcd", exe, "--x"], capture_output=True, text=True)
        self.assertEqual((r.returncode, r.stdout), (7, "doctest ran with --x\n"))
        # A cargo test binary is recorded, not run.
        dep = os.path.join(self.deps, "T-0123456789abcdef")
        write_exe(dep, "#!/bin/sh\necho ran\n")
        r = subprocess.run([sys.executable, TESTRUN, "record", "abcd", dep, "--y"], capture_output=True,
                           env=dict(os.environ, MARKER_ENV="1"), cwd=self.pkg)
        self.assertEqual(r.returncode, 0)
        self.assertTrue(r.stdout.startswith(b"\x1ePOB-TESTRUN-abcd "), r.stdout[:80])
        rec = json.loads(r.stdout.split(b" ", 1)[1])
        self.assertEqual((rec["argv"], rec["cwd"], rec["env"]["MARKER_ENV"]), ([dep, "--y"], self.pkg, "1"))

    def test_sigterm_stops_running_binaries(self):
        self.spec([("A", "test", 30, 0), ("B", "test", 30, 0)])
        p = self.run_tr("test", "-p", "fakepkg", popen=True)
        deadline = time.time() + 20
        while time.time() < deadline and len([e for e in self.read_events_peek() if e[0] == "start"]) < 2:
            time.sleep(0.1)
        p.send_signal(signal.SIGTERM)
        out, err = p.communicate(timeout=20)
        self.assertEqual(p.returncode, 128 + signal.SIGTERM)
        for _, _, _, pid in self.read_events():
            with self.assertRaises(ProcessLookupError):
                os.kill(pid, 0)

    def read_events_peek(self):
        try:
            with open(self.events) as fh:
                return [tuple(line.split()) for line in fh]
        except FileNotFoundError:
            return []


CRATE_FILES = {
    "Cargo.toml": '[package]\nname = "tr-crate"\nversion = "0.1.0"\nedition = "2021"\n\n[workspace]\n',
    "src/lib.rs": '/// ```\n/// assert_eq!(tr_crate::one(), 1);\n/// ```\npub fn one() -> i32 { 1 }\n'
                  'pub fn dump(tag: &str) {\n'
                  '    let mut v: Vec<String> = std::env::vars().filter(|(k, _)| k != "_").map(|(k, v)| format!("{k}={v}")).collect();\n'
                  '    v.sort();\n'
                  '    let a: Vec<String> = std::env::args().skip(1).collect();\n'
                  '    let s = format!("cwd={}\\nargs={:?}\\n{}\\n", std::env::current_dir().unwrap().display(), a, v.join("\\n"));\n'
                  '    std::fs::write(format!("{}/{tag}-{}.txt", std::env::var("DUMP_DIR").unwrap(), std::env::var("DUMP_TAG").unwrap()), s).unwrap();\n'
                  '}\n'
                  '#[test] fn lib_t() { dump("lib"); }\n'
                  '#[test] fn lib_fail() { if std::env::var("FAILME").is_ok() { panic!("boom") } }\n',
    "src/bin/tool.rs": 'fn main() {}\n#[test] fn bin_t() { tr_crate::dump("bin"); }\n',
    "tests/it-a.rs": '#[test] fn it_t() { tr_crate::dump("it"); }\n'
                     '#[test] fn it_slow() { std::thread::sleep(std::time::Duration::from_millis(300)); }\n',
    "tests/Zed.rs": '#[test] fn zed() { if std::env::var("FAILME").is_ok() { panic!("zboom") } }\n',
}


@unittest.skipIf(shutil.which("cargo") is None, "cargo not found")
class RealCargoTest(unittest.TestCase):
    """Plain `cargo test` vs testrun.py on a tiny crate (no dependencies; compiles in about a second)."""

    @classmethod
    def setUpClass(cls):
        cls.tmp = os.path.realpath(tempfile.mkdtemp(prefix="p4-testrun-crate-"))
        cls.crate = os.path.join(cls.tmp, "crate")
        for rel, text in CRATE_FILES.items():
            p = os.path.join(cls.crate, rel)
            os.makedirs(os.path.dirname(p), exist_ok=True)
            with open(p, "w") as fh:
                fh.write(text)
        cls.env = dict(os.environ, CARGO_BUILD_JOBS="1", DUMP_DIR=os.path.join(cls.tmp, "dumps"))
        for k in ("CARGO_TARGET_DIR", "RUSTC_WRAPPER", "FAILME", "POB_QUICK_CORPUS"):
            cls.env.pop(k, None)
        os.makedirs(cls.env["DUMP_DIR"])
        os.makedirs(os.path.join(cls.tmp, "locks"))
        subprocess.run(["cargo", "test", "--no-run", "-q"], cwd=cls.crate, env=cls.env, check=True, capture_output=True,
                       timeout=300)

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.tmp)

    def both(self, args, **extra):
        env = dict(self.env, **extra)
        a = subprocess.run(["cargo"] + args, cwd=self.crate, env=dict(env, DUMP_TAG="cargo"), stdout=subprocess.PIPE,
                           stderr=subprocess.STDOUT, text=True, timeout=300)
        b = subprocess.run([sys.executable, TESTRUN, "run", "--jobs", "3", "--lock-dir", os.path.join(self.tmp, "locks"),
                            "--scope", "0", "--spool", self.tmp, "--history", os.path.join(self.tmp, "h.json"), "--"]
                           + args, cwd=self.crate, env=dict(env, DUMP_TAG="tr"), capture_output=True, text=True,
                           timeout=300)
        return a, b

    @staticmethod
    def norm(s):
        s = re.sub(r"finished in [0-9.]+s", "finished in Xs", s)
        s = re.sub(r"target\(s\) in [0-9.]+s", "target(s) in Xs", s)
        s = re.sub(r"thread '([^']*)' \(\d+\)", r"thread '\1'", s)
        s = re.sub(r"to rerun pass `-p tr-crate ", "to rerun pass `", s)
        s = re.sub(r"(?m)^    `-p tr-crate ", "    `", s)
        return normalise(s)

    def test_same_as_cargo(self):
        # --test-threads=1: libtest's own order of the tests within one binary is not deterministic.
        for args, extra in ((["test"], {}), (["test", "--no-fail-fast"], {"FAILME": "1"}), (["test"], {"FAILME": "1"}),
                            (["test", "it_"], {}), (["test", "-q"], {})):
            args = args + ["--", "--test-threads=1"]
            a, b = self.both(args, **extra)
            self.assertEqual(a.returncode, b.returncode, (args, a.stdout, b.stdout, b.stderr))
            self.assertEqual(self.norm(a.stdout), self.norm(b.stdout), args)

    def test_same_environment_argv_and_cwd(self):
        a, b = self.both(["test", "--", "--test-threads", "2"])
        self.assertEqual((a.returncode, b.returncode), (0, 0), b.stdout)
        d = self.env["DUMP_DIR"]
        for tag in ("lib", "bin", "it"):
            with open(os.path.join(d, "%s-cargo.txt" % tag)) as fa, open(os.path.join(d, "%s-tr.txt" % tag)) as fb:
                ta, tb = fa.read().replace("DUMP_TAG=cargo", "DUMP_TAG=tr"), fb.read()
            self.assertEqual(ta, tb, tag)
            self.assertIn("args=[\"--test-threads\", \"2\"]", tb)


if __name__ == "__main__":
    unittest.main()
