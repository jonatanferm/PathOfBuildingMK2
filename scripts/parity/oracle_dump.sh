#!/usr/bin/env bash
# Dump Path of Building's own calc results for the build corpus: for every
# build, one fresh `pob_oracle all` process (PoB under LuaJIT; no state shared
# between builds) writes
#   .parity/oracle/<stem>/outputs.json   complete env.player/minion.output
#   .parity/oracle/<stem>/stages.json    initEnv mod DBs, active skills, per-skill outputs
#   .parity/oracle/<stem>/log.txt        PoB's console output + timing
# and a summary .parity/oracle/timings.tsv. See crates/pob-oracle/README.md.
#
# Usage: scripts/parity/oracle_dump.sh [build.xml ...]     (default: .parity/builds/*.xml)
# Env:   JOBS (parallel processes, default 3), POB_ORACLE_BIN (skip the cargo build),
#        POB_PARITY_DIR (default: nearest .parity/ upwards), ORACLE_OUT, POB_SRC.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"

parity="${POB_PARITY_DIR:-}"
if [ -z "$parity" ]; then
    # From a git worktree the gitignored corpus lives in an enclosing checkout.
    d="$root"
    while [ "$d" != "/" ]; do
        if [ -d "$d/.parity/builds" ]; then parity="$d/.parity"; break; fi
        d="$(dirname "$d")"
    done
fi
[ -n "$parity" ] || { echo "no .parity/builds found (set POB_PARITY_DIR)" >&2; exit 1; }

bin="${POB_ORACLE_BIN:-}"
if [ -z "$bin" ]; then
    (cd "$root" && cargo build --release -q -p pob-oracle --bin pob_oracle)
    bin="$root/target/release/pob_oracle"
fi

out="${ORACLE_OUT:-$parity/oracle}"
jobs="${JOBS:-3}"
if [ "$#" -gt 0 ]; then builds=("$@"); else builds=("$parity"/builds/*.xml); fi
mkdir -p "$out"

dump_one() {
    local xml="$1" stem dir t0 t1 status
    stem="$(basename "$xml" .xml)"
    dir="$out/$stem"
    mkdir -p "$dir"
    t0=$(date +%s.%N)
    if "$bin" all "$xml" --out-dir "$dir" > "$dir/log.txt" 2>&1; then status=ok; else status=FAIL; fi
    t1=$(date +%s.%N)
    printf '%s\t%s\t%s\n' "$stem" "$status" "$(awk -v a="$t1" -v b="$t0" 'BEGIN { printf "%.2f", a - b }')"
}
export -f dump_one
export bin out

echo "pob_oracle: ${#builds[@]} builds, $jobs at a time -> $out" >&2
start=$(date +%s.%N)
printf '%s\0' "${builds[@]}" \
    | xargs -0 -P "$jobs" -I{} bash -c 'dump_one "$1"' _ {} \
    | tee "$out/timings.tsv.part"
end=$(date +%s.%N)
sort "$out/timings.tsv.part" > "$out/timings.tsv" && rm -f "$out/timings.tsv.part"

awk -F'\t' -v wall="$(awk -v a="$end" -v b="$start" 'BEGIN { print a - b }')" '
    { n++; sum += $3; if ($3 > max) { max = $3; slow = $1 } if ($2 != "ok") { fail++; failed = failed " " $1 } }
    END {
        printf "\n%d builds, %d failed%s\n", n, fail, (fail ? ":" failed : "")
        printf "wall %.1fs, per build avg %.2fs, max %.2fs (%s), sum of per-build times %.1fs\n", wall, sum / n, max, slow, sum
    }' "$out/timings.tsv"
