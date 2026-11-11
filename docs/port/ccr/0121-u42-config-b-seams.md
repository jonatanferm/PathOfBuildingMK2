# CCR-0121: U42 ConfigTab B seams (provenance of the split lines, reset-undo, hook wiring)

Unit: **U42** (ConfigTab B: `ConfigTab.lua:835-1362`, ConfigSetListControl, ConfigModBrowser,
ConfigVisibility). Status: **pending** (code on `port/U42` follows it). Number reserved with
`scripts/port/next_ccr.sh`.

## 1. Lines of U42's range already covered by calc markers

Evidence: `docs/port/scope.toml` (Classes/ConfigTab.lua `839-1129`, `1130-1204`, `1205-1240`,
`1280-1300`, `1337-1362`) and `pob-calc/src/model/config_tab.rs` (`@pob` 1008-1022, 1302-1334,
1337-1362; `@pob-skip Classes/ConfigTab.lua:1024-1116 reason="Draw: UI layout …(ui doc U42)"`).

U42's UI ports of `UpdateControls` (1008-1022), `Draw` (1024-1116) and `SetActiveConfigSet`
(1337-1362, the UI tail) live in `pob-app/src/tabs/config/core.rs` with `/// lua:` notes and **no**
`@pob` markers: a second marker would cover those lines twice (`provenance.py coverage`), as
CCR-0110 §5 already decided for ConfigOptions.lua 11-115. U42's `@pob` markers are only for lines
outside the calc scope: ConfigTab.lua 1242-1248, 1250-1268, 1270-1277, and the three module files.

Proposed: calc:BM-B's `@pob-skip` reason for 1024-1116 names `pob-app/src/tabs/config/core.rs`
(`ConfigTab::draw`) as the port.

Found while checking: `provenance.py coverage --range Classes/ConfigTab.lua:1-1362` reports
1302-1315, 1317-1323, 1325-1334 **covered twice** on `rust-port` before this unit: U41's
`/// @pob Classes/ConfigTab.lua:1302-1334` (ctor.rs, `update_custom_mods_controls`) and BM-B's
marker on the model half (config_tab.rs:810). Proposed: U41's marker becomes a `/// lua:` note
(the same rule as above). U42 does not edit it (U41's file beyond the hook lines, §3).

## 2. DIVERGENCE(reset-undo)

`ConfigTab:Load` ends with `self:ResetUndo()` (L938, UI). The model's Load runs inside
`Init_model` before any UI constructor, so `tabs::config::core::ctor_hook` (now
`BuildHooks::config_tab`) runs U41's constructor and then `ResetUndo` when the build file has a
`Config` section, exactly as U39 does for CalcsTab (CCR-0111). Nothing between PoB's Load and
that point writes the active set's `input`/`customModsList` (Build.lua:685's
`ImportCalcSettings` runs only for an empty `input`).

## 3. Hook wiring (one line each, the CCR-0098 §3 / CCR-0104 §1 / CCR-0110 §1 pattern)

* U41's `ctor.rs::default_config_hooks`: the five U42 entries now name U42's ports
  (`ConfigTab::update_controls`, `ConfigTab::add_undo_state`,
  `ConfigTab::open_config_set_manage_popup`, `mod_browser::open_add_mod_popup_hook`,
  `visibility::isShowAllExcluded`); the `not_ported` helper they used is removed.
* U19's `build/init.rs::default_hooks`: `config_set_active_config_set` =
  `core::set_active_config_set_hook` (the whole SetActiveConfigSet: model half with `init`, then
  UpdateControls with its UI half, BuildModList, the bridge pull, SyncLoadouts — L1361 was
  missing before); `config_tab` = `core::ctor_hook` (§2).
* U20's `build/on_frame.rs::default_shell_hooks`: the `BuildTab::Config` draw arm and the
  `TabFlag::Config` read/write arms (`configTab.modFlag`, the UndoHandler flag).

## 4. Reuse of U32's id-list adapter

`ConfigSetListControl`'s list is `configTab.configSetOrderList` (`Shared<Vec<u32>>`, like
`itemsTab.itemSetOrderList`), so it uses U32's public `tabs::items::item_list::{IdListSource,
id_value, value_id}` instead of a second copy.

## 5. ConfigVisibility keys absent from the data schema

`ifEnemyStat` (a SIMPLE_PREDICATES entry), `ifNode`, `implyMinionCond`, `implyEnemyCond` have no
field in `ConfigOptionDef` (no option of the data at 16de4b82d sets them; U41's `VarExtras`
note). visibility.rs ports their branches against nil constants. If an upstream bump adds one,
the schema (P1) needs the field and both U41's and U42's constants become field reads.

## 6. ConfigModBrowser's `pairs` order (ui doc §8 rule 6 (a))

Evidence: ConfigModBrowser.lua 84, 158, 194, 196, 203, 208 fill `modSet` with `pairs` over
string-keyed tables (and `tree.nodes`, which held the key `root`): the first line met for a
template becomes its displayed `text` ("4%" or "5% additional Physical Damage Reduction while
moving"; observed between the oracle's Lua and Rust sides in the u42 mod-browser scenario before
this fix) and the first mod of a `group` is the one recorded. L246 lists a row's `sources` in
`pairs` order. PoB's own order is per-process random.

Applied: mod_browser.rs walks these tables in a canonical order (keys ascending: node ids and
trade hashes numerically, names bytewise); `tests/u42_config.rs` reloads
`Modules/ConfigModBrowser.lua` on the Lua side with exactly those seven `pairs(` replaced by a
sorted iterator (`__u42SortedPairs`, line count unchanged) and installs its `OpenAddModPopup`
into the module table ConfigTab.lua:14 `require`d. // CCR-0121

Proposed: U8 registers the same substitutions in `pob-ui-oracle/src/patches.rs` (a text patch on
`Modules/ConfigModBrowser.lua` plus `__u42SortedPairs` renamed `__sortedPairs` in
`lua/markers.lua` or `determinism.lua`), so every harness that opens the mod browser (U46's
compare view, the end-to-end runs) sees the canonical order; the test-local copy is then
removed.
