# CCR-0085: PF-A — `mergeBuff` source type, ModList index store, `setSource` on a shared mod

**Status: applied 2026-11-10 by CCR2 (§3 contracts.md#mod-mutation, §6); §1, §2, §5 need code; §4 = CCR-0030 #6. See AMENDMENTS A8.**

Filed status: proposed by PF-A (2026-11-09)

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

**Workaround (revised in the PF-B fix round).** Mods are immutable `Arc`s, so the in-place
write is reproduced by replacement. `setSource_shared` returns the very `Arc` when the
assignment changes nothing, else the re-sourced copy `mod'`. When the Tabulate value is the
stored mod's own payload (`value.value.mod` is `value.mod.value.mod` by `Arc` identity; EvalMod
copies the payload only to scale it, `copyTable` at ModStore.lua:423/512/562, and then Lua mutates
that copy, which nobody else holds), `resource_payload` rebuilds every `EnemyModifier` mod of
the actor's ModDB and its parents whose payload is `mod` with `mod'` as payload, at the same
position. The enemy DB gets `mod'` and the cache holds both `mod` and `mod'` as identities
of the one Lua key (only `mod'` is exported): the Tabulate result is taken before the swap, so a
later entry of the same loop that holds the same payload (one EnemyModifier mod added twice,
e.g. the Config/Pantheon shape with outer source set and inner source nil) still holds `mod`,
and in Lua that is the mutated table and hits the cache. So, as in Lua, the
actor's EnemyModifier payload, the enemy DB mod and the cache key are one object; a later call
with the cache kept (doActorMisc, CalcPerform.lua:976) Tabulates `mod'` and hits the cache.

The first version added a copy and keyed the cache on the original payload. The actor's
payload then kept `source = nil` where PoB's shows the copied source (seen in the PF-B
doActorMisc probes: `LifeRegen` of `Pantheon:Soul of Ryslatha`, allflame_exp_030).

Remaining difference: Lua's write also reaches other holders of the same mod table outside the
actor's ModDB chain (e.g. the tree node's `modList`, which the next calc pass adds again). The
port leaves those holders unchanged. The next pass re-sources them again, with the same value.
Observable consequence: on calc pass 2 and later, a dump of `env.modDB` (or the tree, config or
item modLists) taken before `applyEnemyModifiers` shows the inner `source` in PoB (written by
pass 1) and `nil` in the port. A unit that probes those stores at later hits sees this as a
`source` mismatch on `EnemyModifier` payloads; that mismatch is this difference, not a bug in
that unit.

`resource_payload` also replaces the outer `EnemyModifier` mod `Arc` (a rebuilt copy with the
new payload, at the same position); Lua keeps the outer table and mutates only the inner one.
No port code keys on the outer mod's identity (the cache keys the inner mod), so this is not
observable today; a unit that keys on it files a CCR.

**Proposed resolution.** Record the site in contracts.md#mod-mutation (with the pass-2 note
above) as "shared, ported as a
replacement in the actor's ModDB chain (CCR-0085 §3)".

## 4. `actor.appliedEnemyModifiers` keys (CCR-0030 #6)

CCR-0030 #6 already proposes `Field<Option<Shared<ModPtrSet>>>`. Until it is applied,
actor_a.rs keeps the identity set beside the frozen `Table` (a thread-local list of
`(Weak<Actor>, table, mods)`, entries dropped when the actor dies or the cache is reset), and in
probe builds mirrors each key into the probe registry (`push_obj_key`) so the state export shows
the `$keys` PoB dumps; an injected cache (keys decoded afresh by `pob_oracle::inject`, identity
lost) is matched by value. The keys an injected table held when it was first seen are kept in
its entry, so they still match after the call adds keys of its own (the first version dropped
them once the call's first new key created the entry, so the cached mods listed after a new one
were added again: PF-B doActorMisc probes on allflame_exp_030/048). Keys added by the port
match by identity only, as in Lua. `tests/PF-A.rs` `pfa_apply_enemy_modifiers_filled_cache`
probes those later calls (applyEnemyModifiers hits 13-24 of allflame_exp_030/048) and fails
when none of them keeps a filled cache.

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
