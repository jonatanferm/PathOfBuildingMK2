# CCR-0096: U16 seams to the mode objects, the Main popups, LoadTree, and the oracle

**Status: applied (CCRDOC, 2026-11-12; code `149150b` Merge port/U16). The ui doc §10.1 now describes the `Mode` trait, `main.modes` as `IndexMap<String, Rc<dyn Mode>>`, `MainHooks` (§10.5) and `MainState.input_events: Rc<RefCell<InputEvents>>`. `default_modes` installs U18's and U20's real modes, and `PendingMode` is used only by tests. §C is rejected (the re-export was not added; `main_mode::new_item` calls pob-calc directly, ui doc §10.2 as amended). §D.1 is not adopted by the generic runner's `RustPobApp`: the unit harnesses drive `PobApp` themselves.**

Unit: **U16** (Launch and Main core). Filed status: **pending** (code on `port/U16` follows it).

U16 calls code that later units own (U17 popups, U18 BuildList, U19-U21 Build, U23 LoadTree,
U20 SaveDB) and that does not exist yet; the frozen stubs declare no signature for it. U16 adds
the seams below in its own files, with placeholders, so the later units plug in by one line each
instead of editing U16's logic.

## A. `main.modes` and `main:CallMode`: the `Mode` trait (`pob-app/src/main_mode/mod.rs`)

Evidence: `Main.lua:61-63` (`self.modes[..] = LoadModule(..)`), `:513-518` (`CallMode` looks the
method up by name), callers `:366` (`CanExit "EXIT"`), `:375`, `:389` (`Shutdown`), `:393`
(`Init`), `:409` (`OnFrame`), `:744` (`GetArgs`), `:395`/`:479` (`viewMode`), `:1362`
(`CanExit "UPDATE"`, U17), `Launch.lua:115` (`modes.BUILD.outputRevision`, `.buildFlag`),
`Build.lua:71` (`main.modes.LIST.subPath`, U19 reads U18's field). The frozen `ListMode` /
`BuildMode` widgets have no such methods and a Rust trait has no by-name lookup.

```rust
pub enum ModeArg { Nil, Bool(bool), Num(f64), Str(String) }   // Main.lua:546-559, :744-754
pub trait Mode {
    fn init(&self, cx: &mut Cx, args: &[ModeArg]);
    fn shutdown(&self, cx: &mut Cx) {}
    fn on_frame(&self, cx: &mut Cx, input_events: &mut InputEvents, vp: &ViewPort);
    fn get_args(&self, cx: &Cx) -> Option<Vec<ModeArg>>;
    fn can_exit(&self, cx: &mut Cx, reason: &str) -> Option<bool> { None }
    fn view_mode(&self, cx: &Cx) -> Option<String> { None }
    fn set_view_mode(&self, cx: &mut Cx, v: &str);
    fn output_revision(&self, cx: &Cx) -> Option<f64> { None }
    fn build_flag(&self, cx: &Cx) -> bool { false }
}
// MainState.modes: IndexMap<String, Rc<dyn Mode>>
```

Proposed: U18 adds `pub fn mode() -> Rc<dyn crate::main_mode::Mode>` to `build_list/mod.rs`, U19
the same to `build/mod.rs`, and each replaces its `PendingMode::new(..)` line in
`main_mode/init.rs::default_modes`. Until then `PendingMode` (draws nothing; `GetArgs` returns
the first two `Init` arguments, which is what both real modes return right after `Init`) keeps
Settings.xml round-tripping. U19 reads `main.modes.LIST.subPath` through a method U18 adds to
the trait (or a field on `MainState`), as it prefers.

## B. Main popups and LoadTree: `MainHooks` (`main_mode/mod.rs`)

Evidence: `Main.lua:229` (`OpenOptionsPopup`), `:232`/`:480` (`OpenAboutPopup(tabName or 1)`),
`:235` (`OpenUpdatePopup`) are U17's (`options_popup.rs`, `about_popup.rs`, `update_popup.rs`
contain no function yet); `:152` (`LoadTree(latestTreeVersion)`) is U23's (`tree_view/assets.rs`
names Main.lua:352-364).

```rust
pub struct MainHooks {
    pub open_options_popup: Rc<dyn Fn(&mut Cx)>,
    pub open_about_popup: Rc<dyn Fn(&mut Cx, ModeArg)>,   // helpSectionName: a string or 1
    pub open_update_popup: Rc<dyn Fn(&mut Cx)>,
    pub load_tree: Rc<dyn Fn(&mut Cx, &str)>,
}
```

`init::default_hooks()` installs placeholders: the popups raise `lua_error!("... not ported yet
(U17 ...)")` (PoB's error prompt shows it), `load_tree` does nothing. Proposed: U17 and U23
replace their entries there.

## C. Item constructor not re-exported by `model.rs`

Evidence: `Main.lua:166`, `:180`, `:714`, `:726` (`new("Item")`); `pob-app/src/model.rs` (U15)
re-exports `Item`/`ItemRef` but not `pob_calc::classes::item::new_Item`. Work-around:
`main_mode::new_item` calls pob-calc directly (marked CCR-0096). Proposed diff to `model.rs`:

```diff
 pub use pob_calc::classes::{
     Item, ItemRef, ModDB, ModList, Node, NodeRef, PassiveSpec, PassiveSpecRef, PassiveTree,
     PassiveTreeRef,
 };
+pub use pob_calc::classes::item::new_Item;
```

## D. Oracle runner (U8)

1. `runner::RustPobApp::load_build` / `snapshot` bail. pob-app now has
   `PobApp::load_build(name, xml)` (`main:SetMode("BUILD", false, name, xml)`) and
   `PobApp::snapshot()` (returns `None` until U20's `SaveDB`); `App::on_script_step` handles
   both. Proposed: RustPobApp forwards to them.
2. Patch 4 (Launch.lua:59 dev-mode detection disabled) has a Rust-side equivalent:
   `LaunchState::force_release_install`. The checkout's manifest.xml has no branch/platform, so
   without it the Rust side runs in dev mode (user data in the script dir, Dev Mode label).
   Proposed: `run_with` sets `app.cx.app_mut().launch.force_release_install = true` after
   `PobApp::new` (U16's own test, `tests/u16_main.rs`, does).
3. The Rust side's `OraclePlatform` records no side effects (CCR-0028 item 2), so `Copy`,
   `WindowTitle`, `Screenshot` from pob-app diverge in scenario runs; `tests/u16_main.rs` wraps
   the platform to record them (`RecordingPlatform`), which the runner could adopt.

## E. `MainState.input_events` type

UI-U0 declared `main.inputEvents` as `InputEvents`. U16 (owner) makes it
`Rc<RefCell<InputEvents>>`: `main:OnFrame` lends it to the controls, the mode and the toasts while
`cx` is borrowed, and a Lua error in the middle of a frame leaves the events in place, as the Lua
table survives a failed `PCall`. No other unit used the field.
