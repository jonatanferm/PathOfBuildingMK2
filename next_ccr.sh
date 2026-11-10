#!/usr/bin/env bash
# Reserve the next free CCR number atomically (parallel agents kept picking the same "next free"
# number). Prints the 4-digit number. The reservation lives in the main checkout's
# .parity/ccr-reserved/NNNN (mkdir is atomic), and numbers already used on rust-port or any
# port/* branch are skipped.   Usage: scripts/port/next_ccr.sh <unit>-<slug>
set -euo pipefail
main=/home/ferm/Work/projects/PathOfBuildingMK2
res="$main/.parity/ccr-reserved"; mkdir -p "$res"
used=$( { git -C "$main" for-each-ref --format='%(refname)' refs/heads/rust-port 'refs/heads/port/*' |
          while read -r r; do git -C "$main" ls-tree --name-only "$r" docs/port/ccr/ 2>/dev/null; done
        ls "$res"; } | grep -oE '(^|/)[0-9]{4}' | grep -oE '[0-9]{4}' | sort -u | tail -1)
n=$((10#${used:-0} + 1))
while ! mkdir "$res/$(printf %04d $n)" 2>/dev/null; do n=$((n + 1)); done
echo "${1:-unknown} $(date '+%F %T')" > "$res/$(printf %04d $n)/owner"
printf '%04d\n' "$n"
