# CCR-0088: U15 — inject bound to the live build model (socket groups, gem instances), several documents per registry

Status: proposed by U15 (2026-11-10). Not blocking: U15 works around both items locally
(`crates/pob-ui-oracle/src/lua_calc.rs`, marked `// CCR-0088`).

## 1. Socket groups and gem instances are injected as copies

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

**Workaround (U15):** the bridge tags groups and gems with their load position before the first
calc pass (a wrapper around `calcs.buildOutput` in the bridge VM), dumps
`build.skillsTab.socketGroupList` plus the tags as locals, writes each dumped group / gem onto
the live object at that position (typed fields; dynamic-table groups through
`pob_calc::probe::set_member`), keeps new groups, drops removed ones, and then walks the injected
envs (actors → skills → effects, minions, `auxSkillList`, the groups' display lists) re-pointing
`socketGroup` / `srcInstance` at the live objects.

**Proposed diff** (ORC-X): an injection entry point that binds dumped tables to caller-supplied
live objects, so no copy is ever made:

```rust
/// Like `inject`, but every table of `bind` (a dump-local name → live object list, element i
/// bound to the i-th element of the dumped list) is the given object, refilled in place.
pub fn inject_bound(dump: &StateDump, build: &BuildRef, bind: &[(&str, Vec<PObj>)])
    -> Result<Injected, String>;
```

## 2. `inject` resets the probe registry per document

`inject` starts with `probe::registry_reset()` (`inject.rs:85`), so injecting the mainEnv and
calcsEnv documents one after the other loses the first one's registry side entries
(object-keyed table entries, `notBuff` side extras, `$foreign` placeholders' markers; CCR-0030).
Nothing in the UI reads them today.

**Workaround (U15):** the main env's document is injected last.

**Proposed diff:** `inject_keep(dump, build)` — `inject` without the registry reset (the caller
resets once).
