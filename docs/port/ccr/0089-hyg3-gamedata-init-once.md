# CCR-0089: HYG3 — `pob_gamedata::init` decodes `data.bin` once per process

Status: applied by HYG3 (2026-11-09); behaviour-only change to a U0-frozen file, no signature change.
Numbered 0089 in HYG3's fix round (it was first filed as 0080, which BM-A's build-model CCR holds; 0087
and 0088 are claimed by in-flight branches).

## Problem

`crates/pob-gamedata/src/data.rs` (U0, frozen) initialises the process-global `data` with a
check-then-decode:

```rust
pub fn init(bytes: &[u8]) -> Result<(), LoadError> {
    if DATA.get().is_some() {
        return Ok(());
    }
    let tree = loader::decode_bin(bytes)?;   // ~40 MB postcard, 1.4 s at -O0
    let d = Data::from_tree(&tree)?;
    let _ = DATA.set(d);
    Ok(())
}
```

and `init_from_dir` reads the whole `data.bin` before it calls `init`. Parallel `#[test]`s of one
test binary all call it first thing (`testkit::init_default_data`, the tests' own `init_data`), so
every one of them that gets past the `DATA.get()` check before the first `DATA.set` reads and
decodes the file; all but one result are thrown away. ORC-X has 5 tests, BM-B and the IT-* files
several: up to 4 x 1.4 s of CPU and about 1 GB of extra RSS per binary (test audit 2026-11-09,
fix S5, `data.rs:635-641`).

`OnceLock::get_or_try_init` (which would express this directly) is unstable.

## Change (applied)

```diff
 // crates/pob-gamedata/src/data.rs
+static INIT: std::sync::Mutex<()> = std::sync::Mutex::new(());
+
 pub fn init(bytes: &[u8]) -> Result<(), LoadError> {
     if DATA.get().is_some() {
         return Ok(());
     }
+    let _once = INIT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
+    init_locked(bytes)
+}
+
+fn init_locked(bytes: &[u8]) -> Result<(), LoadError> {
+    if DATA.get().is_some() {
+        return Ok(());
+    }
     let tree = loader::decode_bin(bytes)?;
     ...
 }

 pub fn init_from_dir(dir: &std::path::Path) -> Result<(), LoadError> {
     ... (tree provider registration unchanged)
+    if DATA.get().is_some() {
+        return Ok(());
+    }
+    let _once = INIT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
+    if DATA.get().is_some() {
+        return Ok(());
+    }
     let bytes = std::fs::read(dir.join("data.bin"))...;
-    init(&bytes)
+    init_locked(&bytes)
 }
```

Public signatures and results are unchanged: a second call is still a no-op returning `Ok`, an
error is still returned to the caller that hit it (and a later call retries, as before), and
`init_with` is untouched. The only difference is that concurrent first callers wait for the one
decode instead of each doing their own. The test-side JSON-dump path
(`pob_oracle::testkit::try_init_data`) is serialised the same way with its own mutex.

## Contract text

calc doc §4.4 ("the process-global `data`") gains one sentence (applied, as a doc comment on `init`): "`init` / `init_from_dir` are
thread-safe: concurrent first calls decode `data.bin` once."
