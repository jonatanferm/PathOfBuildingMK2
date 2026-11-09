# CCR-0034: jewel func contract (MP7)

- **Raised by:** MP7 (`crates/pob-modparser/src/jewel_func_list.rs`, `crates/pob-calc/src/modules/jewel_funcs.rs`)
- **Affects:** calc-architecture.md §6 (jewel funcs), §8 (closure families row "Jewel funcs");
  `pob_gamedata::JewelFuncSpec` doc comment; consumers MP3-MP6 (the two `ExtraJewelFunc`
  specialModList entries) and CS0/CS2 (CalcSetup radius jewels).
- **Pin:** 16de4b82d
- **Status:** open. MP7 implements everything below (no frozen type changes; one frozen fn
  signature changes, §1).

## 1. `getThreshold` cannot express its Lua arguments

Frozen stub: `getThreshold(attrib: &str, name: &str, modType: ModType, value: Value, tags: &[Tag])`.

Evidence: ModParser.lua:6455 `getThreshold(attrib, name, modType, value, ...)`;
`attrib` is a table in 8 entries (6526-6533 `{ "Int","Dex" }`, branch 6459 `type(attrib) == "table"`);
`...` goes to `mod(name, modType, value, "", ...)` (6456) and carries flags / keywordFlags in
three entries (6481-6482 `ModFlag.Melee`, 6519 `ModFlag.Attack`, 6520 `0, bor(KeywordFlag.Hit, KeywordFlag.Poison)`).

```diff
-pub fn getThreshold(attrib: &str, name: &str, modType: ModType, value: Value, tags: &[Tag]) -> JewelFuncSpec
+pub fn getThreshold(attrib: Value /* Str or array Table */, name: &str, modType: ModType, value: Value,
+                    flags: u32, keywordFlags: u32, tags: &[Tag]) -> JewelFuncSpec
```

## 2. Additions to `jewel_func_list.rs`

- `jewelSelfUnallocFuncs`, `jewelThresholdFuncs` statics (the stub had one static for 6378-6452
  and none for 6480-6549); same shape as `jewelOtherFuncs`.
- `jewelFuncListSource: LazyLock<AHashMap<&'static str, &'static JewelFuncSpec>>`: the table value
  `v` each jewelFuncList wrapper closes over (6554-6592), by lowercased key. `callJewelFunc` needs
  it because the emitted spec (CCR-0014 §3) names only the key.
- `callCaptureClosure(v, caps) -> Option<JewelFuncSpec>`: Lua `v(cap1, …, cap5)` for the
  capture closures (`function(num) return … end`), returning the node func they build.
- `clusterJewelSkills: LazyLock<AHashMap<String, Vec<Mod>>>` (CCR-0014 §2, as proposed there).
  `scan.rs`'s stand-in copy `CLUSTER_JEWEL_SKILLS_CCR0014` is deleted and the driver reads MP7's
  table (CCR-0014 §2 allowed MP7 to do this).

Value encoding (module docs of `jewel_func_list.rs`): helper closures `"$getSimpleConv"`,
`"$getPerStat"` (`args = [dst, modType, flags, stat, factor]`), `"$getThreshold"`
(`args = [attrib, Value::Mod(baseMod)]`); inline closures by source range
`"Modules/ModParser.lua:<a>-<b>"` (the parse oracle's `src`); the 6544-6547 table as `"$list"`
with the funcs as `Value::JewelFunc` args.

## 3. The two `ExtraJewelFunc` closures (for MP3-MP6)

ModParser.lua:2621-2635 build `mod("ExtraJewelFunc", "LIST", { radius = …, type = "Other", func = function … end }, tags…)`.
`callJewelFunc` runs them when the specialModList entry emits:

| Lua | `JewelFuncSpec` |
|---|---|
| 2622-2627 (upvalue `dmgType`, the 2nd capture) | `key: "Modules/ModParser.lua:2622-2627"`, `args: [Str(dmgType)]`, `kind: Other`, `r#type: Some("Other")`, `radius: Some(firstToUpper(radius))` |
| 2630-2634 (upvalues `val`, `attr`, captures 2-3) | `key: "Modules/ModParser.lua:2630-2634"`, `args: [Str(val), Str(attr)]`, same `kind`/`r#type`, `radius: Some(radius:gsub("^%l", string.upper))` |

The captures are the lowercased-line substrings scan() returns (strings, not `tonumber`ed).
The mod value is the table `{ func = Value::JewelFunc(spec), radius = …, type = "Other" }` as in
the parse oracle.

## 4. `"$default"` (for CS2)

`JewelFuncSpec.key` documents `"$default"` as "ModParser.lua 6552-6595"; the closure is
CalcSetup.lua:877-884 (`{ type = "Self", func = function(node, out, data) … tally Str/Dex/Int }`).
CS2 should emit `JewelFuncSpec { key: "$default", kind: Self_, args: [], r#type: Some("Self"), radius: None }`;
`callJewelFunc` runs the body (provenance stays CS2's; the body carries a plain reference).
Doc fix: `"$default"` (CalcSetup.lua:877-884).

## 5. Reproduced quirks (no change requested)

- `LUA-ALIAS` 6473-6474: getThreshold writes `value.mod.source = data.modSource` into its shared
  upvalue after `copyTable(baseMod)`, so each finish call's copy carries the previous call's source
  (nil the first time). Reproduced with a thread-local keyed by the closure's baseMod.
- The capture thresholds (6513-6515, 6537, 6541, 6542) pass the capture string as the mod value
  (`getThreshold(…, num, …)` with `num` a string): the mod value is `Value::Str("20")`; ModStore
  arithmetic coerces it as Lua does.
- 2632 `firstToUpper(attr):match("^%a%l%l")` is nil for an attribute shorter than 3 letters; Lua then
  adds a mod with a nil name, which a `Name` cannot hold: the port raises a LuaError there (no item
  line reaches it).
- An exact line equal to a capture pattern key (impossible in practice) runs the capture closure with
  `node, out, data` as captures; arithmetic captures raise, as in Lua.
