# CCR 0002 (P1): the oracle LuaJIT is dual-number on this machine; ±0 differs from PoB on x86-64

Requested by: P1 (dump + parse oracle). Affects: calc-architecture.md D8/§3 (Lua-semantics
rules: "LuaJIT NaN and ±0 semantics, oracle-verified"), §9.2 (oracle, P3/ORC-X), §9.5
(nondeterminism runs), and every unit that compares numbers against the oracle.

## Finding

The development machine is ARM64. LuaJIT on ARM64 is always dual-number
(`luajit-src-210.5.12+a4f56a4/luajit2/src/lj_arch.h:252-273`: `LJ_ARCH_NUMMODE LJ_NUMMODE_DUAL`;
`:540-550`: `LJ_DUALNUM 1`, `LUAJIT_NUMMODE=1` is rejected with `#error`). PoB's reference
build (Windows x86-64) is single-number: every Lua number is a double.

Measured on the vendored LuaJIT (mlua, `Lua::new()`), ARM64:

| expression | vendored LuaJIT (ARM64) | x86-64 PoB / IEEE |
|---|---|---|
| `local t = {value=-0}; 1/t.value` | `inf` (+0) | `-inf` |
| `-0.0` literal | +0 | -0 |
| `-tonumber("0")`, interpreter (also with `jit.off()`) | +0 | -0 |
| `-tonumber("0")`, once traced by the JIT | -0 | -0 |
| `tonumber("0") * -1` | +0 | -0 |
| `-(tonumber("0") / 1)`, `0/-1` | -0 | -0 |

(The system LuaJIT 2.1.1788856981, also ARM64, gives -0 for the first and third rows, so
newer LuaJIT fixed part of this; the vendored one has not.)

Evidence that PoB's own semantics is -0: `Data/ModCache.lua` (written by PoB on x86-64) has
14 `value=-0`, e.g. `Data/ModCache.lua:2887` `c["0% reduced Attack Speed"]=...value=-0`, which
is `modValue = -modValue` on `tonumber("0")` (`Modules/ModParser.lua:6760,6767`).

Consequences seen in P1:
- `modCache.json` had `0` instead of `-0` for those 14 entries (the literal is folded to an
  integer 0).
- `parse_oracle.jsonl` was **not deterministic**: `"Debuffs on you expire 0% slower"`
  (`ModParser.lua:4343`, `-num`) came out `-0` in one run and `0` in the next, depending on
  whether the function had been traced yet (trace timing varies with the random string-hash
  seed).

## What P1 did (inside crates/pob-dump)

- `src/lua/prelude.lua`: when this LuaJIT loses the sign of the literal `-0`, a `=-0`
  followed by `,`/`}` is rewritten to `=(0/-1)` as files load (line numbers kept; counted in
  `manifest.json` `stats["negZeroLiterals:<file>"]`; at 16de4b82d: only ModCache.lua, 14).
- Parse oracle: `jit.off()` plus ModParser's `tonumber` returning `n / 1` (a double) through a
  chunk environment that is otherwise `_G`. The oracle now reproduces all 14 ModCache `-0`s
  and is byte-identical across runs (tested).
- Not fixed: values *computed* during `Modules/Data.lua` post-processing by unary minus or
  multiplication of an integer 0 (the main dump runs with the JIT on). None is known, but no
  check exists either.

## Proposed changes

calc-architecture.md §3 (Lua-semantics rules), add:

```diff
+- **Number mode.** PoB's reference LuaJIT is x86-64 single-number: every number is a double and
+  `-x` of a zero is -0. Rust f64 arithmetic matches that. An oracle running on a dual-number
+  LuaJIT (any ARM64 host) does not (`-tonumber("0")` and the literal `-0` give +0 in its
+  interpreter, -0 once traced), so on such hosts the oracle must run with `jit.off()` and with
+  `tonumber` returning doubles, or the diff tool must treat a ±0 mismatch as a known host
+  artefact (recorded in `known_diffs.toml` with this CCR's number), never as a port bug.
```

§9.2 (P3/ORC-X): the patched oracle should install the same two measures as
`crates/pob-dump/src/lua/parse_oracle.lua` (`jit.off()`, `tonumber` → `n / 1`, `-0` literal
rewrite in its loader), and report the host's number mode (`1/loadstring("return -0")() < 0`)
in every dump header, so a ±0 difference can be attributed. Alternatively run the oracle on
an x86-64 host.

§9.5: on a dual-number host, two runs of the same build can differ in the sign of a zero
through JIT trace timing; with the JIT on, a ±0 difference between runs is that, not PoB
nondeterminism.
