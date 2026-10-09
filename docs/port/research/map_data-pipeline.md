# data-pipeline

DATA PIPELINE MAP (read-only survey, PoB upstream 2.67.2 at .PathOfBuilding, HEAD 16de4b82d)

1) crates/pob-extract (~4.9k lines Rust, build-time CLI, mlua 0.10 with features luajit+vendored)
- src/main.rs (807 lines): orchestrates every extractor, writes data/*.json. make_lua() builds a sandbox. It stubs LoadModule/ConPrintf, provides a bit.* shim, and HARDCODES SkillType/KeywordFlag/ModFlag tables (lines 374-591). It does not dofile Data/Global.lua. lua_to_json() turns Lua values into serde_json. Functions become the sentinel {"__lua_function":true}, which carries no id. Mixed or holey int-keyed tables become string-keyed objects ("1","2",...). Empty tables become {}. call_pob_data_chunk handles both the old varargs form and the current `return function(...)` wrapper. copy_pob_runtime() mirrors PoB's src/*.lua, runtime/lua and manifest.xml into data/pob (377 files, 418 MB) so the app can run PoB headless.
- Per-dataset modules, each hand-mapped to snake_case typed structs:
  - bases.rs (151 lines) <- Data/Bases/*.lua
  - gems.rs (50) <- Data/Gems.lua
  - skills.rs (160) <- Data/Skills/*.lua. It re-implements mod/flag/skill as Rust closures that add an extra `__kind` field, so the output is not PoB's shape.
  - minions.rs (315) <- Data/Minions.lua and Spectres.lua
  - tattoos.rs (195) <- TattooPassives.lua
  - enchants.rs (139) <- Enchantment*.lua
  - cluster_jewels.rs (287) and cluster_jewel_mods.rs (227) <- ClusterJewels.lua and ModJewelCluster.lua
  - calc_sections.rs (408) <- Modules/CalcSections.lua. It is lossy: it keeps only the first {N:output:Key} per row and drops matrix formats.
  - timeless_legion.rs (272) <- TimelessJewelData/LegionPassives.lua
  - timeless_nodes.rs (521) <- NodeIndexMapping.lua
  - tree.rs (374) <- TreeData/<v>/tree.lua. It is lossy: it drops sprites/assets and skips legacy 2_6 and 3_6–3_9.
  - lua_value.rs (96): JSON accessors
  - cli.rs (44): --pob (default .PathOfBuilding), --out (default data)
- bin/pob_diff.rs (1341 lines): drives real PoB under mlua and diffs it against the old pob-engine crate. `--dump-modparser-env PATH` runs a Lua serializer (const DUMP_MODPARSER_ENV, around line 190) over PoB's loaded `data` (skills, gems, gemForBaseName, keystones, clusterJewels, ...). This is the only existing precedent for "serialize PoB's loaded data table from inside PoB": it sorts keys, emits shared refs as ids, and caps depth at 6. It generated data/lua/modparser_env.lua (808 KB).
- bin/pob_headless.rs (87): boots PoB (POB_ENGINE=jit uses pob_calc::jit::JitPob, otherwise the pure-Rust interpreter) and prints outputs. bin/lua_bench.rs is tiny.

2) crates/pob-data (~5.6k lines; serde types plus loaders, no fs except disk.rs)
- lib.rs (68) re-exports everything.
- load.rs: load_bases, load_gems, load_skill_file, load_passive_tree.
- disk.rs (131): GameData::load(root) reads skills/*.json, bases, cluster_jewels, cluster_jewel_mods, timeless_*, legion_*, minions. Missing files silently become None. Also provides timeless_lut_compressed() and timeless_lut_stem().
- skill.rs (276): the Skill struct is loose. levels, statMap, qualityStats and constantStats are serde_json::Value. There are no fields for parts, preDamageFunc, baseMods, statDescriptionScope, etc. (serde silently ignores them). It uses lenient deserializers to work around lua_to_json's array/map ambiguity.
- item.rs (1021): item paste parsing for the old UI.
- bases.rs (158), gem.rs, minions.rs (133), tattoos.rs (115), enchants.rs (371), cluster_jewels.rs (123), cluster_jewel_mods.rs (104), calc_sections.rs (92), sprites.rs (68).
- tree.rs (224): PassiveTree, Node, Group, Class, constants. No sprite/asset data.
- timeless_jewels.rs (1330): the keystone table plus legion/seed logic.
- Hand-copied constants that will drift: flags.rs (468; SkillType/KeywordFlag/ModFlag), monster_tables.rs (289; static arrays copied from Misc.lua), jewel_radius.rs (162; copied from Data.lua jewelRadii).
- tests/round_trip.rs (145) loads the extracted JSON and skips if it is absent.
- Consumers: pob-ui (items_tab, lib, calcs_tab, tree_view, ...), pob-engine (50k-line older hand-port), pob-extract.

3) data/ directory (642 MB; /data is gitignored but some files are force-tracked)
- Top-level JSON:
  - bases.json (911 KB): 1106 bases with type, sub_type, socket_limit, tags, influence_tags, implicit text, implicit_mod_types, req, and weapon/armour/flask blocks
  - gems.json (462 KB): 822 gems
  - minions.json (375 KB): minions and spectres with modList as `__kind` mod tables
  - cluster_jewel_mods.json (413 KB): 557 mods
  - cluster_jewels.json (36 KB)
  - tattoos.json (118 KB)
  - enchants_{helmet,gloves,boots,body,belt,weapon,flask}.json
  - calc_sections.json (126 KB): 30 sections
  - legion_passives.json (147 KB): 337 entries
  - legion_nodes.json (159 KB): 381 entries
  - timeless_node_index.json (164 KB)
  - timeless_jewels.json (9 KB): hand-written on 9 May, keystones only
- data/skills/*.json (5.7 MB): 10 files plus index.json, 1526 granted effects total. 134 `__lua_function` sentinels: act_int 68, act_dex 42, act_str 15, other 5, minion 2, spectre 1, sup_str 1. data/skills/skills is a broken symlink to /Users/jferm/... (macOS path).
- data/trees/*.json (43 MB): 36 versions, 3_10 to 3_29 including ruthless and alternate.
- data/sprites/3_25(.json): 3_25 only. Its generator tools/sprites_to_json.lua does not exist in the repo.
- data/timeless/*.zip(.partN) (170 MB): verbatim copies of the LUTs.
- data/lua/modparser_env.lua.
- data/pob/: a full copy of PoB src+runtime used by the current Lua-at-runtime app (identical apart from .bin, Assets and Export).
- Tracked in git: bases.json, calc_sections.json, legion_*.json, timeless_jewels.json, timeless_node_index.json, lua/modparser_env.lua, sprites/3_25*, timeless zips, trees/3_25.json. Everything else is regenerated by `cargo run -p pob-extract`.

QUALITY VERDICT
The existing extractor is a selective, lossy, hand-mapped JSON pipeline built for the old app. It is not a faithful dump.
- Critical bug: the SkillType numbering is stale. Current Global.lua has 135 entries ending CreatesCorpse=133, RequiresStaff=134, Pact=135, with the Removed* slots compacted. pob-extract/main.rs and pob-data/flags.rs use the old 146-entry numbering (Area=11 vs PoB 8, Aura=43 vs 39, Pact=143 vs 135, ...). So every numeric skillTypes, addSkillTypes and excludeSkillTypes key in data/skills/*.json, and in the old Rust flags, disagrees with PoB. ModFlag and KeywordFlag match.
- Closures are dropped without identity.
- Typed structs ignore most of the skill schema.
- Several tables are hand-copied constants.
- Reusable: the mlua harness pattern (make_lua, call_pob_data_chunk, list_files), the tree and timeless extractors' logic, the zip/LUT handling, and above all the pob_diff Lua-side serializer idea plus pob-calc's jit.rs (JitPob::boot), which already boots all of PoB, including Modules/Data.lua, under vendored LuaJIT in-process.
- For a faithful port, replace the pipeline with a "run PoB's Data.lua, serialize the resulting `data` table" dumper rather than extending the per-file extractors.

PoB DATA SOURCES (.PathOfBuilding/src/Data, about 817k Lua lines)
- Plain data, safe to dump directly:
  - Gems.lua (16k lines, 822 gems)
  - Bases/*.lua (22 files; only the `return function(itemBases)` wrapper)
  - Uniques/*.lua (22 files of raw item-text strings; need the Item parser and ModParser at runtime)
  - Uniques/Special/BoundByDestiny, WatchersEye, race, New
  - Mod*.lua item affix tables: ModExplicit 2.0 MB, ModItemExclusive 3.1 MB, ModEldritch 1.9 MB, ModScalability 1.4 MB, ModJewel*, ModFlask, ModCorrupted, ModMaster, ModVeiled, ModSynthesis, ModScourge, ModFoulborn(+Map.jsonc), ModGraft, ModTincture, ModNecropolis, ModMercenary. Each entry is a stat text line plus type, affix, level, group, weightKey/Val, modTags, statOrder.
  - Misc.lua (monster tables, game/character/monster constants)
  - Global.lua (enums plus colorCodes; 5 helper functions: updateColorCode, hexToRGB, MatchKeywordFlags, ...)
  - StatDescriptions/*.lua (22 files, about 4.4 MB for stat_descriptions alone; plain tables with named numeric handlers `k="divide_by_one_hundred"` implemented in Modules/StatDescriber.lua, 299 lines)
  - ClusterJewels, TattooPassives, EnchantmentX, Essence, Pantheons, Bosses, BossSkills, Costs (a metatable alias is added in Data.lua), Rares, Crucible, BeastCraft, Vestigial, FlavourText, CurrencyNames, QueryMods and TradeSiteStats (trade UI only)
  - TimelessJewelData: LegionPassives, NodeIndexMapping, AbyssNotableNames, LegionTradeIds, plus binary zip/part LUTs
  - TreeData/<v>/tree.lua and sprites.lua: pure data, 2.8 MB and 0.7 MB for 3_28, plus png/jpg/webp atlases
- Data built through helper calls (mod/flag/skill constructors), which evaluate to plain tables:
  - Skills/*.lua: 10 files, 1526 skills, about 793 statMap blocks, about 163 skills with `parts`
  - SkillStatMap.lua: 716 global stat→mod entries with div/mult/base/value fields and nested `{mod=...}` values
  - Minions.lua: 64 minions
  - Spectres.lua: 268 spectres
- ModCache.lua (23,207 entries, 4.1 MB): PoB's own memoized ModParser output, keyed by exact text line. 20,393 parse to mod lists (709 of them empty) and 2,814 are unparsed (nil). Main:SaveModCache explicitly excludes JewelFunc and ExtraJewelFunc mods because those contain closures. Everything else is plain data and an excellent golden oracle; pob-calc/tests/modcache_oracle.rs already uses it.

CLOSURES THAT MUST BE HAND-PORTED TO RUST (counts measured)
- Skills/*.lua: 133 function-valued fields on 130 granted effects (8.5% of 1526). By hook:
  - preDamageFunc: 122
  - initialFunc: 4
  - preSkillTypeFunc: 3
  - postCritFunc: 3
  - explosiveArrowFunc: 1
  By file: act_int 68, act_dex 27-41 by regex variant, act_str 15, other 5, minion 2, spectre 1, sup_str 1 (AvengingFlame).
  Size: about 2,050 lines total, 103 distinct bodies (about 1,818 lines) after collapsing AltX/AltY duplicates. Largest: LightningSpireTrap ~120, SeismicTrap 116, BallLightning 82, ExplosiveArrow 80, KineticFusillade postCrit 78, MoltenStrikeAltX 72.
  Call sites in CalcOffence.lua: runSkillFunc(name) at 509-516 with signature (activeSkill, output, breakdown), invoked at 516, 1054, 1908 and 3315. explosiveArrowFunc is called at 3038 as (activeSkill, output, globalOutput, globalBreakdown, env). Some bodies read GlobalCache (AvengingFlame).
- No other closures in skill data. statMap, parts, levels, baseMods, qualityStats and constantStats are all plain.
- ModMap.lua: 41 `apply = function(val, mapModEffect, values, modList, enemyModList)` closures (map mods, Config tab).
- Uniques/Special/Generated.lua: about 20 functions of generator code that builds unique item text from cluster/notable data. Run it at extract time and dump the resulting strings. No port needed.
- Modules/Data.lua (1483 lines): 22 `function` occurrences.
  - powerStatList transform lambdas (6) and GetFromOutput
  - setJewelRadiiGlobally
  - skillStatMapMeta.__index: a lazy copy from the global SkillStatMap plus processMod
  - costs __index metatable
  - setupGem: Vaal AltX/AltY gem synthesis, gemForBaseName
  - bossStats aggregation, itemBaseLists sort, printMissingMinionSkills
  These are derived-data computations. Either dump the post-processed result or port the small logic (processMod injects mod.source="Skill:<id>" and adds {type="ActorCondition",actor="parent",neg=true} for notMinionStat).
- Outside Data but data-shaped (Rust code, out of the dump):
  - Modules/ModParser.lua (7021 lines, about 1238 `function(` occurrences): specialModList has about 2099 patterns with 1025 closures, modTagList 130 closures, modNameList/preFlagList about 7, jewel*Funcs 64 (jewelOtherFuncs, jewelSelfFuncs, jewelSelfUnallocFuncs, jewelThresholdFuncs). JewelFunc/ExtraJewelFunc mods carry runtime closures `func(node,out,data)`.
  - Modules/ConfigOptions.lua (2380 lines, 531 `apply = function` closures).
  - Modules/CalcSections.lua (2638 lines) is plain data with zero closures, so dump it raw instead of the lossy calc_sections.rs.

PROPOSED EXTRACT-TIME DUMP (to replace or augment pob-extract)
1. Boot a minimal PoB environment under the vendored LuaJIT already in the workspace (reuse pob_calc::jit::JitPob's rooted io/require/LoadModule shims, or a slimmer boot): dofile Global.lua and Common.lua, then LoadModule("Modules/Data"). That executes Generated.lua, gem Alt synthesis and processMod exactly as PoB does. Separately load each TreeData/<v>/tree.lua and sprites.lua, StatDescriptions/*, ModCache, CalcSections and ConfigOptions (the last for its plain fields).
2. Serialize with a Lua-side walker, extending the pob_diff DUMP_MODPARSER_ENV approach.
   - Deterministic key order.
   - Identity table for shared refs: gem.grantedEffect becomes {"$ref":"skills/<id>"}, statMap._grantedEffect is dropped, gemForSkill is keyed by id.
   - Strip metatables and record which ones existed (statMap → "falls back to skillStatMap").
   - Every function becomes {"$fn":"<dotted path>","src":"<file>:<linedefined>-<lastlinedefined>","hash":"<sha of body text>"}, taken from debug.getinfo(f,"S") plus reading those lines.
   - Numbers: LuaJIT has only doubles, so emit integral values as ints and keep floats with %.17g.
   - Lua tables: emit a canonical LuaTable {arr:[...], map:{...}} or a typed per-dataset schema, so array/map ambiguity never reaches Rust.
3. Emit a closure manifest (path, hook name, src range, body hash). In Rust, a generated `match (skill_id, hook)` registry maps each id to a hand-ported fn, with a build-time or CI check that every manifest id has an impl and its body hash matches the hash the port was written against. That flags upstream edits to closures automatically.
4. Format: serde-typed structs per dataset (GrantedEffect with levels/statMap/parts/baseFlags/skillTypes; Gem; ItemBase; Minion; ModAffix; UniqueText; StatDescription; Misc; Tree) written as JSON for diffability, plus an optional compact binary (postcard/bincode, or rkyv for zero-copy) for runtime. Alternatively embed via include_bytes! for wasm. Take SkillType/ModFlag/KeywordFlag from Global.lua at dump time and generate the Rust enums (build.rs or a codegen step) instead of hardcoding them.
5. Oracles from the same LuaJIT boot:
   - ModCache.lua (23k lines→mods), expanded by running PoB's ModParser over every stat line in Mod*.lua, Uniques, tree nodes (all versions), tattoos and cluster notables.
   - Per-skill post-data snapshots for the calc parity harness.
   Keep LuaJIT strictly in pob-extract and tests, never in the runtime crates.

## reusable
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-calc/src/jit.rs (JitPob::boot: whole PoB under vendored LuaJIT with rooted io/require — basis for the Data.lua dumper)
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-extract/src/bin/pob_diff.rs (~line 190 DUMP_MODPARSER_ENV: Lua-side serializer of loaded `data`, sorted keys, refs-by-id — template for full dump)
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-extract/src/main.rs (make_lua sandbox, call_pob_data_chunk handles `return function(...)` wrapper, list_files, lua_to_json — but replace hardcoded SkillType table)
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-extract/src/tree.rs (tree.lua->typed PassiveTree; extend to keep sprites/assets)
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-extract/src/timeless_nodes.rs + timeless_legion.rs (NodeIndexMapping/LegionPassives extraction, local->global id remap)
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-data/src/disk.rs (GameData::load, timeless_lut_compressed .zip/.partN concat)
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-data/src/timeless_jewels.rs (seed ranges, legion resolve logic, 1330 lines)
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-calc/tests/modcache_oracle.rs (ModCache.lua as golden oracle for a Rust ModParser; float tolerance helpers)
- /home/ferm/Work/projects/PathOfBuildingMK2/.PathOfBuilding/src/Data/ModCache.lua (23,207 line->mods oracle; 20,393 parsed)
- /home/ferm/Work/projects/PathOfBuildingMK2/.PathOfBuilding/src/Modules/Data.lua lines 52-93 (makeSkillMod/makeFlagMod/makeSkillDataMod/processMod — small, port verbatim) and 1030-1160 (skill/gem post-processing)
- /home/ferm/Work/projects/PathOfBuildingMK2/data/timeless/*.zip(.partN) (verbatim LUTs, already shipped)
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-extract/test-builds/*.xml (small test builds)

## discard
- /home/ferm/Work/projects/PathOfBuildingMK2/data/pob (runtime copy of PoB Lua; only needed for oracle/tests once no Lua at runtime)
- Hardcoded skill_type_pairs/keyword_flag_pairs/mod_flag_pairs in crates/pob-extract/src/main.rs (replace with Global.lua-derived codegen)
- Lenient deserializers in crates/pob-data/src/skill.rs (de_lenient_vec/de_lenient_indexmap) once dump uses canonical encoding
- crates/pob-data/src/monster_tables.rs and jewel_radius.rs hand-copied constants (generate from Misc.lua/Data.lua)
- data/timeless_jewels.json (hand-written keystone-only table)
- data/skills/skills broken symlink
- crates/pob-extract/src/calc_sections.rs lossy row flattening (dump CalcSections.lua raw instead)
- crates/pob-extract/src/bin/pob_diff.rs diffing against pob-engine (old hand-port) — keep only the serializer idea

## workUnits
- **pob-dump: LuaJIT data dumper** [.PathOfBuilding/src/Modules/Data.lua, Data/Global.lua, Modules/Common.lua; reuse crates/pob-calc/src/jit.rs boot + pob_diff serializer] deps=mlua luajit (already vendored) :: Boot minimal PoB env, LoadModule Data, walk `data` with ref-ids, metatable notes, $fn placeholders (path + debug.getinfo src range + body hash), canonical LuaTable encoding; emit JSON + optional postcard/rkyv; closure manifest file.
- **Typed data schema crate (pob-data v2) + codegen enums** [Data/Global.lua (SkillType 135, ModFlag, KeywordFlag), Skills/*, Gems.lua, Bases/*, Minions/Spectres, Misc.lua, Mod*.lua, StatDescriptions/*] deps=pob-dump :: GrantedEffect{levels,statMap,parts,baseFlags,skillTypes,baseMods,qualityStats,constantStats,stats,notMinionStat,...}; enums generated from Global.lua; replaces hand-copied flags/monster tables.
- **Skill hook port (closures)** [Data/Skills/act_int.lua (68), act_dex.lua (~27-41), act_str.lua (15), other.lua (5), minion.lua (2), spectre.lua (1), sup_str.lua (1)] deps=Rust calc ActiveSkill/ModStore API; CalcOffence runSkillFunc call sites (lines 516,1054,1908,3038,3315) :: 133 closures / 103 distinct bodies / ~1.8-2.05k Lua lines; hooks: preDamageFunc 122, initialFunc 4, preSkillTypeFunc 3, postCritFunc 3, explosiveArrowFunc 1. Registry keyed (skillId, hook) checked against dump manifest hashes.
- **SkillStatMap + processMod + statMap fallback** [Data/SkillStatMap.lua (716 entries, plain), Modules/Data.lua:52-93, 1030-1080] deps=pob-dump :: Plain data; port lazy fallback + processMod (~60 Lua lines).
- **Data.lua derived tables** [Modules/Data.lua 1083-1483 (setupGem/Alt vaal gems, gemForBaseName, itemBaseLists, bossStats, clusterJewelInfoForNotable, jewelRadii, costs alias, powerStatList)] deps=pob-dump :: Prefer dumping post-processed results; port only powerStatList transforms (6 lambdas) and setJewelRadiiGlobally.
- **ModMap + ConfigOptions apply closures** [Data/ModMap.lua (41 apply closures), Modules/ConfigOptions.lua (531 apply closures, 2380 lines)] deps=Rust ModList API, Config tab port :: Dump plain fields (label,type,values,tooltip); apply bodies hand-ported.
- **ModParser port with ModCache oracle** [Modules/ModParser.lua (7021 lines, ~1238 closures; specialModList ~2099 patterns), Data/ModCache.lua (23,207 entries)] deps=lua_pattern port (crates/pob-calc/src/lua_pattern.rs exists), typed Mod :: JewelFunc/ExtraJewelFunc (64 jewel funcs) not in cache — port as code. Expand oracle by running PoB ModParser over all Mod*/Uniques/tree stats at extract time.
- **StatDescriptions + StatDescriber** [Data/StatDescriptions/*.lua (22 files, plain), Modules/StatDescriber.lua (299 lines)] deps=pob-dump :: Plain data; ~45 named value handlers to port.
- **Tree + sprite data for UI port** [TreeData/<v>/tree.lua + sprites.lua + atlases (png/jpg/webp)] deps=pob-dump (or extend crates/pob-extract/src/tree.rs) :: Keep sprites/assets (currently dropped); legacy 2_6/3_6-3_9 skipped today.
- **Uniques/Rares text dump** [Data/Uniques/*.lua, Uniques/Special/{Generated,WatchersEye,BoundByDestiny,race,New}.lua, Rares.lua] deps=pob-dump (Generated.lua needs data.clusterJewels loaded) :: Raw item text; parsed by Rust Item parser + ModParser at runtime.

## risks
- STALE SkillType numbering: pob-extract/src/main.rs:374-523 and pob-data/src/flags.rs use an old 146-entry table (Area=11, Aura=43, Pact=143) while current PoB Global.lua has 135 entries (Area=8, Aura=39, Pact=135). All numeric skillTypes/addSkillTypes/excludeSkillTypes keys in data/skills/*.json are mis-numbered relative to PoB. The dumper must read Global.lua, not hardcode it.
- lua_to_json replaces closures with {"__lua_function":true} without identity, so the existing skill JSON can't link closures to Rust ports. pob_data::Skill also silently drops parts, preDamageFunc, baseMods, statDescriptionScope and others.
- Lua table ambiguity: empty {} vs [], holey int keys become string-keyed objects (e.g. Misc totemLifeMult lacks [14]), and level entries mix positional and named keys. That's why pob-data has lenient deserializers. A canonical encoding is needed.
- Statmap semantics: PoB's per-skill statMap has a metatable that falls back lazily to the global SkillStatMap (716 entries) and applies processMod (source tag, notMinionStat ActorCondition). A naive dump of skill.statMap misses this, so dump both and port the fallback.
- Shared references and cycles in `data`: gem.grantedEffect points at the skill, statMap._grantedEffect is a back-reference, and gemForSkill is keyed by a table. A naive serializer duplicates these or loops forever. The pob_diff serializer caps depth at 6, which silently truncates deep tables.
- JewelFunc/ExtraJewelFunc mods contain closures and are absent from ModCache, so radius-jewel behavior (64 jewel*Funcs in ModParser) must be ported as code. ModCache can't cover them.
- Skill closures reach into calc internals (skillModList:Sum, skillData, output, GlobalCache.cachedData in AvengingFlame), so porting them depends on the Rust calc's ActiveSkill/ModStore API being shaped like PoB's.
- Hand-copied constant tables in pob-data (monster_tables.rs, jewel_radius.rs, timeless_jewels.json dated 9 May, timeless_lut_stem) will drift from upstream. Generate them instead.
- data/skills/skills is a broken symlink to /Users/jferm/projects/...; tools/sprites_to_json.lua is referenced in pob-data/src/sprites.rs but missing, and sprites exist only for 3_25.
- Tree extraction drops sprites/assets, which the UI port needs, and skips legacy trees 2_6 and 3_6–3_9.
- LuaJIT numbers are doubles: integer vs float formatting must be canonicalized. ModCache prints 14 significant digits, so tests need tolerance.
- data/ is 642 MB (data/pob 418 MB, timeless 170 MB). data/pob is only needed while Lua runs at runtime. Keep it as the test oracle only and drop it from shipping artifacts.
- Some files can't be dumped as finished data because they run code at load: Uniques/Special/Generated.lua depends on data.clusterJewels, and the Data.lua derived tables (gem AltX/AltY synthesis, gemForBaseName, itemBaseLists, bossStats) need the full Data.lua load, not per-file evaluation.

## commands
- cargo run -p pob-extract -- --pob .PathOfBuilding --out data
- cargo run -p pob-extract --bin pob_diff -- --pob .PathOfBuilding --dump-modparser-env data/lua/modparser_env.lua
- POB_ENGINE=jit cargo run -p pob-extract --bin pob_headless -- crates/pob-extract/test-builds/witch_l90_arc_with_items.xml
- cargo test -p pob-calc --test modcache_oracle
- cargo test -p pob-data --test round_trip
- grep -ohE '[A-Za-z_]+\s*=\s*function' .PathOfBuilding/src/Data/Skills/*.lua | sed 's/ //g' | sort | uniq -c
- grep -c 'apply = function' .PathOfBuilding/src/Data/ModMap.lua .PathOfBuilding/src/Modules/ConfigOptions.lua
- awk '/^SkillType = \{/,/^\}/' .PathOfBuilding/src/Data/Global.lua