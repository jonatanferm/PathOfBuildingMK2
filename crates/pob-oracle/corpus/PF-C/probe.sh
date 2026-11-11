#!/usr/bin/env bash
# Probe builds for the PF-C acceptance test (tests/PF-C.rs): the segment's line probes
# `perform:1289`/`perform:1943` of the main env, the closure-upvalue probes `perform:1691` /
# `perform:1862` (CCR-0079 §1) and every call of the replayed callees.
#
# Usage: crates/pob-oracle/corpus/PF-C/probe.sh [build.xml ...]   (default: .parity/builds/*.xml)
# Output: target/tmp/pfc-probe/<stem>/ (hits.json, state/); the test reads it from there.
set -euo pipefail
root="$(cd "$(dirname "$0")/../../../.." && pwd)"
parity="${POB_PARITY_DIR:-}"
if [ -z "$parity" ]; then
    d="$root"
    while [ "$d" != "/" ]; do
        if [ -d "$d/.parity/builds" ]; then parity="$d/.parity"; break; fi
        d="$(dirname "$d")"
    done
fi
export POB_PARITY_DIR="$parity"
stages="$root/crates/pob-oracle/corpus/PF-C/stages.toml"
bin="${POB_ORACLE_BIN:-$root/target/release/pob_oracle}"
export POB_ORACLE_BIN="$bin"
src="$("$root/scripts/port/oracle.sh" patch --stages "$stages" 2>/dev/null)"
out_root="${PFC_PROBE_DIR:-$root/target/tmp/pfc-probe}"
builds=("$@")
if [ ${#builds[@]} -eq 0 ]; then builds=("$parity"/builds/*.xml); fi
sel='perform:1289@main,perform:1943@main,perform:1691@main,perform:1862@main,pfc:calcFlaskMods*,mergeKeystones:*,createMinionSkills:*,initMinionModDB:*,applyEnemyModifiers:*'
for xml in "${builds[@]}"; do
    stem="$(basename "$xml" .xml)"
    dir="$out_root/$stem"
    rm -rf "$dir"
    mkdir -p "$dir"
    if "$root/scripts/port/oracle.sh" exec probe "$xml" --out-dir "$dir" --patched "$src" \
        --select "$sel" --no-provenance --max-dumps 2000 2> "$dir/log.txt" > /dev/null; then
        # keep the main env's dumps only (the callee globs dump every env's calls)
        python3 -I - "$dir" <<'EOF'
import json, os, sys
d = sys.argv[1]
h = json.load(open(os.path.join(d, "hits.json")))
keep = {x["file"].split("/")[-1] for x in h["dumps"] if x["env"] == h["mainEnv"]}
for f in os.listdir(os.path.join(d, "state")):
    if f not in keep:
        os.remove(os.path.join(d, "state", f))
EOF
        printf '%s\tok\n' "$stem"
    else
        printf '%s\tFAIL\n' "$stem"
    fi
done
