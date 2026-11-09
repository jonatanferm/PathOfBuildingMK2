# CCR-0079: PF-C — mergeFlasks/mergeTinctures upvalues, callee replay, L4 test-environment gaps

Status: proposed by PF-C (2026-11-09)

Unit PF-C ports `calcs.perform` segment 1289-1942 (`calc_perform/setup_flasks.rs`). Items §1 and
§6 are gaps in U0's frozen contracts that the port works around locally (`// CCR-0079`); §2-§5
are gaps in ORC-X's oracle/inject/export that block an exact L4 run of this (and every other)
segment; §7 is CCR-0030 #5 biting.

## §1 The crossing local functions carry upvalues the ctx does not hold

`mergeFlasks` (CalcPerform.lua:1691) and `mergeTinctures` (1862) are closures over segment
locals that the §7.1 audit does not count, because only the *function* local crosses:

| Closure | Upvalues (declaration line) |
|---|---|
| `mergeFlasks` (+ `getFlaskInstantRecovery` 1638, `calcFlaskRecovery` 1652, `calcFlaskMods` 1701) | `effectInc` 1602, `effectIncMagic` 1603, `effectIncMagicNoAdjacent` 1604, `effectIncNonPlayer` 1605, `effectIncMagicNonPlayer` 1606, `flasksApplyToMinion` 1607, `quickSilverAppliesToAllies` 1608, `flaskTotalRateInc` 1610, `flaskDurInc` 1611 (plus `env`, `modDB`, `nonUniqueFlasksApplyToMinion`, which are ctx fields) |
| `mergeTinctures` (+ `calcTinctureMods` 1872) | `effectInc` 1851, `effectIncMagic` 1852 (both **shadow** the flask locals of the same name), `tinctureLimit` 1853, **mutated** by the closure (1909) |

PF-F calls `mergeFlasks` at 3121 and PF-D calls `mergeTinctures` at 2095, so the values must
survive from 1611/1853 to those calls; recomputing them there would be wrong (PF-E adds
`FlaskEffect`/`TinctureEffect` mods in between). The frozen signatures
`mergeFlasks(ctx: &PerformCtx, …)` / `mergeTinctures(ctx: &PerformCtx, …)` have nowhere to get
them from. The oracle cannot dump them either: at `perform:1943` the flask `effectInc` is
shadowed by the tincture one (no name reaches it), and closures' upvalues are unreadable
without `debug` (CCR-0010).

Proposed diff (calc doc §7.2 and `modules/calc_perform/mod.rs`): the ctx fields named after the
function locals hold the closures' captured values.

```diff
 ctx_struct! { pub struct PerformCtx {
     …, nonUniqueFlasksApplyToMinion: bool,
+    mergeFlasks: Option<Rc<setup_flasks::FlaskUpvalues>>,      // closure at 1691
+    mergeTinctures: Option<Rc<setup_flasks::TinctureUpvalues>>, // closure at 1862 (tinctureLimit: Cell<f64>)
     buffs: Table, …
 }}
```

plus two line probes for the L4 tests of PF-D/PF-F, which then seed the fields:
`perform:1691` (before the `local function mergeFlasks` line; locals = the nine flask upvalues,
all still unshadowed there) and `perform:1862` (locals `effectInc`, `effectIncMagic`,
`tinctureLimit`), with `ProbeState` mapping them to the two structs.

Workaround (PF-C, `// CCR-0079 §1`): `setup_flasks.rs` records both upvalue sets per env in a
thread-local side table (`MERGE_UPVALUES`, keyed by a `Weak<Env>`) where Lua creates the
closures; the frozen `mergeFlasks`/`mergeTinctures` look them up and raise a `LuaError` naming
this CCR when the segment has not run for that env (e.g. a PF-D/PF-F L4 test seeded from a
dump).

## §2 No hook for replaying a not-yet-ported callee in the middle of a segment

Calc doc §9.2 ("Replay … swaps a not-yet-ported callee for its recorded in→out state delta")
and the unit rows ("createMinionSkills replayed until AS lands") assume calc code can hand a
callee call to the test. `pob_oracle::replay::Replay::apply_report(hit, env)` exists, but
nothing in pob-calc lets the test run it at the call site: a call into a stub just returns
`Default` (D9).

Proposed (ORC-X, `crates/pob-calc/src/probe.rs`): a probe-feature hook

```rust
pub type CalleeHook = Box<dyn FnMut(&'static str /*callee*/, &[Val] /*args*/)>;
pub fn set_callee_hook(h: Option<CalleeHook>);
/// f(), then — when f only hit the stub `site` — the hook (no-op without the feature)
pub fn callee(site: &'static str, name: &'static str, args: impl FnOnce() -> Vec<Val>, f: impl FnOnce());
```

and `testkit::segment_diff_with_replay(build, stage_in, stage_out, callees)` which loads the
callees' `:in/:out` dumps made directly by the same function call (same `stack`/`$env` as the
`stage_in` dump) and replays them in call order.

Workaround (PF-C): `setup_flasks::callee_replay` (feature `probe` only) implements exactly that
for this file; `tests/PF-C.rs` installs the hook.

## §3 The pob-state/1 item dump leaves out `item.buffModList`

`crates/pob-oracle/lua/state_dump.lua:673-675` (`itemFields`) dumps `modList`, `slotModList`,
`weaponData`, `armourData`, `flaskData`, `tinctureData`, `jewelData`, but not `buffModList`,
which the segment reads for every flask and tincture (CalcPerform.lua:1812, 1815, 1913;
Item.lua builds it). Injected flasks therefore have `buffModList = None` and every L4 run of a
build with a flask raises `attempt to index field 'buffModList'`.

Proposed: add `"buffModList"` to `itemFields` and map it in `inject::fill_item` /
the exporter's Item reflection (`Item.buffModList` is already typed).

Workaround (PF-C test only): `corpus/PF-C/stages.toml` adds a line probe `pfc:calcFlaskMods`
at CalcPerform.lua:1702 dumping `item` and `buffModList`; the test restores each flask's list
by item id before running the segment, and again after every callee replay: `Replay::apply`
writes the merged export back with `inject::apply_onto`, which refills items from the export
and so drops any field the exporter does not carry (a general replay hazard for every
non-exported field, not only this one).

## §4 The injected graph has no passive tree

`modLib.mergeKeystones` (CT, ported) reads `env.spec.tree.keystoneMap[name].modList`
(ModTools.lua:248-259). Injected nodes are `Node` stubs resolved against the build model's spec
(`inject::node`), and the model is empty (`Build:LoadDB` is a stub, BM-A; PassiveTree is PT), so
the keystones are never found and every keystone build differs at `perform:1943`
(`env.keystonesAdded[…]`, the keystone mods in `env.modDB`).

Proposed: once PT lands, `testkit` loads `data/dump/trees/<treeVersion>` into the empty build's
spec (the dump's `env.spec` carries the tree version), so `$treeNode`/`$node` and
`tree.keystoneMap` resolve; until then, dump `env.spec.tree.keystoneMap` entries' `modList` (the
`$treeNode` marker could carry them).

Workaround (PF-C, `// CCR-0079 §4`): with the `probe` feature the segment also reports each
`mergeKeystones` call to the hook (`merge_keystones` in `setup_flasks.rs`), and the test replays
its recorded delta (function probe `mergeKeystones` in `corpus/PF-C/stages.toml`).

## §5 Writes to typed fields keep the injected "absent" mark

`inject` marks typed fields that were nil in the dump as absent; the exporter leaves out an
absent field whose value is the default (`probe.rs` `struct_members`). A segment that assigns
such a field a default-looking value — `env.enemy.output = {}` (CalcPerform.lua:1306),
`env.keystonesAdded = { }` (1291), `minion.modDB = new("ModDB")` (1298) — is exported as if
the key were still nil, so the diff reports `Missing env.enemy.output`.

Proposed: `Field::set` cannot know about the registry, so either (a) the exporter treats a
typed field as present when its value is not pointer-equal to the value injected (the registry
keeps that pointer), or (b) the contract says segments call `probe::present(obj, key)` after
assigning a typed field (feature `probe`; a no-op otherwise).

Workaround (PF-C): `setup_flasks::present(rc, key)` (b) after the seven typed-field
assignments of the segment.

## §6 `mergeBuff(src: &Table, …)` vs the Lua ModList

Every caller passes a `new("ModList"):ModList()` (CalcPerform.lua 1731-1768, 1885-1902, and
PF-E's buff merges), and `destTable[destKey]` is created as a ModList (47) that callers then
`AddList` (1826, 1836, 1844, 1919) or `ipairs` (1923). The frozen `mergeBuff(src: &Table,
destTable: &Table, destKey: Name)` (actor_a.rs, PF-A) takes a table, and the representation of
the merged entry in `destTable` is not specified.

Proposed: `mergeBuff(src: &[ModRef], destTable: &Table, destKey: Name)`, with
`destTable[destKey]` a `Val::Obj(ModList)` (the PartyTab convention, CCR-0019).

Workaround (PF-C): the segment passes `src` as an array table of `Val::Mod` (all mergeBuff
reads is `ipairs(src)`) and reads merged entries as either a ModList object or an array table
of mods (`buff_mods`).

## §7 `env.flaskSlotMap[item]` (CCR-0030 #5)

CalcPerform.lua:1710 reads `env.flaskSlotMap[item]` for magic flasks when
`MagicFlaskNoAdjacentEffect` ≠ 0 (Essence of Desolation). The frozen `Field<Table>` cannot be
keyed by items. PF-C raises a `LuaError` citing CCR-0030 #5 on that path instead of silently
skipping the bonus; it ports as soon as CCR-0030's `Field<Shared<PtrMap<Item, f64>>>` lands.

## §8 Callees that landed for real: fresh skills' typed `bool`s and buff mod-list order

Since AS landed, `createMinionSkills` runs for real inside the segment (the replay hook only
fires on a stub hit, §2), and the skills it creates expose two export/diff gaps:

* `ActiveSkill.{buffSkill, debuffSkill, totemBuffSkill, minionBuffSkill}` and
  `SkillEffect.{superseded, appliesToGrantedSkills, fromItem}` are typed `bool`s, `false` on a
  fresh object, where PoB's tables leave them nil (CalcActiveSkill.lua:85-93, 900-904). Lua only
  assigns `true` to the four skill flags (CalcPerform.lua:2323-2958) and every reader tests
  truthiness, so the calc semantics agree; but the exporter drops a default-valued typed field
  only when it is marked absent, and only injection marks. Proposed (ORC-X / U0): a fresh
  `ActiveSkill::new` / `SkillEffect::new` marks the fields its Lua table leaves nil absent
  (feature `probe`), with explicit `superseded = false` (CalcSetup.lua:1691) unmarking.
  Workaround (test only): `PF-C.rs::mark_fresh_skill_defaults` marks them on the skills not in
  the injected graph, as `AS.rs::fix_absent_marks` does for fresh minions.
* A buff's `modList` (CalcActiveSkill.lua:804-869) is a plain array, positionally diffed, whose
  order inside one source follows `pairs(stats)` in `mergeSkillInstanceMods`
  (CalcActiveSkill.lua:62), random per PoB process (calc doc §3.4). Workaround (test only):
  `PF-C.rs::buff_list_order_only` counts a list as LUA-ORDER when both sides hold the same
  multiset of mods with the same source at every position. Proposed (ORC-X): the nondet record
  or the diff treats `*.buffList[*].modList` like a ModList (multiset + order check).
