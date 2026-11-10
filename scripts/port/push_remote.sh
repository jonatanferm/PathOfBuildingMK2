#!/usr/bin/env bash
# Push rust-port to GitHub (fast-forward only, never main, never --force). Uses the gh CLI's
# stored HTTPS credentials (no SSH key on this machine). Skips while an integrator holds the merge
# lock or a merge is in progress, so a half-done merge is never pushed. Log: .parity/push.log
# Scheduled hourly:  systemd-run --user --unit=pob-port-push --on-active=1h --on-unit-active=1h <abs path>
set -uo pipefail
main=/home/ferm/Work/projects/PathOfBuildingMK2
log="$main/.parity/push.log"
url=https://github.com/jonatanferm/PathOfBuildingMK2.git
if [ -d /tmp/pob-port-merge.lock ] || [ -f "$main/.git/MERGE_HEAD" ]; then
    echo "$(date '+%F %T') skipped: merge in progress" >> "$log"; exit 0
fi
head=$(git -C "$main" rev-parse --short rust-port)
if out=$(git -C "$main" -c credential.helper= -c credential.helper='!gh auth git-credential' \
        push "$url" rust-port:rust-port 2>&1); then
    echo "$(date '+%F %T') pushed rust-port $head: $(echo "$out" | tail -1)" >> "$log"
else
    echo "$(date '+%F %T') FAILED rust-port $head: $(echo "$out" | tail -2 | tr '\n' ' ')" >> "$log"; exit 1
fi
