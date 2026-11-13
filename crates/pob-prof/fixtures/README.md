# `fixtures/user` (PROF1)

The user directory of the profiling scenarios (`fixture userdir=../fixtures/user`): `GetUserPath()`
is this directory and the app reads `Path of Building/` below it.

- `Path of Building/Settings.xml`: opens in the build list (`<Mode mode="LIST">`), sorted by name,
  `showPublicBuilds="false"`; the same settings as pob-ui-oracle's `fixtures/user1`.
- `Path of Building/Builds/`: 7 builds copied from the parity corpus (`.parity/builds`), chosen
  for their archetypes (docs/perf/PROFILING.md "Inputs"): allflame_exp_001 (spell,
  cluster-heavy), allflame_exp_005 (many items, two item sets), allflame_exp_016 (DoT),
  allflame_exp_032 (minion), allflame_exp_048 (small attack build),
  upstream_Dual_Wield_Cospris_CoC (CoC trigger, 3.13 tree), upstream_Generals_Perforate_Zerker
  (attack, 3.13 tree).

Runs never modify these files: writes go to pob-host's in-memory `HeadlessFs` overlay. The
scenarios' coordinates depend on the builds' saved views (tree zoom and position) and contents;
change a build here only together with the scenarios that use it.
