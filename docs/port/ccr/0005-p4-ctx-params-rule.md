# CCR-0005: §7.1 "exactly the crossing locals" does not hold for parameters

**Status: applied 2026-10-09 by CCR1.** Applied to calc doc §7.1.

- **Raised by:** P4 (`scripts/port/locals_audit.py`)
- **Affects:** calc-architecture.md §7.1 (wording only; no table or ctx struct changes)
- **Pin:** 16de4b82d

## Problem

§7.1 says: "A ctx holds **exactly** the locals declared in one segment and referenced in
another." Measured with the scope-aware analyser, every local in the table satisfies that,
but one listed parameter does not: `env` of `calcs.defence` (CalcDefence.lua:651) is
referenced only in 666-1191, i.e. only in the first segment (651-1210), yet the row lists
`params env actor` and `DefenceCtx` (§7.2) has an `env` field.

Conversely `override`, a parameter of `calcs.initEnv` (CalcSetup.lua:475), is not listed:
it is read only at 485 (`local override = override or { }`), which declares the function
local that *is* listed (`override`485).

The table is therefore produced by this rule, which `locals_audit.py` implements and which
reproduces all seven §7.1 rows byte for byte:

> A parameter is a ctx field when it crosses, and also when it does not cross but no
> function-body local of the same name shadows it.

(The orchestrator receives every parameter anyway, so keeping non-crossing parameters in
the ctx costs nothing and keeps the ctx signatures uniform.)

## Proposed diff (calc-architecture.md §7.1, first paragraph)

```diff
-These lists come from `scripts/port/locals_audit.py`, a scope-aware analyser that resolves every identifier to its declaration. The analyser was run against the pin for this document. A ctx holds **exactly** the locals declared in one segment and referenced in another. Everything else stays a local in its segment. If U0's re-run differs, U0's output wins and is recorded in `contracts.md`.
+These lists come from `scripts/port/locals_audit.py`, a scope-aware analyser that resolves every identifier to its declaration. The analyser was run against the pin for this document. A ctx holds **exactly** the locals declared in one segment and referenced in another, plus the function's parameters: a parameter is a field unless it does not cross and a function-body local of the same name shadows it (initEnv's `override`; the local `override`485 is the field). At the pin the only listed parameter that does not itself cross is `env` of `calcs.defence`. Everything else stays a local in its segment. If U0's re-run differs, U0's output wins and is recorded in `contracts.md`.
```

## Local workaround

None needed: `locals_audit.py --check` applies the rule above and prints the `env` case as
a note.
