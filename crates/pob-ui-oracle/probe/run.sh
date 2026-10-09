#!/usr/bin/env bash
# Run the U2 font probe (Launch.lua) with the real SimpleGraphic.dll under Wine and copy its output
# to expected/ (ui doc §11.1 T6). See README.md.
#
# Usage: crates/pob-ui-oracle/probe/run.sh [PATH_OF_BUILDING_CHECKOUT]
#   default checkout: <repo>/.PathOfBuilding (its runtime/ holds the exe, SimpleGraphic.dll, fonts)
set -euo pipefail

probe=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
repo=$(cd "$probe/../../.." && pwd -P)
pob=${1:-$repo/.PathOfBuilding}
command -v wine >/dev/null || { echo "run.sh: wine is not installed (pacman -S wine)" >&2; exit 2; }
[ -f "$pob/runtime/Path of Building.exe" ] || { echo "run.sh: no runtime in $pob" >&2; exit 2; }

run=$(mktemp -d "${TMPDIR:-/tmp}/pob-font-probe.XXXXXX")
trap 'echo "run dir kept: $run"' EXIT
# The launcher looks for Launch.lua next to the exe (its first line names SimpleGraphic.dll);
# SimpleGraphic finds its fonts in SimpleGraphic/Fonts below the exe directory.
cp -r "$pob/runtime/." "$run/"
cp "$probe/Launch.lua" "$probe/cases.lua" "$run/"

(cd "$run" && timeout 300 wine "Path of Building.exe") || echo "run.sh: wine exited with $?" >&2

mkdir -p "$probe/expected"
cp "$run/out/metrics.json" "$probe/expected/metrics.json"
cp "$run/out/scene.json" "$probe/expected/scene.json"
shot=$(ls -t "$run"/Screenshots/*.png 2>/dev/null | head -n 1 || true)
if [ -n "$shot" ]; then
    cp "$shot" "$probe/expected/scene.png"
else
    echo "run.sh: no screenshot found in $run/Screenshots" >&2
fi
echo "probe output copied to $probe/expected; now run: scripts/port/cargo.sh test -p pob-sg --test font_probe"
