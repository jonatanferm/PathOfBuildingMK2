# CCR-0128: TP1 per-thread `unported!` counters (so stub-reading L4 tests can pool their Rust side)

**Status: rejected (CCRDOC, 2026-11-12): not built. `pob-gamedata/src/stub.rs` keeps only the process-wide counters, so the stub-reading L4 tests keep their Rust side on the test thread (testing.md "Open").**

Unit: **TP1** (pob-oracle test speed). Filed status: **pending**. Number reserved with
`scripts/port/next_ccr.sh`. Owner of the change: whoever owns `crates/pob-gamedata/src/stub.rs`
(shared by pob-gamedata, pob-modparser and pob-calc; ORC-X / U0).

## Problem

The `unported!` hit counters are one process-wide map:

* `crates/pob-gamedata/src/stub.rs:9` `static HITS: Mutex<BTreeMap<&'static str, u64>>`,
  `:25` `stub_report()`, `:34` `stub_reset()` (re-exported as `pob_calc::stub::*`,
  `crates/pob-calc/src/stub.rs:5`).

L4 acceptance tests attribute a mismatch to "blocked by another unit's stub" by resetting and
reading these counters around one segment run:

* `crates/pob-oracle/tests/OF-F.rs:223-230` (`stub_reset`, run, `stub_report`), `OF-D.rs:258-271`,
  `OF-E.rs:89`, `OF-B.rs:265`, `CO-C.rs:201/539/791` (`stub_hits_since`), `BM-A.rs` (PSB site).

With TP1's corpus pool (`testkit::par_for_each`, `$POB_TEST_PAR` workers) a concurrent build's
hit would land in another build's report (or be cleared by its reset). TP1 therefore keeps those
tests' Rust side on the test thread (OF-F pools only its dump loading; CO-C only its PoB side;
BM-A counts the growth of one site and fails, rather than attributes, a hit under
`POB_TEST_PAR > 1`). These are among the slowest binaries of the full suite (OF-F 262 s, OF-E
213 s, OF-B 177 s, OF-D 145 s, CO-C 422 s; `.parity/fullsuite/20261111-1426/pob-oracle.log`).

## Proposal

Keep the global counters as they are (every existing caller unchanged) and mirror each hit into a
thread-local map, with two new readers:

```diff
 static HITS: Mutex<BTreeMap<&'static str, u64>> = Mutex::new(BTreeMap::new());
+
+thread_local! {
+    /// The hits of this thread since its last `stub_reset_thread` (CCR-0128).
+    static THREAD_HITS: std::cell::RefCell<BTreeMap<&'static str, u64>> =
+        const { std::cell::RefCell::new(BTreeMap::new()) };
+}

 pub fn hit(site: &'static str) {
     let mut h = HITS
         .lock()
         .unwrap_or_else(std::sync::PoisonError::into_inner);
     let c = h.entry(site).or_insert(0);
     if *c == 0 && std::env::var_os("POB_STUB_LOG").is_some() {
         eprintln!("unported: {site}");
     }
     *c += 1;
+    THREAD_HITS.with(|t| *t.borrow_mut().entry(site).or_insert(0) += 1);
 }
+
+/// The sites this thread hit since its last [`stub_reset_thread`], with counts, sorted by site.
+pub fn stub_report_thread() -> Vec<(&'static str, u64)> {
+    THREAD_HITS.with(|t| t.borrow().iter().map(|(k, v)| (*k, *v)).collect())
+}
+
+/// Forget this thread's hits (the process-wide counters are untouched).
+pub fn stub_reset_thread() {
+    THREAD_HITS.with(|t| t.borrow_mut().clear());
+}
```

and `pub use pob_gamedata::stub::{…, stub_report_thread, stub_reset_thread}` in
`crates/pob-calc/src/stub.rs:5`. A segment run is single-threaded, so the per-thread report of a
`stub_reset_thread`/run/`stub_report_thread` bracket is exactly the run's hits.

Then (TP-follow-up, test files only) OF-B/D/E/F, CO-C and BM-A switch to the `_thread` pair and
move their per-build Rust work into `par_for_each`'s worker closure.

## Cost

One thread-local map update per stub hit (stubs are rare and already take a mutex). No product
behaviour change.
