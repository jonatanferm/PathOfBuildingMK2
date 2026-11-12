#!/usr/bin/env bash
# Full test suite of the port crates on the current rust-port head, full corpus, through cargo.sh
# (memory-capped, slot-limited). Runs from its own worktree so it never touches the main checkout.
# Results: .parity/fullsuite/<stamp>/{summary.txt,<crate>.log} and .parity/fullsuite/latest -> <stamp>.
# Scheduled every few hours by a systemd user timer:
#   systemd-run --user --unit=pob-port-fullsuite --on-active=5min --on-unit-inactive=3h <abs path>
#
# Crates run concurrently in up to $POB_FULLSUITE_LANES lanes (default 2; 1 = one crate at a time as
# before). Each lane's cargo.sh holds one machine-wide run slot for its whole run phase, so the default
# leaves one of the 3 default run slots to other agents' `cargo.sh test` during the long heavy crates. The heavy crates ($POB_FULLSUITE_HEAVY, default pob-oracle and pob-ui-oracle) start
# first, each in its own lane, and the other crates share the remaining lane(s) in list order. Within
# a crate, cargo.sh runs the test binaries in parallel (testrun.py, POB_PORT_TEST_JOBS). Memory stays
# bounded machine-wide however many lanes run: compiles take one of the cargo.sh compile slots, and
# every running test binary holds one of the machine-wide run slots, in its own MemoryMax scope.
#
# summary.txt: the header line, one "<crate>: rc=<rc> <secs>s; <p> passed, <f> failed, <i> ignored[;
# FAILED: <tests>]" line per crate (appended as each crate finishes; rewritten in $crates order at the
# end), then "per-binary times" (one "  <wall>s <rss>MB rc=<rc> <crate> <target>" line per test binary,
# slowest first (ties in $crates order), from cargo.sh's "cargo.sh: bin-time" lines), then the "finished" line.
set -uo pipefail
main="$(cd "$(dirname "$0")/../.." && pwd)"
wt="${POB_FULLSUITE_WT:-$(dirname "$main")/pob-fullsuite}"
crates="${POB_FULLSUITE_CRATES:-pob-lua pob-gamedata pob-modparser pob-calc pob-calc-core pob-calc-setup pob-calc-config pob-calc-defence pob-calc-offence pob-calc-perform pob-dump pob-oracle pob-sg pob-sg-wgpu pob-host pob-ui-core pob-ui-oracle pob-app}"
heavy="${POB_FULLSUITE_HEAVY:-pob-oracle pob-ui-oracle}"
lanes="${POB_FULLSUITE_LANES:-2}"
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

# run_crate C: test crate C into $out/C.log and write its summary line to $out/.line.C.
run_crate() {
    local c="$1" t0 rc pass fails line
    t0=$(date +%s)
    POB_FULL_SUITE=1 POB_PARITY_DIR="$main/.parity" timeout "${POB_FULLSUITE_TIMEOUT:-14400}" scripts/port/cargo.sh test -p "$c" --no-fail-fast > "$out/$c.log" 2>&1
    rc=$?
    pass=$(grep -oE 'test result: [a-zA-Z]+\. [0-9]+ passed; [0-9]+ failed; [0-9]+ ignored' "$out/$c.log" | awk '{p+=$4; f+=$6; i+=$8} END {printf "%d passed, %d failed, %d ignored", p, f, i}')
    fails=$(grep -E '^test .* FAILED$|^    [A-Za-z0-9_:]+$' "$out/$c.log" | grep -oE '^test [^ ]+' | sed 's/^test //' | sort -u | tr '\n' ' ')
    line="$c: rc=$rc $(( $(date +%s) - t0 ))s; $pass${fails:+; FAILED: $fails}"
    echo "$line" > "$out/.line.$c"
    echo "$line" >> "$out/summary.txt"     # progress, in finishing order
}

# The queue: heavy crates first, then the rest in list order.
queue=()
for c in $heavy; do case " $crates " in *" $c "*) queue+=("$c") ;; esac; done
for c in $crates; do case " $heavy " in *" $c "*) ;; *) queue+=("$c") ;; esac; done
running=0
for c in "${queue[@]}"; do
    [ -d "crates/$c" ] || continue
    while [ "$running" -ge "$lanes" ]; do wait -n; running=$((running - 1)); done
    run_crate "$c" &
    running=$((running + 1))
done
wait

# Final summary.txt, in $crates order (written next to it, then renamed over it).
{
    head -1 "$out/summary.txt"
    for c in $crates; do [ -f "$out/.line.$c" ] && cat "$out/.line.$c"; done
    echo "per-binary times (wall, peak RSS, exit status), slowest first:"
    for c in $crates; do
        [ -f "$out/$c.log" ] || continue
        sed -nE 's/^cargo\.sh: bin-time (-p [^ ]+ )?(.*) ([0-9.]+)s rss=([0-9]+)MB rc=(-?[0-9]+|None)$/\3 \4 \5 '"$c"' \2/p' "$out/$c.log"
    done | sort -s -k1,1gr | awk '{printf "  %.1fs %dMB rc=%s %s", $1, $2, $3, $4; for (i = 5; i <= NF; i++) printf " %s", $i; printf "\n"}'
    echo "finished $(date '+%F %T')"
} > "$out/summary.txt.new"
mv -f "$out/summary.txt.new" "$out/summary.txt"
rm -f "$out"/.line.*
ln -sfn "$out" "$main/.parity/fullsuite/latest"
