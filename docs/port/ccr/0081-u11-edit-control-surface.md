# CCR-0081 (U11): EditControl surface amendments, and two oracle gaps the U11 galleries work around

**Status: applied 2026-11-10 by CCR2 (§1, §2: ui doc §6.8, §13 item 12); §3, §4 need code. See AMENDMENTS A8.**

Unit: U11 (Edit family). Filed status: proposed. Number: next free at the time of writing (0076 and
0077 are taken by unmerged branches; renumber on collision, the code comments say `CCR-0081`; renumbered from 0078 at merge, 0078 is U12).

## 1. `changeFunc` has a second argument (ui doc §6.8 `EditArgs`, frozen)

Evidence: `Classes/EditControl.lua:114` `self.changeFunc(self.placeholder, true)` (SetPlaceholder
with `notify`); every other call passes only `buf` (L106, :162, :184, :633, :656, :758). The
second argument is read by `Classes/ConfigTab.lua:305`, `:323`, `:334`
(`function(buf, placeholder) if placeholder then ... configSets[..].placeholder[var] = tostring(buf)
else ... input[var] = ...; self:AddUndoState(); self:BuildModList() end`), and SetPlaceholder with
`notify = true` is called by `Modules/ConfigOptions.lua:2088-2093`. The frozen
`change_func: Option<Rc<dyn Fn(&mut Cx, &str)>>` cannot carry it.

Local workaround (marked `CCR-0081`, `crates/pob-ui-core/src/controls/edit.rs`): a second slot
for the `placeholder == true` call, used only when it is set, so a one-argument Lua function stays
one closure:

```diff
 pub struct EditArgs {
     ...
     pub change_func: Option<Rc<dyn Fn(&mut Cx, &str)>>,
+    /// The `placeholder == true` call of a two-argument changeFunc (EditControl.lua:114).
+    pub placeholder_change_func: Option<Rc<dyn Fn(&mut Cx, &str)>>,
     ...
 }
```

`SetPlaceholder(text, true)` calls `placeholder_change_func` if set, else `change_func` (and
nothing when `change_func` is nil, as `notify and self.changeFunc` says). Proposed: freeze this in
ui doc §6.8 (U41 ConfigTab is the user). The alternative, `Fn(&mut Cx, &str, bool)`, would touch
every changeFunc in ~30 files for three call sites.

## 2. Owner amendments to the U11 stubs (ui doc §13 item 12 lists `EditUndoState (U11)`)

| Stub | Now | Evidence |
|---|---|---|
| `EditUndoState { buf, caret: usize }` | `caret: Rc<Cell<usize>>`; `EditControl.last_undo_state: Option<EditUndoState>` | `self.lastUndoState.caret = self.caret` (L241, :324, :490, :507, :560, :574, :591, :601) mutates the state *table* CreateUndoState returned (L748) or RestoreUndoState kept (L760): the top of the undo stack, so Undo-then-Redo restores the moved caret. The `Rc<Cell>` keeps Lua's table identity (unit test `caret_moves_write_through_last_undo_state`). |
| — | `pub const edit::UNDO: UndoOps<EditUndoState>` | EditControl's UndoHandler "class" (ui doc §6.12), for callers that need `AddUndoState`/`ResetUndo` on an edit (no PoB file outside EditControl.lua does today). |
| `EditControl.filter: Option<String>` | `filter: String`, `filter_pattern: String` (resolved by `init`) | L50 reads `main.unicode` when the argument is nil; `state()` has no `Cx`. `init` runs right after insertion (before any mixin or caller code). |
| — | fields `default_line_height`, `font: Font`, `text_col`, `inactive_col`, `disable_col`, `sel_col`, `sel_bg_col`, `blink_start`, `is_numeric`, `protected`, `placeholder`, `inactive_text: Option<InactiveText>` (`Str` or `Fn(&mut Cx, &str) -> String`), `enter_func: Fn(&mut Cx, &str)`, `paste_filter: Fn(&mut Cx, &str) -> String`, `border_func: Fn(&Cx) -> (f64, f64, f64)` (as CheckBoxControl's, CCR-0069), `number_inc: Option<f64>`, `drag` | L55-L62, L70, L97; callers' assignments: ConfigTab.lua:86, :349, :697; SkillsTab.lua:338, :879, :1097; CompareTab.lua:1474; ImportTab.lua:342, :672; ItemsTab.lua:2728; PartyTab.lua:291; Export/Main.lua:301, :312, :333 |
| — | `EditControl::{zoom_text, update_scroll_bars, scroll_caret_into_view, move_caret_vertically, create_undo_state, restore_undo_state, self_is_mouse_over}` | the remaining Lua methods; `self_is_mouse_over` is Draw's `self:IsMouseOver()` (instance override, else EditControl's own, also through GemSelectControl's class proxy, Common.lua:180-188) |
| `ResizableEditControl::new(.., min_width, max_width, min_height, max_height: f64)` | `min_width, min_height, max_width, max_height: Option<f64>` (Lua's rect order 5..8; nil = the rect's width/height) | ResizableEditControl.lua:14-18; ConfigTab.lua:334 passes `{8, 0, 344, 118, nil, nil, nil, 118 + 16 * 40}` |
| `ResizableEditControl` Widget: Draw only | also delegates IsMouseOver/OnFocusGained/OnKeyDown/OnKeyUp/OnChar to EditControl's | inherited methods (the stub's defaults would raise the nil-method error) |

Hook set (ui doc §6.8 rule): no class hook table is needed. Subclasses (GemSelectControl,
ResizableEditControl) override only framework methods, and the one instance override in PoB
(`TradeQuery.lua:404` `fetchCountEdit:OnFocusLost`) is a framework method too; the full grep is
in the header of `edit.rs`.

## 3. The oracle's `lua-utf8` stand-in has no `utf8.next` (to U8 / pob-oracle owner; CCR-0033 item 4.2)

Evidence: `crates/pob-oracle/src/jit.rs:206-214` (PRELUDE) builds the module from `string.*`
plus `offset`/`codepoint`; `EditControl.lua:558`, `:572` (LEFT/RIGHT without ctrl) raise
`attempt to call field 'next' (a nil value)` under it. Workaround: the U11 gallery Lua files
install a port of luautf8's `Lutf8_next` (lutf8lib.c:1038-1043, `push_offset`/`utf8_offset`, for
the `utf8.next(s, i, off)` call shape with `i >= 1`) into the cached module table before any
EditControl runs. Proposed: the PRELUDE (and `crates/pob-dump/src/lua/prelude.lua`) gets the
same function, or CCR-0033 item 4's real module; then the shim blocks in
`lua/gallery/u11_*.lua` go.

CCR-0033 item 4.1 also still applies: real PoB has `main.unicode == true` (the module sets
`_G.utf8`), the oracle has `false`, so EditControls without a filter use `"^%w%p "` on both sides of
the galleries; the `"%c"` default is covered by the unit test `default_filter_follows_main_unicode`.

## 4. `Copy`/`OpenURL` side effects on the Rust side (CCR-0028 item 2)

EditControl calls `Copy` (L528) and `OpenURL` (L493) from inside its key handlers. pob-sg does not
record those side effects, nor does the oracle's Rust-side `boot::OraclePlatform`, while the Lua
bridge records them. Workaround: `crates/pob-ui-oracle/src/gallery/_u11.rs` wraps the gallery's
platform and moves the queued effects into the frame trace right after `ProcessControlsInput` and
`DrawControls`. Remove with CCR-0028 item 2 (record the effects in `Sys`/the platform).
