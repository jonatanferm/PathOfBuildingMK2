# CCR-0014: HYG1 — LuaJIT `#t` for `Table`, table constructors, test-input policy

**Status: proposed (unit HYG1).** Everything below is already implemented on `port/HYG1`;
the orchestrator only needs to amend the doc text. No frozen signature changed: every item
is an addition, and `LjIntMap<V>` keeps its exact API.

## A. `Table::len` is LuaJIT's border (calc doc §3.2 table, §5.1)

Evidence: calc doc §3.2 maps `#t` to "`t.len()` (border; hole-free contract)"; the U0 code
returned the length of the storage's dense prefix. LuaJIT (`jit.off`) gives
`t[1]=1 t[2]=2 t[3]=3 t[2]=nil; #t` = **1**, `local a,b,c=1,2,3; t={a,b,c}; t[2]=nil; #t` = **3**
(`BC_TNEW`, asize 4), and `t={1,2,3}; t[2]=nil; #t` = **1** (a template table grown by rehashes to
asize 5, `lj_parse.c:1757-1776`); U0 gave 3, 3, 3. `table.insert`/`table.remove` use the border too
(`lib_table.c:82`, `:111`), so contents diverged after a hole as well.

Applied (`crates/pob-calc/src/lua_table.rs`, `crates/pob-lua/src/ljtab.rs`):

```diff
 pub struct TableData {
     arr: Vec<Val>, hash: IndexMap<Key, Val, ahash::RandomState>, meta: Option<IndexMeta>,
+    layout: pob_lua::ljtab::LjTab<Key, ()>,   // LuaJIT's layout of this table (presence only)
 }
-    pub fn len(&self) -> usize;   // #t
+    pub fn len(&self) -> usize;   // #t = lj_tab_len on `layout` (every store, nil included, feeds it)
+/// A table constructor `{ ... }`: expr_table's template (constant fields) + BC_TDUP, or BC_TNEW,
+/// then the run-time stores in field order.
+pub fn from_ctor(fields: Vec<TableField>) -> Table;
+pub struct TableField { pub key: CtorKey<Key>, pub konst: bool, pub val: Val }   // item/named/bracket ctors
+#[doc(hidden)] pub const fn __is_lua_const(rust_source: &str) -> bool;          // tbl! helper
```

`tbl!` now builds through `Table::from_ctor`: a value (or a `[k]` key) written as a literal
(`1.0`, `-2`, `"s"`, `n!("s")`, `true`, `false`, `Val::Nil`) is a Lua constant (template); any
other expression is a run-time store. LuaJIT's constant folding of arithmetic (`{1+2}`) is not
detected: write the folded literal. `copyTable` models LuaJIT filling the copy in LuaJIT's `pairs`
order of the source; `wipe` keeps sizes and dead nodes, as `wipeTable` does. `rawget` and `set`
are unchanged. `pairs()` order (§3.4/D8: insertion order) of a constructed table is field order,
as with the U0 `tbl!`, which stored every field in turn: `from_ctor` stores the fields in turn and
only the `#t` layout model sees LuaJIT's template-then-run-time order (`tbl!{ a = x, b = 1.0, c = x }`
iterates `a, b, c`; test `ctor_pairs_follow_field_order`). Two value changes, both to Lua's
semantics: a positional item is keyed by its position (U0 pushed, so a nil item shifted the rest:
`tbl![a, Val::Nil, c]` gave `{a, c}`, now `{[1] = a, [3] = c}`), and with a duplicate key a run-time
field beats a later constant one (`{a = x, a = 1}` is `x` in LuaJIT, whose template is in place
before the run-time stores); such a key is set to the run-time value after the fields, keeping its
slot unless the constant was nil.

`pob_lua::ljtab` gains the generic engine (same `lj_tab.c` port, now over any key type):

```diff
+pub trait LjKey: Copy + Eq { fn as_int(&self) -> Option<i64>; fn from_int(i: i64) -> Self;
+                             fn hash(&self) -> u32; fn is_str(&self) -> bool { false } }
+pub struct LjTab<K, V>;   // new, with_sizes, with_raw (lj_tab_new), get, contains_key, set, len,
+                          // is_empty, pairs, keys, copyTable, wipe, sizes, reasize (lj_tab_reasize)
+pub fn hashnum(k: i64) -> u32; pub fn hashbool(b: bool) -> u32; pub fn str_sid_standin(s: &str) -> u32;
+pub enum CtorKey<K> { Item, Named(K), Bracket { key: K, konst: bool } }
+pub struct CtorField<K> { pub key: CtorKey<K>, pub konst: bool, pub nil: bool }
+pub struct Ctor<K> { pub table: LjTab<K, ()>, pub fields: Vec<(K, bool /* run-time store */)> }
+pub fn table_ctor<K: LjKey>(fields: &[CtorField<K>]) -> Ctor<K>;   // lj_parse.c expr_table + TNEW/TDUP
-pub struct LjIntMap<V> { /* … */ }
+pub struct LjIntMap<V>(LjTab<i64, V>);   // same methods, same behaviour (L0 l0_ljtab: 0 mismatches)
```

**Limit (proposed doc text for §3.4/§5.1).** LuaJIT hashes a string key by its `sid`
(`lj_tab.h` `hashstr`), an interner id handed out in creation order and reseeded from a PRNG
(`lj_str.c:265-295`), so where string keys land in the hash part, and therefore when a mixed
table rehashes, differs between LuaJIT processes. The model uses a fixed stand-in
(`str_sid_standin`, FNV-1a). Result: `#t` is exact for tables keyed by numbers/booleans; for a
table mixing string keys with *holed* integer keys LuaJIT itself is nondeterministic (measured:
two LuaJIT states disagree on 190 of 3000 random mixed sequences, Rust vs LuaJIT on 196), and
`len()` is always a valid border. Hole-free sequences are unaffected (the border is unique).

Acceptance (`crates/pob-calc/tests/hyg1_len.rs`): 10 000 random construction + mutation
sequences (constructors with constant/run-time items, `name =`, `[k] =` incl. `[0]`, 250-300 and
~2050-item constructors; stores of values and nil, `table.insert` both forms, `table.remove` both
forms, `wipeTable`, `copyTable`; number and boolean keys) against LuaJIT `jit.off`: 0 mismatches in
`#t` after every step and in the final contents.

## B. Test-input policy (AMENDMENTS A5 addition)

Evidence: tests located gitignored inputs in five different ways and mostly *passed* silently when
they were absent (`crates/pob-dump/tests/{dump,codegen,parse_oracle}.rs` "skipping: PoB source not
found", `crates/pob-gamedata/tests/dump_load.rs` "SKIPPED", `crates/pob-oracle/tests/*` unless
`POB_ORACLE_REQUIRE=1`/`CI`, `crates/pob-oracle/src/lua/parser.rs` relative `../../.PathOfBuilding`,
`crates/pob-sg/src/font/mod.rs` "SKIP: PoB fonts not found").

Applied: dev-only crate `crates/pob-testpaths` (`pob_src`, `pob_root`, `data_dir`, `dump_dir`,
`data_bin`, `parity_dir`, `parity_build`, `find`, `require`). Search order: env override
(`POB_SRC`, `POB_ROOT`, `POB_DATA_DIR`, `POB_DUMP_DIR`, `POB_PARITY_DIR`), this workspace and its
ancestors, the linked worktree's main checkout, then `/home/ferm/Work/projects/PathOfBuildingMK2`.
A missing input **fails** the test; `POB_ALLOW_SKIP=1` turns it into `SKIPPED (…)` + return.
`POB_ORACLE_REQUIRE` is retired. Proposed A5 bullet:

> - **Test inputs.** Tests find the PoB checkout, `data/`, `data/dump` and `.parity` only through
>   the dev-dependency `pob-testpaths` and call `pob_testpaths::require`: a missing input fails the
>   test unless `POB_ALLOW_SKIP=1`. Opt-in tests (`POB_ORACLE_PROBE=1`) and hardware checks (no GPU
>   adapter) are not input skips.
