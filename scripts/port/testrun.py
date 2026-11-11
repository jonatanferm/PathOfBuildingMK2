#!/usr/bin/env python3
"""Parallel test-binary runner behind `scripts/port/cargo.sh test` (unit TP3, docs/port/testing.md "Runners").

`cargo test` runs the test binaries of a crate one after the other. This runner keeps cargo in charge
of *what* runs and *how* (selection, filters, `-q`, environment, working directory, doctests) and only
changes *when*: it runs the binaries in parallel, bounded, and prints their output in cargo's order.

    testrun.py run [options] -- <cargo args...>       the run phase (cargo.sh calls this)
    testrun.py record <nonce> <binary> [args...]      the recording target runner (cargo calls this)

How `run` works:
  1. It starts the cargo command it was given (`cargo test ...`, already compiled by cargo.sh's
     `--no-run` phase) with one extra `--config target.'cfg(all())'.runner=[<this file>, record, <nonce>]`.
     For each test binary cargo would run, cargo prints its usual `Running ...` line and starts the
     runner with the binary and its arguments, in cargo's environment and working directory. The
     `record` mode writes {argv, cwd, env} as one marker line on stdout and exits 0 at once, so cargo
     goes straight on to the next binary. rustdoc also starts doctest executables through the target
     runner (`--test-runtool`); those are not cargo test binaries (not `<target>/deps/<name>-<hash>`),
     and `record` execs them unchanged, so doctests run inside that cargo pass exactly as before.
  2. Each recorded binary becomes a job. Up to --jobs jobs run at once. The first one runs under the
     run slot cargo.sh already holds; each further concurrent job takes one more machine-wide run
     slot (flock on <lock-dir>/run.lock[.i], never blocking, and never while another cargo.sh is
     waiting for a run slot: <lock-dir>/run.wait.<pid>). Every job runs in its own memory-capped
     `systemd-run --user --scope` (--mem/--swap) when --scope is 1, so machine-wide at most
     <run slots> test binaries run, each under its own cap, as before.
  3. Longest jobs start first (wall times of earlier runs in --history); output does not depend on
     that: each job's stdout+stderr goes to a spool file, and the blocks are printed in cargo's order,
     the earliest unfinished one streamed live. After each block: cargo's own
     `error: test failed, to rerun pass ...` line when the binary failed, and one
     `cargo.sh: bin-time <target> <wall>s rss=<MB>MB rc=<rc>` line. Doctest output (the rest of the
     cargo pass) comes last, as with cargo, followed by cargo's `error: N targets failed:` list.
  4. Without --no-fail-fast, cargo stops at the first failing binary in its order. Here: once a job
     fails, every job after it in cargo's order is cancelled (killed when running), jobs before it
     still run, and nothing is printed after the first failing block. With --no-fail-fast every job
     runs. Exit status: 101 when a binary failed (cargo's), else that of the cargo pass.

Only the standard library is used. Lua-faithfulness rules do not apply (infrastructure unit).
"""

import fcntl
import json
import os
import re
import secrets
import signal
import subprocess
import sys
import tempfile
import threading
import time

MARK = b"\x1ePOB-TESTRUN-"
RUNNING_RE = re.compile(rb"^\s+Running (?:(unittests) )?(\S+) \(")
DEPS_BIN_RE = re.compile(r"-[0-9a-f]{16}(\.exe)?$")
TAIL_SUMMARY_RE = re.compile(rb"(?m)^error: \d+ targets? failed:\n((?:    `[^\n]*`\n?)*)")
TAIL_ONE_RE = re.compile(rb"(?m)^error: (?:test|doctest) failed, to rerun pass `([^`\n]*)`$")
SIGNAMES = {s.value: s.name for s in signal.Signals}
SIGDESC = {signal.SIGKILL: "kill", signal.SIGTERM: "terminated", signal.SIGSEGV: "segmentation fault",
           signal.SIGABRT: "abort", signal.SIGINT: "interrupt", signal.SIGBUS: "bus error"}


# --------------------------------------------------------------------------------------------------
# record: the target runner cargo starts for each test binary


def record_main(argv):
    nonce, binary, args = argv[0], argv[1], argv[2:]
    base = os.path.basename(binary)
    if os.path.basename(os.path.dirname(binary)) != "deps" or not DEPS_BIN_RE.search(base):
        os.execv(binary, [binary] + args)       # a doctest executable (rustdoc --test-runtool)
    rec = {"argv": [binary] + args, "cwd": os.getcwd(), "env": dict(os.environ)}
    line = MARK + nonce.encode() + b" " + json.dumps(rec).encode() + b"\n"
    while line:
        line = line[os.write(1, line):]
    return 0


# --------------------------------------------------------------------------------------------------
# run


def log(msg):
    try:
        sys.stderr.write("cargo.sh: %s\n" % msg)
        sys.stderr.flush()
    except OSError:
        pass


class Out:
    """stdout, tolerant of a closed pipe (`cargo.sh test ... | head`)."""
    broken = False

    @classmethod
    def write(cls, data):
        if cls.broken or not data:
            return
        try:
            sys.stdout.buffer.write(data)
            sys.stdout.buffer.flush()
        except (BrokenPipeError, OSError):
            cls.broken = True


def target_hint(header, binary, env):
    """cargo's rerun hint for a binary (`-p pkg --test X`), from its `Running` line."""
    pkg = env.get("CARGO_PKG_NAME", "")
    p = "-p %s " % pkg if pkg else ""
    m = RUNNING_RE.match(header) if header else None
    if not m:          # `cargo test -q` prints no Running lines: name the binary
        return p + "--test " + DEPS_BIN_RE.sub("", os.path.basename(binary))
    unit, src = m.group(1) is not None, m.group(2).decode("utf-8", "replace")
    parts = src.split("/")
    stem = os.path.splitext(parts[-1])[0]
    if stem == "main" and len(parts) >= 2 and parts[-2] not in ("src", "bin"):
        stem = parts[-2]        # tests/X/main.rs, src/bin/X/main.rs
    if unit:
        if parts[-1] == "main.rs" and parts[:-1] == ["src"]:
            return p + "--bin " + pkg
        if "bin" in parts[:-1]:
            return p + "--bin " + stem
        return p + "--lib"
    kind = {"tests": "--test", "benches": "--bench", "examples": "--example"}.get(parts[0], "--test")
    return p + kind + " " + stem


class Job:
    def __init__(self, index, header, rec):
        self.index = index
        self.header = header or b""
        self.argv = rec["argv"]
        self.cwd = rec["cwd"]
        self.env = rec["env"]
        self.hint = target_hint(header, self.argv[0], self.env)
        self.state = "pending"        # pending, running, done, cancelled
        self.proc = None
        self.slot = None              # an extra run-slot fd, or None for the base slot
        self.out_path = None
        self.t0 = self.t1 = None
        self.status = None            # raw wait status
        self.maxrss_kb = 0
        self.closed = False           # trailer written: the block is complete

    @property
    def failed(self):
        return self.state == "done" and self.status != 0

    def rc(self):
        if self.status is None:
            return None
        if os.WIFSIGNALED(self.status):
            return -os.WTERMSIG(self.status)
        return os.WEXITSTATUS(self.status)

    def wall(self):
        return (self.t1 or time.time()) - (self.t0 or time.time())


class Runner:
    def __init__(self, opts, cargo_args):
        self.o = opts
        self.nonce = secrets.token_hex(8)
        self.mark = MARK + self.nonce.encode() + b" "
        pre, post = cargo_args, []
        if "--" in cargo_args:
            i = cargo_args.index("--")
            pre, post = cargo_args[:i], cargo_args[i:]
        self.no_fail_fast = "--no-fail-fast" in pre
        runner = [sys.executable, "-I", os.path.abspath(__file__), "record", self.nonce]
        cfg = "target.'cfg(all())'.runner=[%s]" % ",".join(json.dumps(a) for a in runner)
        self.cargo_cmd = self.scoped(["cargo"] + pre + ["--config", cfg] + post)
        self.lock = threading.Lock()
        self.jobs = []
        self.pending_header = None
        self.prelude = bytearray()    # cargo pass output before the first binary
        self.tail = bytearray()       # cargo pass output after it (doctests, errors)
        self.enum_done = False        # the cargo pass closed its output
        self.last_record = None
        self.cargo_proc = None
        self.cargo_rc = None
        self.cargo_killed = False
        self.extra = []               # extra run-slot fds held now
        self.next_slot_try = 0.0
        self.emit_head = -1           # -1: the prelude; i: job i
        self.emit_off = 0
        self.header_out = False
        self.stopping = None
        self.fail_at = None           # fail-fast: index of the first failed job in cargo order
        self.history = self.load_history()
        self.max_running = 0
        self.spool = tempfile.mkdtemp(prefix="pob-testrun-", dir=opts["spool"])

    # --- helpers ---------------------------------------------------------------------------------

    def scoped(self, cmd):
        if self.o["scope"]:
            return ["systemd-run", "--user", "--scope", "--quiet", "-p", "MemoryMax=" + self.o["mem"],
                    "-p", "MemorySwapMax=" + self.o["swap"]] + cmd
        return cmd

    def history_key(self, job):
        return ("quick " if os.environ.get("POB_QUICK_CORPUS") == "1" else "full ") + job.hint

    def load_history(self):
        try:
            with open(self.o["history"]) as fh:
                h = json.load(fh)
            return h if isinstance(h, dict) else {}
        except (OSError, ValueError):
            return {}

    def save_history(self):
        """Merge this run's wall times into the history file (flock + temp file + rename)."""
        new = {}
        for j in self.jobs:
            # Only unfiltered runs: a filtered binary's time says nothing about the next full run.
            if j.state == "done" and not [a for a in j.argv[1:] if a not in ("--quiet", "-q")]:
                new[self.history_key(j)] = round(j.wall(), 1)
        if not new:
            return
        path = self.o["history"]
        try:
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path + ".lock", "a") as lk:
                fcntl.flock(lk, fcntl.LOCK_EX)
                try:
                    with open(path) as fh:
                        cur = json.load(fh)
                    if not isinstance(cur, dict):
                        cur = {}
                except (OSError, ValueError):
                    cur = {}
                cur.update(new)
                fd, tmp = tempfile.mkstemp(prefix=".test-times-", dir=os.path.dirname(path))
                with os.fdopen(fd, "w") as fh:
                    json.dump(cur, fh, indent=0, sort_keys=True)
                os.replace(tmp, path)
        except OSError as e:
            log("could not update %s: %s" % (path, e))

    def expected(self, job):
        k = self.history_key(job)
        if k in self.history:
            return self.history[k]
        other = ("full " if k.startswith("quick ") else "quick ") + job.hint
        return self.history.get(other, float("inf"))     # unknown: assume long, start early

    # --- run slots -------------------------------------------------------------------------------

    def slot_path(self, i):
        return os.path.join(self.o["lock_dir"], "run.lock" if i == 0 else "run.lock.%d" % i)

    def others_waiting(self):
        try:
            names = os.listdir(self.o["lock_dir"])
        except OSError:
            return False
        for n in names:
            if not n.startswith("run.wait."):
                continue
            try:
                pid = int(n[len("run.wait."):])
            except ValueError:
                continue
            if pid == os.getpid():
                continue
            try:
                os.kill(pid, 0)
                return True
            except ProcessLookupError:
                try:
                    os.unlink(os.path.join(self.o["lock_dir"], n))
                except OSError:
                    pass
            except PermissionError:
                return True
        return False

    def try_extra_slot(self):
        if self.others_waiting():
            return None
        for i in range(self.o["run_slots"]):
            try:
                fd = os.open(self.slot_path(i), os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o644)
            except OSError:
                continue
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                return fd
            except OSError:
                os.close(fd)
        return None

    def release_slot(self, fd):
        if fd in self.extra:
            self.extra.remove(fd)
        os.close(fd)

    # --- the cargo pass --------------------------------------------------------------------------

    def start_cargo(self):
        self.cargo_proc = subprocess.Popen(self.cargo_cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                           start_new_session=True)
        threading.Thread(target=self.read_cargo, daemon=True).start()

    def read_cargo(self):
        f = self.cargo_proc.stdout
        for line in iter(f.readline, b""):
            with self.lock:
                self.cargo_line(line)
        with self.lock:
            if self.pending_header is not None:
                self.cargo_text(self.pending_header)
                self.pending_header = None
        rc = self.cargo_proc.wait()
        with self.lock:
            self.cargo_rc = rc
            self.enum_done = True

    def cargo_text(self, line):
        (self.tail if self.jobs else self.prelude).extend(line)

    def cargo_line(self, line):
        if line.startswith(self.mark):
            rec = json.loads(line[len(self.mark):])
            self.jobs.append(Job(len(self.jobs), self.pending_header, rec))
            self.pending_header = None
            self.last_record = time.time()
            return
        if self.pending_header is not None:
            self.cargo_text(self.pending_header)
            self.pending_header = None
        if RUNNING_RE.match(line) or line.lstrip().startswith(b"Running `"):
            self.pending_header = line      # cargo's line for the binary it is about to run
        else:
            self.cargo_text(line)

    def kill_cargo(self):
        if self.cargo_proc and self.cargo_proc.poll() is None and not self.cargo_killed:
            self.cargo_killed = True
            try:
                os.killpg(self.cargo_proc.pid, signal.SIGKILL)
            except OSError:
                pass

    # --- jobs ------------------------------------------------------------------------------------

    def start_job(self, job, slot):
        job.out_path = os.path.join(self.spool, "%04d.log" % job.index)
        out = open(job.out_path, "wb")
        job.slot = slot
        job.t0 = time.time()
        try:
            job.proc = subprocess.Popen(self.scoped(job.argv), cwd=job.cwd, env=job.env, stdin=subprocess.DEVNULL,
                                        stdout=out, stderr=subprocess.STDOUT, start_new_session=True)
        except OSError as e:
            out.write(("error: could not run `%s`: %s\n" % (job.argv[0], e)).encode())
            out.close()
            job.state, job.status, job.t1 = "done", 101 << 8, time.time()
            self.close_job(job)
            return
        out.close()
        job.state = "running"
        threading.Thread(target=self.wait_job, args=(job,), daemon=True).start()

    def wait_job(self, job):
        while True:
            try:
                _, status, ru = os.wait4(job.proc.pid, 0)
                break
            except InterruptedError:
                continue
            except ChildProcessError:
                status, ru = 255 << 8, None
                break
        job.proc.returncode = 0           # reaped here; keep Popen from waiting again
        with self.lock:
            job.t1 = time.time()
            job.status = status
            job.maxrss_kb = ru.ru_maxrss if ru else 0
            if job.state == "running":
                job.state = "done"
            self.close_job(job)

    def close_job(self, job):
        """Append cargo's failure line and the timing line, release the job's slot."""
        if job.slot is not None:
            self.release_slot(job.slot)
            job.slot = None
        if job.state == "cancelled":
            job.closed = True
            return
        rc = job.rc()
        lines = []
        if rc != 0:
            lines.append("error: test failed, to rerun pass `%s`" % job.hint)
            if rc is not None and rc < 0:
                sig = -rc
                lines += ["", "Caused by:", "  process didn't exit successfully: `%s` (signal: %d, %s: %s)"
                          % (" ".join(job.argv), sig, SIGNAMES.get(sig, "SIG?"), SIGDESC.get(sig, "signal"))]
        lines.append("cargo.sh: bin-time %s %.1fs rss=%dMB rc=%s" % (job.hint, job.wall(), job.maxrss_kb // 1024, rc))
        with open(job.out_path, "ab") as fh:
            fh.write(("\n".join(lines) + "\n").encode())
        job.closed = True
        n_done = sum(1 for j in self.jobs if j.state == "done")
        log("[%d/%d%s] %s %.1fs %s" % (n_done, len(self.jobs), "" if self.enum_done else "+",
                                        "ok" if rc == 0 else "FAILED rc=%s" % rc, job.wall(), job.hint))
        if rc != 0 and not self.no_fail_fast:
            if self.fail_at is None or job.index < self.fail_at:
                self.fail_at = job.index
                self.cancel_after(job.index)

    def cancel_after(self, index):
        self.kill_cargo()
        for j in self.jobs:
            if j.index > index and j.state in ("pending", "running"):
                if j.state == "running":
                    try:
                        os.killpg(j.proc.pid, signal.SIGKILL)
                    except OSError:
                        pass
                j.state = "cancelled"
                if j.proc is None:
                    j.closed = True

    def kill_all(self, sig):
        self.kill_cargo()
        for j in self.jobs:
            if j.state == "running":
                try:
                    os.killpg(j.proc.pid, sig)
                except OSError:
                    pass

    # --- scheduling ------------------------------------------------------------------------------

    def schedule(self):
        running = [j for j in self.jobs if j.state == "running"]
        pending = [j for j in self.jobs if j.state == "pending"]
        base_busy = any(j.slot is None for j in running)
        if not pending:
            return
        # Wait until cargo has listed the binaries (or paused for a second) so the longest go first.
        if self.o["jobs"] > 1 and not self.enum_done and time.time() - (self.last_record or 0) < 1.0:
            return
        if self.o["jobs"] > 1:
            pending.sort(key=lambda j: (-self.expected(j), j.index))
        for job in pending:
            if len(running) >= self.o["jobs"]:
                return
            if not base_busy:
                slot = None
                base_busy = True
            else:
                if time.time() < self.next_slot_try:
                    return
                slot = self.try_extra_slot()
                if slot is None:
                    self.next_slot_try = time.time() + 2.0
                    return
                self.extra.append(slot)
            self.start_job(job, slot)
            running.append(job)
            self.max_running = max(self.max_running, len(running))

    # --- output ----------------------------------------------------------------------------------

    def run(self):
        for s in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
            signal.signal(s, self.on_signal)
        t0 = time.time()
        self.start_cargo()
        while True:
            with self.lock:
                if self.stopping is None:
                    self.schedule()
                self.emit_partial()
                active = [j for j in self.jobs if j.state in ("pending", "running") or (j.proc and not j.closed)]
                if self.stopping is not None and not [j for j in self.jobs if j.state == "running" and not j.closed]:
                    break
                if self.enum_done and not active:
                    break
            time.sleep(0.05)
        with self.lock:
            self.finish(t0)
        return self.exit_code()

    def on_signal(self, signum, _frame):
        if self.stopping is None:
            self.stopping = signum
            log("signal %d: stopping %d running test binaries" % (signum, sum(j.state == "running" for j in self.jobs)))
            for j in self.jobs:
                if j.state == "pending":
                    j.state, j.closed = "cancelled", True
            self.kill_all(signal.SIGTERM)
            t = threading.Timer(5.0, self.kill_all, args=(signal.SIGKILL,))
            t.daemon = True
            t.start()

    def emit_partial(self):
        """Print the blocks in cargo's order: the prelude, then each job (the earliest unfinished one
        live), stopping after the first failure in fail-fast mode. finish() prints the tail."""
        while True:
            h = self.emit_head
            if h == -1:
                Out.write(bytes(self.prelude[self.emit_off:]))
                self.emit_off = len(self.prelude)
                if not (self.jobs or self.enum_done):
                    return
                self.emit_head, self.emit_off, self.header_out = 0, 0, False
                continue
            if h >= len(self.jobs):
                return                                  # the tail is printed by finish()
            if self.fail_at is not None and h > self.fail_at:
                return
            job = self.jobs[h]
            if job.state == "pending":
                return
            if job.state == "cancelled":
                if job.proc is not None and not job.closed:
                    return                              # wait until the kill is reaped
                self.emit_head, self.emit_off, self.header_out = h + 1, 0, False
                continue
            if not self.header_out:
                Out.write(job.header)
                self.header_out = True
            closed = job.closed
            with open(job.out_path, "rb") as fh:
                fh.seek(self.emit_off)
                data = fh.read()
            if not closed:
                # Whole lines only while the binary runs, so the progress lines on stderr never land
                # inside a line (`test x ... FAILED` must stay one line in a 2>&1 log).
                data = data[:data.rfind(b"\n") + 1]
            Out.write(data)
            self.emit_off += len(data)
            if not closed:
                return
            self.emit_head, self.emit_off, self.header_out = h + 1, 0, False

    def finish(self, t0):
        self.emit_partial()
        failed = [j for j in self.jobs if j.failed]
        hints = [j.hint for j in failed]
        if self.fail_at is None and self.stopping is None:
            tail = bytes(self.tail)
            m = None
            for m in TAIL_SUMMARY_RE.finditer(tail):
                pass
            if m:
                hints += [h.decode("utf-8", "replace") for h in re.findall(rb"`([^`\n]*)`", m.group(1))]
                tail = tail[:m.start()] + tail[m.end():]
            elif self.cargo_rc:
                hints += [h.decode("utf-8", "replace") for h in TAIL_ONE_RE.findall(tail)]
            Out.write(tail)
            if self.no_fail_fast and hints:
                Out.write(("error: %d target%s failed:\n" % (len(hints), "" if len(hints) == 1 else "s")
                           + "".join("    `%s`\n" % h for h in hints)).encode())
        ran = [j for j in self.jobs if j.state == "done"]
        Out.write(("cargo.sh: test binaries: %d run, %d failed, %d cancelled; wall %.1fs, sum of binary walls"
                   " %.1fs, up to %d at once (POB_PORT_TEST_JOBS=%d)\n"
                   % (len(ran), len(failed), sum(j.state == "cancelled" for j in self.jobs), time.time() - t0,
                      sum(j.wall() for j in ran), self.max_running, self.o["jobs"])).encode())
        if self.stopping is None:
            self.save_history()
        if not self.o.get("keep"):
            for j in self.jobs:
                if j.out_path:
                    try:
                        os.unlink(j.out_path)
                    except OSError:
                        pass
            try:
                os.rmdir(self.spool)
            except OSError:
                pass
        for fd in list(self.extra):
            self.release_slot(fd)
        self.failed = failed

    def exit_code(self):
        if self.stopping is not None:
            return 128 + self.stopping
        if self.failed:
            return 101
        return self.cargo_rc or 0


def parse_run_args(argv):
    o = {"jobs": 3, "run_slots": 3, "lock_dir": None, "scope": False, "mem": "5G", "swap": "1G",
         "spool": None, "history": os.path.join(os.path.expanduser("~"), ".cache", "pob-port", "test-times.json"),
         "keep": False}
    i = 0
    while i < len(argv) and argv[i] != "--":
        k = argv[i]
        if k == "--keep":
            o["keep"] = True
            i += 1
            continue
        if i + 1 >= len(argv):
            raise SystemExit("testrun.py: %s needs a value" % k)
        v = argv[i + 1]
        if k == "--jobs":
            o["jobs"] = max(1, int(v))
        elif k == "--run-slots":
            o["run_slots"] = max(1, int(v))
        elif k == "--lock-dir":
            o["lock_dir"] = v
        elif k == "--scope":
            o["scope"] = v == "1"
        elif k == "--mem":
            o["mem"] = v
        elif k == "--swap":
            o["swap"] = v
        elif k == "--spool":
            o["spool"] = v
        elif k == "--history":
            o["history"] = v
        else:
            raise SystemExit("testrun.py: unknown option %s" % k)
        i += 2
    if i >= len(argv):
        raise SystemExit("testrun.py run: missing `-- <cargo args>`")
    if not o["lock_dir"]:
        raise SystemExit("testrun.py run: --lock-dir is required")
    if not o["spool"]:
        o["spool"] = tempfile.gettempdir()
    os.makedirs(o["spool"], exist_ok=True)
    return o, argv[i + 1:]


def main(argv):
    if len(argv) >= 3 and argv[0] == "record":
        return record_main(argv[1:])
    if argv and argv[0] == "run":
        opts, cargo_args = parse_run_args(argv[1:])
        return Runner(opts, cargo_args).run()
    sys.stderr.write(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
