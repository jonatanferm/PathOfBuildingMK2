# CCR-0136: U46 CompareTab A seams (the CompareEntry handle, CompareHooks, the Widget impl, the folder popup's list host)

**Status: applied (CCRDOC, 2026-11-12; code `f571987` Merge port/U46): §1 `CompareEntryRef`/`CompareHooks` (ui doc §10.5), §2, §3's work-around (the ListControl `ov.on_sel_click`/`on_sel_key_down` slots; U18's `load_build_ov` was not added), §4 and §5.**

Unit: **U46** (`Classes/CompareTab.lua:1-1649`; files `pob-app/src/tabs/compare/{init.rs,import.rs}`).
Filed status: **pending** (code on `port/U46` follows it, with the local workarounds marked `// CCR-0136`).
Number reserved with `scripts/port/next_ccr.sh`.

No frozen contract changes. This CCR records the shapes U46 defined in its own files for U47-U50, the
two one-line hook installs in other units' files (as CCR-0098 §3 / CCR-0109 §5 ask), and one gap of
U18's BuildListControl.

## 1. `CompareEntryRef` and `CompareHooks` (U47, U48)

Evidence: the constructor's closures and the import methods read and call CompareEntry
(CompareEntry.lua, U47) fields and methods: `entry.label` (CompareTab.lua:1319), `entry.calcsTab.mainOutput`
(:1247), `entry.treeTab.specList`/`activeSpec` (:300, :826, :873, :902, :1339), `entry.skillsTab.
skillSetOrderList` (:317), `entry.itemsTab.itemSetOrderList` (:327, :800), `entry.configTab.
configSetOrderList`/`input`/`varControls` (:338, :1193, :1208, :1234), `entry.buildFlag` (:341, :375, ...),
`entry.mainSocketGroup` (:372, ...), `entry.spec` (:887, :1379), `entry.itemsTab.items`/`slots`/`sockets`
(:1404, :1415, :1445-1446), `entry.calcsTab.input` (:600, ...); and `entry:SetActiveSpec` (:301, :827, :874),
`SetActiveItemSet` (:328, :801), `SetActiveSkillSet` (:318), `SetMainSocketGroup` (:360),
`entry.configTab:SetActiveConfigSet` (:340), `entry.configTab:UpdateControls` (:1139 via makeConfigControl),
`entry.treeTab.viewer.searchStr` (:915), `entry.treeTab:OpenVersionConvertPopup` (:889);
`new("ComparePowerReportListControl")` (:1037, U48).

Applied (U46's files):

* `init::CompareEntryRef = Rc<CompareEntryHandle>` with `label: String`, `model: model::BuildRef` (the
  entry's lightweight Build: every model-half field above) and `ext: RefCell<Option<Box<dyn Any>>>` for
  U47's own state (the UI tab halves, its calc facade, `xmlText`, ...). Lua table identity is `Rc::ptr_eq`.
  `State::compareEntries: Vec<CompareEntryRef>`.
* `init::CompareHooks` (`compare_hooks()` / `set_compare_hooks(..)` / `default_compare_hooks()`): one entry
  per entry method/UI field above, each default raising "… is not ported yet (U47)"; `power_report_list`
  (adds the control under `comparePowerReportList`) defaults to `PendingPowerReportList`, a control with
  an empty `Draw` (so the tab's DrawControls visits it, as it visits the real one).
* The minion dropdowns' entries (`selected.itemSetId` / `selected.minionId`, :453-456, :565-568,
  :687-690) are read as U21's `build::init::MinionListValue`, the shape Build.lua's
  RefreshSkillSelectControls already uses; U47's `CompareEntry:RefreshSkillSelectControls` should fill
  the lists with it.

Proposed: U47 replaces the `new_compare_entry` and entry-method entries in `default_compare_hooks` (one
line each) and keeps `CompareEntryHandle`'s three fields; U48 replaces `power_report_list`.

## 2. Where the CompareTab fields and the `Widget` impl live

Ui doc §12.1 puts the class's `Widget` impl with the owner of its `Draw` (U47). BuildMode draws every
tab through `ShellHooks::draw_tab` (CCR-0104 §1), so the impl has no methods (`CAPS = empty`, as for
ItemsTab/ConfigTab, CCR-0120 §2, CCR-0110 §4); the constructor needs it, so it is in `init.rs`. The fields
the constructor sets (CompareTab.lua:133-188) are `init::State` (`cx.w::<CompareTab>(me).init`), including
the power-report fields U48 reads (`comparePowerStat`, `comparePowerCategories`, `comparePowerResults:
Option<Rc<dyn Any>>` (U48 types its entries), `comparePowerCoroutine`, ...) and the content heights U47
reads. `treeLayout` (:153) and `configSectionLayout` (:178) are written only by U47's layout functions:
U47 declares them in `layout::State` (their constructor values nil / `{}` are the defaults).

Proposed: U47 adds the `BuildTab::Compare` arm of `ShellHooks::draw_tab` (its `Draw`), and U47-U50 use
`init::State`'s fields instead of declaring copies.

## 3. BuildListControl has no `LoadBuild` instance slot (U18)

Evidence: ui doc D6 ("Classes carry override slots for their own hooks (List hooks,
`BuildListControl:LoadBuild`)"); CompareTab.lua:1607-1613 overrides `controls.buildList:LoadBuild`, which
BuildListControl.lua:261 (`OnSelClick` on a double click) and :283 (`OnSelKeyDown`) call as
`self:LoadBuild(build)`. U18's `BuildListControl::load_build` is an inherent function that its
`on_sel_click`/`on_sel_key_down` hooks call directly (`build_list/build_list_control.rs:267`, `:752-771`,
`:801-811`).

Workaround (U46's `import.rs`, marked CCR-0136 §3): the popup sets the ListControl instance slots
`ov.on_sel_click` (BuildListControl.lua:259-265 calling the popup's `LoadBuild`) and `ov.on_sel_key_down`
(CompareTab.lua:1614-1618). Proposed (U18's file): a `pub load_build_ov: Option<Rc<dyn Fn(&mut Cx, CtrlId,
&Entry)>>` field on `BuildListControl` that `load_build` calls when set; U46 then sets it and drops its
`on_sel_click` override.

The popup's `listHost` (L1540-1565) is a `ListMode` node of its own (U18's `BuildListControl` takes a
`Ctrl<ListMode>`): its `BuildList`/`FilterBuildList`/`SortList` are BuildList.lua's, which do what the
popup's local functions do (the module notes of `import.rs` give the argument). No change proposed.

## 4. The one-line installs in other units' files

* `build/init.rs` `default_hooks`: `compare_tab: Rc::new(crate::tabs::compare::init::ctor_hook)`
  (CCR-0098 §3). `pending_tab` and `not_ported` there are now unused and carry `#[allow(dead_code)]`.
* `tabs/import/core.rs` `default_import_hooks`: `compare_import_build:
  Rc::new(crate::tabs::compare::init::import_build_hook)` (CCR-0109 §5). `not_ported` there is now unused
  and carries `#[allow(dead_code)]`.

## 5. Notes for U47

* The window titles: CompareEntry's tab constructors send `main:SetWindowTitleSubtext` with the entry's
  build name (TreeTab:SetActiveSpec, PassiveSpec:RestoreUndoState); ImportBuild then restores the primary
  build's (CompareTab.lua:1252-1254). `build::init`'s title queue formats with the open build's
  `buildName`; the entry constructor must flush its model's queued titles with the entry's label.
  `tests/u46_compare.rs` drops those titles on both sides and compares the rest.
* `tests/u46_compare.rs` has a test-only stand-in for the top-bar half of `Draw` (both sides) and for
  CompareEntry on the Rust side (`Init_model` + a LuaCalcBridge BuildOutput); U47 can check its real ports
  against the same scripts by removing the stand-ins.
