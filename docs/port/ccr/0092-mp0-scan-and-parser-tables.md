# CCR-0092: `scan` signature, `clusterJewelSkills` export, jewel-func call shape (MP0)

**Status: applied 2026-11-10 by CCR2 (calc doc §6, contracts.md#mod-mutation). Renumbered from 0014 by CCR2 (0014 stays MS-A's). See AMENDMENTS A8.**

- **Raised by:** MP0 (`crates/pob-modparser/src/{scan,generated_lists,helpers,mod_cache}.rs`)
- **Affects:** calc-architecture.md §6 (`scan` signature); `crates/pob-modparser/src/scan.rs`
  (MP0, frozen signature), `crates/pob-modparser/src/jewel_func_list.rs` (MP7),
  `crates/pob-modparser/src/lib.rs` (U0, re-exports). Consumers: MP1-MP7.
- **Pin:** 16de4b82d
- **Filed status:** open. MP0 works around 1 and 2 locally (marked `CCR-0092`).

## 1. `scan` cannot return `Caps<'l>` borrowed from `line`, nor `&'static T` from a non-static list

Frozen (§6, `scan.rs`):

```rust
pub fn scan<'l, T>(line: &'l str, list: &PatternList<T>, plain: bool) -> Option<(&'static T, String, Caps<'l>)>
```

Evidence:

- ModParser.lua:6620-6622: `local lineLower = line:lower()` … `lineLower:find(pattern, 1, plain)`.
  The captures are substrings of `lineLower`, a string `scan` creates; they are lowercase text
  that does not occur in `line` (e.g. the DMG form reads `dmgTypes[formCap[3]]`, 6819, whose keys
  are lowercase; `"socketed gems are supported by level (%d+) (.+)"` hands the lowercased gem name
  to `extraSupport`). A `Caps<'l>` tied to the caller's `line` cannot hold them without leaking.
- Returning `&'static T` out of `list: &PatternList<T>` is unsound for a non-static list; every
  real list is a `LazyLock` static.

Proposed (as implemented by MP0):

```diff
-pub fn scan<'l, T>(line: &'l str, list: &PatternList<T>, plain: bool) -> Option<(&'static T, String, Caps<'l>)>
+pub fn scan<T: Sync + Send + 'static>(line: &str, list: &'static PatternList<T>, plain: bool)
+    -> Option<(&'static T, String, ScanCaps)>
+/// The captures of a match, owned: `cap[i]` = Lua `cap[i+1]`, `num = tonumber(cap[1])`.
+pub struct ScanCaps { pub num: Option<f64>, pub cap: [Option<String>; 5] }
+impl ScanCaps { pub fn caps(&self) -> Caps<'_>; pub fn get(&self, i1: usize) -> Option<&str>; }
```

`Caps<'s>` (types.rs) is unchanged: the pattern closures still take `&Caps`; the driver lends
it with `ScanCaps::caps()`, after setting `num` per call shape (CCR-0006 §5: specialModList
`tonumber(cap1)`; modTagList `tonumber(cap1)` only when `cap1` contains a digit). Calc doc §6
text: replace the `scan` line with the signature above. `lib.rs` (U0): add
`pub use scan::ScanCaps;` (today it is reachable as `pob_modparser::scan::ScanCaps`).

## 2. `clusterJewelSkills` (ModParser.lua:6596-6607) has no accessor

`jewel_func_list.rs` (MP7) exposes `jewelFuncList: LazyLock<Vec<(&'static str, JewelFuncSpec)>>`
covering 6480-6607, but the driver also reads `clusterJewelSkills[lineLower]` (6653-6656), which
is a different table (line → mods). Proposed addition to MP7's file:

```rust
/// `clusterJewelSkills` (ModParser.lua 6596-6607): lowercased enchant / notable / keystone line → mods.
pub static clusterJewelSkills: LazyLock<ahash::AHashMap<String, Vec<Mod>>>;
```

Workaround: `scan.rs` builds the same table (`CLUSTER_JEWEL_SKILLS_CCR0092`, no `@pob` marker; the
range stays MP7's). When MP7 exports the table, MP0 (or MP7) deletes the copy and reads MP7's.

## 3. The jewel-func value the driver emits (no type change; for MP7)

ModParser.lua:6643-6652 produces `mod("JewelFunc", "LIST", { func = …, type = … })`. The driver
emits `Value::Table { func = Value::JewelFunc(spec), type = "<type>" }` where `spec` is the
`jewelFuncList` entry's `JewelFuncSpec` with:

- `args` = the pattern captures as `Value::Str` (cap1…, non-nil prefix) when the line matched a
  pattern key with a capture (6644-6646: `patternVal.func(cap1, …, cap5)` was called);
- `args` empty when the line equals a key (6649-6651: the stored `{ func, type }` table itself).

`spec.r#type` is set to the entry's type (`"Other"`, `"Self"`, `"SelfUnalloc"`, `"Threshold"`).
MP7's `callJewelFunc` must distinguish the two by `args.is_empty()` (for `"Other"` entries the
exact-key func is the conqueredBy wrapper called directly with `node, out, data`, 6554-6567).
`getSimpleConv` (18-39) returns `JewelFuncSpec { key: "$getSimpleConv", kind: Other, args:
[srcList (array table), dst, type, remove, factor|nil, srcType|nil] }`.

## 4. Notes (no change requested)

- `ModSpec.tag: Option<Tag>` cannot hold DOUBLED's `modExtraTags = { tag = true }` (6868); the
  driver builds that table itself and stores `Tag::default()` as the stand-in (never read: the
  `data[1].tag` branch at 6888 wins).
- DOUBLED with a modNameList table of 3+ names stores the `{ "MORE", "OVERRIDE" }` table as the
  type of mods 3.. (6924 `modType[i] or modType`); `ModType` cannot hold a table, the port stores
  `ModType::Nil` there (`LUA-QUIRK`). The parse oracle has no such line.
- The DOUBLED write into a shared modNameList table (6860, CCR-0006 §1) is reproduced: a
  thread-local override of `modName[2]` per entry (`LUA-ALIAS`), so later parses on that thread
  see the overwritten name, as later parses in the same Lua state do.
- `formList` is `PatternList<Name>` in the frozen stub while lua2rs emits
  `PatternList<&'static str>`; MP1 should convert (or this CCR's successor should align them).
