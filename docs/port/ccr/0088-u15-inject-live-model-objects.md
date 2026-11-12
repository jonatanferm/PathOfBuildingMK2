# CCR-0088: U15 — inject bound to the live build model (socket groups, gem instances, items), several documents per registry

**Status: applied in part, rejected in part (CCRDOC, 2026-11-12). §2 is built (`inject::inject_keep`, ORC-X2 `d8a4407`), but the bridge does not use it (it still injects the main env's document last). §1 `inject_bound` is rejected because it was not built: the as-built design is the bridge's write-back and re-pointing (`pob-ui-oracle/src/lua_calc.rs` `adopt_build_output`, `Rebind`). Since U53 the bridge is dev-only (`POB_UI_CALC=lua`, `LuaCalcBridge::lua`; ui doc §10.4).**

**Previous status: needs code (CCR2, 2026-11-10; ORC-X2). See AMENDMENTS A8. ORC-X2 (2026-11-10) added `inject::inject_keep` (§2; U15's bridge is not switched to it yet); §1 `inject_bound` still needs code.**

Filed status: proposed by U15 (2026-11-10). Not blocking: U15 works around both items locally
(`crates/pob-ui-oracle/src/lua_calc.rs`, marked `// CCR-0088`).

## 1. Socket groups, gem instances and items are injected as copies

`LuaCalcBridge::build_output` (ui doc §10.4) dumps PoB's `calcsTab.mainEnv` / `calcsEnv` after
`BuildOutput` and injects them with `pob_oracle::inject::inject(dump, build)`. The dump resolves
`$object` Build / PassiveSpec and `$node` / `$treeNode` markers to the live build's objects
(`crates/pob-oracle/src/inject.rs:909-916`), but socket groups and gem instances are plain Lua
tables, so `activeSkill.socketGroup` and `skillEffect.srcInstance` become fresh `SocketGroup` /
`GemInstance` objects (`inject.rs:739-740`), not `build.skillsTab.socketGroupList[i]` and its
`gemList[j]`.

The UI needs the live ones: it writes through them (`srcInstance.skillPart = …`,
`Build.lua:494-566`; SkillsTab's group list compares groups by identity), and the Lua calc
itself writes into the build model (`CalcSetup.lua:1464-1530` inserts/rebuilds granted-skill
groups, `:1606`, `:1672-1913` write `displaySkillList`, `displayEffect`, `displayLabel`, …).

Items likewise: every `$item` becomes a fresh `Item::default()` (`inject.rs:707-711`), so
`env.flasks` / `env.tinctures` (`PtrMap<Item, bool>`) are keyed by copies and the UI's lookups
with the live item miss (`ItemsTab.lua:4868` `mainEnv.flasks[item]`, `:4904`
`mainEnv.tinctures[item]`); `actor.itemList`, `radiusJewelList[*].item`, the `sourceItem`s and
the breakdowns' `slots[i].item` hold copies too. The calc's writes onto the items
(`CalcSetup.lua:293-298` `item.jewelRadiusData`, `:852` / `:866` `item.jewelData.limitDisabled`)
reach only PoB's VM.

**Workaround (U15):** the bridge tags groups and gems with their load position before the first
calc pass (a wrapper around `calcs.buildOutput` in the bridge VM), dumps
`build.skillsTab.socketGroupList` plus the tags as locals, writes each dumped group / gem onto
the live object at that position (typed fields; dynamic-table groups through
`pob_calc::probe::set_member`), keeps new groups, drops removed ones, and then walks the injected
envs (actors → skills → effects, minions, `auxSkillList`, the groups' display lists) re-pointing
`socketGroup` / `srcInstance` at the live objects. Items: the same walk re-points every item
with an `id` at `build.itemsTab.items[id]` (`env.flasks` / `tinctures` keys, `aegisItem`,
`actor.itemList`, and items in dynamic tables: breakdowns, outputs, `radiusJewelList`,
`grantedSkills*`, `requirementsTableItems`, `explodeSources`, the groups' `extra`); items the
calc makes itself (no `id`, e.g. Energy Blade's, `CalcSetup.lua:1124`) stay copies. The items'
`jewelRadiusData` and (for jewels of allocated sockets) `jewelData.limitDisabled` are dumped as
locals and written onto the live items.

**Proposed diff** (ORC-X): an injection entry point that binds dumped tables to caller-supplied
live objects, so no copy is ever made:

```rust
/// Like `inject`, but every table of `bind` (a dump-local name → live object list, element i
/// bound to the i-th element of the dumped list) is the given object, refilled in place, and
/// every `$item` with an `id` resolves to `build.itemsTab.items[id]` (not refilled: the
/// encoder's item fields are a whitelist) instead of a fresh `Item`.
pub fn inject_bound(dump: &StateDump, build: &BuildRef, bind: &[(&str, Vec<PObj>)])
    -> Result<Injected, String>;
```

## 2. `inject` resets the probe registry per document

`inject` starts with `probe::registry_reset()` (`inject.rs:85`), so injecting the mainEnv and
calcsEnv documents one after the other loses the first one's registry side entries
(object-keyed table entries, `notBuff` side extras, `$foreign` placeholders' markers; CCR-0030).
In the adopted `calcsEnv` these are: `env.flaskSlotMap` (item-keyed),
`player.appliedEnemyModifiers`, `itemList[*].slotModList` (the `[3]` entry, CCR-0030),
`grantedSkills[*].sourceItem.slotModList`, and the side keys of the `explodeSources` entries.
No UI code reads them (`flaskSlotMap` and `appliedEnemyModifiers` are read only inside
`Modules/Calc*.lua`). The mainEnv document's side entries survive until the next `inject`
(a `MiscCalc::call` resets the registry too), which is equally unobserved by the UI. The registry's absent-field markers are lost the same way, so a
live gem/group adopted from an earlier document exports typed defaults (`false`/`0`) where PoB
has nil (the U15 tests excuse that for typed fields only).

**Workaround (U15):** the main env's document is injected last.

**Proposed diff:** `inject_keep(dump, build)` — `inject` without the registry reset (the caller
resets once).
