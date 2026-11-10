# CCR-0068: DF-D — CalcDefence module locals; `calcs.armourReduction` (withdrawn)

Status: withdrawn by DF-D (2026-11-09): superseded, DF-A landed on rust-port in edd8169
("Merge port/DF-A"). No contract change remains.

This CCR was raised while DF-A (`calc_defence/helpers.rs`, CalcDefence.lua 1-650) had not
landed. DF-D (`calc_defence/est_inputs.rs`, CalcDefence.lua 1681-2152) needed:

- §1 the module-level locals `isElemental` (L24), `hitSourceList` (L27) and `dmgTypeList` (L28);
- §2 a ported `calcs.armourReduction` (L75-77), which it calls at 2030 and 2033.

DF-A as merged provides all of it: `helpers.rs` exports `pub(crate) fn isElemental(&str)`,
`pub(crate) const hitSourceList` and `pub(crate) const dmgTypeList`, and `armourReduction` is
the real L75-77 body. DF-D's private copies and the `// CCR-0068` markers are deleted;
`est_inputs.rs` now does `use super::helpers::{dmgTypeList, hitSourceList, isElemental}`.
`crates/pob-oracle/tests/DF-D.rs` now fails on any hit that reaches an `unported!` stub.

Results after merging rust-port into port/DF-D (L4 `buildDefenceEstimations:1681` →
`buildDefenceEstimations:2153`, 2026-11-09):

| corpus | builds | hits | clean | failed | blocked |
|---|---|---|---|---|---|
| all57 | 57 | 363 | 363 | 0 | 0 |
| unit corpus `corpus/DF-D` | 33 | 198 | 198 | 0 | 0 |
| slice5 | 5 | 30 | 30 | 0 | 0 |
