# CCR-0073 (U10): Tooltip contract notes (ui doc §6.9)

**Status: applied 2026-11-10 by CCR2 (B, C, E: ui doc §6.9, §13 item 12); A and D need code. See AMENDMENTS A8. ORC-X2 (2026-11-10) applied D (`tooltip::wrap_string` re-exports `crate::wrap::wrap_string`); A still needs code.**

Unit: U10 (Tooltip, TooltipHost). Filed status: proposed. Number: next free at the time of writing
(0070/0071 may be taken by unmerged branches; renumber on collision). Every item is worked
around inside U10's own files (`crates/pob-ui-core/src/tooltip/{tooltip,host}.rs`), marked
`// CCR-0073`.

## A. `Tooltip::add_separator` needs `&mut Cx`

`Tooltip.lua:145-149` creates and loads the separator image inside `AddSeparator`:

```lua
if not self.separatorImage or self.separatorImagePath ~= separatorPath then
	self.separatorImage = NewImageHandle()
	self.separatorImage:Load(separatorPath)
	self.separatorImagePath = separatorPath
end
```

`ImageHandle::load` takes `&mut Sg`, but §6.9 freezes `add_separator(&mut self, cx: &Cx, size)`.

Workaround: the handle is created at once (line identity and the path cache are Lua's) and the
`Load` is queued in one thread-local FIFO shared by all tooltips; every `real_draw` flushes it
first (`tooltip::tooltip::flush_pending_loads`, public; `Tooltip::flush_loads` forwards to it).
A per-tooltip queue was not enough: a parent tooltip built together with its `childTooltips`
loaded the parent's header images before the children's separators (u10_headers page 4, 8
divergences per frame). With the shared queue the loads keep Lua's order among themselves; only
their position relative to other side effects between the build and the next tooltip draw can
differ (none in the U10 galleries).

Proposed diff (§6.9 and the stub):

```diff
-    pub fn add_separator(&mut self, cx: &Cx, size: Option<f64>);                          // :120
+    pub fn add_separator(&mut self, cx: &mut Cx, size: Option<f64>);                      // :120
```

and delete `PENDING_LOADS`/`flush_pending_loads`/`flush_loads`. No caller exists yet (grep of `crates/`).

## B. `ModLineRef` downcast target

`Tooltip.lua:302` reads `data.modLine.disabled`; ItemsTab hands the model's mod line in
(`ItemsTab.lua:4479`) and gets it back from `line.modLine` to toggle it (`ItemsTab.lua:1532-1541`,
`:2127`). §6.9 says "reads .disabled etc. via downcast", but pob-ui-core cannot name the model type
(pob-calc's item mod lines are `Table`s). U10 defines the concrete type:

```rust
pub struct TtModLine { pub disabled: Rc<dyn Fn() -> bool>, pub line: Rc<dyn Any> }
```

A `ModLineRef` that is not a `TtModLine` reads as not disabled. Proposed §6.9 text: "`ModLineRef`
is an `Rc<TtModLine>` coerced to `Rc<dyn Any>`; `disabled` reads the model field live, `line` is
the model object (U38 downcasts it)". Alternative: `pub type ModLineRef = Rc<dyn TooltipModLine>`
with a trait `fn disabled(&self) -> bool; fn as_any(&self) -> &dyn Any`.

## C. `TtBackground::Color` has no Lua counterpart

Every `AddLine` background in PoB is an image path string (`ItemsTab.lua:4456`, `:4461`), and
`Tooltip.lua:615-625` loads any string background as an image. Both variants are therefore
treated as image paths. Proposed: drop `Color` (or document it as an alias).

## D. `main:WrapString` before U14

`AddLine` with `maxWidth` calls `main:WrapString` (`Tooltip.lua:106`), which is U14's
`crate::wrap::wrap_string` (still `todo!()`); U10's acceptance needs it ("`maxWidth` wrap") and
U14 is not a U10 dependency. U10 carries a local copy (`tooltip::tooltip::wrap_string`,
`@pob Modules/Main.lua:1795-1822`). When U14 lands, `add_line_ex` calls
`crate::wrap::wrap_string` and the copy is deleted. Proposed: U14 depends on nothing new; the
orchestrator either orders U14 before U10's gate or accepts the copy.

## E. Additions (U10-owned surface, ui doc §13 item 12: `TooltipLine`, `TooltipBlock`)

| Item | Why |
|---|---|
| `TooltipLine.separator_image: Option<ImageHandle>` (was `Option<String>`) | Lua stores the handle object (shared with the tooltip's cache), Tooltip.lua:151-157 |
| `TooltipLine.handled: Cell<bool>`, `bounds: Cell<Option<TtBounds>>` | written by CalculateColumns (`_handled` :281, `bounds` :307) under `&self` (GetSize/GetDynamicSize are `&self`); ItemsTab.lua:1532 reads `bounds` |
| `Tooltip` fields `influence_header1/2`, `foil_type`, `separator_image(_path)`, `header_left/middle/right(_path)`, `influence_icon1/2`, `bg_handles` | the remaining Tooltip.lua fields (§6.9 "…remaining fields") |
| `Tooltip: Default` is hand-written | `new("Tooltip"):Tooltip()` runs `Clear()`: one block `{height = 0}` and `color = {0.5, 0.3, 0}` |
| `Tooltip::calculate_columns`, `TtDrawEntry`, `TtDrawImg` | Tooltip.lua:205-372 (a public method in Lua) |
| `tooltip::tooltip::module_load(cx)`, `RECIPE_NAMES` | the module-level recipe preload (Tooltip.lua:36-41), which Lua runs at the first `new("Tooltip")`. The app shell (U16) calls it where Launch/Main first creates a TooltipHost; U9's galleries keep their stand-in `_u9::tooltip_module_load`, which records the same effects |
| `tooltip::tooltip::flush_pending_loads`, `Tooltip::flush_loads` | item A |
