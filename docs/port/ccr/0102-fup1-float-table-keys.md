# CCR-0102: FUP1: float keys in `Table` (`Key::F`), and the shared patched-source root

Unit: **FUP1** (orchestrator follow-up batch). Status: **pending** (the code on `port/FUP1`
follows it). Renumber on merge if another unit took 0102 first (AMENDMENTS A8).

## 1. `Key` gains a float variant (calc doc §5.1, frozen contract)

Evidence:

- calc doc §5.1 (`calc-architecture.md:754`) freezes
  `pub enum Key { S(Name), I(i64), B(bool) }` with `From<f64>` "(integral → I, else lua_error!)".
- PoB stores non-integral number keys: `Classes/Item.lua:2102` `statOrder[order] = modLine`
  with `order = mod.statOrder[i]`, and flask affixes have fractional stat orders
  (`FlaskBleedCorruptingBloodImmunity3`: `929.1`). IT-B `item_core_vs_pob`, Craft of
  `rare#195`, raised `non-integral number keys are not supported by Table (929.1)` where PoB
  crafts the item (`.parity/fullsuite/20261110-0234/pob-oracle.log`, FOLLOWUPS "HYG2 exposed").
- LuaJIT (`lj_tab.c`): `lj_tab_set`/`lj_tab_get` normalise a number key whose value is an
  integer (`nk == (lua_Number)lj_num2int(nk)`, so `-0.0` is `0`) and keep any other number as
  a hash-part key hashed by `hashnum` (`hashrot(lo, hi << 1)` of its bits); `lj_tab_newkey`
  raises `table index is NaN` (`LJ_ERR_NANIDX`); a NaN read finds nothing.

Change (applied on `port/FUP1`):

```diff
-pub enum Key { S(Name), I(i64), B(bool) }         // From<f64> (integral → I, else lua_error!)
+pub enum Key { S(Name), I(i64), B(bool), F(FKey) } // From<f64>: an integer value (|x| < 2^63,
+                                                   // -0 → 0) → I; NaN → lua_error!("table index
+                                                   // is NaN"); any other float → F
+pub struct FKey(u64);                              // the float's bits (never NaN / ±0); .get() -> f64
+impl Key { pub fn try_num(x: f64) -> Option<Key>;  // a read key: None for NaN (reads nil)
+           pub fn as_num(self) -> Option<f64>; }   // I / F as a number
```

- `Key::F` is always a hash-part key: `pairs` yields it in insertion order with the other
  hash keys (calc doc D8), the `#t` layout model hashes it with
  `pob_lua::ljtab::hashnum_f64` (new; `hashnum` now calls it). `tests/hyg1_len.rs` adds float
  keys (incl. `2.0`, `-0.0`, `1e300`, `2^53`) to its 10 000 LuaJIT-checked sequences: 0
  mismatches.
- `to_val()` gives `Val::Num`. Integral floats up to `2^63` now normalise to `Key::I` (was
  `2^53`), so every integer-valued number has one key.
- `tbl!`: a negated zero literal (`-0.0`) is no Lua constant (`lj_parse.c` `bcemit_unop` does
  not fold `-0`), so `[-0.0] => v` is a run-time key (`__is_lua_const`).
- Exhaustive `match`es on `Key` got a `Key::F` arm (number → `tostring_num`, or the
  number itself): `data_fns/skills_act_dex.rs`, `model/calcs_tab.rs`, `model/party_tab.rs`,
  `calc_defence/est_ehp.rs`, `calc_perform/party_curses.rs`, `probe.rs` (export as a number
  key; `table_set` stores any non-NaN number key instead of the registry fallback), the oracle
  tests' key printers, `pob-ui-oracle/src/lua_calc.rs`. The read helpers of `classes/item.rs`
  (`tget`, `key_lt`) and `classes/item_parse.rs` (`tget_num`) read float keys instead of
  answering nil.

Downstream: code that matches `Key` exhaustively adds the arm; nothing else changes.

## 2. One shared patched-source root (pob-oracle, HYG3 exposed 2)

Evidence: `runner::shared_patch_root` and every test's `patcher::patch(.., &root)` published
one ~839 MB (du; the unpatched files are hard links) copy per `<sha>` per root, and nothing
removed them (FOLLOWUPS "HYG3 exposed" 2).

Change (applied): `patcher::patch` publishes into `patcher::shared_root()`
(`$POB_ORACLE_SRC_ROOT`, else `<.parity>/patched-src`), whatever `root` it is given (`root` is
the fallback without a `.parity`). Every call rewrites `<sha>/.last-used`; once per process it
prunes (`patcher::prune`) the copies of the shared root not used for 24 h
(`$POB_PATCHED_SRC_MAX_AGE_HOURS`, `0` disables), and, in the pre-FUP1 roots (`root`,
`default_root()`, the checkout's `target/oracle-src`), marker-less copies older than 72 h.
`runner::shared_patch_root` is unchanged (editing `runner.rs` retires every probe-cache entry,
`PROBE_RUNTIME`); its result is now only a fallback/legacy root. The sha, and so every cache
key, is unchanged.
