#!/usr/bin/env python3
"""Stand-in for GNU `/usr/bin/time -v` (not installed on this machine).

usage: calc_ptime.py LOGFILE TAG -- cmd args...   (calc-split-plan §1.3; calc_rustc_wrap.sh uses it)
Runs cmd, waits with wait4() and appends one line to LOGFILE:
  TAG maxrss_kb=<ru_maxrss of the child and its waited-for descendants> wall_s user_s sys_s exit
ru_maxrss from wait4 is the same number GNU time prints as "Maximum resident set size".
"""
import os
import sys
import time

log, tag = sys.argv[1], sys.argv[2]
cmd = sys.argv[sys.argv.index("--") + 1:]
t0 = time.monotonic()
pid = os.fork()
if pid == 0:
    try:
        os.execvp(cmd[0], cmd)
    finally:
        os._exit(127)
_, status, ru = os.wait4(pid, 0)
wall = time.monotonic() - t0
code = os.waitstatus_to_exitcode(status)
with open(log, "a") as f:
    f.write(f"{tag} maxrss_kb={ru.ru_maxrss} wall_s={wall:.1f} user_s={ru.ru_utime:.1f} sys_s={ru.ru_stime:.1f} exit={code}\n")
sys.exit(code if code >= 0 else 128 - code)
