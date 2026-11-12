#!/usr/bin/env bash
# Gate INT-2 (calc doc §9.7 "Final (INT-2)", §11 row INT-2): full recalc and power report, Rust port
# vs PoB under LuaJIT (JIT on), single-threaded, release builds. Results: docs/port/parity/INT-2-perf.md.
#
# Usage:
#   scripts/port/perf_bench.sh [--builds a,b,c] [--recalc N] [--warmup N] [--power N] [--out DIR]
#                              [--rounds N] [--rust-only] [--lua-only] [--digest] [--batch]
#
#   --builds   corpus stems (default: the INT-2 set below); "all" = every .parity/builds/*.xml
#   --recalc   timed recalcs per build (default 10), after --warmup untimed ones (default 3)
#   --power    timed power reports per process (default 1; 0 skips the power report). More than 1
#              needs memory headroom: until CCR-0135 the Rust power report keeps 0.1-2.3 GB per run
#              (summonSkill Rc cycles), so 2 runs of a minion build exceed the 3 GB cap
#   --rounds   repeat the whole per-build sequence N times (default 3), fresh processes each round;
#              the summary takes the minimum over every round's samples
#   --out      result dir (default target/int2-perf/<stamp>): lua.jsonl, rust.jsonl, summary.txt
#   --batch    one process per side for all the builds (LuaJIT side, then Rust side) instead of
#              the two sides of each build back to back: one run-slot wait instead of one per build
#   --digest   Rust only, every corpus build, --recalc 0 --power 1 (unless given), one process per
#              build: prints the output and power digests (exact f64 bits) to compare two builds of
#              the calc bit for bit
#
# Rust: `pob-calc`'s `benches/INT-2.rs` (bench profile = release: thin LTO, codegen-units = 1; no
# `probe` feature), one `cargo.sh bench` (run slot, memory-capped scope) per build. LuaJIT:
# `pob-oracle`'s `pob_calc_timing` (release), PoB unpatched with the JIT on, one process per build
# (it takes the machine-wide oracle lock; memory-capped scope). The two sides of a build run back
# to back, so they see the same machine load. Both report wall and process CPU seconds per run;
# the summary compares the minimum CPU time of each side (the least disturbed run on a shared
# machine) and also shows the medians.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"

# The INT-2 gate set (docs/port/parity/INT-2-perf.md §3.1): 6 of the 9 heaviest corpus builds by PoB
# power-report time, the heaviest by PoB recalc time, two of the builds with the highest Rust/LuaJIT
# power ratio in the full-corpus survey, and the PWR gate's cluster-jewel minion build.
default_builds="allflame_exp_013,allflame_exp_051,allflame_exp_030,allflame_exp_007,allflame_exp_039,upstream_Dual_Wield_Cospris_CoC,allflame_exp_043,allflame_exp_011,allflame_exp_000,allflame_exp_014"
builds="" recalc="" warmup=3 power="" out="" rust=1 lua=1 digest=0 batch=0 rounds=3
while [ $# -gt 0 ]; do
    case "$1" in
        --builds) builds="$2"; shift 2 ;;
        --recalc) recalc="$2"; shift 2 ;;
        --warmup) warmup="$2"; shift 2 ;;
        --power) power="$2"; shift 2 ;;
        --out) out="$2"; shift 2 ;;
        --rust-only) lua=0; shift ;;
        --lua-only) rust=0; shift ;;
        --digest) digest=1; lua=0; shift ;;   # one process per build: see --power
        --batch) batch=1; shift ;;
        --rounds) rounds="$2"; shift 2 ;;
        *) awk 'NR > 1 && !/^#/ { exit } NR > 1 { print }' "$0" >&2; exit 2 ;;
    esac
done
if [ "$digest" = 1 ]; then
    builds="${builds:-all}"; recalc="${recalc:-0}"; power="${power:-1}"; warmup=0; rounds=1
fi
builds="${builds:-$default_builds}"; recalc="${recalc:-10}"; power="${power:-1}"

parity="${POB_PARITY_DIR:-}"
if [ -z "$parity" ]; then
    d="$root"
    while [ "$d" != "/" ]; do
        if [ -d "$d/.parity/builds" ]; then parity="$d/.parity"; break; fi
        d="$(dirname "$d")"
    done
fi
[ -d "$parity/builds" ] || { echo "perf_bench: .parity/builds not found (set POB_PARITY_DIR)" >&2; exit 1; }
if [ "$builds" = all ]; then
    builds=$(ls "$parity/builds" | sed -n 's/\.xml$//p' | paste -sd, -)
fi
out="${out:-$root/target/int2-perf/$(date +%Y%m%d-%H%M%S)}"
mkdir -p "$out"

if [ "$rust" = 1 ]; then
    scripts/port/cargo.sh build -q --profile bench -p pob-calc --bench INT-2
fi
if [ "$lua" = 1 ]; then
    scripts/port/cargo.sh build -q --release -p pob-oracle --bin pob_calc_timing
    lua_bin="$root/target/release/pob_calc_timing"
fi

scope=()
if [ "${POB_PORT_NO_SCOPE:-0}" != 1 ] && command -v systemd-run >/dev/null 2>&1 \
    && systemd-run --user --scope --quiet true >/dev/null 2>&1; then
    scope=(systemd-run --user --scope --quiet -p MemoryMax="${POB_PERF_MEM:-3G}" -p MemorySwapMax=1G)
fi
# CPU pinning: on a heterogeneous CPU (this host: Apple M1 Pro, CPUs 0-1 Icestorm at 2.06 GHz,
# 2-7 Firestorm at 3.04 GHz) a run the scheduler moves to an efficiency core is up to ~1.5x
# slower, which swamped the Rust/LuaJIT difference in the first measurements. Both sides run on
# the fastest cores only: $POB_PERF_CPUS (a taskset list), default every CPU whose max frequency is
# the highest; "none" disables pinning.
cpus="${POB_PERF_CPUS:-}"
if [ -z "$cpus" ] && command -v lscpu >/dev/null 2>&1; then
    cpus=$(lscpu -e=CPU,MAXMHZ 2>/dev/null | awk 'NR > 1 && $2 != "" { m[$1] = $2; if ($2 > max) max = $2 }
        END { s = ""; for (c in m) if (m[c] == max) s = s (s == "" ? "" : ",") c; print s }')
fi
pin=()
if [ -n "$cpus" ] && [ "$cpus" != none ] && command -v taskset >/dev/null 2>&1; then
    pin=(taskset -c "$cpus")
    echo "perf_bench: pinned to CPUs $cpus" >&2
fi

: > "$out/lua.jsonl"; : > "$out/rust.jsonl"
IFS=, read -ra list <<< "$builds"
if [ "$batch" = 1 ]; then
    xmls=()
    for b in "${list[@]}"; do
        if [ -f "$parity/builds/$b.xml" ]; then xmls+=("$parity/builds/$b.xml"); else echo "perf_bench: $b.xml not found" >&2; fi
    done
    if [ "$lua" = 1 ]; then
        "${pin[@]}" "${scope[@]}" "$lua_bin" --recalc "$recalc" --warmup "$warmup" --power "$power" "${xmls[@]}" \
            >> "$out/lua.jsonl" 2> "$out/lua.stderr" || echo "perf_bench: LuaJIT failed (see $out/lua.stderr)" >&2
    fi
    if [ "$rust" = 1 ]; then
        "${pin[@]}" scripts/port/cargo.sh bench -q -p pob-calc --bench INT-2 -- \
            --recalc "$recalc" --warmup "$warmup" --power "$power" "${xmls[@]}" \
            2> "$out/rust.stderr" | grep '^{' >> "$out/rust.jsonl" || echo "perf_bench: Rust failed (see $out/rust.stderr)" >&2
    fi
    list=()
fi
[ "${#list[@]}" -gt 0 ] || rounds=0
for round in $(seq 1 "$rounds"); do
for b in "${list[@]}"; do
    xml="$parity/builds/$b.xml"
    [ -f "$xml" ] || { echo "perf_bench: $xml not found" >&2; continue; }
    if [ "$lua" = 1 ]; then
        "${pin[@]}" "${scope[@]}" "$lua_bin" --recalc "$recalc" --warmup "$warmup" --power "$power" "$xml" \
            >> "$out/lua.jsonl" 2>> "$out/lua.$b.stderr" || echo "perf_bench: LuaJIT $b failed (see $out/lua.$b.stderr)" >&2
    fi
    if [ "$rust" = 1 ]; then
        "${pin[@]}" scripts/port/cargo.sh bench -q -p pob-calc --bench INT-2 -- \
            --recalc "$recalc" --warmup "$warmup" --power "$power" "$xml" \
            2>> "$out/rust.$b.stderr" | grep '^{' >> "$out/rust.jsonl" || echo "perf_bench: Rust $b failed (see $out/rust.$b.stderr)" >&2
    fi
    echo "perf_bench: round $round: $b done" >&2
done
done

python3 -I - "$out" "$digest" <<'EOF' | tee "$out/summary.txt"
import json, statistics, sys
out, digest = sys.argv[1], sys.argv[2] == "1"
def load(p):
    # one line per build and round; the rounds' samples are merged
    r = {}
    try:
        for line in open(p):
            line = line.strip()
            if not line.startswith("{"):
                continue
            j = json.loads(line)
            prev = r.get(j["build"])
            if prev is None or "error" in prev:
                r[j["build"]] = j
            elif "error" not in j:
                for kind in ("recalc", "power"):
                    for clock in ("wall", "cpu"):
                        prev[kind][clock] += j[kind][clock]
                prev["peakRssMB"] = max(prev.get("peakRssMB", 0), j.get("peakRssMB", 0))
                for k in ("outputDigest", "powerDigest"):
                    if k in j and prev.get(k) != j[k]:
                        prev[k + "Differs"] = True
    except FileNotFoundError:
        pass
    return r
lua, rust = load(f"{out}/lua.jsonl"), load(f"{out}/rust.jsonl")
if digest:
    for b, j in sorted(rust.items()):
        if "error" in j:
            print(f"{b}: error {j['error']}")
        else:
            print(f"{b}: output {j['outputDigest']} ({j['outputLeaves']}) power {j['powerDigest']} ({j['powerLeaves']})")
    sys.exit(0)
def stat(j, kind):
    if not j or "error" in j or not j[kind]["cpu"]:
        return None
    c = j[kind]["cpu"]
    return min(c), statistics.median(c)
def ms(x):
    return "-" if x is None else f"{x * 1000:.1f}"
def s(x):
    return "-" if x is None else f"{x:.2f}"
rows, ok = [], {"recalc": True, "power": True}
print("| build | recalc LuaJIT ms (min/med) | recalc Rust ms (min/med) | ratio | power LuaJIT s (min/med) | power Rust s (min/med) | ratio | peak RSS MB LuaJIT / Rust |")
print("|---|---|---|---|---|---|---|---|")
tot = {"recalc": [0.0, 0.0], "power": [0.0, 0.0]}
for b in sorted(set(lua) | set(rust), key=lambda b: -(stat(lua.get(b), "power") or stat(lua.get(b), "recalc") or (0, 0))[0]):
    cells = [b]
    for kind, fmt in (("recalc", ms), ("power", s)):
        l, r = stat(lua.get(b), kind), stat(rust.get(b), kind)
        cells.append("-" if l is None else f"{fmt(l[0])} / {fmt(l[1])}")
        cells.append("-" if r is None else f"{fmt(r[0])} / {fmt(r[1])}")
        if l and r:
            ratio = r[0] / l[0]
            cells.append(f"{ratio:.2f}")
            tot[kind][0] += l[0]; tot[kind][1] += r[0]
            if ratio > 1.0:
                ok[kind] = False
        else:
            cells.append("-")
    cells.append(" / ".join(str((e or {}).get("peakRssMB", "-")) for e in (lua.get(b), rust.get(b))))
    if any((rust.get(b) or {}).get(k + "Differs") for k in ("outputDigest", "powerDigest")):
        cells[0] += " (Rust digest differs between rounds)"
    for e in (lua.get(b), rust.get(b)):
        if e and "error" in e:
            cells[0] += f" (error: {e['engine']}: {e['error'][:80]})"
    print("| " + " | ".join(cells) + " |")
for kind in ("recalc", "power"):
    l, r = tot[kind]
    if l > 0:
        print(f"{kind}: total min CPU LuaJIT {l:.3f} s, Rust {r:.3f} s, ratio {r / l:.2f}; every build <= LuaJIT: {ok[kind]}")
EOF
echo "perf_bench: results in $out" >&2
