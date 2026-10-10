# `fixtures/user1` (ui doc §11.3; owner U8)

The user dir of the app scenarios, laid out for U7's `fixture userdir=DIR` rule (CCR-0026,
CCR-0028 item 10): `GetUserPath()` is DIR and PoB reads `DIR/Path of Building/` (Main.lua:94-98).
Scenarios name it relative to their own directory (`fixture userdir=../../fixtures/user1` from
`scenarios/<unit>/`). The oracle copies this directory to a fresh run dir before each side of a
run (`runner::UserDir`); runs never modify these files. `GetFileModifiedTime` reports 0 for them
(U7's headless rule; files written during a run report the virtual write time).

* `Path of Building/Settings.xml`: starts in the build list (`<Mode mode="LIST">` with
  `GetArgs()`'s two arguments: no selected build, root sub-path), build sort by name, the
  `main:Init` defaults otherwise, `showPublicBuilds="false"`.
* `Path of Building/Builds/`: 6 builds of the parity corpus (`.parity/builds`, see
  `scripts/parity`) whose oracle `TotalDPS` is > 0 (`.parity/oracle/<stem>/outputs.json`):

| file | TotalDPS (oracle) |
|---|---|
| allflame_exp_001.xml | 2 145 267.9 |
| allflame_exp_007.xml | 1 912 377.8 |
| allflame_exp_030.xml | 4 932 858.2 |
| allflame_exp_048.xml | 19 564 115.2 |
| upstream_Dual_Wield_Cospris_CoC.xml | 16 493 623.8 |
| upstream_Generals_Perforate_Zerker.xml | 22 360 547.4 |

Scenarios open one with `load-build "Builds/<file>"` (relative to `Path of Building/`).
