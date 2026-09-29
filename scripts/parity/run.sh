#!/usr/bin/env bash
# Run pob_diff over every build in .parity/builds (+ upstream spec builds)
# in parallel batches and write per-build JSON to .parity/results.
set -euo pipefail
cd "$(dirname "$0")/../.."
cargo build --release -q -p pob-extract --bin pob_diff
rm -rf .parity/results && mkdir -p .parity/results
mapfile -t builds < <(ls .parity/builds/*.xml)
jobs=${JOBS:-$(nproc)}
per=$(( (${#builds[@]} + jobs - 1) / jobs ))
for ((i = 0; i < ${#builds[@]}; i += per)); do
    args=()
    for b in "${builds[@]:i:per}"; do args+=(--build "$b"); done
    ./target/release/pob_diff --json-out .parity/results "${args[@]}" > ".parity/results/log_$i.txt" 2>&1 &
done
wait
python3 scripts/parity/report.py .parity/results "$@"
