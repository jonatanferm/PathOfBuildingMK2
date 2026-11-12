#!/usr/bin/env bash
# Bump the port's version (user decision 2026-11-12, DIVERGENCE(branding)): the repo-root VERSION
# file holds one line YYYY.MM.DD.N, N being the release counter of that local date, from 1. Writes
# today's date with N=1, or N+1 when VERSION already carries today's date, and prints the new
# version. Makes no commit or tag of its own; pob-app embeds VERSION at compile time
# (pob_app::launch::PORT_VERSION). Nothing parses the version for updates (update checker deferred).
#   Usage: scripts/port/bump_version.sh
# Test hooks: POB_VERSION_FILE (default <repo>/VERSION), POB_VERSION_TODAY (YYYY.MM.DD; default
# today's local date).
set -euo pipefail
file=${POB_VERSION_FILE:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/VERSION}
today=${POB_VERSION_TODAY:-$(date +%Y.%m.%d)}
if ! [[ $today =~ ^[0-9]{4}\.[0-9]{2}\.[0-9]{2}$ ]]; then
    echo "bump_version.sh: bad date '$today' (want YYYY.MM.DD)" >&2
    exit 2
fi
cur=""
if [[ -f $file ]]; then
    IFS= read -r cur < "$file" || true
    cur=${cur//[[:space:]]/}
fi
n=1
if [[ -n $cur ]]; then
    if ! [[ $cur =~ ^([0-9]{4}\.[0-9]{2}\.[0-9]{2})\.([1-9][0-9]*)$ ]]; then
        echo "bump_version.sh: $file holds '$cur', not YYYY.MM.DD.N" >&2
        exit 2
    fi
    if [[ ${BASH_REMATCH[1]} == "$today" ]]; then
        n=$((10#${BASH_REMATCH[2]} + 1))
    fi
fi
new="$today.$n"
tmp="$file.tmp.$$"
printf '%s\n' "$new" > "$tmp"
mv -f "$tmp" "$file"
echo "$new"
