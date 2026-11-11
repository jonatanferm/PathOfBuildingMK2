#!/usr/bin/env python3
"""Run-level compile/link peak of one cargo run (calc-split-plan §5.3, CCR-0131).

Sample, every 0.2 s, the summed RSS of all compiler/linker processes of one cargo run
(rustc, cc, ld*, lld, collect2) whose command line mentions MATCH (the worktree's target dir),
while CMD runs. Also records the top single process. Appends one line to LOG.
usage: calc_sumall.py LOG TAG MATCH -- cmd..
  e.g. calc_sumall.py peaks.log cold-oracle "$PWD/target" -- scripts/port/cargo.sh test -p pob-oracle --no-run"""
import os, subprocess, sys, time
log, tag, match = sys.argv[1], sys.argv[2], sys.argv[3].encode()
cmd = sys.argv[sys.argv.index("--") + 1:]
SKIP = {"python3", "python", "bash", "sh", "zsh", "cargo", "sccache", "systemd-run", "flock", "sleep"}
p = subprocess.Popen(cmd)
peak = 0; peak_desc = ""; top = 0; top_desc = ""; anon_peak = 0; pss_peak = 0


def name(cl, comm):
    try:
        return comm + ":" + cl[cl.index(b"--crate-name") + 1].decode()
    except ValueError:
        return comm


while p.poll() is None:
    tot = 0; items = []; anon_tot = 0; pss_tot = 0
    for pid in os.listdir("/proc"):
        if not pid.isdigit():
            continue
        try:
            with open(f"/proc/{pid}/comm") as f:
                comm = f.read().strip()
            if comm in SKIP:
                continue
            with open(f"/proc/{pid}/cmdline", "rb") as f:
                cl = f.read().split(b"\0")
            if match not in b" ".join(cl):
                continue
            rss = 0
            with open(f"/proc/{pid}/status") as f:
                for line in f:
                    if line.startswith("VmRSS:"):
                        rss = int(line.split()[1])
                    elif line.startswith("RssAnon:"):
                        anon_tot += int(line.split()[1])
            try:
                with open(f"/proc/{pid}/smaps_rollup") as f:
                    for line in f:
                        if line.startswith("Pss:"):
                            pss_tot += int(line.split()[1])
            except OSError:
                pass
            tot += rss; items.append((rss, name(cl, comm)))
        except (FileNotFoundError, ProcessLookupError, PermissionError):
            continue
    if tot > peak:
        peak = tot; peak_desc = ",".join(f"{n}={r//1024}M" for r, n in sorted(items, reverse=True))
    anon_peak = max(anon_peak, anon_tot); pss_peak = max(pss_peak, pss_tot)
    for r, n in items:
        if r > top:
            top, top_desc = r, n
    time.sleep(0.2)
with open(log, "a") as f:
    f.write(f"{tag} sum_peak_kb={peak} anon_sum_peak_kb={anon_peak} pss_sum_peak_kb={pss_peak} [{peak_desc}] top_single_kb={top} ({top_desc}) exit={p.returncode}\n")
sys.exit(p.returncode)
