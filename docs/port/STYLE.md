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
| `#t` | `t.len()` (border; hole-free contract) |
| `for i, v in ipairs(t)` | `for_ipairs!((i, v) in t => { ... })`. It reads `t[i]` live on every step and stops at the first nil, exactly like Lua. |
| `for k, v in pairs(t)` | `for (k, v) in t.pairs() { ... }`. Keys are a snapshot; values are read live; keys whose live value is nil are skipped. Add `// LUA-ORDER` when the result can depend on order (§3.4). |
| `t_insert(t, v)` / `t_insert(t, i, v)` / `t_remove(t, i)` | `t.push(v)` / `t.insert(i, v)` / `t.remove(Some(i))` |
| `wipeTable(t)` / `copyTable(t)` / `copyTable(t, true)` | `t.wipe()` / `t.copyTable(false)` / `t.copyTable(true)`. Both keep the metatable (Common.lua:496). |
| `goto continue` | `continue` |
| `return` at the top level of a split function | the segment returns `Flow::Return` |
| `return calcs.initEnv(build, mode, override, specEnv)` | the segment returns `Flow::Restart` (§7.2) |
| `error(...)`, or an arithmetic, index or call on nil | `lua_error!("...")` |
| a read of an undeclared global (`dotCfg` at CalcOffence:3527) | literal `nil` / `None`, marked `// LUA-GLOBAL-NIL` |
| a string passed where a cfg is expected (`skillModList:More("MORE", cfg, "Accuracy")`, CalcOffence.lua:2194-2195) | `Some(&ModCfg::STRING_CFG)` (all fields nil; the table that ends up in the name list matches nothing), marked `// LUA-QUIRK` |

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
- **Flags.** Flag bit operations use `u32` (the largest flag is `MatchAll = 0x40000000`, so bit 31 is unused). `bnot` only ever appears inside `band` masks; U0's grep test asserts this.
- **Number formatting.** `tostring(number)` is `%.14g` with LuaJIT spellings (`inf`, `-inf`, `nan`/`-nan`, `-0`). `string.format` is a port of `lj_strfmt` (§4.1).
- **Division.** Division by zero yields IEEE inf/NaN. Never clamp or guard where Lua does not.
- **`m_modf` truncates toward zero.** In `subMod.value = m_modf(round(x, 2))` (ModStore.lua:84) only the first return value is used.
- **`ModStore:Max` drops non-positive values.** It returns nil when every value is ≤ 0, because it starts from `val > (max or 0)` (ModStore.lua:248-257). Port literally.
- **Strings are byte strings with ASCII case mapping** (PoB is ASCII). Lua patterns go through `pob_lua::pattern` (the lstrlib port).

### 3.6 Mutation of mod tables

`ModRef = Arc<Mod>` is immutable once a store holds it. U0's audit (`docs/port/contracts.md#mod-mutation`) lists every Lua site that mutates a mod in place: ModStore.lua 425-430/514-516/564-566, CalcPerform.lua 2452/2539/2564/2610/2718/2846/2921/3881/3897, CalcActiveSkill.lua:38, Item.lua 2336/2558, ModTools.lua 240-242. Each site has been verified to mutate a **fresh copy**: `copyTable` immediately before the mutation, a `copyTable(buffExports.Aura[...])` (CalcPerform.lua:3870), or a mod under construction. Rust ports these as "build a new `Mod`, then insert". A newly found site whose target is shared MUST be filed as a CCR before porting.



## Amendments and U0 conventions

### Accepted rule changes (CCRs)

- **CCR-0001 (LuaJIT build profile) / CCR-0002 (dual-number ±0).** PoB's reference is x86-64
  single-number LuaJIT; the oracle machine is ARM64 dual-number. `pob_lua::{LJ_TARGET_X86ORX64,
  LJ_DUALNUM}` mirror the target's profile; a ±0 difference against an ARM64 oracle is a host
  artefact (record it in `known_diffs.toml` with the CCR number), never a port bug. `lpow` is libm
  `pow` (no `lj_vm_powi`); LuaJIT never prints a NaN sign.
- **CCR-0004 (undeclared globals).** The `LUA-GLOBAL-NIL` sites are §7.3's list plus the nine sites
  of CCR-0004 (`cfg` CalcOffence 2506/2507, `skillCfg` CalcPerform 915 and CalcBreakdown 212/232,
  `aura` PartyTab 919, `cfg` act_dex 12991, `scriptPath` DataLegionLookUpTableHelper 136/144).
  `scripts/port/globals_lint.py --check` must stay green.
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
