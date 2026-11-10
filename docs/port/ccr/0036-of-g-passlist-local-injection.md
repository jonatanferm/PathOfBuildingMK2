# CCR-0036: OF-G — `passList` cannot be injected into `OffenceCtx` (every offence stage after 1998)

**Status: needs code (CCR2, 2026-11-10; ORC-X2). CCR-0065 (OF-D) is superseded by this one. See AMENDMENTS A8.**

Filed status: proposed by OF-G (2026-11-09); worked around in `crates/pob-oracle/tests/OF-G.rs`
(marked `CCR-0036`). The fix belongs to ORC-X (`crates/pob-calc/src/probe.rs` or
`crates/pob-oracle/src/inject.rs`), which OF-G does not own.

## Problem

`OffenceCtx.passList` is `Shared<Vec<Table>>` (calc doc §7.2, `calc_offence/mod.rs`). Its
`ProbeField::from_val` accepts only the boxed form (`v.obj::<SBox<Vec<Table>>>()`,
`crates/pob-calc/src/probe.rs:614-619`, `sbox_field!` at 627-633). Injection decodes every dumped
local with `Kind::Any` (`crates/pob-oracle/src/inject.rs:110-113` visits, 139-146 decodes), so the
dumped `passList` (a Lua sequence of pass tables) arrives as `Val::Table`. `ctx_fill`
(`probe.rs:2965-2984`) then rejects it, and `run_segment` fails before the segment runs:

```text
run offence:4155#4: injected locals of the wrong type: passList = table
```

This hits every offence stage from the declaration of `passList` (CalcOffence.lua:1998) on:
`offence:2174`, `2607`, `3346`, `3957`, `4155`, `4165`, `4990`, `5650`, i.e. the L4 tests of
OF-D … OF-I. `testkit::segment_diff` cannot be used for them as is. (ORC-X's plumbing test
runs `offence:362`, before `passList` exists, so it does not see this.)

## Proposed diff (either one)

1. Accept the plain table in the field conversion (passList is never aliased by another Lua
   table, so a fresh `Shared` holding the same pass tables is the same state):

```diff
 // crates/pob-calc/src/probe.rs, sbox_field! (or a dedicated impl for Vec<Table>)
             fn from_val(v: &Val) -> Option<Self> {
-                v.obj::<SBox<$t>>().map(|b| b.0.clone())
+                v.obj::<SBox<$t>>().map(|b| b.0.clone()).or_else(|| /* Vec<Table> only */
+                    v.table().map(|t| Shared::new(t.ipairs().into_iter()
+                        .filter_map(|(_, p)| p.table()).collect())))
             }
```

2. Or decode ctx locals with the kind of the ctx field they fill (`ProbeState` knows each
   field's `ProbeField::kind()`, here `Kind::TableList`), so `inject` allocates a `TableList`.

## Workaround (applied in OF-G's test)

`tests/OF-G.rs::segment_diff` is `testkit::segment_diff` plus, after injection, replacing the
`passList` local by `Shared::new(<its pass tables>).to_val()`. With it the prelude and body
segments run and diff normally. Remove it once ORC-X applies (1) or (2).

Note: OF-D filed the same problem concurrently (`0034-of-d-passlist-local-injection.md` on
`port/OF-D`); whichever is applied resolves both. (It was merged as CCR-0065, which CCR2 marked superseded by this CCR.)
