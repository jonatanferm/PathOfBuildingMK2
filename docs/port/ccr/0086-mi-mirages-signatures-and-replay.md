# CCR-0086: MI — `calcs.mirages` return value, `calculateMirage(config = nil)`, MI's callee replay seam

Status: proposed by MI (2026-11-10). §1 and §2 are applied in MI's own file
(`crates/pob-calc/src/modules/calc_mirages.rs`); §1 needs a one-line follow-up in PF-G's file,
which MI does not own. §3 and §4 are notes for ORC-X / the L4 harness.

## §1 `calcs.mirages` returns a value (adopts CCR-0077 §2)

**Evidence.** CalcMirages.lua:57 `return not config.calcMainSkillOffence`, :64 / :354 bare
`return`, :427 `return calculateMirage(env, config)`; CalcPerform.lua:3798
`if not calcs.mirages(env) then calcs.offence(env, env.player, env.player.mainSkill) end`.
U0's frozen stub was `pub fn mirages(env: &EnvRef)`.

**Applied (MI).**

```diff
-pub fn mirages(env: &EnvRef) {
+/// The result's truthiness (`nil` → false).
+pub fn mirages(env: &EnvRef) -> bool {
```

Every bare `return` is `false`; `Some(b)` of `calculateMirage` is `b`. The existing call site
still compiles (the value is discarded), so nothing else breaks.

**Needed follow-up (PF-G, `calc_perform/ailments_dispatch.rs` ~L3798).** The call site still
treats the Rust call as returning nil (its CCR-0077 §2 workaround), so after a Reflection /
Tawhoa's Chosen mirage (which return `true`) the Rust perform runs `offence` on the swapped-in
skill where PoB does not:

```diff
     let miragesRet = callee("mirages", &env, &player, || {
-        calcs::mirages(&env);
-        Val::Nil
+        Val::Bool(calcs::mirages(&env))
     });
```

## §2 `calculateMirage` takes `Option<&MirageConfig>`

**Evidence.** CalcMirages.lua:24-27 `local function calculateMirage(env, config) if not config
then return end`; `calcs.mirages` reaches :427 with `config` nil when no family matched and on
the whole General's Cry path (346-425 build no config). U0's stub took `&MirageConfig`.

**Applied (MI).** `pub(crate) fn calculateMirage(env: &EnvRef, config: Option<&MirageConfig>) ->
Option<bool>` (file-local; no other caller, as with CCR-0040).

## §3 MI's callee replay seam (`calc_mirages::replay_hook`, feature `probe`)

Same pattern as PF-G's (CCR-0077 §1): a thread-local hook, compiled only with `probe`, that the
L4 test (`crates/pob-oracle/tests/MI.rs`) uses to replace `copyActiveSkill`, the nested
`calcs.perform(newEnv)`, `buildActiveSkill`, `isTriggered` and `calcMultiSpellRotationImpact`,
plus one observation point that is not a callee (`generalsCryCooldown`: General's Cry's
`cooldown` before the `m_max` at CalcMirages.lua:395, §4.3).
It is typed (`Call` / `Ret` enums), because these callees return Rust values
(`(newSkill, newEnv)`, a bool, `(rate, table)`), not state. When ORC-X moves the seam to
`pob_calc::probe` (CCR-0077 §1 proposal), MI's wrappers (`call_copyActiveSkill` etc.) are the
call sites to switch.

## §4 Replay notes for ORC-X (no change requested from MI)

The MI L4 test works around three things the replay / dump format cannot do yet; each is
confined to `tests/MI.rs`:

1. **Results that are new tables.** `copyActiveSkill`'s `(newSkill, newEnv)` are passed as the
   replay's `ret` by moving `mi:41`'s `newSkill` / `newEnv` locals into `locals.ret`, so
   `Replay::merge` decodes them into the live graph with references translated.
2. **Locals are not paired.** `perform(newEnv)` changes `newSkill`, a local; the test attaches
   it at `env.mi_newSkill` (`Env.extra`) in the live graph and in both dumps for the replay, then
   detaches it.
3. **Another env's output** (`newEnv.player.output`, CCR-0064 §1) is a `$foreign` marker in
   every line probe. For Mirage Archer / Sacred Wisps it stays a marker (the test registers the
   placeholder env's output as that foreign table, so it exports identically). For Reflection /
   Tawhoa's Chosen, where the main env takes it over (`env.player.output = newEnv.player.output`),
   the test grafts `mirages:out`'s `env.player.output` into `mi:53` (ids remapped) minus the
   keys MI writes; that output's *content* is therefore not an independent check of MI (only
   the keys MI writes, and every other path, are). For General's Cry the cached env's
   `HitTime or Time` is rebuilt from PoB's `cooldown` before (`mi:376`) and after (`mi:396`)
   the `m_max` at 395: where the cooldown dominates (`cd376 == cd396`) the attack time is set
   to 0, so the `m_max` takes Rust's own `calcSkillCooldown` result and the DPS MORE mod at 396
   checks it; where the attack time dominates it is set to `cd396`. Rust's pre-`m_max` cooldown
   is also handed to the test (`Call::generalsCryCooldown`, an observation that the hook
   answers with `None`) and compared with `mi:376` directly.

Identity loss in replays (CCR-0077 §6: a callee that replaces tables by fresh ones, e.g. the
nested `initEnv` reassigning `gemInstance.displayEffect`) shows up as mismatches whose two sides
have equal content but different identity. The MI test counts those separately
("identity-only") and gates on content mismatches only.

Known limits of the MI gate (review round 1, for ORC-X): `content_eq` treats tables nested
deeper than 10 levels as equal, so an identity-only classification could in principle hide a
very deep content difference (tightening belongs to the ORC-X diff). Sacred Wisps and Tawhoa's
Chosen have no corpus build in `.parity/builds` and are reviewed against the Lua only; a corpus
build for each would make them L4-checked.
