# CCR-0091: PF-B — untyped minion fields, `Override`'s zero-value return, crate-private helpers in tests

Status: proposed by PF-B (2026-11-09)

## 1. Minion actor fields with no typed `Actor` field

`initMinionModDB` / `addMinionModifiers` (CalcPerform.lua 1154-1253) read four minion-table
fields that CalcActiveSkill writes and the frozen `Actor` (`crates/pob-calc/src/actor.rs`) cannot
hold as a typed field:

| Field | What PoB stores | Written | Read by PF-B |
|---|---|---|---|
| `minion.type` | the minion id string (`"RaisedSkeleton"`, …) | CalcActiveSkill.lua:707 | CalcPerform.lua:1205, 1249 |
| `minion.lifeTable` | one of `data.monsterLifeTable`, `…2`, `…3`, `data.monsterAllyLifeTable` (a level-indexed sequence) | CalcActiveSkill.lua:725-727 | CalcPerform.lua:1160, 1166 |
| `minion.itemSet` | an `itemsTab.itemSets[id]` table | CalcActiveSkill.lua:740, 745 | CalcPerform.lua:1222, 1226-1230 |
| `minion.uses` | `activeGrantedEffect.minionUses` (a data set of slot names); typed `Cell<Option<f64>>` today — CCR-0030 item 2 | CalcActiveSkill.lua:723 | CalcPerform.lua:1222, 1224 |

Inject puts all four in `Actor.extra` (`uses` after marking the typed field absent).

**Workaround (`// CCR-0091` in `calc_perform/actor_b.rs`):** PF-B reads them from `minion.extra`:
`type` as a string, `lifeTable` / `uses` / `itemSet` as indexable values (`Val::get`). The PF-B
test converts the dumped `$data` markers of `lifeTable` and `uses` (inject keeps them as opaque
`pob_calc::probe::DataRef`s) to the Lua tables they stand for before the run and restores them
before the export.

**Proposed diff** (`actor.rs`, `probe.rs` reflect list; AS writes them):

```rust
pub struct Actor {
    …
    /// `minion.type` (CalcActiveSkill.lua:707).
    pub r#type: Field<Option<Name>>,
    /// `minion.lifeTable` (CalcActiveSkill.lua:725-727): one of the `data.monster*LifeTable`s.
    pub lifeTable: Cell<Option<&'static [f64]>>,
    /// `minion.itemSet` (CalcActiveSkill.lua:740/745).
    pub itemSet: Field<Option<Table>>,
    /// `minion.uses` (CalcActiveSkill.lua:723) — replaces `uses: Cell<Option<f64>>` (CCR-0030 §2).
    pub uses: Field<Option<pob_gamedata::NameSet>>,
}
```

and in `probe.rs`: `"type" => r#type, "lifeTable" => lifeTable, "itemSet" => itemSet`, a
`ProbeField` for `Option<&'static [f64]>` that maps the four `data.monster*LifeTable` paths, and
`uses` as a data set. When it lands, `minion_type` / `minion_lifeTable` / the two `extra` reads in
`actor_b.rs` switch to the typed fields and the test adapter goes away.

## 2. Note for every unit: `Override` returns *no value* when nothing matches

`ModDBClass:OverrideInternal` / `ModListClass:OverrideInternal` (ModDB.lua:229-251,
ModList.lua:177-197) fall off the end when no mod matches and there is no parent, so
`modDB:Override(...)` then returns **zero values**, not `nil`. In an argument list this changes
the call: `m_max(modDB:Sum("BASE", nil, "SelfChillOverride"), modDB:Override(nil, "ChillVal"))`
(CalcPerform.lua:870/884/894) is `math.max(sum)` = `sum`, whereas `math.max(sum, nil)` would
raise "bad argument #2 to 'max'". The PF-B oracle run shows it (three player/enemy cases raised
until the port followed it). PF-B ports it as `lua_max_override` (`// LUA-QUIRK`). Proposed:
add to calc doc §3.2 a row "`f(a, modDB:Override(…))` as a trailing argument: an unmatched
Override passes no argument (`OverrideInternal` has no final return)". `Max`/`Min` always return
a value (`return max`), so they are not affected; `Flag` returns `true` or no value but is only
used in boolean position.

## 3. Crate-private helpers are unreachable from `crates/pob-oracle/tests/<unit>.rs`

U0 declared the PF-B helpers (`doActorMisc`, `doActorCharges`, `initMinionModDB`,
`addMinionModifiers`, `allyLifePools`) `pub(crate)`, but the unit's acceptance test (row PF-B:
"function probes") lives in pob-oracle (calc doc §2 rule 2) and `pob_calc::probe::run_segment`
only dispatches line-probe segments. **Workaround:** `actor_b.rs` has a
`#[cfg(feature = "probe")] pub mod probe_fns` with same-signature wrappers (no logic). PF-A
(`getCachedOutputValue`, `mergeBuff`, `doActorAttribsConditions`, `determineCursePriority`,
`applyEnemyModifiers`) has the same gap. **Proposed:** ORC-X adds
`pob_calc::probe::run_function(id, locals) -> Result<Vec<Val>, String>` (function probe id →
the Rust fn, args from the probe's `locals`, results as the packed `ret`), next to
`run_segment`, and the units drop their `probe_fns` modules.
