# PF-F unit corpus

* `stages.toml`: line probes for `tests/PF-F.rs` (the segment boundaries `perform:2997` /
  `perform:3497` plus the replay brackets `perform:3118`/`3125` around `mergeFlasks` and
  `perform:3484` before `defenceForConditionals`/`doActorCharges`/`doActorMisc`).
* `pff_party_minion.xml`: `.parity/builds/allflame_exp_039.xml` (a minion build) with its party
  tab replaced: an aura, a Vaal aura, an aura debuff, `extraAura`, `otherEffects`, two curses
  (one a mark, curse limit 2), a warcry with `warcryPowerBonus` and a link
  (CalcPerform.lua 2997-3113, 3191-3212).

Fill the dumps (probe cache) and the nondet records (`.parity/unit/PF-F/nondet/`) with the
commands in the header of `tests/PF-F.rs`.
