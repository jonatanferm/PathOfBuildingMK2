# Port style (binding for every calc porting unit)

Owned by U0. Sections 1-6 below are calc-architecture.md §3 **verbatim** (reviewers enforce
them). The section after them lists the accepted amendments to those rules (CCRs) and the
conventions U0's skeleton established. `docs/port/AMENDMENTS.md` wins on conflict; then the CCRs
listed here; then the verbatim text.

## Lua-semantics rules (calc doc §3, verbatim)

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
| `#t` | `t.len()`: LuaJIT's border. Every `Table` keeps a model of its LuaJIT layout (`pob_lua::ljtab::LjTab`), so `#t`, `t_insert` and `t_remove` agree with LuaJIT after holes too (CCR-0031 A). `tbl!` builds through `Table::from_ctor`: a literal value or `[k]` key (`1.0`, `-2`, `"s"`, `n!("s")`, `true`, `false`, `Val::Nil`) is a template constant, any other expression a run-time store; write folded literals (LuaJIT folds `{1+2}`). A positional nil item keeps its position (`tbl![a, Val::Nil, c]` is `{[1]=a, [3]=c}`) |
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
| `f(a, modDB:Override(nil, "X"))`, the Override a trailing argument (`m_max(sum, modDB:Override(nil, "ChillVal"))`, CalcPerform.lua:870/884/894) | an unmatched `Override` returns **no value** (`OverrideInternal` has no final return, ModDB.lua:229-251, ModList.lua:177-197), so the call has one argument fewer: `m_max(sum)`, not `m_max(sum, nil)` (which raises). Port it as the shorter call when `Override` gives `Value::Nil` (PF-B's `actor_b::lua_max_override`), marked `// LUA-QUIRK` (CCR-0095 §2). `Max`/`Min` always return a value; `Flag` returns `true` or no value |
| `t.k = modDB:Flag(nil, "X")` (a Flag result stored in a table, e.g. `output.EnergyShieldProtectsMana`, Calcs.lua:406) | assign `true` or nil, never `false`: `t.set(k, if flag { Val::Bool(true) } else { Val::Nil })`. Lua's `Flag` returns `true` or nothing (ModDB.lua:206-227, ModList.lua:155-175), and storing nothing is a nil assignment, which clears any value already at `k`; `if flag { t.set(k, true) }` is wrong because it keeps that value (calcs.rs L406; CCR-0097 §3, FOLLOWUPS "CS0" 1) |

### 3.3 Borrow rules (they also apply under `Rc`)

1. Never hold a `Ref`/`RefMut` across a call. Public APIs return **owned values or handles**, never references into a `RefCell`. `Field::with`/`Shared::with` closures MUST be leaf code: no calls into calc, ModStore or table methods on the same object. Debug builds report a violation as a panic that cites this rule.
2. Compute first, then write: `let v = q(...); output.set(k, v);`.
3. `Field<T>` requires `T: CheapClone`, which is Copy, an `Rc`/`Arc`/`Weak` handle, `Name`, `Table`, `Shared<_>`, or `Option` of these. Collections that Lua mutates in place and aliases are `Shared<Vec<_>>` / `Shared<IndexMap<_,_>>` / `Table`, never a `Field<Vec<_>>`.
4. `Rc` cycles are forbidden. Back-pointers (`ActiveSkill.actor`, `ActiveSkill.summonSkill`, `ModStore.actor`, `Actor.enemy/parent/player`) are `Weak`. `Env` owns its actors. `GlobalCache` owns cached envs, and wiping it frees them. `ActiveSkill.summonSkill` (CCR-0135): the summoning skill owns its minion (`activeSkill.minion`), which owns the minion's skills; readers use `ActiveSkill::summonSkill()`, which panics on a dead handle (an ownership bug, never a Lua nil); `copyActiveSkill` keeps a summoning skill whose actor is not `env.player` alive in the copy's `extra["summonSkillKeep"]` (the same keep-alive as CCR-0113's `actorKeep`). Exception (CCR-0113): a skill made by `copyActiveSkill` whose source actor is not `env.player` (a foreign env's actor, CalcActiveSkill.lua:181) also owns that actor, as `newSkill.extra["actorKeep"]` (`calc_active_skill::actorKeepKey`, a `Val::Obj(ActorRef)`; probe exports show it as an extra key), because the source env can be freed while the copy lives on (CalcPerform.lua:3989); a same-env copy owns nothing. `crates/pob-calc/tests/leak1_cycles.rs` checks that a dropped env and the envs a recalc replaces free their whole graph. Two build-level cycles remain (CCR-0142): `Env.build` / `MiscCalculator.build` ↔ `calcsTab.{mainEnv, calcsEnv, miscCalculator}`, and `ActiveSkill.socketGroup` ↔ `SocketGroup.displaySkillList(Calcs)` are strong (Lua aliases). A recalc replaces both and leaks nothing; whoever drops a whole build or a socket group must clear those edges first. Only the web calc worker does (`worker_web.rs` `run_request`, CCR-0142 §3 A); the desktop app's build switch and socket-group deletion do not (no `Build::teardown`, FOLLOWUPS "CCRDOC" 7).

### 3.4 Iteration order policy

- **Integer-keyed tables iterated with `pairs` whose order is observable** (`env.allocNodes`, `env.grantedPassives`, `spec.allocNodes`, `extraRadiusNodeList`, `nodesInRadius`, and other node-id maps; the full list is in `docs/port/pairs_inventory.md`, produced by P4 and frozen by U0) use `pob_lua::ljtab::LjIntMap<V>`. It reproduces LuaJIT 2.1 `lj_tab.c` exactly: array part, hash part, `hashnum` main position, free-pointer collision chaining, rehash sizing, and `next()` order. Measured on the vendored LuaJIT: keys `{52031,1031,26725,55485,33545,6,7,1,2,3,99999,4184}` iterate as `1,2,3,6,7,26725,4184,55485,99999,1031,52031,33545` in every run.
- **String-keyed tables** use insertion order. PoB's own order is random per process (seeded hash, D8), so any output that depends on it is nondeterministic in PoB. The oracle detects such outputs (§9.5) and the diff compares them as sets of the values PoB produced.
- **Table-identity keys** (`pairs(affectedByAura)`, `isSupporting[srcInstance]`) also follow a random order in PoB (address hashing). Use insertion order (`PtrSet`/`PtrMap`).
- **`LjIntMap` needs Lua's insertion order** (CCR-0024 §5, CCR-0028 §9, CCR-0031). It reproduces LuaJIT for a given sequence of stores. Where that sequence itself comes from a `pairs` over a table that ever held a string key, PoB's order is per-process random: LuaJIT hashes strings by a per-state seeded id, which moves the integer keys' hash slots. Example: `tree.nodes` of the 3.10+ trees holds the string key `root` until PassiveTree.lua:510, so `pairs(tree.nodes)`, the fill order of `spec.nodes` and `pairs(spec.nodes)` differ between PoB processes; such sites compare as multisets (§9.5; ui doc §8 rule 6 (b)). `Table::len` uses a fixed string-hash stand-in, so `#t` of a table that mixes string keys with *holed* integer keys is a valid border but not always LuaJIT's (LuaJIT itself differs between processes there). Data tables written as `[n] = { … }` constructors (`grantedEffect.levels`) live entirely in the hash part: use `calc_tools::levels_next_key` / `levels_len` for `next(levels)` / `#levels`, never `LevelTable::len()` (CCR-0091 §2).
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

`ModRef = Arc<Mod>` is immutable once a store holds it. `docs/port/contracts.md#mod-mutation` lists the Lua sites that mutate a mod (or a tag) in place, in two tables. **Fresh-copy sites** (U0's audit, e.g. ModStore.lua 425-430/514-516/564-566, CalcPerform.lua 2452/2539/2564/2610/2718/2846/2921, CalcActiveSkill.lua:38, Item.lua 2336, ModTools.lua 115) mutate a copy nobody else holds: `copyTable` immediately before the mutation, a `copyTable(buffExports.Aura[...])` (CalcPerform.lua:3870), or a mod under construction. Rust ports these as "build a new `Mod`, then insert". **Shared-target sites** (found after U0 and accepted through CCRs: ModTools.lua 254-255 `mergeKeystones`, CalcPerform.lua 629, 1951-1952, 2380-2384, 2788-2790, 3879-3897, CalcSetup.lua 424, Item.lua 2556-2560, ModParser.lua 6473-6474 and 6860) write into a mod, tag or table that other holders share. Each row of that table names its port (a rebuilt or re-sourced copy replacing the entry where the branch reads it afterwards, or a `LUA-ALIAS` reproduction) and the PoB-visible difference, if any, which goes to `known_diffs.toml`. `modLib.setSource` (ModTools.lua 238-244) also writes `mod.value.mod.source`, so a shallow `copyTable(mod, true)` before it is not a fresh copy of the inner mod; whether a `setSource` call is fresh depends on its caller. A site listed in both tables (CalcPerform.lua 3879-3897, Item.lua 2558) follows its shared-target row. A newly found site whose target is shared MUST be filed as a CCR before porting, and the CCR adds it to the shared-target table.



## Amendments and U0 conventions

### Accepted rule changes (CCRs)

All CCRs below are applied to the calc doc text (unit CCR1, 2026-10-09), so §3 above already
contains them; this list says where each one bites.

- **CCR-0001 (LuaJIT build profile) / CCR-0002 (dual-number ±0).** The shipped port always
  follows x64 single-number PoB, on every host and target: `pob_lua::{LJ_TARGET_X86ORX64,
  LJ_DUALNUM}` are the constants `true`/`false`; the ARM64 dual-number profile exists only behind
  pob-lua's test-only `oracle-dualnum` feature. Oracle parity runs use the LuaJIT interpreter
  (`jit.off()`). A ±0 difference against an ARM64 oracle is a host artefact (the numeric diff
  tolerates it; record sign-derived differences in `known_diffs.toml` with the CCR number), never a
  port bug. `lpow` is libm `pow` (no `lj_vm_powi`); LuaJIT never prints a NaN sign.
- **CCR-0004 (undeclared globals).** The `LUA-GLOBAL-NIL` sites are §7.3's list, which now includes
  the nine sites of CCR-0004 (`cfg` CalcOffence 2506/2507, `skillCfg` CalcPerform 915 and
  CalcBreakdown 212/232, `aura` PartyTab 919, `cfg` act_dex 12991, `scriptPath`
  DataLegionLookUpTableHelper 136/144). `scripts/port/globals_lint.py --check` parses §7.3 and
  must stay green.
- **CCR-0006 (lua2rs shapes, T1).** Adopted by U0: the extra `ModSpec` keys, `TagType::Global`,
  `Tag.excludeItemType`, the conversion impls and the MP0 helper signatures; generated pattern
  lists and config applies compile against the real crates.
- **CCR-0005 (ctx params rule).** A parameter is a ctx field unless it does not cross and a body
  local shadows it (initEnv's `override`).
- **CCR-0011 (U0 contract notes).** In particular: `copyTable` does **not** keep the metatable
  (only `copyTableSafe(…, preserveMeta)` does); `Table::copyTable` follows the Lua. The rest of
  CCR-0011 lists the type-level deviations from the calc doc text (`ModStore.parent` is a
  `Field`, `initEnv` returns `Option` cached DBs, the `InitEnvCtx.env/modDB` are `Option`,
  `ModCfg::STRING_CFG()` is a function, …); `docs/port/contracts.md` is the reference.

CCRs 0014-0094 were decided by unit CCR2 (2026-11-10; AMENDMENTS A8 has the table). The rules
that bite every porting unit:

- **CCR-0031 (`#t`, constructors, test inputs).** `Table::len` is LuaJIT's border (§3.2); write
  `tbl!` constants as literals and fold arithmetic yourself. Tests locate the PoB checkout, `data/`,
  `data/dump` and `.parity` only through the dev-dependency `pob-testpaths`
  (`pob_testpaths::require`): a missing input **fails** unless `POB_ALLOW_SKIP=1`.
- **Runtime writes into `data`** (calc doc §4.4 table: `hasGlobalEffect`, `levelCostCleared`,
  `grantedEffect_fromItem`, the LUT cache, `DataSel`). Read such a field through its accessor,
  never from the immutable data.
- **Shared-mod writes.** contracts.md#mod-mutation lists the sites where Lua writes into a shared
  mod or tag and how each is ported (rebuild and replace where the branch reads it, or a re-sourced
  copy). A new site needs a CCR before porting.
- **Nil in typed fields.** `NaN` for a nil `Cell<f64>`, `$<field>Nil` markers where Save tells nil
  apart, `Val::Obj` for objects inside dynamic tables (contracts.md#class-census conventions).
- **Insertion order.** `LjIntMap` matches LuaJIT only for Lua's insertion sequence; a sequence that
  comes from a `pairs` over a table that held a string key is per-process random (§3.4). Use
  `calc_tools::levels_next_key`/`levels_len` for `next(levels)`/`#levels`.
- **Test seams.** A seam that lets a test answer a callee or a hook (`*::replay`, `callee_replay`,
  `replay_hook`, `set_skill_hook_handler`, `SpecStubs`, `PscCalls`) compiles only under the `probe`
  feature (or is never set by the product) and changes nothing when unset.

CCRs 0043-0145 were decided by unit CCRDOC (2026-11-12; AMENDMENTS A14). The rules that bite:

- **Zero-value returns (CCR-0095 §2, CCR-0097 §3).** An unmatched `Override` returns no value, so as
  a trailing argument it shortens the call (§3.2); a `Flag` result stored in a table is `true` or
  nil, never `false`.
- **Keep-alives instead of cycles (CCR-0113, CCR-0135, CCR-0142).** A strong reference that Lua
  needs but that would close an `Rc` cycle goes into a Rust-only `extra` key (`actorKeep`,
  `summonSkillKeep`, `override.extra["$extraJewelFuncsActor"]`) or an explicit teardown, as §3.3
  rule 4 says.
- **Float keys (CCR-0102).** `Table` keys may be non-integral numbers (`Key::F`); match `Key`
  exhaustively.

### Conventions of the skeleton

- **Provenance.** Every file starts with `//! @pob-file <path> @16de4b82d` per Lua file it ports.
  A ported fn or segment carries `/// @pob <path>:<a>-<b> h=<8hex>`; module-level statements carry
  `// @pob …` comment markers; deliberate exclusions carry `// @pob-skip <path>:<a>-<b> reason="…"`.
  `scripts/port/provenance.py check` and `coverage` (scope: `docs/port/scope.toml`) must stay at
  0 errors / 100 %. Generate a marker with `python3 scripts/port/provenance.py marker PATH:A-B`.
  When a unit splits a stub's marker (one per Lua function, or per block of ~50 specialModList
  entries), the union must still cover the range exactly once.
- **Stubs.** An unported body is `unported!("<path>:<a>-<b>")` (D9): it counts the hit
  (`pob_calc::stub::stub_report()`) and returns `Default`. In statement position write
  `let () = unported!(…);`. A unit is done when no `unported!` remains in its files (§10.2).
- **`U0-SKELETON`.** A few stubs contain a minimal body so the pipeline runs end to end on stubs
  (`init_env_base::seg` allocates the env; `buildOutput` calls initEnv + perform;
  `createActiveSkill`/`copyActiveSkill`/`getMiscCalculator` return placeholder values). The owning
  unit replaces each with the faithful port.
- **Prelude.** Calc modules start with `use crate::prelude::*;` (pob-calc) or
  `use crate::types::*;` (pob-modparser).
- **Ctx structs.** Declared with `crate::ctx_struct!` (fields = Lua local names, in §7.1 order);
  `ProbeState::locals()` lists them for the oracle (ORC-X). A segment receives `&mut Ctx`, assigns
  fields where Lua assigns the local, and returns `Flow`.
- **Registries.** Closures stored in data are rows `FnRow { id, hash, f }` in the owning unit's
  file (`ROWS`); the `registry_complete` test (crates/pob-oracle/tests/U0.rs) requires every
  closures.json id of the family, once, with the dump's sha256.
- **Data.** `pob_gamedata::data()` is the immutable `data`; typed roots are strict schema structs
  (`deny_unknown_fields`); untyped roots are in `data().other` as `DataValue`. A unit that needs an
  untyped root typed files a CCR (new schema struct + compile check).
