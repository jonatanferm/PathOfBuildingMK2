# CCR-0143: U49 CompareTab D seams (TradeHelpers copies, drawing another build's tree, the cursor patch, the entry's item tooltips, the hook install)

**Status: applied (CCRDOC, 2026-11-12; code `2de5161` Merge port/U49): §1 (the TradeHelpers copies; U51 is deferred), §2 `with_entry_as_build`, §3 `with_cursor_origin`, §5 and §6; §7 Buy raises until U51; §8 was fixed by U53 (CCR-0145 §5). Not built: §2 `PassiveTreeView::draw_for`, §3 `Sys::set_cursor_offset` and §4 (the comparison build's item tooltip uses the open build's ItemsTab; FOLLOWUPS "CCRDOC" 4).**

Unit: **U49** (`Classes/CompareTab.lua:3162-4048`: DrawSummary, DrawStatList, DrawTree,
DrawItemExpanded, ShouldShowRing3, AddAbyssSockets, DrawItems; file
`pob-app/src/tabs/compare/draw_a.rs`).
Filed status: **pending** (code on `port/U49` follows it, with the local workarounds marked `// CCR-0143`).
Number reserved with `scripts/port/next_ccr.sh`.

No frozen contract changes. Local workarounds for code other units own, two gaps, and the
three-line install CCR-0137 §3 asks for.

## 1. `tradeHelpers` (TradeHelpers.lua, U51, deferred)

Evidence: DrawItemExpanded and DrawItems call `tradeHelpers.getRarityColor` (CompareTab.lua:3516),
`modLineTemplate` (:3621), `modLineValue` (:3628), `buildModMap` (:3862-3863),
`getSlotDiffLabel` (:3756, :3853), `drawCopyButtons` (:3856), `drawCompactSlotRow` (:3880) and
read `tradeHelpers.ITEM_BOX_W` (:3772). They are TradeHelpers.lua:71-100 and 401-610 (with the
local `fitItemName`, :499-514). U51 (`trade/**`) is optional and not scheduled; `trade/mod.rs`
is U0's empty stub.

Workaround (draw_a.rs, `// CCR-0143 §1`): private free functions with the Lua names, statement
for statement with `lua:` comments and no `@pob` markers (the range stays U51's).
`drawCompactSlotRow`'s two ItemsTab arguments are only returned as `hoverItemsTab`; the copy
returns which side was hovered (`ItemsTabSide`).

Proposed (U51): `trade/helpers.rs` exports these as `pub fn`s; U49 then calls them and deletes the
copies (eight call sites).

## 2. `compareEntry.treeTab.viewer:Draw(compareEntry, ...)` (CompareTab.lua:3480)

Evidence: PassiveTreeView:Draw takes the build to draw (`build.spec`, `build.itemsTab`,
`build.calcsTab`, `build.buildFlag`, `build.outputRevision`, ...). U23's
`PassiveTreeView::draw(&mut self, cx, view_port, input_events)` has no `build` parameter: it
reads the open build (`cx.build().model`, tree_view/input.rs:159, and again in names.rs:150,
tooltip.rs:143/720, compare.rs:190). The comparison build's viewer must draw the entry's build.

Workaround (draw_a.rs `with_entry_as_build`, `// CCR-0143 §2`): for the duration of the right
tree's draw, `cx.build_mut()`'s `model`, `calc` (swapped with the entry's own calc facade) and
`build_name` are swapped with the entry's and restored afterwards (also when the draw raises, by
`catch_unwind` + `resume_unwind`; wasm32 aborts on panic). The view's UI hooks
(`TreeViewHooks::open_mastery_popup`, `modify_node_popup`, `socket_right_click`, U24's tooltip
hooks) still reach the open build's TreeTab/ItemsTab UI nodes; in PoB they reach the entry's
TreeTab/ItemsTab. The u49 scenarios zoom the right tree (PageUp/PageDown routed by the cursor)
and do not click its nodes.

Proposed (U23): `PassiveTreeView::draw_for(&mut self, cx, build: &model::BuildRef, view_port,
input_events)` (and the same `build` argument for the hover tooltip path), `draw` calling it
with `cx.build().model`; U49 then calls `draw_for(.., &compareEntry.model, ..)` and deletes the
swap.

## 3. `GetCursorPos = function() ... end` (CompareTab.lua:3425-3428, 3460-3463, 3475-3478)

Evidence: DrawTree replaces the global `GetCursorPos` with one that subtracts the tree viewport's
origin while the viewer draws (the trees get a `{ x = 0, y = 0 }` viewport inside a
`SetViewport`), and restores it at :3437 / :3485. `pob_sg::Sys::cursor_pos()` reads the cursor
snapshot of `begin_frame`; there is no override.

Workaround (draw_a.rs `with_cursor_origin`, `// CCR-0143 §3`): the cursor is re-snapshotted with
`Sys::begin_frame(phys, scale)` (the same DPI scale) so that `cursor_pos()` returns the patched
position, and re-snapshotted to the original position afterwards. `phys` is the physical position
whose `round(phys / scale)` is the wanted logical one: one exists for every logical position when
`scale >= 1`, and the tree viewports' offsets are integers (`halfWidth` is `m_floor(..) - 2`), so
the result is exact there. For `scale < 1` the nearest physical position is used.

Proposed (pob-sg, U1): `Sys::set_cursor_offset(Option<(f64, f64)>)` (subtracted by `cursor_pos`),
U49 sets and clears it around the draws.

## 4. GAP: `hoverItemsTab:AddItemTooltip(...)` with `compareEntry.itemsTab` (CompareTab.lua:3988)

Evidence: the compact row reports the hovered item's ItemsTab; for a comparison-build item it is
`compareEntry.itemsTab` (CompareEntry.lua:113, a full ItemsTab whose `self.build` is the entry).
The entry has no UI ItemsTab in the port (CCR-0137 §2); U38's
`tabs::items::tooltip::AddItemTooltip(cx, me, ..)` builds its comparison blocks against the open
build (`cx.build()`, the ItemsTab node's slots).

Workaround: the comparison build's item tooltip is built by the open build's ItemsTab (the item
text is the same; the "Equipping this item in ..." stat blocks are the primary build's, PoB shows
the entry's). The u49 scenarios hover a primary-build item only. U24 has the same gap for compare
jewels (CCR-0122 §2).

Proposed (U38, with CCR-0137 §2's per-build tab halves): an `AddItemTooltip` variant that takes the
build it compares against.

## 5. `slots[name].shown()` in AddAbyssSockets (CompareTab.lua:3685-3686)

As CCR-0138 §2: calc:BM-A's `items_tab.rs` `slot_IsShown` is private, so draw_a.rs repeats the
`$shown` match for the abyssal-socket kinds (`abyssal_slot_shown`, `// CCR-0143 §5`).
Proposed (calc:BM-A): `pub fn slot_IsShown`; U48 and U49 call it.

## 6. The hook install (CCR-0137 §3) and U48's copies (CCR-0138 §1)

Applied (U47's file, three lines): `layout::default_draw_hooks` installs
`draw_a::draw_tree_hook`, `draw_summary_hook`, `draw_items_hook` (each marked `// U49 (CCR-0143
§6)`). U47's and U48's tests install their own stand-in tables, so they are unaffected.

`CompareTab::ShouldShowRing3(cx, entry)` and `CompareTab::AddAbyssSockets(cx, entry, &mut slots,
requireBothSides)` are now `pub` in draw_a.rs: power.rs (U48) can replace its private copies
`should_show_ring3` / `add_abyss_sockets` (CCR-0138 §1, two call sites). Not done here (U48's file).

## 7. `buySimilar.openPopup(item, slot, self.primaryBuild)` (CompareTab.lua:3980, U51)

Routed through U33's existing seam `ItemsUiHooks::buy_similar_open_popup` (its default raises
"CompareBuySimilar.openPopup is not ported yet (U51)", the error prompt PoB would not show), with
the open build's ItemsTab node as the build handle. A Buy click therefore raises until U51 lands.

## 8. Not U49's, found by the scenarios

Hovering a jewel socket's label in the compact Items view draws `itemSlotHelper.DrawViewer`
(CompareTab.lua:3896, U32's `slot_helper::DrawViewer`), whose `ItemsTabHooks::socket_viewer_draw`
default raises "PassiveTreeView:Draw is not ported yet (U23)" (slot_helper.rs:174): the ItemsTab's
`socketViewer` is not wired. U49 calls the port as is; the u49 scenarios do not hover jewel
labels. Proposed (U33/U23): install `socket_viewer_draw` (lend the ItemsTab's socket viewer, set
its zoom fields, `draw`).
