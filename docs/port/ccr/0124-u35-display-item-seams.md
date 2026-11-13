# CCR-0124: U35 display item seams

**Status: applied (CCRDOC, 2026-11-12; code `d479e21` Merge port/U35): §1-§4. The §5 finding for U33 (the range slider is drawn unrounded while it is dragged) is fixed by FUP5 (2026-11-13, `ctor.rs` `sync_after_drag_step`; `u35/g_range_lines` drags the slider; FOLLOWUPS "CCRDOC" 10).**

Unit: **U35** (ItemsTab display item, `Classes/ItemsTab.lua:1948-2608`,
`pob-app/src/tabs/items/display_item.rs`).
Filed status: **pending** (code on `port/U35` follows it, with the local workarounds marked
`// CCR-0124`). Number reserved with `scripts/port/next_ccr.sh`.

## 1. `CopyAnointsAndEldritchImplicits` has no hook entry

Evidence: `ItemsTab.lua:1952` (`CreateDisplayItemFromRaw`, U35) calls
`self:CopyAnointsAndEldritchImplicits(newItem, main.migrateEldritchImplicits, false)`, which is
U34's (`ItemsTab.lua:1906-1946`, `core.rs`). Neither U32's `ItemsTabHooks` (CCR-0107 §1) nor
U33's `ItemsUiHooks` (CCR-0120 §1) has an entry for it, and U34 was not merged when U35 was
written (`core.rs` is the UI-U0 stub). Every double-click on an ItemDB or shared-item row
(`ItemDBControl.lua:384`, `SharedItemListControl.lua:65`) and Ctrl+V on the tab (L1464) go through
it with `normalise = true`.

As done (U34 merged first, with the port as the `pub fn`
`core::CopyAnointsAndEldritchImplicits(cx, newItem, copyEldritchImplicits, overwrite,
sourceSlotName)`, `core.rs`, CCR-0126 §5): U35's `CreateDisplayItemFromRaw` (L1952) calls it
directly. The `display_item::DisplayItemHooks` table that U35's first draft added for this one
entry (with a default raising "ItemsTab:CopyAnointsAndEldritchImplicits is not ported yet
(U34)") is dropped: the method is ported, so no seam is needed, and the u35 scenarios now run
U34's port instead of a test-only copy. TradeQuery.lua:1090 (U51) calls the same `pub fn` with
`overwrite = true` and a `sourceSlotName`.

## 2. `ItemsUiHooks::add_mod_comparison_tooltip` lacks `replaceImplicits`

Evidence: `AddModComparisonTooltip(tooltip, mod, replaceImplicits)` (L2503) is called with
`replaceImplicits = true` by U37 (`CorruptDisplayItem`, L3472) and by U36's AddImplicitPopup
(`AddImplicitPopup.lua:365`, `value.type == "vestigial"`). U33's hook entry (CCR-0120 §1) has the
signature `(cx, me, tooltip, Option<&ItemModRef>)`, i.e. only the L927 call (nil).

Applied locally: the port is `display_item::AddModComparisonTooltip(cx, me, tooltip,
Option<&ItemMod>, replaceImplicits: bool)` (a `pub fn`); U33's entry calls it with `false`.
Proposed: U36/U37 call the `pub fn` directly (it is ported, so no hook is needed), and the hook
entry stays for the constructor's closure. No contract change beyond recording this.

## 3. `GetEquippedSlotForItem` returns the model's slot tables

`ItemsTab.lua:2528-2542` returns the slot *control* and the item set. U32's hook (CCR-0107 §1)
returns `(Option<model slot Table>, Option<itemSet Table>)`, and every caller reads only
`slot.slotName`/`slot.selItemId` and the set, which the model tables carry. U35's port keeps that
signature; `GetComparisonSlotNameForItem` (L2551) reaches the slot control (`slot:IsShown()`)
through `ctor::State::slots[slotName]`. No change proposed.

## 4. Notes for U34, U36-U38

* `ToggleDisplayItemModLine(cx, me, Option<&Table>)` (L2123-2136) is a `pub fn` for Draw's
  tooltip click (L1542, U34). As CCR-0126 §1 asks, U35 replaced the
  `core::CoreHooks::toggle_display_item_mod_line` default with it: the entry downcasts the
  tooltip line's `ModLineRef` to `TtModLine` and its `line` to the mod line `Table` (what
  AddItemTooltip stores, CCR-0073 B).
* `UpdateAffixControl` takes the affix dropdown's node and finds its index among
  `displayItemAffix1..9` (for `drop.slider` and the `outputTable`/`outputIndex` fields of
  `ctor::State::affix_controls`); `PowerCache` is the shared `powerCache` table of one
  `UpdateAffixControls` call.
* The dropdown values U35 builds: `display_item::VariantValue { variantId }` (variant group
  entries), `display_item::RangeLineValue { modLine }` (`displayItemRangeLine`), U33's
  `ctor::AffixSeries` (affix entries; the "None" entry is a string) and `ctor::ClusterSkillValue`.

## 5. Finding for U33 (not changed here): `displayItemRangeSlider` while it moves

Observed in U35's scenario `g_range_lines` (showAllItemAffixes off, Abyssus, range line 3
selected): clicking the range slider, and every frame of dragging it, diverges: PoB draws the
knob at x 1163.000032, the port at 1163.0 (`SliderControl.lua:119` vs `slider.rs:317`); the
frames after the mouse is released agree again. PoB shows `round(5/82, 6)` (the re-parsed
`modLine.range`, Item.lua:1855 `round(modLine.range, 6)`), the port the unrounded `5/82`.

Cause: `SliderControl:Draw` sets the value while dragging (`SetValFromKnobX`, L86-89: the
changeFunc stores `line.range = val` and re-parses the item), then calls `self:IsMouseOver()`
(L90), whose `self:IsShown()` evaluates the slider's `shown` function (ItemsTab.lua:1084-1094),
which writes `slider.val = modLine.range` (the rounded value) before the knob is drawn
(L111/L119). U33's DIVERGENCE(range-slider-val) emulation (`ctor::range_slider_overrides`)
syncs `val` only *before* the class methods run, so the value set during Draw's drag step is
drawn unrounded. (OnKeyDown/OnKeyUp evaluate `IsShown`/`IsMouseOver` before they set the value,
so the pre-call sync covers them; the exact line numbers above are SliderControl.lua at
16de4b82d and were read, the cause itself is inferred from the trace, not stepped through.)

Proposed (U33's `ctor.rs`): also sync after the drag step, e.g. in the Draw override run the
drag (`if dragging && !IsKeyDown(LEFTBUTTON) ...; if dragging { SetValFromKnobX }`), sync, then
call the class Draw with the drag already applied (or give SliderControl a post-`SetVal` hook
that the override uses for the `shown` side effect). U35's scenario does not move this slider
until then; U35's own sliders (the affix roll sliders) have no such `shown` and agree.
