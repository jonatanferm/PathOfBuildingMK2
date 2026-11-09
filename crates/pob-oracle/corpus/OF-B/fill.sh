#!/usr/bin/env bash
# OF-B L4 inputs (tests/OF-B.rs): one oracle run per build, owned by this unit.
#
# Writes `.parity/unit/OF-B/<stem>/{outputs,hits}.json` and `state/offence:{1037,1692}#n.json.zst`
# for all57 (`.parity/builds/*.xml`) plus the unit corpus (`corpus/OF-B/*.xml`), or for the
# builds given as arguments. Every dump of a directory comes from the same PoB process, so
# an `offence:1037#n` / `offence:1692#n` pair never mixes two processes' mod orders (the
# shared probe cache `.parity/probe-cache/<sha>/<build>/` accumulates dumps of every run of
# every agent under one key; its `hits.json` names only the latest run's).
#
# Usage: crates/pob-oracle/corpus/OF-B/fill.sh [build.xml ...]
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../../.." && pwd)"
parity="${POB_PARITY_DIR:-}"
if [ -z "$parity" ]; then
    d="$root"
    while [ "$d" != "/" ]; do
        if [ -d "$d/.parity/builds" ]; then parity="$d/.parity"; break; fi
        d="$(dirname "$d")"
    done
fi
[ -n "$parity" ] || { echo "no .parity/builds found (set POB_PARITY_DIR)" >&2; exit 1; }

builds=("$@")
if [ ${#builds[@]} -eq 0 ]; then builds=("$parity"/builds/*.xml "$here"/*.xml); fi

# the patched source once (also builds pob_oracle); later oracle.sh calls reuse that binary
# instead of each waiting for a machine-wide cargo slot for a no-op build
src="$("$root/scripts/port/oracle.sh" patch)"
export POB_ORACLE_BIN="${POB_ORACLE_BIN:-${CARGO_TARGET_DIR:-$root/target}/release/pob_oracle}"
[ -x "$POB_ORACLE_BIN" ] || { echo "no pob_oracle binary at $POB_ORACLE_BIN" >&2; exit 1; }
out_root="$parity/unit/OF-B"
mkdir -p "$out_root"
for xml in "${builds[@]}"; do
    xml="$(cd "$(dirname "$xml")" && pwd)/$(basename "$xml")"
    stem="$(basename "$xml" .xml)"
    dir="$out_root/$stem"
    tmp="$out_root/.$stem.tmp"
    rm -rf "$tmp"
    mkdir -p "$tmp"
    if "$root/scripts/port/oracle.sh" exec probe "$xml" --out-dir "$tmp" --patched "$src" \
        --select 'offence:1037#*,offence:1692#*' --max-dumps 1000000 --no-provenance \
        2> "$tmp/log.txt"; then
        if compgen -G "$tmp/state/*.json" > /dev/null; then
            zstd -q --rm -9 "$tmp"/state/*.json
        fi
        rm -rf "$dir"
        mv "$tmp" "$dir"
        printf '%s\tok\t%s\n' "$stem" "$(find "$dir/state" -name '*.zst' | wc -l)"
    else
        printf '%s\tFAIL (see %s/log.txt)\n' "$stem" "$tmp"
    fi
done
