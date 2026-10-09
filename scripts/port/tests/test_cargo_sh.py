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

FAKE_CARGO = """#!/bin/sh
echo "args=$*"
echo "jobs=$CARGO_BUILD_JOBS"
echo "wrapper=$RUSTC_WRAPPER"
echo "target=$CARGO_TARGET_DIR"
echo "pwd=$(pwd)"
if [ -n "$FAKE_SLEEP" ]; then sleep "$FAKE_SLEEP"; fi
"""


class CargoShTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="p4-cargo-")
        self.bin = os.path.join(self.tmp, "bin")
        os.makedirs(self.bin)
        fake = os.path.join(self.bin, "cargo")
        with open(fake, "w") as fh:
            fh.write(FAKE_CARGO)
        os.chmod(fake, os.stat(fake).st_mode | stat.S_IEXEC)
        self.lockdir = os.path.join(self.tmp, "locks")
        self.env = dict(os.environ)
        self.env["PATH"] = self.bin + os.pathsep + self.env["PATH"]
        self.env["POB_PORT_LOCK_DIR"] = self.lockdir
        for k in ("RUSTC_WRAPPER", "CARGO_TARGET_DIR", "CARGO_BUILD_JOBS", "POB_PORT_ALLOW_WORKSPACE"):
            self.env.pop(k, None)

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def run_sh(self, *args, **kw):
        return subprocess.run([CARGO_SH] + list(args), env=kw.get("env", self.env), capture_output=True, text=True,
                              timeout=60, cwd=kw.get("cwd"))

    @staticmethod
    def root():
        return subprocess.run(["git", "-C", _util.PORT, "rev-parse", "--show-toplevel"], capture_output=True,
                              text=True).stdout.strip()

    @staticmethod
    def parse(stdout):
        return dict(line.split("=", 1) for line in stdout.splitlines())

    def test_env_and_passthrough(self):
        r = self.run_sh("check", "-p", "pob-calc", cwd=self.tmp)
        self.assertEqual(r.returncode, 0, r.stderr)
        out = self.parse(r.stdout)
        self.assertEqual(out["args"], "check -p pob-calc")
        self.assertEqual(out["jobs"], "3")
        root = self.root()
        self.assertEqual(out["target"], os.path.join(root, "target"))
        # Called from outside the worktree: cargo runs in the worktree root.
        self.assertEqual(out["pwd"], root)
        sccache = os.path.expanduser("~/.cargo/bin/sccache")
        if os.access(sccache, os.X_OK):
            self.assertEqual(out["wrapper"], sccache)

    def test_cwd_inside_worktree_is_kept(self):
        # Relative paths in arguments (e.g. --manifest-path) resolve against the caller's directory.
        sub = os.path.join(self.root(), "scripts", "port")
        r = self.run_sh("check", "--manifest-path", "x/Cargo.toml", cwd=sub)
        self.assertEqual(r.returncode, 0, r.stderr)
        out = self.parse(r.stdout)
        self.assertEqual(out["pwd"], os.path.realpath(sub))
        self.assertEqual(out["target"], os.path.join(self.root(), "target"))

    def assertRefused(self, *args, cwd=None):
        r = self.run_sh(*args, cwd=cwd)
        self.assertEqual(r.returncode, 2, (args, r.stdout, r.stderr))
        self.assertIn("refusing", r.stderr)
        self.assertNotIn("args=", r.stdout)

    def assertAllowed(self, *args, cwd=None):
        r = self.run_sh(*args, cwd=cwd)
        self.assertEqual(r.returncode, 0, (args, r.stderr))
        self.assertIn("args=" + " ".join(args), r.stdout)

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
        self.assertRefused("check", "--release", "--workspace")
        self.assertAllowed("build", "--profile", "release", "-p", "pob-calc")
        self.assertAllowed("build", "-r", "-p", "pob-calc")
        # Debug-profile workspace check/build stays allowed (the integrator needs it).
        self.assertAllowed("check", "--workspace")
        self.assertAllowed("build", "--workspace")

    def test_existing_target_dir_and_wrapper_kept(self):
        env = dict(self.env, CARGO_TARGET_DIR="/tmp/x-target", RUSTC_WRAPPER="mywrap")
        r = self.run_sh("build", "-p", "a", env=env)
        self.assertIn("target=/tmp/x-target", r.stdout)
        self.assertIn("wrapper=mywrap", r.stdout)

    def test_guards(self):
        self.assertRefused("test", "--workspace")
        self.assertRefused("build", "--release")
        self.assertAllowed("build", "--release", "-p", "pob-calc")
        r = self.run_sh("test", "--workspace", env=dict(self.env, POB_PORT_ALLOW_WORKSPACE="1"))
        self.assertEqual(r.returncode, 0)
        r = self.run_sh()
        self.assertEqual(r.returncode, 2)

    def test_lock_serialises(self):
        os.makedirs(self.lockdir, exist_ok=True)
        lock = open(os.path.join(self.lockdir, "cargo.lock"), "a")
        fcntl.flock(lock, fcntl.LOCK_EX)
        p = subprocess.Popen([CARGO_SH, "check", "-p", "x"], env=self.env, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE, text=True)
        time.sleep(1.0)
        self.assertIsNone(p.poll(), "cargo.sh must wait while the lock is held")
        fcntl.flock(lock, fcntl.LOCK_UN)
        lock.close()
        out, err = p.communicate(timeout=30)
        self.assertEqual(p.returncode, 0)
        self.assertIn("waiting for the cargo lock", err)
        self.assertIn("args=check -p x", out)

    def test_two_runs_do_not_overlap(self):
        env = dict(self.env, FAKE_SLEEP="1")
        t0 = time.time()
        ps = [subprocess.Popen([CARGO_SH, "check", "-p", str(i)], env=env, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE) for i in range(2)]
        for p in ps:
            p.communicate(timeout=30)
        self.assertGreaterEqual(time.time() - t0, 1.9)


if __name__ == "__main__":
    unittest.main()
