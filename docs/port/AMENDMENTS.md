# Amendments to the port architecture docs (binding, read first)

`calc-architecture.md` and `ui-architecture.md` were written in parallel with the
Wave-0 foundation work. This file reconciles them with what was actually built.
Where this file and either doc disagree, **this file wins**.

## A1. Crate names as built

| Doc says | Reality | Rule |
|---|---|---|
| `pob-luahost` (old `pob-calc` renamed: jit.rs, headless.rs, worker.rs, lua/) | **`crates/pob-oracle`** holds all of it (moved by commit 46ef7e3) plus the oracle harness (`pob_oracle outputs\|stages\|all`, `src/oracle.rs`, `src/oracle.lua`). | Read every `pob-luahost` as `pob-oracle`. Unit **REN is done**. The legacy app (`pob-ui`) imports the worker from `pob_oracle`. |
| `pob-calc` = new calc crate | `crates/pob-calc` already exists and is Lua-free: `flags.rs`, `modifier.rs`, `modifier_serde.rs`, `mod_store.rs`, `lua_pattern.rs`. | calc:U0 turns it into the new calc crate: `modifier.rs`/`modifier_serde.rs` move to `pob-gamedata`, `lua_pattern.rs` moves to `pob-lua/src/pattern.rs`, `flags.rs` is replaced by generated enums, `mod_store.rs` is kept as `classes/legacy_mod_store_reference.rs` (reference only, deleted by MS-B). **Done by U0, with legacy copies kept (CCR-0011 §11):** the old files also stay in pob-oracle (`crates/pob-oracle/src/legacy_modifier.rs`, `legacy_modifier_serde.rs`, `src/lua/lua_pattern.rs`), because the oracle still reads and writes mods in the legacy JSON shape. Until ORC-X there are therefore two `Mod` types (`pob_oracle::legacy_modifier::Mod`, `pob_gamedata::Mod`) and two pattern engines (pob-oracle's, `pob_lua::pattern`). **ORC-X** owns the legacy → `pob_gamedata::Mod` conversion and then deletes the legacy copies. The calc crates (pob-gamedata, pob-lua, pob-calc, pob-modparser) never use the legacy types. |
| `pob-extract dump` / `pob-extract/src/dump/*` / `pob-extract compile` | **`crates/pob-dump`** (binary `pob-dump`) does the dump. | P1's remaining work and U0's `compile.rs` go into `crates/pob-dump/src/`. `pob-extract` stays legacy. |

## A2. Dump format

The binding encoding is `crates/pob-dump/FORMAT.md` (`$seq`, `$num`, `$keys`,
`$meta`, `$ref`, `$fn`, `$float`, `$bytes`; closure hash = sha256 of the source
lines), **not** the table in calc doc §9.1. Output lives in `data/dump/`
(gitignored, regenerate with `cargo run -p pob-dump --release -- --pob .PathOfBuilding/src --out data/dump`).
Already produced: every `data.<key>`, misc, global, gameVersions, calcSections,
configOptions, modCache, statDescriptions, files/*, all 41 tree versions + sprites,
closures.json (733 fns: 133 skill hooks, 532 config apply incl. the shared
`mapAffixDropDownFunction`, 41 modmap apply, …; ids such as `ConfigOptions[<i>].apply`,
sha256 hashes), manifest.json, `parse_oracle.jsonl` (the parse oracle runs with `jit.off()`
and doubles from `tonumber`, so its `-0`s are x64 PoB's; CCR-0002). Generated code:
`crates/pob-dump/generated/{enums,global,misc}.rs` (SkillType Area=8, Aura=39, Pact=135;
OR/AND/NOT = 73/74/75 are SkillType entries), guarded by `pob-dump --codegen --check`;
pob-gamedata includes `global.rs`/`misc.rs` by path from `src/generated/mod.rs` (CCR-0003).
`data.bin` is written by `pob-dump --compile` (U0, `crates/pob-dump/src/compile.rs`).
Gem count is **871** (822 + 49 Vaal AltX/AltY synthesised by Data.lua).

## A3. Oracle as built (P3 starting point)

`crates/pob-oracle`: `pob_oracle outputs|stages|all <build.xml> [--json OUT]` (one
fresh LuaJIT process per build; full typed output incl. nested tables; stage dumps
after `initEnv`: player/enemy/item modDBs as `pob_calc::Mod` JSON, per-skill
types/flags/data/cfg/modList, per-socket-group-skill outputs). Rust API
`Oracle::outputs/stages`. `scripts/parity/oracle_dump.sh` fills
`.parity/oracle/<stem>/{outputs,stages}.json` for all 57 builds (53 s).
`scripts/parity/diff.py` compares output trees. P3 extended this crate with the
patcher, line probes (`stages.toml`), write provenance and the nondeterminism runs
(format details: calc doc §9.2/§9.5 as amended by CCR-0010, `crates/pob-oracle/README.md`).
Every oracle parity run uses the LuaJIT **interpreter** (`JitPob::boot_oracle`, `jit.off()`;
A6) and records `"luajit": {arch, jit, numberMode}` in `outputs.json`/`stages.json`.
`.parity/oracle` dumps made before CCR1 ran with the JIT on and lack that header; regenerate
them (`scripts/parity/oracle_dump.sh`) before relying on a `±0` or on `string.sub`-derived values.
Known: PoB leaks state between builds in one process (corpus 012 CombinedDPS
381,673 after 000-011 vs 537,431 alone), so always one process per build.

## A4. `pob-sg` as built (UI Wave-1 starting point)

`crates/pob-sg` exists (commit 6e883cb): `sg.rs` (Sg API, command recording,
layers, viewport, colour incl. escape leak, DPI), `font.rs` + `tga.rs` (TGF/TGA,
`r_font.cpp` port: even heights, nearest-atlas, CENTER/RIGHT vs screen width,
cursor index 1-based), `color.rs` (palette + colorCodes), `images.rs` (handles,
png/jpg/webp/tga, async threads), `gpu/` (wgpu 30.0.1, gamma-space, WebGL2 compiles),
`host.rs` (winit 0.30.13, PoB key names, wheel down+up, double click), `bin/sg-demo.rs`
(`--screenshot`). 34 tests. Ported from SimpleGraphic **master 8694a07**.

ui:U0 restructures this into the module layout of ui doc §3 **by moving and
adapting the existing code, not rewriting it**: `gpu/` → `crates/pob-sg-wgpu`,
`host.rs` → `crates/pob-host`, the rest split into `types/api/frame/color/font/*/image/assets/sys/trace` (+`raster` feature).
Wave-1 units U1/U2/U3/U6/U7 then start from that code and close the deltas in
ui doc §2 (pin `3b1a346`; `ImageSize` blocks; non-ASCII → `?`; extra OnChar;
swallowed F10/Shift+Pause; trace recorder; CPU raster).

ui:U0 is done (CCR-0013 applied to the ui doc). Behaviour changes of the moved code, with the
moved tests adapted rather than dropped: `SetDrawLayer(nil)` and `DrawImage` with an unloaded
handle raise Lua errors (SG behaviour; the pre-port recorder ignored or skipped them);
`sync_load_and_dedup` no longer asserts a texture release on `Unload` (ui doc §13 item 10);
mip generation is still the moved sRGB-correct filter (ui doc §4.5 asks for a gamma box filter;
U3 switches it).

## A5. Process (both plans)

- **Worktree base.** Harness worktrees start on an old commit. Every agent's first
  command is `git checkout -B port/<unit-id> rust-port` (rust-port is the
  integration branch in the main checkout; it moves as units merge).
- **Build env.** `export CARGO_BUILD_JOBS=3 RUSTC_WRAPPER=$HOME/.cargo/bin/sccache`
  (sccache is installed there; it is not on PATH). Per-worktree target dir (the default).
  Use `scripts/port/cargo.sh` / `scripts/port/oracle.sh` (P4; they flock one compile and
  one LuaJIT oracle machine-wide and export the env above).
- **Gitignored inputs** are absent from worktrees; use absolute paths into the main
  checkout `/home/ferm/Work/projects/PathOfBuildingMK2/{.PathOfBuilding,data,.parity}`
  (read-only unless the unit owns an output there).
- **Concurrency.** At most 3 porting agents at once across both plans (RAM).
- **Merging.** The integrator merges `port/<unit>` branches into `rust-port`
  serially, resolving `Cargo.toml`/`Cargo.lock` conflicts by union.
- **Wine** (real-SimpleGraphic checks, T6) is not installed; T6 gates are pending,
  not blocking.

## A6. LuaJIT build profile and oracle mode (CCR-0001, CCR-0002; orchestrator decision)

- **The shipped Rust product always follows x64 PoB semantics** (single-number LuaJIT,
  `cvttsd2si` casts) regardless of host architecture or target. `pob_lua::LJ_TARGET_X86ORX64`
  and `LJ_DUALNUM` are constants (`true`/`false`). The ARM64 dual-number behaviour (saturating
  casts, int32 `tonumber` zeros) lives in `crates/pob-lua/src/profile.rs` behind the test-only
  feature `oracle-dualnum` (enabled solely by pob-lua's own dev-dependency on itself; no shipping
  crate may enable it; `crates/pob-lua/tests/oracle_dualnum_feature.rs` fails if any other workspace
  manifest names it) as `with_luajit_arm64_profile`.
- **L0** compares the shipped profile with the LuaJIT interpreter (`jit.off`). On a dual-number
  host (this machine is aarch64), a mismatch is tolerated and counted only when the ARM64
  profile reproduces the oracle exactly; any other mismatch fails. `L0_JIT_ON=1` adds an
  informational `jit.on` run.
- **All oracle parity runs use the LuaJIT interpreter** (`jit.off()`): `pob_oracle`
  (`outputs|stages|all`, probes, nondet) boots through `JitPob::boot_oracle`; pob-dump's parse
  oracle already did. `POB_ORACLE_JIT=on` is for diagnostics only. The `pob_timing` baseline
  (gate G1a) and the legacy app's worker keep the JIT on (they are not parity runs).
- **Diffs tolerate a `±0` sign mismatch as a host artefact** (numeric tolerance); values derived
  from the sign (`1/x`, a formatted `"-0"`) go to `known_diffs.toml` citing CCR-0002.
- Smaller §4.1 corrections accepted: `lpow` = libm `pow` (no `lj_vm_powi`), LuaJIT never prints
  a NaN sign (`nan`), the `cat!` memo is keyed on content, `From<&LStr> for FmtArg`,
  `string::sub -> Cow<str>`.

## A7. Contract change requests applied by CCR1 (2026-10-09)

| CCR | Applied to | Notes |
|---|---|---|
| 0001 luajit-build-profile | calc doc §3.5, §4.1, §9.2, §9.4; A6 | pob-lua profile moved behind `oracle-dualnum`; pob-oracle `boot_oracle` |
| 0002 dualnum negative zero | calc doc §3.5, §9.2, §9.5; A6 | ±0 tolerated in diffs; dumps record `luajit.numberMode` |
| 0003 p1 generated global shape (was the duplicate `0001-p1-…`) | calc doc §2.1, §4.2, §9.1, §13; A2 | SkillType operator numbers; generated files stay in `crates/pob-dump/generated/`, included by pob-gamedata |
| 0004 undeclared global sites | calc doc §7.3, §12 | `globals_lint.py --check` parses the doc text; `CCR_0004_SITES` dropped |
| 0005 ctx params rule | calc doc §7.1 | |
| 0006 lua2rs modparser shapes | calc doc §4.3, §4.4, §6, §8.1 | every type-level item was already in U0's frozen code; the `Caps` doc comment now names the three call shapes |
| 0010 pob-state format | calc doc §9.2, §9.5 | |
| 0011 U0 contract corrections | calc doc §3.2, §4.3, §4.4, §5.1-§5.3, §5.10, §6, §7.2, §9.2, §11.1 (ORC-X row); A1 | docs follow the frozen code; A1 records the legacy `Mod`/pattern copies in pob-oracle and ORC-X's ownership of their conversion and deletion (§11) |
| 0012 slice5 corpus | calc doc §9.3 | `slice5_allflame_exp_046` (no weapon) replaced by `slice5_allflame_exp_048` (Flicker Strike, Claw + Shield) |
| 0013 UI-U0 contract notes | ui doc §4.2, §4.5, §4.6, §5.2, §6.4, §6.8, §8, §9.2-§9.4, §13; A4 | docs follow frozen code. **Open:** A2 `ImageHandle::unload` signature is U3's call; A6 Net completions is U7's call |
