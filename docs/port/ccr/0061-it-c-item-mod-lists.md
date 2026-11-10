# CCR-0061 (IT-C): Item mod lists / itemLib contract notes

**Status: applied 2026-11-10 by CCR2 (§5, §7; calc doc §9.4, contracts.md#class-census); §1-§4, §6, §8 need code. See AMENDMENTS A8.**

**Filed status: proposed (IT-C, 2026-11-09).** Every item is worked around inside the IT-C files
`crates/pob-calc/src/classes/{item_mods,item_tools}.rs` (marked `CCR-0061`); no frozen file was
edited.

- **Raised by:** IT-C
- **Affects:** `crates/pob-calc/src/classes/mod.rs` (`Item.slotModList`; CCR-0030 #4),
  `crates/pob-calc/src/classes/mod_list.rs` (MS-A: ModList index API), `item_parse.rs` (IT-A:
  sockets reset), `probe.rs` (ORC-X: export of slot 3), CS2 (reads `slotModList[3]`), the UI port
  (`launch.devModeAlt`)
- **Pin:** 16de4b82d

## 1. `slotModList[3]` (CCR-0030 #4 is still open)

`Item:BuildModList` builds three slot lists for rings (Item.lua:2693-2700, "Ring 3"); the frozen
`Item.slotModList` is `[Field<Option<ModList>>; 2]`.

Workaround: the third list is stored in `Item.extra` under `"$slotModList3"` as
`Val::Obj(Rc<ModList>)`, read with `Item::slotModList3()` (item_mods.rs). `self.slotModList = { }`
clears all three. CS2 (CalcSetup.lua's "Ring 3" slot) must read `item.slotModList3()` until the CCR
is applied; ORC-X's exporter (`probe.rs` `PObj::SlotMods`) reads the probe registry's `slot3`, not
this key.

Proposed: apply CCR-0030 #4 (`slotModList: [Field<Option<ModList>>; 3]`); IT-C then drops the
`extra` key (two lines in `BuildModList`, one accessor).

## 2. ModList has no "remove / replace the mod at index i"

`calcLocal` removes matching mods from the list it is given (`t_remove(modList, i)`,
Item.lua:2213), and BuildModListForSlotNum edits the slot list's own copies in place
(`mod[1] = …`, `t_insert(mod, …)`, Item.lua:2336-2338; contracts.md#mod-mutation says "port as
replace the mod at its index in that (unaliased) list"). MS-A's public ModList API has `AddMod`,
`AddList`, `ipairs` and `wipe` only; the storage is private to MS-A's files.

Workaround: `item_mods::replace_list` rebuilds the list (`wipe`, restore `parent`/`actor`, `AddList`
of the kept or replaced mods, same order). The lists involved are fresh (`new("ModList")` in
BuildModList/BuildModListForSlotNum), so the rebuild is not observable; it costs one pass over the
list per `calcLocal` call that removes something.

Proposed (MS-A, `mod_list.rs`):

```diff
+impl ModStore {
+    /// `t_remove(modList, i)` (ModList; 1-based, LuaJIT bounds).
+    pub fn list_remove(&self, i1: usize) -> Option<ModRef>;
+    /// `modList[i] = mod` for an existing index (ModList).
+    pub fn list_set(&self, i1: usize, m: ModRef);
+}
```

## 3. `item.sockets.colourAlwaysMatches` reset (CCR-0019 §7 follow-up)

CCR-0019 §7 put the string key of the sockets list in `item.extra.colourAlwaysMatches`. In Lua the
key lives on the sockets *table*, so it disappears whenever the table is replaced or wiped:
BuildModList's `wipeTable(self.sockets)` (Item.lua:2645) and `self.sockets = newSockets` (2686)
— IT-C clears `extra.colourAlwaysMatches` there — and ParseRaw's `self.sockets = { }`
(Item.lua:486, IT-A's `item_parse.rs` line ~538), which does not clear it yet. Observable only for
an item object that is re-parsed after a parse that set the flag (re-parse of the same item with a
text that no longer has "All Sockets are White"/"SocketAlwaysMatches"): the stale `true` would
survive until BuildModList runs, which is the next statement that can set it, and BuildModList
never clears it on its own (Item.lua:2688 only sets it). Proposed: IT-A clears
`extra.colourAlwaysMatches` next to `self.sockets.set(Shared::new(Vec::new()))`; or apply CCR-0019's
`Item.socketsColourAlwaysMatches: Cell<bool>` together with the reset.

## 4. Globals `itemLib.formatModLine` reads (ItemTools.lua:361-382)

- `main.defaultItemAffixQuality`: read from IT-A's thread-local (CCR-0025 §1; no second copy).
- `main.notSupportedModTooltips` (Main.lua:119 `true`; the settings file and the options checkbox
  change it, Main.lua:651/1166): the thread-local `item_tools::main_notSupportedModTooltips()` /
  `set_main_notSupportedModTooltips(bool)` (default true); `main.notSupportedTooltipText`
  (Main.lua:120, never reassigned): the constant `item_tools::main_notSupportedTooltipText`.
- `launch.devModeAlt` (Launch.lua:122, `self.devMode and IsKeyDown("ALT")`): UI state; the port has
  the thread-local `item_tools::launch_devModeAlt()` / `set_launch_devModeAlt(bool)` (default false,
  as in a non-dev launch). Proposed: move these two next to CCR-0025's `MainOptions` in `module_state.rs`
  (owned by U0, set by the UI).

## 5. `itemLib.wiki` is UI

`itemLib.wiki` (ItemTools.lua:384-413) opens poewiki.net (`OpenURL`) on F1; it is marked
`@pob-skip` in item_tools.rs (the UI port owns it, like `DrawViewer` in CCR-0032 §1).

## 6. Test seam: `item_tools::replay` (feature `probe`)

`applyRange`'s fallback path (ItemTools.lua:324) and `getRangedModList` (Item.lua:2527) call
`modLib.parseMod`. While MP3-MP7 are unported the IT-C oracle test replays PoB's parser results:
`item_tools::parseMod` consults `item_tools::replay::Hooks.parseMod` under the `probe` feature (the
same shape as CCR-0025 §3's `item_parse::replay`; IT-A's `replay::get` is private to its module, so
IT-C cannot share it). Without the feature, or with no hook set, the real parser runs. Proposed:
keep until ORC-X's generic replay can swap callees by function id.

## 7. Small notes

- `BuildModListForSlotNum(baseList)` with no slot number (every item that is not a weapon or a
  ring): `slotName:gsub("1", tostring(slotNum))` turns `"Flask 1"` into `"Flask nil"` and
  `"Graft 1"` into `"Graft nil"` (`slotNum ~= 1` is true for nil). Ported literally: the mods'
  `sourceSlot` and the `Multiplier:QualityOnFlask nil` names follow PoB.
- `getRangedModList` returns `Option<Vec<Mod>>` (U0's signature): Lua's `nil` (no range),
  `false` (`not extra and list` with an `extra`) and a nil `list` all map to `None` — the caller
  only tests truthiness; `{ }` (zero-value line) is `Some(vec![])`, truthy as in Lua.
- Tincture `manaBurn` divides by `1 + calcLocal(..., "MORE", 0) / 100` where the MORE result is
  already a multiplier (`1.0` when there is none): `(1 + 1/100)`. Ported literally.

## 8. Found by IT-C, not IT-C's: `Item:Craft` with a fractional stat order (IT-B)

With `applyRange` ported, IT-B's oracle test compares `Craft` for real (it counted the comparison
as skipped while applyRange was a stub). 206/207 crafted items match; `rare#195` (a magic Divine
Life Flask with suffix `FlaskBleedCorruptingBloodImmunity3`) fails: the mod's `statOrder` is
`929.1`, and `Craft` keys its local `statOrder` table by it (Item.lua:2086/2102,
`statOrder[order]`), which a calc `Table` refuses ("non-integral number keys are not supported by
Table (929.1)"; `Key::from(f64)`, lua_table.rs). PoB accepts any number key. Proposed (IT-B /
U0): either `Craft` keeps its `statOrder` in a Rust map keyed by the `f64` bits (it is a local,
never exposed), or `Key` gains a float variant. Not worked around here (item.rs is IT-B's).
