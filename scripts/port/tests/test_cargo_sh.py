"""Tests for scripts/port/cargo.sh (calc doc D20, §10.1; AMENDMENTS A5).

A fake `cargo` (and a fake `systemd-run`) on PATH record what cargo.sh would run. Every test uses a
temporary POB_PORT_LOCK_DIR and HOME, and POB_PORT_NO_SCOPE=1 unless it tests the scope itself, so
the shared machine-wide slots, ~/.cache/pob-port-targets and the user's systemd are never touched.

Behaviour covered (see cargo.sh's header and inline comments):
  * environment: CARGO_BUILD_JOBS, RUSTC_WRAPPER=sccache, CARGO_TARGET_DIR not exported;
  * cwd: kept inside the worktree, worktree root otherwise;
  * guards: test/bench need -p, release builds need -p, never --workspace (POB_PORT_ALLOW_WORKSPACE=1);
  * compile slots: POB_PORT_SLOTS (default 2) flocks cargo.lock, cargo.lock.1, ...;
  * test/bench run in two phases: `--no-run` under a compile slot, then the run under one of
    POB_PORT_RUN_SLOTS (default 3) run slots run.lock, run.lock.1, ...;
  * read-only subcommands take no slot;
  * worktrees under /tmp (or /dev/shm) get --target-dir ~/.cache/pob-port-targets/<name>, and target
    dirs whose worktree is gone are removed;
  * in the main checkout `test` uses the quick corpus (POB_PARITY_DIR=.parity-quick, POB_QUICK_CORPUS=1);
  * each cargo run is wrapped in a memory-capped `systemd-run --user --scope`.
"""

import fcntl
import os
import shutil
import stat
import subprocess
import tempfile
import time
import unittest

import _util

CARGO_SH = os.path.join(_util.PORT, "cargo.sh")

# One "---" block of key=value lines per invocation. `held` lists the lock files in the lock dir
# that some process holds while this invocation runs (flock(1) opens its own file description, so it
# conflicts with the flocks cargo.sh holds on fd 9 / fd 8).
FAKE_CARGO = r"""#!/bin/sh
held=""
for f in "$POB_PORT_LOCK_DIR"/*.lock*; do
    [ -e "$f" ] || continue
    flock -n "$f" true || held="$held $(basename "$f")"
done
{
echo "---"
echo "args=$*"
echo "jobs=$CARGO_BUILD_JOBS"
echo "wrapper=$RUSTC_WRAPPER"
echo "target_env=${CARGO_TARGET_DIR-<unset>}"
echo "pwd=$(pwd -P)"
echo "parity=${POB_PARITY_DIR-<unset>}"
echo "quick=${POB_QUICK_CORPUS-<unset>}"
echo "scope=${FAKE_IN_SCOPE:-0}"
echo "held=${held# }"
} | tee -a "${FAKE_LOG:-/dev/null}"
if [ -n "$FAKE_BARRIER" ]; then
    # Wait (up to FAKE_BARRIER_TRIES x 0.1 s) until $FAKE_BARRIER_N invocations are inside cargo at once.
    touch "$FAKE_BARRIER/$$"
    i=0
    while [ "$(ls "$FAKE_BARRIER" | wc -l)" -lt "$FAKE_BARRIER_N" ]; do
        i=$((i + 1))
        if [ "$i" -gt "${FAKE_BARRIER_TRIES:-100}" ]; then echo "barrier=timeout"; exit 0; fi
        sleep 0.1
    done
    echo "barrier=ok"
fi
if [ -n "$FAKE_SLEEP" ]; then sleep "$FAKE_SLEEP"; fi
case " $* " in
    *" --no-run "*) exit "${FAKE_RC_COMPILE:-0}" ;;
    *) exit "${FAKE_RC:-0}" ;;
esac
"""

# Logs its arguments; answers the availability probe; otherwise drops its own options and execs.
FAKE_SYSTEMD_RUN = r"""#!/bin/sh
echo "$*" >> "$FAKE_SCOPE_LOG"
if [ "$*" = "--user --scope --quiet true" ]; then exit "${FAKE_SCOPE_PROBE_RC:-0}"; fi
while [ "$#" -gt 0 ]; do
    case "$1" in
        --user|--scope|--quiet) shift ;;
        -p) shift 2 ;;
        *) break ;;
    esac
done
FAKE_IN_SCOPE=1 exec "$@"
"""

# Subcommands that get --target-dir in a /tmp worktree.
TARGET_DIR_SUBS = ("build", "b", "check", "c", "test", "t", "bench", "run", "r", "clippy", "doc", "d", "rustc", "fix")
READ_ONLY = ("metadata", "tree", "locate-project", "pkgid", "version", "help", "search", "read-manifest")


def write_exe(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as fh:
        fh.write(text)
    os.chmod(path, os.stat(path).st_mode | stat.S_IEXEC)


def read(path):
    try:
        with open(path) as fh:
            return fh.read()
    except FileNotFoundError:
        return ""


def on_tmpfs_path(path):
    return path.startswith("/tmp/") or path.startswith("/dev/shm/")


class CargoShTest(unittest.TestCase):
    def setUp(self):
        # Under /tmp on purpose: the fake worktrees made here must hit cargo.sh's /tmp rewrite.
        self.tmp = os.path.realpath(tempfile.mkdtemp(prefix="p4-cargo-", dir="/tmp"))
        self.bin = os.path.join(self.tmp, "bin")
        write_exe(os.path.join(self.bin, "cargo"), FAKE_CARGO)
        write_exe(os.path.join(self.bin, "systemd-run"), FAKE_SYSTEMD_RUN)
        self.home = os.path.join(self.tmp, "home")
        os.makedirs(self.home)
        self.lockdir = os.path.join(self.tmp, "locks")
        self.scope_log = os.path.join(self.tmp, "scope.log")
        self.env = dict(os.environ)
        for k in ("RUSTC_WRAPPER", "CARGO_TARGET_DIR", "CARGO_BUILD_JOBS", "POB_PORT_ALLOW_WORKSPACE",
                  "POB_PORT_SLOTS", "POB_PORT_RUN_SLOTS", "POB_PORT_JOBS", "POB_PORT_MEM", "POB_PORT_SWAP",
                  "POB_PARITY_DIR", "POB_QUICK_CORPUS", "POB_FULL_SUITE", "FAKE_SLEEP", "FAKE_RC",
                  "FAKE_RC_COMPILE", "FAKE_LOG", "FAKE_BARRIER", "FAKE_BARRIER_N", "FAKE_BARRIER_TRIES", "FAKE_IN_SCOPE"):
            self.env.pop(k, None)
        self.env.update(PATH=self.bin + os.pathsep + self.env["PATH"], HOME=self.home,
                        POB_PORT_LOCK_DIR=self.lockdir, POB_PORT_NO_SCOPE="1", FAKE_SCOPE_LOG=self.scope_log)
        self.held = []

    def tearDown(self):
        for fh in self.held:
            fh.close()
        shutil.rmtree(self.tmp)

    # --- helpers -------------------------------------------------------------------------------

    def run_sh(self, *args, env=None, cwd=None, script=CARGO_SH, **extra):
        e = dict(env or self.env, **extra)
        return subprocess.run([script] + list(args), env=e, capture_output=True, text=True, timeout=60, cwd=cwd)

    def popen(self, *args, script=CARGO_SH, **extra):
        return subprocess.Popen([script] + list(args), env=dict(self.env, **extra), stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, text=True)

    @staticmethod
    def root():
        """The worktree cargo.sh resolves: git's top level, else scripts/port/../.. (as cargo.sh does)."""
        top = subprocess.run(["git", "-C", _util.PORT, "rev-parse", "--show-toplevel"], capture_output=True,
                             text=True).stdout.strip()
        return os.path.realpath(top or os.path.join(_util.PORT, "..", ".."))

    @staticmethod
    def calls(stdout):
        """One dict per fake-cargo invocation, in order."""
        out = []
        for line in stdout.splitlines():
            if line == "---":
                out.append({})
            elif out and "=" in line:
                k, v = line.split("=", 1)
                out[-1][k] = v
        return out

    def one_call(self, r):
        self.assertEqual(r.returncode, 0, r.stderr)
        cs = self.calls(r.stdout)
        self.assertEqual(len(cs), 1, r.stdout)
        return cs[0]

    def expect_args(self, args, root=None):
        """What the fake cargo receives for `args` run through the worktree `root` (the real one by
        default): cargo.sh inserts --target-dir after the subcommand when the worktree is on /tmp."""
        root = root or self.root()
        args = list(args)
        if on_tmpfs_path(root) and args and args[0] in TARGET_DIR_SUBS:
            td = os.path.join(self.home, ".cache", "pob-port-targets", os.path.basename(root))
            args[1:1] = ["--target-dir", td]
        return " ".join(args)

    def hold(self, name):
        os.makedirs(self.lockdir, exist_ok=True)
        fh = open(os.path.join(self.lockdir, name), "a")
        fcntl.flock(fh, fcntl.LOCK_EX | fcntl.LOCK_NB)
        self.held.append(fh)
        return fh

    def release(self, fh):
        fcntl.flock(fh, fcntl.LOCK_UN)
        fh.close()
        self.held.remove(fh)

    def git(self, *args, cwd):
        env = dict(self.env, GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1")
        subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main",
                        "-c", "commit.gpgsign=false"] + list(args), cwd=cwd, env=env, check=True,
                       capture_output=True)

    def fake_checkout(self, name="mainrepo", linked=None):
        """A git repo `self.tmp/<name>` (a main checkout: .git is a directory) holding a copy of
        cargo.sh; with `linked`, also a linked worktree `self.tmp/<linked>` (.git is a file).
        Returns (main_root, linked_root or None)."""
        main = os.path.join(self.tmp, name)
        os.makedirs(os.path.join(main, "scripts", "port"))
        shutil.copy2(CARGO_SH, os.path.join(main, "scripts", "port", "cargo.sh"))
        self.git("init", "-q", cwd=main)
        lt = None
        if linked:
            self.git("add", "-A", cwd=main)
            self.git("commit", "-q", "-m", "init", cwd=main)
            lt = os.path.join(self.tmp, linked)
            self.git("worktree", "add", "-q", "--detach", lt, cwd=main)
        return main, lt

    @staticmethod
    def script_in(root):
        return os.path.join(root, "scripts", "port", "cargo.sh")

    # --- environment and cwd -------------------------------------------------------------------

    def test_env_and_passthrough(self):
        r = self.run_sh("check", "-p", "pob-calc", cwd=self.tmp)
        c = self.one_call(r)
        root = self.root()
        self.assertEqual(c["args"], self.expect_args(["check", "-p", "pob-calc"]))
        self.assertEqual(c["jobs"], "3")
        # CARGO_TARGET_DIR is not exported (sccache hashes CARGO_* env vars): cargo's default
        # <worktree>/target applies because cargo runs inside the worktree.
        self.assertEqual(c["target_env"], "<unset>")
        # Called from outside the worktree: cargo runs in the worktree root.
        self.assertEqual(c["pwd"], root)
        # No sccache in $HOME: no wrapper.
        self.assertEqual(c["wrapper"], "")
        self.assertEqual(c["scope"], "0")
        self.assertEqual(self.one_call(self.run_sh("check", "-p", "x", POB_PORT_JOBS="7"))["jobs"], "7")

    def test_sccache_wrapper(self):
        sccache = os.path.join(self.home, ".cargo", "bin", "sccache")
        write_exe(sccache, "#!/bin/sh\n")
        self.assertEqual(self.one_call(self.run_sh("check", "-p", "x"))["wrapper"], sccache)
        # A non-executable sccache is ignored.
        os.chmod(sccache, 0o644)
        self.assertEqual(self.one_call(self.run_sh("check", "-p", "x"))["wrapper"], "")

    def test_cwd_inside_worktree_is_kept(self):
        # Relative paths in arguments (e.g. --manifest-path) resolve against the caller's directory.
        sub = os.path.join(self.root(), "scripts", "port")
        c = self.one_call(self.run_sh("check", "--manifest-path", "x/Cargo.toml", cwd=sub))
        self.assertEqual(c["pwd"], os.path.realpath(sub))
        self.assertEqual(c["target_env"], "<unset>")
        self.assertEqual(c["args"], self.expect_args(["check", "--manifest-path", "x/Cargo.toml"]))

    def test_existing_target_dir_and_wrapper_kept(self):
        write_exe(os.path.join(self.home, ".cargo", "bin", "sccache"), "#!/bin/sh\n")
        c = self.one_call(self.run_sh("build", "-p", "a", CARGO_TARGET_DIR="/tmp/x-target", RUSTC_WRAPPER="mywrap"))
        self.assertEqual(c["target_env"], "/tmp/x-target")
        self.assertEqual(c["wrapper"], "mywrap")

    def test_exit_status_is_propagated(self):
        self.assertEqual(self.run_sh("check", "-p", "x", FAKE_RC="101").returncode, 101)

    # --- guards --------------------------------------------------------------------------------

    def assertRefused(self, *args, cwd=None, **extra):
        r = self.run_sh(*args, cwd=cwd, **extra)
        self.assertEqual(r.returncode, 2, (args, r.stdout, r.stderr))
        self.assertIn("refusing", r.stderr)
        self.assertNotIn("args=", r.stdout)

    def assertAllowed(self, *args, cwd=None, **extra):
        r = self.run_sh(*args, cwd=cwd, **extra)
        self.assertEqual(r.returncode, 0, (args, r.stderr))
        cs = self.calls(r.stdout)
        self.assertTrue(cs, (args, r.stdout))
        sub = next(a for a in args if not a.startswith(("+", "-")))
        # Hand the rewrite helper the args with the subcommand first, then reassemble.
        lead = list(args[:args.index(sub)])
        want = self.expect_args([sub] + list(args[args.index(sub) + 1:]))
        self.assertEqual(cs[-1]["args"], " ".join(lead + [want]), (args, r.stdout))
        return cs

    def test_test_without_package_is_refused(self):
        # The root manifest is virtual with no default-members: a bare `cargo test` is the whole workspace.
        self.assertRefused("test")
        self.assertRefused("t")
        self.assertRefused("test", "--lib")
        self.assertRefused("test", "--", "-p")             # after `--` it belongs to the test binary
        # ... also from inside a crate directory, and from outside the worktree.
        self.assertRefused("test", cwd=os.path.join(self.root(), "crates", "pob-calc"))
        self.assertRefused("test", cwd=self.tmp)
        self.assertRefused("+nightly", "test")
        self.assertRefused("--config", "x=1", "test")
        self.assertAllowed("test", "-p", "pob-calc")
        self.assertAllowed("t", "--package", "pob-calc", "--lib")
        self.assertAllowed("test", "--package=pob-calc")
        self.assertAllowed("test", "-ppob-calc")
        self.assertAllowed("+nightly", "test", "-p", "pob-calc")

    def test_test_with_package_and_workspace_is_refused(self):
        self.assertRefused("test", "-p", "pob-calc", "--workspace")
        self.assertRefused("t", "--all", "-p", "pob-calc")

    def test_bench_without_package_is_refused(self):
        # bench always builds the release-like bench profile.
        self.assertRefused("bench")
        self.assertRefused("bench", "--workspace")
        self.assertRefused("bench", "-p", "pob-calc", "--workspace")
        self.assertAllowed("bench", "-p", "pob-calc")

    def test_release_build_with_workspace_is_refused_even_with_package(self):
        self.assertRefused("build", "--release", "--workspace", "-p", "pob-calc")
        self.assertRefused("b", "-r", "--all", "-p", "pob-calc")
        self.assertRefused("build", "--profile", "release")
        self.assertRefused("build", "--profile=release", "--workspace", "-p", "pob-calc")
        self.assertRefused("build", "--profile", "bench")
        self.assertRefused("check", "--release", "--workspace")
        self.assertRefused("c", "--release")
        self.assertAllowed("build", "--profile", "release", "-p", "pob-calc")
        self.assertAllowed("build", "-r", "-p", "pob-calc")
        # Debug-profile workspace check/build stays allowed (the integrator needs it).
        self.assertAllowed("check", "--workspace")
        self.assertAllowed("build", "--workspace")
        # --release after `--` belongs to the test binary.
        self.assertAllowed("build", "-p", "x", "--", "--release")

    def test_guards(self):
        self.assertRefused("test", "--workspace")
        self.assertRefused("build", "--release")
        self.assertAllowed("build", "--release", "-p", "pob-calc")
        r = self.run_sh("test", "--workspace", POB_PORT_ALLOW_WORKSPACE="1")
        self.assertEqual(r.returncode, 0, r.stderr)
        r = self.run_sh()
        self.assertEqual(r.returncode, 2)
        self.assertIn("usage", r.stderr)

    def test_allow_workspace_keeps_slots_and_phases(self):
        # POB_PORT_ALLOW_WORKSPACE=1 relaxes only the guards: a workspace test still compiles under a
        # compile slot and runs under a run slot, and read-only subcommands still take no slot.
        # (Before the fix in this unit the override skipped subcommand detection altogether.)
        cs = self.calls(self.run_sh("test", "--workspace", POB_PORT_ALLOW_WORKSPACE="1").stdout)
        self.assertEqual([c["args"] for c in cs],
                         [self.expect_args(["test", "--workspace", "--no-run"]),
                          self.expect_args(["test", "--workspace"])])
        self.assertEqual([c["held"] for c in cs], ["cargo.lock", "run.lock"])
        self.hold("cargo.lock")
        self.hold("cargo.lock.1")
        r = self.run_sh("metadata", POB_PORT_ALLOW_WORKSPACE="1")
        self.assertEqual(self.one_call(r)["args"], "metadata")

    # --- compile slots -------------------------------------------------------------------------

    def test_lock_serialises(self):
        lock = self.hold("cargo.lock")
        p = self.popen("check", "-p", "x", POB_PORT_SLOTS="1")
        time.sleep(1.0)
        self.assertIsNone(p.poll(), "cargo.sh must wait while the only compile slot is held")
        self.release(lock)
        out, err = p.communicate(timeout=30)
        self.assertEqual(p.returncode, 0, err)
        self.assertIn("waiting for one of 1 compile slots", err)
        self.assertEqual(self.calls(out)[0]["args"], self.expect_args(["check", "-p", "x"]))

    def test_default_two_compile_slots(self):
        # One slot busy: the run takes cargo.lock.1 without waiting.
        self.hold("cargo.lock")
        r = self.run_sh("check", "-p", "x")
        self.assertEqual(self.one_call(r)["held"], "cargo.lock cargo.lock.1")
        self.assertNotIn("waiting", r.stderr)
        # Both busy: it waits.
        lock1 = self.hold("cargo.lock.1")
        p = self.popen("check", "-p", "x")
        time.sleep(1.0)
        self.assertIsNone(p.poll(), "cargo.sh must wait while both compile slots are held")
        self.release(lock1)
        out, err = p.communicate(timeout=30)
        self.assertEqual(p.returncode, 0, err)
        self.assertIn("waiting for one of 2 compile slots", err)

    def test_two_runs_do_not_overlap(self):
        # With one compile slot, two compiles run one after the other.
        t0 = time.time()
        ps = [self.popen("check", "-p", str(i), FAKE_SLEEP="1", POB_PORT_SLOTS="1") for i in range(2)]
        for p in ps:
            p.communicate(timeout=30)
            self.assertEqual(p.returncode, 0)
        self.assertGreaterEqual(time.time() - t0, 1.9)

    def test_two_slots_run_concurrently(self):
        # With the default two compile slots, two compiles are inside cargo at the same time.
        barrier = os.path.join(self.tmp, "barrier")
        os.makedirs(barrier)
        ps = [self.popen("check", "-p", str(i), FAKE_BARRIER=barrier, FAKE_BARRIER_N="2") for i in range(2)]
        for p in ps:
            out, err = p.communicate(timeout=30)
            self.assertEqual(p.returncode, 0, err)
            self.assertIn("barrier=ok", out)
        # ... but not a third one.
        os.makedirs(os.path.join(self.tmp, "barrier3"))
        ps = [self.popen("check", "-p", str(i), FAKE_BARRIER=os.path.join(self.tmp, "barrier3"), FAKE_BARRIER_N="3",
                         FAKE_BARRIER_TRIES="20") for i in range(3)]
        outs = [p.communicate(timeout=60)[0] for p in ps]
        self.assertTrue(any("barrier=timeout" in o for o in outs), outs)

    # --- two-phase test/bench ------------------------------------------------------------------

    def test_test_is_compiled_then_run_under_a_run_slot(self):
        for args in (["test", "-p", "x", "--lib", "--", "--nocapture"], ["t", "-p", "x"], ["bench", "-p", "x"]):
            r = self.run_sh(*args)
            self.assertEqual(r.returncode, 0, r.stderr)
            cs = self.calls(r.stdout)
            pre = args[:args.index("--")] if "--" in args else args
            self.assertEqual([c["args"] for c in cs],
                             [self.expect_args(pre + ["--no-run"]), self.expect_args(args)], args)
            # Phase 1 holds only a compile slot; phase 2 holds only a run slot (the compile slot is
            # released so long test runs do not block other compiles).
            self.assertEqual([c["held"] for c in cs], ["cargo.lock", "run.lock"], args)

    def test_no_run_is_a_single_compile_phase(self):
        c = self.one_call(self.run_sh("test", "-p", "x", "--no-run"))
        self.assertEqual(c["args"], self.expect_args(["test", "-p", "x", "--no-run"]))
        self.assertEqual(c["held"], "cargo.lock")
        # A --no-run after `--` belongs to the test binary: still two phases.
        cs = self.calls(self.run_sh("test", "-p", "x", "--", "--no-run").stdout)
        self.assertEqual([c["args"] for c in cs], [self.expect_args(["test", "-p", "x", "--no-run"]),
                                                   self.expect_args(["test", "-p", "x", "--", "--no-run"])])

    def test_non_test_subcommands_are_single_phase(self):
        for sub in ("check", "build", "clippy", "run", "fmt"):
            c = self.one_call(self.run_sh(sub, "-p", "x"))
            self.assertEqual(c["held"], "cargo.lock", sub)

    def test_compile_failure_skips_the_run_phase(self):
        r = self.run_sh("test", "-p", "x", FAKE_RC_COMPILE="101")
        self.assertEqual(r.returncode, 101)
        self.assertEqual(len(self.calls(r.stdout)), 1)
        self.assertEqual(self.run_sh("test", "-p", "x", FAKE_RC="3").returncode, 3)

    def test_default_three_run_slots(self):
        self.hold("run.lock")
        self.hold("run.lock.1")
        cs = self.calls(self.run_sh("test", "-p", "x").stdout)
        self.assertEqual(cs[1]["held"], "run.lock run.lock.1 run.lock.2")
        lock2 = self.hold("run.lock.2")
        p = self.popen("test", "-p", "x")
        time.sleep(1.0)
        self.assertIsNone(p.poll(), "the run phase must wait while all three run slots are held")
        self.release(lock2)
        out, err = p.communicate(timeout=30)
        self.assertEqual(p.returncode, 0, err)
        self.assertIn("waiting for one of 3 test-run slots", err)
        self.assertEqual(len(self.calls(out)), 2)

    def test_waiting_run_phase_frees_the_compile_slot(self):
        # A test waiting for a run slot has already released its compile slot: with a single compile
        # slot, another compile proceeds meanwhile.
        log = os.path.join(self.tmp, "cargo.log")
        run = self.hold("run.lock")
        p = self.popen("test", "-p", "x", POB_PORT_SLOTS="1", POB_PORT_RUN_SLOTS="1", FAKE_LOG=log)
        deadline = time.time() + 20
        while time.time() < deadline and "--no-run" not in read(log):
            time.sleep(0.1)
        self.assertIn("--no-run", read(log), "phase 1 never ran")
        r = self.run_sh("check", "-p", "y", POB_PORT_SLOTS="1")   # may retry once (2 s) on a close race
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIsNone(p.poll(), "the test must still be waiting for its run slot")
        self.release(run)
        out, err = p.communicate(timeout=30)
        self.assertEqual(p.returncode, 0, err)
        self.assertIn("waiting for one of 1 test-run slots", err)
        # Phase 1 ran while this test held run.lock; phase 2 holds run.lock itself.
        self.assertEqual([c["held"] for c in self.calls(out)], ["cargo.lock run.lock", "run.lock"])

    # --- read-only subcommands -----------------------------------------------------------------

    def test_read_only_subcommands_take_no_slot(self):
        self.hold("cargo.lock")
        self.hold("cargo.lock.1")
        for sub in READ_ONLY:
            r = self.run_sh(sub, POB_PORT_SLOTS="2")
            c = self.one_call(r)
            self.assertEqual(c["args"], sub)
            self.assertEqual(c["held"], "cargo.lock cargo.lock.1", sub)
            self.assertNotIn("waiting", r.stderr)
        self.assertEqual(self.one_call(self.run_sh("+nightly", "metadata", "--format-version", "1"))["args"],
                         "+nightly metadata --format-version 1")
        self.assertFalse(os.path.exists(os.path.join(self.lockdir, "run.lock")))

    # --- systemd scope -------------------------------------------------------------------------

    def scope_calls(self):
        if not os.path.exists(self.scope_log):
            return []
        with open(self.scope_log) as fh:
            return fh.read().splitlines()

    def test_scope_wraps_each_cargo_run(self):
        env = dict(self.env)
        env.pop("POB_PORT_NO_SCOPE")
        c = self.one_call(self.run_sh("check", "-p", "x", env=env))
        self.assertEqual(c["scope"], "1")
        self.assertEqual(self.scope_calls(), [
            "--user --scope --quiet true",
            "--user --scope --quiet -p MemoryMax=5G -p MemorySwapMax=1G cargo " + self.expect_args(["check", "-p", "x"]),
        ])
        os.remove(self.scope_log)
        # Both phases of a test run in their own scope; the limits are configurable.
        r = self.run_sh("test", "-p", "x", env=env, POB_PORT_MEM="2G", POB_PORT_SWAP="0")
        self.assertEqual([c["scope"] for c in self.calls(r.stdout)], ["1", "1"])
        self.assertEqual(self.scope_calls()[1:], [
            "--user --scope --quiet -p MemoryMax=2G -p MemorySwapMax=0 cargo " + self.expect_args(["test", "-p", "x", "--no-run"]),
            "--user --scope --quiet -p MemoryMax=2G -p MemorySwapMax=0 cargo " + self.expect_args(["test", "-p", "x"]),
        ])
        os.remove(self.scope_log)
        # Read-only subcommands run bare.
        self.assertEqual(self.one_call(self.run_sh("metadata", env=env))["scope"], "0")
        self.assertEqual(self.scope_calls()[1:], [])

    def test_scope_unavailable_or_disabled_runs_bare(self):
        env = dict(self.env)
        env.pop("POB_PORT_NO_SCOPE")
        c = self.one_call(self.run_sh("check", "-p", "x", env=env, FAKE_SCOPE_PROBE_RC="1"))
        self.assertEqual(c["scope"], "0")
        self.assertEqual(self.scope_calls(), ["--user --scope --quiet true"])
        os.remove(self.scope_log)
        # POB_PORT_NO_SCOPE=1 (the default env of these tests): systemd-run is never called.
        self.assertEqual(self.one_call(self.run_sh("check", "-p", "x"))["scope"], "0")
        self.assertEqual(self.scope_calls(), [])

    # --- /tmp worktrees: --target-dir under ~/.cache/pob-port-targets ---------------------------

    def test_tmp_worktree_gets_cached_target_dir(self):
        _, wt = self.fake_checkout(linked="review-x")
        script = self.script_in(wt)
        td = os.path.join(self.home, ".cache", "pob-port-targets", "review-x")
        c = self.one_call(self.run_sh("check", "-p", "x", script=script, cwd=self.tmp))
        self.assertEqual(c["args"], "check --target-dir %s -p x" % td)
        self.assertEqual(c["pwd"], wt)
        self.assertEqual(c["target_env"], "<unset>")     # an argument, never the env (sccache)
        with open(os.path.join(td, ".worktree")) as fh:
            self.assertEqual(fh.read().strip(), wt)
        # Inserted right after the subcommand, in both phases of a test.
        cs = self.calls(self.run_sh("+nightly", "test", "-p", "x", "--", "--exact", script=script).stdout)
        self.assertEqual([c["args"] for c in cs], [
            "+nightly test --target-dir %s -p x --no-run" % td,
            "+nightly test --target-dir %s -p x -- --exact" % td,
        ])
        for sub in TARGET_DIR_SUBS:
            args = [sub, "-p", "x"]
            cs = self.calls(self.run_sh(*args, script=script).stdout)
            self.assertEqual(cs[-1]["args"], "%s --target-dir %s -p x" % (sub, td), sub)
        # Other subcommands are left alone.
        for args in (["metadata"], ["fmt", "--all"], ["clean", "-p", "x"]):
            self.assertEqual(self.one_call(self.run_sh(*args, script=script))["args"], " ".join(args))

    def test_non_tmp_worktree_keeps_default_target_dir(self):
        root = self.root()
        if on_tmpfs_path(root):
            self.skipTest("this checkout is itself under /tmp")
        c = self.one_call(self.run_sh("check", "-p", "x"))
        self.assertNotIn("--target-dir", c["args"])
        self.assertFalse(os.path.exists(os.path.join(self.home, ".cache", "pob-port-targets")))

    def test_stale_target_dirs_are_removed(self):
        tc = os.path.join(self.home, ".cache", "pob-port-targets")
        alive = os.path.join(self.tmp, "alive")
        os.makedirs(alive)
        for name, marker in (("gone", os.path.join(self.tmp, "no-such-worktree")), ("alive", alive), ("nomarker", None)):
            os.makedirs(os.path.join(tc, name, "debug"))
            if marker:
                with open(os.path.join(tc, name, ".worktree"), "w") as fh:
                    fh.write(marker + "\n")
        self.one_call(self.run_sh("check", "-p", "x"))
        self.assertEqual(sorted(n for n in os.listdir(tc) if n != os.path.basename(self.root())),
                         ["alive", "nomarker"])

    # --- quick corpus in the main checkout -----------------------------------------------------

    def make_quick_corpus(self, root, n=2):
        b = os.path.join(root, ".parity-quick", "builds")
        os.makedirs(b)
        for i in range(n):
            open(os.path.join(b, "b%d.xml" % i), "w").close()

    def test_main_checkout_test_uses_quick_corpus(self):
        main, wt = self.fake_checkout(linked="linked")
        self.make_quick_corpus(main, 3)
        self.make_quick_corpus(wt, 3)
        quick = os.path.join(main, ".parity-quick")
        for sub in ("test", "t"):
            r = self.run_sh(sub, "-p", "x", script=self.script_in(main))
            self.assertEqual(r.returncode, 0, r.stderr)
            cs = self.calls(r.stdout)
            self.assertEqual([(c["parity"], c["quick"]) for c in cs], [(quick, "1")] * 2, sub)
            self.assertIn("main checkout -> quick corpus (3 builds, POB_PARITY_DIR=%s)" % quick, r.stderr)
        # Not for other subcommands (bench, check, ...).
        for args in (["check", "-p", "x"], ["bench", "-p", "x"], ["build", "-p", "x"]):
            cs = self.calls(self.run_sh(*args, script=self.script_in(main)).stdout)
            self.assertEqual({(c["parity"], c["quick"]) for c in cs}, {("<unset>", "<unset>")}, args)
        # POB_FULL_SUITE=1 forces the full corpus; an explicit POB_PARITY_DIR is kept.
        cs = self.calls(self.run_sh("test", "-p", "x", script=self.script_in(main), POB_FULL_SUITE="1").stdout)
        self.assertEqual({(c["parity"], c["quick"]) for c in cs}, {("<unset>", "<unset>")})
        cs = self.calls(self.run_sh("test", "-p", "x", script=self.script_in(main), POB_PARITY_DIR="/x").stdout)
        self.assertEqual({(c["parity"], c["quick"]) for c in cs}, {("/x", "<unset>")})
        # Linked worktrees (.git is a file) keep the full corpus.
        r = self.run_sh("test", "-p", "x", script=self.script_in(wt))
        self.assertEqual({(c["parity"], c["quick"]) for c in self.calls(r.stdout)}, {("<unset>", "<unset>")})
        self.assertNotIn("quick corpus", r.stderr)

    def test_main_checkout_without_quick_corpus_keeps_full(self):
        main, _ = self.fake_checkout()
        r = self.run_sh("test", "-p", "x", script=self.script_in(main))
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual({(c["parity"], c["quick"]) for c in self.calls(r.stdout)}, {("<unset>", "<unset>")})


if __name__ == "__main__":
    unittest.main()
