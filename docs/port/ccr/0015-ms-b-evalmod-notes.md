# CCR-0015: MS-B EvalMod contract notes

**Status: applied 2026-11-10 by CCR2 (§1 ui doc §13 item 14, §2 retracted, §4 getActor doc line, §5 record); §6 needs code (G1a dhat half fails); §3 escalated. See AMENDMENTS A8.**

Filed status: proposed by MS-B (2026-10-10). Numbering: first free number at the time of writing; the
integrator may renumber (the markers in MS-B's files and tests say `CCR-0015`).

Found while porting Classes/ModStore.lua 48-54 and 363-971 (`crates/pob-calc/src/classes/eval_mod.rs`)
and running the MS-B acceptance tests (`crates/pob-oracle/tests/MS-B.rs`, benches `crates/pob-oracle/benches/MS-B.rs`).

## 1. `tag.div`: the no-reader proof and the one reader that does observe it (UI)

calc doc §5.3 requires EvalMod not to mutate a tag and MS-B to prove "that no later reader observes
the Lua mutation" of `tag.div = GetMultiplier(self, tag.divVar, cfg)` (ModStore.lua:401, 499).
The port computes the value into a local `div`. Evidence (`tests/MS-B.rs::tag_div_no_reader`):

- **Writers**: the only `divVar` tags in PoB's source are the two Rage Vortex stat-map mods
  (Data/Skills/act_str.lua:9102, 9105; the parser and ModCache have none). The test fails if a new
  site appears.
- **Readers of a `div` key** (the test fails if the list changes): ModStore.lua 403/405/501
  (EvalMod itself, always after the write in the same call, so identical to the local);
  CalcActiveSkill.lua 69/72 (`map.div` / `modOrGroup.div`, stat-map group scalars, not tags);
  CalcPerform.lua 2381-2382 (`warcryBuff[1].div` of a warcry `GlobalEffect` tag, never a divVar
  tag); **CalcBreakdownControl.lua 481/485** (UI). Whole-tag readers (`copyTable`,
  `modLib.formatTag` via `compareModParams`/`formatMod`) only meet `div` on the two Rage Vortex
  mods, which reach `skillModList` through `mergeLevelMod` (`AddMod`, no `MergeMod`).
- **Dynamic check**: PoB is run twice per build in fresh processes, stock and with `EvalMod`
  wrapped so that every divVar tag's raw `div` is restored when the call returns (the Rust
  semantics). All outputs of the BUILD and CALCS envs (player/enemy/minion), also after a second
  full recalculation (which reads stale `div`s in stock PoB), are identical, on Rage Vortex variants
  of corpus builds (main skill Rage Vortex part 2: the divVar tags are evaluated, asserted via the
  wrapper's count and the stock run's written `div`) and on unmodified builds. Sample size: by
  default the test runs the first 3 Rage Vortex variants and the first 3 unmodified builds of the
  corpus (`MS_B_DIV_RV=3`, `MS_B_DIV_PLAIN=3`, the acceptance run); set the env vars to run more.

So no **calc** reader observes the mutation. The UI does: CalcBreakdownControl.lua:481 prints
`"per "..(tag.div and (tag.div.." ") or "")..var` for a Multiplier tag, i.e. for the two Rage Vortex
mods stock PoB shows the `div` of the last EvalMod call on that (process-global, data-owned) tag,
whichever store/cfg it came from. A Rust tag never carries it.

Proposed (ui doc, unit U40 CalcBreakdownControl): add to the U40 row / §13 notes:
"Multiplier/PerStat tag text (CalcBreakdownControl.lua 481/485): for a tag with `divVar`, Lua
prints the `div` EvalMod last wrote into the shared tag (calc doc §5.3 forbids the write). Port:
print `tag.div` or, when `tag.divVar` is set, `actor.modDB:GetMultiplier(tag.divVar, cfg)` of the
breakdown's actor/cfg; record the difference in `known_diffs.toml` (CCR-0015 §1)."

## 2. (retracted) MS-A `GetStat` on a store without an actor

An earlier draft of this CCR claimed that `ModStore:GetStat` raises on a store made without a
parent (`self.actor = { }`) and proposed making MS-A's `GetStat` raise there. That was wrong and
is withdrawn; **no change to `mod_store.rs` is proposed**.

The final branch of `GetStat` is guarded (ModStore.lua:355):
`return (self.actor.output and self.actor.output[stat]) or (cfg and cfg.skillStats and cfg.skillStats[stat]) or 0`.
On an actor-less store it returns the cfg value or 0 and does not raise. Lua raises only for
`ManaReservedPercent` (L320 `self.actor.output["Mana"]`), `LifeReservedPercent` (L334) and
`ManaUnreserved` (L347), and MS-A's `GetStat` raises in exactly those three cases through
`actor_output("")`. Its final branch (`if let Some(a) = &actor { … }`, then the cfg/0 fallback) is
therefore faithful. The proposed diff (`actor_output("").get(stat)` unconditionally) would have
made every `GetStat` on an actor-less store raise where Lua does not.

Data: over all57 (MS_B_VERBOSE), every synthetic CCR-0015 known difference is an `enemy`
ItemCondition probe (§3, the enemy has no `itemList`); none comes from the no-parent probe store.
The no-parent store is now gated like every other store (`tests/MS-B.rs::compare_synth` has no
carve-out for it), so a Lua `'output'` error there that Rust does not reproduce (the three
reservation stats) fails the test.

## 3. Lua `nil` actor fields the frozen types cannot express

EvalMod reads actor fields that are nil for some actors; the frozen `Actor`/`MinionData` always
hold a (possibly empty) value:

| Lua | Rust | EvalMod effect | all57 (L3 / synthetic probes) |
|---|---|---|---|
| enemy actor has no `itemList` (CalcSetup.lua:519-522) | `Actor.itemList` always a map | ItemCondition `allSlots`: Lua raises (`pairs(nil)`), Rust evaluates over no items; non-`allSlots`: Lua ignores `cfg.item`, Rust uses it | 0 / 1,192 (every one an `allSlots` ItemCondition probe on the enemy: Lua raises at ModStore.lua:715, Rust returns a value; the non-`allSlots` value case has 0 occurrences, it is gated) |
| enemy actor has no `activeSkillList` | always a list | already CCR-0014 §2 (PerStat on `ManaReservedPercent`/`LifeReservedPercent` now also reaches it from EvalMod) | 1,420 (GetStat queries, as in MS-A) / 8,940 |
| an actor table without `output` (any actor the dump marks `hasOutput = false`) | `Actor.output` always a table | GetStat of `ManaReservedPercent` / `LifeReservedPercent` / `ManaUnreserved` (L320/334/347): Lua raises, Rust reads the empty table; every other stat is guarded in Lua (L355) and agrees | 0 / 0 |
| `data.minions.GuardianRelicAll` has no `monsterTags` | `MinionData.monsterTags: Vec<Name>` | MonsterTag: Lua returns nil, Rust treats it as "no tag matches" (differs only with `neg`) | 0 / 0 |

Proposed: `Actor.itemList: Shared<Option<IndexMap<…>>>` (None for the enemy) and
`MinionData.monsterTags: Option<Vec<Name>>`, or accept them in `known_diffs.toml`. MS-B counts and
prints these cases separately (they are not part of the 100% gate). The known-difference filter
in `tests/MS-B.rs::known_diff` requires the Lua error of the nil read (`'ipairs'`, `'pairs'`,
`'output'`) for the list/output rows, and for the `monsterTags` row a MonsterTag-tagged mod in the
queried store's chain (or the probe mod); any other mismatch on those stores is gated.

**Decision needed from the integrator (not only a `known_diffs` entry)**: the enemy `itemList`
row is a real value difference, not just a different error. For a non-`allSlots` ItemCondition
on the enemy, Lua's `elseif self.actor.itemList then` (ModStore.lua:697) is false, so `items`
stays `{ }` and the tag sees no item; the Rust `Actor.itemList` is always a map, so the port takes
that branch and uses `cfg.item` (L708/L711). No real enemy mod reaches it over all57 (L3: 0), but
an enemy-side ItemCondition evaluated with a cfg carrying `item` would differ in value. Either
make `Actor.itemList` an `Option` (frozen-type change, U0) or add an explicit "actor has no item
list" flag that EvalMod can test; MS-B will adapt eval_mod.rs to whichever is chosen.

## 4. Smaller notes

- `getActor` (frozen signature `Option<Name>`, calc doc §5.3 says `Name`): the doc line should read
  `actorType: Option<Name>` (`getActor(self, tag.actor)` is called with a nil actor by PerStat and
  PercentStat). Lua's `self.actor[actorType]` returns any field of the actor table; the port
  returns the actor-valued links (enemy, parent, player, minion, partyMembers) and nil for any
  other key (`LUA-QUIRK`); the census of `actor`/`limitActor`/`thresholdActor` values in PoB's data
  and parser is exactly {enemy, parent, partyMembers, player}.
- Item fields read by ItemCondition (for IT-A/IT-B): `item.corrupted` is `true` or nil in Lua
  (Item.lua:551), modelled as `Item.corrupted: Cell<bool>` with false = nil; `item.shaper` /
  `item.elder` are read from `Item.influences` under the influence keys (Item.lua:104 writes false,
  1234 true). IT-A must keep both.
- `GrantedEffect.baseFlags` is a `NameSet` (never nil), so BaseFlag never falls back to
  `cfg.baseFlags` when a granted effect is present; Lua does when the granted effect has no
  `baseFlags`. No cfg sets `baseFlags` at the pin, so the result is the same.
- CCR-0014 §3 is confirmed from another side: the synthetic tag scan of `modLib.parseModCache`
  (§5) meets a warcry `GlobalEffect` tag carrying `warcryPowerBonus` in a build that uses warcries,
  i.e. CalcPerform.lua:2382 writes into a tag shared with PoB's parser cache (process-global data),
  not into a fresh copy.

- MeleeProximity / DistanceRamp: when both the value and a ramp entry are invalid, the port may
  raise a different one of the two errors first than LuaJIT does (error text only; whether it
  raises, and every value, is the same).
- Both-error queries: when Lua and Rust both raise, the tests count the query as identical
  without comparing the error kind (L3 prints the pairs it met; synthetic probes do not). This is
  looser than the value comparisons and is noted here so the gate is not over-read.

## 6. Gate G1a: the allocations left in `Sum`/`More`/`Flag` are outside MS-B's file

**Status: G1a FAILS at MS-B's commit** (dhat half; the speed half passes). It must not be
declared passed, and Wave 5 must not start (calc doc §9.7 failure rule), until the integrator or
the MS-A / U0 owners apply the diffs below or an equivalent. A reviewer reproduced 34
allocations on 8 builds (ratios 0.21-0.46); the 0-allocation result with both diffs applied
(below) was measured by MS-B only and has not been independently checked.

`benches/MS-B.rs` measures the speed half of G1a and runs the dhat check (calc doc §9.7). EvalMod
itself allocates nothing in steady state: ItemCondition's lower-casing and slot lists are
memoised / inline, and `calcLib.getGameIdFromGemName` (CT's `calc_tools.rs`, which lower-cases
into a fresh `String` on every call) is memoised per name in eval_mod.rs (a pure function of the
immutable data). Over all57 the remaining allocations (see the bench output; every one is
named by query) come from two places MS-B does not own:

(a) `GlobalLimits = ahash::AHashMap<Name, f64>` (classes/mod.rs, U0's frozen type; created lazily
    by MS-A's `SumInternal_DB`/`MoreInternal_DB`/`TabulateInternal_DB`): the first
    `globalLimits[key] = …` (ModStore.lua:963) of a query that meets a `globalLimit` tag
    (e.g. Warning Call's `Armour` INC) allocates the map's table.
(b) `ModStore::GetStat` (MS-A, mod_store.rs:968 and 1003-1004/1033-1034) iterates
    `buffList.snapshot()` and `activeSkillList.snapshot()`, which clone the vectors; PerStat /
    StatThreshold tags on `ManaReservedPercent` / `LifeReservedPercent` (aura effect mods) reach it.

Proposed diffs (both semantics-preserving; EvalMod already uses only `get`/`insert`, so (a) needs
no change in eval_mod.rs):

```diff
--- crates/pob-calc/src/classes/mod.rs
-/// `globalLimits` (ModDB levels only; allocated lazily).
-pub type GlobalLimits = ahash::AHashMap<Name, f64>;
+/// `globalLimits` (ModDB levels only): an inline small map (a query meets a handful of
+/// `globalLimitKey`s at most), so a Sum never allocates for it.
+#[derive(Default, Debug, Clone)]
+pub struct GlobalLimits(smallvec::SmallVec<[(Name, f64); 4]>);
+impl GlobalLimits {
+    /// `globalLimits[k]`.
+    pub fn get(&self, k: &Name) -> Option<&f64> {
+        self.0.iter().find(|(n, _)| n == k).map(|(_, v)| v)
+    }
+    /// `globalLimits[k] = v`.
+    pub fn insert(&mut self, k: Name, v: f64) -> Option<f64> {
+        if let Some(e) = self.0.iter_mut().find(|(n, _)| *n == k) {
+            return Some(std::mem::replace(&mut e.1, v));
+        }
+        self.0.push((k, v));
+        None
+    }
+}
--- crates/pob-calc/src/classes/mod_store.rs (GetStat)
-            for buff in buffList.snapshot() {
+            for i in 1..=buffList.len() {
+                let Some(buff) = buffList.get(i) else { break };
@@ (L322 and L336, twice)
                 let list = actor.as_ref().map(|a| a.activeSkillList.get());
-                for activeSkill in list.map(|l| l.snapshot()).unwrap_or_default() {
+                for i in 1..=list.as_ref().map_or(0, |l| l.len()) {
+                    let Some(activeSkill) = list.as_ref().and_then(|l| l.get(i)) else { break };
```

Verified locally (applied, measured, reverted; not part of MS-B's commit): with both diffs the
dhat check reports **0 allocations (0 bytes) over 133,504** Sum/More/Flag queries (every name of
the all57 player modDBs × {nil, skillCfg} × {Sum BASE, Sum INC, More, Flag}; 20 names left out,
see the bench), and the whole of G1a passes (`G1a: passed`; LuaJIT 50 iterations per build in that
run: Rust/LuaJIT ns/query 0.23-0.56). Without them (MS-B's committed state) the same check reports 160
allocations (4,720 bytes) in 40 queries, all of them `globalLimit` mods (Warning Call `Armour`,
…) or PerStat/StatThreshold reads of the reservation stats (`Damage`/`BuffEffect` More, …), i.e.
(a) and (b); the speed half passes (official run, LuaJIT 200 iterations per build: Rust/LuaJIT
0.25-0.66).

## 5. What MS-B's tests cover (for the record)

- `l3_full_matrix_all57`: the MS-A L3 matrix extended with the actor graph, items, minionData and
  full cfgs (plus per-skill cfgs on each skill's own list and four `skillDist` variants), every
  query gated; and synthetic probes: every distinct tag of `modLib.parseModCache` and of the skills'
  stat maps / base mods (≈1.5k tags) on a probe mod (BASE value, and `{mod=…}` / `{key,value}` LIST
  values for Multiplier/PerStat/PercentStat) in a ModDB child of the player/enemy/minion modDB of
  each build and in a ModDB made without a parent (actor-less, gated with no carve-out), queried
  with every global cfg.
- `tag_div_no_reader`: §1.
- `benches/MS-B.rs`: gate G1a (calc doc §9.7) and the dhat check.
