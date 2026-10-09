# CCR 0001 (P1): generated `global.rs` / `misc.rs` — corrections to calc-architecture.md §4.2 and §9.1

Requested by: P1 (dump + codegen). Affects: calc-architecture.md §4.2 (frozen shape of
`generated/global.rs`), §9.1 step 5, §2.1 ownership line `generated/{global.rs,misc.rs}`.

## 1. `requireSkillTypes` operators are SkillType numbers, not strings

§4.2 says of `SkillTypeExpr`: "data uses the strings "AND"/"OR"/"NOT"". It does not:
the operators are entries of the `SkillType` table itself.

Evidence:
- `Data/Global.lua:291-293`: `OR = 73,` `AND = 74,` `NOT = 75,` inside `SkillType = { ... }`.
- `Data/Skills/act_int.lua:483`: `requireSkillTypes = { SkillType.Instant, SkillType.Spell, SkillType.AND, },`
  (the dump has `[64, 2, 74]`).

So `SkillType` has 135 entries including OR/AND/NOT, and a `requireSkillTypes` /
`excludeSkillTypes` list is a list of `SkillType::T` in which 73/74/75 are operators.

Proposed diff (§4.2):

```diff
-/// requireSkillTypes / excludeSkillTypes RPN tokens (data uses the strings "AND"/"OR"/"NOT").
+/// requireSkillTypes / excludeSkillTypes RPN tokens. The data holds SkillType numbers; the
+/// operators are SkillType.OR (73), SkillType.AND (74), SkillType.NOT (75) (Global.lua:291-293).
 #[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
 pub enum SkillTypeExpr { Type(SkillType::T), AND, OR, NOT }
+impl SkillTypeExpr { pub fn from_skill_type(t: SkillType::T) -> Self; }   // 73/74/75 -> OR/AND/NOT
```

The schema deserialiser (U0) must map the numbers through `from_skill_type`, not parse strings.

## 2. Additions to the generated shape (no removals)

The generated `crates/pob-dump/generated/global.rs` has everything §4.2 lists, with the
same names and types, plus:

- `ModFlag::ENTRIES`, `KeywordFlag::ENTRIES`, `SkillType::ENTRIES` (source order) and
  `SkillType::name(T)`;
- `ClearMatchKeywordFlagsCache()` (a no-op: the memo is not reproduced; CalcSetup.lua:476
  calls it, and §7.2's initEnv header marker names it);
- `/// @pob Data/Global.lua:<a>-<b> h=<sha1[..8]>` markers on every item and a
  `//! @pob-file Data/Global.lua @16de4b82d` header (§3.1 rule 5).

`MatchKeywordFlags` is marked `@pob Data/Global.lua:174-214` (it needs the `band` /
`MatchAllMask` locals at 174-175); `ClearMatchKeywordFlagsCache` 179-184.

## 3. Where the generated files live, and `misc.rs`

Per the P1 assignment, `--codegen` writes `crates/pob-dump/generated/{enums.rs,global.rs,misc.rs}`
(not `pob-gamedata/src/generated/*`, which does not exist yet); U0 moves/includes them.
`misc.rs` holds four record structs: `misc` (`data.misc`, Modules/Data.lua:236-304),
`gameConstants`, `characterConstants`, `monsterConstants` (Data/Misc.lua). Keys that are
not Rust identifiers (e.g. `base_maximum_all_resistances_%`) become fields with `%`→`pct`,
`+`→`plus`, `-`→`minus` and `#[serde(rename = "<verbatim key>")]`; each struct has
`get("<verbatim key>")`, so Lua's `data.characterConstants["base_maximum_all_resistances_%"]`
ports as `data.characterConstants.get("base_maximum_all_resistances_%")` or the field.

Proposed diff (§2.1):

```diff
-  generated/{global.rs,misc.rs}                                          [P1, generated, checked in]
+  generated/{global.rs,misc.rs}   (copied/included from crates/pob-dump/generated/ by U0;
+                                   `pob-dump --codegen --check` guards them)  [P1, generated, checked in]
```

## 4. Dump: `data.misc` was missing

`data.misc` (the magic numbers) was written to `misc.json` and then overwritten by the
group file of the same name (pob-dump's own bug, fixed in P1): data keys whose own file
name is reserved now go into the `misc.json` group, so `misc.json["misc"]` holds it.
FORMAT.md documents this. The manifest no longer carries timings, so two dumps are
byte-identical including `manifest.json` (§9.1 step 6).
