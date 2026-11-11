# CCR-0113: INT-1-A: `copyActiveSkill` keeps a copied skill's foreign actor alive

Unit: **INT-1-A** (INT-1 triage `docs/port/parity/INT-1-triage.md` fix unit for bucket A, named
INT-1B in its §7 table; owner AS). Status: **pending** (the code on `port/INT-1-A` follows the
workaround in §3; §2 is the proposed contract change). Number reserved with
`scripts/port/next_ccr.sh`.

## 1. Problem: `ActiveSkill.actor` is a `Weak`, Lua's `skill.actor` is strong

Evidence:

- calc doc §3.3 rule 4 (`calc-architecture.md:212`): "Back-pointers (`ActiveSkill.actor`,
  `ModStore.actor`, `Actor.enemy/parent/player`) are `Weak`. `Env` owns its actors. `GlobalCache`
  owns cached envs, and wiping it frees them." §5.4 (`calc-architecture.md:958`) types it
  `pub actor: Field<ActorWeak>, // may point into another env (copyActiveSkill, D2)`.
- `Modules/CalcActiveSkill.lua:181`:
  `calcs.createActiveSkill(activeEffect, skill.supportList, skill.actor, …)`: the copy shares
  the source skill's actor, a strong reference in Lua.
- `Modules/CalcMirages.lua:134/137`: the Saviour's `compareFunc` returns
  `GlobalCache.cachedData[env.mode][uuid].ActiveSkill`, a skill of the env that
  `buildActiveSkill` created and cached (`Modules/Common.lua:924-941`); `CalcMirages.lua:40`
  copies it, `:155` installs the copy as `env.player.mainSkill`.
- `Modules/CalcPerform.lua:3989`: `cacheData(cacheSkillUUID(env.player.mainSkill, env), env)`
  files the perform env under the **source** skill's uuid (the copy has the same `srcInstance`)
  and overwrites the cache entry that was the only owner of the source env. In Rust that frees
  the source env, its player, and so the copy's actor; the next copy of the cached copy
  (`Calcs.lua:400-403` cost warnings → `buildActiveSkill` → mirages) raised
  `attempt to index field 'actor' (a nil value)` in `buildActiveSkillModList`
  (`calc_active_skill.rs`, CalcActiveSkill.lua:236ff). Measured on upstream_Dual_Savior: all
  903 main keys, 3 CALCS lists and 4 Rust errors of the CA L6 baseline (triage §3 bucket A, §4 A).

## 2. Proposed contract change (calc doc §3.3 rule 4, §5.4 `ActiveSkill`)

```diff
-4. `Rc` cycles are forbidden. Back-pointers (`ActiveSkill.actor`, `ModStore.actor`, `Actor.enemy/parent/player`) are `Weak`. `Env` owns its actors. `GlobalCache` owns cached envs, and wiping it frees them.
+4. `Rc` cycles are forbidden. Back-pointers (`ActiveSkill.actor`, `ModStore.actor`, `Actor.enemy/parent/player`) are `Weak`. `Env` owns its actors. `GlobalCache` owns cached envs, and wiping it frees them. Exception (CCR-0113): a skill made by `copyActiveSkill` whose source actor is not `env.player` (a foreign env's actor, CalcActiveSkill.lua:181) also owns that actor (`ActiveSkill.actorKeep`), because the source env can be freed while the copy lives on (CalcPerform.lua:3989). A same-env copy owns nothing: `postCalcFunc` stores it in `env.player.mainSkill` (CalcMirages.lua:155/232), so an owning reference would close a cycle.
```

```diff
     pub actor: Field<ActorWeak>,                  // may point into another env (copyActiveSkill, D2)
+    pub actorKeep: Field<Option<ActorRef>>,       // CCR-0113: owns a foreign `actor` (copyActiveSkill only)
```

The typed field touches the frozen `active_skill.rs` (U0) and the probe's `reflect!(ActiveSkill)`
list (where it must be excluded from the state dump: PoB has no such key). That needs
coordination with RETYPE, so INT-1-A does not apply it.

## 3. Workaround on `port/INT-1-A` (until §2 lands)

`crates/pob-calc/src/modules/calc_active_skill.rs` `copyActiveSkill`, right after the L181
`createActiveSkill` call (marked `// CCR-0113`):

```rust
if let Some(actor) = skill.actor() {
    if !Rc::ptr_eq(&actor, &env.player) {
        newSkill.extra.set(actorKeepKey(), Val::Obj(actor)); // "actorKeep"
    }
}
```

- The guard: in PoB every `copyActiveSkill` caller is `CalcMirages.lua:40`, and the only object
  the copy is stored into that the source actor can reach is `env.player.mainSkill`
  (`postCalcFunc`, CalcMirages.lua:155/232; `newEnv.player.mainSkill` at :51 is a fresh actor).
  So the only actor that can close a cycle is `env.player`, which `env` already owns.
- Visible side effect: the probe exports `activeSkill.extra` keys, so a state dump of a copy made
  by the production `copyActiveSkill` with a foreign actor shows an extra `actorKeep` key that PoB
  lacks. No L4 corpus dumps such a skill today (MI's and PF-G's L4 tests replay
  `copyActiveSkill` / `mirages` with PoB's recorded results). §2's typed field, kept out of the
  `reflect!` key list, removes it.
- The foreign actor's own back-pointers (`enemy`, `parent`, `player`) stay `Weak` and die with
  the source env. Lua would keep them alive too; nothing in the copy's calc reads them (AS reads
  `env.enemy`, not `activeSkill.actor.enemy`), and the corpus measurements below show no effect.

## 4. Verification

- `crates/pob-oracle/tests/AS.rs` `as_copy_actor_keepalive` (new): a foreign-env copy keeps the
  source actor alive after the source env is dropped, and frees it after the copy is dropped and
  `wipeGlobalCache()` runs; a same-env copy stores no strong reference and frees `env.player` with
  the copy installed as its `mainSkill`; Dual_Savior's MAIN `buildOutput` result frees its player
  and the kept foreign actor after drop + wipe.
- CA L6 / D13 numbers: see the INT-1-A commit message.
