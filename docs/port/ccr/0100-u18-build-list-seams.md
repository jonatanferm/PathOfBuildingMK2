# CCR-0100: U18 seams: BuildList's background, the mode registration, and U16's test

**Status: applied (CCRDOC, 2026-11-12; code `2ae853b` Merge port/U18): §B, §C and §D. In §A the test seam `build_list::set_draw_background_override` stays (the u18/u52 harnesses use it, and the product never sets it: AMENDMENTS test seams). The proposed deletion is superseded by keeping the seam as a test seam; the product draws U17's real background.**

Unit: **U18** (BuildList mode). Filed status: **pending** (code on `port/U18` follows it). The number was
originally 0095 (taken by PF-B on merge; 0096-0099 held or reserved), renumbered to 0100 on merge (AMENDMENTS A8: in-flight units take 0095+ in
merge order); renumber on merge if another unit took it first.

## A. `main:DrawBackground` in BuildList's OnFrame (to U17, U23)

Evidence: `Modules/BuildList.lua:179` `main:DrawBackground(main.viewPort)`; `Main.lua:1486-1496`
draws `self.tree[latestTreeVersion].assets.Background2/1` (the tree's images, U23's
`LoadTree`/`TreeAssets`); `pob-app/src/main_mode/background.rs` (U17) is still the UI-U0 stub
`todo!("U17: main:DrawBackground")`, so the build list panics on its first frame without a
stand-in.

Work-around (U18's file): `build_list/mod.rs` calls U17's `background::draw_background`
through a thread-local test seam, `build_list::set_draw_background_override(Some(f))`, which the
product never sets (AMENDMENTS "Test seams": changes nothing when unset). U18's oracle test
(`pob-ui-oracle/tests/u18_build_list.rs`) sets it to a no-op and replaces `main:DrawBackground`
with a no-op on the Lua side, so the comparison covers everything except the background quad.

Proposed: once U17 ports `draw_background` and U23 loads the tree assets, delete the seam
(`DRAW_BACKGROUND`, `set_draw_background_override`, and the two stubs in the U18 test).

## B. `main.modes.LIST` registration (CCR-0096 A, done)

`main_mode/init.rs::default_modes` (U16's file) now inserts `crate::build_list::mode()` for
`"LIST"`, the one-line change CCR-0096 A asked of U18. `PendingMode` stays for `"BUILD"`.

Also exported for other units:

- `build_list::sub_path(cx) -> Option<String>`: `main.modes.LIST.subPath` (Build.lua:71, U19);
  `None` before the list mode's first `Init`.
- `build_list::rebuild(cx)`: `main.modes.LIST:BuildList()` (Main.lua:1219, the Options popup,
  U17).
- `build_list::helpers::{BUILD_SORT_DROP_LIST, build_sort_drop_list, scan_folder, filter_list,
  sort_list, can_move_to_sub_path, natural_sort_compare}`: the BuildListHelpers module table
  (CompareTab.lua:15/:1537/:1595 uses it) and Common.lua's `naturalSortCompare` (ui doc §13 item
  15: "the first consuming unit"; U12 did not take it).
- `build_list::folder_list_control::FolderListControl` (Build.lua:1481, U19-U21): `sort_mode` is
  the public `sortMode` field, `sort_list` the method.

## C. U16's oracle test runs BuildList as a stub on the Lua side

Evidence: `pob-ui-oracle/tests/u16_main.rs` `LUA_STUBS` replaces `main.modes.LIST` with a mode
that draws nothing, matching pob-app's `PendingMode` on the Rust side. With B the Rust side runs
the real BuildList, so every U16 scenario would diverge in the list's draws.

Change made by U18 (test-only, U16's file): `RustSide::on_init` re-installs
`PendingMode::new("LIST")` in `main.modes` right after `on_init` (the mode's `Init` runs at the
first `OnFrame`, so this is before any list code runs). One line plus a comment marked CCR-0100.

## D. `main.filterBuildList` lives on the list-mode node

Evidence: `BuildList.lua:97`, `:208` are the only readers and writers of `main.filterBuildList`
(grep of `src/`). `MainState` (U16) has no field for it; U18 keeps it in
`ListMode::filter_build_list`. Not observable. If another unit ever needs it, move it to
`MainState`.
