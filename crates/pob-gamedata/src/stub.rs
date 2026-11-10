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

/// Test hook for `unported!` (CCR-0044): called with the site of every hit; `Some(ret)` is the
/// stub's return value (a `Box` of exactly the stub's return type, e.g. a recorded callee's
/// decoded `ret`), `None` keeps `Default`. Thread-local; never set outside tests.
pub type ReplayHook = Box<dyn FnMut(&'static str) -> Option<Box<dyn std::any::Any>>>;

thread_local! {
    static REPLAY: std::cell::RefCell<Option<ReplayHook>> = const { std::cell::RefCell::new(None) };
}

/// Install (or with `None` remove) the `unported!` replay hook; returns the previous one.
pub fn set_replay_hook(h: Option<ReplayHook>) -> Option<ReplayHook> {
    REPLAY.with(|c| c.replace(h))
}

/// The installed hook's return value for a hit of `site` (`None` without a hook or when it
/// declines). The hook is taken out while it runs, so it may call into the calc. A value of
/// another type than the stub returns is a test bug and panics.
pub fn replay<T: 'static>(site: &'static str) -> Option<T> {
    let mut h = REPLAY.with(|c| c.borrow_mut().take())?;
    let r = h(site);
    REPLAY.with(|c| {
        let mut b = c.borrow_mut();
        if b.is_none() {
            *b = Some(h);
        }
    });
    r.map(|b| match b.downcast::<T>() {
        Ok(v) => *v,
        Err(_) => panic!(
            "unported! replay hook for {site}: value is not a {}",
            std::any::type_name::<T>()
        ),
    })
}

/// Unported body: logs once per site, counts the hit (`stub_report()`),
/// returns `Default` (D9), or the replay hook's value ([`set_replay_hook`], tests only).
#[macro_export]
macro_rules! unported {
    ($site:literal) => {{
        $crate::stub::hit($site);
        $crate::stub::replay($site).unwrap_or_default()
    }};
}

#[cfg(test)]
mod tests {
    fn stub_f64() -> f64 {
        crate::unported!("test:replay-f64")
    }

    #[test]
    fn replay_hook_answers_unported_sites() {
        assert_eq!(stub_f64(), 0.0);
        let prev = super::set_replay_hook(Some(Box::new(|site| {
            (site == "test:replay-f64").then(|| Box::new(7.5_f64) as Box<dyn std::any::Any>)
        })));
        assert!(prev.is_none());
        assert_eq!(stub_f64(), 7.5);
        super::set_replay_hook(None);
        assert_eq!(stub_f64(), 0.0);
        assert!(super::stub_report().contains(&("test:replay-f64", 3)));
    }
}
