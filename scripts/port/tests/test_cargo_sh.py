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
                              timeout=60)

    def test_env_and_passthrough(self):
        r = self.run_sh("check", "-p", "pob-calc")
        self.assertEqual(r.returncode, 0, r.stderr)
        out = dict(line.split("=", 1) for line in r.stdout.splitlines())
        self.assertEqual(out["args"], "check -p pob-calc")
        self.assertEqual(out["jobs"], "3")
        root = subprocess.run(["git", "-C", _util.PORT, "rev-parse", "--show-toplevel"], capture_output=True,
                              text=True).stdout.strip()
        self.assertEqual(out["target"], os.path.join(root, "target"))
        self.assertEqual(out["pwd"], root)
        sccache = os.path.expanduser("~/.cargo/bin/sccache")
        if os.access(sccache, os.X_OK):
            self.assertEqual(out["wrapper"], sccache)

    def test_existing_target_dir_and_wrapper_kept(self):
        env = dict(self.env, CARGO_TARGET_DIR="/tmp/x-target", RUSTC_WRAPPER="mywrap")
        r = self.run_sh("build", "-p", "a", env=env)
        self.assertIn("target=/tmp/x-target", r.stdout)
        self.assertIn("wrapper=mywrap", r.stdout)

    def test_guards(self):
        r = self.run_sh("test", "--workspace")
        self.assertEqual(r.returncode, 2)
        self.assertIn("refusing", r.stderr)
        r = self.run_sh("build", "--release")
        self.assertEqual(r.returncode, 2)
        r = self.run_sh("build", "--release", "-p", "pob-calc")
        self.assertEqual(r.returncode, 0, r.stderr)
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
