# TR-B corpus

One build per CalcTriggers.lua configTable arm that PoB's trigger spec (the TR-A corpus) does
not reach. Each build is a Lua case of `TRB_CASES` in `tests/TR-B.rs` (helpers `TRB_HELPERS`:
equip a unique from `data.uniques` or a raw item, paste socket groups, select the main skill), run
in PoB from a fresh build and saved with `build:SaveDB`. Regenerate with

    scripts/port/cargo.sh test -p pob-oracle --test TR-B -- --ignored trb_generate_corpus

(`TRB_CASES=trb_snipe,…` regenerates a subset). Two arms have no item or skill of their own at the
pin and are reached through a rare item of that name granting another triggered skill:
`call to arms` ("backwards compatibility only") and `ngamahu's flame` (the unique's Molten Burst
is claimed first by the `TriggeredMoltenStrike` skill-id arm).

`tests/TR-B.rs` (`trb_l4_triggers`) runs the L4 comparison over this corpus, the TR-A corpus and
the `.parity/builds` that carry a trigger rate.
