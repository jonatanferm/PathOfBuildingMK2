# Amendments to the port architecture docs (binding, read first)

`calc-architecture.md` and `ui-architecture.md` were written in parallel with the
Wave-0 foundation work. This file reconciles them with what was actually built.
Where this file and either doc disagree, **this file wins**.

## A1. Crate names as built

| Doc says | Reality | Rule |
|---|---|---|
| `pob-luahost` (old `pob-calc` renamed: jit.rs, headless.rs, worker.rs, lua/) | **`crates/pob-oracle`** holds all of it (moved by commit 46ef7e3) plus the oracle harness (`pob_oracle outputs\|stages\|all`, `src/oracle.rs`, `src/oracle.lua`). | Read every `pob-luahost` as `pob-oracle`. Unit **REN is done**. The legacy app (`pob-ui`) imports the worker from `pob_oracle`. |
| `pob-calc` = new calc crate | `crates/pob-calc` already exists and is Lua-free: `flags.rs`, `modifier.rs`, `modifier_serde.rs`, `mod_store.rs`, `lua_pattern.rs`. | calc:U0 turns it into the new calc crate: `modifier.rs`/`modifier_serde.rs` move to `pob-gamedata`, `lua_pattern.rs` moves to `pob-lua/src/pattern.rs`, `flags.rs` is replaced by generated enums, `mod_store.rs` is kept as `classes/legacy_mod_store_reference.rs` (reference only, deleted by MS-B) . |
| `pob-extract dump` / `pob-extract/src/dump/*` / `pob-extract compile` | **`crates/pob-dump`** (binary `pob-dump`) does the dump. | P1's remaining work and U0's `compile.rs` go into `crates/pob-dump/src/`. `pob-extract` stays legacy. |

## A2. Dump format

The binding encoding is `crates/pob-dump/FORMAT.md` (`$seq`, `$num`, `$keys`,
`$meta`, `$ref`, `$fn`, `$float`, `$bytes`; closure hash = sha256 of the source
lines), **not** the table in calc doc §9.1. Output lives in `data/dump/`
(gitignored, regenerate with `cargo run -p pob-dump --release -- --pob .PathOfBuilding/src --out data/dump`).
Already produced: every `data.<key>`, misc, global, gameVersions, calcSections,
configOptions, modCache, statDescriptions, files/*, all 41 tree versions + sprites,
closures.json (733 fns: 133 skill hooks, 532 config apply incl. the shared
`mapAffixDropDownFunction`, 41 modmap apply, …), manifest.json. Generated enums:
`crates/pob-dump/generated/enums.rs` (SkillType Area=8, Aura=39, Pact=135).
Gem count is **871** (822 + 49 Vaal AltX/AltY synthesised by Data.lua).

Still missing from P1: `parse_oracle.jsonl`, `--codegen --check`, generated
`pob-gamedata/src/generated/*`, binary bundle (U0 `compile`).

## A3. Oracle as built (P3 starting point)

`crates/pob-oracle`: `pob_oracle outputs|stages|all <build.xml> [--json OUT]` (one
fresh LuaJIT process per build; full typed output incl. nested tables; stage dumps
after `initEnv`: player/enemy/item modDBs as `pob_calc::Mod` JSON, per-skill
types/flags/data/cfg/modList, per-socket-group-skill outputs). Rust API
`Oracle::outputs/stages`. `scripts/parity/oracle_dump.sh` fills
`.parity/oracle/<stem>/{outputs,stages}.json` for all 57 builds (53 s).
`scripts/parity/diff.py` compares output trees. P3 extends this crate with the
patcher, line probes (`stages.toml`), write provenance and the nondeterminism runs.
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

## A5. Process (both plans)

- **Worktree base.** Harness worktrees start on an old commit. Every agent's first
  command is `git checkout -B port/<unit-id> rust-port` (rust-port is the
  integration branch in the main checkout; it moves as units merge).
- **Build env.** `export CARGO_BUILD_JOBS=3 RUSTC_WRAPPER=$HOME/.cargo/bin/sccache`
  (sccache is installed there; it is not on PATH). Per-worktree target dir (the default).
  Until P4 lands `scripts/port/cargo.sh`/`oracle.sh`, call cargo directly.
- **Gitignored inputs** are absent from worktrees; use absolute paths into the main
  checkout `/home/ferm/Work/projects/PathOfBuildingMK2/{.PathOfBuilding,data,.parity}`
  (read-only unless the unit owns an output there).
- **Concurrency.** At most 3 porting agents at once across both plans (RAM).
- **Merging.** The integrator merges `port/<unit>` branches into `rust-port`
  serially, resolving `Cargo.toml`/`Cargo.lock` conflicts by union.
- **Wine** (real-SimpleGraphic checks, T6) is not installed; T6 gates are pending,
  not blocking.
