# Calc goldens and the PoB cut-over: plan (GOLD-PLAN)

Status: plan for review, 2026-11-13, on rust-port `0dbdab33`; revised after review (fix rounds 1 and 2).
Docs only. The only code is a throw-away size probe in the scratchpad (appendix A).

Inputs:
- the user's decisions of 2026-11-13 (scope file GOLD-PLAN): standalone app, PoB removed from the
  repo, calc goldens only, no UI goldens, a deep refactor next, game data frozen as `data.bin`;
- the test inventory (`scratchpad/audit/test-inventory.md`, rust-port `0dbdab33`; called "the
  inventory" below);
- measurements made for this plan (appendix A).

The old porting rules (provenance markers, Lua-line mirroring, CCRs against the port docs) do not
bind this work. Where this plan names a Lua line or a CCR, it is evidence, not a rule.

Size marks: **[M]** = measured for this plan (method in appendix A). **[E]** = estimate from record
counts. GOLD1 replaced every [E] the existing caches can measure (§2.3, appendix A); the rest
are measured by their units at record time and kept in MANIFEST.

---

## 0. Summary

- **What gets recorded.** PoB's answers, recorded once by a recorder that still runs PoB under
  LuaJIT (the pob-oracle machinery). They are committed under `tests/golden/`.
- **What uses them.** A new dev-only crate `pob-golden`. It has no PoB, no `mlua` and no
  `pob-oracle`. All calls into the calc go through one adapter layer, `pob-golden/src/adapter/`,
  so the refactor re-points only that layer.
- **Two shapes:**
  - **E2E build goldens**: build XML in, behaviour-level outputs out. They cover CA L6, INT-3,
    BM-A, PWR, the misc-calculator overrides, and every special corpus that only segment tests
    reach today.
  - **Function-level query sets**: input in, PoB's answer out, for the parser, tree, spec, LUT,
    items, mod store, skills, config and tools.
  - DF-A and OF-A become **fuzz output sets**: a frozen input generator plus PoB's outputs.
- **Format.** Sorted JSON lines, one value per line.
  - E2E goldens are committed as plain `.jsonl`, so `git diff` is the review tool. Git compresses
    them about as well as zstd does: 3.9 MB of CA lines packs to 512 KB [M].
  - A file whose plain JSONL would exceed about 4 MB is split by key into `.jsonl.zst` shards
    instead, with a `golden diff` tool for review. The threshold is per file: E2E goldens are one
    file per build and all stay plain.
  - No CBOR, and no git LFS.
- **Budget** (§2.3; GOLD1 measured what the caches hold, the rest is still [E]), in two measures:
  - **packed** (what the history and a clone add, measured as the `git gc` pack of `tests/golden/`):
    about 74 MB, build XML included, of which fn.modstore is about 28 MB and must come down to
    20 MB by the §2.3 fallback. **Cap 80 MB.**
  - **working tree** (the checked-out files: plain E2E JSONL at full size, shards at zstd size):
    about 193 MB, of which about 112 MB is plain E2E JSONL. **Cap 250 MB.**
  - **Per file**: at most 8 MB on disk, plain or zstd.

  Lossless elision and dedup come first. Sampling is used only for
  MS-B's trivial answers, DF-A's 600k random cases, CT's support pairs (as a bitset, not a sample)
  and PT's duplicated tree records. Each one is justified in §2.3.
- **Intentional behaviour changes** never rewrite PoB's recording. They go into **overlay files**
  (`*.bless.jsonl`), one line per changed value with a reason. They are written by
  `POB_GOLDEN_BLESS=1 POB_GOLDEN_REASON=...`, and a checklist governs their review (§3.6).
- **Gate before deletion** (§4). Every live PoB test that guards behaviour passes at the gate
  commit, with its pinned counts. Every golden runner passes with zero failures, compares at least
  those counts, runs in a worktree that has no `.PathOfBuilding`, `data` or `.parity`, and fails
  its negative control. In the same worktree, every test target of every crate in CI's unit job
  passes, with no skip variable set and no `SKIP` in any log. The recorder reproduces the
  committed goldens byte for byte (VERIFY).
- **Cut-over** (§5): VENDOR, then the gate, then tag `pob-oracle-final`, then deletion, then a CI
  rewrite.
- **Tests that need no PoB** but live in the crates that CUT1 deletes (BM-A's XML test, the PWR
  stepping test, U0's data.bin test, the par pool, …) each get a destination and an owning unit, or
  an explicit drop (§1.7). They move before the gate, and CUT1 checks that none is left behind.
- **Product tests outside the oracle crates** that read the PoB checkout, `data/dump`, `.parity` or
  files inside the deleted crates (pob-app's `fup5_teardown` and `u54_worker_calc`, pob-sg's font
  and image tests, pob-sg-wgpu's T5 text frames, pob-host's asset test, pob-calc's INT-2 bench,
  `leak1_cycles`, `dump_load`, …) are listed one by one in §1.8, each with an owner and a
  destination (the asset bundle or `tests/golden/builds/`) or a drop. The gate runs every product
  crate's test suite in the PoB-free worktree, with no skip variable set (§4.3).
- **Units** (§6): GOLD1 (format and runner core) and GOLD2 (fixtures). Then GOLD3-GOLD13 run in
  parallel, three at a time. Then GATE, then CUT1. CUT2 (removing the `probe` feature from product
  crates) is optional.
- **Probe result** for the main open risk, "do the segment-test corpora pass end to end today?":
  CA L6 was exact on every key of **117 of the 418** unit-corpus builds: a sample of 110 (headline
  1,430/1,430, keys 86,212/86,212, 0 Rust errors), plus 7 from an aborted first run (BM-B ×3, CO-A,
  CO-B, CO-C and CS0, 749-766 keys each). See appendix B. The other 301 builds are not checked yet;
  GOLD3 runs them.

Open questions for the user are in §7.

---

## 1. Golden inventory

### 1.1 Common encodings

These are used by every set. The formal grammar is in §3.2.

- **Build key**: `<corpus>/<stem>`, for example `all57/allflame_exp_000`, `fresh/fresh_076` or
  `special/TR-B/trb_cwc_spell`. The header also holds the XML's BLAKE3. The runner fails if the
  XML changed and the golden did not (no silent drift).
- **Output key**: the path of PoB's output table, flattened as CA flattens it today (`main.Life`,
  `main.MainHand.PhysicalMin`, `main.Minion.Life`, `calcs.BuffList`). These names are the product's
  own vocabulary: CalcSections, PlayerStat in the XML, the power-stat list. They are not Lua
  internals. If the refactor renames an output struct field, the adapter maps the field back to
  the key.
- **Mod encoding** (`mod/1`):

  ```json
  {"name":…,"type":…,"value":…,"flags":[ModFlag names],"keywordFlags":[KeywordFlag names],"source":…,"tags":[…]}
  ```

  Flags are written as sorted **names**, not bit masks, so renumbering cannot break a golden. Tags
  are objects with sorted keys. Lists of mods keep PoB's order. Where PoB's order is not fixed
  (calc doc §9.5), the list is compared as a multiset, and an order difference is only reported.
- **Values**:
  - JSON numbers, with `{"$num":"nan"|"inf"|"-inf"}` for the others;
  - strings and booleans;
  - markers: `$anyOf`, `$unordered`, `$error`, `$host`, `$known` (§3.2);
  - object references as CA renders them today: `{"$item":name}` and `{"$gemInstance":nameSpec}`.

### 1.2 E2E build goldens (calc behaviour end to end)

Each build golden is plain JSONL: one `[key, value]` line per value, sorted by key. File:
`tests/golden/e2e/<set>/<corpus>/<stem>.jsonl`.

| Set (replaces) | What to record (PoB side) | Form | Stable key | Size raw / zstd |
|---|---|---|---|---|
| **e2e.ca** (CA `ca_l6_corpus`, all57) | `calcsTab.mainOutput` (45,954 keys), `calcsOutput`, the CALCS Buff/Combat/Curse lists, the used-tracking key sets of the 9 `*Used` tables, misc calculator `callDefault` and `callNoFullDPS` (headline keys). Also the number of `BuildOutput` rounds PoB's load made (CA derives it from the `ca:399` hit count; the header records it) | E2E fixture | build key + output key | 3.9 MB / 0.58 MB (git pack 0.51 MB) [M] |
| **e2e.ca** (fresh 100) | the same, on `.parity/fresh` (85,603 main keys) | E2E fixture | build key + output key | ≈7.3 MB / 1.1 MB [E: all57 × 85,603/45,954] |
| **e2e.ca** (special corpora, §1.5) | the same, on about 420 unit-corpus builds plus about 100 derived fixtures | E2E fixture | build key + output key | ≈31 MB / 3 MB [E: about 1,500 lines × 40 B per build] |
| **e2e.ca** mirage block (MI) | for each fixture whose main skill has a mirage: `mainSkill.mirage.output` flattened as `mirage.<key>` | E2E fixture block | build key + `mirage.<key>` | < 0.2 MB [E] |
| **e2e.d13** (CA `ca_d13_fixed_point`) | nothing new from PoB. The warm-cache values of the `D13_POB_WARM_CACHE` table move into the golden as data. The fixed-point check stays Rust against Rust: build → output → output gives the same output and the same `SaveDB` | Rust-only invariant + tiny golden | build key + output key | < 10 KB |
| **e2e.levelcost** (CA `ca_level_cost_cleared*`) | the 3 inline XMLs become fixtures. The recorder measures ManaCost and ManaReserved in PoB, in one process, in the test's order. Today these are hand values measured in PoB | E2E fixture with a sequence | fixture + step + key | < 10 KB |
| **e2e.int3** (INT-3) | `calcsEnv` player and minion breakdowns (10,606 entries), canonicalised as INT-3 does (identities resolved, items and stores as markers; the lists CalcBreakdownControl sorts are sorted on both sides). Every CalcSections Tabulate query and every `Combine` total (38,940 queries, 42,588 rows) | E2E fixture | build key + `bd.<actor>.<entry>` / `tab.<actor>.<modType>.<cfg>.<names>` | int3.json 23.9 / 1.21 MB [M], breakdown part ≤ 24.1 / 2.04 MB [M] → ≈3 MB zstd |
| **e2e.bm** (BM-A) | per all57 build, PoB's load of the **original** XML: the build model at the first `BuildOutput`, PoB's `SaveDB` at the same point (canonicalised, per section), and the model after `UpdateSockets`. Key vocabulary and the records left out: below the table | E2E fixture | build key + `model.` / `save.` / `sockets.` + build-file key | the BM-A cache (114 entries: 57 loads of the original XML, 57 of the Rust-saved XML) is 26.9 / 2.83 MB [M]. The golden keeps the 57 original-XML entries: ≈15 / 1.5 MB [E] |
| **e2e.pwr** (PWR `pwr_l6_report`, `pwr_l5_replay`) | PoB's power report (every node's `power` table, path power, `powerMax`, `clusterNodeMap`) for OccVortex Offence/Defence (7,672 values), and for the 3 replay builds × their modes (Life, PhysicalTakenHit, FullDPS, depth 3) plus the derived Full-DPS fixture | E2E fixture | build key + mode + node id + field | the replay cache is 33.4 / 2.0 MB [M]. Power tables only: ≈8 / 0.8 MB [E] |
| **e2e.mcq** (new; replaces CS2 `cs2_l4_overrides`, which is Rust-only today) | misc-calculator calls with overrides, per all57 build and per CS2 fixture, ≤ 60 calls per build: `repSlotName`+`repItem` (up to 3 other items from the build's list per slot, two-handed swaps included), `toggleFlask` (each flask), `removeNodes` (each keystone and up to 10 notables), `repItem` of a jewel that carries an `ExtraJewelFunc` mod into an allocated jewel socket (the forced-restart path). Records the headline keys and the power-stat keys | calculator-query golden | build key + override id (`rep:<slot>:<itemId>`, `flask:<slot>`, `rm:<nodeId>`, …) + key | ≈5 / 0.6 MB [E: 57 × 60 × 40 values] |
| OF-I dps30 strict | nothing extra. `CombinedDPS` and `TotalDot` are main-output keys, which e2e.ca already compares exactly on every build (dps30 is a subset of all57). The manifest lists them as required keys on dps30, so they cannot drop out silently | covered by e2e.ca | - | 0 |

**e2e.bm: which records, and the key vocabulary.**

- **Kept:** the 57 cache entries of PoB loading the original XML. Each holds four parts
  (BM-A.rs `POB_RUN_CALLS`): `pre` (the model at the first `BuildOutput`), `preSave` (`SaveDB` at
  that point), `post` (the model after `UpdateSockets`) and `outputs`.
  - `pre` → `model.<key>`, `post` → `sockets.<key>`, and `preSave` → `save.<Section>`, one
    canonicalised text per build-file section, as BM-A's `canon_sections` compares them.
  - `outputs` (`__oracle_outputs`: the player and minion output tables) is not stored again.
    GOLD5 checks at record time that it equals the build's e2e.ca `main.*` and `main.Minion.*`
    lines. A key that e2e.ca lacks stays in e2e.bm as `outputs.<key>`.
- **Left out:** the other 57 entries, PoB loading the Rust-saved XML. Rust's saved XML is a moving
  input (see "PoB on the Rust-saved XML" below).
- **Key vocabulary.** A key is `<tab>.<entity>[.<id>].<field>`.
  - `<tab>` is one of the build file's sections: `build`, `items`, `tree`, `skills`, `config`,
    `calcs`.
  - `<id>` is an id from the file: item id, item set id, slot name, spec index, config var name,
    calcs section and subsection id.
  - `<field>` names the build-file attribute or element that holds the value, where there is one
    (sometimes without its suffix: `jewelType` ↔ `jewelTypeId`). Examples:
    `build.timeless.jewelType` ↔ `<TimelessData jewelTypeId>`,
    `items.item.<id>.raw` ↔ the `<Item id>` text, `items.set.<id>.slot.<name>.selItemId` ↔
    `<ItemSet><Slot name itemId>`, `tree.spec.<i>.alloc` ↔ `<Spec nodes>`, `config.input.<var>` ↔
    `<Input name>`, `calcs.section.<id>.<sub>` ↔ `<Section id subsection collapsed>`.
  - A few fields have no element of their own. They are model facts the product shows: slot state
    (`items.slot.<name>.{active,inactive,weaponSet,nodeId,parentSlot,label,items}`),
    `items.orderedSlots`, `items.sockets`, `items.activeSet`, `tree.buildSpecIsActive`.

  These are the product's file-format and model names, not Lua table paths. bma.lua's paths
  already follow them, with three exceptions. The recorder handles those through a fixed rename
  table in `pob-golden/src/encode.rs` (`BM_KEYS`), which the adapter's `model()` uses as well:
  - the Lua array lengths `tree.n` and `items.statSort.n` become `.count`;
  - `items.lastSlot` (the slot that last had UI focus) is dropped, because it is UI state;
  - `items.slot.<name>.activateState` (the state of the flask checkbox control) is dropped. It is
    a UI control, and the build fact is `.active`.

What changes compared with today:

- **Minion outputs.** These are already inside `main.Minion.*`. On the one all57 build whose main
  env has a minion, `ca.json`'s `mainMinion` equals `main.Minion` exactly [M].
- **Used-tracking.** It stays a key-set comparison, as today. Recording values as well is a
  possible extension (§7).
- **BM-A's "PoB on the Rust-saved XML"** cannot be a golden, because Rust's saved XML is an input
  that changes. It is replaced by two checks:
  1. Rust `SaveDB` == PoB `SaveDB` (golden);
  2. Rust's outputs on Rust's saved XML == the e2e.ca golden of the original build.

  PoB's outputs on its own re-load equal its outputs on the original XML (BM-A asserts this today),
  so both checks together say the same thing.
- **PWR replay.** The replay feeds PoB's recorded calc outputs into the Rust PowerBuilder through
  the `power::calc_replay` seam. It is replaced by real end-to-end reports on the same 3 builds and
  modes. That is stronger, but it is not gated today: GOLD6 must show it passes before the replay
  test retires (§4).

### 1.3 Function-level query sets

A query set is a list of `[key, input, expected]` lines. `input` is `null` when the key is the
input (for example a mod line). Files are under `tests/golden/fn/<area>/`. A set whose plain
file would exceed about 4 MB is split by key into `.jsonl.zst` shards, each ≤ 8 MB compressed.
Smaller sets (CO-A/B, for example) stay plain.

| Set (replaces) | What to record | Form | Stable key | Size raw / zstd |
|---|---|---|---|---|
| **fn.parser.lines** (MP6 `whole_parser_l1`) | every one of the 80,993 records of `parse_oracle.jsonl`: `line → {mods (mod/1), extra}`. The `passes` field (pattern and Lua-line provenance) is dropped | query set; the key is the line | mod line text | the jsonl is 54 MB / 1.07 MB with `passes` [M]. Without them ≈25 / 0.7 MB [E] |
| **fn.parser.fuzz** (MP2-MP6 closure tests on synthetic captures) | each synthetic capture becomes a **synthetic line**: the recorder renders the list pattern with the capture substituted (strip `^$`, unescape `%`, fill the `(…)` groups). Then full PoB `parseMod` with the cache bypassed: `line → {mods, extra}`. A line where another pattern wins is still a valid golden. Static list entries are covered by fn.parser.lines | parser fuzz set | synthetic line text (+ origin `list/pattern` as information only) | ≈30k lines, 5 / 0.3 MB [E] |
| **fn.parser.jewel** (MP7 `mp7_oracle`) | each of the 234 jewel funcs on every node of the 3_26 tree plus the tattoo and conquered overlays (≥883,350 calls): `out` mods, `data`, error status. **Empty results are elided**: the recorder checks that every elided call returned the empty answer and stores a per-func node bitmap of the non-empty calls | query set | jewel mod line that creates the func + tree version + node id | ≈15 / 1.5 MB [E] |
| **fn.lut** (LUT) | `readLUT` for types 1-6 (every listed node + 3 unlisted × 200 seeds + invalid and fractional seeds), `readAbyssJewelLUT` for types 7-10, Zorath (11) with and without ascendancies, component resolve and roll, and `repairLUTs`: 2,380,556 results. Written columnar per (type, seed) | query set | jewel type + seed + node/socket id | ≈25 / 5 MB [E] |
| **fn.tree** (PT `passive_tree_acceptance`) | for each of the 41 tree versions: node records (fields, `modList` as mod/1, mods, mastery effects, positions), `nodesInRadius` per socket and keystone (as sets), the maps, groups and orbits, jewel radii, legion and tattoo data (903,303 records, 118,817 nodes). Records **deduplicated across versions** by content hash: each version is an index of record hashes | query set | tree version + record kind + node or socket id | ≈120 MB raw before dedup → ≈8 MB zstd [E] |
| **fn.spec** (PS-A, PS-B, PS-C, merged) | for every corpus `<Spec>`, the 62 synthetic timeless/abyss jewel builds and the synthetic cluster builds (3_29, 3_13): the whole PassiveSpec after load, run **unstubbed** (alloc, depends, paths, conquered/timeless rewrites, cluster subgraphs, legacy hash remap), and the operation queries (alloc and dealloc results, URL encode and decode, Save, ImportFromNodeList, convert). This closes the inventory's gap "the spec is never compared whole" | query set over fixtures | build key + spec index + record kind + node id | 48,932+ records ≈10 / 1 MB [E] |
| **fn.items** (IT-A, IT-B, IT-C, merged) | for every unique (all variants), rare template, the 1,527 corpus items and the synthetic items: the parsed item fields (what ParseRaw yields), the BuildModList outputs (modList, slotModList, weapon/armour/flask/tincture/jewel data), BuildRaw text, the helper results (FindModifierSubstring, NormaliseQuality, MutateMod, spawn weights, applyRange sweep, formatModLine, isModLineScalable, CanHaveMod). **No call traces**: IT-A's applyRange/parseMod sequence is structural. **No replay**: the runner runs the Rust chain end to end | query set | item source (`unique:<name>#<variant>`, `corpus:<stem>:<itemId>`, `synthetic:<n>`) + BLAKE3 of the raw text + field path | the caches hold IT-A 1,156 MB / 2.6 MB, IT-C 600 / 4.6 MB, IT-B 43 / 2.1 MB [M], mostly call traces. Golden ≈80 / 5 MB [E] |
| **fn.modstore** (MS-A, MS-B) | per all57 build: the store inputs (stores, mods, actors, items, cfgs, as MS-B's `l3_full.lua` serialises them), the full L3 matrix (Sum BASE/INC, More, Flag, Override, List, Tabulate, HasMod, Max, Min, GetCondition, GetMultiplier ±noMod, GetStat) with every tag path, MS-B's synthetic tag probes, and MS-A's own parts (ScaleAddMod at six scales, ScaleAddList, createMod, the synthetic MAX/MIN stores, the item and buff list stores). **Trivial answers elided** (§2.3) | query set; input = store graph | build key + store label + op + cfg name + mod names | caches: MS-B 587 / 24.8 MB, MS-A 334 / 15.5 MB [M]. With elision and the MS-A/MS-B overlap removed: ≈15 MB zstd [E] |
| **fn.skills** (AS `as_oracle`) | 5,015 cases (3,790 socket-group × weapon-setup cases over all57, 1,225 granted effects): skillTypes and skillFlags (names), skillData, the ordered skill mod list (mod/1), buffs, minion skills. Stat-map runs are `$unordered` where AS compares them order-free today | query set over fixtures | build key + socket group index + weapon setup, or `effect:<grantedEffectId>` | ≈40 / 3 MB [E] |
| **fn.config** (CO-A, CO-B merged; CO-C) | CO-A/B: for each of the 539 options × its values (4,148 cases), the resulting modList and enemyModList (mod/1), the error status, the radius placeholder, and the boss pre/post state. CO-C: `ConfigTab:BuildModList` for all57 and the all-vars synthetic build, plus every ModMap affix apply | query set | config var name + value / build key | CO-A/B ≈3 / 0.3 MB [E]. CO-C cache 48.3 / 1.25 MB [M, matched to CO-C by content] |
| **fn.tabs** (BM-B) | skills, config and party tab Load, UpdateSocketGroups, Save and round trip on all57 and the BM-B synthetics; ParseBuffs and exportBuffs; ProcessGemLevel for every gem × mode × level (> 50k values). The 5 values BM-B counts as pending today (`BUFFS_BOUNDS` = (3310, 5, 0): `str.Warcry` exports whose mods lose the tag key `warcryPowerBonus`, which `pob_gamedata::Tag` does not model, CCR-0014) are written as `{"$known":"CCR-0014","pob":…}`, with the count pinned to 5 in MANIFEST. BM-B's other buckets are declared up front (below the table), not left to K runs | query set over fixtures | build key + tab path / gem id + mode + level | caches 7.7 / 0.95 MB + 4.0 / 0.22 MB + 0.28 / 0.01 MB [M] |
| **fn.tools** (CT `ct_oracle`) | formatMod, formatSourceMod, formatTags and the round trip on every ModCache mod; type expressions on the frozen random RPN set; support compatibility for all (support, active) pairs × 3 variants **as a bitset**; buildSkillInstanceStats; validateGemLevel; statMap; the gem fns; the breakdown helpers on the frozen random inputs; formatNumSep and formatCalcVal. **Dropped**: the `cacheSkillUUID` check (a GlobalCache key, internal) and copyTableSafe (Lua tables) | query set | function name + input id (gem id, mod line, pair index) | ≈20 / 2 MB [E] |

**fn.tabs: BM-B's buckets.** BM-B tolerates three kinds of values today (BM-B.rs:1512-1526). Each
gets a fixed encoding at record time, because K = 2 runs cannot be relied on to see PoB's other
`pairs` orders (the tabs LUA-ORDER count went 12 → 18/19 between two runs of the same code,
BM-B.rs:1517-1518):

- **Flipped gems** (LUA-ORDER, `lua_order_gem`). An old-format `<Gem>` whose `gemId` and `skillId`
  match several variants (Vaal X / Vaal X AltX) gets the first variant in `pairs` order
  (SkillsTab.lua:474-479). The ambiguous gems are known from the data alone (BM-B's
  `ambiguous_variants`: same `gameId` and `grantedEffectId`; 10 in the corpus). For each one, the
  recorder loads the build once per variant, with that gem's `variantId` written into the XML,
  and records the gem's whole record (every load field under its `gems[n]` prefix, plus the
  `variantId` and `nameSpec` its Save writes) as one line:
  `{"$anyOf":[{record with variant A}, {record with variant B}, …]}`. The runner compares the
  Rust gem record as one object, so fields cannot mix variants. Record-time check: the
  forced-variant record of the variant PoB actually picked equals the record of the original
  load, byte for byte; otherwise the record fails and GOLD5 resolves it. MANIFEST pins the number
  of such gems (`orderDependent.gems`).
- **Reordered multi-line party labels** (LUA-ORDER, `lua_order_label`). The labels PoB builds
  with `pairs` (ParseBuffs `enemyModList`, PartyTab.lua:748; `otherEffects`, 939) are written as
  `{"$unordered":[line, …]}`, split on the newline as `label_lines` does today. The adapter
  encodes the Rust label for the same keys as its list of lines. Every multi-line party label is
  written this way (BM-B's `label_candidates`), whatever order PoB used in the recording run.
  MANIFEST pins their number (`orderDependent.labels`).
- **The `expected` bucket** (the third element, 1, of `TABS_BOUNDS` and `SYNTHETIC_BOUNDS`). It is
  a config input equal to its default, which a Save leaves out (ConfigTab.lua:969-970), so a
  round trip does not bring it back. BM-B checks the round trip Rust against Rust today
  (BM-B.rs:1412-1450). The golden records PoB's own round trip (PoB Save, then PoB Load into a
  fresh build), where PoB drops the same input, so the key is `{"$absent":true}` in the
  round-trip record and needs no tolerance. The two values are listed in GOLDEN-GATE.md.

### 1.4 Fuzz output sets

| Set (replaces) | What to record | Form | Stable key | Size raw / zstd |
|---|---|---|---|---|
| **fuzz.defence** (DF-A `dfa_random_vs_luajit`) | hitChance, armourReduction(F), applyDmgTakenConversion, takenHitFromDamage and reducePoolsByDamage on fixed-seed random inputs: **20,000 per function** (not 100,000, see §2.3), plus every case of the old 100k streams that raised (`$error`) or hit a bucket (inf, NaN, zero). The generated inputs are not stored: a **frozen copy of today's generator** lives in `pob-golden`, with today's four seeds (DF-A.rs: 0xDFA001 for hitChance and armourReduction(F), 0xDFA002 for applyDmgTakenConversion, 0xDFA003 for takenHitFromDamage, 0xDFA004 for reducePoolsByDamage), and each shard records a digest of its inputs. **PoB's `pairs` orders are stored**, as a recorded input, for the two functions that take them (below the table) | fuzz output set | function + case index (the index into that function's full 100k stream) | ≈7.5 / 3.2 MB [E: 120k × 3 numbers + 60k orders] |
| **fuzz.offence** (OF-A `ofa_fuzz_vs_luajit`) | calcDamage, calcAilmentSourceDamage, the area/radius helpers, cooldown, duration, totemLife and calcResistForType on the 1,500-case fixed-seed streams: results, breakdown lines and written tables. Inputs stored, since they include tables | fuzz output set | helper name + case index | ≈5 / 0.5 MB [E] |

**fuzz.defence: generation and the recorded orders.**

- **Regeneration rule.** The runner always generates each function's **full** stream, exactly as
  DF-A does with `DFA_N` = 100,000 today, and then picks the stored case indices from it. A case
  index is the position in that function's 100k stream. This matters because hitChance,
  armourReductionF and armourReduction draw from **one** `Rng(0xDF_A001)`, in that order
  (DF-A.rs:468-470): armourReductionF's case 0 comes after all 100,000 hitChance draws. Cutting
  the generation to 20,000 would change the inputs of the second and third functions, and the
  edge-case indices (which point into the full streams) would name other inputs. The generator is
  cheap: 600,000 small tuples, well under a second [E]. The input digest in each shard header covers
  the full generated streams.
- **The orders are inputs.** Two of the functions see PoB's `pairs` order, which LuaJIT
  randomises per process, and the Rust tables are built in that order today:
  - **takenHitFromDamage**: the Lua side returns its shift-table key order (`DFA_T`,
    DF-A.rs:319-325), and Rust builds `damageShiftTable[dtype]` in that order (DF-A.rs:797-806).
  - **reducePoolsByDamage**: `keyOrder(dmg)` and `keyOrder(dmg2)` (`DFA_P`, DF-A.rs:327-356)
    give the orders of both damage tables, and Rust builds them in those orders
    (DF-A.rs:1137-1176). With a NaN pool, `m_min` over `pairs(damageTable)` (CalcDefence
    401-404) depends on the order, not only the rounding.

  So the golden line of a case of these two functions is
  `[<function>#<case index>, {"orders":[o1, o2, …]}, {"$perOrder":[v1, v2, …]}]`. Each `o` is the
  order PoB used in one recording run, as a list of damage-type names (for reducePoolsByDamage a
  pair `[dmg order, dmg2 order]`, the second empty when the case has no `dmg2`). Each `v` is PoB's output in that run. Runs that saw the same order are
  stored once.
- **How the adapter uses them.** `fuzz_helper("takenHitFromDamage" | "reducePoolsByDamage", input)`
  receives the generated case plus one order, and builds the Rust tables in that order, as DF-A
  does today. The runner calls it once per stored order and compares each result with its `v`.
  When the refactor replaces the order-dependent tables with ordered or keyed types, the adapter
  still takes the order and inserts the keys in it. If a refactor makes a result independent of
  the order on purpose, the cases whose `v`s differ between orders fail, and are blessed with a
  reason (§3.6).
- **K runs.** The K = 2 recording runs (§3.1) each produce their own (order, output) pair, and
  both pairs are kept. `--verify` (§4.2) treats a new order like a new `$anyOf` alternative: it is
  reported and folded in. A known order must give byte-identical output.
- **bitExact.** The `bitExact` pin of these two functions (§3.5) counts (case, order) pairs, and
  is only meaningful because Rust runs on PoB's order. Without the stored orders the NaN-pool
  cases would fail, and the others would lose bit-exactness through summation order.

### 1.5 The special corpora that only segment tests reach today

The rule: **every build any segment test runs on becomes an E2E fixture** (e2e.ca, plus e2e.int3
where the area shows in breakdowns). Where the segment test checked coverage (arms, hooks,
families), the **recorder** checks the same coverage on PoB's side at record time and writes it to
`coverage.json`. The golden then contains the outputs that coverage produces. The runner does not
look at Lua-shaped arms, because the refactor removes them.

| Corpus (test) | Today | Becomes |
|---|---|---|
| PF-A (`corpus/PF-A`, 1 build + all57 hits) | CalcPerform 1-635 helpers (mergeBuff, doActorLifeMana, applyEnemyModifiers, …) on injected state | e2e.ca fixture `special/PF-A/*`. all57 is covered by e2e.ca already. No function golden: these are Lua function boundaries |
| PF-D spectres and Blight (`corpus/PF-D`) | CalcPerform 1943-2307, 100 % on the unit corpus | e2e.ca + e2e.int3 fixtures. The minion outputs are compared under `main.Minion.*` |
| PF-F party (`corpus/PF-F`) | CalcPerform 2997-3496, party and curses | e2e.ca fixture. Party effects show in main output and the CALCS lists |
| SK-A (87 builds, one per act_int hook) | every skill hook hit, state per hit | e2e.ca fixtures. At record time, the recorder runs PoB with the hook probes (existing patcher) and checks that every one of the 68 act_int hooks is **reached by a compared key** in ≥ 1 fixture (`coverage.json`: hook → fixtures, with the witness kind). A hit counts only in one of two ways: (a) the hook ran for the fixture's **main skill and main skill part** in the MAIN env, not only inside `calcFullDPS` or for another socket group or part; if no fixture has that, the recorder derives one (main socket group and skill part set to the hook's skill, §3.1), as for TR-B; or (b) a mutation witness: with the hook made a no-op by the patcher, at least one e2e.ca key of that fixture changes. A hook with neither fails the record |
| SK-B (127 builds) | the other skill files' hooks | e2e.ca fixtures, with the same coverage check and the same two witness kinds. **New**: uncovered hooks fail at record time (SK-B only prints them today) |
| TR-A (60 builds: PoB's own TestTriggers_spec) | trigger handlers through a 600-line test copy | e2e.ca fixtures (the builds stay, the scaffold goes) |
| TR-B (35 builds, one per configTable arm the spec does not reach) | the real `calcs.triggers`, all 82 arms, all 3 handlers | e2e.ca fixtures. At record time: all 82 arms and 3 handlers hit by ≥ 1 fixture (`coverage.json`), and each arm's fixture has the triggered skill as main skill, with `main.SkillTriggerRate` present in the golden. If not, the recorder derives a fixture that sets that main socket group (§3.1 derived fixtures) |
| PS-B (62 synthetic jewel builds, made in code) | BuildAllDependsAndPaths | the recorder **writes them out as XML** under `builds/special/PS-B/`. Then fn.spec (whole spec) **and** e2e.ca (their timeless/abyss mods reach the outputs) |
| PS-C synthetic cluster builds (3_29, 3_13, made in code) | cluster subgraphs | written out as XML; fn.spec + e2e.ca |
| CS2 (`corpus/CS2`, 2 synthetic: one restarts, one is blocked by tags) | the initEnv 1433 restart, PoB-compared | e2e.ca fixtures; the recorder checks that PoB restarted on one and not on the other (`coverage.json`) |
| CS2 overrides (repItem, toggleFlask, two-handed, `$removeNodeIds`, forced restart on all57) | `cs2_l4_overrides`, Rust-only hand expectations | e2e.mcq: the same overrides as misc-calculator calls, **now against PoB** |
| MI mirage families (Mirage Archer, Reflection, Tawhoa's Chosen, General's Cry, Sacred Wisps) | `calcs.mirages` with PoB's outputs copied in (inventory: the biggest tautology) | e2e.ca on the all57 and fresh builds that use each family, plus the mirage block (§1.2). The recorder checks that each family has ≥ 1 fixture where PoB built a mirage. If a family has none, it derives one (main socket group set to that skill) |
| OF-I dps30 CombinedDPS / TotalDot | strict on the dps30 main env | e2e.ca already exact; listed as required keys (§1.2) |
| DF-B..DF-G, OF-B..OF-F, CS0, U0 slice5 unit corpora (≈ 95 builds) | segment-only corpora (not named in the scope, but in the same situation) | e2e.ca fixtures, at no extra harness cost |
| CS1, CS3 (all57 only), AS L4, CA L5 | segments over all57 | nothing new: all57 is in e2e.ca |
| PF-B 1222-1240 (`minion.uses`, "needs itemsTab slots") | uncovered (inventory B) | not covered today. GOLD3 adds a derived fixture if one of the corpora reaches it (a minion with item-granted uses); otherwise §7 lists it as a known gap |

**Derived fixtures** (§3.1) are XML variants that the recorder writes: a different main socket
group, Full DPS flags, a config toggle. They are committed like any build, with
`derived_from: <build key> + <transform>` in the manifest.

### 1.6 Dropped, with the reason

| Test(s) | Reason |
|---|---|
| all CALC-SEGMENT: CS0, CS1, CS2 L4, CS3, PF-A..PF-G, OF-A L4, OF-B..OF-I, DF-A probe, DF-B..DF-G, AS L4, CA L5, MI, TR-A, TR-B, SK-A/SK-B hook dumps | welded to Lua line boundaries (calc doc §7). Their builds move to E2E (§1.5). Their gates were the softest in the suite (98 % thresholds, unbounded excuse buckets: inventory §3) |
| MP0 scan steps (`scan_matches_parse_oracle`, `helpers_match_parse_oracle`, `generated_list_sizes`), MP1, the L1 parts of MP2-MP5, all `list_shape` tests | MP1-MP5 L1 are subsets of MP6. The scan trace, the list sizes (against PoB's source and the dump manifest) and the list shape check the Lua layout. MP0's ModCache-tier test and its hand-value lookups move (§1.7) |
| MP2-MP6 closure calls in the Lua call shape | replaced by fn.parser.fuzz (the same inputs, as lines) |
| MS-B `tag_div_no_reader` | PoB against patched PoB; it justified one Lua-mirroring choice |
| PT `tree_nodes_order_varies` | asserts nothing |
| CO-A/CO-B/SK-A/SK-B `rows_cover_range`, U0 `registry_complete`, `provenance_check_and_coverage` | provenance |
| P3, ORC-X, oracle_harness, worker_smoke, u0_smoke, src tests of `lua/`, patcher, replay, schema, nondet, runner, testkit | oracle tooling. The PoB-free tests of the oracle crates are in §1.7 |
| CT `cacheSkillUUID`, `ct_copy_table_safe`, `ct_data_census`; AS `as_census` | GlobalCache key internals / Lua-table emulation / data facts frozen with `data.bin` |
| pob-dump `parse_oracle`, `dump` | validate PoB's own files. With the data frozen, they have nothing left to guard |
| pob-dump `codegen` | its check compares the generated files with live PoB. After the move (§5 step 5) the generated files are frozen sources |
| pob-dump src tests (4 modules, hand and standard vectors) | they test the dumper, which is deleted with the data frozen |
| all UI-UNIT and UI-E2E against PoB (pob-ui-oracle) | user decision: no UI goldens. Behavioural UI scenarios come later, out of scope |
| LUA-SEMANTICS tests that call real LuaJIT (pob-lua L0, luastr, tier0_len, hyg1_len's `table_len_matches_luajit` and `table_len_with_string_keys`, l0_ljtab). hyg1_len's third test, `ctor_pairs_follow_field_order`, is Rust vs Rust (no LuaJIT) and guards the product's table iteration order, which order-dependent goldens rely on. GOLD1 moves it to `crates/pob-calc/tests/ctor_pairs.rs` | `mlua` goes. Where product output still depends on emulated Lua semantics (`string.format`, pairs order, `math.random`), §7 asks whether to freeze their LuaJIT vectors as goldens before deletion (cheap: < 5 MB) |

### 1.7 Tests in the deleted crates that need no PoB

CUT1 deletes `crates/pob-oracle` and `crates/pob-ui-oracle` whole (§5 step 5). Some of their tests
never run PoB. They check Rust against hand values, or Rust against Rust, and the inventory marks
them KEEP or KEEP-REWRITE (inventory appendix A-D). Each one below gets a destination and an owning
unit, or an explicit drop with a reason.

- **When.** The owning unit moves the test **before G**. The move deletes it from the oracle crate
  in the same commit, so there is never a second copy. GATE then runs it at its new place in the
  PoB-free worktree (§4.3). CUT1's step 4 (§5) checks that nothing in this table is left behind.
- **Inputs after the move.** `data.bin` comes through the product's own data lookup, which VENDOR
  points at the asset bundle. Builds come from `tests/golden/builds/` (GOLD2). No moved test reads
  `data/dump`, `.parity` or `.PathOfBuilding`, and none needs `mlua` or the `probe` feature.

| Test today | Inventory | Destination | Owner | What changes in the move |
|---|---|---|---|---|
| BM-A.rs `xml_reader_writer` | KEEP; hand values | `crates/pob-calc/tests/model_xml.rs` | GOLD5 | nothing: it reads no input |
| BM-B.rs `set_buff_exports`, `fresh_build_entry_points` | KEEP-REWRITE; hand values | `crates/pob-calc/tests/tabs_fresh_build.rs` | GOLD5 | `data.bin` from the product lookup instead of `data/dump`; the file keeps its `SERIAL` lock |
| PWR.rs `pwr_coroutine_steps` | KEEP-REWRITE; Rust vs Rust | `pob-golden`, binary `golden_e2e`, module `invariants::power_steps` (beside e2e.d13) | GOLD6 | Today the misc calculator answers from PoB's recorded calls (`calc_replay`, a seam CUT2 removes). The rewrite runs the real calculator on the same build (`builds/all57/allflame_exp_000.xml`, `BUILDS[1]`) for both the direct and the stepped runs. The assertions are unchanged: frames = 2 + nodes / 101 with the slow clock, one progress callback per time yield, the callback once per run, monotone progress in 0..100, stepped = direct exactly, an error ends the coroutine, dev mode re-raises it. GOLD6 records the runtime; above 60 s in debug it keeps Offence/Defence only |
| PWR.rs `pwr_stat_fns` | KEEP-REWRITE; hand values + dump hashes | `crates/pob-calc/tests/power_stats.rs` | GOLD6 | `data.bin` from the product lookup. The transform-hash part stays: `data.bin` is frozen, so it can only fail on a new data snapshot, which is when it is needed |
| OF-C.rs `ofc_combine_stat_modes` | KEEP-REWRITE; CALC-UNIT | `crates/pob-calc/tests/combine_stat.rs` | GOLD12 | Today env, actor and active skill come from an injected PoB `offence:2174` dump (`pob_oracle::inject`). The rewrite takes them from `initEnv` (MAIN) on an all57 fixture with an attack main skill. The test replaces output, skillFlags, skillData, breakdown and globalOutput anyway, so the hand-made MainHand/OffHand tables and the expected values (the Lua expressions restated) are unchanged |
| U0.rs `compile_zero_unknown_keys` | KEEP; pinned counts, reads `data/dump` | merged into `crates/pob-gamedata/tests/dump_load.rs` | GOLD1 | `data/dump` goes in step 7, so the test starts from `data.bin`: decode, `Data::from_tree` (the same strict schema path, so an unknown key still fails), the same pins (gems 871, ModCache 23,207, 41 tree versions, the SkillType values, foulbornMap 248, nodeIDList 1,937, powerStatList 85, the Arc identities, the typed/dynamic root split), then encode and decode to an equal tree, and the release-only < 1 s load gate (which CI's release perf job enforces, §5 step 8). Lost: parsing PoB's JSON dump files. That guards the dumper, which goes |
| U0.rs `unknown_key_is_an_error` | KEEP | `crates/pob-gamedata/tests/dump_load.rs` | GOLD1 | nothing: it builds its own tree |
| IT-A.rs `parse_raw_units`, IT-B.rs `item_core_units`, IT-C.rs `item_tools_units` | KEEP-REWRITE; the hand-value cases of those files | `crates/pob-calc/tests/item_units.rs` | GOLD9 | `data.bin` from the product lookup. One IT-A case (a crafted RARE re-parse) installs `item_parse::replay` stubs, which need the `probe` feature. GOLD9 moves it onto the real callees and, if that changes the asserted outcome, asserts the real outcome after checking it against PoB, which still exists then |
| MP0.rs `modcache_tier_serves_cached_lines`, `appendmod_and_lookups` | KEEP-REWRITE; hand values | `crates/pob-modparser/tests/mod_cache.rs` (a new tests dir) | GOLD7 | `data.bin` from the product lookup. Kept: all 23,207 cached lines served exactly, results are copies, a novel line is parsed and memoised, the appendMod and lookup cases. Dropped: the informational comparison with PoB's native parse (`parse_oracle.jsonl`), because fn.parser.lines is that comparison, gated |
| CT.rs `ct_global_cache` | KEEP-REWRITE (the cache part); hand values | `crates/pob-calc/tests/global_cache.rs` | GOLD11 | `data.bin` from the product lookup |
| AS.rs `as_copy_actor_keepalive` | KEEP-REWRITE; PERF/MEMORY | `crates/pob-calc/tests/copy_actor_keepalive.rs` (beside `leak1_cycles.rs`) | GOLD11 | `.parity/builds/upstream_Dual_Savior.xml` → `tests/golden/builds/all57/upstream_Dual_Savior.xml` |
| CS2.rs `cs2_l4_overrides` | RETIRE / KEEP-REWRITE as E2E; hand expectations | replaced by e2e.mcq | GOLD6 | the same overrides, now against PoB (§1.5) |
| CA.rs `ca_d13_fixed_point`, `ca_level_cost_cleared*` | KEEP | e2e.d13, e2e.levelcost in `golden_e2e` | GOLD3 | §1.2 |
| `src/par.rs` (6 tests) | KEEP while the pool is used | `crates/pob-golden/src/par.rs` | GOLD1 | moved with the pool (§3.3), tests unchanged |
| `src/diff.rs` tests `tolerance`, `stores_multiset_and_order`, `nondet_values_and_orders`, `unordered_lists_with_nested_orders`, `store_alternatives_per_name` | KEEP-REWRITE | `crates/pob-golden/src/compare.rs` | GOLD1 | The compare engine takes over num_close, the leaf and tree equality, and the mod-store multiset with its order report (§3.5). The tests move with it, with their inputs rewritten from the oracle's `$store` and nondet-path forms to the golden markers (`$unordered`, `$anyOf`). `units` is dropped: it reads the Lua unit ranges of calc-architecture.md |
| pob-ui-oracle `u53_perf.rs` | KEEP; PERF, no oracle | `crates/pob-app/tests/tree_perf.rs` | GOLD13 | It drives `PobApp` through pob-ui-oracle's scenario runner and reads the tree assets from the PoB checkout. The move puts a minimal driver in the test file, uses the asset bundle, and reads its 4 builds (all all57: allflame_exp_001, allflame_exp_048, the Cospris CoC and Generals Perforate builds) from `tests/golden/builds/all57/`. CI's release perf job runs it (§5 step 8) |
| pob-ui-oracle PoB-free UI parts: `u26_progress_steps`, `u48_progress_steps`, `u38_undo_round_trip`, `u43_share_rust`, `u44_rust_unit`, `u45_disable_party_effects` | KEEP / KEEP-REWRITE; UI-UNIT | `crates/pob-app/tests/` | GOLD13 | Moved if they run on `PobApp` without pob-ui-oracle's runner. Otherwise they are dropped under the user's decision (no UI goldens, UI scenarios later), and each drop is listed in GOLDEN-GATE.md |

One product test also reads `.parity`: `crates/pob-calc/tests/leak1_cycles.rs`, through
`pob_testpaths::parity_build` (it is the only one outside the oracle crates and pob-testpaths).
GOLD2 re-points it at `tests/golden/builds/all57/<stem>.xml`, so it runs in the PoB-free gate
(§4.3). Its LUT script path, which reads the PoB checkout, moves to the bundle with VENDOR.

**Dropped although they need no PoB:**

| Test | Reason |
|---|---|
| CT `ct_data_census`, `ct_copy_table_safe`; AS `as_census` | §1.6: data facts frozen with `data.bin`, Lua-table emulation |
| MP0 `generated_list_sizes` | it reads PoB's source and the dump manifest (§1.6) |
| PF-B `pfb_ally_life_pools` | it asserts the identity of a Lua module-level local (inventory: RETIRE) |
| `u31_rebuilt_slot_controls` | it checks control-table structure (inventory: RETIRE) |
| CS3 `cs3_object_key_order_class`, PF-G `pfg_replay_hook_roundtrip`, OF-B `ofb_attribution_patterns` | self-tests of the probe and replay harness, which goes |
| PF-A `pfa_stages_file_patches`, U0 `registry_complete`, `provenance_check_and_coverage`, `stages_*` | provenance and PoB source (§1.6) |
| `u0_smoke`, `worker_smoke`, `oracle_harness` | smoke tests of the oracle boot. e2e.ca supersedes them |
| `src/runner.rs` (query cache), `src/testkit.rs` (cache refresh) | they cache PoB runs. The committed goldens replace the cache |
| pob-ui-oracle `u29_data_invariants` | it checks data facts that the UI port's ORDER notes rely on (inventory: RETIRE); the facts are frozen with `data.bin` |
| pob-ui-oracle src tests: `markers` (2), `oracle_cache` (9), `patches` (4), `runner` (4), `sg_bridge` (4) | self-tests of the UI oracle tooling: patch anchors in the pinned PoB, the Lua-side cache, the scenario runner against `run_headless`, the bridge's glob and net fixtures (inventory: RETIRE with the oracle tooling) |

### 1.8 Product tests outside the oracle crates that read PoB, `data/dump` or the deleted crates

VENDOR moves the product's **runtime** reads to the asset bundle and leaves the tests alone
(VENDOR item 5: "Tests and oracle crates may keep reading the PoB checkout. They are deleted at
the cut-over"). So this plan owns every product test that still reads `.PathOfBuilding`,
`data/dump`, `.parity`, or a file inside pob-oracle, pob-ui-oracle or pob-dump. Without this
section they would first fail in CUT1's CI unit job.

How the list was made (rust-port `0dbdab33`):
`grep -rnE '\.PathOfBuilding|POB_ROOT|POB_SRC|pob_testpaths|data/dump|\.parity|pob-ui-oracle/|pob-oracle/|pob-dump/'`
over `src/`, `tests/` and `benches/` of every workspace crate except pob-oracle, pob-ui-oracle,
pob-dump and pob-testpaths, then each hit read. The hits in product code (app/pob-desktop with
its `sg_demo` bin, app/pob-web, pob-host's checkout search in `loader_native.rs`, pob-gamedata's `#[path]` includes
of pob-dump's generated files) are VENDOR's and §5 step 5's.

Rules for every row:
- **The bundle** means VENDOR's asset bundle, found through the lookup the product uses. No test
  searches ancestors for `.PathOfBuilding` any more.
- **A missing input fails.** No `pob_testpaths::require` skip, no early `SKIP:` or `PENDING`
  return, no `POB_ALLOW_SKIP`.
- **Timing.** GOLD13 waits for VENDOR. GOLD1 and GOLD2 can finish before VENDOR merges. A test they
  move or re-point keeps reading `data/dump` for its data.bin until then, and GOLD13 switches it to
  the bundle. §4.3 fails on any test that still reads the old places.

| Test | Reads today | Destination | Owner | What changes |
|---|---|---|---|---|
| pob-app `tests/fup5_teardown.rs` (2 tests), `tests/u54_worker_calc.rs` (4 tests) | `pob_testpaths::require(pob_root(), …)` and `require(dump_dir(), "data/dump")` (fup5_teardown.rs:36-37, u54_worker_calc.rs:39-40): PoB's `src` and `runtime` as the headless host's assets, `data/dump` for data.bin, PoB's `src` as the timeless LUT script path. Also the fixture user dir `crates/pob-ui-oracle/fixtures/user1` (fup5_teardown.rs:50, u54_worker_calc.rs:53): `Settings.xml` and 6 builds | stay in pob-app. Assets, data.bin and LUT data from the bundle. `Settings.xml` and the README move to `crates/pob-app/tests/fixtures/user1/`. The 6 builds are copied at test start into a temporary user dir from `tests/golden/builds/all57/` (all 6 are byte-identical to their `.parity/builds` files today, checked with `cmp`), so they are not committed twice | GOLD13 | `root()` becomes a bundle lookup that panics when the bundle is missing |
| pob-app `src/tabs/party.rs` `notes_desc_is_the_long_string` | `pob_src()`: PartyTab.lua's `notesDesc` | dropped | GOLD13 | It checks the port's copy `NOTES_DESC` against PoB's source. After VENDOR the copy is the source (VENDOR item 3 moves such text into code or the bundle) |
| pob-sg `tests/font_atlases.rs`, `font_cursor.rs`, `font_draw.rs`, `font_width.rs`, and `font_goldens.rs` `goldens_match_reference` (all through `font_util::locate`) | PoB's `runtime/SimpleGraphic/Fonts` (`$POB_ROOT` or an ancestor) | stay. `font_util::locate` becomes the bundle lookup | GOLD13 | `font_goldens.json` stays as committed data. Its generator `crates/pob-ui-oracle/probe/rfont_ref.py` (an independent transcription of SimpleGraphic's `r_font.cpp`; it reads only the font files) moves to `crates/pob-sg/tests/font_ref/rfont_ref.py`, and the regenerate line in `font_goldens.rs` points at it and at the bundle's fonts |
| pob-sg `font_goldens.rs` `probe_cases_match_goldens` | `crates/pob-ui-oracle/probe/cases.lua` (font_goldens.rs:170-176) | dropped | GOLD13 | it checks that the Wine probe's case table equals the goldens' strings. The probe goes with pob-ui-oracle |
| pob-sg `tests/font_probe.rs` (`real_simplegraphic_metrics`, `comparator_self_test`) | `crates/pob-ui-oracle/probe/expected/metrics.json` | dropped | GOLD13 | That file was never recorded, so the test prints `PENDING` and passes (a silent skip today). Its purpose is parity with the real `SimpleGraphic.dll` under Wine: UI parity with PoB, which the user ruled out. The self-test only tests that comparator |
| pob-sg `src/font/mod.rs` tests that load fonts (`tgf_parse_counts`, `height_map`, `known_widths`, `tofu_width`, `cursor_index`, `layout_alignment_and_escapes`, `offscreen_line_still_applies_escapes`, `atlases_white_rgb`, `glyph_cell_has_ink_top_down`) | `pob_testpaths::pob_root()` fonts; skip with `POB_ALLOW_SKIP` | stay. `tests::locate` becomes the bundle lookup; pob-sg's `pob-testpaths` dev-dependency goes | GOLD13 | - |
| pob-sg `src/image.rs` `pob_assets_decode` (and the ignored `pob_all_tree_versions_decode`) | PoB's `src/Assets` and `src/TreeData/*` images; returns early with `SKIP:` (image.rs:1611-1614) | stay, on the bundle's images (the art and tree sheets VENDOR bundles) | GOLD13 | the `SKIP` return becomes a failure. The floor of 130 images stays, or is re-pinned to the bundle's count with the reason in GOLDEN-GATE.md |
| pob-sg `src/color.rs` `palette_equals_dll_table` | PoB's `runtime/SimpleGraphic.dll`; returns early with `SKIP:` | dropped | GOLD13 | It checks the port's `PALETTE` against the colour table inside PoB's DLL. The DLL is PoB's Windows runtime, not an app asset, so the bundle does not carry it. `PALETTE` is a constant, and the `read_color_escape` hand tests stay |
| pob-host `src/loader_native.rs` `pob_assets_load_async` | PoB's `src/Assets` and `src/TreeData/3_27` (`locate_src`); returns early with `SKIP:` | stay, on the bundle | GOLD13 | as `image.rs`: `SKIP` becomes a failure |
| pob-sg-wgpu `tests/t5.rs` `t5_text`, `t5_text_dpi_1_5` | PoB's fonts (`locate_pob`, t5.rs:160-191). A missing font skips under `POB_T5_ALLOW_SKIP=1`, the same variable that allows a missing GPU adapter | stay; fonts from the bundle | GOLD13 | The font load no longer consults `allow_skip`: missing fonts always fail. `POB_T5_ALLOW_SKIP` then covers only "no GPU adapter" (t5.rs:533-556). A new GPU-free test, `t5_fonts_load`, loads the bundle's `FontSet` and renders the text frame on the CPU raster, so CI, which sets `POB_T5_ALLOW_SKIP=1`, still checks the font part |
| pob-calc `benches/INT-2.rs` | `data/dump/data.bin` (`POB_DUMP_DIR` or an ancestor) and `.PathOfBuilding/src` for the timeless LUT data (`POB_SRC`), INT-2.rs:254-268 | stays (it is Rust-only). data.bin and LUT data from the bundle; builds are command-line paths (`tests/golden/builds/…`) | GOLD13 | A per-build error and "no builds given" exit non-zero. Its doc stops naming the LuaJIT side. If PROF1 has replaced the bench by then, GOLD13 deletes it instead and says so in GOLDEN-GATE.md |
| pob-calc `tests/leak1_cycles.rs` | `dump_dir()`, `pob_src()` (LUT data), `parity_build()` | builds from `tests/golden/builds/all57/`; data.bin and LUT data from the bundle | GOLD2 (builds, §1.7), GOLD13 (bundle) | - |
| pob-gamedata `tests/dump_load.rs` | `data_bin()`: `data/dump/data.bin` | the bundle's `data.bin` | GOLD1 (merges U0's tests in, §1.7), GOLD13 (bundle) | - |
| pob-lua `tests/common/mod.rs` (`pob_src()`, used by `l0_format`, `l0_math`, `l0_string`); pob-ui-core `tests/tier0_*.rs` (`.PathOfBuilding/src`) | PoB's source, LuaJIT through `mlua` | deleted with the `mlua` tests (§5 step 5) | CUT1 | they test the emulated Lua semantics against real LuaJIT (§1.6; §7 question 3) |

**pob-testpaths.** Its dev-dependents today are **pob-app, pob-calc, pob-gamedata, pob-lua and
pob-sg** (each `crates/*/Cargo.toml`), besides the deleted crates. pob-host and pob-sg-wgpu do
not use it. After the rows above, none of the five calls it: GOLD13 removes the dev-dependency
from pob-app, pob-calc, pob-gamedata and pob-sg, and pob-lua's goes with its L0 tests. CUT1 then
deletes the crate (§5 step 5).

---

## 2. Size budget and storage

### 2.1 Layout

```
tests/golden/
  README.md                      how to run, bless, review (from §3.6)
  MANIFEST.json                  every file: set, path, BLAKE3, line count, compared-count and bitExact pins, sizes, origin
  coverage.json                  record-time coverage witnesses (TR arms, SK hooks, MI families, CS2 restart)
  builds/
    all57/*.xml                  from .parity/builds (2.9 MB raw)
    fresh/*.xml                  from .parity/fresh (4.9 MB raw; see §7 privacy)
    special/<unit>/*.xml         from crates/pob-oracle/corpus/* and the synthetic builds (≈ 4 MB raw)
    derived/<unit>/*.xml         recorder-made variants
  e2e/<set>/<corpus>/<stem>.jsonl         plain, sorted
  fn/<area>/<set>[.<shard>].jsonl[.zst]
  fuzz/<set>.<shard>.jsonl.zst
  overlay/<same relative path>.bless.jsonl   deliberate deviations from PoB (§3.6)
```

The location is the workspace root, not inside the crate. Goldens are product test data that
outlive any crate, and the refactor may split or rename crates.

### 2.2 Format choice

- **JSON lines, sorted, one value per line.** A behaviour change shows as a changed line, and an
  added or removed key as a line in `git diff`. Small corrections stay small: the PoC showed git
  packs the 57 plain CA files (3.9 MB) into 512 KB [M], within 10 % of per-file zstd (576 KB)
  [M].
- **`.jsonl.zst` only for a file whose plain JSONL would exceed about 4 MB** (fn.modstore, fn.tree,
  fn.items, fn.lut, the larger fn sets, fuzz). The rule is per file, so the E2E sets, one file per
  build, are all plain. Git cannot delta-compress zstd blobs, so each re-record of such a shard
  adds its full
  size to history. Overlays (§3.6) keep shards immutable, so in practice they are written once.
- **Not CBOR.** It is not diffable, and after zstd its size advantage over JSON is small. We need
  reviewability more than parse speed (the largest set parses in seconds).
- **No git LFS.** The total is about 61 MB packed (about 180 MB checked out), and every file is
  ≤ 8 MB on disk (GitHub warns at 50 MB and rejects files at 100 MB). LFS would add a hosting
  dependency to a standalone repo for no gain.

### 2.3 Budget, elision, dedup and sampling

Two measures:
- **Working tree**: the bytes of the checked-out files. Plain files count at full size, `.zst`
  shards at their compressed size.
- **Packed**: the `git gc --aggressive` pack of `tests/golden/` alone. This is what the history and
  a clone add. Plain JSONL packs about as well as zstd ([M]: 3.9 MB of CA lines → 489 KB); `.zst`
  shards pack at their own size.

GOLD1 measured every row that the existing caches can answer (2026-11-13; method and raw numbers
in appendix A, "GOLD1 measurements"): the cached PoB results were re-encoded into the `golden/1`
line form, written one file per build or per 4 MB shard, and measured as raw bytes, per-file
`zstd -19` and the `git gc --aggressive` pack of the set alone. **[M]** is such a measurement;
**[M~]** a measurement of part of the set, scaled as the row says; **[E]** a row no cache holds in
a usable form (no recording exists yet), left at the plan's estimate. Its unit measures it at
record time, and MANIFEST holds the real sizes from then on.

| Area | Working tree | Packed | Reduction used |
|---|---:|---:|---|
| e2e.ca all57 | 3.9 MB [M] | 0.49 MB [M] | none (plain JSONL) |
| e2e.ca fresh 100 | ≈7.3 MB [E] | ≈0.9 MB [E] | none. No cached PoB result of the fresh corpus exists; scaled from all57 by main keys (85,603 / 45,954) |
| e2e.ca special and derived (≈520 builds) | ≈36 MB [M~] | 1.4-4.5 MB [M~] | none. The probe caches hold PoB's outputs of 87 distinct non-all57 builds: their main block is 2.37 MB raw, 0.43 MB zstd, a 0.09 MB pack (27 KB per build). Scaled by all57's full/main byte ratio (2.57) and to 520 builds. The pack range: the 87 builds' own pack ratio (they share much) to all57's |
| e2e.ca mirage block | < 0.2 MB [E] | < 0.2 MB [E] | none |
| e2e.int3 | 41.4 MB [M] | 2.5 MB [M] | none (plain JSONL). An upper bound: every Tabulate answer plus the whole cached calcsEnv dump to depth 3, which holds more than the breakdowns. Largest file 1.16 MB |
| e2e.bm | ≈10.5 MB [M~] | ≤ 0.76 MB [M] | the original-XML loads only: the 114 cached loads are 21.0 MB raw, 1.44 MB zstd, a 0.76 MB pack; the golden keeps 57 of them |
| e2e.pwr, e2e.mcq, d13, levelcost | ≈13 MB [E] | ≈1.4 MB [E] | PWR: power tables only. The replay cache holds recorded calc outputs, not power tables, so it does not measure the golden |
| fn.parser.lines | 0.54 MB [M] (17.3 MB raw) | 0.54 MB [M] | `line → {mods, extra}`: `passes` and `sources` dropped; 5 shards of 4 MB, 80,993 records |
| fn.parser.fuzz, fn.parser.jewel | ≈1.8 MB [E] | ≈1.8 MB [E] | MP7 empty calls elided (lossless) |
| fn.lut | ≈5 MB [E] | ≈5 MB [E] | columnar per (type, seed) |
| fn.tree | ≈8 MB [E] | ≈8 MB [E] | cross-version record dedup (lossless) |
| fn.spec | ≈1 MB [E] | ≈1 MB [E] | none |
| fn.items | ≈5 MB [E] | ≈5 MB [E] | call traces dropped (structural). The current query cache holds only the IT counts, not the records |
| **fn.modstore** | **≈28 MB [M~]** | **≈28 MB [M~]** | trivial matrix answers elided. MS-B measured: 24.6 MB zstd (818 MB raw for MS-A and MS-B together). Of MS-B's 6,950,664 matrix results, 3,127,148 are trivial and elided; what remains is dominated by the **synthetic tag probes** (5.68M lines, 290 MB raw, about half of MS-B's bytes) and the GetMultiplier / GetCondition / GetStat queries (3.1M lines, which this measurement did not elide). MS-A's own parts (createMod, ScaleAdd, ScaleAddList: 55 MB raw) ≈ 3.2 MB zstd by raw share; MS-A's matrix (16.2 MB zstd with it) is the overlap that is removed. **Above the 20 MB of the fallback below**: GOLD10 applies it, or elides the trivial answers of the multiplier, condition and stat queries and of the synthetic probes as well, with the record-time check |
| fn.tabs (BM-B) + fn.config CO-C | 1.9 MB [M] (61 MB raw) | 1.9 MB [M] | as `.zst` shards: one CO-C build is 38 MB of plain JSONL, so CO-C cannot stay plain |
| fn.skills, fn.config CO-A/B, fn.tools | ≈8 MB [E] | ≈5.3 MB [E] | support pairs as a bitset. CO-A/B (≈3 MB) stays plain |
| fuzz.* | ≈3.7 MB [E] | ≈3.7 MB [E] | DF-A sampled to 20k per function; its `pairs` orders stored (§1.4) |
| builds/ (XML) | 18.0 MB [M] | 3.3 MB [M] | none. all57 2.8 MB, fresh 4.9 MB, the 418 corpus XMLs 10.2 MB; the synthetic and derived fixtures come on top |
| **Total** | **≈193 MB** (≈112 MB of it plain E2E JSONL) | **≈74 MB** (with fn.modstore at 28 MB) | **caps: 80 MB packed, 250 MB working tree, 8 MB per file on disk** |

The totals are within the caps, but the packed one only just: fn.modstore must come down to its
20 MB (see the fallback below), which leaves ≈66 MB. The largest single file measured is a 4 MB
plain shard (by construction) or 1.16 MB of plain E2E JSONL; every `.zst` shard measured is
below 1 MB.

The 80 MB cap is the packed one, because that is what every clone and the history pay. The working
tree cap guards the disk of each worktree (this machine runs about 11 of them). The units measure
both with `scripts/golden/size.sh` (`du -sb tests/golden` and the pack size of a scratch
repository after `git gc --aggressive`), which writes them to MANIFEST (`sizes`; `golden manifest
sizes` fails when a cap is exceeded). GATE re-checks them.

Justifications:

- **MS-B (6.9M matrix queries + 5.7M probes) and MS-A (≈3.9M)**: lossless elision, no sampling.
  - A query over a mod name with no mod anywhere in the store's parent chain has a fixed answer:
    Sum = 0, More = 1, Flag = false, and nil or empty for Override, List, Tabulate, Max and Min.
    That answer is computable from the recorded store inputs.
  - The recorder checks that PoB gave exactly that answer for every query it elides. Any query
    where PoB did not stays explicit.
  - The runner regenerates the elided queries from the stores and checks them all. Nothing is lost
    against PoB.
  - On the sampled build, 64,612 of 112,724 matrix results were zero, nil, false or empty, before
    counting More = 1 [M].
  - MS-A's non-tag matrix over the main env's stores is a subset of MS-B's matrix (MS-B is "MS-A's
    matrix with every tag path gated", MS-B.rs:3-5). It is recorded once. MS-A's own parts
    (ScaleAdd*, createMod, the synthetic MAX/MIN, item and buff stores) stay.
  - If the measured result still exceeds 20 MB, the fallback is to keep the full matrix on a
    tag-type set cover of builds (greedy over MS-B's `tag_types` counts) and the probes on all 57.
    That is a real sample, and it would be justified by tag-type coverage.
- **DF-A (600k cases → 120k)**:
  - The inputs are i.i.d. draws from a fixed-seed generator. A regression that changes the result
    on a fraction p of the input space survives n cases with probability (1-p)^n. With n = 20,000
    and p = 0.05 %, that probability is e^-10 ≈ 4.5·10^-5.
  - Every case of the full 100k streams that raised or fell in an edge bucket (inf, NaN, zero
    division) is kept as well, so the rare branches are not left to chance.
  - Storing all 600k outputs would cost about 15 MB, almost entirely random doubles that compress
    poorly. Section 7 offers that option if the user prefers it.
  - The sample is a set of case indices into the full 100k streams, which the runner always
    generates in full (§1.4 regeneration rule), so sampling never shifts another function's
    inputs.
- **CT support compatibility (1.1M pairs × 3 variants)**: a bitset (1 bit per pair, about 0.4 MB
  raw), keyed by the sorted support list and the sorted active-effect list in the header. Nothing
  is sampled.
- **PT (903,303 records over 41 versions)**: most nodes repeat across versions. Records are stored
  once by content hash, and each version is a list of hashes. This is lossless.
- **Build XML**: GOLD2 stores each XML once, by BLAKE3. A second path with the same content
  becomes a manifest alias, not a second file. Measured on `0dbdab33`: none of the 418
  `crates/pob-oracle/corpus/**/*.xml` is byte-identical to another corpus file or to any
  `.parity/builds` or `.parity/fresh` XML (sha256 over all of them). The U0 slice5 builds
  (`slice5_allflame_exp_007/024/030/031/048`) are not copies of their all57 sources: each has 2-12
  lines changed (`derive_slice5.py` disables every socket group but the main one), so they are
  separate fixtures. Git's pack deltas them against their sources.
- **LUT (2,380,556 results)**: kept in full, columnar. They are small integers and compress well.
  Reducing seeds from 200 to 50 per type would save about 3 MB. Not proposed: the bugs that matter
  here are in seed arithmetic at range edges, and the 200-seed spread covers them.

### 2.4 Keeping a golden diff reviewable

- **One value per line, sorted keys, per-build files.** A calc change that moves Life on 3 builds
  shows as 3 changed lines in 3 files.
- **`golden diff`** (a `pob-golden` binary). It decompresses `.zst` shards, joins old and new by
  key, applies the num_eq tolerance, and prints a summary:
  - per set and per key: count of changed, added and removed values;
  - the 20 largest relative changes;
  - which builds are affected.

  `golden diff <git-rev>` compares the working tree with that revision. It runs in the PR
  checklist (§3.6).
- **Overlays.** Deliberate deviations live in `overlay/*.bless.jsonl`, one line each:
  `[key, pob_value, new_value, reason, commit]`. So "PoB said X, we now say Y, because Z" can be
  read and audited in one place, and PoB's original recording stays untouched.
- **MANIFEST pins.** Line counts and compared counts per file are in MANIFEST.json, so a key that
  disappears on both sides changes a pin and shows in review.

---

## 3. Recorder and runner design

### 3.1 Recorder: `pob_golden_record` (in pob-oracle; runs while PoB exists)

- **Where.** A new bin in `crates/pob-oracle` (feature `luajit`), deleted with the crate. It
  depends on the `pob-golden` lib for the writer and the encodings (§3.2), so the recorder and the
  runner share one schema.
- **What it reuses.** Every piece of PoB-running machinery, with no new oracle code paths:
  - `JitPob::boot_oracle` (JIT off), `OracleLock` and the oracle slots;
  - the `par` pool, which is bounded;
  - the probe cache: `QueryCache`, `oracle_dir`, `cached_query`, **only to resume an interrupted
    first run**. Recording runs 2..K and every `--verify` run must each start a fresh, uncached PoB
    process (`POB_QUERY_CACHE=off`, or a private cache dir per run). The cache is keyed by the query,
    so a cached answer would repeat run 1. That would quietly empty the nondeterminism capture
    (`$anyOf`, `$unordered`, the fuzz.defence `pairs` orders) and the determinism check;
  - the patcher, only for the coverage witnesses (SK hooks, TR arms, MI families, CS2 restart);
  - the existing Lua scripts, unchanged or extended: `corpus/CA/ca.lua`, `corpus/INT-3/int3.lua`,
    `corpus/BM-A/bma.lua`, `corpus/PWR/pwr.lua`, `corpus/MS-A/l3_queries.lua`,
    `corpus/MS-B/l3_full.lua`, `corpus/MP7/jewel_funcs.lua`, and the Lua halves of PT, PS-*, LUT,
    IT-*, CO-*, BM-B, CT, AS, DF-A and OF-A.

  Each GOLD unit moves its test's PoB half into a `record_<set>()` function: the same Lua, the same
  inputs, writing goldens instead of comparing.
- **Command line.**

  ```
  pob_golden_record <set|all> [--corpus all57|fresh|special|derived] [--builds a,b]
                    [--runs K] [--out tests/golden] [--verify]
  ```

  `--verify` re-records into a temporary directory and byte-compares with the committed files. It
  is the gate's determinism check (§4).
- **Multi-run for nondeterminism** (calc doc §9.5):
  - E2E sets run each build in **K = 3 fresh PoB processes**. While a run adds a new value, it goes
    on up to 6 runs, as `pob_oracle nondet --max-runs` does. fresh_076 needed 6 runs for
    `ManaCostRaw` (`.parity/nondet/fresh_076.json`: 45.76 / 45.7580952…).
  - The existing `.parity/nondet/*.json` records are merged in too.
  - A leaf that differs between runs becomes `{"$anyOf":[…]}`. A list whose elements permute
    becomes `{"$unordered":[…]}`.
  - Function-level sets run twice (K = 2), to catch value nondeterminism such as AltX/AltY gem
    variants (CT, AS). Their known order-free fields are declared `$unordered` up front, as the
    tests compare them today (AS stat-map runs, PT `pairs()` orders, PS-A `leap`, CO-B `bossSkills`).
    Two more sets depend on PoB's per-process `pairs` order in ways that K runs cannot be trusted
    to see, so they are encoded up front as well: BM-B's flipped gems (`$anyOf` over whole gem
    records, one PoB load per variant) and reordered party labels (`$unordered` lines) in fn.tabs
    (§1.3), and the shift-table and damage-table orders of fuzz.defence, which are recorded as
    inputs (`$perOrder`, §1.4).
- **Host artefacts.**
  - The oracle host is aarch64, where LuaJIT runs in dual-number mode. The product follows x64 PoB
    semantics (port decision).
  - The header records `luajit: {arch, numberMode, jit}`.
  - A `±0` difference needs no marker: num_eq treats the two as equal.
  - The `%d` of NaN/±inf strings that INT-3 accepts today (CCR-0001) are written as
    `{"$host":{"arm64":<PoB text>,"x64":<text the port's x64 profile prints>}}`. The runner
    accepts the x64 text only. It reports the case if Rust prints the arm64 text.
- **Errors.** A PoB `pcall` failure is `{"$error":"<message>"}`. The runner requires that Rust
  raise or return an error too. The message is compared for information only, as today's
  both-raise buckets do.
- **Known representation limits** (MS-A/MS-B CCR-0014/0015, MS-B undecodable probes): the value is
  recorded with `{"$known":"CCR-0014","pob":…}`. The count is pinned to today's bound in
  MANIFEST. The runner counts these values and does not compare them. If a count grows, the run
  fails.
- **Derived fixtures and synthetic builds.** The recorder writes the PS-B and PS-C synthetic builds
  and the derived variants (main socket group, Full DPS, config toggles) as XML through PoB's own
  `SaveDB`. Their golden is then recorded from **loading that XML**, so a fixture is an ordinary
  file.
- **Coverage witnesses.** A record-time pass with the existing patcher probes (`triggers`,
  `runSkillFunc`, `mirages`, `initEnv:1433`) writes `coverage.json` and fails if:
  - a TR arm has no fixture;
  - one of the 3 TR handlers has no fixture;
  - an SK-A or SK-B hook has no fixture where it reaches a compared key (main skill and part, or
    the mutation witness, §1.5);
  - a mirage family has no fixture;
  - the CS2 restart expectation does not hold.
- **Cost bounds.** One PoB process at a time per recorder (the oracle lock), at most about 1 GB
  RSS. `--builds` and `--corpus` allow resumable, chunked runs. Output is written atomically
  (temp file + rename).

### 3.2 Golden file grammar (`golden/1`)

Line 1 is a header object:

```json
{"golden":1,"set":"e2e.ca","key":"all57/allflame_exp_000",
 "input":{"xml":"builds/all57/allflame_exp_000.xml","blake3":"…","rounds":2},
 "origin":{"pob":"v2.67.2@16de4b82d","luajit":{"arch":"arm64","numberMode":"dual","jit":"off"},"runs":3},
 "recorder":"rust-port@<sha>","tolerance":"num_eq","lines":1927}
```

The lines that follow:
- an E2E line is `[key, value]`;
- a query line is `[key, input, value]`.

The file is sorted by key, bytewise.

Value markers:
- `{"$num":"nan"|"inf"|"-inf"}`;
- `{"$anyOf":[v…]}`: the Rust value must equal one of them;
- `{"$unordered":[v…]}`: compared as a multiset;
- `{"$error":msg}`: Rust must fail;
- `{"$host":{"arm64":a,"x64":b}}`: Rust must equal `b`;
- `{"$known":id,"pob":v}`: counted, not compared, with a bounded count;
- `{"$perOrder":[v…]}`: only in a query line whose input carries `"orders":[o…]` (fuzz.defence,
  §1.4). The adapter is called once per order, and its result for `o[i]` must equal `v[i]`;
- `{"$absent":true}`: PoB produced nothing. The key is listed because a Rust-only key is a
  failure.

### 3.3 Runner: crate `pob-golden` (dev-only; no PoB, no mlua, no pob-oracle)

- **Dependencies.** Any product crate the adapters need: pob-calc (and its split crates through
  it), pob-gamedata, pob-modparser, and pob-lua, whose `Table` and `Name` the fuzz and tool
  adapters build (the ordered damage tables of §1.4, for example). Plus serde_json, zstd and
  blake3. The real constraint is what it must **not** pull in:
  - no `mlua`, no `pob-oracle`, no `pob-ui-oracle`, no `pob-dump`, no `pob-testpaths`;
  - no `probe` feature of any product crate (the replay seams that CUT2 removes).

  A test in the crate checks this: `cargo tree -p pob-golden -e normal,dev` has no line matching
  `mlua|pob-oracle|pob-ui-oracle|pob-dump|pob-testpaths`, and
  `cargo tree -p pob-golden -e features` has no `probe` feature.
- **Test binaries: three**, so link time and disk stay low. Today a single CA test binary is
  238 MB in debug [M].
  - `golden_e2e`: all `e2e.*` sets;
  - `golden_fn`: all `fn.*` sets;
  - `golden_fuzz`: the `fuzz.*` sets.

  Each walks MANIFEST.json, so a new golden file needs no new test code.
- **No skip paths.** Goldens and fixtures are committed: a missing file fails. This removes the
  inventory's silent-skip class (`skip()` in BM-A, CA, CO-C, DF-C..G and INT-3).
  The only runtime inputs are VENDOR's asset bundle (`data.bin`, the timeless LUT data, the
  trees), found the way the product finds them.
- **Parallelism and memory.** The bounded corpus pool (moved from `pob_oracle::par`), with 3
  workers by default (`POB_GOLDEN_JOBS`). Per-build state is dropped between builds; the global
  cache is cleared per build, as CA does. Target < 3 GB RSS per binary.
- **Determinism.** Results are sorted by key before reporting. The failure output is the same at
  any job count.

### 3.4 The adapter: the only code the refactor re-points

`pob-golden/src/adapter/{e2e,parser,tree,spec,lut,items,modstore,skills,config,tools,fuzz}.rs`
are the only files that name `pob_calc` internals. Each function is behaviour-level and documents
the PoB behaviour it stands for:

```rust
// e2e
fn load_build(xml: &str, rounds: usize) -> Result<Build, Error>;     // Build:Init + the OnFrame rebuilds PoB made
fn main_output(b: &Build) -> Flat;  fn calcs_output(b: &Build) -> Flat;
fn calcs_lists(b: &Build) -> Flat;  fn used_keys(b: &Build) -> BTreeMap<String, BTreeSet<String>>;
fn misc_calc(b: &Build, o: &OverrideSpec, full_dps: Option<bool>) -> Result<Flat, Error>;  // OverrideSpec: item ids, slot names, node ids
fn breakdowns(b: &Build, actor: Actor) -> Vec<(String, Json)>;  fn tabulate(b: &Build, actor: Actor, q: &SectionQuery) -> Result<Rows, Error>;
fn save_xml(b: &Build) -> String;   fn model(b: &Build) -> Flat;   fn power_report(b: &Build, mode: &PowerMode) -> Flat;
fn mirage_output(b: &Build) -> Option<Flat>;
// parser / tree / spec / items / modstore / skills / config / tools / fuzz
fn parse_line(line: &str) -> (Vec<ModCanon>, Option<String>);
fn jewel_func(func_line: &str, tree: &str, node: u32) -> Result<JewelOut, Error>;
fn tree_records(ver: &str) -> Vec<(RecordKey, Json)>;   fn spec_records(b: &Build, spec: usize) -> Vec<(RecordKey, Json)>;
fn lut_read(kind: LutKind, seed: f64, at: u32) -> Json;   fn item_records(raw: &str, ctx: &ItemCtx) -> Vec<(String, Json)>;
fn modstore_rebuild(inputs: &Json) -> Stores;   fn modstore_query(s: &Stores, q: &StoreQuery) -> Result<Json, Error>;
fn active_skill(b: &Build, group: usize, weapons: u8) -> Json;   fn config_apply(var: &str, value: &Json) -> Result<Json, Error>;
fn tool(name: &str, input: &Json) -> Result<Json, Error>;   fn fuzz_helper(name: &str, input: &Json) -> Result<Json, Error>;
```

- **Encodings.** Each adapter writes the encodings of §1.1. The recorder writes the same
  encodings from PoB's side. Shared encoders live in `pob-golden/src/encode.rs`.
- **Status today.** The adapter bodies call today's Lua-named APIs (`loadBuildModelFromXML`,
  `calcsTab_BuildOutput`, `new_PassiveSpec`, `parseModNative`, …). That is fine: those calls are in
  one place.
- **During the refactor.** When a function is renamed or reshaped, only its adapter body changes.
  The golden files, the compare engine and the test binaries stay as they are.
- **Rule for adapter changes.** A refactor PR that changes adapter code must show
  `golden diff` = 0, or overlays with reasons. If an adapter encoding changes (for example when a
  concept disappears), that is a golden-format change: it needs the same review as a bless and
  bumps the set's version.
- **The `rounds` input** reproduces PoB's load (the number of BuildOutput calls). A refactored
  calc that is a pure function of the build may ignore it, and the golden still holds.

### 3.5 Comparison and tolerance

- **Numbers:** `num_eq`, `|a-b| ≤ 1e-9·max(1,|a|)`. NaN equals NaN, and ±0 are equal. This is the
  §9.2 rule CA uses today (CA.rs:171).
  - DF-A and CT compare `%.17g` text exactly today. The runner applies num_eq to them as well,
    because the refactor may reorder float operations. This is a deliberate weakening, so the
    count of bit-exact matches per set is **pinned in MANIFEST** (`bitExact`). A count below the
    pin fails like a changed value. A bless lowers the pin, with its reason, so every loss of
    bit-exactness is reviewed (§3.6 checklist item 4).
- **Strings and booleans:** exact.
- **Rust-only keys:** a failure. A missing key: a failure. Both as CA does today.
- **Mod lists:** a multiset per name, with an order check. An order difference is reported, not
  failed, as today's LUA-ORDER handling does.
- **Report.** Per set: compared, exact, accepted through `$anyOf`, `$host` and `$known`, and
  failed. Then the first 20 failures per file. A diff TSV goes to `POB_GOLDEN_DIFFS=file`, as
  `CA_L6_DIFFS` does today.

### 3.6 Re-record (bless) workflow and review checklist

When there is no PoB, "re-record" means: **accept Rust's new value deliberately**.

```
POB_GOLDEN_BLESS=1 POB_GOLDEN_REASON="<why>" [POB_GOLDEN_FILTER=<set>[:<key glob>]] \
  scripts/port/cargo.sh test -p pob-golden --test golden_e2e
```

- The runner writes one overlay line for each **failing** value: PoB's value, the new value, the
  reason, and `HEAD`. It never touches the PoB-recorded file.
- It refuses to run without a reason, or when the filter matches more than
  `POB_GOLDEN_BLESS_MAX` (default 200) values. A bigger bless must be split by key, or raised on
  purpose.
- `POB_GOLDEN_BLESS=rebase` (exceptional, for example a new game-data snapshot, which is a later
  problem) rewrites the base files. MANIFEST then records `origin: rust@<sha>` for the set, and
  the PR must say so in its title.

**Review checklist** (PR template section, required when `tests/golden/` changes):

1. `golden diff origin/rust-port` summary pasted in the PR. Every changed key is one the change
   meant to move. No unrelated set changed.
2. Each overlay reason names the behaviour, and the issue or PoB commit it follows.
3. No `$anyOf`, `$unordered`, `$host` or `$known` marker was removed or widened, unless the PR
   explains why.
4. The MANIFEST pins changed only by the overlaid or added keys. A lowered `bitExact` pin
   carries its reason.
5. Fixture XML unchanged, or the set re-recorded with it (the BLAKE3 matches).
6. The full golden suite is green after the bless, and its negative control still fails (§4).

---

## 4. Validation gate (before anything is deleted)

All on one commit **G** of rust-port: every GOLD unit merged, VENDOR merged, PoB still present.

### 4.1 Live PoB tests pass, with their pins

```sh
# at the root of a full-corpus worktree, with POB_ORACLE_REQUIRE=1 (no silent skips)
export POB_ORACLE_REQUIRE=1
root="$PWD"
mkdir -p "$root/gate"
for f in "$root"/crates/pob-oracle/tests/*.rs; do
  t=$(basename "$f" .rs)
  scripts/port/cargo.sh test -p pob-oracle --test "$t" -- --nocapture > "$root/gate/live-$t.log" 2>&1 || echo "FAIL $t"
done
# cargo runs integration tests from the package root (crates/pob-oracle), and CA.rs reads
# CA_CORPUS_DIR relative to it (CA.rs:145, CA.rs:1874): pass an absolute path
CA_CORPUS_DIR="$root/.parity/fresh" scripts/port/cargo.sh test -p pob-oracle --test CA -- ca_l6_corpus --nocapture \
  > "$root/gate/live-CA-fresh.log" 2>&1 || echo "FAIL CA fresh"
```

**Every** pob-oracle test binary runs, not a selection. Every segment test's builds become
fixtures (§1.5): TR-A, PF-A, DF-B..DF-G, OF-B..OF-H, CS0, CS1 and CS3 as much as SK-A, TR-B or MI.
So the gate must start from a suite that is green on all of them. A segment test that fails at G
would mean its fixtures' goldens record behaviour that the Rust side does not match, and nobody
has looked at it. The tooling self-tests (P3, ORC-X, oracle_harness, …) run too. They are cheap,
and a broken oracle would make every recording suspect.

Run them one binary at a time (the loop does), so no two heavy binaries run at once (CA is
2.8 GB, OF-F 3.1 GB). Expected: every test passes, at the pinned counts the tests assert today:

| Live test | Expected (asserted in the test) |
|---|---|
| CA L6 all57 | 57 builds, headline 741/741, keys 45,954/45,954, CALCS lists 171/171, used 513/513, misc 1,248/1,248, 0 Rust errors (CA.rs `L6_ALL57`) |
| CA L6 fresh | 100 builds, headline 1,300, keys 85,603, lists 300, used 900, misc 2,200 (`L6_FRESH`) |
| CA D13, level cost | pass |
| INT-3 | breakdown entries 10,606; Tabulate queries 38,940, rows 42,588 (`BD_FULL_ENTRIES`, `TAB_FULL_*`) |
| BM-A | 57 builds, 0 differences |
| PWR L6 | 7,672 values exact (`L6_OCCVORTEX_KEYS`); replay on 3 builds exact |
| MP6 whole parser | 80,993 records, allow-list empty (`PARSE_ORACLE_RECORDS`) |
| MP7 | 234 funcs, ≥ 883,350 calls, ≤ 1 both-raise |
| LUT | 2,380,556 results, 0 mismatches |
| PT | 41 versions, ≥ 118,817 nodes / 903,303 records, 0 parser-dependent |
| PS-A | (57, 59) coverage, ≥ 48,932 records |
| IT-A/B/C | 1,527 corpus items; IT-A ≥ 10,032 parses; IT-B ≥ 239,070 passes; IT-C ≥ (10,042, 71,549) calls |
| AS | (3,790, 1,225) cases |
| CO-A / CO-B | (266, 1,741) / (273, 2,407); both-error ≤ 18 / ≤ 108 |
| BM-B | tabs 148,776; buffs 3,310 (5 pending: in fn.tabs the `$known` CCR-0014 values, §1.3); synthetic 1,302 |
| DF-A | 6 × 100,000 cases at the `DFA_BOUNDS` |
| TR-B | all 82 arms and 3 handlers covered |
| MS-A, MS-B, CO-C, CT, OF-A, SK-A, SK-B, MI, PF-D, PF-F, CS2, OF-I | pass at their current bounds |
| every other pob-oracle binary (TR-A, PF-A..PF-G, DF-B..DF-G, OF-B..OF-H, CS0, CS1, CS3, MP0, MP1, U0, the tooling self-tests) | pass at their current bounds |

### 4.2 The recorder reproduces the committed goldens

```sh
POB_QUERY_CACHE=off scripts/port/cargo.sh run -p pob-oracle --bin pob_golden_record -- all --verify   # takes the machine-wide OracleLock itself; uncached: every run is a fresh PoB process
```

Expected: every file is byte-identical. The one exception: `$anyOf` and `$unordered` sets, and
the `orders` lists of fuzz.defence, may **grow**, when a run finds a new PoB alternative or a new
`pairs` order. Any growth is reported, folded in, and the verify is repeated until it is clean.
For an order that is already stored, the output must be byte-identical.

### 4.3 The golden runners and every product test pass without PoB

```sh
git worktree add --detach /tmp/pob-gate G     # a fresh worktree has no .PathOfBuilding, data/ or .parity (gitignored)
cd /tmp/pob-gate
test ! -e .PathOfBuilding && test ! -e .parity && test ! -e data/dump
mkdir -p gate
# nothing points back at the main checkout, and nothing is allowed to skip
clean() { env -u POB_ROOT -u POB_SRC -u POB_DATA_DIR -u POB_DUMP_DIR -u POB_PARITY_DIR \
              -u POB_ALLOW_SKIP -u POB_T5_ALLOW_SKIP "$@"; }
clean scripts/port/cargo.sh test -p pob-golden > gate/golden.log 2>&1 || echo "FAIL pob-golden"
# every product crate of the CI unit job, one test target at a time (scripts/golden/gate_unit.sh)
clean scripts/golden/gate_unit.sh gate/
scripts/port/cargo.sh tree -p pob-golden -e normal,dev | grep -cE 'mlua|pob-oracle|pob-ui-oracle|pob-dump|pob-testpaths'   # 0
scripts/port/cargo.sh tree -p pob-golden -e features | grep -c '"probe"'                                                 # 0
grep -lE '\b(SKIP|SKIPPED|PENDING)\b' gate/*.log                                                   # no file
```

`scripts/golden/gate_unit.sh` (GATE unit) does this:
- **Crates.** It reads `scripts/golden/unit_crates.txt`, the same file CI's unit job reads (§5
  step 8), so the gate and CI cannot drift: pob-lua, pob-gamedata, pob-modparser, pob-calc,
  pob-calc-core, pob-calc-setup, pob-calc-config, pob-calc-defence, pob-calc-offence,
  pob-calc-perform, pob-host, pob-sg, pob-sg-wgpu, pob-ui-core, pob-app.
- **One target at a time.** Per crate: `cargo.sh test -p <crate> --lib`, then `--bins` and
  `--doc` where the crate has them, then `--test <t>` for each `tests/*.rs` target, each with its
  own log `gate/unit-<crate>-<target>.log`. Never two test binaries at once.
- **Left out by name:** only the targets that §5 step 5 deletes before CI exists, listed in
  `scripts/golden/cut1_deleted_tests.txt`: pob-lua `l0_*` and `oracle_dualnum_feature`, pob-calc
  `hyg1_len`, pob-ui-core `tier0_*` and `compile_shape`. Everything else runs, including the
  moved tests of §1.7 (`model_xml`, `tabs_fresh_build`, `power_stats`, `combine_stat`,
  `item_units`, `global_cache`, `copy_actor_keepalive`, `dump_load`, `mod_cache`, `tree_perf`)
  and every row of §1.8 that stays (`fup5_teardown`, `u54_worker_calc`, the pob-sg font and
  image tests, pob-host's `pob_assets_load_async`, `leak1_cycles`).
- **T5.** pob-sg-wgpu runs with `POB_T5_ALLOW_SKIP` **unset**: the gate machine has a Vulkan
  adapter (Asahi), so a skip of any kind fails. Its font part, including the new GPU-free
  `t5_fonts_load` (§1.8), loads the bundle's fonts.
- **INT-2.** `cargo.sh test -p pob-calc --bench INT-2 -- --recalc 1 --warmup 0 --power 0
  "$PWD/tests/golden/builds/all57/allflame_exp_000.xml"`: the bench runs once in the test
  profile, on the bundle's data, and must exit 0.
- **No silent skip.** After the runs, no log may contain `SKIP`, `SKIPPED` (pob-testpaths'
  `POB_ALLOW_SKIP` message) or `PENDING` (the last line of the block above). The tests of §1.8
  that printed them no longer return early, so a match means an input was not found.

Expected: 0 failures, and each set compares at least the live counts:
- **e2e.ca all57**: headline 741, keys 45,954, lists 171, used 513, misc 1,248.
- **e2e.ca fresh**: headline 1,300, keys 85,603, lists 300, used 900, misc 2,200.
- **e2e.int3**: 10,606 entries; 38,940 queries / 42,588 rows.
- **e2e.pwr OccVortex**: 7,672 values.
- **fn.parser.lines**: 80,993 lines.
- **fn.parser.jewel**: 883,350 calls (elided calls included).
- **fn.lut**: 2,380,556 results.
- **fn.tree**: 903,303 records.
- **fn.spec**: ≥ 48,932 records.
- **fn.skills**: 5,015 cases.
- **fn.config**: 1,741 + 2,407 cases.
- **fn.tabs**: 148,776 + 3,310 + 1,302.
- **fuzz.defence**: 6 × 20,000 + edge cases.
- **The rest**: the line counts in MANIFEST.
- **The tests moved out of the oracle crates (§1.7)**: pass, with their own pins (gems 871,
  ModCache 23,207, …).
- **Every other product test target** of the unit crates (§1.8 included): passes, with no `SKIP`
  or `PENDING` in its log.

The runner prints `golden <set>: compared N (pin N)` lines. A gate script, `scripts/golden/gate.sh`
(GATE unit), checks every pin.

### 4.4 Negative control

```sh
POB_GOLDEN_SELFTEST=1 scripts/port/cargo.sh test -p pob-golden
```

The runner changes one expected value per set and must report exactly one failure per set. This
proves that no set passes vacuously. The value is picked by a fixed rule, so the check cannot pass
by accident:
- **Which value.** In key order, the first number that is **compared**, **finite** and
  **non-zero**. A zero is skipped, because 0 × (1 + 1e-6) is still 0, and zeros are common (MS
  answers, many e2e outputs). Values inside `$known`, `$anyOf`, `$unordered`, `$host` or
  `$perOrder`, `$absent` lines and elided (regenerated) answers are skipped too: they are not
  compared one to one, or not stored at all.
- **The change.** That number × (1 + 1e-6), which is well outside `num_eq` (1e-9 relative).
- **A set with no such number** (a string-only or bitset set): the first compared string gets
  `"\u0001"` appended, or the first compared boolean is negated, or the first bit of the bitset
  is flipped.
- **The report** names the changed key, and the self-test checks that the one failure is on that
  key.

### 4.5 Where a golden is stronger than its live test

Some goldens check more than the live test did:
- fn.spec runs the spec whole, unstubbed;
- fn.items has no replay of parseMod or BuildModList;
- e2e.pwr covers more builds and modes;
- e2e.mcq is PoB-backed;
- the special corpora were never L6-checked;
- MI is no longer a tautology.

So "pass exactly where the live test passes" means two things here. **The live test passes, and
the golden passes too.** A golden failure there is a real Rust difference that the live test hid.
It blocks deletion until one of these happens:
- it is fixed in a FIX unit, while PoB still exists and can be rerun;
- the user accepts it, and it becomes an overlay with a reason and a FOLLOWUPS entry.

The GOLD unit that owns the set runs this triage. The gate allows no unexplained failure.

---

## 5. Cut-over checklist (in order)

1. **VENDOR merged.**
   - The app runs without a PoB checkout, from the in-repo asset bundle (`assets/`, with
     `data.bin`, the timeless LUT data, the trees, fonts and art, plus `THIRD_PARTY_NOTICES`).
   - VENDOR's own test forbids `POB_ROOT`, `.PathOfBuilding` and `pob_src` in product code.
   - The golden runners use the same bundle.
2. **All GOLD units merged.** The gate (§4) passes on commit G. The report goes in
   `docs/standalone/GOLDEN-GATE.md`, with the logs.
3. **Tag:**

   ```sh
   git tag -a pob-oracle-final G -m "last commit with the PoB oracle (PoB v2.67.2@16de4b82d)"
   git push origin pob-oracle-final
   ```

   Recovering the oracle later means checking out this tag. It also needs the local
   `.PathOfBuilding`, `data/dump` and `.parity`, so tell the user before step 7 deletes them on
   disk.
4. **Move the PoB-free tests out of the oracle crates (§1.7).** Their owning units moved them
   before G, so this step is normally a check. If one is still there, it is moved now.
   - For each moved row of §1.7, its test function must no longer exist under `crates/pob-oracle`
     or `crates/pob-ui-oracle`, and it must exist at its destination. A small script,
     `scripts/golden/check_moved.sh`, greps for each function name on both sides, using the list
     in `tests/golden/README.md` that GOLD1 writes from §1.7.
   - A test still in an oracle crate is moved now as §1.7 says (`git mv` of its body into the
     destination file, then the input changes listed there). It must then pass in the PoB-free
     worktree of §4.3 before step 5 runs.
   - Each dropped row has its reason in `docs/standalone/GOLDEN-GATE.md`.
   - The same check for **§1.8**: each row that stays reads the bundle and
     `tests/golden/builds/`, and each dropped row is gone. `check_moved.sh` also greps the
     product crates (everything but the three deleted crates and pob-testpaths) for
     `\.PathOfBuilding|POB_ROOT|POB_SRC|pob_testpaths|data/dump|\.parity|pob-ui-oracle/|pob-oracle/|pob-dump/`
     in `src/`, `tests/` and `benches/`. The only hits allowed are the targets step 5 deletes
     (`scripts/golden/cut1_deleted_tests.txt`), pob-gamedata's `#[path]` includes and
     app/pob-desktop's `lua-calc` feature, which step 5 handles. A hit elsewhere is fixed as
     §1.8 says before step 5 runs.
5. **Delete crates** (one commit; workspace members and `Cargo.lock` updated):
   - `crates/pob-oracle`: with `corpus/` and `lua/`, after GOLD2 copied the corpora into
     `tests/golden/builds/special/`;
   - `crates/pob-ui-oracle`: with `scenarios/`;
   - `crates/pob-dump`: **first** move `generated/global.rs` and `generated/misc.rs` into
     `crates/pob-gamedata/src/generated/`, and `FORMAT.md` into `docs/`. pob-gamedata includes
     these two files by path today (`src/generated/mod.rs:6,10`), and they become frozen sources.
     `generated/enums.rs` does not need to move: pob-gamedata does not include it (its
     `SkillType`, `ModFlag` and `KeywordFlag` come from `global.rs`), and its only user is
     pob-dump's own `tests/dump.rs`. It goes with the crate;
   - `crates/pob-testpaths`. Its dev-dependents besides the deleted crates are pob-app,
     pob-calc, pob-gamedata, pob-lua and pob-sg (each `crates/*/Cargo.toml`; pob-host does not
     use it). After §1.8, the first four no longer call it (GOLD13 removed their
     dev-dependency) and pob-lua's use goes with its L0 tests below, so the crate and its
     workspace entry are deleted. If step 4's grep found a caller, this waits until it is fixed;
   - `app/pob-desktop`: drop the `lua-calc` feature and the `pob-ui-oracle` dependency
     (`Cargo.toml:12-22`);
   - `mlua` dev-dependencies and their tests:
     - pob-lua: `tests/l0_*.rs`, `tests/common/`, `oracle_dualnum_feature`, the `oracle-dualnum`
       feature with its `cfg` code in `src/profile.rs`, and the `pob-testpaths` dev-dependency;
     - pob-calc: `hyg1_len` (its two LuaJIT tests; `ctor_pairs_follow_field_order` has already moved
       to `tests/ctor_pairs.rs` under GOLD1);
     - pob-ui-core: `luastr` (its LuaJIT parts), `tier0_*`, `compile_shape`;
     - the workspace `mlua` entry;
   - nothing else: every other product test that read the PoB checkout, `data/dump`, `.parity` or
     the deleted crates' files was re-pointed or dropped before G (§1.8, owners GOLD1, GOLD2,
     GOLD13), and step 4 checked it.
6. **Delete scripts:**
   - `scripts/port/`:
     - `oracle.sh`, `provenance.py`, `luaast.py`, `globals_lint.py`, `locals_audit.py`,
       `segments.py`, `calcscope.py`, `pairs_inventory.py`;
     - `fresh_corpus_run.sh`, `fresh_corpus_summary.py`, `fresh_corpus_datacheck.lua`,
       `fetch_fresh_corpus.py`;
     - `quick_parity.sh`, `pobpaths.py`, `next_ccr.sh`, `lua2rs/`;
     - `calc_depgraph*.py`, `calc_ptime.py`, `calc_rustc_wrap.sh`, `calc_sumall.py` (CSPLIT
       measurement tools; keep them if the refactor wants them);
     - `perf_bench.sh` (INT-2 vs LuaJIT), unless PROF1 keeps a Rust-only variant;
   - from `scripts/port/tests/`: `test_luaast`, `test_provenance`, `test_lint_inventory`,
     `test_locals_segments`;
   - `scripts/parity/` (all of it: `diff.py`, `fetch_ninja.py`, `oracle_dump.sh`, `pb.py`,
     `report.py`);
   - **Keep:** `cargo.sh`, `testrun.py`, `full_suite.sh` (rewritten crate list), `oomguard.sh`,
     `bump_version.sh`, `push_remote.sh`, `with_merge_lock.sh`, and their tests.
7. **Delete docs and data:**
   - `docs/port/`, everything that describes porting against PoB:
     - `stages.toml`, `scope.toml`, `known_diffs.toml`, `pairs_inventory.md`;
     - `ccr/`, `research/`, `parity/` (move `fresh-corpus.md`'s provenance table into
       `tests/golden/README.md` first), `calc-split-plan.md`.
   - Keep `AMENDMENTS.md`, `calc-architecture.md`, `ui-architecture.md`, `STYLE.md`,
     `contracts.md`, `testing.md` and `FOLLOWUPS.md` as history under `docs/history/`, or delete
     them (§7).
   - Rewrite `testing.md` for the golden suite.
   - **Local, gitignored** (only with the user's OK, after the tag):
     - `.parity/`: probe-cache 27 GB, patched-src 1.0 GB, probe 434 MB, oracle 82 MB, unit 67 MB,
       fresh 28 MB, nondet 2.8 MB, builds 2.9 MB, fullsuite, results*, headless, ccr-reserved;
       about 29 GB in total [M];
     - `.parity-quick/`;
     - `.PathOfBuilding/`;
     - `data/pob` (418 MB) and `data/dump`, except what VENDOR moved into the bundle
       (`parse_oracle.jsonl` 54 MB goes, because fn.parser.lines holds it);
     - `data/lua`, and the legacy `data/*.json`, if VENDOR does not claim them.
8. **CI rewrite** (`.github/workflows/ci.yml`). Triggers: pull requests, and pushes to `main` and
   `rust-port`. Today it has never run on rust-port (inventory, CI). No PoB checkout, no `.parity`.
   Jobs:
   - **calc-golden**: `cargo test -p pob-golden` (3 binaries), with a cargo cache.
   - **unit**: `cargo test` for each crate in `scripts/golden/unit_crates.txt` (the list §4.3
     ran: pob-lua, pob-gamedata, pob-modparser, the pob-calc crates, pob-host, pob-sg,
     pob-sg-wgpu, pob-ui-core and pob-app). The CI runner has no GPU, so it sets
     `POB_T5_ALLOW_SKIP=1`, which after §1.8 skips only the GPU read-back. The fonts load from
     the bundle or fail, and `t5_fonts_load` checks the text frame on the CPU raster.
   - **wasm**: `cargo check --target wasm32-unknown-unknown` (the runtime crates).
   - **lint**: `fmt --check`, `clippy -D warnings`.
   - **perf** (nightly, release): u53_perf, `dump_load`, PROF1's compare. The perf gates run in
     release so they actually enforce something.
   - Remove `POB_ALLOW_SKIP` from CI. It has no effect any more: pob-testpaths is gone.
   - A grep job fails on `\.PathOfBuilding|POB_ROOT|mlua` anywhere outside `docs/history`.
9. **After the cut-over** (expected; measured in CUT1, which updates this table):

   | | Before (inventory, 159 binaries) | After (expected) |
   |---|---|---|
   | Suite test-binary time | 153 min (≈ 2 h wall) | **≈ 12-20 min**, of which the goldens take 8-12 min with 3 jobs. Rust-side times from today's tests: CA ≈ 1.4 s/build, so ≈ 580 E2E builds ≈ 13.5 min single-threaded. fn.* ≈ 5 min. Plus the unit, render and host tests (< 2 min) [E] |
   | Heaviest RSS | 3.6 GB (u21) | ≤ 3 GB (golden_e2e; CA today is 2.8 GB, most of it per-build state) [E] |
   | Test binaries | 159 | ≈ 45 (3 golden + the remaining unit/render/host/lib) [E] |
   | Debug test binary size | CA alone 238 MB [M] | ≈ 150-200 MB each for the 3 golden binaries, with no LuaJIT and no `probe` feature [E] |
   | Release desktop binary | unchanged by the oracle removal (the oracle never shipped) | same, minus nothing; `lua-calc` was opt-in |
   | Repo | + 12 MB crates/pob-oracle/corpus, 21 MB oracle crates source | − ≈ 37 MB source/corpus, + `tests/golden/` with XML: ≈ 61 MB packed, ≈ 180 MB checked out [E] |

   Fast tier per PR: e2e all57 + special corpora + fn.* + unit. Nightly: e2e fresh, fuzz, perf.
10. **CUT2 (optional; can fold into the refactor):** remove the `probe` feature (`pob-calc`
   `Cargo.toml:35-42`; 17 product source files gate code on it) and the per-unit replay seams
   (`calc_replay`, `callee_replay`, `replay_hook`, `item_parse::replay`, `item_tools::replay`,
   `stub::set_replay_hook`) from the product crates. Nothing uses them once the oracle is gone.

---

## 6. Unit breakdown

| Unit | Scope | Depends on | Parallel with | Record cost on this 16 GB aarch64 machine [E] |
|---|---|---|---|---|
| **GOLD1** format + runner core | `pob-golden` crate: `golden/1` reader/writer, the compare engine (num_eq, markers), MANIFEST, bless/overlay, `golden diff`, the self-test, the `par` pool (moved), and the adapter skeleton. The `pob_golden_record` bin skeleton in pob-oracle. **Measure the real sizes** of the [E] rows from the existing caches and update §2.3 | - | VENDOR | none (no PoB) |
| **GOLD2** fixtures | copy all57, fresh and every `corpus/**/*.xml` into `tests/golden/builds/`, stored once per BLAKE3 (§2.3); write out the PS-B, PS-C and CS2 synthetics and the level-cost XMLs; the derived-fixture generator; privacy step (§7); re-point `leak1_cycles.rs` at `builds/all57/` (§1.7) | GOLD1 (layout only) | VENDOR | ≈ 5 min PoB (SaveDB of the synthetics) |
| **GOLD3** e2e.ca + d13 + levelcost + mirage block + coverage witnesses | the recorder for CA (ca.lua + mirage block, K = 3..6 runs), the TR/SK/MI/CS2 coverage pass, the runner adapter `e2e.rs`; triage of any special-corpus failure | GOLD1, GOLD2 | GOLD4-GOLD12 | all57 + fresh + ≈ 520 special/derived builds × 3 runs. The probe measured ≈ 4.5 s/build for PoB (uncached, with CA's probes) + Rust, so ≈ 1-1.5 h; coverage pass (patched PoB, probes) ≈ 1 h; RSS ≤ 1 GB (PoB) + ≤ 3 GB (runner) |
| **GOLD4** e2e.int3 | recorder (int3.lua + the breakdown part of the calcsEnv dump), adapter breakdown/tabulate canonicalisation (moves the CalcBreakdownControl sort logic from the test into the adapter) | GOLD1, GOLD2 | yes | 57 × 3 runs ≈ 30 min (INT-3 takes 88 s warm today); RSS ≈ 1 GB |
| **GOLD5** e2e.bm + fn.tabs | BM-A model and SaveDB golden, BM-B tabs, buffs and gem levels; BM-B's flipped gems, reordered labels and `expected` values encoded as §1.3 says (one extra PoB load per ambiguous variant, about 10 gems) | GOLD1, GOLD2 | yes | ≈ 20 min (BM-A 16 s warm, 683 s cold in an earlier full suite); ≤ 1 GB |
| **GOLD6** e2e.pwr + e2e.mcq | power reports (3 builds × modes + the Full-DPS fixture + OccVortex); the misc-calculator override queries; **show that the E2E power on the replay builds passes** before the replay retires | GOLD1, GOLD2 | yes | PoB power runs ≈ 30-60 min (each report runs the calc per node); MCQ ≈ 57 × 60 calls ≈ 30 min; ≤ 1.5 GB |
| **GOLD7** fn.parser.* | lines (from parse_oracle.jsonl, no PoB needed), fuzz lines (PoB parseMod), jewel funcs (MP7 Lua) | GOLD1 | yes | ≈ 15 min; MP7 1.3 GB today |
| **GOLD8** fn.tree + fn.spec + fn.lut | PT records (with dedup), whole spec unstubbed (triage new differences), LUT columnar | GOLD1, GOLD2 | yes | PT ≈ 10 min (2.0 GB today: build one version at a time), PS ≈ 30 min, LUT ≈ 10 min (1.2 GB) |
| **GOLD9** fn.items | item records without traces or replay; triage new differences | GOLD1, GOLD2 | yes | ≈ 30 min (IT-A/IT-C ≈ 150-170 s each warm; their PoB halves dominate cold) |
| **GOLD10** fn.modstore | MS-B + MS-A unique parts, elision with record-time checks | GOLD1 | yes | ≈ 20-30 min cold (MS-B 1,127 s cold in the latest full suite); ≤ 1 GB |
| **GOLD11** fn.skills + fn.config + fn.tools | AS, CO-A/B merged, CO-C, CT (bitset) | GOLD1, GOLD2 | yes | ≈ 20 min; ≤ 1 GB |
| **GOLD12** fuzz.* | frozen generators copied from DF-A and OF-A, always generating the full 100k streams; outputs; edge cases from the 100k streams; PoB's `pairs` orders of takenHitFromDamage and reducePoolsByDamage stored per case, and the adapter that builds the Rust tables in them (§1.4) | GOLD1 | yes | DF-A ≈ 5 min (the full 100k streams are needed once to extract edge cases), OF-A ≈ 2 min; DF-A 1.8 GB today |
| **GOLD13** PoB-free UI tests and the product tests of §1.8 | the pob-ui-oracle rows of §1.7: `u53_perf` into `crates/pob-app/tests/tree_perf.rs`, and `u26_progress_steps`, `u48_progress_steps`, `u38_undo_round_trip`, `u43_share_rust`, `u44_rust_unit`, `u45_disable_party_effects` moved or dropped with a reason. Every §1.8 row it owns: pob-app `fup5_teardown` and `u54_worker_calc` (bundle, user-dir fixture), the pob-sg font, image and colour tests, pob-host's asset test, T5's font part and `t5_fonts_load`, the INT-2 bench, and the bundle switch of `leak1_cycles` and `dump_load`; the `pob-testpaths` dev-dependency removed from pob-app, pob-calc, pob-gamedata and pob-sg | GOLD2, VENDOR (assets) | GOLD3-GOLD12 | none (no PoB); `u53_perf` ≈ 1 min in release |
| **GATE** | §4 in full on commit G; `scripts/golden/gate.sh`, `gate_unit.sh`, `unit_crates.txt`, `cut1_deleted_tests.txt`; `docs/standalone/GOLDEN-GATE.md` | GOLD1-13, VENDOR | - | live suite ≈ 1.5-3 h (pob-oracle binaries one at a time) + recorder verify ≈ 3-4 h + golden runs ≈ 20 min |
| **CUT1** | §5 steps 3-9 | GATE | - | none |
| **CUT2** (optional) | §5 step 10 | CUT1 | the refactor | none |

- **Scheduling.** GOLD1 → GOLD2 → {GOLD3 … GOLD13}, at most 3 agents at a time. GOLD7, GOLD10
  and GOLD12 do not need GOLD2 and can start right after GOLD1. GOLD13 also waits for VENDOR.
- **The §1.7 moves and the §1.8 re-points.** Each unit also moves the PoB-free tests that §1.7
  assigns to it (GOLD1, GOLD5, GOLD6, GOLD7, GOLD9, GOLD11, GOLD12, GOLD13) and re-points the
  product tests that §1.8 assigns to it (GOLD1, GOLD2, GOLD13), in the same branch as its
  goldens.
- **Machine limits.** One PoB per recorder (the oracle lock). Never two heavy test binaries at
  once. The runner binaries stay ≤ 3 GB. The total recording time of the whole set, cold, is
  about 6-8 h machine time, spread over the parallel units.
- **Unit acceptance (every GOLD unit):**
  1. the live test(s) it replaces pass at the unit's base commit (the rust-port commit its branch
     starts from, merged up). G does not exist yet when a unit finishes, so GATE re-checks this at
     G (§4.1);
  2. its golden runner passes in a PoB-free worktree, with the compared counts ≥ the live pins;
  3. `--verify` reproduces its files;
  4. its negative control fails;
  5. its files are within the size caps, with measured sizes in MANIFEST;
  6. the §1.7 tests it owns are gone from the oracle crates, the §1.8 tests it owns read no PoB,
     `data/dump` or `.parity` path, and both pass at their destination in the PoB-free worktree,
     with no skip variable set.

---

## 7. Risks and open questions

### Decisions (user, 2026-11-13)

1. **Privacy:** strip identifiers. GOLD2 removes the character name, account name and import
   links from every committed build XML before recording. The goldens are recorded from the
   stripped XML.
2. **DF-A sample:** 20k cases per function (≈ 3 MB), plus every edge case taken from the full 100k
   streams.
3. **Lua-semantics vectors:** freeze them. A unit (GOLD14) records LuaJIT's answers for the
   L0/luastr vectors (`string.format`/`%g`/`tostring`, patterns, `table.sort`, pairs-order tables,
   `math.random` streams) as small goldens (< 5 MB) with PoB-free runners, before `mlua` goes. They
   are deleted when the refactor removes the emulation.
4. **docs/port:** CUT1 moves it to `docs/history/port/`. It is not deleted.
5. **Used-tracking values** (orchestrator decision): GOLD3 records used-tracking **values**, not
   only key sets, as long as that stays within the size cap.

### Questions for the user (answered above)

1. **Privacy of the fresh corpus.** The 100 fresh builds are poe.ninja exports of real
   characters. `docs/port/parity/fresh-corpus.md` already lists their character links. The
   all57 builds come from public sources too. Should their XMLs be committed as they are, or
   with names and links stripped? GOLD2 can strip what the calc does not read (character name,
   account name, import links) before recording, so the goldens match the stripped XML.
2. **DF-A sample size.** 20k per function (≈ 3 MB, the plan) or the full 100k (≈ 15 MB)?
3. **Lua-semantics vectors.** Product output still depends on emulated LuaJIT formatting
   (`string.format`, `%g`), pairs order and `math.random` in places. Should the L0 vectors be
   frozen as small goldens (< 5 MB) before `mlua` goes, so the emulation keeps a reference until
   the refactor removes it?
4. **docs/port after the cut-over:** keep it under `docs/history/`, or delete it (git history and
   the tag keep it)?
5. **Used-tracking values.** Extend e2e.ca from key sets to values (a new check, not needed for
   the gate)?

### Risks

- **The special corpora fail end to end** (they were only segment-checked). Mitigation: GOLD3
  triages them while PoB exists. The probe (appendix B) found no difference on 117 of the 418
  builds; the other 301 are unchecked.
- **Stronger goldens expose hidden differences** (fn.spec unstubbed, fn.items unreplayed, e2e.pwr
  more builds, e2e.mcq). Handled by §4.5: each one is fixed or explicitly accepted before
  deletion.
- **PoB nondeterminism missed by K runs.** fresh_076 needed 6 runs. Mitigation: up to 6 runs while
  values keep appearing; the `.parity/nondet` records merged in; `--verify` at the gate is
  another independent set of runs. A miss shows later as a flaky golden. It is fixed with an
  `$anyOf` overlay, not a code change.
- **Adapter drift.** An adapter that is "fixed" to make a golden pass would hide a regression.
  Mitigation: changes to `adapter/` need `golden diff` = 0 and review (§3.4 rule).
- **Size estimates are off.** GOLD1 measures before anything is committed. Above the caps, the
  fallbacks of §2.3 apply (the MS set cover, fewer LUT seeds), with the justification recorded.
- **PF-B 1222-1240 (`minion.uses`) has no oracle coverage today.** The goldens cannot cover it
  either, unless a fixture reaches it. GOLD3 looks for one. Otherwise it stays a documented gap.

---

## Appendix A. Measurements and method

- **CA all57 PoB results.** From the shared probe cache entry `32df714a41bf822c` (the current CA
  key): 57 `ca.json`, 2,903,049 B raw; 416,403 B with per-file `zstd -19`; 140,251 B as one solid
  zstd stream.
- **Flat golden PoC.** `scratchpad/goldpoc/flat.py` (not committed) re-encoded those 57 files as
  sorted `[key, value]` lines over main, calcs, minion, used and misc: 109,839 lines, 3,913,409 B
  raw; 575,936 B per-file zstd -19; 219,724 B solid; a `git gc --aggressive` pack of the plain
  files is 512 KB.
- **INT-3 results** (cache `32df714a41bf822c`, 57 builds): int3.json 23,913,587 B raw, 1,209,677 B
  zstd; calcs.json (the calcsEnv dump) 24,134,542 B, 2,042,197 B.
- **Query cache** (`.parity/probe-cache/45a4bd578fe68b36`, the current source key), grouped by
  content:

  | Content | Test | Entries | Raw | zstd (as cached) |
  |---|---|---:|---:|---:|
  | `{"probes":…}` | MS-B | 57 | 586,640,779 | 24,844,242 |
  | `{"picked":…}` | MS-A | 57 | 334,099,276 | 15,480,235 |
  | `["build.targetVersion …` | BM-A | 114 | 26,938,107 | 2,829,473 |
  | `["{\"buildFlag\":…` | CO-C (by content) | 58 | 48,349,487 | 1,247,965 |
  | `["skills.activeSkillSet…` | BM-B tabs | 60 | 7,737,885 | 946,186 |
  | `[{"base":{"top":…` | PWR replay records | 7 | 33,438,541 | 2,025,480 |
  | `["pgl.normalMaximum…` | BM-B ProcessGemLevel | 1 | 3,985,692 | 218,800 |
  | `{"src":"unique:amulet#1"…` | IT-A records | 1 | 1,156,247,936 | 2,606,443 |
  | `{"t":"A","line":…` | IT-C records | 1 | 599,572,868 | 4,635,204 |
  | `{"items":[{"src":"unique…` | IT-B | 1 | 43,298,118 | 2,091,065 |

  The test attribution follows the content and each test's script (MS-A's result has `picked`,
  `stores`, `results`, `scale`, `scaleList`; MS-B's has `probes`, `synth`, `stores`, `actors`,
  `items`, `cfgs`, `results` with `[store, op, cfg, names, answer]` tuples).
- **MS-B triviality.** On one build: 112,724 matrix results, of which 64,612 are 0, nil, false or
  empty.
- **parse_oracle.jsonl**: 80,993 lines, 54 MB raw, 1,074,950 B zstd -19.
- **Disk:**
  - `.parity`: probe-cache 27 GB, patched-src 1.0 GB, probe 434 MB, oracle 82 MB, unit 67 MB,
    fresh 28 MB;
  - `data/pob` 418 MB, `data/dump` 281 MB, `data/timeless` 170 MB;
  - `crates/pob-oracle/corpus` 12 MB (418 XML);
  - pob-oracle 81,340 Rust lines, pob-ui-oracle 56,392, pob-dump 6,126.

### GOLD1 measurements (2026-11-13, rust-port `ef2b24ca`)

Script: `scratchpad/gold1/measure.py` (not committed). It reads the caches named in this appendix,
writes golden-form files (`golden/1` header, sorted compact `[key, value]` / `[key, input, value]`
lines, one file per build, 4 MB plain shards for fn.parser.lines) and reports per set: raw bytes,
the sum of per-file `zstd -19`, and the `git gc --aggressive` pack of the set's directory alone.

| Set | Source | Files | Lines | Raw | zstd (per file) | Pack |
|---|---|---:|---:|---:|---:|---:|
| e2e.ca all57 | `32df714a41bf822c` ca.json (main, calcs, mainMinion, mainUsed, misc) | 57 | 109,839 | 3,918,368 | 579,706 | 489,420 |
| e2e.ca special, main block | `outputs.json.zst` of the 87 non-all57 builds in the probe caches | 87 | 71,606 | 2,372,543 | 431,243 | 89,901 |
| e2e.int3 | `32df714a41bf822c` int3.json (Tabulate) + calcs.json (calcsEnv to depth 3) | 57 | 49,407 | 41,351,436 | 2,721,739 | 2,512,053 |
| e2e.bm | `45a4bd578fe68b36` `["build.targetVersion…` (both loads per build) | 114 | 316,966 | 20,951,971 | 1,444,800 | 761,443 |
| fn.parser.lines | `data/dump/parse_oracle.jsonl` → `[line, null, {mods, extra}]` | 5 | 80,993 | 17,293,865 | 537,567 | 931,418 |
| fn.modstore | `45a4bd578fe68b36` MS-B `{"probes"…` (inputs, synth, matrix with trivial answers elided) + MS-A `{"picked"…` (createMod, scale, scaleList, matrix with trivial answers elided) | 114 | 13,096,630 | 817,754,492 | 40,747,304 (MS-B 24,591,126, MS-A 16,156,178) | 78,278,882 (plain) |
| fn.tabs + CO-C | `45a4bd578fe68b36` BM-B tabs, ProcessGemLevel, CO-C | 116 | 156,970 | 61,096,888 | 1,870,597 | 2,376,758 |
| builds XML | `.parity/builds`, `.parity/fresh`, `crates/pob-oracle/corpus/**` | 575 | - | 17,957,913 | - | 3,255,801 |

- all57's full/main byte ratio is 2.5674 (the calcs, used and misc blocks on top of main).
- fn.modstore lines by kind: MS-B synth 5,678,619 lines / 289.9 MB; MS-B inputs 208,828 / 49.7 MB;
  GetMultiplier(NoMod) 1,674,008 / 81.4 MB; GetCondition(NoMod) 945,350 / 46.5 MB; GetStat
  520,357 / 21.7 MB; the other matrix ops 681,682 / 48.9 MB; MS-A matrix 3,165,987 / 224.7 MB;
  MS-A scale 219,252 / 40.1 MB, createMod 2,280 / 13.7 MB, scaleList 267 / 1.2 MB. "Trivial" here
  is the answer alone (Sum 0, More 1, Flag false, Override/Max/Min nil, List/Tabulate nil or
  empty, HasMod false), not yet the record-time check of §2.3.
- Not measurable from the caches: e2e.ca fresh, mirage, d13, levelcost, pwr, mcq;
  fn.parser.fuzz/jewel, lut, tree, spec, items (the current source key holds only the IT
  counts), skills, config CO-A/B, tools; fuzz.*. Their [E] rows stand until their units record.

## Appendix B. Probe: CA L6 on the segment-test corpora

To settle the plan's main risk, `ca_l6_corpus` was run unchanged, with:
- `CA_CORPUS_DIR` = a copy of a prioritised sample of `crates/pob-oracle/corpus/*/*.xml` (stems
  prefixed by unit);
- `POB_QUERY_CACHE=off`, so the shared cache was not written.

The sample: CS2 2, PF-A 1, PF-D 1, PF-F 1, TR-B 35, SK-A 20, SK-B 20, TR-A 10, DF-D 4, OF-D 4,
OF-E 2, DF-F 2, U0 5, DF-B 1, OF-B 2. That is 110 builds.

Result (rust-port `0dbdab33`, 2026-11-13; log in
`scratchpad/goldprobe/sub_l6.log`):

```
CA L6: 110 builds (0 PoB runs failed), 0 Rust runs raised; headline 1430/1430 exact (100.0%),
110 builds with every headline key exact; all keys 86212/86212 exact (100.00%); CALCS lists 330/330;
used-tracking key sets 990/990; misc calculator 2420/2420
CA L6: 0 main-output values accepted as one of PoB's recorded nondeterministic values (§9.5): []
test result: ok. 1 passed; 0 failed; ... finished in 497.44s
```

That is 4.5 s per build for PoB (uncached) and Rust together. An earlier attempt over all 418
corpus XMLs was stopped after 7 builds, all of them exact on every key with the headline exact
(`scratchpad/goldprobe/unit_l6_full_aborted.log`):
- BM-B `synthetic_empty` 756/756, `synthetic_legacy` 749/749, `synthetic_sets` 749/749;
- CO-A, CO-B and CO-C `empty` 756/756 each;
- CS0 `synthetic_socket_props` 766/766.

So **117 builds are checked** (110 + 7).

Not checked: the other **301** unit-corpus builds (SK-A 67, SK-B 107, TR-A 50, DF-D 29, OF-D 14,
DF-F 12, DF-E 7, DF-C 3, OF-B 3, OF-E 4, DF-G 2, OF-F 2, DF-B 1; 117 + 301 = 418), the PS-B/PS-C
synthetic builds (they exist only in code until GOLD2), and the derived fixtures. The SK-A, SK-B
and TR builds of the sample covered only some of the hooks and arms. So this is evidence, not
proof, that the special corpora carry over without fixes.
