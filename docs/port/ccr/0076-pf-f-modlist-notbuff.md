# CCR-0034: PF-F — `modList.notBuff` on a ModList object (CCR-0030 row 7, made concrete)

Status: proposed by PF-F (2026-11-09)

## Problem

`calcs.perform` marks some buff ModLists as "not a buff" by setting a key on the ModList
*object* and reads it back when it applies the buffs:

| Site | Code |
|---|---|
| CalcPerform.lua:2331 (PF-E) | `buffs[buff.name].notBuff = true` |
| CalcPerform.lua:2810, 2834 (PF-E) | same, on `buffs[…]` / `minionBuffs[…]` |
| CalcPerform.lua:3346 (PF-F) | `if not modList.notBuff then modDB.multipliers["BuffOnSelf"] = … + 1 end` |

The frozen `ModStore` (calc doc §5.3, `crates/pob-calc/src/classes/mod.rs`) has no field for
it and no `extra` table. ORC-X keeps the injected value in the probe registry
(`Registry::side_extra`, private to `crates/pob-calc/src/probe.rs`, CCR-0030 row 7), which the
calc cannot (and must not) read. So PF-F cannot see the flag, and PF-E has nowhere to write it.

Evidence in the corpus (`perform:2997` dumps of `corpus/PF-F/stages.toml`):
`allflame_exp_002` has `env.buffs["Blood Rage"].notBuff = true`; PoB's
`env.modDB.multipliers.BuffOnSelf` at `perform:3497` is 7, the port's is 8. The same happens
on `allflame_exp_004` (6 vs 7).

## Proposed diff (calc doc §5.3 and the frozen struct; CCR-0030 row 7)

```diff
 // crates/pob-calc/src/classes/mod.rs
 pub struct ModStore {
     pub kind: StoreKind,
     pub(crate) mods: RefCell<StoreMods>,
     pub parent: Field<Option<ModStoreRef>>,
     pub actor: Field<ActorWeak>,
     pub conditions: Table,
     pub multipliers: Table,
+    /// `modList.notBuff = true` (CalcPerform.lua:2331/2810/2834; read 3346). Lua nil = false.
+    pub notBuff: Cell<bool>,
 }
```

with `Default`, `ModStore::ModDB` and `ModStore::ModList` initialising it to `false`, ORC-X's
inject setting it from a store's `notBuff` key (instead of the registry) and the exporter
writing `"notBuff": true` when set (absent otherwise, as Lua's nil).

## Workaround until applied (PF-F, marked `// CCR-0030`)

* `party_curses.rs::notBuff` reads `notBuff` from a table-shaped buff and is `false` for a
  ModList object.
* `crates/pob-oracle/tests/PF-F.rs` counts the `:in` buffs with `notBuff = true` and accepts a
  `BuffOnSelf` mismatch of exactly that size as known (reported as `known:` lines, not hidden).

When the field exists, PF-F's `notBuff` becomes `store.notBuff.get()` for a store and the
test's allowance is removed.
