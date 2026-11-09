# CCR-0014: CT (calc tools) contract corrections and findings

Unit: CT (calc doc §11.1 row CT). Status: proposed. Every item is applied locally in CT's own
files (marked `CCR-0014` where it touches code); the integrator only needs to amend the doc text
(and, for items 3 and 4, tell the owning units).

## 1. `Breakdown` helper signatures (calc doc §5.5) — CT corrects U0's reading, as §5.5 asks

Evidence (callers pass nil):

- `breakdown.area(…, incBreakpoint, moreBreakpoint, redBreakpoint, lessBreakpoint, label, radiusMod)`:
  `calcRadiusBreakpoints` returns four nils when the radius is 0 (CalcOffence.lua:198-223); the
  calls at CalcOffence.lua:1317, 1401, 1409 pass no `label` / `radiusMod`; `skillData.radiusLabel`
  is nil for most skills (395, 408, 476). CalcBreakdown.lua:97 inserts the nil label (a no-op) and
  :106 tests the four breakpoints for nil.
- `breakdown.effMult(…, invertChance, effectiveResist)`: CalcOffence.lua 4587, 4858, 5199, 5209,
  5720, 5836 pass eight arguments; CalcBreakdown.lua:118 (`effectiveResist or resist - pen`) and
  :125 (`invertChance and …`) handle the nils.
- `breakdown.leech(…, rate, …)`: `rate` is a mod name (`"LifeLeechRate"`, CalcOffence.lua:4145-4151),
  as the frozen code already has it; the doc text says `f64`.

Applied in `crates/pob-calc/src/modules/calc_breakdown.rs`:

```diff
-    pub fn area(&self, base: f64, areaMod: f64, total: f64, incBreakpoint: f64, moreBreakpoint: f64,
-                redBreakpoint: f64, lessBreakpoint: f64, label: &str, radiusMod: Option<f64>) -> Table;
-    pub fn effMult(&self, damageType: Name, resist: f64, pen: f64, taken: f64, mult: f64, takenMore: f64,
-                   sourceRes: Option<Name>, useRes: bool, invertChance: f64, effectiveResist: f64) -> Table;
+    pub fn area(&self, base: f64, areaMod: f64, total: f64, incBreakpoint: Option<f64>, moreBreakpoint: Option<f64>,
+                redBreakpoint: Option<f64>, lessBreakpoint: Option<f64>, label: Option<&str>, radiusMod: Option<f64>) -> Table;
+    pub fn effMult(&self, damageType: Name, resist: f64, pen: f64, taken: f64, mult: f64, takenMore: f64,
+                   sourceRes: Option<Name>, useRes: bool, invertChance: Option<f64>, effectiveResist: Option<f64>) -> Table;
-    pub fn leech(&self, instant: f64, instantRate: f64, instances: f64, pool: f64, rate: f64, max: f64,
+    pub fn leech(&self, instant: f64, instantRate: f64, instances: f64, pool: f64, rate: Name, max: f64,
```

## 2. `calcLib.validateGemLevel` receives skill effects, not only gem instances

Evidence: CalcActiveSkill.lua:54 (`skillEffect`), :513 (`activeEffect`), CalcSetup.lua:1460
(`normalizedGrantedSkill`), :1716 (`supportEffect`) pass skill-effect tables; only SkillsTab.lua:1225
passes a gem instance. The frozen `validateGemLevel(gemInstance: &GemInstance)` cannot take a
`SkillEffect`. Applied in `crates/pob-calc/src/modules/calc_tools.rs`:

```diff
-pub fn validateGemLevel(gemInstance: &GemInstance);
+pub trait GemLevelTarget { fn lua_grantedEffect(&self) -> Option<GrantedEffectRef>; fn lua_gemData(&self) -> Option<GemDataRef>;
+                           fn lua_level(&self) -> f64; fn set_lua_level(&self, v: f64); }   // impl for GemInstance, SkillEffect
+pub fn validateGemLevel<G: GemLevelTarget + ?Sized>(gemInstance: &G);
+/// `next(grantedEffect.levels)` (LuaJIT order, replayed with LjIntMap).
+pub fn levels_next_key(levels: &LevelTable) -> Option<f64>;
+/// `#grantedEffect.levels` as LuaJIT computes it.
+pub fn levels_len(levels: &LevelTable) -> usize;
```

Finding (checked against LuaJIT for every skill): the data files build `levels` with explicit
`[n] = { … }` entries whose values are tables, so LuaJIT's parser emits `TNEW` with no array part
and one `TSET` per entry: every level lives in the hash part. `next(levels)` therefore returns 2
(not 1) for a contiguous 1..n table, and `#levels` is `unbound_search`. `LevelTable::len()` (the
leading run) is not what `#` returns on holey tables, so CT does not use it. Any unit that ports
`pairs(grantedEffect.levels)` / `next(levels)` / `#levels` should use these helpers.

Callers with an `Rc` pass `&*rc` (no deref coercion through a generic).

## 3. The statMap fallback mutates `grantedEffect.hasGlobalEffect` at calc time

Evidence: Data.lua:1034-1043 (`__index`) calls `processMod`, whose L74-79 set
`grantedEffect.hasGlobalEffect = true` on the shared `data.skills` entry when a fallback mod has a
`GlobalEffect` tag. data.skillStatMap has eight such entries (SkillStatMap.lua 503, 506, 1319, 1322,
1466, 2087, 2230, 2233). Skills that reach them through the fallback while loaded with
`hasGlobalEffect` nil: ChainHook, CleaveAltX, CatsStealthTriggeredIntimidatingCry, GeneralsCry,
SupportBlind, SupportRage (dump census). After the first `statMap[stat]` lookup (CalcActiveSkill.lua:63,
352) PoB reads `true` at CalcSetup.lua:1755 and SkillsTab.lua:1015 for the rest of the process.

`data` is immutable in the port, so CT keeps the flag in thread-local state (one Lua VM = one thread)
next to the lookup memo (`grantedEffect.statMap[key] = map`, Data.lua:1038). Added to
`crates/pob-gamedata/src/skill_stat_map.rs`:

```diff
+/// `grantedEffect.hasGlobalEffect` as PoB sees it now (the loaded value, or true once a fallback
+/// lookup processed a GlobalEffect mod for this skill).
+pub fn hasGlobalEffect(ge: &GrantedEffect) -> bool;
+/// The state of a freshly booted PoB process (tests).
+pub fn reset_stat_map_state();
```

Proposed doc text (§4.4, after `statMapLookup`): "Readers of `grantedEffect.hasGlobalEffect`
(CalcSetup.lua:1755 [CS3], SkillsTab.lua:1015 [BM-B]) MUST call `skill_stat_map::hasGlobalEffect(ge)`,
not read the field." The oracle test `tests/CT.rs` (`stat_map`) checks the flag of every skill after
the lookups against PoB.

The fallback also raises where Lua raises: `processMod` indexes `tag.type` of every tag, and
`data.skillStatMap["spell_damage_+%_per_10_int"]` (SkillStatMap.lua:1735, `skill(…)` =
`makeSkillDataMod`, whose varargs put `ModFlag.Spell, 0` among the tags) has number tags; the copy
is memoised before the error (Data.lua:1038 precedes 1040), so later lookups return it. Ported
literally (no skill reaches that stat at the pin).

## 4. §3.6 audit miss: `mergeKeystones` mutates shared tree mods

Evidence: ModTools.lua:254-255: `for _, mod in ipairs(env.spec.tree.keystoneMap[k].modList) do
modDB:AddMod(fromTree and modLib.setSource(mod, modObj.mod.source) or mod)`. `setSource`
(ModTools.lua:239-245) writes `mod.source` of the keystone node's own mod (shared by every spec and
build using that tree in the process), not of a copy. After a build with an item-granted keystone
(a `Keystone` LIST mod from an item), the tree node's mod keeps the item's source; a later
allocation of the same keystone in the tree adds it with that source.

The port cannot mutate an `Arc<Mod>` held by the tree: `mod_tools_env.rs` adds a re-sourced copy and
leaves the node's mod alone. Exact reproduction would need the node's `modList` entry replaced by the
re-sourced mod (an MS-A / PT API). Proposed: add this site to §3.6 as a known shared-target mutation
and record the divergence in `known_diffs.toml` (only `mod.source` differs; it is observable in Calcs
tab source columns and in `source`-filtered queries) until PT/MS-A decide.

Also a contract note for PT: `mergeKeystones` reads `tree.keystoneMap[name]` as
`Val::Obj(NodeRef)` (`Val::obj::<Node>()`); PT must store the nodes that way.

## 5. `formatNumSep` (Common.lua:803-828) has no owner

`formatCalcVal` (CalcFormat.lua:13) calls it; scope.toml's Common.lua ranges stop at 800. CT ports
it in `crates/pob-calc/src/modules/calc_format.rs` (`@pob Modules/Common.lua:803-828`), with
`main.showThousandsSeparators` / `thousandsSeparator` / `decimalSeparator` in a thread-local
`NumSepSettings` (defaults Main.lua:108-112) set through `set_num_sep_settings`. Proposed: add
`803-828` to scope.toml's Common.lua ranges (owner CT) and tell U39 (CalcsTab) to call
`set_num_sep_settings` when the options change.

`formatNumSep` uses PoB's `lua-utf8` C module (`utf8.reverse` / `utf8.find` / `utf8.sub` by code
point). The oracle's `lua-utf8` is a byte-level shim of `string` (`crates/pob-oracle/src/jit.rs`,
`package.preload["lua-utf8"]`), so for a multi-byte thousands separator the oracle differs from PoB
(the shim never strips the leading separator). The port follows the C module; the CT oracle test
covers ASCII separators only. Unverified: the C module's `gsub` may reject a lone `%` in the
replacement (Lua 5.3 rules) where LuaJIT's `string.gsub` appends a NUL; the port follows LuaJIT.

## 6. Smaller notes (no action needed unless an owner disagrees)

- `grantedEffect.unsupported` (CalcTools.lua:87) is not a schema field; no data sets it, so the
  port reads it as nil.
- `SkillLevel.levelRequirement` is a plain `f64` (0 when absent). Seven levels in the dump have no
  `levelRequirement` (DarkMarionetteExplode, …); `buildSkillInstanceStats`' `level.levelRequirement
  or 1` would give 1 there, but those skills have no stats, so the difference is not observable.
- `modLib.formatValue` / `formatTag` print a jewel closure (`Value::JewelFunc`'s `func`) as
  `function: builtin`; Lua prints its address (`function: 0x…`, different per process).
- `modLib.parseFormattedSourceMod` can produce a mod whose `type` is any string (a formatted mod
  whose value contains `|` shifts the fields). `Mod.type` is an enum, so such a string is kept in
  `Mod.extra["type"]` with `type = ModType::Nil`; `mod_tools::mod_type_text(m)` reads it.
- The CT oracle sees PoB's own run-to-run nondeterminism (calc doc §9.5): ExplosiveTrap and
  ExplosiveTrapAltX share a `statMap` table that Data.lua:1049-1080 processes once per skill in
  `pairs(data.skills)` order, so the surviving `mod.source` names either skill (the dump holds one
  process's choice). The test accepts either.
