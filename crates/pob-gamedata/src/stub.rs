//! Unported bodies (calc doc §5.7, D9): `unported!("File.lua:a-b")` logs once
//! per site, counts the hit and returns `Default`. Shared by pob-gamedata,
//! pob-modparser and pob-calc (whose `stub` module re-exports this), so one
//! report covers every crate.

use std::collections::BTreeMap;
use std::sync::Mutex;

static HITS: Mutex<BTreeMap<&'static str, u64>> = Mutex::new(BTreeMap::new());

/// Record a hit of an unported site (logs the first one to stderr when
/// `POB_STUB_LOG` is set).
pub fn hit(site: &'static str) {
    let mut h = HITS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let c = h.entry(site).or_insert(0);
    if *c == 0 && std::env::var_os("POB_STUB_LOG").is_some() {
        eprintln!("unported: {site}");
    }
    *c += 1;
}

/// Every site hit so far, with its count, sorted by site.
pub fn stub_report() -> Vec<(&'static str, u64)> {
    HITS.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .map(|(k, v)| (*k, *v))
        .collect()
}

/// Forget all hits.
pub fn stub_reset() {
    HITS.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}

/// Unported body: logs once per site, counts the hit (`stub_report()`),
/// returns `Default` (D9).
#[macro_export]
macro_rules! unported {
    ($site:literal) => {{
        $crate::stub::hit($site);
        Default::default()
    }};
}
