# Amendments to the port architecture docs (binding, read first)

`calc-architecture.md` and `ui-architecture.md` were written in parallel with the
Wave-0 foundation work. This file reconciles them with what was actually built.
Where this file and either doc disagree, **this file wins**.

## A1. Crate names as built

| Doc says | Reality | Rule |
|---|---|---|
| `pob-luahost` (old `pob-calc` renamed: jit.rs, headless.rs, worker.rs, lua/) | **`crates/pob-oracle`** holds all of it (moved by commit 46ef7e3) plus the oracle harness (`pob_oracle outputs\|stages\|all`, `src/oracle.rs`, `src/oracle.lua`). | Read every `pob-luahost` as `pob-oracle`. Unit **REN is done**. The legacy app (`pob-ui`) imports the worker from `pob_oracle`. |
| `pob-calc` = new calc crate | `crates/pob-calc` already exists and is Lua-free: `flags.rs`, `modifier.rs`, `modifier_serde.rs`, `mod_store.rs`, `lua_pattern.rs`. | calc:U0 turns it into the new calc crate: `modifier.rs`/`modifier_serde.rs` move to `pob-gamedata`, `lua_pattern.rs` moves to `pob-lua/src/pattern.rs`, `flags.rs` is replaced by generated enums, `mod_store.rs` is kept as `classes/legacy_mod_store_reference.rs` (reference only, deleted by MS-B). **Done by U0, with legacy copies kept (CCR-0011 §11):** the old files also stay in pob-oracle (`crates/pob-oracle/src/legacy_modifier.rs`, `legacy_modifier_serde.rs`, `src/lua/lua_pattern.rs`), because the oracle still reads and writes mods in the legacy JSON shape. Until ORC-X there are therefore two `Mod` types (`pob_oracle::legacy_modifier::Mod`, `pob_gamedata::Mod`) and two pattern engines (pob-oracle's, `pob_lua::pattern`). **ORC-X** owns the legacy → `pob_gamedata::Mod` conversion and then deletes the legacy copies. **Done by ORC-X:** the oracle reads and writes mods as `pob_gamedata::Mod` (`pob_oracle::mod_json`, `pob_calc::probe::mod_json`) and uses `pob_lua::pattern`; the three legacy files are deleted, so there is one `Mod` type and one pattern engine. The calc crates (pob-gamedata, pob-lua, pob-calc, pob-modparser) never use the legacy types. |
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
mip generation is SG's `stbir_resize_uint8_srgb_edgemode` chain, ported by U3 (CCR-0023 B; the
gamma box filter §4.5 used to ask for was wrong).

## A5. Process (both plans)

- **Worktree base.** Harness worktrees start on an old commit. Every agent's first
  command is `git checkout -B port/<unit-id> rust-port` (rust-port is the
  integration branch in the main checkout; it moves as units merge).
- **Build env.** `export CARGO_BUILD_JOBS=3 RUSTC_WRAPPER=$HOME/.cargo/bin/sccache`
  (sccache is installed there; it is not on PATH). Per-worktree target dir (the default).
  Use `scripts/port/cargo.sh` / `scripts/port/oracle.sh` (P4; they flock one compile and
  at most `$POB_ORACLE_SLOTS` LuaJIT oracles machine-wide, default 1, CCR-0127, and export the env above).
- **Gitignored inputs** are absent from worktrees; use absolute paths into the main
  checkout `/home/ferm/Work/projects/PathOfBuildingMK2/{.PathOfBuilding,data,.parity}`
  (read-only unless the unit owns an output there).
- **Concurrency.** At most 3 porting agents at once across both plans (RAM).
- **Merging.** The integrator merges `port/<unit>` branches into `rust-port`
  serially, resolving `Cargo.toml`/`Cargo.lock` conflicts by union.
- **Wine** (real-SimpleGraphic checks, T6) is not installed; T6 gates are pending,
  not blocking.
- **Test inputs** (CCR-0031 B). Tests find the PoB checkout, `data/`, `data/dump` and `.parity` only
  through the dev-dependency `pob-testpaths` and call `pob_testpaths::require`: a missing input fails
  the test unless `POB_ALLOW_SKIP=1` (search order: `POB_SRC`/`POB_ROOT`/`POB_DATA_DIR`/`POB_DUMP_DIR`/
  `POB_PARITY_DIR`, this workspace and its ancestors, the linked worktree's main checkout, then
  `/home/ferm/Work/projects/PathOfBuildingMK2`). `POB_ORACLE_REQUIRE` is retired (21 oracle tests still read it, 20 to skip a missing input and DF-F as a strictness flag: A8 needs-code 16). Opt-in tests
  (`POB_ORACLE_PROBE=1`) and hardware checks (no GPU adapter) are not input skips.

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

## A8. CCRs applied by CCR2 (2026-11-10)

CCR2 processed every CCR whose Status was not `applied` (0014-0090). **Renumbering** (every
reference in `crates/`, `docs/`, `scripts/` updated, code comments included): the three 0014s keep
MS-A at 0014 and move CT to **0091** and MP0 to **0092**; the three 0034s keep U4 at 0034 and move
PF-D to **0093** and PF-E to **0094** (by merge order on rust-port). 0036 (OF-G) and 0065 (OF-D)
describe the same `passList` injection gap: 0065 is superseded by 0036, and the tests' comments now
cite 0036. Stale references left by earlier renumbers were fixed too (`CCR-0038` → 0055 in
`dot_combined.rs`, `CCR-0039` → 0075 in `tests/OF-H.rs`). 0089 is used by HYG3's in-flight branch
and was left free. Three in-flight units hold CCR files numbered **0091**, which CT's CCR now
holds on rust-port: PF-B (`0091-pf-b-minion-fields-and-fn-probes.md` on `port/PF-B`; it was 0034
earlier), U16 (`0096-u16-main-mode-seams.md`, `port/U16` worktree) and CA (`0091-ca-calcs-seams.md`,
`port/CA` worktree). Each must take the next free number (0095 or later, one per unit in merge
order) when it merges, and update its markers to match.

Decisions: **applied** = the binding text (calc doc, ui doc, contracts.md, STYLE.md, this file)
now follows what was built and merged; **needs code** = a shared change that units worked around
locally, listed below for **ORC-X2**; **escalated** = changes a frozen public type or signature in
a way that conflicts with other (merged or in-flight) units, left to the orchestrator.

| CCR | Title | Decision | Applied to / notes |
|---|---|---|---|
| 0014 | MS-A ModStore contract notes | applied §2 (divergence accepted; its `known_diffs.toml` entry is item 17), §4, §5, §3 (mutation site); needs code §1, §3 (typed `Tag.warcryPowerBonus`, optional: `Tag.extra` round-trips it) | calc doc §5.3 (`MergeMod`), §11.1 MS-A row; contracts#modstore, #mod-mutation |
| 0015 | MS-B EvalMod notes | applied §1, §2 (retracted), §4, §5; needs code §6 (G1a fails), `known_diffs.toml` entries for §1 and §3 (item 17); escalated §3 | calc doc §5.3 (`getActor`); ui doc §13 item 14; contracts#modstore |
| 0019 | BM-B tab model contracts | applied §1, §3, §8 (rule; its readers need code, item 11), §9, §11, §12; §10 superseded by CCR-0080 (`Init_model`); needs code §2, §4-§7 (model fields) | calc doc §4.4 table, §11.1 BM-B row; ui doc §10.2; contracts#model-census, #class-census |
| 0020 | LUT signatures, cache, file access | applied; §7 superseded by CCR-0052 §4; §8 done by PS-B (CCR-0070 §4) | calc doc §4.4 table |
| 0021 | U1 SG semantics at 3b1a346 | applied | ui doc §2 (source, Viewport, DrawImage(Quad), Key names, Swallowed keys, Shader, Frame loop), §13 item 1 |
| 0022 | U2 font notes | applied | ui doc §2 Glyph advance, §4.4 |
| 0023 | U3 images at 3b1a346 | applied | ui doc §2 (Textures, Missing file, ImageSize), §4.5, §13 item 10; A4 |
| 0024 | PT PassiveTree notes | applied §1-§4, §6, §8, §9; needs code §5 (dump `tree.nodes` source order + `tree_node_order`), §7 (dump `buildTreeDependentUniques`) | calc doc §3.4, §4.4 table; contracts#class-census |
| 0025 | IT-A ParseRaw notes | applied §2, §4, §5, §6; needs code §1 (`MainOptions`), §3 (shared replay) | calc doc §11.1 IT-A row; contracts#class-census, #mod-mutation |
| 0026 | U7 host contract notes | applied A, B, D, E, F, G; needs code C (`HttpRequest` proxy/protocol/noSSL) | ui doc §2, §4.6, §5.2, §13 item 11 |
| 0028 | U8 ui-oracle notes | applied 1, 3 (FileWrite out of T3 until U16), 5-9, 11, 12; needs code 2, 4, 10 (switch-over recipe), 13 | ui doc §8 rule 6, §11.2 |
| 0030 | ORC-X frozen types that cannot hold PoB values | escalated (needs code); **applied by RETYPE (A10)** | calc doc §9.2 (absent-mark note); see E1 |
| 0031 | HYG1 `#t`, constructors, test inputs | applied | calc doc §3.2, §3.4 (and STYLE.md §3); A5 |
| 0032 | IT-B Item core stubs, `Item.extra` shape | applied | contracts#class-census (signatures already in `item.rs`/`item_tools.rs`) |
| 0033 | U5 Lua-helper audit | applied §5, §6 (owners); needs code §1-§4 | ui doc §8 rule 8, §13 item 15 |
| 0034 | U4 ui-core framework notes | applied | ui doc §6.5, §6.7, §6.12, §6.13, §13 item 12 |
| 0035 | OF-G ailment closure signatures | applied | calc doc §7.2 |
| 0036 | OF-G `passList` injection | needs code | calc doc §9.2 |
| 0037 | CO-A `varControls` never populated | applied (resolved by `ConfigTab::ctor_controls_model`) | calc doc §11.1 BM-B row; contracts#model-census |
| 0038 | CO-B control methods | needs code (model methods in `config_tab.rs`) | contracts#model-census |
| 0039 | MP7 jewel func contract | applied; needs code: the `JewelFuncSpec.key` doc comment still says `"$default"` = ModParser.lua (comment only) | calc doc §4.3, §6; contracts#mod-mutation |
| 0040 | AS contract gaps | applied §5, §6; needs code §3, §4, §7, §8; escalated §1, §2 (with CCR-0030) | contracts#model-census |
| 0041 | OF-A helper signatures, hook replay | applied §2, §3; needs code §1, §4, §5 | calc doc §7.2, §9.2 |
| 0043 | OF-B `calcRadiusBreakpoints` nils | needs code (same change as CCR-0041 §1) | calc doc §7.2 |
| 0044 | OF-B mid-segment callee replay | needs code | calc doc §9.2 |
| 0046 | SK-A `calcSkillDuration` with `{}` env | escalated (needs code); **applied by RETYPE (A10)** | E5 |
| 0049 | OF-H ailment body B integration | needs code (§1 OF-H's private CCR-0035 copies, §2, §3 `isValueInArray`, §4) | calc doc §9.2 |
| 0052 | PS-A PassiveSpec notes | applied §1, §2, §7, §8 (owner: PS-A's `main_LoadTree`), §10-§12; needs code §3-§6, §9 | contracts#class-census |
| 0055 | OF-C `combineStat` vararg | applied §1; needs code §2 | calc doc §7.2, §9.2 |
| 0061 | IT-C item mod lists | applied §5, §7; needs code §1 (CCR-0030 #4), §2, §3, §4, §6, §8 | calc doc §9.4 L2; contracts#class-census |
| 0063 | DF-C nil-source `NewMod` | needs code (`source: impl Into<Option<&str>>`, existing callers compile) | contracts#modstore |
| 0064 | OF-I foreign mirage output, deps | applied §2; needs code §1 | calc doc §11.1 OF-I row, §9.2 |
| 0065 | OF-D `passList` injection | superseded by CCR-0036 | — |
| 0067 | DF-E CalcDefence module locals | needs code (§1-§2: DF-A landed, DF-E's private copies remain; §3) | calc doc §9.2 |
| 0068 | DF-D CalcDefence locals | withdrawn (by DF-D, before CCR2) | — |
| 0069 | U9 basic controls surface | applied B; needs code A (`MainCore` field) | ui doc §6.11, §13 item 12 |
| 0070 | PS-B depends/paths | applied | contracts#class-census |
| 0072 | PS-C cluster subgraphs | applied §3, §5, §7, §8; needs code §1, §2, §4, §6 | contracts#class-census |
| 0073 | U10 Tooltip contract | applied B, C, E; needs code A, D (switch to `crate::wrap::wrap_string`, landed by U14) | ui doc §6.9, §13 item 12 |
| 0074 | CS0 CalcSetup helper signatures | applied §1-§5, §7, §8; needs code §6 (`SkillEffect.req`, frozen field) | calc doc §5.10; contracts#mod-mutation, #model-census |
| 0075 | OF-F `globalOutput` in L4 runs | needs code | calc doc §9.2 |
| 0076 | PF-F `notBuff` on a ModList | escalated with CCR-0030 row 7 (0076's diff is the concrete one); **applied by RETYPE (A10)** | E1 |
| 0077 | PF-G callee replay, mirages, pools, absent marks | applied §2 (by MI, CCR-0090), §7 (resolved by HYG2's `u0_smoke` rewrite); needs code §1, §3 (confirm with PF-B, in flight), §4-§6 | calc doc §8.1, §9.2 |
| 0078 | U12 list hooks frozen | applied A, C (A was already in the ui doc); needs code B (`Base.other_drag_source/targeting`) | ui doc §6.8, §13 item 12 |
| 0079 | PF-C setup/flasks gaps | applied §6 (superseded by CCR-0085 §1), §7 (= CCR-0030 #5); needs code §2-§5, §8; escalated §1 | calc doc §9.2; E4 |
| 0080 | BM-A build model load/save | applied §1, §5-§8, §10; needs code §2-§4, §9 | calc doc §11.1 BM-A row; ui doc §10.2; contracts#model-census |
| 0081 | U11 EditControl surface | applied §1, §2; needs code §3, §4 | ui doc §6.8, §13 item 12 |
| 0082 | CS1 init env base notes | applied §1 (resolved by HYG2's `u0_smoke` rewrite), §2, §4; needs code §3 | calc doc §5.4 (`Override`), §9.2 |
| 0083 | CS3 init env skills | applied §4 (extra keys), §6, §7; needs code §1 (CT still reads the data flag), §3, §4 (typed fields), §5; §2 = CCR-0030 #3 | calc doc §4.4 table; contracts#model-census, #mod-mutation |
| 0084 | U14 popups surface | applied §1, §2, §5, §6; needs code §3, §4 | ui doc §6.10, §13 item 16 |
| 0085 | PF-A perform helpers | applied §3, §6; needs code §1, §2, §5; §4 = CCR-0030 #6 | contracts#mod-mutation, #model-census |
| 0086 | U13 dropdown hooks frozen | applied (§6.8 text was already in the ui doc) | ui doc §6.8, §13 item 12 |
| 0087 | TR-A `TriggerConfig` | escalated (needs code); **applied by RETYPE (A10)** | calc doc §8.1 note; E2 |
| 0088 | U15 inject bound to live objects | needs code | — |
| 0090 | MI mirages signatures and replay | applied §1, §2; needs code §1 follow-up (PF-G call site), §3, §4 | calc doc §8.1 |
| 0091 | CT calc tools (was 0014) | applied §1-§4, §6 (§3's CS3 reader needs code, item 11; §4's `known_diffs.toml` entry is item 17); needs code §5 (`scope.toml` Common.lua 803-828; one `formatNumSep`) | calc doc §3.4, §4.4, §5.5; contracts#mod-mutation |
| 0092 | MP0 scan and parser tables (was 0014) | applied | calc doc §6; contracts#mod-mutation |
| 0093 | PF-D callee replay and export gaps (was 0034) | applied §4, §5 (resolved by HYG2); needs code §1-§3 | calc doc §9.2; contracts#mod-mutation |
| 0094 | PF-E buff loop gaps (was 0034) | applied §1, §2 (mutation sites); §3 = CCR-0030 #7; §4 superseded by CCR-0085 §1; needs code §1 (Tag field, with CCR-0014 §3), §5 (= CCR-0063) | contracts#mod-mutation, #modstore |

### Needs code (for ORC-X2)

Each item names the CCRs, the files to change, and the local workarounds to delete afterwards.

1. **Shared callee-replay seam.** One `callee!`/`set_callee_hook` in `crates/pob-calc/src/probe.rs`
   (feature `probe`), `pob_oracle::testkit::segment_diff_with_replay`, `[[function]]` probes for
   OF-A's local helpers in `docs/port/stages.toml`, and (CCR-0044) a replay hook behind `unported!`
   in `crates/pob-gamedata/src/stub.rs`; move OF-A's `l4kit` into `testkit.rs` (CCR-0041 §4).
   Then switch the per-unit seams: `calc_perform/{pools_charges,setup_flasks,ailments_dispatch}.rs`,
   `calc_mirages.rs`, `classes/{item_parse,item_tools}.rs` (CCR-0025 §3, 0044, 0061 §6, 0077 §1,
   0079 §2, 0090 §3, 0093 §1).
2. **`passList` injection** (CCR-0036; 0065 superseded): accept the plain table in
   `probe.rs` `sbox_field!`/`ctx_fill`, or decode ctx locals by field kind in
   `crates/pob-oracle/src/inject.rs`; delete the rebuild in `tests/OF-{D,E,F,G,H,I}.rs`.
3. **`$foreign` references**: dump or inject the referenced env's stores and outputs
   (`crates/pob-oracle/lua/state_dump.lua`, `src/inject.rs`, `pob-calc/src/probe.rs`) (CCR-0041 §5,
   0044, 0049 §4, 0064 §1, 0075, 0090 §4).
4. **`run_segment` state**: seed `OFFENCE_STATE.globalOutput/globalBreakdown` from the injected
   actor for offence stages from 2174 on (CCR-0075), and fill `OffenceCtx.output/breakdown` from the
   actor for pass-body stages (CCR-0049 §2) in `probe.rs`; delete the seeding in `tests/OF-*.rs`.
5. **Exporter absent marks and registry**: treat a typed field as present once it no longer holds
   the injected value (or `Some(_)` of an `Option`), mark fresh objects' Lua-nil fields absent,
   `Registry::keep` injected objects, `inject_keep` without the registry reset (`probe.rs`,
   `inject.rs`) (CCR-0040 §8, 0055 §2, 0077 §4, 0079 §5, §8, 0082 §3, 0088 §2, 0093 §3a); delete
   `setup_flasks::present`, `unmark_absent` calls, `fix_absent_marks`, `mark_fresh_skill_defaults`.
6. **Same-run pairing**: `testkit::segment_diff` takes `:in` and `:out` from the first directory
   holding both, or dumps carry a run id (CCR-0041 §4, 0067 §3).
7. **pob-state/1 coverage**: `item.buffModList`, `item.sockets`, the item `*ModLines`, a `model`
   member (skills tab, slots, active item set, main socket group), the tree for `keystoneMap`
   (testkit loads `data/dump/trees/<ver>`), kind hints for locals, closures as `{"$fn": src}`
   (a `LuaClosure` trait), per-process table ids for replay identity, `testkit::load_build` via
   `loadBuildModelFromXML` (CCR-0077 §5-§6, 0079 §3-§4, 0080 §9, 0083 §5, 0085 §5, 0093 §3b-c).
   Files: `crates/pob-oracle/lua/state_dump.lua`, `src/{inject,replay,testkit}.rs`, `pob-calc/src/probe.rs`.
8. **`inject_bound`** for live socket groups, gem instances and items (CCR-0088 §1;
   `crates/pob-oracle/src/inject.rs`, then `crates/pob-ui-oracle/src/lua_calc.rs`).
9. **ModStore/ModList API** (`crates/pob-calc/src/classes/{mod,mod_store,mod_list}.rs`):
   insertion-ordered `StoreMods::DB` (CCR-0014 §1); public `list_at`/`list_remove`/`list_set`
   (CCR-0040 §4, 0061 §2, 0085 §2; then `calc_active_skill.rs`, `item_mods.rs`, `actor_a.rs`);
   `NewMod`/`ReplaceMod` with an optional source (CCR-0063, 0094 §5; then `calc_defence/recovery.rs`,
   `calc_perform/buffs.rs`); **G1a**: `GlobalLimits` as an inline map and `GetStat` without
   `snapshot()` (CCR-0015 §6; G1a's dhat half fails until then).
10. **Shared helpers and duplicate copies**: `isValueInArray` in `modules/common.rs` (CT; private
    copies in `calc_offence/ailments_b.rs`, `calc_active_skill.rs`; CCR-0049 §3); OF-H's private
    `ccr0035` module → `use super::ailments_a as closures` (CCR-0049 §1, `ailments_b.rs`); DF-E's
    copies of DF-A's `isElemental`/`dmgTypeList`/`allyLifePoolList`/`calcLifeHitPoolWithLossPrevention`
    (CCR-0067 §1-§2, `calc_defence/est_pools.rs`); `ConfigTab::{varControl, SetPlaceholder,
    SelByValue}` used by `apply_a.rs`/`apply_b.rs` (CCR-0038); a `MainOptions` thread-local in
    `module_state.rs` for `defaultItemAffixQuality`, `notSupportedModTooltips`, `devModeAlt`
    (CCR-0025 §1, 0061 §4); `formatNumSep` and `lua-utf8` in pob-lua, one copy for CT
    (`calc_format.rs`) and U5 (`luastr.rs`), and Common.lua 803-828 in `docs/port/scope.toml`
    (CCR-0091 §5, 0033 §2); `pob_lua` `cvtt_i64`/`cvtt_i32` public (CCR-0033 §1); LuaJIT
    `math.random`/`randomseed` (CCR-0033 §3); U10's `wrap_string` copy → `crate::wrap::wrap_string`
    (CCR-0073 D, 0084 §5); U8's local `normalize`/`diff` copy → `pob_sg::trace` (CCR-0028 §13).
11. **Correctness follow-ups found by CCR2** (**priority**: each a wrong value in the shipped calc today, not only a test gap; the fix round's review confirmed the first, second, fifth and sixth):
    `calc_perform/ailments_dispatch.rs:1187-1190` still discards `calcs::mirages`'s result, so
    `offence` runs after a Reflection / Tawhoa mirage where PoB skips it (CCR-0090 §1);
    `calc_tools.rs:295-303` reads `grantedEffect.fromItem` from data, not through
    `init_env_skills::grantedEffect_fromItem`, so an item ExtraSupport can support an item-granted
    skill (CCR-0083 §1; move the accessor to CT); `Item:Craft` with a fractional `statOrder` raises
    (CCR-0061 §8, `item.rs`); IT-A's `self.sockets = { }` does not clear `colourAlwaysMatches`
    (CCR-0061 §3, `item_parse.rs`); `calc_setup/init_env_skills.rs:1074` (CalcSetup.lua:1755) reads
    `grantedEffect.hasGlobalEffect` from data, not `skill_stat_map::hasGlobalEffect` (CCR-0091 §3);
    no reader of `grantedEffectLevel.cost` (`calc_offence/costs_passes.rs`,
    `calc_perform/pools_charges.rs`) consults `skills_tab::levelCostCleared` (CCR-0019 §8). The
    calc doc §4.4 table states the rule these sites break. (Closed since: both cost readers consult
    `levelCostCleared`, INT-1F-A, CCR-0133; `Item:Craft` with a fractional `statOrder` no longer
    raises, `Key::F`, FUP1, CCR-0102; A14.)
12. **Unit-owned frozen stub signatures** (callers compile against the delegating frozen fn
    today): `calcRadiusBreakpoints -> (Option<f64>, …)` (CCR-0041 §1, 0043; OF-B drops its
    wrapper); `mergeBuff(src: &Val, …)` (CCR-0085 §1, 0079 §6, 0094 §4; PF-C/E/F drop their copies);
    PS-A `ConnectToClass -> bool`, `GetShortestPathToClassStart -> Option<LjIntMap<bool>>`,
    `ImportFromNodeList` (LjIntMap params), `CountAllocNodes`/`GetSocketedJewel` arities
    (CCR-0052 §3-§6, 0020 §7); PS-C `BeginLegacyClusterHashConversion -> bool`, the orbit helpers on
    `&LjIntMap<NodeRef>` (CCR-0072 §1-§2); `Tooltip::add_separator(&mut Cx)` (CCR-0073 A).
13. **UI platform and framework gaps**: `MainCore.invert_slider_scroll_direction` (CCR-0069 A,
    `pob-ui-core/src/main_core.rs`); `Base.other_drag_source/other_drag_targeting` (CCR-0078 B,
    U4's `Base`); `HttpRequest` proxy/protocol/noSSL (CCR-0026 C, `pob-sg/src/sys.rs`);
    `Sg::record_effect` and the platform effect calls (CCR-0028 §2, 0081 §4); pob-oracle's
    `NewFileSearch` (`findDirectories`, CCR-0028 §4, `crates/pob-oracle/src/jit.rs`); the oracle's
    `lua-utf8` (global `utf8`, `utf8.next`, or the real module; CCR-0033 §4, 0081 §3; `jit.rs`,
    `crates/pob-dump/src/lua/prelude.lua`); U8's switch-over to `run_headless` (CCR-0028 §10);
    `Platform::cloud_provider`, `Sys::make_dir_lua` (CCR-0084 §3-§4).
14. **Data dump**: `tree.nodes` source order (`pob-dump`, `pob_gamedata::tree_node_order`) and the
    tree-dependent generated uniques (CCR-0024 §5, §7); `SkillLevel.cost: Option` (CCR-0093 §2),
    `GrantedEffect.minionList: Option` (CCR-0040 §3) in `pob-gamedata/src/schema/skills.rs`; the
    `JewelFuncSpec.key` doc comment (CCR-0039 §4, `pob-gamedata/src/modifier.rs`).
15. **Model fields** (U0's `model/mod.rs`, owners BM-A/BM-B): `SkillsTab` UI settings, `ConfigTab.input/placeholder: Field<Table>`,
    `defaultState`, `Option` socket-group/gem nils, `Item.socketsColourAlwaysMatches` (CCR-0019 §2, §4-§7);
    `Build.extra`/typed fields, `ItemsTab.showStatDifferences`, `TreeTab.showConvert`,
    `CalcsTab.sectionList`, `Load -> bool` (CCR-0080 §2-§4, 0052 §9, 0072 §6 `buildName`);
    `SocketGroup.mainActiveSkill(Calcs)`/`Build.mainSocketGroup: Option`, typed `sourceItem`,
    `displayGemList`, `reqOverride`, `explodeSource` (CCR-0083 §3-§4); `PassiveSpec.subGraphs:
    Shared<LjIntMap<Table>>` (CCR-0072 §4); `ActiveSkill.summonSkill` as `Weak` (CCR-0040 §7; done by LEAK1, CCR-0135, A12).

16. **Test-input policy** (CCR-0031 B): `crates/pob-oracle/tests/{BM-A,CO-C,DF-C,DF-D,DF-E,DF-G,MI,OF-D,PF-A,PF-C,PF-D,PF-E,PF-F,PF-G,PS-B,PS-C,SK-A,SK-B,TR-A,TR-B}.rs`
    still skip a missing input unless `POB_ORACLE_REQUIRE=1`; switch them to `pob_testpaths::require`.
    Two more name the variable: `tests/DF-F.rs:53` reads it as a strictness flag (pending hits count
    as failures; keep `DFF_STRICT`/`CI` and drop the retired name) and `tests/CS1.rs:33` mentions it in
    its module doc.
17. **`known_diffs.toml` entries for accepted divergences** (owners add them with the keys their
    diff tool prints, `docs/port/known_diffs.toml` format): CCR-0014 §2 (MS-A: `GetStat`
    `ManaReservedPercent`/`LifeReservedPercent` on the enemy store, PoB raises at
    ModStore.lua:322/336 on the nil `activeSkillList`, Rust returns 0; MS-A's test counts them);
    CCR-0015 §3 needs no entry for the enemy `itemList` and `GuardianRelicAll.monsterTags` rows
    any more: E3 is applied (A10), both are `Option`s that are `None` where Lua has nil, and MS-B
    compares those queries exactly (its CCR-0015 bucket keeps only the actor-without-`output`
    rows; the `activeSkillList` row is CCR-0014 §2's); CCR-0015 §1 (U40: the `divVar` tag text of
    CalcBreakdownControl.lua 481/485, ui doc §13 item 14, entry added with U40); CCR-0091 §4 (CT:
    `mergeKeystones` leaves the tree node's keystone mod with its own `source`, PoB re-sources the
    shared mod, ModTools.lua:254-255; observable in Calcs source columns and `source`-filtered
    queries after an item-granted keystone; entry until PT/MS-A provide a node `modList` replace).

`docs/port/scope.toml` follows the §11.1 rows CCR2 extended (fix round 1): Common.lua 1110-1116
(IT-A), the BM-B model halves (SkillsTab.lua 323-349, 1103-1127, 1275-1278, 1452-1475; ConfigTab.lua
672-692, 1280-1300, 1337-1362; PartyTab.lua 22-36) and the BM-A model halves (Build.lua 64-110,
603-698; ItemsTab.lua 37, 70-74, 101-108, 140-235, 1196-1215, 2560-2605, 2978-2993;
ItemSlotControl.lua 21-123; TreeTab.lua 26-38; CalcsTab.lua 24-45, 151-154, 370-375;
CalcSectionControl.lua 20-49; SkillsTab.lua 89, 95-96, 423-426; ConfigTab.lua 1205-1240).
`provenance.py coverage` is 100% over them: CCR2 added the missing markers (`build_xml.rs`
SkillsTab 89/95-96/423-426, `calcs_tab.rs` CalcsTab 45/151-154/370-375 and CalcSectionControl
20-49, a `@pob-skip` for the search control 41-42, `config_tab.rs` 683-692 → 682-692) and
corrected the CalcsTab range, which the BM-A row and CCR-0080 gave as 147-150 (the CalcSections
loop is 151-154). `pairs_inventory.py` classifies the three `pairs` sites the wider scope reaches
(Common.lua:561, Build.lua:679, ItemSlotControl.lua:96). Common.lua 803-828 stays out of scope
until item 10 leaves one `formatNumSep` marker (two cover it today).

### Escalated (not decided by CCR2)

All five were approved by the orchestrator and applied by unit RETYPE (2026-11-10, **A10**).

- **E1. CCR-0030 (+ CCR-0076, CCR-0040 §1-§2, CCR-0074 §6, CCR-0083 §2).** Retypes frozen fields
  (`ActiveSkill.triggeredBy`, `Actor.uses`, `Env.explodeSources`, `SocketGroup.explodeSources`,
  `Item.slotModList` to 3, `Env.flaskSlotMap`, `Actor.appliedEnemyModifiers`, `ModStore.notBuff`)
  that AS, PF-A, PF-C, PF-E, PF-F, IT-C, CS0, CS3 and TR-A work around today and that in-flight
  CS2/TR-B/PF-B touch. The proposals conflict on `Actor.uses` (`Option<Value>` in CCR-0030 #2,
  `Option<NameSet>` in CCR-0040 §1), and CCR-0040 §1 adds `Actor.type/lifeTable/itemSet`. Without
  `notBuff` the **shipped** calc loses the flag (PoB `BuffOnSelf` 7, Rust 8 on allflame_exp_002).
  **Applied by RETYPE (A10)**; `Actor.uses` is `Option<NameSet>`.
- **E2. CCR-0087 (`TriggerConfig`).** `sourceWeapon: bool`, `triggeredSkillCond`, a closure
  `comparer` with optional arguments: TR-B (in flight) builds configs through TR-A's `extra`
  setters, so retyping the struct now conflicts with it. (PF-B's in-flight CCR no longer uses 0087;
  see the renumbering note above.) **Applied by RETYPE (A10).**
- **E3. CCR-0015 §3.** `Actor.itemList: Option` (None for the enemy) and
  `MinionData.monsterTags: Option` (frozen U0 types; CS1/CS2 write `itemList`, inject fills it);
  MS-B asked the integrator for a decision. **Applied by RETYPE (A10)** (`Field<Option<ItemList>>`).
- **E4. CCR-0079 §1.** Fields for the `mergeFlasks`/`mergeTinctures` closures' upvalues in the
  frozen `PerformCtx` (calc doc §7.1/§7.2; `locals_audit.py --check` pins 88 crossing locals) plus
  two line probes; PF-D/PF-F L4 seeding depends on it. PF-C's per-env side table works meanwhile.
  **Applied by RETYPE (A10).**
- **E5. CCR-0046.** `calcSkillDuration(…, env: Option<&EnvRef>, enemyDB: Option<&ModDB>)` changes a
  frozen OF-A function called by OF-A..OF-I, PF-E and the skill hooks; SK-A's private
  `calcSkillDuration_noEnv` is the alternative. **Applied by RETYPE (A10)**; the private copy is deleted.

## A9. A8 needs-code items done by ORC-X2 (2026-11-10)

ORC-X2 implemented part of the A8 "Needs code" list once, centrally, and deleted the per-unit
copies and workarounds it replaces. Items not listed here are still open as A8 describes them.

| A8 item | What was built | Workarounds deleted |
|---|---|---|
| 2 `passList` (CCR-0036; 0065 superseded) | `probe::ctx_fill` hands a plain Lua sequence of tables to a `Shared<Vec<Table>>` ctx field (`table_list_set`). `ProbeField::from_val` stays strict: a lenient one also changes how `inject` fills typed fields of injected objects, and those sequences can be aliased (`socketGroup.explodeSources` is `env.explodeSources`; tried, 27 OF-G failures) | the `passList` rebuilds in `tests/OF-{D,E,F,G,H,I}.rs` |
| 4 `run_segment` state (CCR-0075, CCR-0049 §2) | for every offence stage from `offence:2174` on, `run_segment` sets `OFFENCE_STATE.globalOutput/globalBreakdown` from the injected `actor` (CalcOffence.lua 2176/2607/4165 copy the function-level `output`/`breakdown`, 354/356). At `offence:3346`/`4990` it fills `OffenceCtx.output/breakdown` from the actor; the loop locals fill the pass locals | the `OFFENCE_STATE` seeding in `tests/OF-F.rs`, `tests/OF-H.rs` |
| 6 same-run pairing (CCR-0041 §4, 0067 §3) | `testkit::find_pair` (both dumps from the first directory that holds both; `segment_diff` uses it). Every dump now carries `"run": "<pid>-<nanos>"` of its probe process (`runner::probe_prelude` → `__probe_cfg.runId` → state_dump.lua `encodeDoc`; `StateDump::run`), and `testkit::same_run` (called by `segment_mismatches`) reports a pair from two processes as an error instead of diffing it. Dumps made before ORC-X2 have no id and pass | DF-E's private pairing (`tests/DF-E.rs` uses `testkit::{find_pair, segment_mismatches}`) |
| 3 `$foreign` (CCR-0041 §5, 0044 §2, 0049 §4, 0064 §1, 0075) | state_dump.lua `inlineForeign`: another env's **actors, mod stores and actor `output` tables** are written in full where the dump reaches them; its envs, active skills and breakdowns stay `{"$foreign": …}` markers. A copied skill's `skillModList.actor` and `baseSkillModList.parent` (CalcActiveSkill.lua:181/184/492) and `activeSkill.mirage.output` therefore inject with their contents. New dumps only: the shared probe cache keeps its old dumps (another patched-source sha) | — (the tests' foreign classification stays, it now matches only hits whose dump still reaches a marker) |
| 1 (part) callee seam (CCR-0044) | `pob_gamedata::stub::{set_replay_hook, replay, ReplayHook}` (re-exported by `pob_calc::stub`): `unported!` returns the hook's value when one is installed (tests only), else `Default` | — (the shared `callee!` seam, `testkit::segment_diff_with_replay`, the `[[function]]` probes for OF-A's helpers and the switch of the six per-unit seams are still open) |
| 8 (part) (CCR-0088 §2) | `inject::inject_keep` (`inject` without the registry reset) | — (U15's bridge still injects the main env last; `inject_bound`, §1, is open) |
| 9 (part) `NewMod`/`ReplaceMod` source (CCR-0063, 0094 §5) | `source: impl ModSourceArg<'s>`: any `&T: AsRef<str>` (every existing call site) or `Option<&str>` (nil) | the local no-source helpers in `calc_defence/recovery.rs`, `calc_perform/buffs.rs`, `calc_setup/init_env_base.rs`, `calc_triggers/{config_table,handlers}.rs` |
| 10 (part) shared helpers | `modules::common::isValueInArray` (Common.lua:569-575, generic, 1-based index); `ConfigTab::{varControl, SetPlaceholder, SelByValue}` (CCR-0038); `pob_lua::{lj_num2int, lua_tointeger}` (CCR-0033 §1) | `ailments_b.rs`' private `isValueInArray` and `mod ccr0035` (now `use super::ailments_a as closures`, CCR-0049 §1, §3); `calc_active_skill.rs`' `isValueInArray`; `est_pools.rs`' copies of DF-A's `isElemental`/`dmgTypeList`/`allyLifePoolList`/`calcLifeHitPoolWithLossPrevention` (CCR-0067 §1-§2); `apply_b.rs`' control helpers (thin delegates now) and `apply_a.rs`' `enemyRadius` placeholder write; `luastr::{lua_int, lua_integer}` (re-exports); U10's `tooltip::wrap_string` copy (re-export of `crate::wrap::wrap_string`, CCR-0073 D) |

New shared testkit API: `find_pair`, `same_run`, `segment_diff_dumps`, `segment_mismatches`,
`run_dump`/`SegmentRun`, `panic_text`.

Still open from A8 (not touched by ORC-X2): item 1 apart from the stub hook; item 3 for
foreign envs/skills/breakdowns; items 5, 7, 8 §1, 11-17; from item 9 the insertion-ordered
`StoreMods::DB`, `list_at/list_remove/list_set`, G1a; from item 10 the `MainOptions`
thread-local, `formatNumSep`/`lua-utf8` in pob-lua, `math.random`, U8's local
`normalize`/`diff`.

Verification (ORC-X2, one test binary at a time; before = rust-port 571c0c9, after = ORC-X2):
OF-D all57 1090 clean + 17 foreign → 1107/1107 clean, unit corpus 333 + 11 foreign → 344/344
(fresh probes); OF-G 103 ok + 2 foreign → 105 ok; OF-H 233 pass + 1 foreign-ref → 234 pass
(18 stub-blocked on PF-B's `actionSpeedMod` both times); OF-E 1114 clean / 28 stub-blocked /
21 mirage-foreign, OF-F 3346 1148 + 21 foreign-parent and 3957 1149, OF-I 54 clean + 4 foreign,
DF-C 3/14/57, PF-E 24/57, CO-A 1741/1741, CO-B 2407/2407, AS 56/56, CS1 2590/2590, TR-A
563/563, ORC-X unchanged (all round trips exact): identical before and after (these read the
shared probe cache, whose dumps predate the state_dump.lua change). DF-E corpus 42/42 both; its
all57 leg fails its 98% gate before and after alike (171/363 on the probe cache's mixed-run
dumps), an input problem the run ids now report for new dumps.
After rebasing onto rust-port 85604a9 (PF-B merged): OF-G 105/105, OF-H 252 pass / 0
stub-blocked, OF-D 1107/1107 + 344/344, DF-C 3/14/57, CO-B 2407/2407, ORC-X unchanged.

## A10. A8 escalations E1-E5 applied by RETYPE (2026-11-10)

The orchestrator approved the five escalations of A8; unit RETYPE retyped the frozen fields
once and deleted the per-unit workarounds that stood in for them. The applied CCRs' status
lines (0015, 0030, 0040, 0046, 0074, 0076, 0079, 0083, 0087, 0095) point here.

| A8 item (CCRs) | What was retyped | Workarounds deleted |
|---|---|---|
| E1 (CCR-0030 #1) | `ActiveSkill.triggeredBy: Field<Option<SkillEffectRef>>` (the trigger support's effect, CalcActiveSkill.lua:536; CalcTriggers' run-time keys go in the effect's `extra`) | `calc_active_skill::triggeredBy()` (the `extra` table read) and its uses in AS, TR-A, TR-B |
| E1 (CCR-0030 #2, CCR-0040 §1, CCR-0095 §1) | `Actor.uses: Field<Option<NameSet>>` (the data set, by identity), `Actor.type: Field<Option<Name>>`, `Actor.lifeTable: Cell<Option<&'static [f64]>>`, `Actor.itemSet: Field<Option<Table>>`; inject resolves the `$data` markers to them | `calc_active_skill::LifeTable` and `minion.extra["uses"/"type"/"lifeTable"/"itemSet"]`; the `minion.uses`/`lifeTable` representation allowances in `tests/AS.rs`; `tests/PF-B.rs` `adapt_minion`/`minions_of` (marker ↔ table conversion) |
| E1 (CCR-0030 #3, CCR-0083 §2) | `Env.explodeSources` / `SocketGroup.explodeSources: Shared<Vec<ExplodeSource>>` (one list; `ExplodeSource::{Node, Item}`) | CS2's and CS3's `env_explodeSources` (`env.extra["explodeSources"]`) |
| E1 (CCR-0030 #4) | `Item.slotModList: [Field<Option<ModList>>; 3]` (Ring 3) | `item_mods::SLOT_MOD_LIST_3` and `Item::slotModList3`/`set_slotModList3` (`Item.extra`); the probe registry's `slot3`/`set_slot3`; `tests/CS2.rs` `slot3_equal` |
| E1 (CCR-0030 #5, CCR-0079 §7) | `Env.flaskSlotMap: Field<Shared<PtrMap<Item, f64>>>` | CS2's `$flaskSlotMap` side table (`FLASK_SLOT_MAP`, `flaskSlotMap_get`, `new_flaskSlotMap`, `set_flaskSlotMap`) |
| E1 (CCR-0030 #6, CCR-0085 §4) | `Actor.appliedEnemyModifiers: Field<Option<Shared<ModPtrSet>>>` (mod identity, insertion order) | `calc_perform::actor_a::enemy_cache` (a thread-local per-actor key list beside the frozen `Table`, which also matched injected keys by value); in its place `inject` rebinds the dumped keys to the actor's payload mods (`patch_enemy_modifier_keys`, below) |
| E1 (CCR-0030 #7, CCR-0076) | `ModStore.notBuff: Cell<bool>` (PF-E writes it at CalcPerform.lua 2331/2810/2834, PF-F reads it at 3346/3425; inject and the exporter carry it) | `tests/PF-F.rs` `count_not_buff` (the `BuffOnSelf` allowance) and the probe registry's `side_extra` for `notBuff` |
| E1 (CCR-0040 §2, CCR-0074 §6) | `SocketGroup.sourceItem: Field<Option<ItemRef>>`, `SkillEffect.req: Cell<Option<f64>>` | the `extra` reads/writes of both keys |
| E2 (CCR-0087) | `TriggerConfig.sourceWeapon: bool`, `triggerSkillCond: Option<SkillCondFn>`, `comparer: Option<ComparerFn>` (TR-B's arms set them, TR-A's handlers read them) | `CondBox`/`ComparerBox` in `extra` and the `set_triggeredSkillCond`/`triggeredSkillCond`/`set_comparer`/`comparer_fn`/`sourceWeapon` accessors |
| E3 (CCR-0015 §3) | `Actor.itemList: Field<Option<ItemList>>` (CS1 sets the enemy's and the player's pre-L536 to `None`, AS a minion's until CalcActiveSkill.lua:722, inject a dumped actor without the key); `MinionData.monsterTags: Option<Vec<Name>>` | — (the MS-B known-difference rows for these two, A8 item 17, no longer occur) |
| E4 (CCR-0079 §1) | `PerformCtx.mergeFlasks: Option<Rc<FlaskUpvalues>>`, `mergeTinctures: Option<Rc<TinctureUpvalues>>` (the closures' upvalues; `None` = nil before 1691 / 1862); line probes `perform:1691` / `perform:1862` in `corpus/PF-C/stages.toml` and `corpus/PF-F/stages.toml`; `testkit::seed_merge_closures` replaces the `$fn` markers of an injected perform dump with the upvalues from those line dumps | PF-C's per-env `MERGE_UPVALUES` side table (`MergeUpvalues`, `record_upvalues`, `flask_upvalues`, `tincture_upvalues`) |
| E5 (CCR-0046) | `calcSkillDuration(…, env: Option<&EnvRef>, enemyDB: Option<&ModDB>)`; `env = None` stands for Lua's `{}` (no `mode_effective`; Data/Skills/act_int.lua), not for a nil `env` | SK-A's `calcSkillDuration_noEnv` copy in `data_fns/skills_act_int.rs` |

`patch_enemy_modifier_keys` (`crates/pob-oracle/src/inject.rs`): the dump writes mods as
values, so an `appliedEnemyModifiers` key is decoded as a copy. Each key is rebound one-to-one
(dump key order) to an equal payload mod of the actor's store chain, first among the payloads
the actor's `Tabulate(nil, nil, "EnemyModifier")` returns at injection time, then among the
rest. Several equal payloads are common (Yoke of Suffering: nine `EnemyModifier` mods, one per
ailment, with equal `DamageTaken` payloads; the keys are those whose ailment held at
CalcPerform.lua:1350), and only the live ones are looked up again (976). The shipped calc never
injects: there the set holds the very `Arc`s it inserted.

Fix round 1 (after review; one commit on port/RETYPE that consolidates the paused WIP
bf189bd and the parallel attempt port/RETYPE-fixwip 337829c, without fixwip's
`ailments_dispatch.rs`, which is rust-port's): the two remaining `extra` reads of a moved key,
`minion.type` in OF-B (`skill_types.rs`, CalcOffence.lua:1505, raised "attempt to concatenate a
nil value" for every minion skill with a duration) and in `activeSpectreList`
(`pools_charges.rs`, CalcPerform.lua:2215-2216, left the list empty), read `minion.r#type`; a
grep over every retyped key (`type`, `uses`, `lifeTable`, `itemSet`, `sourceItem`,
`triggeredBy`, `req`, `explodeSources`, `notBuff`, `appliedEnemyModifiers`, `flaskSlotMap`,
`itemList`, `sourceWeapon`) finds no other in pob-calc and pob-oracle (pob-app was not searched
then; fix round 2 below). The live-first rebind above replaced a rebind by
first equal payload, which bound Yoke keys to payloads of ailments that did not hold and
re-applied a `DamageTaken` at doActorMisc. A minion's `itemList` is `None` from
CalcActiveSkill.lua:704 (`local minion = { }`) until 722 (`calc_active_skill.rs` unsets
`Actor::new`'s empty list), and inject unsets it for a dumped actor without the key, so an
`allSlots` ItemCondition on either raises as at ModStore.lua:696/747. E4's seeding is wired
into PF-F: `testkit::seed_merge_closures`, the `perform:1691` / `perform:1862` stages in
`corpus/PF-F/stages.toml`, and `tests/PF-F.rs` reads all seven stages from one probe-cache
directory when one holds them (an older five-stage fill earlier in the cache order otherwise
supplies a `perform:2997` with no closure dumps beside it).

Verification (2026-11-11, one test binary at a time through `scripts/port/cargo.sh`, after
merging rust-port 41e8c3a): PF-F dumps refilled for all 58 builds (57 `.parity/builds` +
`pff_party_minion`) with the 7-stage fill command, into probe-cache `71b97fdcfcdf9d92`; PF-F L4
`perform:2997` → `perform:3497` 58/58 pass, 0 mismatches, both closures seeded on every build
(before: 0/58, all raising "attempt to call local 'mergeFlasks' (a nil value)" at L3121);
allflame_exp_002 `env.modDB.multipliers.BuffOnSelf` is PoB's 7 in the 3497 dump and matches
(CCR-0076: 8 without `notBuff`). PF-B all57: 1238/1238 gated cases clean (`initMinionModDB`
20/21, the one other the documented not-gated itemsTab-slots case of allflame_exp_039); with
`PFB_BUILDS=allflame_exp_013,allflame_exp_015,allflame_exp_001` 2 passed / 0 failed (before
the live-first rebind: 8 of 63 gated cases not clean, all `doActorMisc:in#7/9/11/13`). OF-B L4
on allflame_exp_001,007,013: 66 exact, 3 `$foreign` inputs, 0 raised. PF-D 57/57 + 1/1, PF-C,
AS 3/3, TR-A 1 (1 ignored), TR-B 1 (1 ignored), IT-C 2/2, SK-A 2/2, CS1 1/1, CS2 2/2 pass.
MS-B all57 2/2 (`CCR-0015` known rows 0 on every build). CS3 fails identically on port/RETYPE
and on rust-port 41e8c3a, so not from the retype: L4 pass 329 / restart 6 / fail 8, all 8 (4
builds x `initEnv:1451#9/#10`) on `env.override.extraJewelFuncs.actor.modDB.mods.LifeRegenPercent`
(the `warcryPowerBonus` tag, CCR-0083 §7 / CCR-0014 §3); L5 171/171 runs raise "attempt to index
a nil value" (`lua_table.rs:163`, the nil class stats at CalcSetup.lua:593).

Fix round 2 (after review): U30's Skills tab, which arrived with the rust-port merge, still used
the old representation, and pob-app (so pob-ui-oracle) did not compile. `tabs/skills/core.rs`:
`copy_socket_group` copies `sourceItem`; the source note (SkillsTab.lua 389-394) and
`AddSocketGroupTooltip` (1310-1312) iterate `explodeSources.snapshot()` through
`ExplodeSource::to_val`; the source note (398-399) and the tooltip (1319) read
`socketGroup.sourceItem` (the typed field) where they read `extra["sourceItem"]`, which nothing
writes any more (the item never showed, the note fell through to `sourceNode` or `?`). U15's
bridge (`pob-ui-oracle/src/lua_calc.rs` `copy_group`) copies `sourceItem` too, so the Rust side
of the UI oracle gets the item PoB's calc set. New U30 scenario `f_item_source` (allflame_exp_028
with its item-granted group first, so the displayed group has a `sourceItem`): before the fix
8 divergences per screen (the note read `'^xC8C8C8?^7'`), after 0. It also showed a U30 slip in
the same label: L412's `[[` drops the newline after it, so `sourceName` runs straight into
"will automatically apply" (the port had `\nwill`). The second rust-port merge (2167f22)
brought three more: U31's `skill_set_list::copy_socket_group` copies `sourceItem`; U38's
`tabs/items/tooltip.rs` copies all three `slotModList` slots in `copy_item` and reads Ring 3's
as `slotModList[2]` (it called the deleted `Item::slotModList3`); U34's
`tests/u34_items_core.rs` `shallow_copy_item` copies the third slot. The grep, redone over
every crate (`n!("<key>")` and `"<key>"` for all keys listed above plus `triggerSkillCond`,
`comparer`, `mergeFlasks`, `mergeTinctures`, `monsterTags`, `slotModList3`), finds no other read of a moved
key from a retyped object: the remaining hits are other tables' keys of the same name (mod
`type`, `weaponData.type`, buff `type`, the `ExtraAura` LIST value's `notBuff`,
`grantedSkill.sourceItem` and the requirements entries' `sourceItem`, plain tables in PoB too),
probe/inject registry names, or tests.

Verification (fix round 2, after merging rust-port 2167f22, one test binary at a time through
`scripts/port/cargo.sh`): `check -p pob-calc -p pob-app -p pob-ui-core -p pob-ui-oracle -p
pob-oracle --tests` clean (every crate that depends on pob-calc). u30_skills 11 runs (6 scripts,
`f_item_source` new), 0 divergences, T4 29 Skills sections identical; u31_skills 2/2 (10 runs,
T4 10 pairs); u38_tooltips 3/3 (400 unique tooltips, 48 compare tooltips, 0 failures);
u34_items_core 1/1 (9 runs, T4 33 pairs); AS 4/4 (one earlier run of `as_oracle` exceeded its
LUA-ORDER bucket bound and passed on the rerun: PoB's per-process `pairs` order, not this
change, which touches no calc code).

## A11. pob-calc split into six crates (CSPLIT, CCR-0131; orchestrator decision)

`crates/pob-calc` is now a facade that re-exports every public path; the code lives in six crates
(docs/port/calc-split-plan.md, CCR-0131). Every `pob_calc::…` path is unchanged, so code outside
the calc depends on `pob-calc` only, never on a sub-crate. Docs that name
`crates/pob-calc/src/<p>` mean `crates/pob-calc-<crate>/src/<p>`:

| `<p>` under `src/` | crate |
|---|---|
| `lua_table cfg flow stub breakdown global_cache module_state actor active_skill env prelude probe late data_fns_types` (`.rs`), `classes/`, `model/`, `modules/{mod,calc_tools,common,calc_format,calc_breakdown,mod_tools_env,jewel_funcs,calc_base,calc_perform_shared}.rs` | `pob-calc-core` |
| `modules/calc_setup/`, `modules/calc_active_skill.rs` | `pob-calc-setup` |
| `modules/config_options/`, `modules/mod_map.rs` | `pob-calc-config` |
| `modules/calc_defence/` | `pob-calc-defence` |
| `modules/calc_offence/`, `data_fns/` | `pob-calc-offence` |
| `modules/calc_perform/`, `modules/calc_triggers/`, `modules/{calc_mirages,calcs,power}.rs`, `calcs_facade.rs`, `probe_segments.rs`, `late_provider.rs` | `pob-calc-perform` |
| `lib.rs` (the facade), `tests/hyg1_len.rs` | `pob-calc` |

Binding rules (CCR-0131 §2): calc doc §2 rule 1 becomes `pob-lua ← pob-gamedata ← pob-modparser ←
pob-calc-core ← {setup, config, defence}`, `defence ← offence`, `{core, setup, config, defence,
offence} ← perform ← pob-calc ← UI/oracle`, and rule 3 (wasm32, no mlua) covers all seven calc
crates. Five calls from the core to perform (`BuildOutput`, `configApply`,
`MiscCalculator::call`, `doActorLifeMana`, `doActorLifeManaReservation`) go through the
link-time seam in `pob-calc-core/src/late.rs` (`late_fns!`; provider
`pob-calc-perform/src/late_provider.rs`). **`#[allow(unsafe_code)]` is approved for exactly those
two macro-generated `seam` modules**; every type in `late_fns!` is a full path. Only `pob-calc`'s and
the sub-crates' own `probe` features (and the middle crates' `pob-calc-perform` dev-dependency
with `probe`) enable `pob-calc-core/probe`. The unit-test binaries of setup, config, defence and
offence link the provider (dev-dependency + `extern crate pob_calc_perform as _` under
`cfg(test)`, `doctest = false`); the core's own unit tests use panicking `cfg(test)` stubs.
Moved items and the new files' owners: CCR-0131 §3 (`calc_perform_shared.rs` [PF-B], `late.rs`
/ `late_provider.rs` [CSPLIT, frozen], `data_fns_types.rs` [U0], `probe_segments.rs` [ORC-X]).
A branch that adds a file under the old `crates/pob-calc/src/` gets a directory-rename conflict on
merge: `git mv` it into the crate of the table above (git's suggested directory can be wrong,
because `src/modules/` is split across crates). `POB_PORT_SLOTS` stays 1.

## A12. CCR-0135 applied by LEAK1 (2026-11-12; orchestrator approved)

`ActiveSkill.summonSkill` is `Field<Option<ActiveSkillWeak>>` (calc doc §3.3 rule 4, §5.4): the
strong back-pointer closed the cycle summoning skill → `minion` → minion skill → `summonSkill`,
so every env with a minion skill leaked its skill graph (CCR-0135 §1-§2). Code: the field and the
reader `ActiveSkill::summonSkill()` (`pob-calc-core/src/active_skill.rs`; a dead handle panics, it
is an ownership bug, not a Lua nil), `createActiveSkill` stores `Rc::downgrade`, every reader
upgrades (`calc_active_skill.rs` copy/modlist, `calc_tools.rs`, `buffs.rs`, `skills_misc.rs`,
`pob-ui-oracle/src/lua_calc.rs`), `ProbeField for Weak<ActiveSkill>` (`probe.rs`; a dead handle
reads as nil). CCR-0135 §3's open point: `copyActiveSkill` keeps a foreign summoning skill (actor
not `env.player`) alive in `newSkill.extra["summonSkillKeep"]` (`summonSkillKeepKey`), like
CCR-0113's `actorKeep`; it closes no cycle (the summoning skill never reaches the copy). Test:
`crates/pob-calc/tests/leak1_cycles.rs` (a CALCULATOR env, and the envs a recalc replaces, incl.
every `GlobalCache` entry's: 0 survivors on allflame_exp_039, _007, _001, _000). Two
build-level cycles remain (CCR-0142, decided by CCRDOC in A14: §3 A applied, §3 B not built:
`Env.build`/`MiscCalculator.build` ↔ `calcsTab`, and `ActiveSkill.socketGroup` ↔
`SocketGroup.displaySkillList(Calcs)`); they leak a dropped build or a deleted socket group, not a
recalc. The web calc worker, which drops a build
per request, breaks both locally (`worker_web.rs` `run_request`, CCR-0142 §3 A; gate
`build_teardown_frees_everything_ccr0142` in the same test file); the general teardown is
CCR-0142 §3 B. Timing (`perf_bench.sh`, bench profile, 3 rounds, load average 6-12, Rust-only runs of
rust-port b9888df vs LEAK1, output and power digests identical): power report allflame_exp_039
6.87 → 7.52 s (+9 %), allflame_exp_007 4.90 → 5.41 s (+10 %); recalc 47.3 → 50.7 ms and 46.8 →
50.9 ms (+7-9 %): freeing the graphs costs what the leak saved (CCR-0135 §4). RSS after one power
report 2,770 → 691 MB and 2,033 → 452 MB. With LuaJIT in the same run, LEAK1 stays below it
(power 0.73 / 0.71, recalc 0.77 / 0.81).

## A13. INT-4: legacy crates retired (2026-11-12)

The last calc row (calc doc §11.1 INT-4), after U53's cut-over (the product UI is `pob-app` on
`RustCalc`; `crates/pob-ui` and the legacy apps were already deleted, CCR-0145 §6).

**Deleted.** `crates/pob-engine`, `crates/pob-data` and `crates/pob-extract`. Before deleting:
`cargo tree --workspace -i pob-engine -e normal,dev,build` and `-i pob-data` listed only
`pob-extract` (and pob-engine for pob-data); `-i pob-extract` listed nothing; a grep for
`pob_engine`, `pob_data`, `pob-extract` over `crates/`, `app/` and `scripts/` found no other
user, and no remaining crate reads the files pob-extract wrote (`data/*.json`, `data/trees`,
`data/skills`, `data/timeless`, the `data/pob/` runtime copy: the app reads `data/dump/` and PoB's
`src/`; only the lua2rs tests still accept an existing `data/pob/` as a fallback for
`.PathOfBuilding`). So nothing was ported or redirected; pob-extract went with the two crates
(A1 already made `pob-dump` the data tool). Also deleted: `scripts/parity/run.sh` (ran
pob-extract's `pob_diff`; `report.py` stays, `pob_calc_diff` writes its input), CI's engine
build/test/clippy jobs and its manual `pob_diff` job, and the workspace keys only the deleted
crates or the deleted egui app used (`pob-engine`, `pob-data`, `thiserror`, `base64`,
`quick-xml`, `eframe`, `egui`, `rfd`, `wgpu = "22"`). CI's calc job now runs the `mlua` gate below,
a wasm32 check of the calc crates and the pob-lua / pob-gamedata / pob-modparser tests
(`POB_ALLOW_SKIP=1`; not run on GitHub by INT-4).

**`pob-luahost` stays, dev-only.** It is `crates/pob-oracle` (A1). The oracle tests need it, and
so does pob-ui-oracle (`jit::PRELUDE`/`install_natives` in `boot.rs`, `headless` and `jit` in
`LuaCalcBridge`), so it is kept as a dev-only crate (its manifest description and README say so).
No shipping package depends on it or on pob-ui-oracle: `cargo tree -e normal` of pob-calc,
pob-app, pob-desktop and pob-web lists neither. Its `worker` module (the legacy app's calc
thread) is now used only by `tests/worker_smoke.rs`, and its `mod_parser` module by nothing
(its input `data/lua/modparser_env.lua` was written by `pob_diff --dump-modparser-env`, now
deleted; a doc note in `mod_parser.rs` says so). Removing those, and the pure-Rust interpreter
`lua/` that `headless` boots, is a pob-oracle change left to its owner.

**Gates (measured on port/INT-4).**

| `scripts/port/cargo.sh tree -p <p> -e normal \| grep -c mlua` | count |
|---|---|
| pob-calc | **0** |
| pob-lua, pob-gamedata, pob-modparser, pob-calc-{core,setup,config,defence,offence,perform} | 0 each |
| pob-sg, pob-sg-wgpu, pob-host, pob-ui-core | 0 each |
| **pob-app**, **pob-desktop**, **pob-web** (shipped products) | **0, 0, 0** |
| pob-app, pob-web with `--target wasm32-unknown-unknown` | 0, 0 |
| pob-desktop `--features lua-calc` (non-default, dev: LuaCalcBridge) | 3 |

So the `POB_UI_CALC=lua` bridge is already behind a non-default feature (pob-desktop's `lua-calc`,
`dep:pob-ui-oracle`); nothing had to move. `mlua` remains a normal dependency only of the dev and
build-time crates (pob-oracle under its `luajit` feature, pob-ui-oracle, pob-dump) and a
dev-dependency of pob-lua, pob-calc and pob-ui-core. `cargo check --target wasm32-unknown-unknown`
of the 16 runtime packages (pob-lua, pob-gamedata, pob-modparser, the six calc crates and
pob-calc, pob-sg, pob-sg-wgpu, pob-host, pob-ui-core, pob-app, pob-web) is green (7 dead-code
warnings in `pob-calc-perform/src/modules/power.rs`, wasm32 only: the parallel power report is
native-only; pre-existing). docs/port/testing.md "Runtime and dev-only crates" lists the commands.

**Tests.** `scripts/port/cargo.sh check --workspace --tests`: green, 0 warnings. Quick corpus
(`.parity-quick`, 14 builds), `cargo.sh test -p pob-oracle --test CA`: 5 passed, 0 failed (L6
headline 182/182, all keys 11,990/11,990, CALCS lists 42/42, used-tracking 126/126, misc
308/308, 0 Rust errors; L5 used-tracking and CALCS lists 14/14 clean; D13 19/19 fixed points, 0
Lua errors). `cargo.sh test -p pob-ui-oracle --test u53_e2e`: 1 passed (27 runs, 1,668,755 Lua draw ops, 0 T3
divergences, T4 33 pairs identical, 0 failed; 317 s, peak RSS 1,450 MB).

**Docs.** calc doc D4, D18, §2 rules 2 and 5, the INT-4 row; ui doc D1, D16, §3 and the U53 row
(CCR-0145 §6, now applied); STATUS.md (port and root), testing.md, README.md, packaging.md, and
history notes on docs/divergences.md, docs/parity-plan.md and ADR 0001.

## A14. CCRs 0043-0145 decided by CCRDOC (2026-11-12)

Every planned row of both unit tables is merged (U51 Trade excepted, deferred by the user).
CCRDOC gave each CCR file that had no final status one, checked against the code on `rust-port`
`f0ef34a`, and brought the binding docs to the code as built. Each CCR file's first status line
names the merge commit of the code it describes. The decisions:

- **applied**: the binding text follows the code. For a UI "seams" CCR this means: the hook
  table, trait or representation it declared exists, and its owners' one-line installs were
  made (ui doc §10.5 lists the tables).
- **rejected (not built)**: a proposal nobody implemented. The local workaround the CCR names is
  the as-built design and stays. Where the difference is visible in the product, FOLLOWUPS
  "CCRDOC" lists it.
- **superseded**: a later CCR or unit replaced it.

Most UI CCRs mix the two kinds: the seams were applied, and their requests to other owners (a
re-export, a `pub fn`, a shared helper) were not built.

| CCR | Unit | Decision | Binding text changed / not built |
|---|---|---|---|
| 0043 | OF-B | rejected | calc doc §7.2 (`calcRadiusBreakpointsOpt` + the frozen NaN shape) |
| 0044 | OF-B | applied in part (A9), rest rejected | calc doc §9.2 (the per-unit seams are the mechanism; no `segment_diff_with_replay`) |
| 0068 | DF-D | superseded (withdrawn; DF-A) | — |
| 0088 | U15 | §2 applied (unused), §1 rejected | ui doc §10.4 (`adopt_build_output`/`Rebind`) |
| 0095 | PF-B | applied (§1 A10, §4, §2 now); §3 rejected | calc doc §3.2 + STYLE.md (trailing `Override`) |
| 0096 | U16 | applied; §C rejected | ui doc §10.1 (`Mode`, `MainState`), §10.2, §10.5 |
| 0097 | CA | applied | calc doc §3.2 + STYLE.md (Flag in a table), §5.10, §9.2 |
| 0098 | U19 | applied; §2, §4, §6 rejected | ui doc §10.1 (`BuildState`), §10.5 |
| 0099 | U17 | §A applied (U23); §B rejected | FOLLOWUPS "CCRDOC" 1 (LIST not rebuilt after Options Save) |
| 0100 | U18 | applied; §A deletion superseded (test seam kept) | ui doc §10.1, §10.5 |
| 0101 | PWR | applied (typed fields not added) | calc doc §5.10; contracts#model-census |
| 0102 | FUP1 | applied | calc doc §5.1 (`Key::F`); STYLE.md; A8 item 11 note |
| 0103 | U21 | applied; §3-§5 rejected | ui doc §10.5 |
| 0104 | U20 | applied; §4, §6 rejected | ui doc §10.5 |
| 0105 | CS2 | applied as `override.extra` keys; typed field rejected; §2-§3 superseded (A10) | calc doc §5.4; contracts#model-census |
| 0106 | U22 | applied; §4 clean-ups not done | ui doc §10.5; FOLLOWUPS "CCRDOC" 9 |
| 0107 | U32 | applied; §2-§5 rejected | ui doc §10.5, §13 item 15 |
| 0108 | U29 | applied | ui doc U29 row, §13 item 15 (StatDescriber, `itemLib.wiki`) |
| 0109 | U43 | applied; §1/§3 as frozen changes, §4, §6, §8-§10 rejected | ui doc §10.1, §10.5, U43/U44 rows, §13 item 15; FOLLOWUPS "CCRDOC" 2, 3 |
| 0110 | U41 | applied | ui doc §10.5 (var-controls rule), §12.1 (tab `Widget` impl), U42 row |
| 0111 | U39 | applied | ui doc §10.5 |
| 0112 | U28 | applied; §2 move not done | ui doc §10.5, §13 item 15 |
| 0113 | INT-1-A | applied as `extra["actorKeep"]`; typed field rejected | calc doc §3.3 rule 4 + STYLE.md; contracts#model-census |
| 0114 | U45 | applied; shared `Inflate`, §4, §6 not built | ui doc §10.5, §13 item 15 |
| 0115 | U23 | applied; §B rejected | ui doc §10.2, §10.5 |
| 0116 | U40 | applied | ui doc §10.5 |
| 0117 | U44 | applied; §2, §5, §8 rejected | ui doc §10.2, §10.5 |
| 0118 | U30 | applied | ui doc §10.5 |
| 0119 | U31 | applied | ui doc §10.5 |
| 0120 | U33 | applied; §3 rejected | ui doc §10.5, §12.1 |
| 0121 | U42 | applied; §1 marker fix and §6 patch not done | FOLLOWUPS "CCRDOC" 6 |
| 0122 | U24 | applied; §1, §2 not built | FOLLOWUPS "CCRDOC" 4 |
| 0123 | U25 | applied; §3, §4 kept as divergences; §9 rejected | ui doc §10.3 (one undo stack per spec), §10.5 |
| 0124 | U35 | applied | FOLLOWUPS "CCRDOC" 10 (§5 finding) |
| 0125 | U38 | applied with its workarounds; §1 fix, §3, §5, §6 not built | contracts#model-census (temp ids); FOLLOWUPS "CCRDOC" 9 |
| 0126 | U34 | applied; §4 hook not built (DIVERGENCE(load-tail)) | ui doc §10.5 |
| 0127 | TP1 | slots applied (default 1); default 3 rejected | calc doc D20; A5; ui doc §11.3 |
| 0128 | TP1 | rejected (not built) | calc doc §9.2; testing.md "Open" |
| 0129 | U37 | applied; §1, §3 patch not built | ui doc §10.5 |
| 0130 | U36 | applied | ui doc §10.5 |
| 0132 | U27 | applied; §2 param, §4 bridge, §5, §6 shared helper not built | calc doc §5.4 (pseudo node); ui doc §10.5, §13 item 15 |
| 0133 | INT-1F-A | applied (both cost readers) | calc doc §4.4; A8 item 11 note |
| 0134 | U54 | applied; §3-§5 not built | ui doc §10.4 (`WorkerCalc`) |
| 0136 | U46 | applied | ui doc §10.5, §12.1 |
| 0137 | U47 | applied; §1, §2 not built | ui doc §10.5; FOLLOWUPS "CCRDOC" 5 |
| 0138 | U48 | applied; §1 switch, §3 deletion not done | FOLLOWUPS "CCRDOC" 9 |
| 0140 | FUP3 | rejected (not built; test-only) | FOLLOWUPS "CCRDOC" 11 |
| 0141 | FUP2 | applied (amends CCR-0091 §6: a jewel closure prints `function: 0x` + an FNV-1a hash of its spec, `mod_tools::function_text`) | — |
| 0142 | LEAK1 | §3 A applied; §3 B rejected | calc doc §3.3 rule 4 + STYLE.md; A12; FOLLOWUPS "CCRDOC" 7 |
| 0143 | U49 | applied; §2-§4 not built | ui doc §10.5, §13 item 15; FOLLOWUPS "CCRDOC" 4 |
| 0144 | U50 | applied | ui doc §10.5 |
| 0145 | U53 | applied (§6 by INT-4); §4 typed field rejected | ui doc §10.3, §10.4, §10.5, §11.3; contracts#model-census |

CCR-0135, 0139, 0146 and 0147 already had a final status (0135: A12), as did 0001-0094 (A7, A8,
A10). Every file in `docs/port/ccr/` now carries one. The A8 "Needs code" list is CCR2's record
of 2026-11-10. CCRDOC re-checked only the items that belong to the CCRs above (0043, 0044, 0088;
item 11's two clauses noted there). The other items of CCRs 0014-0094 keep the status CCR2, ORC-X2
(A9) or RETYPE (A10) gave them.

**The UI boundary as built.** The ui doc §10.2 rule "pob-app names model and calc types only
through `model.rs`" does not hold: about 48 pob-app files name `pob_calc::…` paths directly, each
marked with the CCR that asked for the re-export (ui doc §10.2 now says so). No CCR of this batch
changed a frozen calc type: CCR-0105, 0113 and 0145 §4 kept their Rust-only `extra` keys or
markers (contracts#model-census lists them).
