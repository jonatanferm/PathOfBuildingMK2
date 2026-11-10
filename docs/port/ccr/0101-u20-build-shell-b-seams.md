# CCR-0101: U20 build shell B seams (ShellHooks, stat-line fields, main.tree peek, T4 notes)

Unit: **U20** (Build shell B: `Build.lua:771-1617`, `2222-2335`). Status: **pending** (code on
`port/U20` follows it). Numbered 0101, the next free number on rust-port when U20 started; an
in-flight unit that merges first with the same number keeps it and this file takes the next.

## 1. `on_frame::ShellHooks`: the calls U20's lines make into units not ported yet

Evidence: Build.lua:1182 (`importTab:SelectControl(importTab.controls.importCodeIn)`, U43),
1213-1215 / 1228-1246 (`calcsTab.controls.breakdown`, `calcsTab.displayPinned/displayData`,
U39/U40), 1267-1269 / 1348 / 1354-1388 (the sidebar `controls.breakdown`, a
CalcBreakdownControl: `pinned`, `SetBreakdownData`, `borderColour`, U40), 1291 and 1371
(`RefreshSkillSelectControls`, `GetSidebarBreakdown`, U21), 1300-1318 (each tab's `Draw`,
U22-U46), 1327 / 1101-1110 (the tabs' `modFlag`, `treeTab.searchFlag`, `spec.modFlag` of the
spec's UndoHandler, U25), 1536-1537 (MinionListControl / MinionSearchListControl, U31),
1590-1599 (ExtBuildListControl, U52; `Deflate`/`common.base64` of the build code, U43). None has
a frozen Rust signature; their files are other units'.

Applied (U20's files): `pob-app/src/build/on_frame.rs` declares `pub struct ShellHooks` (one
entry per call, each documented with its owner), `default_shell_hooks()`, `shell_hooks()` /
`set_shell_hooks(..)`, the enums `BuildTab` / `TabFlag`, `CalcsBreakdown`, `BreakdownInputs`, and
the `OverlayPane` trait for `self.overlayPanes` (CalcSectionControl's ToggleOverlay /
RaiseOverlay push and reorder it through `push_overlay_pane` / `overlay_panes_mut`). Defaults:
what OnFrame calls every frame reads "no tab / nothing pinned / not modified" and draws nothing;
`GetSidebarBreakdown`, the minion lists and the similar-builds list raise
`lua_error!("… is not ported yet (Uxx)")`. Proposed: each owner replaces its entry in
`on_frame::default_shell_hooks` (one line each), as with CCR-0098 §3.

Also in `ShellHooks`: `save_ui_section(cx, elem) -> Option<XmlNode>` for the UI-only savers
(`Notes`, `TreeView`, `Import`; Build.lua:629-639). `io::save_db` replaces BM-A's passthrough
section (CCR-0080 §8) with the hook's node; the default keeps BM-A's. The load side is U19's
`BuildHooks::load_ui_section`.

## 2. The stat-box line fields SetDisplayStat reads (U21)

Evidence: Build.lua:1361-1378 read `hovered.line.breakdown`, `.modNames`, `.ignoredSections`,
`.actorName`, `.breakdownColour`; the lines are built by U21's `AddDisplayStatList` and live in
`TextListControl.list` as `TextListRow` (U9), whose caller-defined keys go in
`TextListRow::extra: Option<Rc<dyn Any>>`.

Applied: `on_frame::StatLineInfo { breakdown, mod_names, ignored_sections, actor_name,
breakdown_colour }`; SetDisplayStat downcasts `line.extra` to it (a line without one reads as
all-nil). Proposed: U21 stores a `StatLineInfo` in each stat line's `extra` and implements
`ShellHooks::get_sidebar_breakdown(cx, &StatLineInfo) -> Table` (the `{ { breakdown = key },
… }` list; SetDisplayStat sets its `x`, `y`, `width` and compares it with `tableDeepEquals`,
ported here from Common.lua:625-640).

## 3. U19's `BuildHooks` entries U20 installed

`init::default_hooks` now names U20's functions for `on_frame`, `shutdown`, `can_exit`,
`close_build`, `open_save_popup`, `save_db_file`, `open_save_as_popup`,
`open_conversion_popup`, `sync_loadouts`, `update_class_dropdowns`, `open_spectre_library`,
`set_display_stat` and `clear_display_stat` (one line each). The last two are U20's
(Build.lua:1353-1391 is in U20's range; the `BuildHooks` doc comment attributed them to U21).
`refresh_stat_list` stays U21's placeholder. `init::pending_on_frame` is no longer the default;
`tests/u19_build.rs` keeps using it and now also stubs `sync_loadouts` (its Lua side stubs
SyncLoadouts).

## 4. Missing model re-exports / accessors

* `wipeGlobalCache` (Build.lua:1260) lives in `pob_calc::modules::common`, which `model.rs`
  (U15) does not re-export. Work-around: `on_frame.rs` calls
  `pob_calc::modules::common::wipeGlobalCache()` directly (marked CCR-0101 §4). Proposed:
  `pub use pob_calc::modules::common::wipeGlobalCache;` in `pob-app/src/model.rs`.
* `main.tree[treeVersion]` (Build.lua:1554) indexes the cache without loading:
  `pob_calc::classes::passive_spec::main_LoadTree` also runs `setJewelRadiiGlobally`
  (Main.lua:354), which would switch the jewel radii to the latest tree on Init's
  `UpdateClassDropdowns()` (Build.lua:646) for an older-tree build. Work-around
  (`class_drop::main_tree`): `build.latestTree` for the latest version, else the tree of a spec
  of the build with that version, else `main_LoadTree` (marked CCR-0101 §4). Proposed
  (calc:PS-A, `passive_spec.rs`):

```diff
+/// `main.tree[treeVersion]` (no load, no jewel radii).
+pub fn main_tree_get(treeVersion: Name) -> Option<PassiveTreeRef> {
+    MAIN_TREE.with(|m| m.borrow().get(&treeVersion).cloned())
+}
```

## 5. `main.modes.BUILD` registered (CCR-0098 §5)

Applied: `main_mode/init.rs` `default_modes` installs `crate::build::mode()`. The acceptance
tests that stub BUILD on their Lua side install a Rust `PendingMode` for it after `OnInit`:
`tests/u16_main.rs` and `tests/u18_build_list.rs` (one insert each, as they already do for
LIST); `u17_popups.rs` and `u19_build.rs` install their own modes already. `app.rs`'s
`PobApp::snapshot` (U16, "CCR-0096: the build's SaveDB is U20's") now returns
`build::io::snapshot`.

## 6. `self.dbFileName` has two homes

Evidence: U19's `BuildState::db_file_name` (UI) and calc:BM-A's side table
(`build_xml::dbFileName`, read by its `SaveDBFile`/`LoadDBFile`). Applied:
`io::set_db_file_name` writes both (Save As, Build.lua:1489; Shutdown's `~~temp~~`, :956).
Proposed (CCR-0080 §2 follow-up): one home, the model's, with `BuildState` reading it.

## 7. `SaveDBFile` (Build.lua:2304-2333)

calc:BM-A's `Build::SaveDBFile` carries the `@pob` marker and ports the model statements with
`std::fs` and stderr errors, leaving the UI statements as comments. The UI needs them in place
(the Save As popup, the error popup, `actionOnSave`, ResetModFlags, CloseBuild / Exit /
ApplyUpdate) and writes through `cx.sys.fs` (wasm storage). Applied: `io::save_db_file` is the UI
function; it calls the model's `SaveDB` and carries a `lua:` comment, not a second `@pob` marker
(the range is BM-A's in `scope.toml`). Proposed: none; recorded so a reviewer does not count
the range twice.

## 8. T4 findings outside U20 (the acceptance test sets them aside, with counts)

* **A8 item 15** (CCR-0019 §4-7): the frozen `GemInstance.enableGlobal1/2` and
  `SocketGroup.includeInFullDPS` are `Cell<bool>`, `SocketGroup.mainActiveSkill(Calcs)`
  `Cell<f64>`. The `EnemyExplode` gem the calc adds for explode sources and item-granted groups
  have them nil in Lua, so PoB saves `"nil"`, Rust `"false"` / `"0"` (31 attributes over the six
  fixtures).
* **UI savers** (§1): BM-A writes the loaded `Import` / `TreeView` section back, or an empty
  element for a build without one; ImportTab (U43) adds `exportParty="false"`, PassiveTreeView
  (U25-U28) writes its viewer state. `tests/u20_build.rs` reports `Import` and `TreeView` as
  differing in the fixture run and the new-build run (`Notes` matched).
* **PoB nondeterminism**: an old-format gem (no `variantId`) takes the first match of
  `pairs(possibleVariants)` (SkillsTab.lua:472-478; string keys) and Vaal Righteous Fire shares
  its granted effect with its AltX variant, so PoB's own `variantId`/`nameSpec` differ between
  runs (Cospris fixture). `pairs(spec.jewels)` / `pairs(hashOverrides)` (PassiveSpec.lua:226/239)
  write `Socket`/`Override` children in per-process order (calc doc §3.4).
* **Display stats** (U21, `build_xml::set_save_stats_hook`): no `PlayerStat` / `FullDPSSkill` /
  `MinionStat` until U21 registers the writer.
* `calc:PWR`'s `power::BuildOutput` is still `unported!` on rust-port, so `calcsTab.mainOutput`
  stays nil with the Rust calc (EstimatePlayerProgress then shows `ExtraPoints` 0). U20's oracle
  test runs the Rust side on U15's LuaCalcBridge (ui doc §12.1 `ext:calc`).
