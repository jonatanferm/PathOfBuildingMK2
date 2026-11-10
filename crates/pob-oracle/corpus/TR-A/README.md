# TR-A corpus

One build per case of PoB's own trigger spec (`spec/System/TestTriggers_spec.lua` at the pin,
16de4b82d): each `it(...)` body (assertions removed) is run in PoB from a fresh build and the
result saved with `build:SaveDB`. Regenerate with

    scripts/port/cargo.sh test -p pob-oracle --test TR-A -- --ignored tra_generate_corpus

`tests/TR-A.rs` (`tra_l4_triggers`) probes every `triggers:in`/`triggers:out` hit of these builds
(plus the `.parity/builds` that carry a trigger rate) and runs the L4 comparison.
