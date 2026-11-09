# CCR-0034: OF-D — `passList` cannot be injected into `OffenceCtx`

Status: proposed by OF-D (2026-11-09)

## Problem

Every offence segment that starts after the `passList` declaration
(CalcOffence.lua:1998; stages `offence:2174`, `offence:2606`/`2607`, `3346`,
`3957`, `4155`, `4165`, `4990`, `5650`) receives `passList` as an injected
local. `pob_oracle::inject` visits and decodes every dumped local with
`Kind::Any` (crates/pob-oracle/src/inject.rs:110-113 and 140-146), so the
Lua array of pass tables arrives as a plain `Val::Table`. The ctx field is
`passList: Shared<Vec<Table>>` (calc doc §7.2; crates/pob-calc/src/modules/calc_offence/mod.rs),
whose `ProbeField::from_val` accepts only an `SBox<Vec<Table>>`
(crates/pob-calc/src/probe.rs:614-620). `ctx_fill` therefore rejects it and
`run_segment("offence:2174#n", …)` fails on every build with

    injected locals of the wrong type: passList = table

(`crates/pob-oracle/tests/OF-D.rs`, first run, 2/2 builds).

## Proposed diff (ORC-X files; either one fixes it)

1. `pob_oracle::inject`: decode a local whose name is a ctx field with that
   field's `ProbeField::kind()` (`Kind::TableList` for `passList`) instead of
   `Kind::Any`, e.g. by passing the `ProbeState::FIELDS` kinds of the stage's
   ctx structs to `inject` (or a `local_kinds: &[(&str, Kind)]` table next to
   `run_segment`).
2. Or, smaller: `impl ProbeField for Shared<Vec<Table>>`'s `from_val` also
   accepts a `Val::Table` whose `ipairs` values are all tables, building
   `Shared::new(those tables)` (the pass tables keep their identity; `passList`
   itself is referenced only by the local, CalcOffence.lua:1998-2064).

## Workaround (until applied)

`crates/pob-oracle/tests/OF-D.rs` (`pass_list`, marked `CCR-0034`) rebuilds
the injected `passList` local as `Shared<Vec<Table>>` from the same pass tables
before calling `run_segment`. The other offence units' tests need the same until
this is applied.
