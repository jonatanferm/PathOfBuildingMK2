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
  one LuaJIT oracle machine-wide and export the env above).
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
    calc doc §4.4 table states the rule these sites break.
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
    Shared<LjIntMap<Table>>` (CCR-0072 §4); `ActiveSkill.summonSkill` as `Weak` (CCR-0040 §7).

16. **Test-input policy** (CCR-0031 B): `crates/pob-oracle/tests/{BM-A,CO-C,DF-C,DF-D,DF-E,DF-G,MI,OF-D,PF-A,PF-C,PF-D,PF-E,PF-F,PF-G,PS-B,PS-C,SK-A,SK-B,TR-A,TR-B}.rs`
    still skip a missing input unless `POB_ORACLE_REQUIRE=1`; switch them to `pob_testpaths::require`.
    Two more name the variable: `tests/DF-F.rs:53` reads it as a strictness flag (pending hits count
    as failures; keep `DFF_STRICT`/`CI` and drop the retired name) and `tests/CS1.rs:33` mentions it in
    its module doc.
17. **`known_diffs.toml` entries for accepted divergences** (owners add them with the keys their
    diff tool prints, `docs/port/known_diffs.toml` format): CCR-0014 §2 (MS-A: `GetStat`
    `ManaReservedPercent`/`LifeReservedPercent` on the enemy store, PoB raises at
    ModStore.lua:322/336 on the nil `activeSkillList`, Rust returns 0; MS-A's test counts them);
    CCR-0015 §3 (MS-B: the enemy `itemList` and `GuardianRelicAll.monsterTags` rows while E3 is
    open; the `activeSkillList` row is CCR-0014 §2's); CCR-0015 §1 (U40: the `divVar` tag text of
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
