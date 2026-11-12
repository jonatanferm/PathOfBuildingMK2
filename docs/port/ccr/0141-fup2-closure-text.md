# CCR-0141: a per-closure text for jewel closures in `modLib.formatValue`

**Status: applied (CCRDOC, 2026-11-12; code `ae0dc52` Merge port/FUP2): `mod_tools::{FUNCTION_TEXT_PREFIX, function_text}`. It amends CCR-0091 §6 (AMENDMENTS A14).**

Unit: **FUP2** (docs/port/FOLLOWUPS.md "PWR" item 2; owner of the file: CT). Filed status: **pending**.
Code on `port/FUP2` follows it. Amends CCR-0091 §6 (the "smaller notes" line on
`formatValue` / `formatTag`). Number reserved with `scripts/port/next_ccr.sh`.

## 1. The fixed text made distinct closures one modKey

ModTools.lua:197-198 (`formatValue`): a non-table value prints as `tostring(value)`; for a
jewel closure (`{ type = …, func = function … }`, `Value::JewelFunc` in the port) that is
`function: 0x…`, the closure's address. CCR-0091 §6 recorded that the port prints the fixed
`function: builtin` instead (`pob_gamedata::mod_tools::FUNCTION_TEXT`).

The text is not only displayed. PassiveTree.lua:848 (and PassiveSpec.lua:2448 for cluster
nodes) builds `node.modKey` from `modLib.formatMod`, and the power report keys its cache by
`node.modKey` (CalcsTab.lua:613-626, 672, 720; CompareTab.lua:2670). Two nodes whose mods differ
only in their closure (same `type` / `radius`, other captures) got one key in the port and two
in PoB, so the second node's power was the first node's.

In PoB one closure prints the same everywhere, and every parse of a mod line returns the same
closure (ModParser.lua:6994-7018 caches the parse and `copyTable` keeps functions by reference),
so equal lines give equal keys.

## 2. Contract change

`crates/pob-gamedata/src/mod_tools.rs`:

```rust
/// was: pub const FUNCTION_TEXT: &str = "function: builtin";
pub const FUNCTION_TEXT_PREFIX: &str = "function: 0x";
/// `function: 0x` + 16 hex digits: FNV-1a 64 of the closure's spec (`key`, `kind`, `args`).
pub fn function_text(j: &JewelFuncSpec) -> String;
```

`lua_pairs_of_value` gives a `JewelFunc`'s `func` key the value `function_text(j)`, so
`formatValue`, `formatTag` and `formatMod` print it. The spec is what the port runs for the
closure (the jewelFuncList key or the defining closure's `file:a-b`, and the captures), so two
closures that behave differently print differently, and the same closure prints the same in
every process (an address would not, and the port has no stable address: specs are rebuilt by
the parser and the dump loader). Two different PoB closures with equal specs (two lines that
build the same closure) share one text in the port; they run the same code on the same
captures, so a shared power-cache entry gives the same values (PoB would make one more calc
call).

Readers that compare with PoB's text normalise the hex part, as for PoB's address:
`crates/pob-oracle/tests/PT.rs` `fn_text` (`function: 0x%x+` → `table`, now on both sides),
`PWR.rs` `norm_modKey` and `pob-ui-oracle/tests/u26_power.rs` `norm_mod_key`
(`function: %w+` → `function`, unchanged). `CT.rs`'s `LUA_NORM` maps PoB's text to
`function: builtin`; no `Data/ModCache.lua` entry holds a function, so it compares no closure.

**Proposed diff** (CCR-0091 §6, the `formatValue` / `formatTag` note): "print a jewel closure
… as `function: builtin`" → "print a jewel closure … as `function: 0x` and a hash of its spec
(`mod_tools::function_text`, CCR-0141); Lua prints its address (`function: 0x…`, different per
process)".
