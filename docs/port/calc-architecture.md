# Calc port architecture (binding contract)

> **Read `docs/port/AMENDMENTS.md` first** — it reconciles this document with the crates already built (pob-oracle, pob-dump, pob-sg) and wins on conflict.

**Status:** FINAL. This document is binding for every calc porting agent. CCRs 0001-0006 and 0010-0013 are applied in place (unit CCR1, 2026-10-09); `docs/port/ccr/` keeps each CCR with its evidence.
**Upstream pin:** PathOfBuilding v2.67.2, commit `16de4b82d57f1c0de6eb40f37143c32d4da36a02`, checked out at `.PathOfBuilding/`. All Lua line numbers in this document refer to that commit and were checked against it.
**Scope:** everything from "PoB build XML or UI edits" up to and including `env.player.output`, `env.player.breakdown`, minion and enemy outputs, and the actor and skill mod stores that the Calcs tab Tabulates after the calc. Included: the misc calculator and power report (`Calcs.lua`, `CalcsTab.lua` 441-773). Excluded: rendering, controls and tabs. Those are covered by the UI architecture, which consumes this crate.
**Source of this design:** the "fidelity" proposal won both judge panels and is the base. Ideas are grafted from the "parallel-porting" and "performance" proposals. Every factual error the judges found was re-verified against the Lua and fixed (§12 lists them).

Normative words: **MUST**, **MUST NOT**, **SHOULD**. Where this document and a proposal disagree, this document wins. Where this document and the Lua disagree about what PoB *does*, the Lua wins, and you file a CCR (§10.3).

---

## 1. Decisions

| # | Decision | Rationale |
|---|---|---|
| D1 | **Literal 1:1 mirror.** The port keeps PoB identifiers verbatim (`skillModList`, `calcs::initEnv`, `output.TotalDPS`), uses the same function decomposition, keeps statement order, and keeps Lua comments. | A reviewer can diff Rust against Lua statement by statement, and an upstream change can be re-applied one function at a time. The judges scored this highest on fidelity, and it is the only way to reach "aim for exact". |
| D2 | **The aliasing model is `Rc` + interior mutability.** Mutable Lua tables that are aliased become `Rc` handles (`Table`, `ModStoreRef`, `ActorRef`, `ActiveSkillRef`, `EnvRef`, `Shared<T>`) whose accessors take short borrows. | PoB's calc relies on live aliasing across envs. Examples: `copyActiveSkill` reuses the cached skill's foreign `actor` (CalcActiveSkill.lua:181), and its new `skillModList` is parented to that foreign actor's modDB (:492). `GlobalCache` entries hold the whole `Env` (Common.lua:924-942), and `calcFullDPS` keeps re-performing the same env object. `override.extraJewelFuncs.actor` points at the env of an aborted initEnv attempt (CalcSetup.lua:1427). Id arenas and snapshots get these cases wrong; `Rc` gets them right without special code. |
| D3 | **Dynamic tables only where Lua keys are computed.** `output`, `breakdown`, `skillData`, `skillFlags`, `conditions`, `multipliers`, `configInput` and similar are a dynamic `Table`. Fixed-shape objects (Actor, ActiveSkill, SkillEffect, Env, ModCfg, Item, Node, ...) are structs. A struct field is a `Field`/`Cell` exactly when Lua reassigns it after construction. | Gives type safety where shapes are fixed and exact Lua semantics where they are not. `skillFlags[string.lower(ailment)]` (CalcOffence.lua:4158) and `skillFlags[map.skillFlag]` (CalcActiveSkill.lua:354) rule out a generated flag enum. |
| D4 | **No Lua at runtime.** LuaJIT lives only in `pob-luahost` (today's `pob-calc`, renamed), `pob-oracle` (tests) and `pob-extract` (data dump). | This is the user's decision. `cargo tree -p pob-calc -e normal` MUST NOT contain `mlua`. |
| D5 | **Data comes from a LuaJIT dump.** `pob-extract dump` boots PoB, runs `Modules/Data.lua`, and serialises the post-processed `data` table to canonical JSON (diffable) and then postcard (runtime). Enums are generated from `Data/Global.lua`. | This reproduces Generated uniques, Vaal Alt synthesis and `processMod` exactly. It also kills the stale SkillType table (the correct values are Area=8, Aura=39, Pact=135). |
| D6 | **Giant functions are split into segments.** `initEnv`, `perform`, `defence`, `buildDefenceEstimations` and `offence` are split at the statement boundaries in §7. A U0-owned orchestrator calls the segments in Lua order and passes a context struct whose fields are exactly the **measured** cross-segment locals (§7.1). Segments return `Flow`, which includes `Restart` for initEnv's recursive restarts. | Lets ~40 agents work on disjoint files. The measurement was redone for this document with `scripts/port/locals_audit.py` (§12 lists the corrections). |
| D7 | **Closures are split by where they live.** A closure stored in data or in a mod becomes an id plus a Rust registry checked against a body-hash manifest. A closure that runs where it is defined becomes a Rust fn, method or closure in the same file. | Upstream changes to closures are detected automatically (`registry_complete` test). |
| D8 | **pairs() order follows LuaJIT where LuaJIT is deterministic.** Verified on the vendored LuaJIT (luajit-src 210.5.12): the string hash is seeded from a secure PRNG (`lj_str.c:367`, `g->str.seed = lj_prng_u64`), so the `pairs()` order of string keys differs between PoB processes. Integer-keyed order is deterministic but is not insertion order. Rust therefore uses (a) insertion order for string-keyed tables, with the oracle detecting PoB nondeterminism by running each build twice (§9.5), and (b) an exact emulation of LuaJIT's integer-key hash layout (`pob_lua::ljtab::LjIntMap`) for integer-keyed tables whose `pairs()` order is observable (node-id maps). | Matching a random order is impossible. Matching a deterministic order is cheap and removes allow-lists. |
| D9 | **Stubs return defaults.** Every unported body is `unported!("File.lua:a-b")`, which logs once, counts the hit and returns `Default`. | The end-to-end pipeline and the diff harness run from day one, and parity rises unit by unit. |
| D10 | **Oracle segment injection.** A patched copy of PoB fires line-anchored probes at every segment boundary (`docs/port/stages.toml`), with `<id>#<n>` hit numbering. A unit test injects the dumped state at `<seg>:in`, runs only the Rust segment, and diffs the result against `<seg>:out`. Write-provenance proxies map end-to-end mismatches to Lua line ranges. | Units can be ported in any order, and every failure points at one unit. |
| D11 | **GlobalCache keeps live `EnvRef`s with PoB's lifetime rules**, including CALCULATOR entries that persist until `wipeGlobalCache`. | CalcMirages.lua:127-137, CalcTriggers.lua:529 and Calcs.lua:402 rebuild an entry only when it is absent, so stale CALCULATOR entries are observable. A per-job cache is not exact. |
| D12 | **The power report is exact when sequential.** The power report and misc calculator run sequentially on one thread with the shared cache by default. Parallel mode is opt-in and documented as possibly inexact for builds whose mirage or trigger logic reads stale cache entries. | Follows from D11. `pob-calc` is `!Send` by design (one PoB VM is one thread). |
| D13 | **The calc mutates the build model in place, as Lua does.** Examples: granted-skill socket groups, `srcInstance.skillPart`, `isSupporting`, `jewelData.limitDisabled`, `node.finalModList`. A fixed-point test (§9.4 L5) guards it. | This is literal fidelity. The parallel-porting proposal's Writeback channel is not needed with `Rc`, but its fixed-point test is kept. |
| D14 | **Performance grafts must not change semantics.** They are: interned `Name` (Ustr) plus call-site caching `n!` and memoised `cat!`; `Arc<Mod>` sharing; a per-name position index on ModList (safe because `ModList.SumInternal` loops names outside and mods inside, ModList.lua:101-121); lazy `globalLimits`; `mergeLevelCache` keyed by `Arc` pointer; and a dhat no-allocation test on `Sum`. Two gates enforce them: G1a (ModStore micro-benchmark) and G1b (vertical slice plus misc-calc time ratio against LuaJIT), each with a stop-and-profile rule. | The target is at least LuaJIT speed for the power report, without a design that trades away fidelity. |
| D15 | **Lua runtime errors are reproduced.** `lua_error!` panics with a `LuaError` payload. On desktop, `try_buildOutput` catches it with `catch_unwind`, as PoB shows a calc error. On wasm32 (`panic=abort`), the calc runs in a dedicated Web Worker that the UI restarts after an abort. | PoB really errors in some cases, for example `pairs(affectedByAura)` on an undeclared global (CalcPerform.lua:3464). `catch_unwind` does not work on `wasm32-unknown-unknown`. |
| D16 | **Undeclared-global reads are literal `nil`.** For example `dotCfg` at CalcOffence.lua:3527/3538 is the nil global, not the local declared at 4425/5772. `scripts/port/globals_lint.py` lists every such site, and each port carries `// LUA-GLOBAL-NIL`. | Literal fidelity. |
| D17 | **Module-level mutable upvalues are explicit thread-local state.** Example: `globalOutput`/`globalBreakdown` (CalcOffence.lua:65/67), which persist across `calcs.offence` calls. Module-level scratch tables that do not escape (`tempTable1-3`) become locals. | The judges found that every proposal mishandled this. |
| D18 | **Crates are `pob-lua`, `pob-gamedata`, `pob-modparser` and `pob-calc` (new), plus `pob-oracle` (dev).** The old `pob-calc` is renamed to `pob-luahost`. The old `pob-data` and `pob-engine` stay untouched until INT-4 deletes them. | Only one rename, so the shipping app keeps working. |
| D19 | **Tooling is Python 3 (stdlib only) under `scripts/port/`.** The locals audit, segment table, provenance, globals lint, pairs inventory and the lua2rs transpiler all live there. | No extra Rust compile load on a 3 GB machine. The analyser already exists and produced §7's measurements. |
| D20 | **The machine budget is binding.** At most 3 agents run at once. Each agent has its own git worktree and target dir. All cargo and oracle invocations go through `scripts/port/cargo.sh` / `oracle.sh`, which `flock` a global lock (one compile and one LuaJIT oracle at a time). | 3 GB of free RAM: rustc uses about 1 GB per crate and the LuaJIT oracle about 400 MB. A shared target dir across worktrees thrashes fingerprints. |

---

## 2. Crates and module layout

```
                         ┌──────────────┐
                         │   pob-lua    │  Lua stdlib semantics, Common.lua math, patterns,
                         │              │  format/sort, LuaJIT int-key order, Name, Field/Shared, LuaError
                         └──────┬───────┘
                         ┌──────▼───────┐
                         │ pob-gamedata │  generated Global.lua enums, Mod/Tag/Value/ModType, ModTools,
                         │              │  typed `data` schema + loader, statMap fallback, timeless/abyss LUTs
                         └──────┬───────┘
                         ┌──────▼───────┐
                         │pob-modparser │  ModParser.lua (+ ModCache tier). Emits Mods; JewelFunc as data.
                         └──────┬───────┘
                         ┌──────▼───────┐
                         │   pob-calc   │  lua_table, Classes/{ModStore,ModDB,ModList,Item,PassiveTree,PassiveSpec},
                         │              │  model (calc halves of Build + tabs), Modules/Calc*, ConfigOptions/ModMap
                         │              │  apply, jewel funcs, skill hooks, GlobalCache, calcs facade, probe export
                         └──────┬───────┘
          ┌─────────────────────┼──────────────────────┐
   ┌──────▼──────┐       ┌──────▼──────┐        ┌──────▼──────┐
   │ pob-ui (new)│       │ pob-oracle  │ dev    │ pob-extract │ build-time tool
   └─────────────┘       │ (LuaJIT)    │        │ (+dump)     │
                         └──────┬──────┘        └──────┬──────┘
                                └──────────┬───────────┘
                                    ┌──────▼──────┐
                                    │ pob-luahost │  = today's crates/pob-calc renamed (jit.rs, headless.rs,
                                    └─────────────┘    worker.rs, lua/ interpreter). Keeps the current app shipping.
scripts/port/   Python tooling (locals_audit, segments, provenance, globals_lint, pairs_inventory, lua2rs, cargo.sh, oracle.sh)
```

**Dependency rules.**

1. Dependencies point one way only: `pob-lua ← pob-gamedata ← pob-modparser ← pob-calc ← pob-ui(new)`.
2. `pob-oracle` depends on `pob-calc` with `features = ["probe"]` and on `pob-luahost`. **All** oracle and acceptance tests live in `crates/pob-oracle/tests/<unit>.rs`, so there is no dev-dependency cycle and no second copy of pob-calc.
3. `pob-lua`, `pob-gamedata`, `pob-modparser` and `pob-calc` MUST NOT depend on `mlua` or use `std::fs` outside `cfg(not(target_arch = "wasm32"))` loaders, and MUST pass `cargo check --target wasm32-unknown-unknown`.
4. `pob-calc` is single-threaded (`Rc`, `!Send`), mirroring PoB's single Lua VM. `pob-gamedata::Data` is immutable, `Send + Sync`, and held in a process global. Its only mutable "globals" are kept in a thread-local `DataSel` (§4.4).
5. The existing `pob-ui` and `pob-extract` keep compiling against `pob-luahost` through a dependency alias: `pob-calc = { package = "pob-luahost", path = "../pob-luahost" }`.

### 2.1 File tree and owners

Owner ids refer to §11. `[U0]` files are **frozen contract files** after Wave 1; they change only through a CCR (§10.3).

```
crates/pob-lua/src/
  lib.rs name.rs field.rs error.rs profile.rs                           [U0] (profile.rs: CCR-0001/0002)
  math.rs bit.rs format.rs string.rs pattern.rs(copied lua_pattern.rs)
  sort.rs ipairs.rs ljtab.rs                                            [P2]
crates/pob-gamedata/src/
  lib.rs data.rs loader.rs fnref.rs modifier.rs value.rs                [U0]
  schema/{skills,gems,bases,minions,uniques,affixes,tree,config_defs,
          mod_map_defs,calc_sections,stat_descriptions,misc}.rs          [U0]
  generated/mod.rs    (includes crates/pob-dump/generated/{global.rs,misc.rs} by path; one copy,
                       `pob-dump --codegen --check` guards them; CCR-0003)   [P1, generated, checked in]
  mod_tools.rs            (Modules/ModTools.lua 1-258 minus mergeKeystones) [CT]
  skill_stat_map.rs       (Modules/Data.lua 51-93, 1033-1077 fallback)       [CT]
  timeless_lut.rs         (DataLegion*/DataAbyss*/DataJewelFileLoader)       [LUT]
crates/pob-modparser/src/
  lib.rs types.rs                                                        [U0]
  helpers.rs scan.rs mod_cache.rs generated_lists.rs                     [MP0]
  form_list.rs mod_name_list.rs                                          [MP1]
  mod_flag_list.rs pre_flag_list.rs mod_tag_list.rs                      [MP2]
  special_a.rs [MP3]  special_b.rs [MP4]  special_c.rs [MP5]  special_d.rs [MP6]
  jewel_func_list.rs                                                     [MP7]
crates/pob-calc/src/
  lib.rs lua_table.rs flow.rs stub.rs env.rs actor.rs active_skill.rs
  cfg.rs breakdown.rs global_cache.rs calcs_facade.rs module_state.rs    [U0]
  probe.rs                                                               [ORC-X]
  classes/ mod.rs (types)                                                [U0]
           mod_store.rs mod_db.rs mod_list.rs                            [MS-A]
           eval_mod.rs                                                   [MS-B]
           item.rs        (Item.lua 1-423, 1646-2191; ItemSlotHelper; Utils) [IT-B]
           item_parse.rs  (Item.lua 424-1645)                            [IT-A]
           item_mods.rs   (Item.lua 2192-2736)  item_tools.rs (ItemTools.lua) [IT-C]
           passive_tree.rs (PassiveTree.lua 1-908, 1041-1062)            [PT]
           passive_spec.rs (1-1091)                                      [PS-A]
           passive_spec_paths.rs (1092-1696)                             [PS-B]
           passive_spec_cluster.rs (1697-2494)                           [PS-C]
  model/   mod.rs (Build + tab model structs)                            [U0]
           build_xml.rs items_tab.rs tree_tab.rs calcs_tab.rs xml.rs     [BM-A]
           skills_tab.rs party_tab.rs config_tab.rs                      [BM-B]
           config_build_mod_list.rs                                      [CO-C]
  modules/ common.rs calc_tools.rs calc_breakdown.rs calc_base.rs
           calc_format.rs mod_tools_env.rs                               [CT]
           calcs.rs (Calcs.lua)                                          [CA]
           power.rs (CalcsTab.lua 496-773)                               [PWR]
           calc_active_skill.rs                                          [AS]
           calc_setup/ mod.rs (InitEnvCtx + initEnv orchestrator)        [U0]
                       init_mod_db.rs node_mods.rs support.rs            [CS0]
                       init_env_base.rs [CS1] init_env_items.rs [CS2] init_env_skills.rs [CS3]
           calc_perform/ mod.rs (PerformCtx + orchestrator)              [U0]
                       actor_a.rs [PF-A] actor_b.rs [PF-B] setup_flasks.rs [PF-C]
                       pools_charges.rs [PF-D] buffs.rs [PF-E] party_curses.rs [PF-F]
                       ailments_dispatch.rs [PF-G]
           calc_defence/ mod.rs (DefenceCtx, DefEstCtx + orchestrators)  [U0]
                       helpers.rs [DF-A] primary.rs [DF-B] recovery.rs [DF-C]
                       est_inputs.rs [DF-D] est_pools.rs [DF-E] est_ehp.rs [DF-F] est_maxhit.rs [DF-G]
           calc_offence/ mod.rs (OffenceCtx, HitPassLocals, AilmentPassLocals + orchestrator) [U0]
                       helpers_setup.rs [OF-A] skill_types.rs [OF-B] costs_passes.rs [OF-C]
                       speed.rs [OF-D] crit.rs [OF-E] hit_dps.rs [OF-F]
                       ailments_a.rs [OF-G] ailments_b.rs [OF-H] dot_combined.rs [OF-I]
           calc_triggers/ mod.rs (TriggerConfig types)                   [U0]
                       handlers.rs [TR-A] config_table.rs [TR-B]
           calc_mirages.rs                                               [MI]
           config_options/ mod.rs (registry)                             [U0]
                       apply_a.rs [CO-A] apply_b.rs [CO-B]
           mod_map.rs                                                    [CO-C]
           jewel_funcs.rs                                                [MP7]
  data_fns/ mod.rs (registry + hash check)                               [U0]
            skills_act_int.rs [SK-A]  skills_act_dex.rs skills_act_str.rs skills_misc.rs [SK-B]
crates/pob-oracle/
  src/{lib,runner,patcher,schema,diff,nondet}.rs lua/{state_dump,probe,provenance}.lua
  src/bin/{pob_oracle,pob_calc_diff,pob_timing}.rs                        [P3]
  src/{inject,replay,testkit}.rs                                         [ORC-X]
  tests/<unit-id>.rs, corpus/<unit-id>/*.xml                             [each unit owns its own]
crates/pob-extract/src/dump/{mod.rs,walker.lua,canonical.rs,codegen.rs}  [P1]   compile.rs [U0]
scripts/port/{locals_audit,segments,provenance,globals_lint,pairs_inventory}.py, cargo.sh, oracle.sh [P4]
scripts/port/lua2rs/                                                     [T1]
docs/port/{stages.toml,scope.toml,STYLE.md,contracts.md,known_diffs.toml,ccr/}  [U0]
```

No file has two owners. A unit edits only its own files, its `crates/pob-oracle/tests/<unit>.rs` and its `crates/pob-oracle/corpus/<unit>/`.

---
## 3. Lua-semantics rules (copied verbatim into `docs/port/STYLE.md`; reviewers enforce them)

### 3.1 Naming and structure

1. **Keep identifiers verbatim.** This covers struct fields, functions, locals, table keys and output keys. The calc crates set `#![allow(non_snake_case, non_upper_case_globals, non_camel_case_types)]`. Rust keywords use raw identifiers (`r#type`, `r#override`, `r#mod`, `r#match`). Rust-only plumbing uses snake_case, so it is easy to tell apart.
2. **One Lua function becomes one Rust function** with the same name and the same parameter order. Lua methods `Class:Fn(...)` become `impl Class { pub fn Fn(&self, ...) }`. Varargs name lists become `&[Name]`.
3. **Keep statement order.** Never reorder, merge or "improve" statements. That includes float operation order, the order of `NewMod`/`AddMod` calls and loop order. Never fuse `a*b+c` into `mul_add`.
4. **Keep Lua comments.** Copy them verbatim as `//` comments. Put an anchor `// L<n>` before every function-level statement and every nested `if`/`for`/`while`.
5. **Mark provenance on everything.** Each file begins with `//! @pob-file <path> @16de4b82d`. Each ported fn or segment carries `/// @pob <path>:<a>-<b> h=<8hex>`. The hash is the first 8 hex digits of SHA-1 over those lines, each with trailing whitespace stripped and joined with `\n`. Deliberate exclusions use `// @pob-skip <path>:<a>-<b> reason="..."`, for example `Calcs.lua:38-88 infoDump`.

### 3.2 Translation table

| Lua | Rust |
|---|---|
| `local x = e` | `let mut x = e;` (same name) |
| `output.Life = v` / `output[dt.."Min"] = v` | `output.set(n!("Life"), v)` / `output.set(cat!(dt, "Min"), v)` |
| `output.X or 0` | `output.num0(n!("X"))` |
| `output.X or d` | `output.num_or(n!("X"), d)` |
| `if output.X then` (0 is **true**) | `if output.truthy(n!("X"))` |
| `output.X` in arithmetic (nil → Lua error) | `output.num(n!("X"))` (raises `LuaError` on nil) |
| `modDB:Sum("BASE", cfg, "A", "B")` | `modDB.Sum(BASE, cfg, nm!["A", "B"])` |
| `..., cond and "X" or nil` (nil names are dropped) | `&names([Some(n!("A")), cond.then(\|\| n!("X"))])` |
| `modDB:NewMod("X","BASE",v,"Src",ModFlag.Attack,{type="Condition",var="Y"})` | `modDB.NewMod(n!("X"), BASE, v, "Src", ModFlag::Attack, 0, &[tag!({ type = Condition, var = "Y" })])` |
| `a and b or c`, where b can never be falsy | `if a { b } else { c }` |
| `a and b or c`, where b may be falsy or nil | `lor(land(a, \|\| b), \|\| c)` on `Val`, or explicit `if` keeping the Lua result |
| `m_floor/m_ceil/m_min/m_max/m_modf/m_huge` | `pob_lua::math::*` (LuaJIT NaN and ±0 semantics) |
| `round/floor/ceil_b/floor_b/...` (Common.lua) | `pob_lua::math::*` |
| `s_format("%.2f", x)` / `tostring(n)` / `a..n` | `lfmt!("%.2f", x)` / `tostring_num(n)` / `cat!(a, n)` (`%.14g`) |
| `x % y` | `lmod(x, y)` (floored) |
| `x ^ y` | `lpow(x, y)` (LuaJIT `^`) |
| `#t` | `t.len()` (border; hole-free contract) |
| `for i, v in ipairs(t)` | `for_ipairs!((i, v) in t => { ... })`. It reads `t[i]` live on every step and stops at the first nil, exactly like Lua. |
| `for k, v in pairs(t)` | `for (k, v) in t.pairs() { ... }`. Keys are a snapshot; values are read live; keys whose live value is nil are skipped. Add `// LUA-ORDER` when the result can depend on order (§3.4). |
| `t_insert(t, v)` / `t_insert(t, i, v)` / `t_remove(t, i)` | `t.push(v)` / `t.insert(i, v)` / `t.remove(Some(i))`. Positions are `i64` and follow LuaJIT 2.1 `lib_table.c`: no bounds error; `remove` acts only for `1 <= i <= #t` (CCR-0011 §10). |
| `wipeTable(t)` / `copyTable(t)` / `copyTable(t, true)` | `t.wipe()` / `t.copyTable(false)` / `t.copyTable(true)`. `wipe` keeps the metatable; `copyTable` does not (Common.lua:477-487; only `copyTableSafe(…, preserveMeta)` copies it, 494-496; CCR-0011 §1). |
| `goto continue` | `continue` |
| `return` at the top level of a split function | the segment returns `Flow::Return` |
| `return calcs.initEnv(build, mode, override, specEnv)` | the segment returns `Flow::Restart` (§7.2) |
| `error(...)`, or an arithmetic, index or call on nil | `lua_error!("...")` |
| a read of an undeclared global (`dotCfg` at CalcOffence:3527) | literal `nil` / `None`, marked `// LUA-GLOBAL-NIL` |
| a string passed where a cfg is expected (`skillModList:More("MORE", cfg, "Accuracy")`, CalcOffence.lua:2194-2195) | `Some(ModCfg::STRING_CFG())` (all fields nil; the table that ends up in the name list matches nothing), marked `// LUA-QUIRK` |

### 3.3 Borrow rules (they also apply under `Rc`)

1. Never hold a `Ref`/`RefMut` across a call. Public APIs return **owned values or handles**, never references into a `RefCell`. `Field::with`/`Shared::with` closures MUST be leaf code: no calls into calc, ModStore or table methods on the same object. Debug builds report a violation as a panic that cites this rule.
2. Compute first, then write: `let v = q(...); output.set(k, v);`.
3. `Field<T>` requires `T: CheapClone`, which is Copy, an `Rc`/`Arc`/`Weak` handle, `Name`, `Table`, `Shared<_>`, or `Option` of these. Collections that Lua mutates in place and aliases are `Shared<Vec<_>>` / `Shared<IndexMap<_,_>>` / `Table`, never a `Field<Vec<_>>`.
4. `Rc` cycles are forbidden. Back-pointers (`ActiveSkill.actor`, `ModStore.actor`, `Actor.enemy/parent/player`) are `Weak`. `Env` owns its actors. `GlobalCache` owns cached envs, and wiping it frees them.

### 3.4 Iteration order policy

- **Integer-keyed tables iterated with `pairs` whose order is observable** (`env.allocNodes`, `env.grantedPassives`, `spec.allocNodes`, `extraRadiusNodeList`, `nodesInRadius`, and other node-id maps; the full list is in `docs/port/pairs_inventory.md`, produced by P4 and frozen by U0) use `pob_lua::ljtab::LjIntMap<V>`. It reproduces LuaJIT 2.1 `lj_tab.c` exactly: array part, hash part, `hashnum` main position, free-pointer collision chaining, rehash sizing, and `next()` order. Measured on the vendored LuaJIT: keys `{52031,1031,26725,55485,33545,6,7,1,2,3,99999,4184}` iterate as `1,2,3,6,7,26725,4184,55485,99999,1031,52031,33545` in every run.
- **String-keyed tables** use insertion order. PoB's own order is random per process (seeded hash, D8), so any output that depends on it is nondeterministic in PoB. The oracle detects such outputs (§9.5) and the diff compares them as sets of the values PoB produced.
- **Table-identity keys** (`pairs(affectedByAura)`, `isSupporting[srcInstance]`) also follow a random order in PoB (address hashing). Use insertion order (`PtrSet`/`PtrMap`).
- **ModParser `scan()`** iterates its pattern tables with `pairs` (ModParser.lua:6621), so full ties (same start, same end, same pattern length) are resolved randomly in PoB. Rust picks the lowest Lua source line. The L1 test enumerates full ties and accepts any of the candidates' outputs.

### 3.5 Numbers and strings

- Lua numbers are `f64` everywhere. Integers are used only for indices and ids.
- **Number mode and LuaJIT build profile (CCR-0001, CCR-0002).** PoB's reference LuaJIT is the x64 build: single-number (every number is a double, so `-x` of a zero is `-0`) with `cvttsd2si` float→int casts (NaN and out-of-range give MIN). **The shipped port always follows that profile, on every host and target** (desktop aarch64 and wasm32 included): Rust `f64` arithmetic matches it, and `pob_lua::{LJ_TARGET_X86ORX64, LJ_DUALNUM}` are the constants `true`/`false`. ARM64 LuaJIT builds (this project's oracle host) are dual-number: integer-valued results are int32 values without `-0` (`-tonumber("0")` and the literal `-0` give `+0` in the interpreter, `-0` once traced), casts saturate (`string.format("%d", 1e30)` = `9223372036854775807`, `("%d", 0/0)` = `0`), and `tonumber("-0", base)` / `tonumber("-0b0")` are `+0`. That behaviour exists in `pob-lua` only behind the test-only feature `oracle-dualnum` (`pob_lua::profile::with_luajit_arm64_profile`), which the L0 tests use to explain host differences (§9.4). Oracle parity runs use the LuaJIT interpreter (`jit.off()`, §9.2), and the diff treats a `±0` sign mismatch as a host artefact, never as a port bug; values derived from the sign (`1/x` = `±inf`, a formatted `"-0"`) are recorded in `known_diffs.toml` citing CCR-0002.
- **LuaJIT traces are not the reference.** Compiled traces of the vendored LuaJIT differ from its interpreter: `string.sub(s, i, j)` with a variable negative `j` returns the wrong substring in traces, trace narrowing drops the sign of zero of `math.floor`/`math.ceil` results, and the JIT folds `x^2` to `x*x` (1 ulp from libm `pow` for some `x`). The port follows the interpreter (CCR-0001).
- **Flags.** Flag bit operations use `u32` (the largest flag is `MatchAll = 0x40000000`, so bit 31 is unused). `bnot` only ever appears inside `band` masks; U0's grep test asserts this.
- **Number formatting.** `tostring(number)` is `%.14g` with LuaJIT spellings (`inf`, `-inf`, `nan`, `-0`; LuaJIT 2.1's own formatter never prints a NaN sign). LuaJIT's `%.14g` rounds exact decimal ties up (`nd_add_m10e`), unlike glibc `printf`: `tostring(123456789012345)` = `1.2345678901235e+14`. `string.format` is a port of `lj_strfmt` (§4.1).
- **Division.** Division by zero yields IEEE inf/NaN. Never clamp or guard where Lua does not.
- **`m_modf` truncates toward zero.** In `subMod.value = m_modf(round(x, 2))` (ModStore.lua:84) only the first return value is used.
- **`ModStore:Max` drops non-positive values.** It returns nil when every value is ≤ 0, because it starts from `val > (max or 0)` (ModStore.lua:248-257). Port literally.
- **Strings are byte strings with ASCII case mapping** (PoB is ASCII). Lua patterns go through `pob_lua::pattern` (the lstrlib port).

### 3.6 Mutation of mod tables

`ModRef = Arc<Mod>` is immutable once a store holds it. U0's audit (`docs/port/contracts.md#mod-mutation`) lists every Lua site that mutates a mod in place: ModStore.lua 425-430/514-516/564-566, CalcPerform.lua 2452/2539/2564/2610/2718/2846/2921/3881/3897, CalcActiveSkill.lua:38, Item.lua 2336/2558, ModTools.lua 240-242. Each site has been verified to mutate a **fresh copy**: `copyTable` immediately before the mutation, a `copyTable(buffExports.Aura[...])` (CalcPerform.lua:3870), or a mod under construction. Rust ports these as "build a new `Mod`, then insert". A newly found site whose target is shared MUST be filed as a CCR before porting.

---

## 4. Core shared Rust code

All code in §4-§8 is the contract. U0 types it in exactly. Bodies shown as `…` belong to the unit named in the comment.

### 4.1 `pob-lua`

```rust
// ===== name.rs [U0] =====
pub type Name = ustr::Ustr;                       // interned, Copy, precomputed hash, Send + Sync

/// Literal key; one intern per call site (OnceLock), then a plain load.
#[macro_export] macro_rules! n {
    ($s:literal) => {{ static N: ::std::sync::OnceLock<$crate::Name> = ::std::sync::OnceLock::new();
                       *N.get_or_init(|| $crate::Name::from($s)) }};
}
/// `"A", "B"` query name lists.
#[macro_export] macro_rules! nm { ($($s:literal),* $(,)?) => { &[$($crate::n!($s)),*][..] }; }
/// Lua `a .. b .. c` producing an interned Name. Numbers use tostring (%.14g).
/// Formats into a 128-byte stack buffer (heap only beyond that), then calls `intern`, which consults
/// a thread-local memo keyed on the concatenated bytes before touching the global interner (a (ptr,len)
/// key is unsound for transient parts; CCR-0001).
#[macro_export] macro_rules! cat {
    ($($p:expr),+ $(,)?) => {{ let mut b = $crate::name::CatBuf::new();
        $( $crate::format::LuaConcat::concat_into(&$p, &mut b); )+ b.intern() }};
}
pub struct CatBuf { /* smallvec::SmallVec<[u8; 128]> + memo key */ }
impl CatBuf { pub fn new() -> Self; pub fn push_str(&mut self, s: &str); pub fn intern(self) -> Name; }
/// Lua drops nil names in ModStore varargs: `names([Some(a), cond.then(|| b)])`.
pub fn names<const N: usize>(xs: [Option<Name>; N]) -> smallvec::SmallVec<[Name; 4]> { xs.into_iter().flatten().collect() }
/// "Condition:"..var / "Multiplier:"..var — ModStore.lua 20-28 memo tables.
pub fn conditionName(var: Name) -> Name;
pub fn multiplierName(var: Name) -> Name;

// ===== field.rs [U0] =====
/// Marker: cloning is O(1) and allocation-free.
pub trait CheapClone: Clone {}
impl CheapClone for f64 {} impl CheapClone for bool {} impl CheapClone for u32 {} impl CheapClone for i64 {}
impl CheapClone for usize {} impl CheapClone for Name {}
impl<T: ?Sized> CheapClone for std::rc::Rc<T> {} impl<T: ?Sized> CheapClone for std::sync::Arc<T> {}
impl<T> CheapClone for std::rc::Weak<T> {} impl<T: CheapClone> CheapClone for Option<T> {}
impl<T> CheapClone for Shared<T> {}

/// A reassignable field of an aliased Lua table. Never hands out references.
#[derive(Default)]
pub struct Field<T: CheapClone>(std::cell::RefCell<T>);
impl<T: CheapClone> Field<T> {
    pub fn new(v: T) -> Self { Self(std::cell::RefCell::new(v)) }
    #[inline] pub fn get(&self) -> T { self.0.borrow().clone() }
    #[inline] pub fn set(&self, v: T) { *self.0.borrow_mut() = v; }
    #[inline] pub fn replace(&self, v: T) -> T { self.0.replace(v) }
}
impl<T: CheapClone + Default> Field<T> { pub fn take(&self) -> T { self.0.take() } }

/// A Lua table with a fixed Rust shape that is aliased and mutated in place (lists, maps).
pub struct Shared<T>(std::rc::Rc<std::cell::RefCell<T>>);
impl<T> Clone for Shared<T> { fn clone(&self) -> Self { Self(self.0.clone()) } }
impl<T> Shared<T> {
    pub fn new(v: T) -> Self;
    pub fn ptr_eq(&self, o: &Self) -> bool;
    /// Leaf-only closures (§3.3).
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R;
    pub fn with_mut<R>(&self, f: impl FnOnce(&mut T) -> R) -> R;
}
impl<T: Clone> Shared<Vec<T>> {
    pub fn len(&self) -> usize; pub fn get(&self, i1: usize) -> Option<T>;   // 1-based, Lua t[i]
    pub fn push(&self, v: T); pub fn insert(&self, i1: usize, v: T); pub fn remove(&self, i1: Option<usize>) -> Option<T>;
    pub fn set(&self, i1: usize, v: T); pub fn wipe(&self); pub fn snapshot(&self) -> Vec<T>;
}
/// Lua tables keyed by table identity ({[srcInstance] = true}).
pub struct PtrSet<T>(/* Vec<Rc<T>> by address, insertion order */);
pub struct PtrMap<K, V>(/* Vec<(Rc<K>, V)> */);

// ===== error.rs [U0] =====
#[derive(Debug, Clone)] pub struct LuaError(pub String);
#[macro_export] macro_rules! lua_error { ($($t:tt)*) => { ::std::panic::panic_any($crate::error::LuaError(format!($($t)*))) } }
/// Desktop only (wasm32-unknown-unknown is panic=abort; see D15).
#[cfg(not(target_arch = "wasm32"))]
pub fn catch<R>(f: impl FnOnce() -> R) -> Result<R, LuaError>;

// ===== math.rs [P2] (LuaJIT math.* + Common.lua 709-800, 1013-1023) =====
pub const m_huge: f64 = f64::INFINITY; pub const m_pi: f64 = std::f64::consts::PI;
pub fn m_floor(x: f64) -> f64; pub fn m_ceil(x: f64) -> f64; pub fn m_abs(x: f64) -> f64;
pub fn m_sqrt(x: f64) -> f64; pub fn m_exp(x: f64) -> f64; pub fn m_log(x: f64, base: Option<f64>) -> f64;
pub fn m_modf(x: f64) -> (f64, f64);
pub fn m_min(a: f64, b: f64) -> f64;                 // LuaJIT NaN / ±0 behaviour, oracle-verified
pub fn m_max(a: f64, b: f64) -> f64;
pub fn m_min_n(xs: &[f64]) -> f64; pub fn m_max_n(xs: &[f64]) -> f64;
pub fn lmod(a: f64, b: f64) -> f64;                  // a - floor(a/b)*b
pub fn lpow(a: f64, b: f64) -> f64;                  // LuaJIT `^` = libm pow, as BC_POW calls it (the JIT folds x^2
                                                     // to x*x; the port follows the interpreter; operands black_boxed)
pub fn round(val: f64, dec: Option<i32>) -> f64;     // Common.lua `round`
pub fn floor(val: f64, dec: Option<i32>) -> f64;     // Common.lua `floor(val, dec)` (+0.0001 fudge)
pub fn roundSymmetric(val: f64, dec: Option<i32>) -> f64; pub fn floorSymmetric(val: f64, dec: Option<i32>) -> f64;
pub fn ceilSymmetric(val: f64, dec: Option<i32>) -> f64;  pub fn alwaysPositiveRound(val: f64, dec: Option<i32>) -> f64;
pub fn triangular(n: f64) -> f64; pub fn ceil_b(x: f64, base: f64) -> f64; pub fn floor_b(x: f64, base: f64) -> f64;

// ===== bit.rs [P2] =====
pub fn band(a: u32, b: u32) -> u32; pub fn bor(a: u32, b: u32) -> u32; pub fn bxor(a: u32, b: u32) -> u32;
pub fn bnot(a: u32) -> u32; pub fn lshift(a: u32, n: u32) -> u32; pub fn rshift(a: u32, n: u32) -> u32;
pub fn tobit(x: f64) -> i32;

// ===== format.rs [P2] =====
pub enum FmtArg<'a> { Num(f64), Str(&'a str), Bool(bool), Nil }
impl From<f64> for FmtArg<'_> { /* … */ }  /* + &str, &String, Name, &Name, &LStr (FmtArg::Str borrows), bool, i64, Option<f64> */
/// lj_strfmt: %d %i %u %c %x %X %o %e %E %f %g %G %q %s %%, flags `-+ #0`, width, precision.
/// `%d` on a non-integral number follows LuaJIT 2.1 (oracle-verified), and `%s` of a number uses tostring_num.
pub fn lua_format(fmt: &str, args: &[FmtArg]) -> String;
#[macro_export] macro_rules! lfmt { ($f:expr $(, $a:expr)*) => { $crate::format::lua_format($f, &[$($crate::format::FmtArg::from($a)),*]) } }
pub fn tostring_num(x: f64) -> String;
pub fn tonumber(s: &str) -> Option<f64>;             // lua_str2number: hex, surrounding spaces, exponents
pub fn tonumber_base(s: &str, base: u32) -> Option<f64>;
pub trait LuaConcat { fn concat_into(&self, out: &mut crate::name::CatBuf); }   // &str, Name, LStr, f64, i64

// ===== string.rs + pattern.rs [P2] (byte-wise, ASCII) =====
pub fn lower(s: &str) -> String; pub fn upper(s: &str) -> String;
pub fn sub(s: &str, i: i64, j: Option<i64>) -> std::borrow::Cow<'_, str>;   // 1-based, negative indices; byte-wise
                                                     // (`sub_bytes` is the reference; Owned only when a UTF-8 char is split)
pub fn rep(s: &str, n: usize) -> String; pub fn byte(s: &str, i: i64) -> Option<u8>;
pub use crate::pattern::{find, r#match, gmatch, gsub, gsub_fn, gsub_table, Captures, Capture};
/// PoB Common.lua helpers used by calc/model: split, firstToUpper, sanitiseText (306), stripEscapes.
pub fn split(s: &str, sep: &str) -> Vec<String>; pub fn sanitiseText(s: &str) -> String;

// ===== sort.rs [P2] =====
/// Exact port of LuaJIT lib_table.c auxsort: the same permutation for comparators with ties,
/// and "invalid order function for sorting" raised as LuaError where LuaJIT raises it.
pub fn sort<T>(v: &mut [T], lt: impl FnMut(&T, &T) -> bool);

// ===== ipairs.rs [P2] =====
/// for_ipairs!((i, v) in list => body): reads list[i] live each step and stops at the first nil.
#[macro_export] macro_rules! for_ipairs { /* … */ }
/// Vec<Option<T>> with holes punched during iteration (createActiveSkill rejectedSupportsIndices, CalcActiveSkill.lua:49-54).
pub fn ipairs_live<T: Clone>(v: &mut Vec<Option<T>>, f: impl FnMut(usize, T, &mut Vec<Option<T>>));

// ===== profile.rs [U0] (CCR-0001/0002) =====
/// The LuaJIT build profile the port follows: always x64 single-number (§3.5).
pub const LJ_TARGET_X86ORX64: bool = true; pub const LJ_DUALNUM: bool = false;
pub fn x64_casts() -> bool; pub fn dualnum() -> bool;          // constant true / false without the feature
/// Feature `oracle-dualnum` ONLY (pob-lua's own dev-dependency on itself; no shipping crate enables it):
/// run `f` with the ARM64 dual-number profile on this thread (saturating casts, int32 `tonumber` zeros).
#[cfg(feature = "oracle-dualnum")] pub fn with_luajit_arm64_profile<R>(f: impl FnOnce() -> R) -> R;

// ===== ljtab.rs [P2] =====
/// Integer-keyed Lua table whose `pairs()` order is LuaJIT 2.1's (D8). Values are opaque to the layout.
pub struct LjIntMap<V> { /* array part Vec<Option<V>>, hash nodes [ (key, val, next) ], free ptr, sizes */ }
impl<V: Clone> LjIntMap<V> {
    pub fn new() -> Self;                              // `{}`
    pub fn with_sizes(narray: u32, nhash: u32) -> Self; // lua_createtable / table constructors
    pub fn get(&self, k: i64) -> Option<V>;
    pub fn set(&mut self, k: i64, v: Option<V>);       // None = assign nil (keeps the dead node, as LuaJIT does)
    pub fn len(&self) -> usize;                        // `#t` (lj_tab_len)
    pub fn pairs(&self) -> Vec<(i64, V)>;              // lj_tab_next order; snapshot of keys, live values
    pub fn copyTable(&self) -> Self;                   // Common.lua copyTable: new table filled in pairs order
}
```

### 4.2 `pob-gamedata`: generated enums (P1 output; shape fixed here)

The files are generated into `crates/pob-dump/generated/{global.rs,misc.rs}` and included by path from `pob-gamedata/src/generated/mod.rs`, so there is one copy; `pob-dump --codegen --check` guards them (CCR-0003). Every item carries a `/// @pob Data/Global.lua:<a>-<b> h=<8hex>` marker and the file a `//! @pob-file Data/Global.lua @16de4b82d` header (§3.1 rule 5).

```rust
// generated/global.rs — GENERATED from Data/Global.lua @16de4b82d by `pob-dump --codegen`. Do not edit.
#![allow(non_upper_case_globals, non_snake_case)]
pub mod ModFlag {
    pub const Attack: u32 = 0x0000_0001; pub const Spell: u32 = 0x0000_0002; /* … every entry … */
    pub const WeaponMask: u32 = /* … */ 0;
    pub const ENTRIES: &[(&str, u32)];                   // source order
    pub fn by_name(s: &str) -> Option<u32>;
}
pub mod KeywordFlag {
    pub const Aura: u32 = 0x0000_0001; /* … */ pub const MatchAll: u32 = 0x4000_0000;
    pub const ENTRIES: &[(&str, u32)];
    pub fn by_name(s: &str) -> Option<u32>;              // KeywordFlag[damageType.."Dot"]
}
/// Global.lua:174-214 (needs the `band`/`MatchAllMask` locals at 174-175; the cache is a pure memo, so it is
/// not reproduced).
pub fn MatchKeywordFlags(keywordFlags: u32, modKeywordFlags: u32) -> bool;
/// Global.lua:179-184; a no-op (no memo). CalcSetup.lua:476 calls it.
pub fn ClearMatchKeywordFlagsCache();
pub mod SkillType {
    pub type T = u8;
    pub const Attack: T = 1; pub const Spell: T = 2; /* … */ pub const Area: T = 8; /* … */
    pub const Aura: T = 39; /* … */ pub const OR: T = 73; pub const AND: T = 74; pub const NOT: T = 75; /* … */
    pub const Pact: T = 135;
    pub const COUNT: usize = 136;
    pub const ENTRIES: &[(&str, T)];                     // 135 entries incl. OR/AND/NOT, source order
    pub fn by_name(s: &str) -> Option<T>; pub fn name(t: T) -> Option<&'static str>;
}
/// requireSkillTypes / excludeSkillTypes RPN tokens. The data holds SkillType numbers; the operators are
/// SkillType.OR (73), SkillType.AND (74), SkillType.NOT (75) (Global.lua:291-293; act_int.lua:483 is
/// `{ SkillType.Instant, SkillType.Spell, SkillType.AND }`). The schema maps numbers through `from_skill_type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SkillTypeExpr { Type(SkillType::T), AND, OR, NOT }
impl SkillTypeExpr { pub fn from_skill_type(t: SkillType::T) -> Self; }   // 73/74/75 -> OR/AND/NOT
/// `{[SkillType.X] = true}`; 136 bits.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub struct SkillTypeSet(pub [u64; 3]);
impl SkillTypeSet { pub fn has(&self, t: SkillType::T) -> bool; pub fn set(&mut self, t: SkillType::T, v: bool);
                    pub fn iter(&self) -> impl Iterator<Item = SkillType::T> + '_; }
pub static colorCodes: &[(&str, &str)] = &[("NORMAL", "^xC8C8C8") /* … */];
```

`generated/misc.rs` holds four record structs: `Misc` (`data.misc`, Modules/Data.lua:236-304), `GameConstants`, `CharacterConstants` and `MonsterConstants` (Data/Misc.lua). Keys that are not Rust identifiers become fields with `%`→`pct`, `+`→`plus`, `-`→`minus` and `#[serde(rename = "<verbatim key>")]`; each struct has `get("<verbatim key>")`, so `data.characterConstants["base_maximum_all_resistances_%"]` ports as `.get("base_maximum_all_resistances_%")` or the field.

### 4.3 `pob-gamedata`: Mod, Tag, Value (U0, adapted from today's `pob-calc/src/modifier.rs`)

```rust
#![allow(non_snake_case, non_camel_case_types)]
use crate::Name; use std::sync::Arc;

pub type ModRef = Arc<Mod>;                      // clone = refcount; immutable once inserted (§3.6)
pub type AStr = Arc<str>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ModType { BASE, INC, MORE, FLAG, OVERRIDE, LIST, MAX, MIN, Flag /* "Flag" (CanNotUseItem) */, CHANCE, DUMMY,
                   Nil /* a nil type: sup_str.lua:1448 mod("CorruptingCryStagesFromExerted", nil, 0, …) */ }

/// Immutable Lua value carried by mods and data (Send + Sync).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Value {
    #[default] Nil,
    Bool(bool),
    Num(f64),
    Str(AStr),
    Mod(ModRef),                                 // the `mod` member of `{ mod = … }` payloads
    Table(Arc<ValueTable>),                      // { key=…, value=… }, { mod=…, onlyAllies=… }, arrays …
    JewelFunc(Arc<JewelFuncSpec>),               // replaces the Lua closure in JewelFunc/ExtraJewelFunc LIST mods
}
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ValueTable { pub array: Vec<Value>, pub fields: Vec<(Name, Value)> }   // field order = dump order
impl ValueTable { pub fn get(&self, k: Name) -> &Value; pub fn with(&self, k: Name, v: Value) -> ValueTable; }
impl Value {
    pub fn truthy(&self) -> bool;  pub fn num(&self) -> Option<f64>;  pub fn get(&self, k: Name) -> &Value;
    pub fn inner_mod(&self) -> Option<&ModRef>;
}
/// Conversions the generated ModParser/ConfigOptions code relies on (CCR-0006 §4): `From<T> for Value` for
/// f64, i32, Option<f64> (None → Nil), bool, &str, String, AStr, Name, Option<Name> (None → Nil), ValueTable,
/// ModRef and Mod (→ `Value::Mod(Arc::new(m))`, the `{ mod = … }` payloads).

/// A ModParser jewel closure as data: which jewelFuncList entry it is, plus the captured upvalues.
#[derive(Debug, Clone, PartialEq)]
pub struct JewelFuncSpec {
    pub key: Name,                               // lowercased jewelFuncList key, or "$default" (ModParser.lua 6552-6595)
    pub kind: JewelFuncKind,
    pub args: smallvec::SmallVec<[Value; 4]>,    // captures, as passed by the defining entry
    pub r#type: Option<Name>,                    // { type = "Self"/"SelfUnalloc"/"Threshold"/"Other" }
    pub radius: Option<Name>,                    // { radius = "Large" } (ExtraJewelFunc, ModParser.lua:2622/2630)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum JewelFuncKind { Other, Self_, SelfUnalloc, Threshold }

#[derive(Debug, Clone, PartialEq)]
pub struct Mod {
    pub name: Name,
    pub r#type: ModType,
    pub value: Value,
    pub flags: u32,
    pub keywordFlags: u32,
    pub source: Option<AStr>,
    pub tags: smallvec::SmallVec<[Tag; 1]>,      // Lua mod[1], mod[2], …
    pub extra: Option<Box<ValueTable>>,          // rare non-standard keys (e.g. sourceSlot); keeps round-trips lossless:
                                                 // "flags" = a tag table given as flags (SkillStatMap.lua:509; MS-A raises
                                                 // where Lua's band raises), "#n" = a tag at a non-sequence index (holey
                                                 // tags, mod[1] = nil; ipairs never sees it)
}
impl Mod { pub fn source_category(&self) -> Option<&str>; /* mod.source:match("[^:]+") */ }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TagType { #[default] Condition, ActorCondition, Multiplier, MultiplierThreshold, PerStat, PercentStat,
    StatThreshold, DistanceRamp, MeleeProximity, Limit, ItemCondition, SocketedIn, SkillName, SkillId, SkillPart,
    SkillType, BaseFlag, SlotName, ModFlagOr, KeywordFlagAnd, MonsterTag,              // the 21 EvalMod branches
    GlobalEffect, InSlot, SlotNumber, DisablesItem, IgnoreCond,
    Global,      // { type = "Global" } (1252 census hits; inert in EvalMod; ModParser.lua:427/1087/2410/5013)
    Skill,       // 2 census hits
    LuaNonTable, // a non-table tag entry (SkillStatMap.lua:1735 passes flags into makeSkillDataMod's varargs);
                 // the raw value is in Tag.extra["$value"]
    Other(Name) }

/// One PoB tag table. Field names are verbatim Lua keys, so `tag!({ type = Condition, var = "X" })`
/// reads like `{ type = "Condition", var = "X" }`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tag {
    pub r#type: TagType,
    pub var: Option<Name>, pub varList: Option<Arc<[Name]>>, pub neg: bool, pub actor: Option<Name>,
    pub stat: Option<Name>, pub statList: Option<Arc<[Name]>>, pub div: Option<f64>, pub divVar: Option<Name>,
    pub base: Option<f64>, pub noFloor: bool, pub invert: bool, pub floor: bool,
    pub limit: Option<f64>, pub limitVar: Option<Name>, pub limitStat: Option<Name>, pub limitTotal: bool,
    pub limitNegTotal: bool, pub limitActor: Option<Name>, pub percent: Option<f64>, pub percentVar: Option<Name>,
    pub threshold: Option<f64>, pub thresholdVar: Option<Name>, pub thresholdStat: Option<Name>,
    pub thresholdActor: Option<Name>, pub thresholdPercent: Option<f64>, pub thresholdPercentVar: Option<Name>,
    pub upper: bool, pub equals: bool, pub ramp: Option<Ramp>,
    pub itemSlot: Option<Name>, pub allSlots: bool, pub bothSlots: bool, pub excludeSelf: bool,
    pub searchCond: Option<AStr>, pub rarityCond: Option<Name>, pub corruptedCond: Option<bool>,
    pub shaperCond: Option<bool>, pub elderCond: Option<bool>, pub nameCond: Option<AStr>,
    pub slotName: Option<Name>, pub slotNameList: Option<Arc<[Name]>>, pub keyword: Option<Name>,
    pub socketColor: Option<Name>, pub sockets: Option<Sockets>,
    pub skillName: Option<Name>, pub skillNameList: Option<Arc<[Name]>>, pub includeTransfigured: bool,
    pub summonSkill: bool, pub skillId: Option<Name>, pub skillPart: Option<f64>, pub skillPartList: Option<Arc<[f64]>>,
    pub skillType: Option<SkillType::T>, pub skillTypeList: Option<Arc<[SkillType::T]>>, pub baseFlag: Option<Name>,
    pub modFlags: u32, pub keywordFlags: u32, pub monsterTag: Option<Name>, pub monsterTagList: Option<Arc<[Name]>>,
    pub globalLimit: Option<f64>, pub globalLimitKey: Option<Name>, pub num: Option<f64>,
    pub effectType: Option<Name>, pub effectName: Option<Name>, pub effectCond: Option<Name>,
    pub effectEnemyCond: Option<Name>, pub effectStackVar: Option<Name>, pub effectStackLimit: Option<f64>,
    pub modCond: Option<Name>, pub unscalable: bool, pub allowTotemBuff: bool, pub applyNotPlayer: bool,
    pub applyMinions: bool,
    pub excludeItemType: Option<Name>,           // { type = "DisablesItem", excludeItemType = "Tincture" } (ModParser.lua:5256;
                                                 // read at CalcSetup.lua:952/970)
    pub extra: Option<Box<ValueTable>>,          // any other key (U0 census over dump + ModCache + parse oracle). A numeric
                                                 // key that holds a Lua string (`div = "10"`, `limit`, `percent`) keeps
                                                 // tonumber(s) in the field and the string here under the same key
}
/// `tag.ramp`: DistanceRamp's `{ {35,0}, {70,1} }` points, or MeleeProximity's flat `{ max, min }` pair
/// (ModParser.lua:3005; read as tag.ramp[1]/[2] at ModStore.lua:615-630).
#[derive(Debug, Clone, PartialEq)] pub enum Ramp { Points(Arc<[[f64; 2]]>), Flat(Arc<[f64]>) }
#[derive(Debug, Clone, PartialEq)] pub enum Sockets { All, Count(f64), List(Arc<[f64]>) }
#[macro_export] macro_rules! tag { ({ type = $t:ident $(, $k:ident = $v:expr)* $(,)? }) => {{
    let mut t = $crate::Tag { r#type: $crate::TagType::$t, ..Default::default() };
    $( t.$k = $crate::modifier::IntoTagField::into_tag_field($v); )* t }}; }
/// &str/Name/Option<Name> → Option<Name>; &str → Option<AStr>; f64/i32/Option<f64> → Option<f64>; bool → bool and
/// Option<bool>; u32 → u32; SkillType::T → Option<SkillType::T>; &[&str]/&[Name]/Vec<Name>, &[f64], &[SkillType::T]
/// → the matching Option<Arc<[_]>>; &[[f64; 2]]/Ramp → Option<Ramp>; Sockets → Option<Sockets> (CCR-0006 §4).
pub trait IntoTagField<T> { fn into_tag_field(self) -> T; }

// mod_tools.rs [CT] — ModTools.lua
pub fn createMod(modName: Name, modType: ModType, modVal: impl Into<Value>, source: Option<&str>,
                 flags: u32, keywordFlags: u32, tags: &[Tag]) -> Mod;
pub fn parseTags(line: &str) -> Vec<Tag>;
pub fn parseFormattedSourceMod(line: &str) -> Option<Mod>;
pub fn compareModParams(a: &Mod, b: &Mod) -> bool;
pub fn formatFlags(flags: u32, src: &[(&str, u32)]) -> String; pub fn formatTag(t: &Tag) -> String;
pub fn formatTags(t: &[Tag]) -> String; pub fn formatValue(v: &Value) -> String;
pub fn formatModParams(m: &Mod) -> String; pub fn formatMod(m: &Mod) -> String; pub fn formatSourceMod(m: &Mod) -> String;
pub fn setSource(m: &Mod, source: &str) -> Mod;           // returns the re-sourced copy (ModTools.lua:239-246)
```

### 4.4 `pob-gamedata`: data schema and access (U0; P1's key inventory finalises the field lists)

```rust
pub type GrantedEffectRef = Arc<GrantedEffect>;  pub type GemDataRef = Arc<GemData>;
pub type ItemBaseRef = Arc<ItemBase>;            pub type MinionDataRef = Arc<MinionData>;
pub type SkillLevelRef = Arc<SkillLevel>;        pub type NameSet = Arc<ahash::AHashSet<Name>>;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq, Hash)]
pub struct LuaFnRef { #[serde(rename = "$fn")] pub id: Name, pub src: String, pub hash: String }

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct GrantedEffect {
    pub id: Name, pub name: Name,
    #[serde(default)] pub baseTypeName: Option<Name>, #[serde(default)] pub color: Option<u8>,
    #[serde(default)] pub description: Option<AStr>, #[serde(default)] pub support: bool,
    #[serde(default)] pub hidden: bool, #[serde(default)] pub fromItem: bool, #[serde(default)] pub fromTree: bool,
    #[serde(default)] pub plusVersionOf: Option<Name>, #[serde(default)] pub isTrigger: bool,
    #[serde(default)] pub skillTypes: SkillTypeSet, #[serde(default)] pub minionSkillTypes: Option<SkillTypeSet>,
    #[serde(default)] pub addSkillTypes: Vec<SkillType::T>,
    #[serde(default)] pub requireSkillTypes: Vec<SkillTypeExpr>, #[serde(default)] pub excludeSkillTypes: Vec<SkillTypeExpr>,
    #[serde(default)] pub addMinionTypes: Vec<SkillType::T>, #[serde(default)] pub ignoreMinionTypes: bool,
    #[serde(default)] pub supportGemsOnly: bool,
    #[serde(default)] pub weaponTypes: Option<NameSet>,
    #[serde(default)] pub baseFlags: NameSet, #[serde(default)] pub addFlags: Option<NameSet>,
    #[serde(default)] pub statDescriptionScope: Option<Name>, #[serde(default)] pub castTime: Option<f64>,
    #[serde(default)] pub baseEffectiveness: Option<f64>, #[serde(default)] pub incrementalEffectiveness: Option<f64>,
    #[serde(default)] pub parts: Vec<SkillPart>,
    #[serde(default)] pub baseMods: Vec<ModRef>,
    #[serde(default)] pub qualityStats: Vec<(Name, f64)>, #[serde(default)] pub constantStats: Vec<(Name, f64)>,
    #[serde(default)] pub stats: Vec<Name>, #[serde(default)] pub notMinionStat: Vec<Name>,
    pub levels: LevelTable,                      // 1-based, holes preserved
    #[serde(default)] pub statMap: ahash::AHashMap<Name, StatMapEntry>,   // OWN entries only; use statMapLookup()
    #[serde(default)] pub minionList: Vec<Name>, #[serde(default)] pub skillTotemId: Option<f64>,
    pub modSource: AStr,                         // "Skill:<id>"
    #[serde(default)] pub hasGlobalEffect: bool,
    // closures: identity only; the implementations live in pob_calc::data_fns
    #[serde(default)] pub initialFunc: Option<LuaFnRef>, #[serde(default)] pub preSkillTypeFunc: Option<LuaFnRef>,
    #[serde(default)] pub preDamageFunc: Option<LuaFnRef>, #[serde(default)] pub postCritFunc: Option<LuaFnRef>,
    #[serde(default)] pub explosiveArrowFunc: Option<LuaFnRef>, #[serde(default)] pub preDotFunc: Option<LuaFnRef>,
}
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct LevelTable(pub Vec<Option<SkillLevelRef>>);    // index = level-1
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct SkillLevel {
    pub values: Vec<f64>,                        // positional "#1".."#n"
    #[serde(default)] pub levelRequirement: f64, #[serde(default)] pub statInterpolation: Vec<u8>,
    #[serde(default)] pub cost: Vec<(Name, f64)>,
    #[serde(default)] pub extra: Vec<(Name, Value)>,   // exactly the census keys (SkillLevel::EXTRA_KEYS: manaMultiplier,
                                                       // critChance, damageEffectiveness, cooldown, storedUses, …); others fail the load
}
impl SkillLevel { pub fn get(&self, k: Name) -> &Value; pub fn num(&self, k: Name) -> Option<f64>; }
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct StatMapEntry {                        // a statMap[stat] table: array of mods/groups + value/mult/div/base
    pub items: Vec<StatMapItem>, pub value: Option<Value>, pub mult: Option<f64>, pub div: Option<f64>, pub base: Option<f64>,
    pub skillFlag: Option<Name>,                 // skillFlag = "permanentMinion" (data.skillStatMap, 2 entries)
}
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub enum StatMapItem { Mod(ModRef), Group { mods: Vec<ModRef>, value: Option<Value>, mult: Option<f64>, div: Option<f64>, base: Option<f64> } }
#[derive(Debug, serde::Deserialize, serde::Serialize)]
/// A part's other keys are boolean skill flags (CalcActiveSkill.lua:257-263 copies true/false into skillFlags).
pub struct SkillPart { pub name: AStr, pub flags: Vec<(Name, bool)> }

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct GemData { pub id: Name, pub name: Name, pub gameId: Name, pub variantId: Name, pub baseTypeName: Name,
    pub grantedEffect: GrantedEffectRef, pub secondaryGrantedEffect: Option<GrantedEffectRef>,
    pub grantedEffectList: Vec<GrantedEffectRef>, pub vaalGem: bool, pub tags: NameSet, pub tagString: AStr,
    pub reqStr: f64, pub reqDex: f64, pub reqInt: f64, pub naturalMaxLevel: f64 }

/// The PoB global `data` (post-processed). Immutable.
pub struct Data {
    pub skills: indexmap::IndexMap<Name, GrantedEffectRef>,   // dump order
    pub gems: indexmap::IndexMap<Name, GemDataRef>,            // dump order
    /// The Lua values are gem ids, not gem tables (`data.gems[data.gemForSkill[ge]]`); `Data::gem_for_skill` does
    /// the double lookup.
    pub gemForSkill: ahash::AHashMap<Name, Name>, pub gemForBaseName: ahash::AHashMap<AStr, Name>,
    pub skillStatMap: ahash::AHashMap<Name, StatMapEntry>,
    pub itemBases: indexmap::IndexMap<AStr, ItemBaseRef>,
    pub minions: indexmap::IndexMap<Name, MinionDataRef>, pub spectres: indexmap::IndexMap<Name, MinionDataRef>,
    pub highPrecisionMods: ahash::AHashMap<Name, ahash::AHashMap<ModType, i32>>, pub defaultHighPrecision: i32,
    pub misc: generated::misc::Misc,                          // verbatim keys: data().misc.EnemyMaxResist
    pub characterConstants: generated::misc::CharacterConstants,
    pub monsterConstants: generated::misc::MonsterConstants, pub gameConstants: generated::misc::GameConstants,
    pub monsterLifeTable: Vec<f64>, /* … every monster*Table, ally tables … */
    pub weaponTypeInfo: ahash::AHashMap<Name, WeaponTypeInfo>, pub unarmedWeaponData: Vec<ValueTable>,
    pub nonDamagingAilment: indexmap::IndexMap<Name, AilmentData>,   // AilmentData.default: Option<f64> (nil for Freeze;
                                                                     // ModParser.lua:2942-2947, 3736, 3769-3793; CCR-0006 §3)
    pub ailmentTypeList: Vec<Name>, pub elementalAilmentTypeList: Vec<Name>,
    pub jewelRadii: ahash::AHashMap<Name, Vec<JewelRadius>>,  // "3_15", "3_16"
    pub clusterJewels: ClusterJewelData, pub keystones: Vec<Name>,
    pub uniques: UniqueTexts, pub rares: Vec<AStr>, pub itemMods: AffixTables, pub enchantments: Enchantments,
    pub pantheons: Pantheons, pub bosses: Bosses, pub bossSkills: BossSkills, pub costs: Costs,
    pub configOptions: Vec<ConfigOptionDef>, pub mapMods: ModMapDefs, pub calcSections: CalcSections,
    pub statDescriptions: StatDescriptionsLazy, pub pearlSupports: ValueTable, pub treeVersions: Vec<TreeVersionMeta>,
    pub modCache: ModCacheData,                               // Data/ModCache.lua (parse tier)
    pub timeless: TimelessMeta,                               // timelessJewel*/legionPassives kept as DataValue; LUT owns those reads
    // Calc-read roots typed by CCR-0011 §7 (schema/item_data.rs), each with its reader: enchantments (Item.lua:1043-1048),
    // foulbornMap (1028, 1553), rareLikeUniques (1036-1038, 1493-1495, 2059, 2719-2721), itemTagSpecial and
    // itemTagSpecialExclusionPattern (164-324), casterTagCrucibleUniques / minionTagCrucibleUniques (2713/2716),
    // abyssNotableNames (PassiveTree.lua:728), modScalability (ItemTools.lua:146-191), bossStats (ConfigOptions.lua:2185-2220),
    // gemVaalGemIdForBaseGemId (CalcTools.lua:287), gemsByGameId (SkillsTab.lua:466), powerStatList (CalcsTab.lua:743-744;
    // GetFromOutput and the transform closures as LuaFnRef, owner PWR), nodeIDList (DataLegionLookUpTableHelper.lua, LUT),
    // monsterExperienceLevelMap (Build.lua:236).
    pub other: indexmap::IndexMap<AStr, DataValue>,           // data::DYNAMIC_ROOTS (20): read only by UI/trade code outside
                                                              // scope.toml (contracts.md#schema names each reader)
}
impl Costs { pub fn by_resource(&self, r: Name) -> Option<&CostInfo>; }   // Data.lua:722-726 __index
static DATA: std::sync::OnceLock<Data> = std::sync::OnceLock::new();
pub fn data() -> &'static Data { DATA.get().expect("pob_gamedata::init not called") }
/// `data.bin` is postcard of the dump tree (roots + ref targets); `Data` is deserialised from it at load
/// (≈0.27 s release). Typed `Data` is not serialised. Every root is in TYPED_ROOTS, a stat-description scope, or
/// DYNAMIC_ROOTS; anything else fails the compile.
pub fn init(bytes: &[u8]) -> Result<(), LoadError>;          // desktop reads the file, wasm fetches it
pub fn tree_bytes(version: &str) -> Option<std::borrow::Cow<'static, [u8]>>;   // lazy per tree version

/// The only mutable parts of PoB's `data` (Data.lua:583-600, TreeTab.lua:553). Thread-local, like the Lua VM.
pub struct DataSel { pub jewelRadius: Vec<JewelRadius>, pub maxJewelRadius: f64 }
pub fn setJewelRadiiGlobally(treeVersion: &str);              // [PT]
pub fn jewelRadius() -> std::rc::Rc<DataSel>;                 // read as data.jewelRadius / data.maxJewelRadius

/// Data.lua:1033-1046 statMap __index fallback + processMod (52-93). Memoised per (grantedEffect id, stat).
/// The fallback copies the global entry and calls processMod on each top-level element (`for _, mod in ipairs(map)`).
/// A group element is passed to processMod as if it were a mod, exactly as Lua does; CT ports this literally. [CT]
pub fn statMapLookup(ge: &GrantedEffect, stat: Name) -> Option<Arc<StatMapEntry>>;
```

---
## 5. `pob-calc` core types (all [U0], frozen)

### 5.1 `lua_table.rs`: dynamic Lua values

```rust
use pob_lua::{Name, CheapClone}; use pob_gamedata::{Value, ModRef};
use std::{rc::Rc, cell::RefCell, any::Any};

pub type LStr = Rc<str>;                          // non-key strings (breakdown lines) are not interned

#[derive(Clone, Default)]
pub enum Val {
    #[default] Nil,
    Bool(bool),
    Num(f64),
    Str(LStr),
    Table(Table),                                 // mutable calc table
    Data(Value),                                  // immutable value from data or mods (writes raise LuaError)
    Mod(ModRef),
    Obj(Rc<dyn Any>),                             // calc struct stored in a dynamic table (ActiveSkillRef in SkillDPS rows, …)
}
impl Val {
    #[inline] pub fn truthy(&self) -> bool { !matches!(self, Val::Nil | Val::Bool(false)) }
    pub fn num(&self) -> f64;                     // arithmetic operand: Num, or a numeric Str (Lua coercion), else lua_error!
    pub fn opt_num(&self) -> Option<f64>;         // Some only for Num
    pub fn num_or(&self, d: f64) -> f64;          // `x or d`: Nil/false → d, Num → n, anything else → lua_error!
    pub fn str(&self) -> Option<LStr>;  pub fn table(&self) -> Option<Table>;
    pub fn obj<T: 'static>(&self) -> Option<Rc<T>>;
    pub fn get(&self, k: impl Into<Key>) -> Val;  // indexing Val (Table/Data); lua_error! on nil/number
}
impl From<f64> for Val {} impl From<bool> for Val {} impl From<&str> for Val {} impl From<Table> for Val {}
impl From<Option<f64>> for Val {}                 // None → Nil
pub fn lor(a: Val, b: impl FnOnce() -> Val) -> Val { if a.truthy() { a } else { b() } }
pub fn land(a: Val, b: impl FnOnce() -> Val) -> Val { if a.truthy() { b() } else { a } }

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Key { S(Name), I(i64), B(bool) }         // From<Name>, From<i64>, From<usize>, From<f64> (integral → I, else lua_error!)

/// A mutable Lua table. Clone = new alias (Rc).
#[derive(Clone, Default)]
pub struct Table(Rc<RefCell<TableData>>);
pub struct TableData {
    arr: Vec<Val>,                                // keys 1..=len, no trailing nil
    hash: indexmap::IndexMap<Key, Val, ahash::RandomState>,  // insertion order; nil assignment leaves a tombstone (Lua dead key)
    meta: Option<IndexMeta>,
}
/// setmetatable(t, { __index = … }). U0 audited every in-scope site (§8).
#[derive(Clone)]
pub enum IndexMeta {
    Table(Table),                                 // { __index = parent }: CalcActiveSkill.lua:482/487
    OrChain(smallvec::SmallVec<[Table; 2]>),      // __index = function(_,k) return a[k] or b[k] end: CalcOffence.lua:4422/4695/5000
}
impl Table {
    pub fn new() -> Self; pub fn with_meta(meta: IndexMeta) -> Self;
    // reads (each is one short borrow)
    pub fn get(&self, k: impl Into<Key>) -> Val;             // t[k] including __index
    pub fn rawget(&self, k: impl Into<Key>) -> Val;
    pub fn truthy(&self, k: impl Into<Key>) -> bool;         // if t[k] then
    pub fn num0(&self, k: impl Into<Key>) -> f64;            // t[k] or 0
    pub fn num_or(&self, k: impl Into<Key>, d: f64) -> f64;  // t[k] or d
    pub fn num(&self, k: impl Into<Key>) -> f64;             // t[k] in arithmetic (LuaError on nil)
    pub fn opt_num(&self, k: impl Into<Key>) -> Option<f64>;
    pub fn str(&self, k: impl Into<Key>) -> Option<LStr>;
    pub fn table(&self, k: impl Into<Key>) -> Option<Table>;
    pub fn obj<T: 'static>(&self, k: impl Into<Key>) -> Option<Rc<T>>;
    // writes
    pub fn set(&self, k: impl Into<Key>, v: impl Into<Val>); // Nil deletes (tombstone)
    pub fn get_or_new_table(&self, k: impl Into<Key>) -> Table; // t[k] = t[k] or {}
    pub fn add(&self, k: impl Into<Key>, d: f64);            // t[k] = (t[k] or 0) + d
    // array part
    pub fn len(&self) -> usize;                              // #t
    pub fn push(&self, v: impl Into<Val>);                   // t_insert(t, v)
    pub fn insert(&self, pos: i64, v: impl Into<Val>);      // LuaJIT lib_table.c: no bounds error (pos > #t+1 only
                                                             // sets t[pos]; pos <= 0 shifts through the hash part)
    pub fn remove(&self, pos: Option<i64>) -> Val;          // nil (nothing) unless 1 <= pos <= #t
    /// pairs(): key snapshot (array then hash, insertion order); each value read live; skip keys that are nil when reached.
    pub fn pairs(&self) -> TablePairs;
    pub fn wipe(&self);                                      // wipeTable (keeps the metatable)
    pub fn copyTable(&self, noRecurse: bool) -> Table;       // Common.lua:477-487 (no metatable on the copy; CCR-0011 §1)
    pub fn ptr_eq(&self, o: &Table) -> bool;
    pub fn meta(&self) -> Option<IndexMeta>;
}
pub struct TablePairs { /* Table + Vec<Key> snapshot + cursor */ }
impl Iterator for TablePairs { type Item = (Key, Val); fn next(&mut self) -> Option<(Key, Val)>; }
impl CheapClone for Table {}
#[macro_export] macro_rules! tbl { /* tbl!{ "a", "b"; key = v, ["k"] = v } → Table */ }

pub type Output = Table;   // actor.output, output.MainHand, minion output: same type and keys
```

### 5.2 `cfg.rs`: ModCfg (`skillCfg`, `weapon1Cfg`, ad-hoc `cfg` tables)

```rust
pub type Cfg = Rc<ModCfg>;
/// activeSkill.skillTypes is shared by identity with cfg.skillTypes (CalcActiveSkill.lua ~467).
pub type SkillTypes = Shared<SkillTypeSet>;

/// Every field mutates in place because Lua aliases cfg tables (copyTable(cfg, true) is shallow).
#[derive(Default)]
pub struct ModCfg {
    pub flags: Cell<u32>, pub keywordFlags: Cell<u32>,
    pub source: Cell<Option<Name>>, pub actor: Cell<Option<Name>>,
    pub skillName: Cell<Option<Name>>, pub summonSkillName: Cell<Option<Name>>,
    pub skillGem: Field<Option<GemDataRef>>, pub skillGrantedEffect: Field<Option<GrantedEffectRef>>,
    pub skillPart: Cell<Option<f64>>,
    pub skillTypes: Field<Option<SkillTypes>>,
    pub skillCond: Field<Option<Table>>,          // layered via IndexMeta
    pub skillDist: Cell<Option<f64>>,
    pub skillStats: Field<Option<Output>>,        // weapon1Cfg.skillStats = output.MainHand (CalcOffence.lua:2008/2025)
    pub slotName: Cell<Option<Name>>, pub socketColor: Cell<Option<Name>>, pub socketNum: Cell<Option<f64>>,
    pub strengthGems: Cell<Option<f64>>, pub dexterityGems: Cell<Option<f64>>, pub intelligenceGems: Cell<Option<f64>>,
    pub item: Field<Option<ItemRef>>,
    pub conversionTable: Field<Option<Table>>,    // CalcOffence.lua:77/145 read cfg.conversionTable
    pub baseFlags: Field<Option<NameSet>>,        // ModStore.lua:882 reads cfg.baseFlags (BaseFlag tag)
    pub extra: Table,                             // any other key a cfg literal sets (U0 census)
}
impl ModCfg {
    pub fn copyTable(&self, noRecurse: bool) -> Cfg;  // shallow shares skillCond/skillTypes/skillStats handles
    /// The `"MORE"` string used as a cfg (CalcOffence.lua:2194-2195): every field nil. A function, not a
    /// `static`: ModCfg holds Cell/RefCell/Rc and is not Sync (one leaked all-nil cfg per thread; CCR-0011 §5).
    pub fn STRING_CFG() -> &'static ModCfg;
}
```

### 5.3 `classes/mod.rs`: ModStore, ModDB, ModList

Today's `mod_store.rs` bodies are the starting point for MS-A/MS-B. The arena (`DbId`/`ActorId`/`EvalEnv`) is replaced by `Rc`. Two known divergences go away with it: there are no `globalLimits` on ModList (ModList.lua passes none), and Replace/Convert now recurse into the parent.

```rust
pub type ModStoreRef = Rc<ModStore>;
pub type ModDB = ModStoreRef;  pub type ModList = ModStoreRef;   // names mirror PoB classes

pub struct ModStore {
    pub kind: StoreKind,
    mods: RefCell<StoreMods>,                     // private; only the *Internal fns touch it
    pub parent: Field<Option<ModStoreRef>>,       // self.parent (false → None); reassigned at CalcSetup.lua:689-692
    pub actor: Field<ActorWeak>,                  // ModStore.lua:42: parent.actor at construction time, else {} (= dangling Weak)
    pub conditions: Table, pub multipliers: Table,
}
#[derive(Clone, Copy, PartialEq, Eq)] pub enum StoreKind { ModDB, ModList }
enum StoreMods {
    DB(ahash::AHashMap<Name, Vec<ModRef>>),
    /// ModList: flat list in insertion order, plus a name → positions index. The index is order-safe because
    /// SumInternal and friends iterate names outside and mods inside (ModList.lua:101-121).
    List { list: Vec<ModRef>, index: ahash::AHashMap<Name, smallvec::SmallVec<[u32; 2]>> },
}
pub struct TabRow { pub value: Value, pub r#mod: ModRef }
pub type GlobalLimits = ahash::AHashMap<Name, f64>;   // allocated lazily, ModDB levels only

impl ModStore {
    pub fn ModDB(parent: Option<ModStoreRef>) -> ModStoreRef;     // new("ModDB"):ModDB(parent)
    pub fn ModList(parent: Option<ModStoreRef>) -> ModStoreRef;   // new("ModList"):ModList(parent)
    // ModStore.lua 56-164 [MS-A]
    pub fn ScaleAddMod(&self, m: &ModRef, scale: f64, replace: bool);
    pub fn CopyList(&self, modList: &[ModRef]);
    pub fn ScaleAddList(&self, modList: &[ModRef], scale: f64, replace: bool);
    pub fn NewMod(&self, name: Name, ty: ModType, value: impl Into<Value>, source: &str, flags: u32, keywordFlags: u32, tags: &[Tag]);
    pub fn ReplaceMod(&self, name: Name, ty: ModType, value: impl Into<Value>, source: &str, flags: u32, keywordFlags: u32, tags: &[Tag]);
    pub fn ConvertMod(&self, oldName: Name, name: Name, ty: ModType, value: impl Into<Value>, source: &str, flags: u32, keywordFlags: u32, tags: &[Tag]);
    pub fn Combine(&self, modType: ModType, cfg: Option<&ModCfg>, names: &[Name]) -> Val;
    // ModStore.lua 165-362 public queries [MS-A]
    pub fn Sum(&self, modType: ModType, cfg: Option<&ModCfg>, names: &[Name]) -> f64;
    pub fn More(&self, cfg: Option<&ModCfg>, names: &[Name]) -> f64;
    pub fn Flag(&self, cfg: Option<&ModCfg>, names: &[Name]) -> bool;
    pub fn Override(&self, cfg: Option<&ModCfg>, names: &[Name]) -> Value;      // Value::Nil = nil
    pub fn List(&self, cfg: Option<&ModCfg>, names: &[Name]) -> Vec<Value>;
    pub fn Tabulate(&self, modType: Option<ModType>, cfg: Option<&ModCfg>, names: &[Name]) -> Vec<TabRow>;
    pub fn Max(&self, cfg: Option<&ModCfg>, names: &[Name]) -> Option<f64>;     // nil unless some value > 0
    pub fn Min(&self, cfg: Option<&ModCfg>, names: &[Name]) -> Option<f64>;
    /// ModDB only. On a ModList, Lua errors (ModList has no HasModInternal), so this raises LuaError.
    pub fn HasMod(&self, modType: ModType, cfg: Option<&ModCfg>, names: &[Name]) -> bool;
    pub fn GetCondition(&self, var: Name, cfg: Option<&ModCfg>, noMod: bool) -> bool;
    pub fn GetMultiplier(&self, var: Name, cfg: Option<&ModCfg>, noMod: bool) -> f64;
    pub fn GetStat(&self, stat: Name, cfg: Option<&ModCfg>) -> f64;
    // ModDB.lua / ModList.lua [MS-A]
    pub fn AddMod(&self, m: ModRef);
    pub fn AddList(&self, modList: &[ModRef]);
    pub fn AddDB(&self, modDB: &ModStore);
    pub fn MergeMod(&self, m: ModRef);            // ModList only (ModList.lua:73)
    pub fn MergeNewMod(&self, m: Mod);            // ModList only (ModList.lua:96)
    pub fn len(&self) -> usize;                   // #modList (ModList)
    pub fn ipairs(&self) -> Vec<ModRef>;          // `for _, mod in ipairs(modList)` (ModList); Arc clones
    pub fn wipe(&self);                           // wipeTable(modDB.mods) etc.
    // Rust-only (oracle, injection, UI)
    pub fn mods_named(&self, name: Name) -> Vec<ModRef>;  pub fn all_mods(&self) -> Vec<ModRef>;
}
// classes/eval_mod.rs [MS-B]: ModStore.lua 363-971
impl ModStore {
    /// `context` is the store the query started on (passed down the parent chain exactly as Lua does).
    pub fn EvalMod(&self, m: &Mod, cfg: Option<&ModCfg>, globalLimits: Option<&mut GlobalLimits>) -> Value;
    pub(crate) fn getActor(&self, actorType: Name) -> Option<ActorRef>;   // ModStore.lua:48-54
}
```

EvalMod MUST NOT mutate a tag. Lua's `tag.div = …` write is computed into a local, and MS-B's test proves that no later reader observes the Lua mutation. EvalMod returns a *new* `Value` wherever Lua does `copyTable(value)` followed by `value.mod.value = …`.

### 5.4 `actor.rs`, `active_skill.rs`, `env.rs`

```rust
pub type ActorRef = Rc<Actor>;  pub type ActorWeak = std::rc::Weak<Actor>;

pub struct Actor {
    pub modDB: Field<ModDB>,
    pub level: Cell<f64>,
    pub output: Field<Output>,                    // perform reassigns env.player.output = {} (CalcPerform.lua:1305)
    pub breakdown: Field<Option<Breakdown>>,
    pub itemList: Shared<indexmap::IndexMap<Name, ItemRef>>,   // slot name → item
    pub weaponData1: Field<Table>, pub weaponData2: Field<Table>,
    pub weaponRange1: Cell<Option<f64>>, pub weaponRange2: Cell<Option<f64>>,
    pub mainSkill: Field<Option<ActiveSkillRef>>,
    pub activeSkillList: Field<Shared<Vec<ActiveSkillRef>>>,
    pub enemy: Field<ActorWeak>, pub parent: Field<ActorWeak>, pub player: Field<ActorWeak>,
    pub minion: Field<Option<ActorRef>>, pub partyMembers: Field<ActorWeak>,
    pub minionData: Field<Option<MinionDataRef>>,
    pub damageShiftTable: Field<Option<Table>>, pub damageOverTimeShiftTable: Field<Option<Table>>,
    pub allyLifeList: Field<Option<Table>>, pub strDmgBonus: Cell<f64>, pub hostile: Cell<bool>,
    pub appliedEnemyModifiers: Field<Option<Table>>, pub infoTrigger: Field<Option<LStr>>,
    pub itemSetId: Cell<Option<f64>>, pub uses: Cell<Option<f64>>,
    pub extra: Table,                             // computed-key fields: actor["reserved_"..name.."Base"], "<X>Efficiency", …
}
impl Actor { pub fn new() -> ActorRef; }

pub type SkillEffectRef = Rc<SkillEffect>;
pub struct SkillEffect {                          // activeEffect / supportEffect / skillEffect tables
    pub grantedEffect: Field<GrantedEffectRef>,
    pub level: Cell<f64>, pub quality: Cell<f64>,
    pub srcInstance: Field<Option<GemInstanceRef>>,   // the build model's gem table (identity used by cacheSkillUUID)
    pub gemData: Field<Option<GemDataRef>>, pub gemCfg: Field<Option<Cfg>>,
    pub grantedEffectLevel: Field<Option<SkillLevelRef>>,
    pub isSupporting: Field<Option<Shared<PtrSet<GemInstance>>>>,
    pub superseded: Cell<bool>, pub appliesToGrantedSkills: Cell<bool>, pub fromItem: Cell<bool>,
    pub actorLevel: Cell<Option<f64>>, pub globalQuality: Cell<Option<f64>>, pub itemQuality: Cell<Option<f64>>,
    pub supportQuality: Cell<Option<f64>>, pub req: Field<Option<Table>>, pub gemPropertyInfo: Field<Option<Table>>,
    pub extra: Table,                             // applyGemMods: effect[value.key] = effect[value.key] + value.value (CalcSetup.lua 382-418)
}

pub type ActiveSkillRef = Rc<ActiveSkill>;
pub struct ActiveSkill {
    pub activeEffect: Field<SkillEffectRef>,
    pub supportList: Field<Shared<Vec<SkillEffectRef>>>, pub effectList: Field<Shared<Vec<SkillEffectRef>>>,
    pub actor: Field<ActorWeak>,                  // may point into another env (copyActiveSkill, D2)
    pub summonSkill: Field<Option<ActiveSkillRef>>, pub socketGroup: Field<Option<SocketGroupRef>>,
    pub skillTypes: Field<SkillTypes>, pub minionSkillTypes: Field<Option<SkillTypes>>,
    pub skillFlags: Field<Table>, pub skillData: Field<Table>,
    pub skillModList: Field<ModList>, pub baseSkillModList: Field<ModList>,
    pub extraSkillModList: Field<Shared<Vec<ModRef>>>,
    pub skillCfg: Field<Cfg>, pub weapon1Cfg: Field<Option<Cfg>>, pub weapon2Cfg: Field<Option<Cfg>>,
    pub weapon1Flags: Cell<Option<u32>>, pub weapon2Flags: Cell<Option<u32>>,
    pub bleedCfg: Field<Option<Cfg>>, pub poisonCfg: Field<Option<Cfg>>, pub igniteCfg: Field<Option<Cfg>>,
    pub OHbleedCfg: Field<Option<Cfg>>, pub OHpoisonCfg: Field<Option<Cfg>>, pub OHigniteCfg: Field<Option<Cfg>>,
    pub dotCfg: Field<Option<Cfg>>, pub decayCfg: Field<Option<Cfg>>,
    pub buffList: Field<Shared<Vec<Table>>>,      // buff tables (type, name, modList, cond, …) stay dynamic
    pub minion: Field<Option<ActorRef>>, pub minionList: Field<Shared<Vec<Name>>>,
    pub mirage: Field<Option<Table>>,
    pub skillPart: Cell<Option<f64>>, pub skillPartName: Field<Option<LStr>>,
    pub disableReason: Field<Option<LStr>>, pub infoMessage: Field<Option<LStr>>, pub infoMessage2: Field<Option<LStr>>,
    pub infoTrigger: Field<Option<LStr>>, pub conversionTable: Field<Option<Table>>,
    pub triggeredBy: Field<Option<Table>>, pub slotName: Cell<Option<Name>>, pub skillTotemId: Cell<Option<f64>>,
    pub buffSkill: Cell<bool>, pub debuffSkill: Cell<bool>, pub totemBuffSkill: Cell<bool>, pub minionBuffSkill: Cell<bool>,
    pub activeMineCount: Cell<Option<f64>>, pub activeStageCount: Cell<Option<f64>>,
    pub extra: Table,
}
impl ActiveSkill {
    /// activeSkill[pass.label ~= "Off Hand" and "igniteCfg" or "OHigniteCfg"]: computed field names over a fixed set.
    pub fn cfg_field(&self, name: Name) -> &Field<Option<Cfg>>;   // lua_error! for unknown names
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)] pub enum Mode { MAIN, CALCS, CALCULATOR }

pub type EnvRef = Rc<Env>;
pub struct Env {
    pub build: BuildRef, pub data: &'static Data,
    pub mode: Mode, pub mode_buffs: Cell<bool>, pub mode_combat: Cell<bool>, pub mode_effective: Cell<bool>,
    pub buildBreakdown: Cell<bool>,
    pub configInput: Table, pub configPlaceholder: Table, pub calcsInput: Table,
    pub spec: Field<PassiveSpecRef>, pub r#override: Rc<Override>, pub classId: Cell<f64>,
    pub modDB: Field<ModDB>, pub enemyDB: Field<ModDB>, pub itemModDB: Field<ModDB>,
    pub initialNodeModDB: Field<Option<ModDB>>, pub aegisModList: Field<Option<ModList>>,
    pub weaponModList1: Field<Option<ModList>>, pub theIronMass: Field<Option<ModList>>,
    pub enemyLevel: Cell<f64>,
    pub player: ActorRef, pub enemy: ActorRef, pub minion: Field<Option<ActorRef>>, pub partyMembers: Field<Option<ActorRef>>,
    pub allocNodes: Field<Shared<LjIntMap<NodeRef>>>, pub grantedPassives: Field<Shared<LjIntMap<bool>>>,
    pub grantedSkills: Field<Shared<Vec<Table>>>, pub grantedSkillsNodes: Field<Shared<Vec<Table>>>,
    pub grantedSkillsItems: Field<Shared<Vec<Table>>>,
    pub radiusJewelList: Field<Shared<Vec<Table>>>, pub extraRadiusNodeList: Field<Shared<LjIntMap<NodeRef>>>,
    pub flasks: Field<Shared<PtrMap<Item, bool>>>, pub tinctures: Field<Shared<PtrMap<Item, bool>>>,
    pub flaskSlotOccupied: Field<Table>, pub flaskSlotMap: Field<Table>,
    pub itemWarnings: Field<Table>, pub requirementsTable: Field<Table>,
    pub requirementsTableItems: Field<Table>, pub requirementsTableGems: Field<Table>,
    pub auxSkillList: Field<Shared<Vec<ActiveSkillRef>>>, pub limitedSkills: Field<Option<Table>>,
    pub mainSocketGroup: Cell<Option<f64>>, pub crossLinkedSupportGroups: Field<Table>,
    pub keystonesAdded: Field<Table>, pub explodeSources: Field<Shared<Vec<Table>>>,
    pub aegisItem: Field<Option<ItemRef>>,
    pub buffs: Field<Table>, pub minionBuffs: Field<Table>, pub debuffs: Field<Table>, pub curseSlots: Field<Table>,
    // MAIN/CALCS used-tracking (Calcs.lua 437-872)
    pub skillsUsed: Field<Option<Table>>, pub conditionsUsed: Field<Option<Table>>, pub enemyConditionsUsed: Field<Option<Table>>,
    pub minionConditionsUsed: Field<Option<Table>>, pub multipliersUsed: Field<Option<Table>>,
    pub enemyMultipliersUsed: Field<Option<Table>>, pub perStatsUsed: Field<Option<Table>>,
    pub enemyPerStatsUsed: Field<Option<Table>>, pub tagTypesUsed: Field<Option<Table>>, pub modsUsed: Field<Option<Table>>,
    pub extra: Table,
}

/// The PoB `override` table. Mutable and shared by identity: initEnv sets extraJewelFuncs (CalcSetup.lua:1424-1430),
/// appends conditions (1921-1922), restarts with the same object, and buildActiveSkill/copyActiveSkill reuse env.override.
#[derive(Default)]
pub struct Override {
    pub spec: Field<Option<PassiveSpecRef>>,
    pub addNodes: Field<Option<Shared<LjIntMap<NodeRef>>>>, pub removeNodes: Field<Option<Shared<LjIntMap<NodeRef>>>>,
    pub repSlotName: Field<Option<Name>>, pub repItem: Field<Option<ItemRef>>,
    pub toggleFlask: Field<Option<ItemRef>>, pub toggleTincture: Field<Option<ItemRef>>,
    pub conditions: Field<Option<Shared<Vec<Name>>>>,
    pub extraJewelFuncs: Field<Option<ModList>>,
    pub extra: Table,
}
/// specEnv argument of initEnv (Calcs.lua:93/192, CalcSetup.lua:478-482).
#[derive(Default, Clone)]
pub struct SpecEnv { pub cachedPlayerDB: Option<ModDB>, pub cachedEnemyDB: Option<ModDB>,
                     pub cachedMinionDB: Option<ModDB>, pub env: Option<EnvRef>, pub accelerate: Accelerate }
#[derive(Default, Clone, Copy)]
pub struct Accelerate { pub nodeAlloc: bool, pub requirementsItems: bool, pub requirementsGems: bool, pub skills: bool, pub everything: bool }
```

### 5.5 `breakdown.rs`

```rust
/// One `require(calcs.breakdownModule)(modDB, output, actor)` instance: the data table plus the closure context.
#[derive(Clone)]
pub struct Breakdown { pub t: Table, pub modDB: ModStoreRef, pub output: Output, pub actor: ActorWeak }
impl std::ops::Deref for Breakdown { type Target = Table; fn deref(&self) -> &Table { &self.t } }
impl CheapClone for Breakdown {}
pub fn newBreakdown(modDB: &ModStoreRef, output: &Output, actor: &ActorRef) -> Breakdown;
// modules/calc_breakdown.rs [CT] — CalcBreakdown.lua, verbatim names
impl Breakdown {
    pub fn multiChain(&self, out: &Table, chain: &Table) -> f64;
    pub fn simple(&self, extraBase: Option<f64>, cfg: Option<&ModCfg>, total: f64, names: &[Name]) -> Option<Table>;
    pub fn r#mod(&self, modList: &ModStore, cfg: Option<&ModCfg>, names: &[Name]) -> Option<Table>;
    pub fn slot(&self, source: &str, sourceName: Option<&str>, cfg: Option<&ModCfg>, base: f64, total: Option<f64>, names: &[Name]);
    pub fn area(&self, base: f64, areaMod: f64, total: f64, incBreakpoint: f64, moreBreakpoint: f64,
                redBreakpoint: f64, lessBreakpoint: f64, label: &str, radiusMod: Option<f64>) -> Table;
    pub fn effMult(&self, damageType: Name, resist: f64, pen: f64, taken: f64, mult: f64, takenMore: f64,
                   sourceRes: Option<Name>, useRes: bool, invertChance: f64, effectiveResist: f64) -> Table;
    pub fn dot(&self, out: &Table, baseVal: f64, inc: f64, more: f64, mult: Option<f64>, rate: Option<f64>,
               aura: Option<f64>, effMult: Option<f64>, total: f64);
    pub fn critDot(&self, dotMulti: f64, critMulti: f64, dotChance: f64, critChance: f64) -> Table;
    pub fn leech(&self, instant: f64, instantRate: f64, instances: f64, pool: f64, rate: f64, max: f64,
                 dur: f64, instantLeechProportion: f64, hitRate: f64) -> Table;
}
```

The parameter lists above are U0's reading of CalcBreakdown.lua. CT MUST correct any that differ from the Lua, and record the change in the same PR as a CCR.

### 5.6 `global_cache.rs`, `module_state.rs`

```rust
/// Data/Global.lua:356 `GlobalCache`; filled by cacheData (Common.lua:924-942), wiped by wipeGlobalCache (944-948).
/// Entries hold LIVE handles (D11): .Env aliases an env that may later be re-performed (calcFullDPS).
pub struct CachedData {
    pub Name: Name, pub Speed: Val, pub HitSpeed: Val, pub ManaCost: Val, pub LifeCost: Val, pub ESCost: Val,
    pub RageCost: Val, pub HitChance: Val, pub AccuracyHitChance: Val, pub PreEffectiveCritChance: Val,
    pub CritChance: Val, pub TotalDPS: Val, pub ActiveSkill: ActiveSkillRef, pub Env: EnvRef,
}
#[derive(Default)]
pub struct GlobalCacheT { pub cachedData: [indexmap::IndexMap<Name, Rc<CachedData>>; 3] }   // index = Mode as usize
thread_local! { pub static GlobalCache: RefCell<GlobalCacheT> = RefCell::default(); }
pub fn cachedData(mode: Mode, uuid: Name) -> Option<Rc<CachedData>>;   // read helper; no borrow escapes
// modules/common.rs [CT] (Common.lua 897-950)
pub fn cacheSkillUUID(skill: &ActiveSkillRef, env: &EnvRef) -> Name;
pub fn cacheData(uuid: Name, env: &EnvRef);
pub fn wipeGlobalCache();

/// Module-level mutable upvalues that persist across calls (D17). Thread-local like the Lua VM.
pub struct OffenceModState {
    pub globalOutput: Field<Option<Output>>,          // CalcOffence.lua:65, assigned at 2176/2607/4165, read at 2137 (combineStat)
    pub globalBreakdown: Field<Option<Breakdown>>,    // CalcOffence.lua:67
}
thread_local! { pub static OFFENCE_STATE: OffenceModState = OffenceModState::default(); }
// CalcOffence.lua:27-29, CalcSetup.lua:16 and CalcDefence.lua:22 tempTableN are scratch tables that do not escape
// (verified: buildConversionTable 1937-1983, CalcSetup 1465, CalcDefence 862), so they become locals.
// CalcActiveSkill.lua:21 mergeLevelCache becomes a thread-local memo keyed by (Arc ptr of the data mod, value bits).
```

### 5.7 `flow.rs`, `stub.rs`

```rust
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flow { Normal, Return, Restart }        // Restart: initEnv only (CalcSetup.lua:1433, 1923)
#[macro_export] macro_rules! seg { ($e:expr) => { match $e { Flow::Normal => {}, Flow::Return => return, Flow::Restart => unreachable!("Restart outside initEnv") } } }

/// Unported body: logs once per site, counts the hit (stub_report()), returns Default (D9).
#[macro_export] macro_rules! unported { ($site:literal) => {{ $crate::stub::hit($site); Default::default() }} }
pub fn hit(site: &'static str); pub fn stub_report() -> Vec<(&'static str, u64)>;
```

### 5.8 Build model (`model/mod.rs`): calc halves of `Build.lua` and the tabs

Field paths are PoB's (`build.itemsTab.items`, `build.skillsTab.socketGroupList`, `build.configTab.input`). The UI's tab controls wrap these models. Calc code mutates the model through `Field`/`Shared` exactly where Lua does (D13).

```rust
pub type BuildRef = Rc<Build>;
pub struct Build {
    pub characterLevel: Cell<f64>, pub targetVersion: Field<Name>, pub viewMode: Field<Name>,
    pub mainSocketGroup: Cell<f64>, pub spectreList: Field<Shared<Vec<Name>>>, pub bandit: Field<Name>,
    pub pantheonMajorGod: Field<Name>, pub pantheonMinorGod: Field<Name>,
    pub spec: Field<PassiveSpecRef>, pub treeTab: TreeTab,
    pub itemsTab: ItemsTab, pub skillsTab: SkillsTab, pub configTab: ConfigTab,
    pub calcsTab: CalcsTab, pub partyTab: PartyTab,
    pub data: &'static Data, pub outputRevision: Cell<f64>,
}
pub struct ItemsTab { pub items: Shared<indexmap::IndexMap<u32, ItemRef>>, pub slots: Shared<indexmap::IndexMap<Name, Table>>,
    pub orderList: Shared<Vec<u32>>, pub itemSets: Shared<indexmap::IndexMap<u32, Table>>, pub activeItemSetId: Cell<f64>,
    pub sockets: Shared<LjIntMap<Table>> }
pub struct SkillsTab { pub socketGroupList: Field<Shared<Vec<SocketGroupRef>>>, pub skillSets: Shared<indexmap::IndexMap<u32, Table>>,
    pub activeSkillSetId: Cell<f64>, pub defaultGemLevel: Field<Option<Name>>, pub defaultGemQuality: Cell<Option<f64>>,
    pub displayGroup: Field<Option<SocketGroupRef>> }
pub type SocketGroupRef = Rc<SocketGroup>;
pub struct SocketGroup { pub label: Field<LStr>, pub enabled: Cell<bool>, pub slotEnabled: Cell<bool>,
    pub slot: Field<Option<Name>>, pub source: Field<Option<LStr>>, pub includeInFullDPS: Cell<bool>,
    pub groupCount: Cell<Option<f64>>, pub gemList: Field<Shared<Vec<GemInstanceRef>>>,
    pub mainActiveSkill: Cell<f64>, pub mainActiveSkillCalcs: Cell<f64>, pub noSupports: Cell<bool>,
    pub displaySkillList: Field<Option<Shared<Vec<ActiveSkillRef>>>>, pub displaySkillListCalcs: Field<Option<Shared<Vec<ActiveSkillRef>>>>,
    pub displayLabel: Field<Option<LStr>>, pub explodeSources: Field<Option<Table>>, pub extra: Table }
pub type GemInstanceRef = Rc<GemInstance>;
pub struct GemInstance { pub nameSpec: Field<LStr>, pub skillId: Field<Option<Name>>, pub gemId: Field<Option<Name>>,
    pub level: Cell<f64>, pub quality: Cell<f64>, pub enabled: Cell<bool>, pub enableGlobal1: Cell<bool>,
    pub enableGlobal2: Cell<bool>, pub count: Cell<Option<f64>>, pub gemData: Field<Option<GemDataRef>>,
    pub grantedEffect: Field<Option<GrantedEffectRef>>,
    pub skillPart: Cell<Option<f64>>, pub skillPartCalcs: Cell<Option<f64>>, pub skillStageCount: Cell<Option<f64>>,
    pub skillStageCountCalcs: Cell<Option<f64>>, pub skillMineCount: Cell<Option<f64>>, pub skillMineCountCalcs: Cell<Option<f64>>,
    pub skillMinion: Field<Option<Name>>, pub skillMinionCalcs: Field<Option<Name>>,
    pub skillMinionItemSet: Cell<Option<f64>>, pub skillMinionItemSetCalcs: Cell<Option<f64>>,
    pub skillMinionSkill: Cell<Option<f64>>, pub skillMinionSkillCalcs: Cell<Option<f64>>,
    pub fromItem: Cell<bool>, pub noSupports: Cell<bool>, pub triggered: Cell<bool>,
    pub displayEffect: Field<Option<SkillEffectRef>>, pub supportEffect: Field<Option<SkillEffectRef>>,
    pub reqLevel: Cell<Option<f64>>, pub extra: Table }
pub struct ConfigTab { pub input: Table, pub placeholder: Table, pub varControlsSelIndex: Table /* varControls[var].selIndex read by apply fns */, pub modList: Field<ModList>, pub enemyModList: Field<ModList>,
    pub enemyLevel: Cell<Option<f64>>, pub configSets: Shared<indexmap::IndexMap<u32, Table>>, pub activeConfigSetId: Cell<f64> }
pub struct CalcsTab { pub input: Table, pub mainEnv: Field<Option<EnvRef>>, pub calcsEnv: Field<Option<EnvRef>>,
    pub mainOutput: Field<Option<Output>>, pub calcsOutput: Field<Option<Output>>,
    pub miscCalculator: Field<Option<Rc<MiscCalculator>>>, pub powerBuildFlag: Cell<bool> }
pub struct PartyTab { pub actor: ActorRef, pub enableExportBuffs: Cell<bool>, pub enemyModList: Field<ModList>,
    pub buffExports: Field<Option<Table>>, pub extra: Table }
pub struct TreeTab { pub specList: Shared<Vec<PassiveSpecRef>>, pub activeSpec: Cell<f64> }
```

### 5.9 Item, PassiveTree, Node, PassiveSpec (struct fields [U0]; bodies in IT-*/PT/PS-*)

```rust
pub type ItemRef = Rc<Item>;
pub struct Item {
    pub id: Cell<Option<f64>>, pub raw: Field<LStr>, pub name: Field<LStr>, pub title: Field<Option<LStr>>,
    pub baseName: Field<Option<Name>>, pub base: Field<Option<ItemBaseRef>>, pub r#type: Field<Name>,
    pub rarity: Field<Name>, pub corrupted: Cell<bool>, pub mirrored: Cell<bool>, pub fractured: Cell<bool>,
    pub influences: Table /* shaper, elder, … */, pub quality: Cell<Option<f64>>, pub itemLevel: Cell<Option<f64>>,
    pub sockets: Field<Shared<Vec<Table>>>, pub abyssalSocketCount: Cell<f64>,
    pub variant: Cell<Option<f64>>, pub variantList: Field<Option<Shared<Vec<LStr>>>>,
    pub explicitModLines: Field<Shared<Vec<Table>>>, pub implicitModLines: Field<Shared<Vec<Table>>>,
    pub enchantModLines: Field<Shared<Vec<Table>>>, pub scourgeModLines: Field<Shared<Vec<Table>>>,
    pub crucibleModLines: Field<Shared<Vec<Table>>>, pub buffModLines: Field<Shared<Vec<Table>>>,
    pub modList: Field<Option<ModList>>, pub slotModList: [Field<Option<ModList>>; 2],
    pub weaponData: Field<Option<Table>>,             // Lua weaponData[1], [2]
    pub armourData: Field<Option<Table>>, pub flaskData: Field<Option<Table>>, pub tinctureData: Field<Option<Table>>,
    pub jewelData: Field<Option<Table>>,              // funcList, radiusIndex, conqueredBy, limit, limitDisabled, …
    pub jewelRadiusIndex: Cell<Option<f64>>, pub jewelRadiusData: Field<Option<Table>>,
    pub requirements: Table, pub grantedSkills: Field<Shared<Vec<Table>>>,
    pub classRestriction: Field<Option<Name>>, pub limit: Cell<Option<f64>>,
    pub modSource: Field<Option<LStr>>,
    pub extra: Table,                                 // every other ParseRaw field (U0 census of Item.lua `self.<x> =`)
}
pub type NodeRef = Rc<Node>;
/// Tree node (PassiveTree.lua). Spec nodes overlay tree nodes via `setmetatable({...}, treeNode)` (PassiveSpec.lua:56):
/// `SpecNode` holds the overlay fields and falls back to `tree` for anything unset.
pub struct Node {
    pub id: f64, pub dn: Field<LStr>, pub sd: Field<Shared<Vec<LStr>>>, pub r#type: Field<Name>,
    pub isKeystone: Cell<bool>, pub isNotable: Cell<bool>, pub isMastery: Cell<bool>, pub isJewelSocket: Cell<bool>,
    pub ascendancyName: Field<Option<Name>>, pub isMultipleChoiceOption: Cell<bool>,
    pub modList: Field<ModList>, pub modKey: Field<LStr>, pub mods: Field<Shared<Vec<Table>>>,
    pub finalModList: Field<Option<ModList>>,        // written by CalcSetup (278/287) and CalcsTab:BuildOutput (452-455)
    pub grantedSkills: Field<Option<Shared<Vec<Table>>>>,
    pub x: Cell<f64>, pub y: Cell<f64>, pub group: Field<Option<Table>>, pub o: Cell<f64>, pub oidx: Cell<f64>,
    pub linked: Field<Shared<Vec<NodeRef>>>, pub nodesInRadius: Field<Option<Table>>, pub attributesInRadius: Field<Option<Table>>,
    pub conqueredBy: Field<Option<Table>>, pub alloc: Cell<bool>, pub expansionJewel: Field<Option<Table>>,
    pub tree: Option<NodeRef>,                        // Some for spec overlay nodes (the __index parent)
    pub extra: Table,
}
pub type PassiveTreeRef = Rc<PassiveTree>;
pub struct PassiveTree { pub treeVersion: Name, pub nodes: LjIntMap<NodeRef>, pub classes: Table,
    pub keystoneMap: Table, pub notableMap: Table, pub ascendancyMap: Table, pub clusterNodeMap: Table,
    pub sockets: LjIntMap<NodeRef>, pub masteryEffects: Table, pub orbitRadii: Vec<f64>, pub skillsPerOrbit: Vec<f64>,
    pub extra: Table }
pub type PassiveSpecRef = Rc<PassiveSpec>;
pub struct PassiveSpec { pub build: std::rc::Weak<Build>, pub tree: Field<PassiveTreeRef>, pub treeVersion: Field<Name>,
    pub nodes: Shared<LjIntMap<NodeRef>>, pub allocNodes: Shared<LjIntMap<NodeRef>>,
    pub curClassId: Cell<f64>, pub curAscendClassId: Cell<f64>, pub curSecondaryAscendClassId: Cell<f64>,
    pub masterySelections: Shared<LjIntMap<f64>>, pub jewels: Shared<LjIntMap<f64>>,
    pub subGraphs: Table, pub hashOverrides: Shared<LjIntMap<NodeRef>>, pub allocExtendedNodes: Shared<Vec<f64>>,
    pub allocatedNotableCount: Cell<f64>, pub allocatedKeystoneCount: Cell<f64>, pub allocatedMasteryCount: Cell<f64>,
    pub allocatedMasteryTypes: Table, pub allocatedTattooTypes: Table, pub extra: Table }
```

The fields above are what the calc and the oracle read and write. U0 MUST complete each struct with a census of `self.<field> =` assignments in its class file. Long-tail fields go into `extra: Table` until a unit needs them typed (CCR).

### 5.10 `calcs_facade.rs`: the `calcs.*` namespace and UI entry points

```rust
pub mod calcs {
    pub use crate::modules::calcs::{getMiscCalculator, calcFullDPS, buildActiveSkill, buildOutput, MiscCalculator, FullDPS};
    pub use crate::modules::calc_setup::{initEnv, initModDB, buildModListForNode, buildModListForNodeList};
    pub use crate::modules::calc_active_skill::{createActiveSkill, copyActiveSkill, buildActiveSkillModList,
                                                createMinionSkills, mergeSkillInstanceMods};
    pub use crate::modules::calc_perform::{perform, actionSpeedMod};
    pub use crate::modules::calc_defence::{defence, buildDefenceEstimations, resistances, hitChance, armourReduction,
        armourReductionF, applyDmgTakenConversion, takenHitFromDamage, reducePoolsByDamage, defenceForConditionals};
    pub use crate::modules::calc_offence::{offence, calcTotemLife};
    pub use crate::modules::calc_triggers::triggers;
    pub use crate::modules::calc_mirages::mirages;
}
// Lua globals defined by calc modules, kept as free fns with the same names:
//   wipeEnv (CalcSetup:306), doActorLifeMana (CalcPerform:69), doActorLifeManaReservation (551),
//   isTriggered / calcMultiSpellRotationImpact (CalcTriggers:41/91), cacheSkillUUID / cacheData / wipeGlobalCache (Common).

// modules/calcs.rs [CA]
/// The cached DBs are `ModDB?` in Lua (CalcSetup.lua:472-474): nil until CS1's specCopy (590-692).
pub fn initEnv(build: &BuildRef, mode: Mode, r#override: Option<Rc<Override>>, specEnv: Option<&SpecEnv>)
    -> (EnvRef, Option<ModDB>, Option<ModDB>, Option<ModDB>);                  // CalcSetup.lua:475
pub fn perform(env: &EnvRef, skipEHP: bool);
pub fn buildOutput(build: &BuildRef, mode: Mode) -> EnvRef;                    // Calcs.lua:384
pub fn calcFullDPS(build: &BuildRef, mode: Mode, r#override: Rc<Override>, specEnv: Option<&SpecEnv>) -> FullDPS;
pub fn buildActiveSkill(env: &EnvRef, mode: Mode, skill: &ActiveSkillRef, targetUUID: Option<Name>, limitedProcessingFlags: &[Name]);
/// Calcs.lua:89-114: returns (closure, base output). The closure calls initEnv(build, "CALCULATOR", override) with no specEnv.
pub fn getMiscCalculator(build: &BuildRef) -> (Rc<MiscCalculator>, Output);
pub struct MiscCalculator { build: BuildRef, usedFullDPS: bool }
impl MiscCalculator { pub fn call(&self, r#override: Rc<Override>, useFullDPS: Option<bool>) -> Output; }
pub struct FullDPS { pub combinedDPS: f64, pub TotalDotDPS: f64, pub skills: Table, pub TotalPoisonDPS: f64,
    pub causticGroundDPS: f64, pub impaleDPS: f64, pub igniteDPS: f64, pub burningGroundDPS: f64, pub bleedDPS: f64,
    pub corruptingBloodDPS: f64, pub decayDPS: f64, pub dotDPS: f64, pub cullingMulti: f64, pub extra: Table }
/// Desktop wrapper used by the UI (D15).
#[cfg(not(target_arch = "wasm32"))]
pub fn try_buildOutput(build: &BuildRef, mode: Mode) -> Result<EnvRef, pob_lua::LuaError>;
// modules/power.rs [PWR] — CalcsTab.lua 496-773 (PowerBuilder as a resumable iterator, CalculatePowerStat,
// CalculateCombinedOffDefStat). Sequential by default (D12); `PowerOptions { parallel: bool }`, where parallel
// builds one Build per worker from the XML text.
```

---
## 6. ModParser contract (`pob-modparser/src/types.rs`, [U0])

```rust
pub enum Cap<'s> { Str(&'s str), Nil }
/// Lua closure params, in three call shapes (CCR-0006 §5):
///   specialModList `f(tonumber(cap[1]), unpack(cap))` (6662-6663): #1 = caps.num, #k≥2 = caps.cap[k-2];
///   preFlagList    `f(unpack(preFlagCap))` (6680-6682):             #1 = caps.cap[0] (a string), #k≥2 = caps.cap[k-1];
///   modTagList     `f(tonumber(c1), …)` if c1 matches "%d+", else `f(c1, …)` (6698-6712): #1 = caps.num or caps.cap[0].
/// `scan` fills caps.num = tonumber(cap1) for specialModList and, for modTagList, whenever cap1 contains a digit.
pub struct Caps<'s> { pub num: Option<f64>, pub cap: [Option<&'s str>; 5] }
/// The Lua "misc" table returned by form/tag/flag lists (ModParser.lua 6640-6993 reads these keys;
/// contracts.md#modspec has the census).
#[derive(Default, Clone)]
pub struct ModSpec {
    pub flags: u32, pub keywordFlags: u32, pub tag: Option<Tag>, pub tagList: Vec<Tag>,
    pub addToMinion: bool, pub addToMinionTag: Option<Tag>, pub addToSkill: Option<Tag>, pub addToAura: bool,
    pub onlyAddToBanners: bool,                    // read at 6935 (set at 1292)
    pub applyToEnemy: bool, pub actorEnemy: bool,  // actorEnemy: read at 6983 (set at 1283)
    pub newAura: bool, pub newAuraOnlyAllies: bool, pub affectedByAura: bool,
    pub playerTag: Option<Tag>, pub playerTagList: Vec<Tag>,   // 6959-6963, 6976-6979 (set at 1133-1137, 1263-1269)
    pub modSuffix: Option<Name>, pub modName: Option<Vec<Name>>, pub modType: Option<ModType>,
    /// The modNameList entry was a table, not a string: the DOUBLED form (6858-6863) writes
    /// `modName[2] = "Multiplier:"..modName[1].."Doubled"` into the shared table itself, so the overwrite persists
    /// for later parses; a string gets a fresh table each time. MP0/MP1 reproduce it or list it as a known diff.
    pub modNameIsTable: bool,
    pub entries: Vec<ModSpec>,                     // the array part of a multi-mod modExtraTags table (6888-6907)
    pub extra: Vec<(Name, Value)>,                 // any other key
}
pub enum TagVal { Spec(ModSpec), Fn(fn(&Caps) -> ModSpec) }
pub enum SpecialVal { Mods(Vec<Mod>), Fn(fn(&Caps) -> Option<Vec<Mod>>), Dyn(std::sync::Arc<dyn Fn(&Caps) -> Option<Vec<Mod>> + Send + Sync>) }
pub struct PatternEntry<T> { pub pat: &'static str, pub val: T, pub lua_line: u32 }
/// Built once (LazyLock) in source order. Generated entries (keystones 5943-5955, skill-name loops) use `Dyn`.
pub struct PatternList<T>(pub Vec<PatternEntry<T>>);
/// ModParser.lua:6616 scan(): lowest start, then longest end, then longest pattern; full ties → lowest lua_line (§3.4).
pub fn scan<'l, T>(line: &'l str, list: &PatternList<T>, plain: bool) -> Option<(&'static T, String, Caps<'l>)>;
/// modLib.parseMod (ModParser.lua 6994-7021): the two-order pass + memo cache, seeded from data().modCache.
/// Returns fresh copies on each call (Lua: unpack(copyTable(cache[line]))).
pub fn parseMod(line: &str, isComb: bool) -> (Option<Vec<Mod>>, Option<String>);
pub fn parseModNative(line: &str) -> (Option<Vec<Mod>>, Option<String>);   // the ModCache tier bypassed (L1 tests)
```

MP0's `helpers.rs` signatures (the generated lists call these; argument conversion such as `tonumber(level)` happens at the call site, CCR-0006 §6): `pub use pob_gamedata::mod_tools::createMod as r#mod` (createMod's argument dispatch, ModTools.lua:36-51, is resolved by the generator); `flag(name, source: Option<&str>, flags, keywordFlags, tags: &[Tag]) -> Mod`; `grantedExtraSkill(name: &str, level: Option<f64>, noSupports: Option<bool>) -> Option<Vec<Mod>>`; `triggerExtraSkill(name: &str, level: Option<f64>, options: Option<TriggerExtraSkillOptions>) -> Vec<Mod>` with `TriggerExtraSkillOptions { noSupports: Option<bool>, sourceSkill: Option<String>, triggerChance: Option<f64>, ignoreHexproof: bool, onCrit: bool }`; `extraSupport(name, level: Option<f64>, slot: Option<&str>, appliesToGrantedSkills: Option<bool>) -> Option<Vec<Mod>>`; `explodeFunc(chance: Option<f64>, amount: LuaArg, r#type: &str, tags: &[Tag]) -> Option<Vec<Mod>>` with `enum LuaArg<'a> { Nil, Num(f64), Str(&'a str) }`; `dealNoNonDamageType(dmgType: &str, forMinion: bool) -> Option<Vec<Mod>>`; `gemIdLookup(name: &str) -> Option<Name>`; `dmgTypes(name: &str) -> Option<Name>`. ConfigOptions applies that call `NewMod` without a source port as `modList.AddMod(Arc::new(createMod(…, None, …)))` (ModStore.lua:109-111).

Known parse-level differences (MP3-MP6 record them as `known_diffs.toml` entries in ModCache / parse-oracle comparisons; EvalMod, the only reader in the calc, sees the same): 8 specialModList entries (4576-4580, 5686, 5689, 5692) call `mod(…, nil, {tag})`, so createMod stores nil at `mod[1]` and `ipairs(mod)` never reaches the tag; 10 tag fields (`div`/`limit`/`percent`/`globalLimit`/`base`) hold a capture string in Lua and `tonumber` of it in Rust, only where the capture can hold nothing but digits.

Jewel funcs: the parser emits `Value::JewelFunc(JewelFuncSpec)`. The bodies run in pob-calc:

```rust
// pob-calc/src/modules/jewel_funcs.rs [MP7]
/// func(node, out, data) from ModParser.lua 6151-6615; node None = "finish" call (CalcSetup.lua 259-305).
pub fn callJewelFunc(spec: &JewelFuncSpec, node: Option<&NodeRef>, out: &ModList, data: &Table);
```

---

## 7. The giant functions: segments, contexts and orchestrators

### 7.1 Measured cross-segment locals

These lists come from `scripts/port/locals_audit.py`, a scope-aware analyser that resolves every identifier to its declaration. The analyser was run against the pin for this document. A ctx holds **exactly** the locals declared in one segment and referenced in another, plus the function's parameters: a parameter is a field unless it does not cross and a function-body local of the same name shadows it (initEnv's `override`; the local `override`485 is the field). At the pin the only listed parameter that does not itself cross is `env` of `calcs.defence` (CCR-0005). Everything else stays a local in its segment. If U0's re-run differs, U0's output wins and is recorded in `contracts.md`.

| Function | Segment starts (inclusive) | Crossing locals (declaration line) |
|---|---|---|
| `calcs.offence` 348-6263 | 348 · 1037 · 1692 · 2174 · [pass loop 2606-3955: 2606 · 3346] · 3957 · 4155 · [ailment loop 4164-5648: 4164 · 4990] · 5650 | params `env actor activeSkill`; `modDB`350 `enemyDB`352 `output`354 `breakdown`356 `skillModList`358 `skillData`359 `skillFlags`360 `skillCfg`361; local fns `calcAreaOfEffect`374 `calcResistForType`494 `runSkillFunc`509; `isAttack`1052 `debuffDurationMult`1491 `costs`1693 `monsterLife`1916 `canDeal`1932 `passList`1998 `combineStat`2066 (fn) `storedMainHandAccuracy`2171 `storedMainHandAccuracyVsEnemy`2172 `storedSustainedTraumaBreakdown`2173 `quantityMultiplier`2600 `ailmentData`4156 |
| pass loop body 2606-3955, split at 3346 | | `pass`2606 `source`2608 `output`2610 `cfg`2611 `breakdown`2613 `hitRate`3300 |
| ailment loop body 4164-5648, split at 4990 | | `pass`4164 `output`4168 `cfg`4169 `breakdown`4171 `ailmentMode`4289; local fns `checkWeapon1HFlags`4308 `calcAverageSourceDamage`4315 `calcAilmentDamage`4333 |
| `calcs.perform` 1286-3990 | 1286 · 1943 · 2308 · 2997 · 3497 | params `env skipEHP`; `modDB`1287 `enemyDB`1288 `output`1308 `partyTabEnableExportBuffs`1312 `hasGuaranteedBonechill`1331 `guaranteedChillSkills`1332 `summonedMinionOverride`1504 `breakdown`1514 `nonUniqueFlasksApplyToMinion`1609 `mergeFlasks`1691 (fn) `mergeTinctures`1862 (fn) `buffs`2194 `guards`2196 `minionBuffs`2197 `debuffs`2199 `curses`2201 `minionCurses`2202 `linkSkills`2205 `allyBuffs`2206 `buffExports`2207 `appliedCombustion`2306 `warcryList`2307 |
| `calcs.defence` 651-1673 | 651 · 1211 | params `env actor`; `modDB`653 `enemyDB`654 `output`656 `breakdown`658 `spellSuppressionChance`1137 |
| `calcs.buildDefenceEstimations` 1674-3873 | 1674 · 2153 · 2612 · 3165 | params `env actor`; `modDB`1675 `enemyDB`1676 `output`1678 `breakdown`1680 `damageCategoryConfig`1684 |
| `calcs.initEnv` 475-1975 | 475 · 800 · 1451 | params `build mode specEnv`; `cachedPlayerDB`478 `cachedEnemyDB`479 `cachedMinionDB`480 `env`481 `accelerate`482 `override`485 `modDB`486 |

The only early exits are `return` at CalcOffence.lua:371 (skill disabled), giving `Flow::Return` in OF-A, and the two initEnv restarts at CalcSetup.lua:1433 (in CS2) and 1923 (in CS3), giving `Flow::Restart`. `goto continue` stays inside one segment.

### 7.2 Context structs and orchestrators ([U0], frozen)

Rules:

- The ctx field names are the Lua local names. Local functions that cross a segment boundary become `pub(super) fn` items in the file of the unit that owns the defining lines. They take the ctx (and the loop-locals struct for loop closures) as explicit parameters. U0 declares their signatures in the stub. A local function that does not cross stays private to its file.
- The orchestrator owns only the function header and the ctx-initialisation statements listed in its `@pob` marker. Every other line belongs to a segment.
- `ctx_struct!` (from `probe.rs`) generates the struct together with `ProbeState`, so injection and export map locals to fields by name.

```rust
// modules/calc_offence/mod.rs
ctx_struct! { pub struct OffenceCtx {
    env: EnvRef, actor: ActorRef, activeSkill: ActiveSkillRef,
    modDB: ModDB, enemyDB: ModDB, output: Output, breakdown: Option<Breakdown>,
    skillModList: ModList, skillData: Table, skillFlags: Table, skillCfg: Cfg,
    isAttack: bool, debuffDurationMult: f64, costs: Table, monsterLife: f64, canDeal: Table,
    passList: Shared<Vec<Table>>, storedMainHandAccuracy: Option<f64>, storedMainHandAccuracyVsEnemy: Option<f64>,
    storedSustainedTraumaBreakdown: Table, quantityMultiplier: f64,
    ailmentData: Option<&'static IndexMap<Name, AilmentData>>,   // = data.nonDamagingAilment (4156), immutable data; None until OF-G
}}
ctx_struct! { pub struct HitPassLocals { pass: Table, source: Table, output: Output, cfg: Cfg, breakdown: Option<Breakdown>, hitRate: Option<f64> } }
ctx_struct! { pub struct AilmentPassLocals { pass: Table, output: Output, cfg: Cfg, breakdown: Option<Breakdown>, ailmentMode: Option<Name> } }
// declared in helpers_setup.rs [OF-A]:
//   pub(super) fn calcAreaOfEffect(ctx: &OffenceCtx, skillModList: &ModList, skillCfg: &Cfg, skillData: &Table,
//                                  skillFlags: &Table, output: &Output, breakdown: Option<&Breakdown>);
//   pub(super) fn calcResistForType(ctx: &OffenceCtx, damageType: Name, cfg: Option<&ModCfg>) -> f64;
//   pub(super) fn runSkillFunc(ctx: &OffenceCtx, name: SkillHook);
// costs_passes.rs [OF-C]: pub(super) fn combineStat(ctx: &OffenceCtx, stat: Name, mode: Name, extra: Option<&Table>);
// ailments_a.rs [OF-G]: checkWeapon1HFlags(ctx, al, cfg) / calcAverageSourceDamage(ctx, al, …) / calcAilmentDamage(ctx, al, …)

/// @pob Modules/CalcOffence.lua:348-361 (header + ctx init)
pub fn offence(env: &EnvRef, actor: &ActorRef, activeSkill: &ActiveSkillRef) {
    let mut ctx = OffenceCtx::init(env, actor, activeSkill);        // L349-361 (exact alias reads)
    seg!(helpers_setup::seg_setup(&mut ctx));                       // OF-A 362-1036 (incl. return at 371)
    seg!(skill_types::seg_skillTypeStats(&mut ctx));                // OF-B 1037-1691
    seg!(costs_passes::seg_costsAndPasses(&mut ctx));               // OF-C 1692-2173
    seg!(speed::seg_speed(&mut ctx));                               // OF-D 2174-2605 (sets globalOutput at 2176)
    for_ipairs!((_i, pass) in ctx.passList => {                     // L2606
        let mut hp = HitPassLocals::new(pass.table().unwrap());
        seg!(crit::seg_passCrit(&mut ctx, &mut hp));                 // OF-E 2607-3345 (sets globalOutput at 2607)
        seg!(hit_dps::seg_passHit(&mut ctx, &mut hp));               // OF-F 3346-3954
    });
    seg!(hit_dps::seg_combinePasses(&mut ctx));                     // OF-F 3956-4154
    seg!(ailments_a::seg_ailmentPrelude(&mut ctx));                 // OF-G 4155-4163
    for_ipairs!((_i, pass) in ctx.passList => {                     // L4164
        let mut al = AilmentPassLocals::new(pass.table().unwrap());
        seg!(ailments_a::seg_bleedPoison(&mut ctx, &mut al));        // OF-G 4165-4989 (sets globalOutput at 4165)
        seg!(ailments_b::seg_igniteNonDmgImpale(&mut ctx, &mut al)); // OF-H 4990-5647
    });
    seg!(dot_combined::seg_dotCombined(&mut ctx));                  // OF-I 5649-6263
}

// modules/calc_perform/mod.rs
ctx_struct! { pub struct PerformCtx {
    env: EnvRef, skipEHP: bool, modDB: ModDB, enemyDB: ModDB, output: Output, breakdown: Option<Breakdown>,
    partyTabEnableExportBuffs: bool, hasGuaranteedBonechill: bool, guaranteedChillSkills: Table,
    summonedMinionOverride: Option<f64>, nonUniqueFlasksApplyToMinion: bool,
    buffs: Table, guards: Table, minionBuffs: Table, debuffs: Table, curses: Table, minionCurses: Table,
    linkSkills: Table, allyBuffs: Option<Table> /* env.partyMembers["Aura"], 2206: nil when absent */,
    buffExports: Table, appliedCombustion: bool, warcryList: Table,
}}
// setup_flasks.rs [PF-C]: pub(super) fn mergeFlasks(ctx: &PerformCtx, flasks: &PtrMap<Item,bool>, onlyRecovery: bool,
//                                                   checkNonRecoveryFlasksForMinions: bool);
//                         pub(super) fn mergeTinctures(ctx: &PerformCtx, tinctures: &PtrMap<Item,bool>);
/// @pob Modules/CalcPerform.lua:1286-1288
pub fn perform(env: &EnvRef, skipEHP: bool) {
    let mut ctx = PerformCtx::init(env, skipEHP);                   // L1287-1288
    seg!(setup_flasks::seg(&mut ctx));                              // PF-C 1289-1942 (output 1305-1308 is ctx-assigned here)
    seg!(pools_charges::seg(&mut ctx));                             // PF-D 1943-2307
    seg!(buffs::seg(&mut ctx));                                     // PF-E 2308-2996
    seg!(party_curses::seg(&mut ctx));                              // PF-F 2997-3496
    seg!(ailments_dispatch::seg(&mut ctx));                         // PF-G 3497-3990 (defence/offence/triggers/mirages/cacheData)
}

// modules/calc_defence/mod.rs
ctx_struct! { pub struct DefenceCtx { env: EnvRef, actor: ActorRef, modDB: ModDB, enemyDB: ModDB, output: Output,
                                      breakdown: Option<Breakdown>, spellSuppressionChance: Option<f64> } }
ctx_struct! { pub struct DefEstCtx { env: EnvRef, actor: ActorRef, modDB: ModDB, enemyDB: ModDB, output: Output,
                                     breakdown: Option<Breakdown>, damageCategoryConfig: Name } }
/// @pob Modules/CalcDefence.lua:651-658
pub fn defence(env: &EnvRef, actor: &ActorRef) { let mut c = DefenceCtx::init(env, actor);
    seg!(primary::seg(&mut c)); seg!(recovery::seg(&mut c)); }       // DF-B 659-1210, DF-C 1211-1673
/// @pob Modules/CalcDefence.lua:1674-1680
pub fn buildDefenceEstimations(env: &EnvRef, actor: &ActorRef) { let mut c = DefEstCtx::init(env, actor);
    seg!(est_inputs::seg(&mut c)); seg!(est_pools::seg(&mut c));      // DF-D 1681-2152, DF-E 2153-2611
    seg!(est_ehp::seg(&mut c)); seg!(est_maxhit::seg(&mut c)); }      // DF-F 2612-3164, DF-G 3165-3873

// modules/calc_setup/mod.rs
ctx_struct! { pub struct InitEnvCtx {
    build: BuildRef, mode: Mode, specEnv: Option<SpecEnv>,
    cachedPlayerDB: Option<ModDB>, cachedEnemyDB: Option<ModDB>, cachedMinionDB: Option<ModDB>,
    env: Option<EnvRef>, accelerate: Accelerate, r#override: Rc<Override>, modDB: Option<ModDB>,   // env/modDB nil until CS1 (481, 486)
}}
/// @pob Modules/CalcSetup.lua:475-487 (header, ClearMatchKeywordFlagsCache, accelerator locals)
pub fn initEnv(build: &BuildRef, mode: Mode, r#override: Option<Rc<Override>>, specEnv: Option<&SpecEnv>)
    -> (EnvRef, Option<ModDB>, Option<ModDB>, Option<ModDB>) {
    let mut ctx = InitEnvCtx::init(build, mode, r#override, specEnv);   // L476-487 (override = override or {})
    for f in [init_env_base::seg as fn(&mut InitEnvCtx) -> Flow,       // CS1 488-799
              init_env_items::seg,                                     // CS2 800-1450 (Restart at 1433)
              init_env_skills::seg] {                                  // CS3 1451-1975 (Restart at 1923; final return)
        match f(&mut ctx) {
            Flow::Normal => {}
            Flow::Restart => return initEnv(build, mode, Some(ctx.r#override.clone()), ctx.specEnv.as_ref()),
            Flow::Return => unreachable!(),
        }
    }
    ctx.finish()                                                       // `return env, cachedPlayerDB, …` (1974)
}
```

A segment receives `&mut Ctx` and assigns ctx fields where Lua assigns the local. A field whose local is *declared* in a later segment is default-initialised by `init` and first written by its declaring segment. The probe dump validates that nothing reads it earlier.

### 7.3 Module-level state and globals

- `globalOutput`/`globalBreakdown` live in `OFFENCE_STATE` (§5.6). Segments read and write them through `OFFENCE_STATE.with(|s| s.globalOutput.get())`. The explosiveArrowFunc hook receives them (CalcOffence.lua:3038).
- The undeclared-global read sites listed by `globals_lint.py` at the pin (CCR-0004) are: `dotCfg` (CalcOffence 3527, 3538), `cfg` (CalcOffence 2506, 2507; no `cfg` is in scope at the top level of `calcs.offence` there, so `Sum("BASE", nil, "Multiplier:TraumaStacks")`), `affectedByAura` (CalcPerform 3464; `pairs(nil)` raises a LuaError when an `AffectedByAuraMod` LIST mod exists), `skillCfg` (CalcPerform 915; CalcBreakdown 212, 232), `modSource` (CalcDefence 1606, so `nil or "…"` takes the literal), `aura` (PartyTab 919; the loop variable of 911-917 is out of scope), `cfg` (Data/Skills/act_dex.lua:12991, Scourge Arrow `preDamageFunc`) and `scriptPath` (DataLegionLookUpTableHelper 136, 144; `nil .. "/Data/…"` raises a LuaError). Each is ported literally with `// LUA-GLOBAL-NIL`. `nullValue` (ModDB.lua:262, ModList.lua:207) is also an undeclared global (nil), so `EvalMod(…) or nullValue` leaves nil results out of `List`. `globals_lint.py --check` parses this bullet and fails unless the reported set equals it exactly.
- `globals_lint.py` also lists *implicit globals*: assignments to a name that no scope declares, inside a function (e.g. `mult` CalcPerform 2575/2580, `newMod` 1925, `srcList` 1738, `uncancellableReservation` 563, `simBreakdown` CalcTriggers 814), and their reads. Ports keep them as function locals unless a read before the write is reachable.

---

## 8. Closure families

| Family | Count (verified) | Lua location | Rust representation | Owner | Identity / drift check |
|---|---|---|---|---|---|
| ModParser `specialModList` | 2,098 entries | ModParser 2130-5942 (+ generated 5943-5955) | `PatternList<SpecialVal>` in source order; static entries become `Mods`, closures become `Fn` | MP3-MP6 | provenance marker on each block of ≈50 entries; L1 coverage |
| `formList`/`modNameList`/`modFlagList`/`preFlagList`/`modTagList` | modTagList 136 closures, preFlagList 7, rest static | ModParser 72-2010 | `PatternList<TagVal>` / `PatternList<ModSpec>` | MP1, MP2 | entry count + order equal to the dump; provenance |
| Helper constructors (`getSimpleConv`, `grantedExtraSkill`, `triggerExtraSkill`, `extraSupport`, `explodeFunc`, …) | ≈10 | ModParser 13-71, 2011-2129 | plain fns in `helpers.rs` | MP0 | provenance |
| Jewel funcs | jewelOtherFuncs + Self + SelfUnalloc + Threshold = 64-69 bodies | ModParser 6151-6615 | `Value::JewelFunc(JewelFuncSpec)` + `callJewelFunc` `match spec.key` | MP7 | per-func oracle (§9.4 L2) |
| Skill hooks | 133: preDamage 122, initial 4, preSkillType 3, postCrit 3, explosiveArrow 1; preDot 0 (CalcOffence:5806 calls it, no data defines it). By file: act_int 68, act_dex 41, act_str 15, other 5, minion 2, spectre 1, sup_str 1 | Data/Skills/*.lua | `GrantedEffect.<hook>: Option<LuaFnRef>` → registry | SK-A (act_int), SK-B (rest) | `registry_complete`: every manifest id implemented once, hash equal (shared bodies such as AltX/AltY copies may map to one fn) |
| ConfigOptions `apply` | 531 | ConfigOptions.lua | `ConfigOptionDef.apply: Option<LuaFnRef>` → registry | CO-A (1-1188), CO-B (1189-2380) | `registry_complete` |
| ModMap `apply` | 41 | Data/ModMap.lua | registry | CO-C | `registry_complete` |
| CalcTriggers `configTable` | 82 | CalcTriggers 902-1584 | `fn configTable(key: Name, env: &EnvRef) -> Option<TriggerConfig>` as one `match` in Lua order | TR-B | provenance per arm |
| CalcMirages configs | ≈10 | CalcMirages 60-427 | `MirageConfig` built inline with boxed closures, as Lua builds them | MI | provenance |
| CalcBreakdown funcs | 9 | CalcBreakdown 17-255 | methods on `Breakdown` (§5.5) | CT | provenance |
| Local closures in calc fns | ≈40 | Calc*.lua | private fns or ctx-taking fns (§7.2) | owning segment | provenance |
| Memo metatables | `damageStatsForTypes`, `multiplierName`/`conditionName`, `matchKeywordFlagsCache`, `mergeLevelCache` | CalcOffence 53, ModStore 20-28, Global 178, CalcActiveSkill 21 | `LazyLock` table / `conditionName()` / plain fn / thread-local memo | OF-A / U0 / P1 / AS | provenance |
| `statMap` `__index` + `processMod` | 1 | Data.lua 52-93, 1033-1046 | `statMapLookup` | CT | provenance |
| `skillCond` metatables | 5 sites | CalcActiveSkill 482/487; CalcOffence 4422/4695/5000 | `IndexMeta::{Table, OrChain}` | U0 type | provenance |
| `table.sort` comparators | many | everywhere | closure passed to `pob_lua::sort::sort` | caller | provenance of caller |

### 8.1 Registry code (`data_fns/mod.rs`, `config_options/mod.rs`, `mod_map.rs`; [U0] frames)

```rust
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SkillHook { initialFunc, preSkillTypeFunc, preDamageFunc, postCritFunc, preDotFunc, explosiveArrowFunc }
/// func(activeSkill, output, breakdown): `output`/`breakdown` are the offence top-level ones (runSkillFunc, CalcOffence.lua:509-514).
pub type SkillFunc = fn(activeSkill: &ActiveSkillRef, output: &Output, breakdown: Option<&Breakdown>);
/// explosiveArrowFunc(activeSkill, output, globalOutput, globalBreakdown, env) (act_dex.lua:6699).
pub type ExplosiveArrowFunc = fn(&ActiveSkillRef, &Output, &Output, Option<&Breakdown>, &EnvRef);
pub enum SkillFuncImpl { Std(SkillFunc), ExplosiveArrow(ExplosiveArrowFunc) }
pub struct FnRow<F> { pub id: &'static str, pub hash: &'static str, pub f: F }
pub static SKILL_FUNCS: LazyLock<AHashMap<Name, &'static FnRow<SkillFuncImpl>>>;   // = act_int::ROWS ++ act_dex::ROWS ++ …
pub fn skillFunc(r: &LuaFnRef) -> &'static SkillFuncImpl;   // unknown id → unported!() no-op

/// apply(val, modList, enemyModList, build) (ConfigOptions.lua).
pub type ConfigApply = fn(val: &Val, modList: &ModList, enemyModList: &ModList, build: &BuildRef);
pub static CONFIG_APPLY: LazyLock<AHashMap<Name, &'static FnRow<ConfigApply>>>;  // apply_a::ROWS ++ apply_b::ROWS
/// ModMap apply. The caller (ConfigOptions.lua:117-130 mapAffixDropDownFunction) picks one of three call shapes from
/// affixData.type, and the Lua bodies' own parameter lists vary (ModMap.lua:12 vs :20). Each body is ported against the
/// call shape it actually receives, so positional mismatches are reproduced. The tier comes from UI state
/// (`build.configTab.varControls['multiplierMapModTier'].selIndex`), modelled as `ConfigTab.varControlsSelIndex`.
pub enum ModMapApply {
    check(fn(val: &Val, mapModEffect: f64, modList: &ModList, enemyModList: &ModList)),
    list(fn(tier: f64, mapModEffect: f64, values: &Table, modList: &ModList, enemyModList: &ModList)),
    count(fn(tier: f64, count: f64, mapModEffect: f64, values: &Table, modList: &ModList, enemyModList: &ModList)),
}
pub static MODMAP_APPLY: LazyLock<AHashMap<Name, &'static FnRow<ModMapApply>>>;

// calc_triggers/mod.rs
pub struct TriggerConfig {
    pub source: Option<ActiveSkillRef>, pub triggerName: Option<Name>, pub triggerOnUse: bool, pub useCastRate: bool,
    pub triggerChance: Option<f64>, pub assumingEveryHitKills: bool, pub sourceWeapon: Option<Name>,
    pub triggerSkillCond: Option<Rc<dyn Fn(&EnvRef, &ActiveSkillRef) -> bool>>,
    pub customHandler: Option<fn(&EnvRef, &mut TriggerConfig) -> Option<Table>>,
    pub comparer: Option<fn(&EnvRef, Name, &ActiveSkillRef, f64) -> bool>,
    pub extra: Table,                              // remaining keys read by defaultTriggerHandler (TR-A census)
}
// calc_mirages.rs
pub struct MirageConfig {
    pub calcMainSkillOffence: bool, pub mirageSkill: Option<ActiveSkillRef>,
    pub compareFunc: Option<Box<dyn Fn(&ActiveSkillRef, &EnvRef, &MirageConfig, Option<ActiveSkillRef>) -> Option<ActiveSkillRef>>>,
    pub preCalcFunc: Box<dyn Fn(&EnvRef, &ActiveSkillRef, &EnvRef)>,
    pub postCalcFunc: Box<dyn Fn(&EnvRef, &ActiveSkillRef, &EnvRef)>,
    pub mirageSkillNotFoundFunc: Box<dyn Fn(&EnvRef, &MirageConfig)>,
}
```

Function ids are the ids of `data/dump/closures.json` (AMENDMENTS A2, `crates/pob-dump/FORMAT.md`), for example `skills.Barrage.preDamageFunc` and `ConfigOptions[<i>].apply` (1-based position in ConfigOptions.lua; not `configOptions.<var>.apply`, CCR-0006 §7), with sha256 body hashes.

---
## 9. Data pipeline, oracle and testing

### 9.1 Dump (P1: `cargo run -p pob-extract --release -- dump`)

1. **Boot.** Boot PoB under LuaJIT through `pob_luahost::jit` (the full PoB boot), so `Data.lua` post-processing has run.
2. **Walk.** Walk the result with `walker.lua`, iterating keys in **sorted order**, and write `data/dump/`:
   - `data.json`: the `data` table, minus UI-only keys.
   - `trees/<ver>.json` and `sprites/<ver>.json`: all 41 `treeVersionList` entries, including legacy 2_6 and 3_6-3_9.
   - `mod_cache.json`, `skill_stat_map.json`, `config_options.json`, `mod_map.json`, `calc_sections.json`, `stat_descriptions/<scope>.json`, `pearl_supports.json`, `game_versions.json`.
   - `global.json`: ModFlag, KeywordFlag, SkillType, colorCodes.
   - `closures.json`: the manifest.
   - `parse_oracle.jsonl`: PoB `modLib.parseMod` run over every stat line in Mod*.lua, Uniques, all tree versions, tattoos and cluster notables. Each record holds the line, the mods, `extra`, and the winning pattern with its Lua line, from an instrumented `scan`.
   - `meta.json`.
3. **Canonical encoding** (the array/map ambiguity never reaches Rust):

   | Lua value | JSON |
   |---|---|
   | table with keys exactly 1..n | array |
   | any other table | object; integer keys are written `"#<n>"` |
   | integral number, \|x\| < 2^53 | int |
   | other finite number | `%.17g` |
   | NaN / ±inf | `{"$num":"nan"\|"inf"\|"-inf"}` |
   | function | `{"$fn":"<dotted path>","src":"<file>:<a>-<b>","hash":"<sha1[0..8] of whitespace-stripped source lines>"}` |
   | shared table | emitted once at its canonical path; other references become `{"$ref":"<path>"}` |
   | known back-references (`statMap._grantedEffect`) | dropped, and re-linked by the loader |
   | metatable | `"$meta":"<name>"` |

4. **Compile (U0's `pob-extract compile`).** Deserialise into `pob_gamedata::Data` with `#[serde(deny_unknown_fields)]` on every schema struct, then write `data/dump/data.bin` (postcard). Runtime loads only `data.bin`, plus lazily loaded tree files.
5. **Codegen (P1).** `pob-dump --codegen` writes `crates/pob-dump/generated/{enums.rs,global.rs,misc.rs}`: `global.rs` and the verbatim-key record structs for `misc`, `characterConstants`, `monsterConstants` and `gameConstants` (§4.2). pob-gamedata includes `global.rs`/`misc.rs` by path (CCR-0003). Generated files are checked in, and `pob-dump --codegen --check` fails CI when they differ. `data.misc` is in `misc.json["misc"]` (data keys whose own file name is reserved go into the `misc.json` group).
6. **Determinism.** Two dumps MUST be byte-identical.

### 9.2 Oracle infrastructure (P3, ORC-X)

**Process model.** One fresh LuaJIT PoB process per build, which avoids the known cross-build state leak (corpus 031→051). Every run goes through `scripts/port/oracle.sh`, which holds a `flock`, so only one oracle process exists at a time (≈400 MB).

**Interpreter only (CCR-0001/0002).** Every oracle parity run uses the LuaJIT interpreter: `JitPob::boot_oracle` runs `jit.off()` (and `jit.flush()`) before anything else (`pob_oracle::jit::ORACLE_JIT_OFF`), as the parse oracle in pob-dump does. Compiled traces of this LuaJIT are wrong for `string.sub` with a variable negative end and drop the sign of zero of narrowed results (§3.5). `POB_ORACLE_JIT=on` exists for diagnostics only; the `pob_timing` baseline (G1a) runs PoB as it ships, JIT on. Every `outputs.json`/`stages.json` records the LuaJIT that produced it: `"luajit": {"arch", "jit", "numberMode": "single"|"dual"}`. On a dual-number host a `±0` difference is a host artefact (§3.5).

**Patched source.** The patcher copies `.PathOfBuilding/src` to `target/oracle-src/<sha>/`.
- For every `[[stage]]` in `docs/port/stages.toml` (`id`, `file`, `line`, `locals`), it inserts `if __PROBE then __PROBE("<id>", {<locals>}) end` before the line. Both the `:in` and `:out` boundaries of every segment in §7 are stages.
- It wraps function probes around `calcs.initEnv`, `perform`, `defence`, `buildDefenceEstimations`, `offence`, `triggers`, `mirages`, `buildActiveSkillModList`, `createActiveSkill`, `runSkillFunc`, every skill hook, every config apply, jewel funcs, `getCachedOutputValue` and `ConfigTab:BuildModList`.
- It validates the patched files with `loadstring`.
- Hits are numbered `<id>#<n>` per process, which separates passes, recursive envs and repeated performs. Each dump also records the env identity (`$env`) and the call stack of probe ids. A build load runs `calcs.buildOutput` several times (plus `buildActiveSkill`/`calcFullDPS` envs), so `#1` is usually not the main env: the selector `<glob>@main` picks the first hit of each matching probe in `calcsTab.mainEnv` and is the default for env-level and line probes. Tests name hits as `hits.json` lists them (e.g. `offence:out#4`), never assume `#1` (CCR-0010).
- Line probes before a split function's final `end` are `<fn>:end`; `<fn>:in`/`<fn>:out` are the patcher's function probes (a line probe named `<fn>:out` would collide with them; CCR-0011 §9).

**pob-state/1 dump.** Each dump is deterministic JSON, keys sorted, with an identity-preserving graph (`$id`/`$ref`). Data objects are written as `{"$data":"data.skills.Barrage"}` (pob-dump canonical paths, FORMAT.md, so a marker resolves against `data/dump` directly; CCR-0010), nodes as `{"$node":id}` (allocated) / `{"$treeNode":id}`, and items field-wise with modList, slotModList, weaponData, armourData, flaskData and jewelData. The full format is in `crates/pob-oracle/README.md`. Contents:
- **env:** modes, flags, override, alloc nodes, granted lists.
- **Each actor:** fields, output (nested), breakdown (structural).
- **Each mod store:** mods per name in order, written with a canonical `formatMod`-equivalent encoding plus the inserting Lua `file:line`; conditions; multipliers; parent `$ref`.
- **Each active skill:** uuid, effect ids/levels/quality, skillTypes, skillFlags, skillData, every cfg (skillCond layers via `$meta.__index`: a `$ref` for table layers, `{"$fn": "Modules/CalcOffence.lua:4422-4422", "upnames": ["skillCfg", "cfg"]}` for the function layer, whose upvalues the oracle cannot read since its state has no `debug` library; CCR-0010), skillModList, buffList.
- **The listed locals.**
- **GlobalCache entries:** scalars plus `$ref` to env and skill.

**Write provenance.** In the probe build, `actor.output`, `output.MainHand`/`OffHand`/`Minion` and `breakdown` get `__newindex` proxies that record the last writer `file:line` of every key. `ModDB:AddMod`/`ModList:AddMod` record each inserted mod's caller line. `pob_calc_diff --unit <id>` uses this to filter mismatches by the unit's Lua ranges.

**Inject and export.**
- `pob_oracle::inject::state(&dump, &BuildRef) -> (EnvRef, Locals)` rebuilds the Rust graph with sharing intact. `ctx_struct!`'s `ProbeState` maps locals to ctx fields by name.
- `pob_calc::probe` (feature `probe`) exports the same schema from Rust via `probe!(ctx, "<id>")`.

**Replay.** `pob_oracle::replay` swaps a not-yet-ported callee for its recorded `in→out` state delta, keyed by hit id. Examples: PF-G calling `offence`, OF-A calling skill hooks, triggers calling `buildActiveSkill`.

**Diff.**
- Numbers must agree within `|a-b| ≤ 1e-9·max(1,|a|)`, so a `±0` sign mismatch passes (a dual-number host artefact, CCR-0002); bools and strings must be identical.
- ModStores compare as a multiset per name, plus an order check whose failures are reported as `LUA-ORDER`.
- Lists compare in order, except the nondeterministic set (§9.5).
- Output is `{build, pob, ours}`, so `scripts/parity/report.py` works unchanged.

**Cache.** Dumps are cached at `.parity/probe-cache/<sha>/<build-blake3>/<stage>.json.zst`. A test whose cache entry is missing prints the `oracle.sh` command to fill it and is skipped. CI pre-fills the cache.

### 9.3 Corpora

- `all57`: `.parity/builds`.
- `slice5`: `crates/pob-oracle/corpus/U0/slice5.txt` — five all57 builds reduced to their main socket group (`corpus/U0/derive_slice5.py`, CCR-0012; files in `corpus/U0/slice5/`): exactly one weapon, a single attack or spell, no aura, curse, buff, warcry, mirage, trigger or minion skill and no `Extra*` aura/curse/link/minion mod (checked against the oracle with `derive_slice5.py --check`). No all57 build meets that as is. Entries: `slice5_allflame_exp_024` (Winter Orb), `_007` (Spark of the Nova), `_030` (Ethereal Knives of the Massacre), `_031` (Shield Crush), `_048` (Flicker Strike; CCR1 replaced `_046`, which had no weapon). If undoctored builds are preferred later, P3's `synthetic` one-skill builds are the alternative.
- `dps30`: the builds with TotalDPS > 0.
- `curated/<topic>`: triggers, mirages, timeless, cluster, minion, aura, party, flasks.
- `unit/<id>`: micro-builds each unit authors in `crates/pob-oracle/corpus/<id>/`.
- `synthetic`: P3 generates one build per skill, setting each skill as the main skill from a template, plus all-config-vars builds.

### 9.4 Test levels

| Level | What | Gate |
|---|---|---|
| L0 | `pob-lua` helpers and `LjIntMap` vs the LuaJIT **interpreter** (`jit.off`; `L0_JIT_ON=1` adds an informational `jit.on` run whose trace-only divergences are counted): random and edge vectors, ≥10k per fn. Includes every `s_format` literal in Calc*.lua, NaN/±0 cases for `m_min`/`m_max`, and `table.sort` permutations with tie-heavy comparators. The port's x64 profile is compared as shipped; on a dual-number host a mismatch is tolerated (and counted) only when `with_luajit_arm64_profile` reproduces the oracle exactly (CCR-0001 table). Run in debug and `--release` (`lpow` folding) | 100% identical (modulo the counted, ARM64-profile-reproduced host differences) |
| L1 | ModParser: all of ModCache (23,207 lines) with the cache tier **disabled**, plus `parse_oracle.jsonl` | ≥ 99.95%; full-tie lines accepted per §3.4; every entry in a unit's range is hit by ≥ 1 oracle line, or listed |
| L2 | Model. Item: ParseRaw fields, modList, slotModList[1..2], weaponData, armourData, flaskData, tinctureData, jewelData for every unique (all variants) and corpus item. Tree: node.modList and nodesInRadius for every node of every version. Spec: alloc, conquered and timeless rewrites, cluster subgraphs. Jewel funcs: each func on every node of the 3_26 tree | 100% |
| L3 | ModStore query matrix on dumped DBs at `perform:out`: every mod name × cfg ∈ {nil, skillCfg, weapon1Cfg, weapon2Cfg, dotCfg, bleed/poison/ignite cfgs} × {Sum BASE/INC, More, Flag, Override, List, Max, Min, Tabulate, GetCondition, GetMultiplier, GetStat} | 100% |
| L4 | Segment: inject at `<seg>:in#n`, run the Rust segment, diff against `<seg>:out#n` | 100% on the unit's curated + unit corpus; ≥ 98% of hits on all57, with the remainder documented in `known_diffs.toml` |
| L5 | Stage: `initEnv:out`, `perform:out`, `buildOutput` in each mode, every socket group as main skill. Plus the D13 fixed-point test: build → buildOutput → buildOutput gives identical outputs and model state | as L4 |
| L6 | End to end. Headline keys (Life, ES, Mana, all resists, TotalEHP, TotalDPS, CombinedDPS, FullDPS) must be exact. ≥ 99.5% of all keys exact. Power report: node power deltas equal the oracle's for 3 builds × all nodes | nightly; per-PR on a 12-build smoke set |

### 9.5 PoB nondeterminism (D8)

`pob_oracle` runs each build **twice** (two processes, so two hash seeds). Any key, mod order or list order that differs between the runs is recorded in `.parity/nondet/<build>.json`. The diff accepts a Rust value that equals any value PoB produced; for orders, it compares as sets. For a build flagged by the first pair, the oracle runs a third time (`pob_oracle nondet --max-runs N` keeps running while runs add entries). Integer-keyed orders MUST NOT appear in this set: if one does, `LjIntMap` usage is missing at that site, which is a bug.

Recorded by P3 (CCR-0010): over the corpus PoB's outputs never differed between runs, but the dumped states do — mostly mod order within mod lists / ModDB names, and also values: the gem variant a skill resolves to varies between PoB processes (`ExplosiveTrap` / `ExplosiveTrapAltX` in allflame_exp_019; `VaalRighteousFire` / its AltX variant in upstream_Dual_Wield_Cospris_CoC), so the calc port accepts either variant where PoB does. The record (`pob-nondet/2`) uses the paths the state diff walks, records a list as unordered only when its own elements move, and records mod-store differences per store name. Order-only differences inside mod stores are sampled, not enumerated: the state diff compares mod stores as multisets and reports unrecorded order differences as `LUA-ORDER` (not a value mismatch); PoB's run-to-run value differences inside a mod store are recorded per store name as alternative multisets. With the JIT off (§9.2), a `±0` difference between runs cannot come from trace timing.

### 9.6 Provenance and upstream sync (P4)

- `scripts/port/provenance.py check` verifies every `@pob` hash against `.PathOfBuilding`.
- `provenance.py coverage` reports in-scope lines (`docs/port/scope.toml`) that no marker or `@pob-skip` covers, or that two markers cover. It MUST report 0 from U0 onward.
- `provenance.py bump --to <sha>`:
  - remaps every range through `git diff -U0` hunks;
  - rewrites line numbers in markers whose hash is unchanged;
  - lists each changed range with its Lua diff as a work item.
- Data closures follow the same process through `closures.json` and `registry_complete`. Schema drift is caught by `deny_unknown_fields`.

### 9.7 Performance gates

- **G1a (after MS-B).**
  - `Sum`/`More`/`Flag` over the all57 player DBs at `perform:out`, measured with criterion: Rust ns/query ≤ the LuaJIT ns/query of the same query, timed by `pob_timing`.
  - `dhat`: `Sum`, `More` and `Flag` allocate nothing when names are `n!` constants.
- **G1b (vertical slice, after CS3 + PF-A..D + DF-A..C + OF-A..F).**
  - slice5 from XML: headline keys strict.
  - `MiscCalculator::call` per node: ratio ≤ 1.0 against LuaJIT.
- **Failure rule.** If either gate fails, Wave 5 does not start. Profile with `perf` and fix the hot path in its owning unit (expected: MS-*, `Table`, `cat!`). Semantic changes are never allowed.
- **Final (INT-2).** Power report over the full tree ≤ LuaJIT time on this machine, single-threaded.

---

## 10. Process rules for agents

### 10.1 Environment

```sh
export CARGO_BUILD_JOBS=3
command -v sccache >/dev/null && export RUSTC_WRAPPER=sccache
export CARGO_TARGET_DIR=$WORKTREE/target          # per-worktree (shared dirs thrash path-based fingerprints)
scripts/port/cargo.sh check -p pob-calc           # flock-serialised cargo (one compile machine-wide)
scripts/port/cargo.sh test -p pob-oracle --test <unit-id>
scripts/port/oracle.sh run --build X.xml --stage <id>   # flock-serialised LuaJIT
```

- At most **3 agents** run at once. Each works in its own git worktree on branch `port/<unit-id>`.
- Never build the whole workspace in release mode. Never run `cargo test --workspace`.

### 10.2 Definition of done for a unit

1. `grep -n 'unported!' <owned files>` is empty, except for documented `@pob-skip` sites.
2. The acceptance test in `crates/pob-oracle/tests/<unit-id>.rs` passes at its gate.
3. `provenance.py check` and `coverage` pass for the unit's files.
4. No edits outside owned files. Contract gaps are recorded as CCRs.
5. Every `LUA-ORDER`, `LUA-QUIRK`, `LUA-GLOBAL-NIL` and `LUA-ALIAS` site in the unit's range is marked.

### 10.3 Contract change requests (CCRs)

- Frozen files change only through `docs/port/ccr/NNNN-<slug>.md`, which states the problem, the Lua evidence (file:line) and the proposed diff.
- Until the CCR is merged, the requesting agent works around the gap locally in its own file and marks the workaround `// CCR-NNNN`.
- The integrator applies CCRs serially between merges and re-runs `cargo.sh check` for all crates. An applied CCR file gets a `Status: applied <date> by <unit>` line and stays in `docs/port/ccr/` as the evidence record; the binding text is the edited doc.

---
## 11. Work units

A **wave** is a dependency level: it lists what *may* run in parallel. It does not mean everything in it runs at once, since the hard cap is 3 concurrent agents (D20). **Deps** must be merged before a unit starts. Units in Waves 3-4 seed from the oracle and replay their callees, so they depend on U0, ORC-X and their helper units only, never on upstream segments. Every unit also owns `crates/pob-oracle/tests/<id>.rs` and `crates/pob-oracle/corpus/<id>/`. File paths are relative to `crates/` unless they start with `scripts/` or `docs/`.

### 11.1 Unit table

| ID | Lua sources (lines @16de4b82d) | Owned Rust files | Deps | Acceptance test | Wave |
|---|---|---|---|---|---|
| **REN** | — | workspace `Cargo.toml`; `pob-luahost/**` (moved from `pob-calc/`); `pob-ui/Cargo.toml`, `pob-extract/Cargo.toml` (alias deps) | — | `cargo.sh check -p pob-ui -p pob-extract`; `cargo.sh test -p pob-luahost --test worker_smoke`; the app still runs | 0 |
| **P1** dump + codegen | Data/Global.lua 1-360; Modules/Data.lua (executed); TreeData/*; GameVersions.lua; ModCache; ConfigOptions; ModMap; CalcSections; StatDescriptions; PearlSupports | `pob-extract/src/dump/{mod.rs,walker.lua,canonical.rs,codegen.rs}`, `pob-gamedata/src/generated/*` | REN | two dumps byte-identical; SkillType has 135 entries (Area=8, Aura=39, Pact=135); `closures.json` = 133 skill + 531 config + 41 modmap; 41 tree versions; `parse_oracle.jsonl` ≥ 80k lines | 0 |
| **P2** pob-lua | Common.lua 709-800, 1013-1023, 306 (sanitiseText); LuaJIT `lib_math`, `lib_string` format, `lib_table` sort, `lj_tab` | `pob-lua/src/{math,bit,format,string,pattern,sort,ipairs,ljtab}.rs` | — | L0: ≥ 10k vectors per fn identical to LuaJIT (dev-dep on pob-luahost); `LjIntMap` order equals LuaJIT for 10k random insert/delete sequences | 0 |
| **P3** oracle core | HeadlessWrapper boot; patching of Calc*.lua | `pob-oracle/{Cargo.toml,src/{lib,runner,patcher,schema,diff,nondet}.rs,lua/*.lua,src/bin/*}`, `scripts/port/oracle.sh` | REN | function probes dump pob-state/1 for all57; two runs identical outside the nondet set; the patcher round-trips a dummy stages.toml through `loadstring`; write provenance covers ≥ 99% of output keys; `pob_calc_diff` output accepted by `report.py` | 0 |
| **P4** port scripts | all in-scope Lua (analysis only) | `scripts/port/{locals_audit,segments,provenance,globals_lint,pairs_inventory}.py`, `scripts/port/cargo.sh` | — | `locals_audit` reproduces §7.1 exactly; `globals_lint --check` reports exactly the §7.3 sites (parsed from the doc text); `pairs_inventory` lists every `pairs()` site in scope with its key class (int/string/identity); provenance check/coverage/bump pass on fixtures | 0 |
| **U0** skeleton | everything (signatures, census) | every `[U0]` file in §2.1; `pob-extract/src/compile.rs`; `docs/port/{stages.toml,scope.toml,STYLE.md,contracts.md,known_diffs.toml,pairs_inventory.md}`; one stub per unit-owned file with final signatures, `@pob` markers and `unported!` bodies | P1-P4 | `cargo.sh check -p pob-lua -p pob-gamedata -p pob-modparser -p pob-calc` green; wasm32 check green; `compile` reports zero unknown keys and `data.bin` loads in < 1 s; `registry_complete` lists every manifest id; `provenance coverage` = 100%; the patched oracle with every stage loads; `buildOutput` on slice5 runs end to end on stubs without panicking (`tests/u0_smoke.rs`) | 1 |
| **T1** transpiler | ModParser table syntax; `apply = function(val, modList, enemyModList)` forms | `scripts/port/lua2rs/**` | P4 | formList, modNameList and modFlagList translate to compiling Rust; ≥ 80% of ConfigOptions applies and ≥ 95% of static plus one-line specialModList entries translate; golden tests | 1 |
| **MS-A** ModStore/DB/List | ModStore.lua 1-362; ModDB.lua 1-370; ModList.lua 1-249 | `pob-calc/src/classes/{mod_store,mod_db,mod_list}.rs` | U0 | L3 for all non-tag paths; ScaleAddMod/ScaleAddList aura scaling on all57; Replace/Convert parent recursion; no ModList globalLimits; `HasMod` on a ModList raises LuaError; Max returns nil for all values ≤ 0 | 2 |
| **MS-B** EvalMod | ModStore.lua 363-971 | `pob-calc/src/classes/eval_mod.rs` | MS-A (signatures) | L3 full matrix 100% on all57; `tag.div` no-reader proof; **gate G1a** | 2 |
| **CT** calc tools | CalcTools.lua 1-303; CalcBreakdown.lua 1-255; CalcBase.lua 1-93; CalcFormat.lua 1-74; ModTools.lua 1-258; Common.lua 477-560, 897-962; Data.lua 52-93, 1033-1046 | `pob-calc/src/modules/{common,calc_tools,calc_breakdown,calc_base,calc_format,mod_tools_env}.rs`, `pob-gamedata/src/{mod_tools,skill_stat_map}.rs` | U0 | canGrantedEffectSupportActiveSkill over all (support, skill) pairs; doesTypeExpressionMatch fuzz; buildSkillInstanceStats for every gem at levels {1, 20, 21} × quality {0, 20}; formatMod identical on ModCache; statMapLookup = Lua `statMap[stat]` for all skills × stats; cacheSkillUUID on the corpus; breakdown helpers byte-equal on dumped inputs | 2 |
| **MP0** parser core | ModParser.lua 1-71, 2011-2129, 5943-6150, 6616-7021 | `pob-modparser/src/{helpers,scan,mod_cache,generated_lists}.rs` | U0 | ModCache tier serves 100% of cached lines; scan tie-break matches every non-full-tie in `parse_oracle`; whole-parser L1 once MP1-6 land | 2 |
| **MP1** forms + names | ModParser.lua 72-912 | `pob-modparser/src/{form_list,mod_name_list}.rs` | U0, T1 | entry count and order equal the dump; L1 lines whose form/name come from this range pass | 2 |
| **MP2** flags + tags | ModParser.lua 913-2010 | `pob-modparser/src/{mod_flag_list,pre_flag_list,mod_tag_list}.rs` | U0, T1 | as MP1; each of the 143 closures checked against LuaJIT on sample captures | 2 |
| **LUT** timeless/abyss | DataLegionLookUpTableHelper.lua 1-262; DataAbyssJewelLookUpTableHelper.lua 1-275; DataJewelFileLoader.lua 1-95 | `pob-gamedata/src/timeless_lut.rs` | U0 | `readLUT`/abyss component results identical for every jewel type × 200 seeds × all affected nodes | 2 |
| **PT** PassiveTree | PassiveTree.lua 1-908, 1041-1062; Data.lua 583-600 (setJewelRadiiGlobally) | `pob-calc/src/classes/passive_tree.rs` | U0, MP0 | L2: every node's fields, modList, modKey, positions (1e-9), nodesInRadius for all 41 versions | 2 |
| **IT-A** Item ParseRaw | Item.lua 424-1645 | `pob-calc/src/classes/item_parse.rs` | U0, MP0 | L2 ParseRaw fields for every unique (all variants) and corpus item | 2 |
| **IT-B** Item core | Item.lua 1-423, 1646-2191; ItemSlotHelper.lua 1-40; Utils.lua 1-125 | `pob-calc/src/classes/item.rs` | U0 | `BuildRaw(ParseRaw(x))` text equals Lua; NormaliseQuality, variants, GetPrimarySlot, FindModifierSubstring | 2 |
| **BM-B** skills/party/config model | SkillsTab.lua 441-630, 1129-1274, 1478-1502; PartyTab.lua 516-669, 718-1042; ConfigTab.lua 839-1129 | `pob-calc/src/model/{skills_tab,party_tab,config_tab}.rs` | U0 | socketGroupList after UpdateSocketGroups equals the oracle on the corpus; ParseBuffs mod lists equal; config Load/Save round trip | 2 |
| **ORC-X** inject/replay | — | `pob-oracle/src/{inject,replay,testkit}.rs`, `pob-calc/src/probe.rs` | U0, P3, MS-A (AddMod) | identity round trip (Lua dump → inject → Rust export = dump) at `initEnv:out`, `perform:in` and `offence:in` for all57; replay of `offence`/`runSkillFunc` deltas | 2 |
| **MP3-MP6** specialModList | ModParser.lua 2130-3083 / 3084-4036 / 4037-4988 / 4989-5942 | `pob-modparser/src/special_{a,b,c,d}.rs` | U0, T1, MP2 (ModSpec helpers) | L1: every entry in range hit and matching; after MP6, whole-parser L1 ≥ 99.95% with the cache disabled | 3 |
| **MP7** jewel funcs | ModParser.lua 6151-6615 | `pob-modparser/src/jewel_func_list.rs`, `pob-calc/src/modules/jewel_funcs.rs` | MP0, MS-A | per func: PoB vs Rust on every node of the 3_26 tree → identical `out` ModList and `data` table | 3 |
| **IT-C** item mod lists | Item.lua 2192-2736; ItemTools.lua 1-413 | `pob-calc/src/classes/{item_mods,item_tools}.rs` | IT-A, IT-B, MS-A | L2: modList, slotModList, weaponData, armourData, flaskData, tinctureData, jewelData | 3 |
| **PS-A** PassiveSpec core | PassiveSpec.lua 1-1091 | `pob-calc/src/classes/passive_spec.rs` | PT | Load/Save, URL decode/encode, alloc and paths equal the oracle on the corpus | 3 |
| **PS-B** depends/paths | PassiveSpec.lua 1092-1696 | `pob-calc/src/classes/passive_spec_paths.rs` | PS-A, LUT | L2 BuildAllDependsAndPaths: conquered/timeless/abyss rewrites on the corpus + 60 synthetic timeless builds | 3 |
| **PS-C** clusters | PassiveSpec.lua 1697-2494 | `pob-calc/src/classes/passive_spec_cluster.rs` | PS-A | L2 subgraph ids, mods and legacy remap on cluster builds | 3 |
| **BM-A** build XML + items/tree tabs | Build.lua 976-1099, 2222-2335; ItemsTab.lua 1216-1727, 2998-3010; TreeTab.lua 500-560; CalcsTab.lua 163-225; runtime/lua/xml.lua 1-186 | `pob-calc/src/model/{build_xml,items_tab,tree_tab,calcs_tab,xml}.rs` | IT-A, IT-B, PS-A, BM-B | all57 load; save→load round trip; a Rust-saved XML loaded in LuaJIT PoB gives identical outputs | 3 |
| **CO-A** config apply A | ConfigOptions.lua 1-1188 | `pob-calc/src/modules/config_options/apply_a.rs` | MS-A, MP0, T1 | every option in range × {default, 3 non-defaults} → identical modList/enemyModList (apply probe) | 3 |
| **CO-B** config apply B | ConfigOptions.lua 1189-2380 | `pob-calc/src/modules/config_options/apply_b.rs` | as CO-A | as CO-A | 3 |
| **CO-C** BuildModList + ModMap | ConfigTab.lua 1130-1204; Data/ModMap.lua 1-441 | `pob-calc/src/model/config_build_mod_list.rs`, `pob-calc/src/modules/mod_map.rs` | BM-B, CO-A/B (registry only) | `configTab.modList`/`enemyModList` equal (order included) on the corpus + all-vars synthetic builds | 3 |
| **AS** CalcActiveSkill | CalcActiveSkill.lua 1-934 | `pob-calc/src/modules/calc_active_skill.rs` | MS-A/B, CT | `createActiveSkill:out` and `buildActiveSkillModList:out` (skillTypes, skillFlags, skillData, cfgs, skillModList order, buffList, minion skills) for every skill in corpus + synthetic | 3 |
| **SK-A** skill hooks A | Data/Skills/act_int.lua closures (68) | `pob-calc/src/data_fns/skills_act_int.rs` | CT, ORC-X | per hook: inject `runSkillFunc:in#n`, diff `:out#n` (activeSkill, output, breakdown deltas); `registry_complete` for its ids | 3 |
| **SK-B** skill hooks B | act_dex (41), act_str (15), other (5), minion (2), spectre (1), sup_str (1) closures | `pob-calc/src/data_fns/{skills_act_dex,skills_act_str,skills_misc}.rs` | as SK-A | as SK-A | 3 |
| **PF-A** perform helpers A | CalcPerform.lua 1-635 | `calc_perform/actor_a.rs` | MS, CT, ORC-X | function probes: getCachedOutputValue, mergeBuff, doActorLifeMana, doActorAttribsConditions, doActorLifeManaReservation, determineCursePriority, applyEnemyModifiers | 3 |
| **PF-B** perform helpers B | CalcPerform.lua 636-1285 | `calc_perform/actor_b.rs` | as PF-A | function probes: doActorMisc, doActorCharges, actionSpeedMod, initMinionModDB, addMinionModifiers | 3 |
| **PF-C** | CalcPerform.lua 1289-1942 | `calc_perform/setup_flasks.rs` | PF-A, PF-B (sigs) | L4 `1289`→`1943` (createMinionSkills replayed until AS lands) | 3 |
| **PF-D** | CalcPerform.lua 1943-2307 | `calc_perform/pools_charges.rs` | PF-A (sigs) | L4 `1943`→`2308` | 3 |
| **PF-E** | CalcPerform.lua 2308-2996 | `calc_perform/buffs.rs` | PF-A (sigs) | L4 `2308`→`2997` on aura/herald/curse/warcry builds | 3 |
| **PF-F** | CalcPerform.lua 2997-3496 | `calc_perform/party_curses.rs` | PF-C (mergeFlasks sig) | L4 `2997`→`3497` | 3 |
| **PF-G** | CalcPerform.lua 3497-3990 | `calc_perform/ailments_dispatch.rs` | ORC-X replay | L4 `3497`→`perform:out`, with defence/offence/triggers/mirages replayed | 3 |
| **DF-A** defence helpers | CalcDefence.lua 1-650 | `calc_defence/helpers.rs` | MS, CT | 100k random inputs vs LuaJIT for hitChance, armourReduction(F), applyDmgTakenConversion, takenHitFromDamage, reducePoolsByDamage; resistances probe | 3 |
| **DF-B..G** | CalcDefence.lua 659-1210 / 1211-1673 / 1681-2152 / 2153-2611 / 2612-3164 / 3165-3873 | `calc_defence/{primary,recovery,est_inputs,est_pools,est_ehp,est_maxhit}.rs` (one per unit) | DF-A (sigs), ORC-X | L4 on each segment's stage pair | 3 |
| **OF-A** | CalcOffence.lua 1-347 (helpers), 362-1036 | `calc_offence/helpers_setup.rs` | ORC-X, CT | L4 `362`→`1037` (hooks replayed); calcDamage, area, cooldown, duration and totemLife fuzzed vs LuaJIT | 3 |
| **OF-B** | CalcOffence.lua 1037-1691 | `calc_offence/skill_types.rs` | OF-A (sigs) | L4 | 3 |
| **OF-C** | CalcOffence.lua 1692-2173 | `calc_offence/costs_passes.rs` | OF-A (sigs) | L4 incl. conversionTable | 3 |
| **OF-D** | CalcOffence.lua 2174-2605 | `calc_offence/speed.rs` | OF-A (sigs) | L4 | 3 |
| **OF-E** | CalcOffence.lua 2606-3345 (pass body A) | `calc_offence/crit.rs` | OF-A (sigs) | L4 `2607#p`→`3346#p` | 3 |
| **OF-F** | CalcOffence.lua 3346-3955 (pass body B), 3956-4154 | `calc_offence/hit_dps.rs` | OF-C (combineStat sig) | L4 `3346#p`→ next pass / `4155` | 3 |
| **OF-G** | CalcOffence.lua 4155-4989 | `calc_offence/ailments_a.rs` | OF-A (sigs) | L4 `4155`→`4990#p` | 3 |
| **OF-H** | CalcOffence.lua 4990-5648 | `calc_offence/ailments_b.rs` | OF-G (closure sigs) | L4 `4990#p`→ next pass / `5649` | 3 |
| **OF-I** | CalcOffence.lua 5649-6263 | `calc_offence/dot_combined.rs` | OF-C (sigs) | L4 `5649`→`offence:out`; CombinedDPS/TotalDot strict on dps30 | 3 |
| **CS0** | CalcSetup.lua 1-474 | `calc_setup/{init_mod_db,node_mods,support}.rs` | MP7, PT, CT | function probes: initModDB, buildModListForNode(List), wipeEnv, applyGemMods, applySocketMods, addBestSupport | 4 |
| **CS1** | CalcSetup.lua 488-799; PantheonTools.lua 1-18 | `calc_setup/init_env_base.rs` | CS0, CO-C, BM-B | L4 `488`→`800` | 4 |
| **CS2** | CalcSetup.lua 800-1450 | `calc_setup/init_env_items.rs` | CS0, IT-C, PS-B, PS-C | L4 `800`→`1451`, incl. the Restart path (extraJewelFuncs) and toggleFlask/repItem overrides | 4 |
| **CS3** | CalcSetup.lua 1451-1975 | `calc_setup/init_env_skills.rs` | CS0, AS, BM-B | L4 `1451`→`initEnv:out`; Energy Blade Restart; then L5 `initEnv:out` for all57 × 3 modes; **gate G1b** once PF/DF/OF slices pass | 4 |
| **TR-A** | CalcTriggers.lua 1-901 | `calc_triggers/handlers.rs` | OF-*, ORC-X (cache injection) | L4 `triggers:in/out` with the GlobalCache injected; corpus: CoC, CWC, Mjölner, Cospri's, focus, CwDT, unique triggers | 5 |
| **TR-B** | CalcTriggers.lua 902-1627 | `calc_triggers/config_table.rs` | TR-A | as TR-A; every configTable arm hit | 5 |
| **MI** | CalcMirages.lua 1-427 | `modules/calc_mirages.rs` | AS, OF-* | `mirages:out` on mirage archer, Reflection/Saviour, Tawhoa, General's Cry, Sacred Wisps builds, incl. stale-CALCULATOR-cache cases | 5 |
| **CA** | Calcs.lua 1-872 (38-88 `@pob-skip` infoDump) | `modules/calcs.rs` | all of Waves 2-4 | L5/L6: buildOutput MAIN/CALCS, calcFullDPS, getMiscCalculator, cost warnings (Calcs.lua 399-436), MAIN/CALCS post-processing and used-tracking (437-872), D13 fixed-point test | 5 |
| **PWR** | CalcsTab.lua 441-495, 496-773 | `modules/power.rs` | CA | node power values equal PoB's for 3 builds × all nodes (sequential mode); parallel mode's divergences reported, not gated | 5 |
| **INT-1** parity triage | — | fixes land in owner files (reviewed by owner) | Wave 5 | L6 gates on all57 + 100 fresh poe.ninja builds | 6 |
| **INT-2** performance | — | profiling-driven, no semantic change | CA, PWR | power report ≤ LuaJIT time single-threaded; full recalc ≤ LuaJIT | 6 |
| **INT-3** breakdown parity | — | owner files | CA | CALCS-mode breakdown tables structurally equal on all57; Tabulate after calc equal | 6 |
| **INT-4** retire | — | delete `pob-engine`, old `pob-data`; `pob-luahost` kept until the new UI ships | new UI on pob-calc | `cargo tree -p pob-calc -e normal \| grep -c mlua` = 0; wasm32 check of all runtime crates | 6 |

### 11.2 Critical path and scheduling

- **Critical path:** REN → P1/P3/P4 → U0 → MS-A → MS-B (G1a) → ORC-X → {OF-*, PF-*, DF-*} → CS3 (G1b) → CA → INT-1.
- **Parallel branches.** The ModParser branch (MP0-MP7) and the model branch (IT-*, PT, PS-*, BM-*, CO-*) run beside the critical path and join at CS0-CS3. The calc segments seed from the oracle, so they never wait for the parser or the model.
- **Suggested order inside a wave** (3 agents at a time):
  - Wave 2: MS-A, CT, MP0 first; then MS-B, ORC-X, MP1, MP2; then PT, IT-A, IT-B, LUT, BM-B.
  - Wave 3: OF-A..F and PF-A..D first, because they feed G1b.
- **Shipping.** The current LuaJIT-backed app keeps working throughout, via REN. The new UI switches to `pob_calc::calcs::buildOutput` once CA passes L6.

---
## 12. Errata: judge findings, re-verified and fixed in this document

| Proposal claim | Verified fact at the pin | Fixed in |
|---|---|---|
| fidelity: OffenceCtx without `ailmentData` | `local ailmentData = data.nonDamagingAilment` (CalcOffence:4156) is used at 4246/4319 (OF-G) and 5386-5499 (OF-H) | §7.1, OffenceCtx |
| fidelity: PerformCtx "verified" incl. minionCounts, hasActiveSpectreSkill, activeSpectreList, maxImpaleStacks, curseSlots | none of these cross a segment. Missing from the list: `breakdown` (1514), `mergeFlasks` (1691 → 3121), `mergeTinctures` (1862 → 2095), `nonUniqueFlasksApplyToMinion` (1609), `summonedMinionOverride` (1504) | §7.1 (measured) |
| fidelity: HitPassCtx with exertedDoubleDamage/maxCullPercent; globalOutput as a per-pass default | only `pass source output cfg breakdown hitRate` cross 3346. `globalOutput`/`globalBreakdown` are file-level upvalues (CalcOffence:65/67), assigned at 2176/2607/4165, and read by combineStat (2137) after the pass loops | §5.6 OFFENCE_STATE, §7.2 |
| fidelity: `Override` with plain fields | initEnv mutates it: `extraJewelFuncs` (CalcSetup:1424-1430), `conditions` (1921-1922) | §5.4 Override (Field-based, shared Rc) |
| fidelity: seg!/Flow cannot restart initEnv | `return calcs.initEnv(build, mode, override, specEnv)` at CalcSetup:1433 (CS2) and 1923 (CS3) | `Flow::Restart`, §7.2 |
| fidelity: Table::pairs snapshots values | Lua `next` reads values live and skips keys niled during traversal | §5.1 `pairs()` |
| fidelity: "a borrow panic cannot occur" | `with`/`update` closures can re-enter | §3.3 rule 1, `CheapClone`, leaf-only closures |
| fidelity: `catch_unwind` for LuaError on all targets | wasm32-unknown-unknown is panic=abort | D15 |
| fidelity: Field::get clones IndexMap/Vec | — | `CheapClone` bound + `Shared<_>` |
| fidelity: todo_port! stubs | — | `unported!` returns Default (D9) |
| performance: "PoB always rebuilds CALCULATOR entries before reading" | false: CalcMirages:127-137, CalcTriggers:529, Calcs:402 rebuild only when absent; entries persist until wipeGlobalCache (Common:944-948; Build.lua:693/1260) | D11, D12 |
| performance: CondLayer with a single parent | the OrChain `skillCfg.skillCond[k] or cfg.skillCond[k]` at CalcOffence:4422/4695/5000 | `IndexMeta::OrChain` |
| performance: CachedSkill.env = None for calcFullDPS | readers of `.Env.player.output[...]` include Calcs:408-413 (MAIN, from buildActiveSkill envs) and CalcTriggers:529-533 / getCachedOutputValue (CalcPerform:31-43). calcFullDPS's CALCULATOR entries alias the re-performed fullEnv, whose `player.output` is replaced on every perform (CalcPerform:1305) | live `EnvRef` (D11) |
| performance: quirk at CalcOffence:2189 | the string-as-cfg calls are at 2194-2195 | §3.2 |
| performance: `skill_stats: &SymMap<f64>` | `cfg.skillStats = output.MainHand/OffHand` (CalcOffence:2008/2025) aliases a pass output; GetStat reads it (ModStore:355) | `ModCfg.skillStats: Field<Option<Output>>` |
| performance: generated SkillFlag bitset | computed keys: `skillFlags[string.lower(ailment)]` (CalcOffence:4158), `skillFlags[map.skillFlag]` (CalcActiveSkill:354); false ≠ nil | dynamic `Table` (D3) |
| performance: alloc_nodes in ascending id order | LuaJIT integer-key order is deterministic but not sorted | `LjIntMap` (D8) |
| parallel-porting: 4990 split crosses only ailmentMode + fns | `ailmentData` and the loop locals pass/output/cfg/breakdown also cross | §7.1 |
| parallel-porting: only accelerate/cached DBs/override cross initEnv 800 | `env` (481), `modDB` (486, used e.g. 1118, 1916) and the params `build mode specEnv` cross too | §7.1 |
| parallel-porting: snapshot GlobalCache is enough | `copyActiveSkill` uses the cached skill's `actor`, `supportList`, `socketGroup`, `summonSkill` (CalcActiveSkill:181) and parents the new skillModList to that actor's modDB (492) | D2, D11 |
| parallel-porting: `Arc<CalcOverride>` immutable | see Override above | §5.4 |
| parallel-porting: 22/33 parallel agents; shared CARGO_TARGET_DIR across worktrees | 3 GB RAM → ≤ 3 agents; per-worktree target dirs + flock | D20, §10.1 |
| all three: insertion order = PoB pairs order | string-key order is random per process (seeded hash, lj_str.c:367); integer-key order is deterministic | D8, §3.4, §9.5 |
| all three: no module-level mutable upvalues | `globalOutput`/`globalBreakdown`; `mergeLevelCache` (CalcActiveSkill:21) | §5.6 |
| all three: no undeclared-global handling | `dotCfg` (CalcOffence:3527/3538), `affectedByAura` (CalcPerform:3464), `modSource` (CalcDefence:1606), `nullValue` (ModDB:262, ModList:207); CCR-0004 added `cfg` (CalcOffence:2506/2507, act_dex:12991), `skillCfg` (CalcPerform:915, CalcBreakdown:212/232), `aura` (PartyTab:919), `scriptPath` (DataLegionLookUpTableHelper:136/144) | D16, §7.3 |
| (new) `data` treated as immutable | `data.setJewelRadiiGlobally` mutates `data.jewelRadius`/`maxJewelRadius` on SetActiveSpec (Data.lua:583-600, TreeTab.lua:553) and PassiveTree reads them (PassiveTree.lua:662-676) | `DataSel` (§4.4) |
| (new) ModMap apply has one signature | three call shapes chosen by affix type (ConfigOptions.lua:117-130); bodies differ in arity (ModMap.lua:12 vs :20) | `ModMapApply` enum (§8.1) |
| (new) ModList:HasMod works | ModList.lua defines no HasModInternal → Lua error | §5.3 |

## 13. Verified counts and constants (for quick reference)

- **Calc file sizes:** CalcOffence 6263, CalcPerform 3990, CalcDefence 3874, CalcSetup 1975, CalcTriggers 1627, CalcActiveSkill 934, Calcs 872, CalcMirages 427, CalcTools 303, CalcBreakdown 255, ModParser 7021, ModStore 971, ModDB 370, ModList 249.
- **ModParser:**
  - specialModList: 2,098 entries, all anchored `^…$` (ModParser:5951-5953).
  - modTagList: 136 closures; preFlagList: 7.
  - jewelFuncList: built from 4 tables (6151-6595).
- **Closures:**
  - skill hooks: 133 (122/4/3/3/1, preDot 0);
  - ConfigOptions apply: 531;
  - ModMap apply: 41;
  - CalcTriggers configTable: 82.
- **ModFlag/KeywordFlag:** u32, max bit `MatchAll = 0x40000000`.
- **SkillType:** 135 entries incl. the operators OR 73, AND 74, NOT 75 (Area 8, Aura 39, Pact 135); `COUNT` = 136 bits.
- **Hooks:** `runSkillFunc` passes the offence top-level `output`/`breakdown` (CalcOffence:509-514). Call sites: 516, 1054, 1908, 3315, 5806. explosiveArrowFunc is called at 3038.
- **Global functions defined by calc modules:** `wipeEnv`, `doActorLifeMana`, `doActorLifeManaReservation`, `isTriggered`, `calcMultiSpellRotationImpact`, `cacheSkillUUID`, `cacheData`, `wipeGlobalCache`, `MatchKeywordFlags`, `ClearMatchKeywordFlagsCache`.
