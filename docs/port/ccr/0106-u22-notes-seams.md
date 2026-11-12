# CCR-0106: U22 Notes tab seams (mutable UI savers, the per-tab hook arms)

**Status: applied (CCRDOC, 2026-11-12; code `32b022c` Merge port/U22): §1 (`&mut Cx` savers), §2 (the per-tab arms; ui doc §10.5), §3, §4 and §5 (`app/pob-desktop` initialises the game data). The §4 clean-ups were not done: `on_frame::State.last_*` and `StatLineInfo` stay as fallbacks (FOLLOWUPS "CCRDOC" 9).**

Unit: **U22** (NotesTab, `Classes/NotesTab.lua:1-112`, the wave-4 template gate). Filed status:
**pending** (code on `port/U22` follows it). Number reserved with `scripts/port/next_ccr.sh`.

## 1. `ShellHooks::save_ui_section` and `io::save_db` take `&mut Cx`

Evidence: `NotesTab.lua:84-88`: `NotesTab:Save(xml)` first calls `self:SetShowColorCodes(false)`
(L85: writes `showColorCodes`, the toggle button's `label` and `controls.edit.buf`), then sets
`self.lastContent` (L87). `buildMode:SaveDB` (`Build.lua:2288-2292`) calls every saver's
`Save`, so every save (Ctrl+S, the Save/Save As popups, the similar-builds code, the oracle's
`snapshot` step) changes the tab: after a save with the codes shown, the button reads "Show
Color Codes" and the buffer holds the unescaped codes. CCR-0104 §1 declared
`save_ui_section: Rc<dyn Fn(&Cx, &str) -> Option<XmlNode>>` and `io::save_db(cx: &Cx, ..)`,
which cannot do that.

Applied (U20's files, one signature each):

```diff
 // pob-app/src/build/on_frame.rs (ShellHooks)
-    pub save_ui_section: Rc<dyn Fn(&Cx, &str) -> Option<model::XmlNode>>,
+    pub save_ui_section: Rc<dyn Fn(&mut Cx, &str) -> Option<model::XmlNode>>,
 // pob-app/src/build/io.rs
-pub fn save_db(cx: &Cx, file_name: &str) -> Option<String> {
-    let b = cx.app().build.as_ref()?;
-    let text = b.model.SaveDB(file_name)?;
+pub fn save_db(cx: &mut Cx, file_name: &str) -> Option<String> {
+    let m = cx.app().build.as_ref()?.model.clone();
+    let text = m.SaveDB(file_name)?;
-pub fn snapshot(cx: &Cx) -> Option<String> {
+pub fn snapshot(cx: &mut Cx) -> Option<String> {
 // pob-app/src/app.rs (PobApp::snapshot already takes &mut self)
-        crate::build::io::snapshot(&self.cx)
+        crate::build::io::snapshot(&mut self.cx)
```

All callers already held `&mut Cx` (`save_db_file`, the similar-builds closure in
`popups.rs`, `PobApp::snapshot`). Order: the Lua runs the savers in `pairs(self.savers)` order
after `Build` (L2288); the Rust side runs BM-A's `SaveDB` first and then the UI savers. No saver
reads another saver's state, so only the `Notes` side effects matter, and they happen in the
same call.

## 2. The per-tab arms in `default_hooks` / `default_shell_hooks` (the wave-4 template)

`BuildHooks::{load_ui_section}` and `ShellHooks::{draw_tab, tab_flag, set_tab_flag,
save_ui_section}` are one closure for all tabs (CCR-0098 §3, CCR-0104 §1 say "one line each").
Applied: each default is a `match` with one arm per ported tab and a no-op/false/`None`
fallback; U22 adds the Notes arms, each a call into `tabs/notes.rs`:

```rust
// init::default_hooks
load_ui_section: Rc::new(|cx, elem, node, db| match elem {
    "Notes" => crate::tabs::notes::load_ui_section(cx, node, db),
    _ => false,
}),
notes_tab: Rc::new(crate::tabs::notes::ctor_hook),
// on_frame::default_shell_hooks
draw_tab: Rc::new(|cx, tab, vp, ev| match tab {
    BuildTab::Notes => crate::tabs::notes::draw_tab(cx, vp, ev),
    _ => {}
}),
tab_flag: Rc::new(|cx, flag| match flag { TabFlag::Notes => crate::tabs::notes::mod_flag(cx), _ => false }),
set_tab_flag: Rc::new(|cx, flag, v| match flag { TabFlag::Notes => crate::tabs::notes::set_mod_flag(cx, v), _ => {} }),
save_ui_section: Rc::new(|cx, elem| match elem {
    "Notes" => crate::tabs::notes::save_ui_section(cx),
    _ => None,
}),
```

Proposed for wave 4: every tab owner adds its arm(s) the same way (one line per arm) and
keeps the tab-side functions (`ctor_hook`, `draw_tab`, `mod_flag`, `set_mod_flag`,
`load_ui_section` / `save_ui_section` for UI-only savers) in its own file. The tab node is
created with `cx.add_free(Some(build), Tab::new(build))` and found again through
`BuildState::tabs.<tab>` (`tabs/notes.rs::tab` checks it with `cx.ui.try_w`, so a test that
installs `pending_tab()` reads "no tab").

Test files that replace whole closures must keep the defaults' arms: `tests/u22_notes.rs`
wraps the default `tab_flag`/`set_tab_flag` instead of replacing them. `tests/u20_build.rs`
still replaces `tab_flag` / `set_tab_flag` and stubs the Lua `notesTab.Draw`; see §3.

## 3. The earlier units' oracle tests with the real Notes tab

With the Notes defaults installed, `u19_build`/`u20_build`/`u21_stats` construct and load the
real `NotesTab` on the Rust side (the Lua side always did). `u20_build`'s Lua stand-ins replace
every tab's `Draw` (`notesTab` included) and `RefreshSkillSelectControls` with no-ops, so its
`rust_stubs` now also sets `draw_tab` and `refresh_skill_select_controls` (§4) to no-ops. Its
T4 already compared the `Notes` section separately ("set aside" on a difference). `u19_build`
(`pending_on_frame`) and `u21_stats` (its own OnFrame) never call `draw_tab`.

## 4. U20 <-> U21 seams nobody wired (found by the end-to-end gate)

U20 and U21 merged in parallel, each against the other's placeholder. With both real, the full
app (`tests/u22_notes.rs`) showed:

* `RefreshStatList` (U21, Build.lua:2117) calls `EstimatePlayerProgress` through
  `stats::set_estimate_player_progress` (CCR-0103 §2), which nobody registered: the
  `pointDisplay` width read a nil `str` (`bad argument #3 to 'DrawStringWidth'`) on the first
  frame of every opened build. Applied (`stats.rs`): with nothing registered, RefreshStatList
  calls U20's `loadouts::estimate_player_progress`.
* `ShellHooks::refresh_skill_select_controls` / `get_sidebar_breakdown` (CCR-0104 §1) still
  had the placeholder defaults although U21 ported both. Applied (`on_frame.rs`): the defaults
  call `skill_select::refresh_skill_select_controls` and `stats::get_sidebar_breakdown`.
* OnFrame (L1274-1288, U20) compared `on_frame::State.last_*`, while U21's FormatStat writes
  `stats::State.last*` (its own copy): the window title was set again on every frame. Applied
  (`on_frame.rs`): OnFrame reads U21's fields, falling back to U20's (which nothing writes now).
  Proposed: delete `on_frame::State.last_*`.
* SetDisplayStat downcasts the stat row's `extra` to `on_frame::StatLineInfo`; U21's rows carry
  `stats::StatLine`. Applied (`on_frame.rs`): the downcast also accepts `stats::StatLine`
  (converted field by field). Proposed: keep one type (U21's) and delete `StatLineInfo`. The
  stat-box hover is not covered by `u22_notes` (its scripts keep the mouse over the tab).
* `io::save_db`'s UI-saver branch (CCR-0104 §1) composed `ParseXML_root`'s nameless top node,
  which wrote a document without a root element as soon as a UI saver was installed. Applied:
  it composes the `PathOfBuilding` element.

## 5. The desktop binary never initialised the game data

Evidence: `app/pob-port-desktop/src/main.rs` (U7) ran `pob_host::run_desktop` without
`pob_gamedata::init*`, so Launch stopped at "Error loading main script: pob_gamedata::init not
called" (`pob-gamedata/src/data.rs:625`). Applied (U7's file, gate fix): `locate_dump_dir()`
(`$POB_DUMP_DIR`, else the first `data/dump/data.bin` above the working directory or the
executable, as `locate_pob_root` finds `.PathOfBuilding`) and `pob_gamedata::init_from_dir`
before `run_desktop`; `pob-gamedata` added to the binary's dependencies. Proposed: U7 (or the
packaging unit) owns where the shipped data lives.

With it the binary starts (dev mode: no manifest, so PoB opens a new build directly), draws the
build shell and the stat box, and the Notes tab works (typing, Tab, RETURN, colour buttons,
show/hide codes, Ctrl+Z). Opening a build shows one error prompt, "TreeTab:GetSpecList is not
ported yet (U25)", from the buildLoadouts selFunc (Build.lua:395, reached from SyncLoadouts' SetSel): the
U25 placeholder in `init::default_hooks`; dismissing it (Escape) continues. The oracle tests use
the TreeTab.lua:492-498 stand-in for it (`u20_build`, `u22_notes`).
