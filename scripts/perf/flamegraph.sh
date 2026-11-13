#!/usr/bin/env bash
# A sampling profile and flamegraph of one profiling scenario (PROF1; docs/perf/PROFILING.md).
#
# Usage: scripts/perf/flamegraph.sh <scenario> [--steps a,b] [--render none|cpu|wgpu] [--freq 999]
#                                   [--out DIR] [--top 15]
#
#   <scenario>  a name from `scripts/perf/prof.sh list` (e.g. c_tree_alloc)
#   --steps     sample only these steps (default: every measured step; never `_setup`, start-up
#               or the build load of the set-up)
#   --freq      perf sampling frequency (default 999 Hz)
#   --out       output directory (default target/pob-prof/flame/<scenario>-<stamp>)
#
# Writes DIR/perf.data, DIR/folded.txt (stacks folded for flamegraph tools), DIR/hotspots.txt (top
# functions by self time, self time by crate, the inclusive share of the calc stages) and, when
# `inferno-flamegraph` (cargo install inferno) or `flamegraph.pl` is on PATH, DIR/flame.svg.
#
# How: `perf record -g` (frame pointers: the aarch64 Rust code keeps them; works for this binary)
# starts disabled (`-D -1`) with a control fifo, and pob-prof enables sampling at the start of each
# selected step and disables it after (POB_PROF_PERF_CTL / POB_PROF_PERF_STEPS, crate::driver), so
# only the measured interaction is in the profile. Needs perf_event_paranoid <= 2 (user-space
# sampling of own processes; the default on this machine).
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
[ "$#" -ge 1 ] || { awk 'NR > 1 && !/^#/ { exit } NR > 1 { print }' "$0" >&2; exit 2; }
scenario="$1"; shift
steps="" render=none freq=999 out="" top=15
while [ $# -gt 0 ]; do
    case "$1" in
        --steps) steps="$2"; shift 2 ;;
        --render) render="$2"; shift 2 ;;
        --freq) freq="$2"; shift 2 ;;
        --out) out="$2"; shift 2 ;;
        --top) top="$2"; shift 2 ;;
        *) echo "flamegraph.sh: unknown argument $1" >&2; exit 2 ;;
    esac
done
command -v perf >/dev/null || { echo "flamegraph.sh: perf not found" >&2; exit 1; }
out="${out:-$root/target/pob-prof/flame/$scenario-$(date +%Y%m%d-%H%M%S)}"
mkdir -p "$out"
POB_PORT_JOBS="${POB_PORT_JOBS:-1}" scripts/port/cargo.sh build -q --profile prof -p pob-prof >&2
bin="$root/target/prof/pob-prof"
case "$root" in
    /tmp/*|/dev/shm/*) bin="$HOME/.cache/pob-port-targets/$(basename "$root")/prof/pob-prof" ;;
esac
ctl="$out/perf.ctl"
rm -f "$ctl"; mkfifo "$ctl"
export POB_PROF_PERF_CTL="$ctl" POB_PROF_PERF_STEPS="$steps"
cpus="${POB_PERF_CPUS:-}"
if [ -z "$cpus" ] && command -v lscpu >/dev/null 2>&1; then
    cpus=$(lscpu -e=CPU,MAXMHZ 2>/dev/null | awk 'NR > 1 && $2 != "" { m[$1] = $2; if ($2 > max) max = $2 }
        END { s = ""; for (c in m) if (m[c] == max) s = s (s == "" ? "" : ",") c; print s }')
fi
pin=()
if [ -n "$cpus" ] && [ "$cpus" != none ]; then pin=(taskset -c "$cpus"); fi
scope=()
if command -v systemd-run >/dev/null 2>&1 && systemd-run --user --scope --quiet true >/dev/null 2>&1; then
    scope=(systemd-run --user --scope --quiet -p MemoryMax="${POB_PERF_MEM:-3G}" -p MemorySwapMax=1G)
fi
echo "flamegraph.sh: recording $scenario (steps: ${steps:-all measured}) at $freq Hz into $out" >&2
"${scope[@]}" "${pin[@]}" perf record -F "$freq" -g -D -1 --control "fifo:$ctl" -o "$out/perf.data" -- \
    "$bin" exec "$scenario" --render "$render" > "$out/rep.json" 2> "$out/exec.log" || {
    echo "flamegraph.sh: the run failed (see $out/exec.log)" >&2; tail -3 "$out/exec.log" >&2; exit 1; }
rm -f "$ctl"
perf script -i "$out/perf.data" --no-inline 2> "$out/perf-script.log" | "$bin" collapse > "$out/folded.txt"
"$bin" hotspots "$out/folded.txt" --top "$top" | tee "$out/hotspots.txt"
if command -v inferno-flamegraph >/dev/null 2>&1; then
    inferno-flamegraph --title "pob-prof $scenario${steps:+ ($steps)}" < "$out/folded.txt" > "$out/flame.svg"
elif command -v flamegraph.pl >/dev/null 2>&1; then
    flamegraph.pl --title "pob-prof $scenario${steps:+ ($steps)}" < "$out/folded.txt" > "$out/flame.svg"
else
    echo "flamegraph.sh: no inferno-flamegraph/flamegraph.pl on PATH; folded stacks in $out/folded.txt (cargo install inferno)" >&2
fi
echo "flamegraph.sh: $out" >&2
