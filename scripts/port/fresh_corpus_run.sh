#!/usr/bin/env bash
# Run the CA L6 corpus harness and the D13 fixed point on the INT-1-FRESH corpus
# (docs/port/parity/fresh-corpus.md), in chunks of $CHUNK builds per test process.
#
# Usage: scripts/port/fresh_corpus_run.sh [stem ...]      (default: every .parity/fresh/*.xml)
#
# One test process at a time, each under scripts/port/cargo.sh (5 GB scope); inside it the
# harness takes the builds one at a time and drops each build's state (GlobalCache, LUTs) before
# the next. Chunking only bounds how much one crash loses and how long one process lives; every
# cargo.sh call waits for the machine-wide compile slot, so one call per build is too slow.
# Per chunk: <fresh>/results/<first>..<last>.log (the test's stderr) and .tsv (CA_L6_DIFFS:
# every L6 value that is not exact, `stem block key rust pob`). A stem listed in
# results/done.txt is skipped; a chunk whose test process dies of a signal is not marked done.
# PoB's runs are cached by the test in target/tmp/ca-probe/<stem>/ (error.txt = PoB failed).
set -uo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
chunk="${CHUNK:-20}"
fresh="${FRESH_DIR:-}"
if [ -z "$fresh" ]; then
    d="$root"
    while [ "$d" != "/" ]; do
        if [ -d "$d/.parity/fresh" ]; then fresh="$d/.parity/fresh"; break; fi
        d="$(dirname "$d")"
    done
fi
[ -n "$fresh" ] || { echo "no .parity/fresh (set FRESH_DIR)" >&2; exit 1; }
res="$fresh/results"
mkdir -p "$res"
touch "$res/done.txt"
all=()
if [ "$#" -gt 0 ]; then
    all=("$@")
else
    for f in "$fresh"/*.xml; do all+=("$(basename "$f" .xml)"); done
fi
todo=()
for s in "${all[@]}"; do
    grep -qx "$s" "$res/done.txt" || todo+=("$s")
done
cd "$root" || exit 1
i=0
while [ "$i" -lt "${#todo[@]}" ]; do
    part=("${todo[@]:i:chunk}")
    i=$((i + chunk))
    name="${part[0]}..${part[${#part[@]}-1]}"
    list=$(IFS=,; echo "${part[*]}")
    rm -f "$res/$name.tsv"
    start=$(date +%s)
    CA_CORPUS_DIR="$fresh" CA_BUILDS="$list" CA_L6_DIFFS="$res/$name.tsv" \
        scripts/port/cargo.sh test --release -q -p pob-oracle --test CA -- \
        ca_l6_corpus ca_d13_fixed_point --nocapture --test-threads=1 > "$res/$name.log" 2>&1
    rc=$?
    echo "exit $rc after $(( $(date +%s) - start ))s" >> "$res/$name.log"
    echo "$name: exit $rc"
    # 0 = all exact, 101 = a gate assertion failed (expected on a fresh corpus); anything else
    # (a signal, the OOM scope) leaves the chunk to be rerun
    if [ "$rc" = 0 ] || [ "$rc" = 101 ]; then
        printf '%s\n' "${part[@]}" >> "$res/done.txt"
    fi
done
