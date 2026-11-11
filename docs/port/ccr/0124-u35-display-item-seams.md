# CCR-0124: U35 display item seams

Unit: **U35** (ItemsTab display item, `Classes/ItemsTab.lua:1948-2608`,
`pob-app/src/tabs/items/display_item.rs`).
Status: **pending** (code on `port/U35` follows it, with the local workarounds marked
`// CCR-0124`). Number reserved with `scripts/port/next_ccr.sh`.

## 1. `CopyAnointsAndEldritchImplicits` has no hook entry

Evidence: `ItemsTab.lua:1952` (`CreateDisplayItemFromRaw`, U35) calls
`self:CopyAnointsAndEldritchImplicits(newItem, main.migrateEldritchImplicits, false)`, which is
U34's (`ItemsTab.lua:1906-1946`, `core.rs`). Neither U32's `ItemsTabHooks` (CCR-0107 §1) nor
U33's `ItemsUiHooks` (CCR-0120 §1) has an entry for it, and U34 was not merged when U35 was
written (`core.rs` is the UI-U0 stub). Every double-click on an ItemDB or shared-item row
(`ItemDBControl.lua:384`, `SharedItemListControl.lua:65`) and Ctrl+V on the tab (L1464) go through
it with `normalise = true`.

Applied locally (U35's own file): `display_item::DisplayItemHooks` with one entry,
`copy_anoints_and_eldritch_implicits: Rc<dyn Fn(&mut Cx, CtrlId /* the ItemsTab */, &ItemRef
/* newItem */, bool /* copyEldritchImplicits */, bool /* overwrite */, Option<&str>
/* sourceSlotName */)>`, plus `display_item_hooks()` / `set_display_item_hooks(..)` /
`default_display_item_hooks()`. The default raises "ItemsTab:CopyAnointsAndEldritchImplicits is
not ported yet (U34)", as the other hook tables do.

Proposed: U34 replaces the entry in `default_display_item_hooks` with its port (one line):

```diff
-        copy_anoints_and_eldritch_implicits: Rc::new(|_, _, _, _, _, _| {
-            lua_error!("ItemsTab:CopyAnointsAndEldritchImplicits is not ported yet (U34)")
-        }),
+        copy_anoints_and_eldritch_implicits: Rc::new(super::core::CopyAnointsAndEldritchImplicits),
```

(or, if U34 prefers, it adds the entry to its own table and U35's call site switches to it; one
line in `display_item.rs`). TradeQuery.lua:1090 (U51) calls the same method with
`overwrite = true` and a `sourceSlotName`, which the signature already carries.

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
  tooltip click (L1542, U34).
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
