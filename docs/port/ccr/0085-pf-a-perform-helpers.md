# CCR-0085: PF-A — `mergeBuff` source type, ModList index store, `setSource` on a shared mod

Status: proposed by PF-A (2026-11-09)

Unit PF-A ports CalcPerform.lua 1-635 into
`crates/pob-calc/src/modules/calc_perform/actor_a.rs`. Three contract gaps, each worked around
in that file and marked `// CCR-0085`.

## 1. `mergeBuff(src: &Table, …)` cannot receive a ModList

**Problem.** The frozen stub is `mergeBuff(src: &Table, destTable: &Table, destKey: Name)`.
Almost every caller passes a **ModList object** as `src`:

- `local srcList = new("ModList")` … `mergeBuff(srcList, flaskBuffs, baseName)`
  (CalcPerform.lua:1729-1741, 1756-1768, 1883-1886, 1899-1902; PF-C);
- `mergeBuff(srcList, buffs, buff.name)` and friends (2329-2990; PF-E), 3004-3113 (PF-F).

Only `mergeBuff(buff.modList, …)` (2808, 2832) and `mergeBuff(guard.modList, …)` (3339)
pass a plain list of mods. In Rust a ModList is a `ModStoreRef`, which is not a `Table`.
`destTable[destKey]` likewise holds a ModList (`new("ModList"):ModList()`, L47), stored in the
dynamic table as `Val::Obj(ModStoreRef)`.

**Proposed diff** (actor_a.rs; also the calc doc §2.1 stub signature):

```diff
-pub(crate) fn mergeBuff(src: &Table, destTable: &Table, destKey: Name)
+pub(crate) fn mergeBuff(src: &Val, destTable: &Table, destKey: Name)   // a ModList obj or a list of mods
```

**Workaround.** `mergeBuff_v(src: &Val, …)` holds the body; the frozen `mergeBuff(&Table, …)`
delegates to it with the table as a plain list of mods. Either form works: PF-C/PF-E/PF-F
(written against the stub) currently copy the `srcList` mods into a list table and call the
frozen `mergeBuff`; `mergeBuff_v(&Val::Obj(srcList), …)` avoids that copy. Both read the source
mods in order, so the merged result is the same. `destTable[destKey]` is always created as a
`Val::Obj` ModList (L47), which is what those units read back with `obj::<ModStore>()`.

## 2. `dest[index] = mod` on a ModList has no public API

**Problem.** `mergeBuff` replaces a mod in place (`dest[index] = mod`, CalcPerform.lua:56)
when a stronger instance of the same mod arrives. `ModStore` (MS-A) has no public
"store at index" for a ModList; `mod_list.rs`'s `list_set` is private.

**Proposed diff** (crates/pob-calc/src/classes/mod_list.rs, MS-A):

```diff
+impl ModStore {
+    /// `self[i] = mod` on a ModList (1-based `i`, an existing slot).
+    pub fn list_set_at(&self, i1: usize, m: ModRef) { self.list_with(|l, idx| list_set(l, idx, i1 - 1, m)) }
+}
```

**Workaround.** actor_a.rs writes `list[index - 1] = mod` through the crate-visible
`ModStore.mods` storage. `compareModParams` has already required equal names (ModTools.lua:122),
so the name → positions index stays valid (debug-asserted).

## 3. `applyEnemyModifiers` calls `modLib.setSource` on a shared mod (§3.6 audit gap)

**Problem.** calc doc §3.6 and contracts.md#mod-mutation list ModTools.lua 240-242 as
"`setSource` on fresh copies". At CalcPerform.lua:629 it is not:

```lua
local mod = value.value and value.value.mod          -- 626: the inner mod of the actor's
...                                                   --      EnemyModifier LIST mod (shared)
local source = mod.source or value.mod.source        -- 628
enemyDB:AddMod(modLib.setSource(mod, source))        -- 629: mutates it, adds the same table
cache[mod] = true                                    -- 630: keyed by its identity
```

When the inner mod has no source of its own, Lua writes `mod.source` (and
`mod.value.mod.source` for a nested payload) into the table that also lives inside the
actor's `EnemyModifier` mod, and the enemy's ModDB holds that very table.

**Workaround.** `setSource_shared`: when the assignment changes nothing (the inner mod already
has that source), the very `Arc` is added to the enemy DB, as Lua does; otherwise a re-sourced
copy is added. The identity cache is keyed on the original inner `Arc` (what Lua keys on).
Observable difference: the actor's own `EnemyModifier` payload keeps `source = nil` where PoB's
shows the copied source (state dumps of `actor.modDB` only; no calc reads it, and the enemy DB
mod is identical in value).

**Proposed resolution.** Record the site in contracts.md#mod-mutation as "shared, ported as a
copy (CCR-0085)". Making it exact would need the actor's mod rebuilt with the re-sourced
payload (changing the identity the cache keys on), which no output depends on.

## 4. `actor.appliedEnemyModifiers` keys (CCR-0030 #6)

CCR-0030 #6 already proposes `Field<Option<Shared<ModPtrSet>>>`. Until it is applied,
actor_a.rs keeps the identity set beside the frozen `Table` (a thread-local list of
`(Weak<Actor>, table, mods)`, entries dropped when the actor dies or the cache is reset), and in
probe builds mirrors each key into the probe registry (`push_obj_key`) so the state export shows
the `$keys` PoB dumps; an injected cache (keys decoded afresh by `pob_oracle::inject`, identity
lost) is matched by value.

## 5. Oracle gap: pob-state/1 items carry no mod lines (for P3 / ORC-X)

**Problem.** An `ItemCondition` tag with `searchCond` (ModStore.lua:714-719) calls
`item:FindModifierSubstring(...)`, which searches the item's `*ModLines`
(Item.lua, IT-B). pob-state/1 dumps items field-wise (`modList`, `slotModList`,
`weaponData`, … per calc doc §9.2) without `explicitModLines` / `implicitModLines` /
`enchantModLines` / …, so on an injected item the search finds nothing and the tag evaluates
differently from PoB. Seen in the PF-A function probes on `allflame_exp_016`
(`15% increased Life`, Tree:41415, tag `{ type = "ItemCondition", itemSlot = "body armour ",
searchCond = "life", neg = true }`; Kaom's Heart): Rust Life 9762, PoB 9228; and on
`allflame_exp_049` (the same tag on `15% increased Life`, Tree:44316; the body armour's
`Regenerate 10% Life over one second when Hit while Sane`): Rust Life 3140, PoB 2909. Every
other all57 pair is exact, so these two builds (doActorLifeMana, and doActorAttribsConditions,
which ends by calling it) are the only exclusions (listed in `KNOWN_DIFFS` of
`crates/pob-oracle/tests/PF-A.rs`, which fails when an entry goes stale).

**Proposed resolution.** state_dump.lua / `pob_oracle::inject` dump and inject the item mod-line
lists (`{ line = …, … }` tables), or inject items by re-parsing `item.raw` once IT-A/BM-A land.

## 6. getCachedOutputValue acceptance needs a buildActiveSkill replay (note for CA / ORC-X)

Not a contract change; recorded because the PF-A gate depends on it. `getCachedOutputValue`
calls `calcs.buildActiveSkill` (CA, still a stub) when the skill's GlobalCache entry is absent,
which is the case in the corpus probes. The PF-A test (`tests/PF-A.rs`,
`replay_build_active_skill`) replays that callee as the `getCachedOutputValue:in → :out` delta
(`pob_oracle::replay`) and fills the outputs the function reads from the cached entry's env from
PoB's return values, because the entry's `.Env` is another env that pob-state/1 dumps only as a
`$env` marker. It also rebuilds `build.skillsTab.socketGroupList` from the build XML (the build
loader, BM-A, is a stub), since `cacheSkillUUID` numbers groups by that list; a wrong index shows
as a missed GlobalCache key. The nested initEnv re-points build-model gem instances
(`displayEffect`, D13), which the delta replay applies in place; the resulting `$ref` identity
differences are counted and excluded for this function only. What the gate checks for this
function is therefore the uuid, the lookup, the unpacking and the absence of other side effects.
Once CA lands, the test can run the real `buildActiveSkill`.
