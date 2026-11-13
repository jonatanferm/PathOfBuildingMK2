#!/usr/bin/env bash
# The profiling suite of regular use (PROF1; docs/perf/PROFILING.md).
#
# Usage: scripts/perf/prof.sh <pob-prof command> [args...]
#   scripts/perf/prof.sh run [--scenarios a,b] [--reps 5] [--warmup 1] [--render none|cpu|wgpu] [--out F]
#   scripts/perf/prof.sh compare BASE.json NEW.json [--threshold 10]   # the RSS peaks keep 20 unless
#                                                  # --field-threshold rss_peak_mb=PCT
#   scripts/perf/prof.sh bless NEW.json            # make NEW the committed baseline
#   scripts/perf/prof.sh check [run args...]       # run, then compare against the baseline
#   scripts/perf/prof.sh inspect|exec|stages|list|show|hotspots ...
#
# It builds `pob-prof` in the `prof` profile (release + line tables) through scripts/port/cargo.sh
# with POB_PORT_JOBS=1 (a release build of the app at 3 jobs can exceed the 5 GB cargo scope), then
# runs it. Measuring commands (run, exec, inspect, stages, check) run:
#   - under one machine-wide run slot of scripts/port/cargo.sh (so test runs and measurements do
#     not pile up),
#   - in their own systemd scope (MemoryMax $POB_PERF_MEM, default 3G),
#   - pinned with taskset to the fastest cores ($POB_PERF_CPUS, a taskset list; default: every CPU
#     with the highest max frequency, the Firestorm cores 2-7 on this M1 Pro; "none" = no pinning).
# The CPU frequency governor cannot be pinned without root: the run records it, and
# docs/perf/PROFILING.md "Noise" says how to set it when you have root.
# POB_PROF_NO_BUILD=1 skips the build. POB_PROF_FEATURES=dhat-heap builds the heap-profiling binary
# (into the same target path: build again without it before measuring).
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
[ "$#" -ge 1 ] || { awk 'NR > 1 && !/^#/ { exit } NR > 1 { print }' "$0" >&2; exit 2; }
cmd="$1"; shift
baseline="$root/crates/pob-prof/baseline/baseline.json"

if [ "${POB_PROF_NO_BUILD:-0}" != 1 ]; then
    feat=()
    [ -n "${POB_PROF_FEATURES:-}" ] && feat=(--features "$POB_PROF_FEATURES")
    POB_PORT_JOBS="${POB_PORT_JOBS:-1}" scripts/port/cargo.sh build -q --profile prof -p pob-prof "${feat[@]}" >&2
fi
# cargo.sh builds /tmp and /dev/shm worktrees into ~/.cache/pob-port-targets/<name>/
bin="$root/target/prof/pob-prof"
case "$root" in
    /tmp/*|/dev/shm/*) bin="$HOME/.cache/pob-port-targets/$(basename "$root")/prof/pob-prof" ;;
esac
[ -x "$bin" ] || { echo "prof.sh: $bin not built" >&2; exit 1; }

case "$cmd" in
    compare|show|list|hotspots|collapse)
        exec "$bin" "$cmd" "$@" ;;
    bless)
        [ "$#" -ge 1 ] || { echo "usage: prof.sh bless RESULTS.json" >&2; exit 2; }
        exec "$bin" bless "$1" --out "$baseline" ;;
esac

# --- measuring commands -----------------------------------------------------------------------
cpus="${POB_PERF_CPUS:-}"
if [ -z "$cpus" ] && command -v lscpu >/dev/null 2>&1; then
    cpus=$(lscpu -e=CPU,MAXMHZ 2>/dev/null | awk 'NR > 1 && $2 != "" { m[$1] = $2; if ($2 > max) max = $2 }
        END { s = ""; for (c in m) if (m[c] == max) s = s (s == "" ? "" : ",") c; print s }')
fi
pin=()
if [ -n "$cpus" ] && [ "$cpus" != none ] && command -v taskset >/dev/null 2>&1; then
    pin=(taskset -c "$cpus")
fi
scope=()
if [ "${POB_PORT_NO_SCOPE:-0}" != 1 ] && command -v systemd-run >/dev/null 2>&1 \
    && systemd-run --user --scope --quiet true >/dev/null 2>&1; then
    scope=(systemd-run --user --scope --quiet -p MemoryMax="${POB_PERF_MEM:-3G}" -p MemorySwapMax=1G)
fi
# one of scripts/port/cargo.sh's run slots (fd 8, $lock_dir/run.lock[.i])
lock_dir="${POB_PORT_LOCK_DIR:-${XDG_RUNTIME_DIR:-/tmp}/pob-port}"
mkdir -p "$lock_dir"
n="${POB_PORT_RUN_SLOTS:-3}"; waited=0
while :; do
    got=0
    for i in $(seq 0 $((n - 1))); do
        if [ "$i" = 0 ]; then l="$lock_dir/run.lock"; else l="$lock_dir/run.lock.$i"; fi
        exec 8>>"$l"
        if flock -n 8; then got=1; break; fi
        exec 8>&-
    done
    [ "$got" = 1 ] && break
    if [ "$waited" = 0 ]; then echo "prof.sh: waiting for a run slot ($lock_dir)..." >&2; waited=1; fi
    sleep 2
done
echo "prof.sh: CPUs ${cpus:-all}, load $(cut -d' ' -f1-3 /proc/loadavg)" >&2

if [ "$cmd" = check ]; then
    out="$root/target/pob-prof/check-$(date +%Y%m%d-%H%M%S).json"
    "${scope[@]}" "${pin[@]}" "$bin" run --out "$out" "$@"
    exec "$bin" compare "$baseline" "$out"
fi
exec "${scope[@]}" "${pin[@]}" "$bin" "$cmd" "$@"
