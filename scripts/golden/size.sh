#!/usr/bin/env bash
# Measure the goldens (docs/standalone/GOLDEN-PLAN.md §2.3) and record the sizes in MANIFEST.json.
#
# Usage: scripts/golden/size.sh [--no-write] [<goldens dir>]     (default: tests/golden)
#
# Two measures, the caps of §2.3:
#   working tree  the bytes of the checked-out files (`du -sb`: plain files at full size, .zst shards
#                 at their compressed size)                                        cap 250 MB
#   packed        the `git gc --aggressive` pack of the goldens directory alone, in a scratch
#                 repository: what the history and a clone add                     cap  80 MB
# plus the largest file on disk (cap 8 MB). `golden manifest sizes` writes them into MANIFEST.json and
# exits 1 when a cap is exceeded; with --no-write the numbers are only printed.
set -euo pipefail

write=1
if [ "${1:-}" = "--no-write" ]; then
    write=0
    shift
fi
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
root=$(git -C "$here" rev-parse --show-toplevel)
dir=$(cd "${1:-$root/tests/golden}" && pwd -P)

working=$(du -sb "$dir" | cut -f1)

tmp=$(mktemp -d "${TMPDIR:-/tmp}/golden-size.XXXXXX")
trap 'rm -rf "$tmp"' EXIT
cp -r "$dir" "$tmp/golden"
export GIT_DIR="$tmp/git" GIT_WORK_TREE="$tmp/golden"
export GIT_AUTHOR_NAME=size GIT_AUTHOR_EMAIL=size@localhost GIT_COMMITTER_NAME=size GIT_COMMITTER_EMAIL=size@localhost
git init -q
git add -A
git commit -q -m size --no-verify
git gc -q --aggressive --prune=now
packed=0
for p in "$GIT_DIR"/objects/pack/*.pack; do
    [ -e "$p" ] || continue
    packed=$((packed + $(stat -c %s "$p")))
done
unset GIT_DIR GIT_WORK_TREE

largest=$(find "$dir" -type f -printf '%s %P\n' | sort -n | tail -1)
echo "goldens $dir: working tree $working B, packed $packed B, largest file ${largest:-0 -}"

if [ "$write" = 1 ]; then
    "$root/scripts/port/cargo.sh" run -q -p pob-golden --bin golden -- --root "$dir" manifest sizes "$working" "$packed"
fi
