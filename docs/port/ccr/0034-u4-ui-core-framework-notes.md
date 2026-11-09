# CCR-0034: U4 (ui-core framework) contract notes (ui doc §6.5-§6.7, §6.11-§6.13)

(Renumbered from 0023 at merge: 0023 is U3's images CCR.)

**Status: proposed.** Unit: U4 (ui doc §12.3). Every item is already implemented in U4's own files
(`crates/pob-ui-core/src/{cx,arena,input,coroutine,undo,main_core,focus}.rs`); the orchestrator only
needs to amend the doc text. All items are additive or clarify behaviour the doc leaves open; no
frozen signature changes, no other unit's file is touched, nothing in the calc-plan crates changes.

## A. Corrections to reference bodies

### A1. §6.12 `reset_undo` evaluates `CreateUndoState` after the wipes

Evidence: `UndoHandler.lua:25-29`:

```lua
function UndoHandlerClass:ResetUndo()
	self.undo = wipeTable(self.undo)
	self.redo = wipeTable(self.redo)
	self.undo[1] = self:CreateUndoState()
end
```

The §6.12 body calls `create` first, so a `CreateUndoState` that reads `self.undo` (or anything the
wipe changes) would see the old stacks. `tests/tier0_undo.rs` records `#self.undo` inside the state
and catches the difference. Proposed (as implemented):

```diff
     pub fn reset_undo(&self, cx: &mut Cx, me: CtrlId) {
-        let s = (self.create)(cx, me);
-        let st = (self.stack)(cx, me); st.undo.clear(); st.redo.clear(); st.undo.push_front(s);
+        { let st = (self.stack)(cx, me); st.undo.clear(); st.redo.clear(); }
+        let s = (self.create)(cx, me);
+        (self.stack)(cx, me).undo.push_front(s);
     }
```

### A2. §6.7 `InputEvents` is a `pob_lua::LjIntMap<InputEvent>`

The frozen private fields (`slots`, `asize`) are replaced by the calc plan's exact LuaJIT 2.1 table
model (`pob_lua::ljtab::LjIntMap`: array part sizing, hash part, rehash, dead keys). Public API
unchanged. `consume(i)` on a key that is not in the table inserts a dead key as `lj_meta_tset` does
(LuaJIT `t[k] = nil` creates the node), which can move later `#t` borders; PoB never does it.
`tests/tier0_len.rs`: 2000 random push/consume/wipe/fresh sequences (77 912 operations) and 300
PoB-style frames against LuaJIT with `jit.off()`, 0 divergences.

### A3. §6.13 `Coroutine::resume` return value

Doc: "Returns false once dead." Implemented as Lua's `res`: `true` when the body ran (to its next
yield or to its end, so the call that finishes the body returns `true` and `is_dead()` is then
true), `false` when the coroutine was already dead. A panic inside leaves the coroutine dead and
keeps unwinding (the `lua_error!` payload is the message).

## B. Additions (all in U4-owned files)

| Item | Where | Why |
|---|---|---|
| `Coroutine::try_resume(&mut self, cx) -> (bool, Option<String>)` (not on wasm32) | coroutine.rs | `local res, errMsg = coroutine.resume(co)` exactly: a Lua error inside the body is returned, not raised (CalcsTab.lua:482-485, ItemDBControl.lua:314, NotableDBControl.lua:204, GemSelectControl.lua:513, CompareTab.lua:3149 only rethrow in dev mode). |
| `co::running() -> bool` | coroutine.rs | `coroutine.running()` as a condition (CalcsTab.lua:514, :655/:700/:728, Common.lua:646, CompareTab.lua:2704 ff.). True while any `resume` is on the stack, also inside `with_cx`. |
| `co::any_alive() -> bool` | coroutine.rs | `coroutine._list()` (Common.lua:46-57) for SG frame pacing / `App::wants_frames()` (§6.13). Counts coroutines created and not dead (finished, failed or dropped). |
| `MainCore::new(node: CtrlId) -> MainCore` | main_core.rs | The `main:Init` defaults of the §6.11 fields (Main.lua:65, :108-125; `unicode` false until U16 runs Main.lua:309). |
| `#[doc(hidden)] Cx::headless() -> Cx` | cx.rs | A `Cx` with inert Platform/Fs/Net/ImageLoader and `FontSet::empty()`, for tests and galleries (the T0 suites, later widget units' unit tests). |

## C. Arena policies the doc leaves open (D7/D8), as implemented

- `add`: the node is created (creation seq), `inits` then `mixins` run, and only then is it stored
  under `key` (Lua evaluates `new(..):X(..){..}` before the table store). The doc's order
  "assigns the creation seq, runs inits, then mixins" is kept; the store position is now explicit.
- A control replaced under a key (`add`/`put`) is buried only when the host owns it and it is not
  still stored under another key of the host; otherwise it is only unlinked.
- `put` of a buried node revives it (a Lua reference keeps the object alive).
- `clear_children` (wipeTable(self.controls)) buries the children the host owns.
- `end_frame_gc` frees every buried node and, transitively, what it owns; it unlinks them from
  surviving hosts' tables and owners' lists and drops tab groups with no live member.
- DIVERGENCE(gc): an id still held by a `selControl`, a tab group or a widget field after its node
  was freed panics on use ("attempt to index a dead control (expected <Class>)"), where Lua's GC
  would keep the object alive. This only differs for objects the Lua code drops from every table
  but keeps using.
- Typed access to a node of another class panics with both class names
  (`"control … is a X, not a Y"`); `child_as` on a missing key panics like a nil index.
- Trace markers: `trace_push(host, id)` pushes `"<HostClass>.<key>"` (integer keys in decimal) only
  while `sg.trace` is set.

## D. T0 harness notes (for U8 and later units)

`crates/pob-ui-core/tests/lua/shim.lua` is the class shim (newClass/new/getClass, `__seq` stamping,
`__canonPairs`, `GetCursorPos`). The suites load the real `Control.lua`, `ControlHost.lua` (with
oracle patch 6 applied by text substitution: exactly two `pairs(self.controls)`),
`UndoHandler.lua`, and `Common.lua`'s own `wipeTable`/`isValueInArray`/`isMouseInRegion` lines, and
run LuaJIT with `jit.off()` (AMENDMENTS A6). The PoB checkout is found through `$POB_DIR` or the
nearest ancestor with `.PathOfBuilding/src`.
