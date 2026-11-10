# CCR-0098: U19 build shell seams (BuildState, Init_model, hooks, mode registration)

Unit: **U19** (Build shell A: `Build.lua:1-770`). Status: **pending** (code on `port/U19`
follows it). Numbered 0098 to stay clear of the three pending 0091s (CA, PF-B, U16), which take
0095-0097 when they merge (AMENDMENTS A8).

## 1. `BuildState` (frozen in ui doc §10.1; owner U19)

Evidence: Lua has one `self.buildFlag`; the model halves of the calc units write it on the
model (`pob-calc/src/model/tree_tab.rs:189`, `config_tab.rs:918`, `skills_tab.rs:295/312/1351`,
`items_tab.rs:1287/1360`, `config_options/apply_b.rs:179`, `build_xml.rs:454`), and
`Launch.lua:115` reads it (`Mode::build_flag`). A second UI copy would drift.

Applied in `pob-app/src/build/mod.rs`:

```diff
 pub struct BuildState {
     ...
-    pub build_flag: bool, pub mod_flag: bool,
-    pub db_file_name: Option<String>, pub db_file_sub_path: Option<String>, pub build_name: String,
+    pub mod_flag: bool,
+    pub db_file_name: Option<String>,
+    pub db_file_name_is_false: bool,   // GetArgs / Settings.xml tell `false` from nil (Main.lua:744 ipairs)
+    pub db_file_sub_path: Option<String>,
+    pub build_name: String,
+    pub import_link: Option<String>,
+    pub save_as_sort_mode: String,
+    pub abort_save: Option<bool>,
+    pub unsaved: bool,
 }
+impl Default for TabIds { /* every tab Ctrl::null() */ }
```

`self.buildFlag` is `model.buildFlag` everywhere in the UI. `self.modFlag` stays UI-side
(`BuildState.mod_flag`); Init copies BM-A's load-time value (`build_extra(..).modFlag`,
Build.lua:94/100). ui doc §10.1's `BuildState` listing should follow.

## 2. `build_xml::Init_model` ignores `main.defaultCharLevel` (calc:BM-A)

Evidence: `pob-calc/src/model/build_xml.rs:277-288` hard-codes Build.lua:83/88 for a nil
`main.defaultCharLevel` (`characterLevel = 1`, `characterLevelAutoMode = true`). PoB reads the
user's Options value there; it matters for builds without a `Build` section (new builds).

Proposed:

```diff
-pub fn Init_model(build, dbFileName, buildName, buildXML, convertBuild, importLink, buildOutput) -> bool
+pub fn Init_model(build, dbFileName, buildName, buildXML, convertBuild, importLink, buildOutput,
+                  defaultCharLevel: Option<f64>) -> bool
     // L83
-    build.characterLevel.set(m_min(m_max(1.0, 1.0), 100.0));
+    build.characterLevel.set(m_min(m_max(defaultCharLevel.unwrap_or(1.0), 1.0), 100.0));
     // L88
-    extra.set(n!("characterLevelAutoMode"), true);
+    extra.set(n!("characterLevelAutoMode"), defaultCharLevel == Some(1.0) || defaultCharLevel.is_none());
```

Work-around (`build/init.rs`, `DIVERGENCE(default-char-level)`): with a non-nil
`main.defaultCharLevel` and no `Build` section, Init sets both afterwards and runs
`configTab:BuildModList()` once more (it only derives the enemy level from them).

## 3. `BuildHooks`: the calls U19's lines make into units not ported yet

Evidence: Build.lua:91/97/108/125/127/131/137/263/311/345/444/490/549/554/560/577/620/646/665/696/726
call buildMode methods of U20 (CloseBuild, OpenSavePopup, SaveDBFile, OpenSaveAsPopup,
OpenConversionPopup, SyncLoadouts, UpdateClassDropdowns, OpenSpectreLibrary, OnFrame, Shutdown,
CanExit) and U21 (RefreshStatList, SetDisplayStat, ClearDisplayStat), and methods of the tabs
(U25 `spec:AddUndoState` / `TreeTab:GetSpecList` / `SetActiveSpec` tails, U30, U33/U34, U42,
U43), the tab constructors (U22, U25, U30, U34, U39, U41, U43, U45, U46-U47) and
`CalcBreakdownControl` (U40). None of them has a frozen Rust signature yet; their files are other
units' (ui doc §12.1).

Applied (U19's files): `pob-app/src/build/init.rs` declares `pub struct BuildHooks` (one entry
per call, each documented with its owner) and `pub fn default_hooks()`; `hooks(cx)` /
`set_hooks(..)` reach the installed set. Defaults: the three calls Init makes unconditionally
(UpdateClassDropdowns, RefreshStatList, SyncLoadouts) do nothing, `OnFrame` is
`init::pending_on_frame` (input + `DrawControls` only), the tab constructors create no node
(`Ctrl::null()`), `breakdown` stores a hidden placeholder control, the model-backed set switches
call the model halves, and the user-triggered rest raise `lua_error!("… not ported yet (Uxx)")`.
Proposed: each owner replaces its entry in `init::default_hooks` (one line each), as with
CCR-0091 B.

## 4. `model.rs` (U15) lacks two re-exports

Evidence: `pob-app/src/model.rs:12` re-exports `PassiveSpec` but not its constructor
(Build.lua:329 `new("PassiveSpec"):PassiveSpec(self, latestTreeVersion)`), nor
`passive_spec_cluster::set_window_title_subtext_hook` (`pob-calc/src/classes/passive_spec_cluster.rs:63`;
CCR-0072 §6: the UI registers the receiver of `main:SetWindowTitleSubtext`).
Work-around: `build/init.rs` calls `pob_calc::classes::{passive_spec::new_PassiveSpec,
passive_spec_cluster::set_window_title_subtext_hook}` directly (marked CCR-0098 §4). Proposed:

```diff
 pub use pob_calc::classes::{ ... };
+pub use pob_calc::classes::passive_spec::new_PassiveSpec;
+pub use pob_calc::classes::passive_spec_cluster::set_window_title_subtext_hook;
```

## 5. Registering `main.modes.BUILD` is deferred

Evidence: CCR-0091 A has U19 replace `PendingMode::new("BUILD")` in
`pob-app/src/main_mode/init.rs:42` (`default_modes`). Doing so breaks U16's acceptance tests:
`pob-ui-oracle/tests/u16_main.rs` keeps PoB's real modes out on the Lua side (`stubMode`) and
relies on pob-app's `PendingMode` on the Rust side; `tests/u16/settings/b_build_custom.xml`
(a BUILD mode whose file does not exist: the real Init calls CloseBuild, U20) and
`h_number_args.migrate.xml` (number arguments) then diverge or raise.
Applied: `build::mode()` exists (`pob-app/src/build/mod.rs`) and U19's test installs it after
`OnInit`; `default_modes` still installs the placeholder. Proposed: when U20 lands CloseBuild and
OnFrame, U20 (with U16's consent for `tests/u16_main.rs`, which then installs a Rust-side
`PendingMode` for BUILD the way its Lua side installs `stubMode`) switches the line to
`crate::build::mode()`.

## 6. `TreeTab:SetActiveSpec`'s UI lines have no hook (calc:BM-A)

Evidence: `pob-calc/src/model/tree_tab.rs:190-191` leaves TreeTab.lua:556-557
(`self.build.spec:SetWindowTitleWithBuildClass()`, `self.build:UpdateClassDropdowns(curSpec.treeVersion)`)
to the UI, but the UI cannot see the calls `Init_model` makes (the constructor's
`SetActiveSpec(1)`, TreeTab.lua:37, and the end of each `Tree` section's `Load`, :525). PoB sets
the window title at both (observable: `SetWindowTitle` side effects; the T3 diff counts them).
Proposed: a registered UI hook called at L556-557:

```diff
+thread_local! { static SET_ACTIVE_SPEC_UI: RefCell<Option<Rc<dyn Fn(&BuildRef, Name)>>> = .. }
+pub fn set_set_active_spec_ui_hook(f: Option<Rc<dyn Fn(&BuildRef, Name)>>)
     // L556-557
-    // L556 UI: ... // L557 UI: ...
+    if let Some(f) = hook { f(build, curSpec.treeVersion.get()) }
```

Work-around: `build/init.rs::tree_tab_set_active_spec_ui_tails` replays both lines after
`Init_model` (the constructor's spec is a fresh class-0 spec, PassiveSpec.lua:2391; each Tree
load's spec is the build's spec). Delete it with the hook. Other callers of the model's
`SetActiveSpec` (U25, the loadouts selFunc) need the same.

## 7. `SetWindowTitleWithBuildClass` reaches the UI without `cx`

Evidence: the CCR-0072 §6 hook is `Fn(&str)`; `main:SetWindowTitleSubtext` needs `cx`
(`main_mode/folders.rs:239`). Applied: U19 registers a receiver that queues the class text;
`init::set_window_title_with_build_class(cx)` and `init::flush_window_titles(cx)` send the
queued titles in order with `self.build.buildName`. Units that run model code which may set the
title flush after it (no frame passes in between, so the effect order is PoB's).

## 8. `buildMode:GetArgs` (Build.lua:967-969, U20's range)

The `Mode` trait needs it in `build/mod.rs` (`BuildModeObj::get_args`); U19 implements the one
statement there (`dbFileName`: string, `false` or nil; `buildName`).
