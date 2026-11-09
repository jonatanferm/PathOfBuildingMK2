#!/usr/bin/env bash
# (Re)build .parity-quick/: the quick corpus used by merge-time test runs in the main checkout.
# Every .parity entry is symlinked except builds/, which holds only QUICK_BUILDS: each build the
# tests name explicitly, plus coverage picks (minions, mirage/DoT, triggers, heavy auras).
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
src="$root/.parity"; dst="$root/.parity-quick"
QUICK_BUILDS="upstream_OccVortex upstream_Generals_Perforate_Zerker upstream_Dual_Wield_Cospris_CoC
upstream_Dual_Savior upstream_Mirage_Archer_Toxic_Rain allflame_exp_001 allflame_exp_007 allflame_exp_019
allflame_exp_024 allflame_exp_030 allflame_exp_031 allflame_exp_039 allflame_exp_046 allflame_exp_048"
rm -rf "$dst"; mkdir -p "$dst/builds"
for e in "$src"/* "$src"/.[!.]*; do
    [ -e "$e" ] || continue
    n=$(basename "$e"); [ "$n" = builds ] && continue
    ln -s "$e" "$dst/$n"
done
for b in $QUICK_BUILDS; do ln -s "$src/builds/$b.xml" "$dst/builds/$b.xml"; done
echo "quick corpus: $(ls "$dst/builds" | wc -l) builds in $dst"
