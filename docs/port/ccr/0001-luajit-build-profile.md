# CCR-0001: The oracle's LuaJIT is an ARM64 dual-number build; pob-lua mirrors the target's LuaJIT profile

**Raised by:** P2 (pob-lua). **Affects:** calc doc D8, §3.4, §3.5, §4.1 (`math.rs`, `format.rs`), §9.4 L0, §9.5; P3/ORC-X (oracle), every unit that relies on the sign of zero.

## Problem

The calc doc assumes an x64, single-number LuaJIT (`cvttsd2si` casts, `-0`
results, "`tostring` spellings `nan`/`-nan`"). The project's oracle machine is
**aarch64** (`uname -m`; `rustup target list --installed` shows
`aarch64-unknown-linux-gnu`), so the vendored LuaJIT that `pob-oracle` builds
(`luajit-src 210.5.12+a4f56a4`) is the ARM64 build. PoB itself ships on
Windows x64. The two LuaJIT builds differ observably:

| Behaviour | x64 build (PoB) | ARM64 build (oracle) | Evidence |
|---|---|---|---|
| Number mode | single-number | **dual-number** (`LJ_DUALNUM`): integer-valued results are int32 values, which have no `-0` | `lj_arch.h:273` (`LJ_ARCH_NUMMODE LJ_NUMMODE_DUAL` in the ARM64 block, 255-273); `lj_arch.h:544-546` |
| `(int64_t)n`, `(int32_t)n` (`lj_strfmt_putfnum_int/uint`, `lj_num2int` for `%c`, `string.sub` args, …) | `cvttsd2si`: NaN / out of range → MIN | `fcvtzs`: saturating, NaN → 0 | `lj_strfmt.c` `lj_strfmt_putfnum_int`; `lj_obj.h:1000`. Observed: `string.format("%d", 1e30)` = `9223372036854775807`, `("%d", 0/0)` = `0`, `("%x", 1e30)` = `7fffffffffffffff` |
| `tonumber("-0b0")`, `tonumber("-0", base)` | `-0` | `+0` (scanned as an integer) | `lj_strscan.c` `strscan_bin` INT case; `lib_base.c` `tonumber` `LJ_DUALNUM` branch |
| `hashrot` | x86 spelling | generic spelling | `lj_tab.h:18-32` — **the two spellings are the same function** (`lo' = (lo^hi) - rol(hi,14)`, `hi' = lo' ^ rol(hi,19)`, result `hi' - rol(lo',13)`), so `LjIntMap`/`pairs()` order is platform-independent. The §3.4 example order was re-verified against the oracle. |

Independently of the platform, compiled traces disagree with the interpreter
in a few places (found by L0, which runs every vector with `jit.off` and
`jit.on`):

1. **`string.sub(s, i, j)` with a variable negative `j` is wrong in compiled
   traces of this LuaJIT**: the trace returns the substring as if `j` were -1.
   1897 of 20000 random vectors differ from the interpreter (the system
   `/usr/bin/luajit`, a different 2.1 build, has no such bug). Minimal repro
   (prints `bad 2413`):
   ```lua
   local n, seed = 12000, 12345
   local function rnd(m) seed = (seed * 1103515245 + 12345) % 2147483648 return seed % m end
   local I, J, S, out = {}, {}, {}, {}
   for k = 1, n do I[k] = rnd(29) - 14 J[k] = rnd(29) - 14 S[k] = ("abcdefghij"):sub(1, rnd(11)) end
   for k = 1, n do out[k] = S[k]:sub(I[k], J[k]) end
   jit.off() local bad = 0
   for k = 1, n do if out[k] ~= S[k]:sub(I[k], J[k]) then bad = bad + 1 end end
   print("bad", bad)
   ```
   Wherever PoB code reaches `sub` with a variable negative end inside a
   compiled trace, oracle outputs can be wrong relative to both the
   interpreter and x64 PoB (not yet audited which PoB call sites can).
2. Trace narrowing drops the sign of zero of `math.floor`/`math.ceil` results
   (`roundSymmetric(-0.4)` is `-0` interpreted, `+0` in a trace: 210 of 12000
   vectors).
3. The JIT folds `x ^ 2` to `x * x` (`lj_opt_fold.c:1112-1123`), which differs
   from libm `pow` by 1 ulp in rare cases (1 of 20928 vectors).

Smaller contract inaccuracies found on the way:

- §4.1 `lpow`: "integral exponent: lj_vm_powi" — this LuaJIT has no
  `lj_vm_powi`; `BC_POW` calls libm `pow` (`vm_x64.dasc:3352-3355`).
- §3.5 / §4.1 "LuaJIT spellings … `nan`/`-nan`": LuaJIT 2.1's own formatter
  (`lj_strfmt_num.c`, non-finite branch of `lj_strfmt_wfnum`) never prints a
  NaN sign; only `nan`/`NAN` (and `inf`/`-inf`).
- `%.14g` rounds exact decimal ties up (`nd_add_m10e`), unlike glibc
  `printf`: `tostring(123456789012345)` = `1.2345678901235e+14` (oracle-verified).
- §4.1 `CatBuf`: a memo "keyed on the parts' (ptr,len)" is unsound for
  transient (non-`'static`, non-interned) parts; implemented as a
  content-keyed thread-local memo with the same API.
- §4.1 `impl From<LStr> for FmtArg`: `FmtArg::Str` borrows, so the impl is
  `From<&LStr>` (`&Rc<str>`).

## What pob-lua does now (workaround, `// CCR-0001`)

- `pob_lua::LJ_TARGET_X86ORX64` / `pob_lua::LJ_DUALNUM` (lib.rs) select the
  LuaJIT build profile of the compilation target: x86/x86_64/**wasm32** → x64
  profile (PoB's shipping platform), everything else (aarch64 here) → ARM64
  dual-number profile. Used by `format.rs` (float→int casts, `tonumber`
  dual-number zeros). So L0 on this machine compares like with like, and
  x64/wasm builds behave like x64 PoB. The x64 profile is a faithful
  transcription but **cannot be verified on this machine** (no x64 LuaJIT).
- `lpow` passes both operands through `std::hint::black_box`. Without it,
  release builds inlined `lpow(x, 2.0)` and LLVM rewrote `pow(x, 2.0)` as
  `x*x`, which is the JIT's fold (item 3 above) and not the interpreter's
  libm `pow`. L0 `lj_math_binary` failed in `--release` only, on 1 of 20928
  vectors (`x = -1.9948972224101745e7`). LLVM also rewrites
  `pow(x, -1)` → `1/x`, `pow(2, n)` → `exp2`/`ldexp` and `pow(10, x)` →
  `exp10`, and constant-folds `pow` at compile time. `black_box` hides
  the operands from all of these. The unit test
  `math::tests::lpow_is_not_folded` guards this, and L0 must be run in
  both debug and `--release`.
- **libm provenance.** `exp`/`log`/`log2`/`log10`/`pow` come from the
  target's libm: glibc on this machine (the same libm the oracle's LuaJIT
  links), MSVC's UCRT for Windows PoB, and Rust's bundled `libm` port
  (musl-derived) on wasm32. These implementations are not all correctly
  rounded, so a wasm32 (or any non-Windows) build can differ from Windows
  PoB by 1 ulp in these functions. Only the native-glibc profile is checked
  by L0. The diff tolerance for libm-derived keys on wasm32 still needs to
  be decided (calc doc §9).
- L0 policy: the interpreter (`jit.off`) is the bit-exact reference. In
  `jit.on` runs, a result where the trace differs from the interpreter and
  Rust equals the interpreter is counted and reported ("JIT-only
  divergences tolerated"), not failed.

## Proposed diff

calc doc §3.5, add:

> **LuaJIT build profile.** LuaJIT's number semantics depend on the build:
> x64 (PoB's platform) is single-number with `cvttsd2si` casts; ARM64 is
> dual-number with saturating casts. `pob_lua::{LJ_TARGET_X86ORX64,
> LJ_DUALNUM}` select the profile of the compilation target (wasm32 → x64).
> The Rust calc's arithmetic is single-number (`f64` everywhere); where the
> ARM64 oracle produces `+0` for an integer-valued result that x64 PoB
> produces as `-0`, the diff treats `±0` as equal unless the key is printed
> with `tostring`. The interpreter is the reference for L0; trace-only
> divergences of LuaJIT are tolerated and counted.

calc doc §4.1: replace "`// LuaJIT ^ (integral exponent: lj_vm_powi)`" with
"`// LuaJIT ^ = libm pow (the JIT folds x^2 to x*x)`"; replace
"`nan`/`-nan`" with "`nan`"; `CatBuf` memo "keyed on the concatenated bytes";
`From<&LStr>`.

calc doc §9.2 / P3 (ORC-X): run the oracle with `jit.off()` (or verify on an
x64 host) for parity dumps, or at least record that oracle values from code
that calls `string.sub` with variable negative ends in hot loops are suspect
(bug 1 above). Consider running the nondeterminism pass (§9.5) once with
`jit.off` to separate JIT artefacts from hash-seed nondeterminism.
