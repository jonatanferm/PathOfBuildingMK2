#!/usr/bin/env bash
# Full test suite of the port crates on the current rust-port head, full corpus, one crate at a
# time through cargo.sh (memory-capped, slot-limited). Runs from its own worktree so it never
# touches the main checkout. Results: .parity/fullsuite/<stamp>/{summary.txt,<crate>.log} and
# .parity/fullsuite/latest -> <stamp>. Scheduled every few hours by a systemd user timer:
#   systemd-run --user --unit=pob-port-fullsuite --on-active=5min --on-unit-inactive=3h <abs path>
set -uo pipefail
main="$(cd "$(dirname "$0")/../.." && pwd)"
wt="${POB_FULLSUITE_WT:-$(dirname "$main")/pob-fullsuite}"
crates="${POB_FULLSUITE_CRATES:-pob-lua pob-gamedata pob-modparser pob-calc pob-dump pob-oracle pob-sg pob-sg-wgpu pob-host pob-ui-core pob-ui-oracle pob-app}"
if [ ! -d "$wt" ]; then git -C "$main" worktree add --detach "$wt" rust-port >/dev/null; fi
git -C "$wt" checkout -q --detach -f rust-port
head=$(git -C "$wt" rev-parse --short HEAD)
stamp=$(date +%Y%m%d-%H%M)
out="$main/.parity/fullsuite/$stamp"; mkdir -p "$out"
{ echo "full suite at rust-port $head, started $(date '+%F %T')"; } > "$out/summary.txt"
cd "$wt"
# Gitignored inputs live in the main checkout: tests that look in ancestor dirs or $POB_ROOT
# (fonts, upstream PoB for the UI patches) failed in the first run without these.
[ -e "$wt/.PathOfBuilding" ] || ln -s "$main/.PathOfBuilding" "$wt/.PathOfBuilding"
export POB_ROOT="$main/.PathOfBuilding" POB_SRC="$main/.PathOfBuilding/src" POB_DATA_DIR="$main/data"
export POB_ORACLE_REQUIRE=1
for c in $crates; do
    [ -d "crates/$c" ] || continue
    t0=$(date +%s)
    POB_FULL_SUITE=1 POB_PARITY_DIR="$main/.parity" timeout 7200 scripts/port/cargo.sh test -p "$c" --no-fail-fast > "$out/$c.log" 2>&1
    rc=$?
    pass=$(grep -oE 'test result: [a-zA-Z]+\. [0-9]+ passed; [0-9]+ failed; [0-9]+ ignored' "$out/$c.log" | awk '{p+=$4; f+=$6; i+=$8} END {printf "%d passed, %d failed, %d ignored", p, f, i}')
    fails=$(grep -E '^test .* FAILED$|^    [A-Za-z0-9_:]+$' "$out/$c.log" | grep -oE '^test [^ ]+' | sed 's/^test //' | sort -u | tr '\n' ' ')
    echo "$c: rc=$rc $(( $(date +%s) - t0 ))s; $pass${fails:+; FAILED: $fails}" >> "$out/summary.txt"
done
echo "finished $(date '+%F %T')" >> "$out/summary.txt"
ln -sfn "$out" "$main/.parity/fullsuite/latest"
