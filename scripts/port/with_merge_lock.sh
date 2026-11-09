#!/usr/bin/env bash
# Run a command in the main checkout while holding the cross-workflow merge lock
# (/tmp/pob-port-merge.lock, the same mkdir lock the integrators use). Waits as long as needed,
# then also waits for a clean main checkout, and releases the lock only if it still owns it.
#   scripts/port/with_merge_lock.sh <owner-name> <command...>
set -uo pipefail
lock=/tmp/pob-port-merge.lock; owner="$1"; shift
main="$(cd "$(dirname "$0")/../.." && pwd)"
until mkdir "$lock" 2>/dev/null; do
    if [ -d "$lock" ] && [ $(( $(date +%s) - $(stat -c %Y "$lock") )) -gt 5400 ]; then rm -rf "$lock"; fi
    sleep 5
done
echo "$owner $$" > "$lock/owner"
release() { [ "$(cat "$lock/owner" 2>/dev/null)" = "$owner $$" ] && rm -rf "$lock"; }
trap release EXIT
while [ -f "$main/.git/MERGE_HEAD" ] || [ -n "$(git -C "$main" status --porcelain --untracked-files=no)" ]; do sleep 5; done
cd "$main" && "$@"
