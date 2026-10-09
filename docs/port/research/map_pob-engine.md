# pob-engine

CRATE: /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine — "pure, wasm-clean" calc engine written by hand. It was never a module-by-module port of PoB. It was built feature by feature ("Phase 2/3", "issue #NN slice N"). Size: about 33.4k LOC in src plus about 9k LOC of tests. Dependencies: pob-data (the shared types crate, 5.3k LOC), serde/serde_json, smallvec, indexmap, ahash, bitflags, flate2, base64, quick-xml. No Lua. STATUS.md (2026-09-30) says it scores about 53% parity. It is now used only by the web build, as a placeholder before PoB answers, and by MK2's compare tab and per-mod attribution. The real calc runs PoB's Lua through crates/pob-calc (LuaJIT, plus a Rust Lua interpreter).

MODULES (path under crates/pob-engine/src, LOC, purpose, key types, verdict):
- lib.rs (107): re-export hub. Exports a large flat API: compute_*, import/export, timeless, cluster, power, and so on.
- modifier.rs (333): Mod{name:String, kind:ModType, value:ModValue, flags:ModFlag, keyword_flags:KeywordFlag, source:Option<Source>, tags:SmallVec<[Tag;2]>}.
  - ModType = {Base, Inc, More, Override, Flag, List, Max, Min}.
  - ModValue = Number | Range{min,max} | Bool | Str. It cannot carry PoB's table-valued LIST payloads (e.g. {mod=...}, {type=..., value=...}, ExtraSkill, MinionModifier, EnemyModifier, ExtraAura).
  - Source is an enum (Tree, Passive(u32), Item(u32), Skill, Buff, Config, Other). PoB uses free-form strings like "Tree:123" or "Item:5:Name".
  - TagKind has 12 variants plus Unknown(Value). PoB's EvalMod handles about 30 tag types (Condition with varList, ActorCondition, Multiplier with varList/base/globalLimit, PerStat with statList, StatThreshold, MultiplierThreshold, PercentStat, SkillType, SkillName, SkillId, SkillPart, SlotNumber, ItemCondition, SocketedIn, GlobalEffect, DistanceRamp, Limit, InSlot, LimitTotal, ...).
  - Verdict: the concept is reusable, but the shape must be redesigned to mirror PoB's mod table exactly.
- mod_db.rs (714): ModStore trait (iter_all, iter_named, sum, more, flag, override_value, applied); ModList (Vec); ModDB (name to Vec map with specialised queries); QueryCfg {source_category, flags, keyword_flags, skill_name}; EvalState {conditions, multipliers, stats: AHashMap<String,_>}; eval_mod().
  - Diverges from PoB in these ways:
    - queries take one name, where PoB takes varargs names;
    - no parent ModDB chain (PoB's modDB.parent, used for minion and enemy inheritance);
    - conditions and multipliers live in one global EvalState rather than per actor (modDB.conditions/multipliers/actor);
    - SkillName/SkillType/SlotName are faked as "SkillName:X" condition strings;
    - no Max query, no Tabulate, no List with tables, no GetMultiplier/GetStat/GetCondition with actor lookups;
    - unknown tags pass through as true.
  - The KeywordFlag::matches and ModFlag subset check are correct (they live in pob-data/src/flags.rs).
  - Verdict: rewrite as a faithful port of Classes/ModStore.lua (971), ModDB.lua (370) and ModList.lua (249). Only trivial patterns survive: name-hashed buckets, no-alloc key joins.
- mod_parser.rs (3733): hand-written prefix/suffix stripping, about 300 contains/starts_with/strip_prefix calls, 46 tests. It produces MK2-invented stat names in places (e.g. "<Ailment>AsThoughDealing", "AvoidChance"). It is not a port of ModParser.lua (7021 LOC: formList, modNameList, modFlagList, preFlagList, modTagList, specialModList, jewelFuncs). THROW AWAY. Port ModParser.lua faithfully. PoB ships .PathOfBuilding/src/Data/ModCache.lua (4.1 MB of precomputed parse results), which is a ready oracle and fallback table for the Rust parser.
- character.rs (1403): MK2's own build model. Character, CharacterSnapshot (serde, .mk2 format), ConfigState (about 20 typed enemy fields plus conditions/multipliers maps plus custom_mods), SocketGroup{gems:Vec<MainSkill>}, NamedItemSet/SkillSet/ConfigSet/Spec, PartyMember, Bandit/MajorGod/MinorGod enums with as_pob_name/from_pob_name, EnemyBoss, ClassRef(String). Also allocate_path/unallocate helpers. This is the type pob-ui depends on most (Character appears in about 20 UI files). Verdict: THROW AWAY for a faithful port. PoB state lives in Build.lua plus PassiveSpec, ItemsTab, SkillsTab, ConfigTab and the socket-group tables. The Bandit/Pantheon name tables could be kept as reference.
- env.rs (71): Env{mod_db, state, output}; Output = flat AHashMap<String,f64> with set_concat/get_concat. Far too thin: PoB's env has player/minion/enemy actors, mainSkill, activeSkillList, auxSkillList, breakdown tables, output tables with nested MainHand/OffHand, mode flags (MAIN, CALCS, CALCULATOR, CACHE). THROW AWAY.
- calc_data.rs (87): CalcData<'a> bundle of Option<&data> plus compute_with_data(). A thin facade. THROW AWAY; the idea of a borrowed data bundle may survive.
- perform.rs (5433): one monolith covering init_env_inner (about 430 lines of CalcSetup-like setup) and these passes:
  - perform_basic_stats
  - perform_reservations
  - perform_curses
  - detect_active_auras
  - detect_warcries (special-cased by skill id: SeismicCry, RallyingCry, ...)
  - perform_skill_dps (L2846 to about L4595, about 1750 lines; only the single main skill; supports gated only by add/exclude skill types)
  - perform_flask_recovery
  - perform_ehp
  - perform_enemy_damage_sim
  - fill_static_defaults

  It is full of hard-coded constants (base life 50+12/level, pen 3%, simplified EHP = pool/taken), "Phase 5 minimum" comments and skill-name match arms. It has no activeSkill/skillCfg structure, no CalcActiveSkill buildActiveSkillModList, no triggers or mirages, no per-actor env, and no minion perform pass. Verdict: THROW AWAY in full. The new code should be split 1:1 into CalcSetup (1975), CalcPerform (3990), CalcOffence (6263), CalcDefence (3874), CalcActiveSkill (934), CalcTriggers (1627), CalcMirages (427), Calcs (872) and CalcTools (303). Public entry points used by the UI: compute, compute_full, compute_full_with_env, compute_full_with_clusters(_and_timeless(_lut)), ClusterContext, canonical_weapon_class.
- breakdown.rs (10471, 212 tests): re-derives about 20 headline outputs after the fact (Breakdown, BreakdownStep, ModSource, derive_for, COVERED_KEYS). PoB builds breakdowns inline in CalcOffence/CalcDefence (env.breakdown / breakdown.lua helpers) and renders them in CalcBreakdownControl. THROW AWAY.
- skill.rs (594): SkillRegistry{by_id: HashMap<String, pob_data::Skill>} with get/iter_active/variants_of; QualityId{Default, Anomalous, Divergent, Phantasmal} with alt_suffix; MainSkill{skill_id, level, quality, quality_id, enabled}; skill_for_quality, skill_base_damage, iter_skill_stats.
  - parse_extractor_mod(&Value, f64) -> Mod decodes the {"__kind":"mod", name, type, flags, keywordFlags, "1":{tag}} records that pob-extract writes from statMap mod() calls. This is a usable bridge to extracted data.
  - global_skill_stat_mods is a tiny hand-picked subset of Data/SkillStatMap.lua.
  - Verdict: the data-side idea (extract Data/Skills/*.lua to JSON with mlua stubs at build time, keeping statInterpolation, costs, critChance, damageEffectiveness and statMap mod recordings) is REUSABLE as a loading strategy. The runtime model (MainSkill, the alt-quality hack) is MK2-specific and gets replaced by PoB's gem/grantedEffect/activeEffect structures and SkillStatMap. pob_data::Skill keeps levels/stats/qualityStats as loose serde_json::Value and should get typed structs.
- item_parser.rs (1208, 16 tests): parse_item(raw) -> pob_data::Item. Handles PoE copy-paste and PoB internal bodies with variants, ranges, the {crafted}/{range:} prefixes and (implicit)/(enchant) suffixes. Also apply_item_set(_with_bases) into a ModDB, slot_to_index and ItemApplyReport. A permissive approximation of Item.lua:ParseRaw; Item.lua (2736 LOC) also handles BuildModList, base stats, local mods, influences, catalysts, sockets and requirements. Verdict: mostly rewrite as an Item.lua port. Some string helpers (split_variant_prefix, apply_range, split_sections) and the test fixtures are reusable.
- minion.rs (1692, 33 tests): MinionState, select_minion_type, parse_minion_intrinsic_mods, apply_minion_outputs (writes Minion.Life etc.). Not a parallel minion env. THROW AWAY.
- power.rs (1626, 38 tests): recomputes the full build per candidate node or item modline (NodeScore, rank_node_additions, score_item_swap, rank_item_modlines, compare_*_attribution). PoB's PowerBuilder/CalcsTab uses calcFunc/getMiscCalculator fast paths. THROW AWAY. The UI result struct NodeScore is trivial.
- jewel_radius.rs (5787, 81 tests): node_position plus orbit_angle_rad (16/40-slot tables matching PassiveTree.lua CalcOrbitAngles), nodes_in_radius, allocated_nodes_in_radius, about 15 bespoke HandlerKind cases per unique (Healthy Mind, Fertile Mind, Karui Heart, Pure Talent, Watcher's Eye, ...), SocketedJewels, Intuitive Leap helpers, apply_radius_jewels. PoB does this generically through ModParser jewelFuncs plus CalcSetup radius-jewel processing. KEEP node_position/orbit-angle geometry (about 80 lines, after verifying against PassiveTree.lua). THROW AWAY the handlers.
- jewel_geometry.rs (281) and jewel_threshold.rs (276): thin helpers and a scaffold. THROW AWAY or fold in.
- timeless.rs (1211, 22 tests): identify_timeless_jewel, keystone and notable replacement via inflated LUTs (inflate_timeless_lut = zlib), TimelessLutContext, set/format_timeless_marker. The LUT indexing (flat notable_count x seedSize layout, a special GV layout) is partly validated. PARTIALLY REUSABLE as a reference for porting PassiveSpec's timeless/legion logic and TimelessJewelData reading. inflate_timeless_lut is also called by pob-extract/src/timeless_nodes.rs.
- cluster_synth.rs (1185, 9 tests): ClusterJewelSpec, parse_cluster_jewel, synthesise_for_socket/all, make_synth_id (the ID bit scheme mirrors BuildSubgraph). The file header says its coordinates are simplified, not PoB's orbit math. REWRITE as a faithful port of PassiveSpec BuildClusterJewelGraphs/BuildSubgraph; the UI renderer needs exact positions. The ID scheme and test cases are reusable.
- pathfind.rs (278, 6 tests): BFS shortest_path_from_allocated, anchor_nodes, anchored_subset. Small and generic. PoB uses PassiveSpec BuildAllDependsAndPaths (pathDist, depends). Port PoB's version; this file is optional reference.
- pob_import.rs (1362, 16 tests): resolve_share_url (pobb.in, pastebin, poe.ninja, ... to raw code), pob_code_to_xml (url-safe base64 then zlib), pob_xml_from_input, pob_tree_version, import_pob_xml -> Character (quick-xml streaming over Build/Tree/Spec/Items/ItemSet/Skills/Config/Notes). REUSABLE: share-URL resolution and code decoding. The XML-to-Character mapping is THROW AWAY; a faithful port loads XML through each tab's Load(xml) the way Build.lua does.
- pob_export.rs (645): export_pob_xml(Character), encode_pob_code (zlib then url-safe base64). encode_pob_code is REUSABLE; the rest goes.
- share.rs (450): MK2-only "MK2|base64(zlib(json))" codes plus save_state_fingerprint. THROW AWAY.
- ggg_import.rs (1689, 27 tests): serde structs for the pathofexile.com character-window JSON (CharacterSummary, PassiveSkillsResponse, ItemsResponse, GggItem, GggSocket, ...), decode_mastery_effects, ggg_slot_to_pob, URL builders. Partially reusable as serde shapes. The mapping logic should follow ImportTab.lua.
- bin/perf_bench.rs (176), examples/gen_share_fixture.rs (17).
- tests/:
  - end_to_end.rs (8169, 120 tests): MK2 behaviour canaries, not PoB oracle numbers;
  - radius_jewels.rs (514), tree_alloc.rs (518), validation.rs (185), parser_coverage.rs (123).
  - None of these are reliable parity oracles. The real oracle is pob-calc (LuaJIT) plus the pob_diff corpus (57 builds).

PROJECT-SIDE DATA LOADING (pob-data plus pob-extract, which feed this crate). pob-extract runs PoB's Data/*.lua under mlua with stubbed mod/flag/skill helpers and writes JSON into data/: bases.json, gems.json, skills/{act_*,sup_*,glove,minion,other,spectre}.json, minions.json, cluster_jewel(_mods).json, enchants_*.json, legion_*.json, tattoos.json, timeless_node_index.json, calc_sections.json, trees/, timeless/*.zip. pob-data/src/flags.rs has ModFlag/KeywordFlag/SkillType bit values copied exactly from Global.lua; this is the most reusable piece. pob-data PassiveTree/Node/Group/TreeConstants are serde structs over the extracted tree JSON. Lua functions in data (skill preDamageFunc/initialFunc/statMap function entries, Data.lua computed tables, ModParser jewelFuncs) cannot go through JSON, so the port must hand-port them.

PUBLIC API USED BY crates/pob-ui: 206 references across 20 files.
- Most used: Character (about 21 refs; also character::{ConfigState, EnemyBoss, SocketGroup, NamedItemSet, ClassRef}), ClassRef::ranger (19, mostly tests), export_code/import_code/export|import_{item,skill,config}_set, compute_full / compute_full_with_env / compute_full_with_clusters_and_timeless_lut / compute_with_data, Env, Output, apply_minion_outputs, ClusterContext::new, TimelessLutContext, identify_timeless_jewel, inflate_timeless_lut, compute_keystone_replacements, compute_notable_replacements, set_timeless_marker, timeless_lut_types_needed, cluster_synth::synthesise_all + ClusterJewelSpec, rank_node_additions + NodeScore (heatmap), compare_item_modline_attribution, compare_node_attribution, rank_item_modlines, parse_item, import_pob_xml, import_pob_code, pob_xml_from_input, pob_tree_version, resolve_share_url, export_pob_xml, encode_pob_code, save_state_fingerprint, SkillRegistry::from_files, MainSkill, QualityId, skill::base_skill_id, skill::parse_extractor_mod, mod_parser::parse_mod_line, mod_db::{eval_mod, EvalState, QueryCfg}, ModStore, ModType, ModValue::*, TagKind::* (calcs_tab tag display), pathfind, ggg_import.
- Heavy users: lib.rs (calls compute at L718, L863, L3298, L4675, L4746), calcs_tab, items_tab, skills_tab, config_tab, party_tab, compare_tab, node_power_heatmap, undo, import_export_tab, cluster_overlay, ggg_fetch(_wasm), pob_session (Character/Output only).
- Other crates: pob-extract uses inflate_timeless_lut (timeless_nodes.rs) and import_pob_xml + SkillRegistry + timeless_lut_types_needed + ModStore (bin/pob_diff.rs diff_against_pob_engine). app/* has no direct use.
- If pob-ui is replaced by a port of PoB's UI, this whole surface disappears. The Character model goes too: the ported Build/tab state replaces it.

OVERALL: roughly 90% of the crate should be discarded. It is heuristic, monolithic and string-keyed with MK2-invented names, and its tests encode MK2 behaviour rather than PoB's. Worth salvaging: (1) the exact flag bit tables (pob-data), (2) the extracted-JSON data pipeline idea and parse_extractor_mod, (3) PoB share-code encode/decode and share-URL resolution, (4) tree node geometry and orbit-angle tables, (5) timeless LUT decoding knowledge, (6) cluster synth ID scheme, (7) GGG API serde shapes, (8) small perf idioms (no-alloc key joins, name-bucketed ModDB). Everything else should be written fresh as 1:1 ports of PoB's Lua files.

## reusable
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-data/src/flags.rs — ModFlag/KeywordFlag (bit values exact from Global.lua) + KeywordFlag::matches (MatchKeywordFlags semantics) + SkillType enum; keep as-is (verify SkillType list vs current Global.lua)
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/skill.rs::parse_extractor_mod (L319-428) — decodes pob-extract's {__kind:'mod',name,type,flags,keywordFlags,'1':{tag}} recordings into a Mod; adapt to new Mod/Tag types
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-extract/src/skills.rs (+ data/skills/*.json) — mlua-based extraction of Data/Skills/*.lua with mod/flag/skill stubs, preserves statInterpolation/cost/critChance/damageEffectiveness/statMap; reusable data-loading strategy (but Lua functions in skill data must be hand-ported)
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/pob_import.rs: resolve_share_url (L69), pob_code_to_xml (L156), pob_xml_from_input (L166), pob_tree_version (L195) — share URL/code decoding (url-safe base64 + zlib) reusable
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/pob_export.rs::encode_pob_code (L427) — zlib+base64 encoding
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/jewel_radius.rs: node_position (L148) + orbit_angle_rad 16/40-slot tables — mirrors PassiveTree.lua CalcOrbitAngles; reusable for tree model/renderer after verification
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/timeless.rs: inflate_timeless_lut (L487) and LUT indexing in compute_notable_replacements (L378) — reference for porting PassiveSpec timeless/legion handling
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/cluster_synth.rs::make_synth_id (L719) — cluster subgraph node id bit scheme mirroring PassiveSpec BuildSubgraph
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/ggg_import.rs serde structs (CharacterSummary L44, PassiveSkillsResponse L73, ItemsResponse L105, GggItem L138, GggSocket L213) + decode_mastery_effects (L676) + URL builders (L1026-1090)
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/item_parser.rs helpers split_sections (L503), split_variant_prefix (L474), apply_range (L433), split_pob_prefixes (L396) and its 16 test fixtures — useful when porting Item.lua ParseRaw
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/mod_db.rs: no-alloc prefixed key lookup idiom (EvalState::condition_prefixed) and name-bucketed ModDB storage pattern (perf idioms only)
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/character.rs: Bandit/MajorGod/MinorGod as_pob_name/from_pob_name tables (L442-580) — small name mappings
- /home/ferm/Work/projects/PathOfBuildingMK2/.PathOfBuilding/src/Data/ModCache.lua (4.1MB) — precomputed ModParser outputs; oracle/fallback for a Rust ModParser port (not in pob-engine but directly relevant)

## discard
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/perform.rs (5433) — monolithic heuristic calc; replace with 1:1 ports of CalcSetup/CalcPerform/CalcOffence/CalcDefence/CalcActiveSkill/CalcTriggers/CalcMirages/Calcs/CalcTools
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/breakdown.rs (10471) — after-the-fact re-derivation; PoB breakdowns are produced inline by the calc modules
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/mod_parser.rs (3733) — hand-rolled; replace with a ModParser.lua port validated against Data/ModCache.lua
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/mod_db.rs (714) — rewrite as a port of ModStore/ModDB/ModList.lua
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/modifier.rs (333) — redesign to match PoB's mod tables (tables in values, full tag set, string sources)
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/env.rs (71) and calc_data.rs (87)
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/minion.rs (1692)
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/power.rs (1626) — replace with a port of PoB's power/calculator paths
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/character.rs (1403) — MK2 model; replaced by ported Build/PassiveSpec/ItemsTab/SkillsTab/ConfigTab state
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/share.rs (450) — MK2-only share format
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/jewel_radius.rs handlers (all except node_position/orbit_angle_rad), jewel_geometry.rs, jewel_threshold.rs
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/pob_import.rs import_pob_xml (XML to Character mapping) and pob_export.rs export_pob_xml
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/cluster_synth.rs layout and synthesis (rewrite faithfully, keep the ID scheme)
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/skill.rs runtime model (MainSkill, QualityId alt hack, global_skill_stat_mods subset)
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/tests/* — MK2-behaviour canaries, not parity oracles

## workUnits
- **ModStore/ModDB/ModList + Mod representation** [.PathOfBuilding/src/Classes/ModStore.lua (971), ModDB.lua (370), ModList.lua (249), Modules/ModTools.lua; replaces pob-engine/src/{modifier.rs,mod_db.rs}] deps=pob-data/src/flags.rs (keep) :: Mod value must support tables (LIST payloads). Full EvalMod tag set, parent chain, per-actor conditions/multipliers, varargs names, Tabulate/Max/List. Only perf idioms from old mod_db.rs carry over.
- **ModParser** [.PathOfBuilding/src/Modules/ModParser.lua (7021), Data/ModCache.lua (oracle); replaces pob-engine/src/mod_parser.rs] deps=ModStore unit :: Validate line by line against ModCache.lua. jewelFuncs are Lua closures and must be hand-ported.
- **Data loading (skills/gems/bases/minions/tree)** [crates/pob-extract (mlua extraction to data/*.json), crates/pob-data, Data/*.lua, Modules/Data.lua] deps=flags :: Keep the extraction approach and parse_extractor_mod. Type the loose serde_json::Value fields. Hand-port the Lua functions embedded in data (preDamageFunc, initialFunc, etc.) and SkillStatMap.lua.
- **Item model + parsing** [.PathOfBuilding/src/Classes/Item.lua (2736); replaces pob-engine/src/item_parser.rs] deps=ModParser, bases data :: Reuse the old string helpers and test fixtures only.
- **Passive tree/spec (incl. cluster, timeless, radius jewels, pathing)** [Classes/PassiveTree.lua (1062), PassiveSpec.lua (2494); replaces jewel_radius.rs, cluster_synth.rs, timeless.rs, pathfind.rs] deps=data loading, ModParser :: Reuse node_position/orbit-angle tables, the synth ID scheme and the timeless LUT decoding as reference.
- **Calc pipeline** [Modules/CalcSetup.lua (1975), CalcPerform.lua (3990), CalcOffence.lua (6263), CalcDefence.lua (3874), CalcActiveSkill.lua (934), CalcTriggers.lua (1627), CalcMirages.lua (427), Calcs.lua (872), CalcTools.lua (303), CalcBreakdown.lua (255); replaces perform.rs, breakdown.rs, minion.rs, power.rs, env.rs] deps=all of the above :: Nothing from perform.rs is reusable. Oracle: pob-calc LuaJIT plus the pob_diff corpus.
- **Build I/O (XML, share codes, account import)** [Modules/Build.lua and tab Load/Save, Classes/ImportTab.lua; reuse pob_import.rs resolve_share_url/pob_code_to_xml, pob_export.rs encode_pob_code, ggg_import.rs serde structs] deps=build state model :: Drop the Character-based mapping and the MK2 share.rs format.

## risks
- perform.rs (5433 LOC), breakdown.rs (10471), minion.rs, power.rs, mod_parser.rs (3733), env.rs and the jewel_radius.rs handlers (about 5.5k) all encode MK2-invented stat names and heuristics. Copying any of them into the faithful port carries the 53% parity error rate along with it.
- pob-engine tests (about 700 #[test] across src and tests/, end_to_end.rs alone 8169 LOC) assert MK2 behaviour, not PoB numbers. Do not treat them as oracles. Use pob-calc (LuaJIT) and the pob_diff 57-build corpus instead.
- ModValue cannot hold PoB's table-valued LIST mods. Source is an enum, but PoB parses source strings ('Item:5:Name', 'Tree:123'). TagKind covers about 12 of PoB's ~30 EvalMod tag types and silently passes unknown tags. ModStore queries take a single name and have no parent chain or per-actor state. Reusing mod_db.rs as-is would cap accuracy.
- Extracted skill/minion JSON drops Lua functions (preDamageFunc, initialFunc, statMap entries with function values, Data.lua computed tables, ModParser jewelFuncs). These must be hand-ported, or the extraction has to grow.
- pob-ui depends heavily on pob_engine::Character (about 20 files) and on compute_* entry points (lib.rs L718/L863/L3298/L4675/L4746). Removing pob-engine breaks the current UI and the web build (which still falls back to it). Plan the removal together with the UI rewrite.
- pob-extract/src/timeless_nodes.rs and bin/pob_diff.rs depend on pob-engine (inflate_timeless_lut, import_pob_xml, SkillRegistry, timeless_lut_types_needed). Move or replace these before deleting the crate.
- cluster_synth.rs uses simplified node coordinates, not PoB's. Unsuitable for a faithful tree renderer.
- Timeless LUT tests self-skip when the data/timeless/*.zip files are Git LFS pointers, so their correctness is only partly verified.

## commands
- wc -l /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/src/*.rs /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-engine/tests/*.rs
- grep -rhoE 'pob_engine::[A-Za-z_:]+' --include='*.rs' /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-ui /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-extract | sort | uniq -c | sort -rn
- grep -rn 'use pob_engine' --include='*.rs' /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-ui
- wc -l /home/ferm/Work/projects/PathOfBuildingMK2/.PathOfBuilding/src/Modules/{ModParser,CalcSetup,CalcPerform,CalcOffence,CalcDefence,CalcActiveSkill,CalcTriggers,CalcMirages,Calcs,CalcTools}.lua /home/ferm/Work/projects/PathOfBuildingMK2/.PathOfBuilding/src/Classes/{ModStore,ModDB,ModList,Item,PassiveSpec,PassiveTree}.lua
- cargo run -p pob-extract --release   # regenerates data/*.json and data/pob/