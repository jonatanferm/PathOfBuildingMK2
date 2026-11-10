# CCR-0033: U5 Lua-helper audit: gaps, `lua-utf8`, and oracle shims

**Status: applied 2026-11-10 by CCR2 (§5 ui doc §8 rule 8, §6 ui doc §13 item 15); §1-§4 need code. See AMENDMENTS A8. ORC-X2 (2026-11-10) applied §1 (`pob_lua::{lj_num2int, lua_tointeger}`; `luastr::{lua_int, lua_integer}` re-export them); §2-§4 still need code.**

**Filed status: proposed** (U5, ui doc §12.3). Nothing here edits `pob-lua` or the oracle crates: U5
worked around every item locally (marked `CCR-0033` in `crates/pob-ui-core/src/luastr.rs`).
The §8 resolution table itself lives in the module docs of `crates/pob-ui-core/src/luastr.rs`
and is compile-checked by `crates/pob-ui-core/tests/luastr.rs::audit_resolves`.

Renumbered from 0014 at integration (0014 was already used by CT, MP0 and MS-A); the code comments say `CCR-0033`.

## 1. `pob-lua`: make the Lua-number → integer casts public (to calc:P2)

Evidence: `crates/pob-lua/src/format.rs:253` `pub(crate) fn cvtt_i64` and `:265`
`pub(crate) fn cvtt_i32` are the x64 `cvttsd2si` casts that LuaJIT applies to number arguments:
`lj_lib_checkint`/`lj_lib_optint` (`lj_lib.c:242-254`; the `string.sub` fast path's
`cvttsd2si r32`, `vm_x64.dasc:1935, 1948`) and `lua_tointeger` on 64-bit (what C modules such
as `lua-utf8` see). UI code keeps indices as `f64` (ui doc §8 rules 1, 3), so it needs both;
`pob_lua::string::sub` takes `i64` and leaves the conversion to the caller.
U5 workaround: copies in `luastr::{lua_int, lua_integer}` (marked `CCR-0033 (item 1)`), tested
against LuaJIT (`sub_with_number_indices_vs_luajit`) and, for the x64-only NaN/±inf/out-of-range
results, against the VM source (`x64_number_arguments`).

```diff
 // crates/pob-lua/src/format.rs
-pub(crate) fn cvtt_i64(n: f64) -> i64 {
+pub fn cvtt_i64(n: f64) -> i64 {          // `(lua_Integer)n`, lua_tointeger on 64-bit
-pub(crate) fn cvtt_i32(n: f64) -> i32 {
+pub fn cvtt_i32(n: f64) -> i32 {          // lj_num2int, lj_lib_checkint
 // crates/pob-lua/src/lib.rs
+pub use format::{cvtt_i32 as lj_num2int, cvtt_i64 as lua_tointeger};
```

After that, `luastr::{lua_int, lua_integer}` become re-exports.

## 2. `lua-utf8` and `formatNumSep` have no owner; the calc needs them too (to calc:P2 / calc:U0)

Evidence: PoB loads the C module `lua-utf8` (`EditControl.lua:10`, `Common.lua:30`); the DLL
(`runtime/lua-utf8.dll`) is built by PathOfBuilding-SimpleGraphic, whose pin `3b1a346` vendors
`libs/luautf8` at starwing/luautf8 **bdd3d7f** (GitHub contents API for `libs?ref=3b1a346`; the
DLL's strings list `grapheme_indices` and no `widthlimit`, matching that commit's entry table).
Uses: `utf8.match`/`utf8.next` (`EditControl.lua:555-572`, ctrl/plain LEFT/RIGHT caret moves),
`utf8.reverse/gsub/find/sub` (`Common.lua:812-826`, `formatNumSep`). Neither plan assigns them;
`pob_lua` has no UTF-8 functions. Its semantics are not Lua's `string`: character indices,
Unicode classes from `unidata.h`, `invalid UTF-8 code` errors, `%` must be followed by a digit or
`%` in replacements, `reverse` keeps combining marks attached unless `lax`.

`formatNumSep` (`Common.lua:803-828`) is called by the UI (CalcBreakdownControl, TreeTab,
PassiveTreeView, CompareTab, Build, ExtBuildListControl, CalcFormat) **and by the calc**:
`CalcOffence.lua:4046-4048` (`output.DisplayDamage` in `CALCS` mode). It reads
`main.showThousandsSeparators/thousandsSeparator/decimalSeparator`, which the calc env has no
contract for.

U5 did: `pob_ui_core::luastr::utf8` (a port of `lutf8lib.c` for `sub`, `reverse`, `next`, `find`,
`match`, `gsub` and the whole matcher; `unidata.rs` transcribes the needed `unidata.h` tables) and
`luastr::format_num_sep(s, show, sep, dec) -> (String, usize)` plus `format_num_sep_main(&MainCore, s)`.
Both are verified against the real C module and PoB's own `formatNumSep` text (6200 vectors,
`tests/luastr_data/`).

Proposed: move both into `pob-lua` (`pob-lua/src/utf8.rs`, `pob_lua::string::formatNumSep(str,
showThousandsSeparators, thousandsSeparator, decimalSeparator)`), keep `luastr` re-exporting them,
and give the calc a way to read the three `main` settings in `CALCS` mode (the calc doc has no
contract for `main.*` reads). The move is mechanical (no UI-only dependency except
`format_num_sep_main`, which stays in `luastr`).

## 3. `math.random` / `math.randomseed` (to calc:P2, and U8 for determinism)

Evidence: `TimelessJewelListControl.lua:7, 133-134` (`m_random(2, 17)` picks a devotion variant
when none is set) and `PoEAPI.lua:94-101` (OAuth state). `pob_lua::math` has no PRNG. Proposed:
port LuaJIT's `lib_math.c` `math_random`/`math_randomseed` (PRNG state per Lua state) into
`pob_lua::math`, and have U8 seed it (or patch the call) so traces stay deterministic. Owners:
U28 (timeless list) and U43 (PoEAPI) call it; nothing blocks Wave 1.

## 4. The oracles' `lua-utf8` stand-in diverges from PoB (to U8 / calc:ORC-X; pob-oracle owner)

Evidence: `crates/pob-oracle/src/jit.rs:205-213` and `crates/pob-dump/src/lua/prelude.lua:101-109`
preload `lua-utf8` as a table of `string.*` functions plus `offset`/`codepoint`:

1. **No global `utf8`.** The real module registers with `luaL_register(L, "utf8", libs)` (Lua 5.1
   API; the DLL imports `luaL_register`), which also creates `_G.utf8`. Verified with LuaJIT and
   the module built from bdd3d7f: after `require("lua-utf8")`, `type(_G.utf8) == "table"` and
   `rawequal(_G.utf8, module)`. So in real PoB `main:DetectUnicodeSupport()` (`Main.lua:307-313`)
   sets `main.unicode = true` (and prints `Unicode support detected`), and every EditControl
   without an explicit filter uses `"%c"` instead of `"^%w%p "` (`EditControl.lua:50`). Under the
   oracle shim `main.unicode` is false. The Rust `MainCore::unicode` must be `true` (U16), and the
   UI oracle must reproduce the global, or the EditControl filter differs between the two sides.
2. **No `utf8.next`.** `EditControl.lua:558/572` (LEFT/RIGHT without ctrl) would raise
   `attempt to call field 'next' (a nil value)` under the oracle.
3. **Byte semantics instead of UTF-8 semantics** for `sub`/`find` (character indices), `%w`/`%s`/
   `%p` (Unicode tables), `gsub` replacements (`%x` raises), `reverse`. Equal only on ASCII input.

Proposed for U8 (`pob-ui-oracle`): load the real module. `lutf8lib.c` + `unidata.h` (MIT) compile
against the vendored LuaJIT headers (`mlua-sys`'s `luajit-build/include`) with the `cc` crate in a
dev-only build script, `-DNDEBUG` (the DLL is a Release build: `Lutf8_reverse`'s `assert(ends ==
prev)` is compiled out, and stray continuation bytes are copied, which the port reproduces), and
`luaopen_utf8` is registered in `package.preload["lua-utf8"]` with the `luaL_register` global
side effect. At minimum, the shim must set `_G.utf8` and provide `next`.

## 5. Error messages from C functions carry the Lua caller's position (doc note, ui doc §8 rule 8)

Evidence: with the real module, an error raised by `utf8.gsub` inside `formatNumSep` reads
`Modules/Common.lua:813: invalid use of '%' in replacement string` (LuaJIT `luaL_where(L, 1)` is
the calling Lua frame); called directly from `pcall` it has no prefix. `lua_error!` messages in
`pob_lua` and `luastr` never carry the position. The vectors compare messages without the
`<chunk>:<line>: ` prefix. Proposed: ui doc §8 rule 8 adds "messages are compared without the
`file:line:` prefix that LuaJIT adds for errors raised from Lua code".

## 6. Unowned `Common.lua` helpers seen by the audit (to the ui doc §12 table)

Callers exist in several units, but no unit row names these ranges: `naturalSortCompare` 673-704
(FolderListControl, BuildListHelpers), `getFormatNumSep` 830-834, `formatRound`/`getFormatRound`
837-845, `formatPercent`/`getFormatPercent` 848-856, `formatSec`/`getFormatSec` 859-871
(CalcBreakdownControl, CompareCalcsHelpers), `urlEncode`/`urlDecode` 1024-1036 (ImportTab,
TradeQuery, CompareBuySimilar), `string:matchOrPattern` 1038-1079 (ItemDBControl,
MinionSearchListControl, NotableDBControl, PassiveTreeView, ConfigTab, CompareTab). Proposed: assign them to U5's file
(`luastr.rs`) as a follow-up, or to the first consuming unit in wave order (U12 for
`naturalSortCompare`/`matchOrPattern`, the CalcBreakdownControl unit for the `format*` family,
U43 for `url*`).
