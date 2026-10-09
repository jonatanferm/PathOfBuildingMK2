# pob-calc

CRATE crates/pob-calc (9,924 lines Rust incl. tests; Cargo feature `luajit` = default, pulls mlua 0.10 vendored LuaJIT + ureq; no unsafe). Its Cargo description still says "Faithful Rust port of Path of Building's calculation pipeline", but that plan was dropped on 2026-09-30 (docs/parity-plan.md). Today the crate mostly embeds and hosts PoB's own Lua. It splits into three groups.

(A) Native Rust mod model. It is NOT used at runtime anywhere: grep finds ModDb/mod_store referenced only in lib.rs and tests/modcache_oracle.rs. All three files came from one commit, 13caf9c (2026-09-29), and have barely changed since.
- src/flags.rs (179 lines): ModFlag / KeywordFlag bit constants copied exactly from Data/Global.lua, plus name lookup and MatchKeywordFlags. Complete and correct. 1 unit test.
- src/modifier.rs (727 lines): typed form of a modLib.createMod table.
  - `ModType` enum: BASE/INC/MORE/FLAG/OVERRIDE/LIST/MAX/MIN, plus "Flag" (CanNotUseItem), CHANCE and DUMMY.
  - `Value` enum: Nil/Bool/Num/Str/Mod(Box)/Table(Box<Table{fields: Vec<(String,Value)>, array}>), so it can round-trip every Lua value PoB stores.
  - `TagType` enum: all 26 PoB tag types plus Other(String).
  - `Tag` struct: ~70 flattened Option fields (var, varList, limit*, threshold*, ramp, itemSlot, the cond fields, skill* fields, globalLimit/Key, effect*, unscalable, …), plus `extra` for anything else.
  - `Mod` struct: {name, ty, value, flags, keyword_flags, source, tags: SmallVec<[Tag;1]>, replaced, converted}, with builder helpers, set_source and source_category.
  - Display impls exist; they are not byte-identical to modLib.formatMod.
  - Quality: faithful and complete as a data model. tests/modcache_oracle.rs shows it round-trips all 23,207 Data/ModCache.lua entries. Weakness: Tag is very large and gets cloned per Mod, so memory and performance are poor for heatmap-scale use.
- src/mod_store.rs (1,433 lines): port of Classes/ModStore.lua (971) + ModDB.lua (370) + ModList.lua (249).
  - Design: `ModDb` {mods: AHashMap<String, Vec<Mod>>, conditions, multipliers, parent: Option<DbId>, actor: ActorId}; `ModList` {mods: Vec<Mod>, …}; `Store<'s>` enum {Db, List} as the query context.
  - Every query takes `&dyn EvalEnv` (traits EvalEnv/ActorView/ItemView/WeaponView). These resolve parents (DbId), actors (getActor player/enemy/parent/minion), outputs, items, weapons, monster tags, highPrecisionMods and gem lookups. The design is clean and testable.
  - Implemented: Sum, More (per-name rounding plus highPrecision floor, exactly as ModDB.MoreInternal), Flag, Override/override_num, List, Tabulate (same keep rule), Max, Min, HasMod, GetCondition, GetMultiplier, GetStat (special cases delegated to ActorView::special_stat), and EvalMod. EvalMod covers all 21 tag branches of ModStore.lua: Multiplier, MultiplierThreshold, PerStat, PercentStat, StatThreshold, DistanceRamp, MeleeProximity, Limit, Condition (incl. the Varunastra allOneH quirk), ActorCondition, ItemCondition, SocketedIn, SkillName, SkillId, SkillPart, SkillType, BaseFlag, SlotName, ModFlagOr, KeywordFlagAnd, MonsterTag, plus global limits.
  - Mutators: add_mod/add_list/add_db/new_mod/replace_mod/convert_mod.
  - I spot-checked Condition, ActorCondition, Sum, More, Max/Min, Tabulate and GetCondition/GetMultiplier line by line against the Lua; they match.
  - MISSING:
    - ScaleAddMod / ScaleAddList: these do aura and buff effect scaling and are critical for DPS.
    - CopyList, Combine, ModList MergeMod/MergeNewMod, Print.
    - modLib.compareModParams / parseTags / formatMod / mergeKeystones.
  - DIVERGENCES FOUND:
    1. Store::sum/more/tabulate allocate and pass globalLimits for ModList levels too. Lua ModList.SumInternal/MoreInternal/TabulateInternal call EvalMod(mod, cfg) with globalLimits = nil, so globalLimit caps should NOT apply to ModList stores (e.g. skillModList).
    2. replace_mod (ModDb and ModList) and convert_mod never recurse into the parent. Lua ReplaceModInternal/ConvertModInternal do recurse before falling back to AddMod.
    3. Name strings are built per query (format!("Condition:{var}")). Lua memoises them. Allocation cost only.
  - Tests: 4 small unit tests with a mock EvalEnv. It has never been validated against the oracle at query or EvalMod level.
  - Verdict: a solid, reusable skeleton for the Rust calc's ModDB layer. It needs the missing methods, the 2 divergence fixes, perf work (intern names, slimmer Tag) and an oracle test.

(B) Lua-execution machinery. Under the new decision it is oracle-only or should be removed.
- src/lua/ (≈4.5k lines: interp 757, parser 919, stdlib 861, value 726, host 747, lexer 383, mod 111): a pure-Rust tree-walking Lua 5.1/LuaJIT-dialect interpreter. It boots all of PoB (metatables, goto, +=, bit, io, require). The doc comment in mod.rs ("no metatables, goto") is stale. Used as the wasm engine.
- src/lua_pattern.rs (759 lines): exact port of lstrlib.c's matcher (find/match/gmatch/gsub, %b, %f, back-references), 4 tests. Highly reusable for a hand-port of ModParser and other pattern-driven parsers (item text, mod ranges) without Lua.
- src/mod_parser.rs (459 lines): runs PoB's Modules/ModParser.lua (7,021 lines) unchanged inside src/lua. The only patch replaces `scan` with a native version (`native_scan`: earliest, then longest match, then longest pattern). It needs data/lua/modparser_env.lua (824 KB), dumped by `pob_diff --dump-modparser-env`.
  - lua_to_value/lua_to_tag/lua_to_mod convert Lua mod tables to the typed Mod. They use the Rust interpreter's Val, not mlua.
  - Oracle tests/modcache_oracle.rs: 23,204 / 23,207 ModCache lines identical (the other 3 are stale cache entries).
  - This is Lua at runtime, so it conflicts with the no-Lua decision. ModParser must be hand-ported, or replaced by a generated cache plus a port for uncached lines.
- src/headless.rs (693 lines):
  - `trait Engine { run(code) -> Vec<Ret>; load_build_xml(xml, name); player_output() -> Outputs }`.
  - `Outputs { numbers: AHashMap<String,f64>, flags, strings }`; minion keys get a "Minion" prefix.
  - `Pob` boots PoB like HeadlessWrapper.lua: wrapper prefix up to dofile("Launch.lua"), native Inflate/Deflate/NewFileSearch, Launch.lua, then BOOT_SCRIPT (OnInit plus the first OnFrame without LoadItems, and __mk2_* UI helper Lua).
  - LOAD_SCRIPT calls main:SetMode("BUILD", false, name, xml), then OnFrame. OUTPUT_SCRIPT reads calcsTab.mainEnv (or calcsEnv) .player.output and .minion.output.
- src/jit.rs (547 lines): `JitPob::boot(src)`, the same boot under mlua LuaJIT (5–10× faster), plus PRELUDE rooting io/dofile/require at src/, OAuth listener, OpenURL and DownloadPage over HTTP. It implements Engine. This is the oracle to keep.
- src/worker.rs (356 lines): `PobHandle::spawn(src)` starts a thread with a 256 MB stack. Commands: LoadXml / NewBuild / Lua / Refresh / Query. Events: Booted / BootFailed / Snapshot / QueryResult. Snapshot carries xml, outputs, sidebar, warnings, point display, skill select, sets and popup. It picks JitPob unless MK2_POB_ENGINE=rust. This is the current app's UI-to-Lua bridge and is irrelevant to a pure Rust port, apart from being a model for a UI/calc thread split.

(C) Parity tooling, in crates/pob-extract/src/bin (not in pob-calc):
- pob_headless.rs (~95 lines). The best per-key oracle dumper.
  - Default engine is the Rust interpreter; POB_ENGINE=jit uses LuaJIT.
  - POB_ROOT defaults to .PathOfBuilding, so run it from the repo root.
  - It boots once and loads each XML argument in turn.
  - HEADLESS_JSON_OUT=DIR writes DIR/<stem>.json as a flat {key: number} map (finite numbers only, player keys plus Minion-prefixed keys; bools and strings dropped).
  - HEADLESS_EXEC=lua runs arbitrary Lua after load (`build` is bound) and prints the returns. This is how to dump intermediate state such as the modDB, skill cfgs or breakdowns.
  - HEADLESS_RECALC forces BuildOutput.
  - Verified: OccVortex gives boot 2.1 s, load 1.8 s, Life 6728, TotalDPS 211560.48, 744 keys. allflame_exp_000 gives 702 keys.
- pob_diff.rs (1,340 lines). A separate mlua sandbox with its own host shims (~300 lines duplicating jit.rs). It diffs PoB env.player.output against the OLD hand-written crates/pob-engine (≈50k lines; heuristic, ~53% parity, slated for deletion) via pob_engine::import_pob_xml + compute_with_data.
  - --json-out DIR writes {"build", "pob": {numbers, bools, strings}, "ours": {numbers}}.
  - --explain STAT[,…] prints PoB's modDB:Tabulate rows for a stat (source, value, tags, flags), next to pob-engine's.
  - POB_DIFF_EXEC runs arbitrary Lua; POB_DIFF_TIME times BuildOutput; --dump-modparser-env PATH writes the ModParser env.
  - Tolerance close_enough: |d| < 0.5 or ≤ 1% relative.
- scripts/parity:
  - run.sh: builds pob_diff, runs it in parallel over .parity/builds/*.xml into .parity/results, then calls report.py.
  - report.py: reads {pob, ours} JSONs. Prints headline-stat match counts, overall shared-key match %, the most divergent keys and the missing keys. --key K gives a per-build view; --top N. ALIASES Str→Strength etc. exist only for pob-engine naming.
  - fetch_ninja.py [league] [kind] [n]: pulls poe.ninja characters' PoB exports. pb.py is a protobuf helper for it.

CORPUS (gitignored .parity/):
- builds/: 52 allflame_exp_NNN.xml (3.29 poe.ninja) plus 5 upstream_*.xml (PoB spec TestBuilds 3.13).
- headless/*.json: flat oracle maps, ~700 keys per build.
- results/ and results_seq/: pob_diff JSONs.
- Scoreboard in docs/parity-plan.md: embedded PoB vs LuaJIT oracle = 100% (38,384 keys); old pob-engine = 53.3%.
- PoB's own spec/TestBuilds/3.13/*.lua expected outputs are STALE (OccVortex TotalDPS 46997 there vs 211560 live). Do not use them; use the live oracle.

HOW TO COMPARE A NEW RUST CALC PER KEY:
1. Run the LuaJIT oracle via pob_headless (or pob_calc::jit::JitPob + Engine::player_output in a Rust test) to get {key: f64} per build.
2. Run the Rust calc on the same XML to get its output table.
3. Write {"build": stem, "pob": oracle, "ours": rust} into one dir and run scripts/parity/report.py on it (same 1%/0.5 rule).

Better long term: a new bin or test (e.g. pob_calc_diff) that links JitPob as a dev-dependency, loads each build in its own process or a fresh boot, and diffs per key with the same tolerance. Plus stage-level oracles via HEADLESS_EXEC: dump env.player.modDB.mods, skill lists and per-skill output after initEnv/perform, convert them through an mlua→Mod converter (still to be written; lua_to_mod only takes the Rust interpreter's Val) and compare ModDB contents and EvalMod results before comparing final outputs.

## reusable
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-calc/src/flags.rs — ModFlag/KeywordFlag constants exactly as in Global.lua + match_keyword_flags; use as is
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-calc/src/modifier.rs — Mod/ModType/Value/Table/Tag/TagType/Sockets typed mod model; proven complete against all ModCache.lua entries; keep, maybe slim Tag and intern names
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-calc/src/mod_store.rs — ModDb/ModList/Store + EvalEnv/ActorView/ItemView/WeaponView traits; Sum/More/Flag/Override/List/Tabulate/Max/Min/HasMod/GetCondition/GetMultiplier/GetStat/EvalMod with all 21 tag branches; base for the Rust ModDB layer once the gaps below are fixed
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-calc/src/lua_pattern.rs — exact Lua 5.1 pattern engine (find/matches/gmatch/gsub/gsub_str/match1/to_number); key enabler for hand-porting ModParser.lua and item parsing with the pattern tables copied over unchanged
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-calc/src/mod_parser.rs native_scan() (lines ~117-180) — native version of ModParser's scan (earliest, then longest match, then longest pattern); lua_to_mod/lua_to_tag/lua_to_value (lines 298-459) show the Lua-table→Mod field mapping to mirror in an mlua converter
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-calc/src/jit.rs JitPob::boot(&Path) + impl Engine — the LuaJIT test oracle; keep, behind a dev or feature gate
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-calc/src/headless.rs — trait Engine {run, load_build_xml, player_output}, struct Outputs {numbers, flags, strings}, LOAD_SCRIPT / OUTPUT_SCRIPT / BOOT_SCRIPT constants (oracle driving recipe)
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-calc/tests/modcache_oracle.rs — template for oracle tests (value_close / mod_close tolerance helpers; self-skips without a PoB checkout)
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-extract/src/bin/pob_headless.rs — oracle dumper (POB_ENGINE=jit, HEADLESS_JSON_OUT, HEADLESS_EXEC)
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-extract/src/bin/pob_diff.rs explain_pob() (line 771) — Lua snippet that dumps modDB:Tabulate for a stat; reuse for stage debugging. diff/report printing (lines 522-770) reusable after swapping pob_engine for the new calc
- /home/ferm/Work/projects/PathOfBuildingMK2/scripts/parity/report.py — scoreboard over {build,pob,ours} JSON files (1% / 0.5 tolerance); engine-agnostic
- /home/ferm/Work/projects/PathOfBuildingMK2/scripts/parity/fetch_ninja.py + pb.py — corpus fetcher
- /home/ferm/Work/projects/PathOfBuildingMK2/.parity/builds/*.xml (57 builds) and .parity/headless/*.json (flat oracle output maps, ~700 keys each) — ready-made corpus and baseline (gitignored, local only)
- /home/ferm/Work/projects/PathOfBuildingMK2/.PathOfBuilding/src/Data/ModCache.lua — PoB's precomputed line→mods cache (23,207 lines); a Rust port could load or convert it to skip ModParser for most known item/tree lines
- /home/ferm/Work/projects/PathOfBuildingMK2/data/lua/modparser_env.lua — dumped slice of PoB's data table that ModParser needs (gems, skills, keystones, cluster jewels)

## discard
- crates/pob-calc/src/lua/ (interp, parser, lexer, stdlib, value, host) — runtime Lua interpreter; not needed under the no-Lua decision (only as an optional wasm oracle)
- crates/pob-calc/src/worker.rs — UI to Lua bridge (Snapshot/sidebar/__mk2_* helpers); replaced by a native calc + UI
- crates/pob-calc/src/headless.rs BOOT_SCRIPT __mk2_* UI helpers (__mk2_json, __mk2_ctl, __mk2_tooltip, __mk2_sets, __mk2_popup) — app glue for the Lua-driven UI
- crates/pob-calc/src/jit.rs OAuth listener / OpenURL / DownloadPage plumbing — app features, not oracle
- crates/pob-extract/src/bin/pob_diff.rs diff_against_pob_engine/explain_ours and its separate mlua sandbox (build_lua_sandbox, install_host_shims, install_bit) — tied to the old pob-engine and duplicates jit.rs
- scripts/parity/report.py ALIASES (Str→Strength etc.) — only needed for old pob-engine key names
- .PathOfBuilding/spec/TestBuilds/3.13/*.lua expected outputs — stale

## workUnits
- **Mod model (keep)** [crates/pob-calc/src/flags.rs, crates/pob-calc/src/modifier.rs; PoB: Data/Global.lua, Modules/ModTools.lua] deps=none :: Already faithful. Optional: intern mod names (u32 ids), slim Tag into an enum of per-type payloads, add modLib.formatMod-compatible formatting and compareModParams.
- **ModStore/ModDB/ModList completion + fixes** [crates/pob-calc/src/mod_store.rs; PoB: Classes/ModStore.lua (971), Classes/ModDB.lua (370), Classes/ModList.lua (249)] deps=Mod model :: Add ScaleAddMod/ScaleAddList/CopyList/Combine/MergeMod/MergeNewMod. Fix the ModList globalLimits divergence and the parent recursion in Replace/ConvertModInternal. Add an oracle test that dumps env.player.modDB (and skillModList) per build via JitPob, converts to Mod, rebuilds the Rust ModDb, and compares Sum/More/Flag/EvalMod for every stat name.
- **ModParser without Lua** [.PathOfBuilding/src/Modules/ModParser.lua (7021), Data/ModCache.lua; crates/pob-calc/src/lua_pattern.rs, mod_parser.rs (native_scan)] deps=Mod model, lua_pattern :: Options: (a) hand-port the pattern tables to Rust using lua_pattern for exact matching; (b) build-time cache generated by the oracle for all known lines (tree/uniques/bases/gems) plus (a) for custom mods. Keep the tests/modcache_oracle.rs gate (23,204/23,207 today with the Lua route).
- **Oracle harness for the Rust calc** [crates/pob-calc/src/jit.rs, headless.rs (Engine/Outputs/LOAD_SCRIPT/OUTPUT_SCRIPT), crates/pob-extract/src/bin/pob_headless.rs, pob_diff.rs explain_pob, scripts/parity/report.py, .parity/builds] deps=LuaJIT oracle only (dev-dep / feature) :: Move JitPob into an oracle-only crate or feature. New diff bin/test: per build, fresh oracle boot → Outputs; Rust calc → map; diff with close_enough (0.5 abs / 1% rel); write {build,pob,ours} so report.py works unchanged. Add stage dumps via Engine::run Lua snippets (modDB after initEnv, active skill list, per-skill output) and an mlua→Mod converter mirroring mod_parser::lua_to_mod.
- **Retire Lua runtime + old engine** [crates/pob-calc/src/lua/*, worker.rs, mod_parser.rs, headless.rs Pob; crates/pob-engine (~50k lines); pob_diff's pob_engine coupling] deps=Rust calc reaching parity :: The Rust Lua interpreter (~4.5k lines) is currently the wasm engine and the ModParser host. Under the new decision keep only lua_pattern.rs. pob-engine is the old heuristic engine (53% parity) and is unrelated to a faithful port.

## risks
- mod_store.rs is not validated against the oracle: only 4 mock unit tests, no use anywhere, unchanged since its single commit 13caf9c. Treat it as unverified.
- mod_store.rs divergence 1: Store::sum/more/tabulate pass globalLimits for ModList levels. PoB's ModList.SumInternal/MoreInternal/TabulateInternal call EvalMod without globalLimits, so caps are wrongly applied on skillModList-type stores.
- mod_store.rs divergence 2: ModDb::replace_mod/convert_mod and ModList::replace_mod never recurse into the parent store. PoB's ReplaceModInternal/ConvertModInternal do recurse before adding.
- mod_store.rs is missing ScaleAddMod/ScaleAddList, which scales aura/buff/curse effect with highPrecision and defaultHighPrecision rounding and the keyOfScaledMod handling. Without it, buff-heavy DPS will be wrong by far more than a few %. Also missing: CopyList, Combine, MergeMod/MergeNewMod (ModList), Print, modLib.compareModParams/parseTags/formatMod/mergeKeystones.
- Performance: Tag has ~70 fields and is cloned with every Mod. Names are String keys in AHashMap and a format! runs per GetCondition/GetMultiplier. All queries go through dyn traits. Likely too slow for the per-node power heatmap; intern names and slim Tag.
- mod_parser.rs runs PoB's ModParser.lua in the embedded Lua interpreter, which violates the no-Lua-at-runtime decision. ModParser.lua is 7,021 lines of pattern tables and closures that PoB edits every league. A hand port drifts unless it is kept checked by the ModCache oracle test. Also consider a build-time generated cache of every known mod line plus a ported parser for custom or unknown lines.
- PoB oracle cross-build state leak: loading build A before B changes B's output in real PoB too (doc: corpus 031→051 mana reservation). pob_headless and pob_diff load builds in sequence in one boot, so for exact comparison use one build per process or a fixed order.
- PoB's spec/TestBuilds/3.13/*.lua expected outputs are stale (OccVortex TotalDPS 46997 there vs 211560 live); use the live LuaJIT oracle only.
- Some corpus builds have TotalDPS=0 or FullDPS=0 (e.g. allflame_exp_000, and FullDPS=0 on OccVortex), so DPS accuracy needs a curated subset or a per-skill output dump (e.g. via HEADLESS_EXEC over calcsTab skill outputs), not just mainOutput.
- pob_diff is hard-wired to the old pob-engine crate (≈50k lines, ~53% parity) and duplicates jit.rs boot shims in its own mlua sandbox. scripts/parity/run.sh therefore measures the OLD engine. A new diff driver is needed for the Rust port.
- The pob-calc crate enables the `luajit` feature by default (mlua vendored LuaJIT + ureq). If the oracle stays in pob-calc, the shipped Rust calc crate pulls LuaJIT. The oracle should move to a dev-dependency or separate crate.
- lua/mod.rs doc comment is stale (says no metatables or goto). The interpreter needs a 256 MB thread stack because it is a recursive tree-walker.
- Oracle output only captures scalar number/bool/string keys of env.player.output and env.minion.output. Nested tables (breakdowns, per-skill/per-element sub-tables, enemy output) are not captured unless dumped explicitly.

## commands
- cd /home/ferm/Work/projects/PathOfBuildingMK2 && cargo build --release -p pob-extract --bin pob_headless --bin pob_diff
- cd /home/ferm/Work/projects/PathOfBuildingMK2 && POB_ENGINE=jit ./target/release/pob_headless .parity/builds/upstream_OccVortex.xml   # boot 2.1s, prints Life/ES/Str/FireRes/TotalDPS/FullDPS + key count
- cd /home/ferm/Work/projects/PathOfBuildingMK2 && POB_ENGINE=jit HEADLESS_JSON_OUT=.parity/headless ./target/release/pob_headless .parity/builds/*.xml   # flat {key:number} oracle maps per build
- cd /home/ferm/Work/projects/PathOfBuildingMK2 && POB_ENGINE=jit HEADLESS_EXEC='local e=build.calcsTab.mainEnv; local n=0; for k,v in pairs(e.player.modDB.mods) do n=n+#v end; return "MODS="..n, "DPS="..tostring(build.calcsTab.mainOutput.TotalDPS)' ./target/release/pob_headless .parity/builds/upstream_OccVortex.xml 2>/dev/null | grep -E 'MODS|DPS='   # arbitrary oracle introspection (verified: MODS=613 DPS=211560.47733333)
- POB_ROOT=/path/to/PathOfBuilding ./target/release/pob_headless X.xml   # override PoB checkout (default .PathOfBuilding relative to cwd)
- cd /home/ferm/Work/projects/PathOfBuildingMK2 && ./target/release/pob_diff --build .parity/builds/upstream_OccVortex.xml --explain Life,Str --json-out /tmp/out   # PoB modDB:Tabulate per stat; diffs vs OLD pob-engine
- cd /home/ferm/Work/projects/PathOfBuildingMK2 && ./target/release/pob_diff --dump-modparser-env data/lua/modparser_env.lua
- cd /home/ferm/Work/projects/PathOfBuildingMK2 && scripts/parity/run.sh   # JOBS=N; pob_diff over corpus -> .parity/results + scoreboard (old engine)
- cd /home/ferm/Work/projects/PathOfBuildingMK2 && python3 scripts/parity/report.py .parity/results --key TotalDPS --top 40
- cd /home/ferm/Work/projects/PathOfBuildingMK2 && python3 scripts/parity/fetch_ninja.py allflame exp 100   # refresh corpus into .parity/builds
- cd /home/ferm/Work/projects/PathOfBuildingMK2 && cargo test -p pob-calc   # unit tests + modparser_smoke + worker_smoke (asserts OccVortex Life==6728) + modcache_oracle
- cd /home/ferm/Work/projects/PathOfBuildingMK2 && ORACLE_LIMIT=2000 cargo test -p pob-calc --release --test modcache_oracle -- --nocapture   # ModCache.lua line-by-line parser oracle (>=99.9% required)
- MK2_POB_ENGINE=rust <app>   # force worker onto the pure-Rust Lua interpreter instead of LuaJIT