#!/usr/bin/env bash
# Fill the PF-D L4 dumps (tests/PF-D.rs): every given build (default: .parity/builds/*.xml and
# crates/pob-oracle/corpus/PF-D/*.xml) probed with corpus/PF-D/stages.toml into the probe cache.
# Builds that already have pfd:* dumps for the current patched source are probed again.
set -u
root="$(cd "$(dirname "$0")/../../../.." && pwd)"
cd "$root" || exit 1
parity="${POB_PARITY_DIR:-}"
if [ -z "$parity" ]; then
    d="$root"
    while [ "$d" != "/" ]; do
        if [ -d "$d/.parity/builds" ]; then parity="$d/.parity"; break; fi
        d="$(dirname "$d")"
    done
fi
builds=("$@")
if [ ${#builds[@]} -eq 0 ]; then
    builds=("$parity"/builds/*.xml crates/pob-oracle/corpus/PF-D/*.xml)
fi
for x in "${builds[@]}"; do
    [ -f "$x" ] || continue
    echo "== $x"
    scripts/port/oracle.sh run --build "$x" --stages crates/pob-oracle/corpus/PF-D/stages.toml \
        --stage 'perform:1943@main,perform:2308@main,pfd:*@main,getCachedOutputValue:*@main' 2>&1 | tail -1
done
echo ALLDONE
