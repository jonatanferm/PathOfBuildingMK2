//! Bounded parallel corpus loops for the oracle tests (TP1; re-exported by [`crate::testkit`]).
//!
//! Most heavy acceptance tests walk the corpus one build at a time. [`par_for_each`] runs the
//! per-build work `f` on at most [`test_par`] worker threads (`$POB_TEST_PAR`, default 3) and
//! hands every result to `consume` **on the calling thread, in input order**, so a loop body
//! that tallies and prints in `consume` reports exactly what the serial loop reported, whatever
//! the scheduling. Workers run at most [`window`] items ahead of `consume` (bounded memory:
//! results wait in a reorder buffer of at most that many entries).
//!
//! What may go into `f`: anything that does not touch process-global mutable state, e.g. a
//! cached oracle query ([`crate::runner::cached_query`], [`crate::testkit::probe_selection`]),
//! or Rust-side work whose state is per thread. pob-calc's caches and hooks are thread-local, so
//! a worker starts without whatever the test thread set up: put such setup in the `init` of
//! [`par_for_each_init`] (it runs once per worker). Process-global counters
//! (`pob_calc::stub::stub_report`, the timeless-jewel LUT cache) are shared by all workers;
//! work that reads them belongs in `consume`.
//!
//! With `POB_TEST_PAR=1` (or a single item) everything runs serially on the calling thread
//! (the exact pre-TP1 behaviour). A panic in `f` is caught and re-raised on the calling thread
//! when `consume` reaches that item (so the first failing item in input order is the one
//! reported, as in the serial loop); a panic in `consume` stops the workers and is re-raised.

use std::any::Any;
use std::collections::BTreeMap;
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::sync::{mpsc, Condvar, Mutex, PoisonError};

/// The worker count variable.
pub const TEST_PAR_ENV: &str = "POB_TEST_PAR";

/// Default worker count (COMMON memory rule: bounded, 3).
pub const DEFAULT_TEST_PAR: usize = 3;

/// `$POB_TEST_PAR` (at least 1), default [`DEFAULT_TEST_PAR`].
#[must_use]
pub fn test_par() -> usize {
    std::env::var(TEST_PAR_ENV)
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_TEST_PAR)
        .max(1)
}

/// How far (in items) workers may run ahead of the consumer: twice the worker count.
#[must_use]
pub fn window(workers: usize) -> usize {
    2 * workers.max(1)
}

/// Worker stack size: `$RUST_MIN_STACK` when larger, else 64 MiB (deep -O0 calc recursion;
/// only touched pages cost memory).
fn worker_stack() -> usize {
    let min = 64 << 20;
    std::env::var("RUST_MIN_STACK")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .map_or(min, |s| s.max(min))
}

/// [`par_map`] with the order kept: `f` on every item in parallel (bounded), the results in
/// input order.
pub fn par_map<T, R, F>(items: &[T], f: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> R + Sync,
{
    let mut out = Vec::with_capacity(items.len());
    par_for_each(items, |_, t| f(t), |_, _, r| out.push(r));
    out
}

/// `f(index, item)` on up to [`test_par`] workers; `consume(index, item, result)` on the calling
/// thread in input order (see the module docs).
pub fn par_for_each<T, R, F, C>(items: &[T], f: F, consume: C)
where
    T: Sync,
    R: Send,
    F: Fn(usize, &T) -> R + Sync,
    C: FnMut(usize, &T, R),
{
    par_for_each_init(items, || (), |(), i, t| f(i, t), consume);
}

/// [`par_for_each`] with per-worker state: `init()` runs once on each worker thread (or once on
/// the calling thread when serial) before its first item, and `f` gets that state. The state
/// never leaves its thread, so it may hold `Rc`s, a `JitPob`, thread-local setup, ...
pub fn par_for_each_init<T, S, R, I, F, C>(items: &[T], init: I, f: F, consume: C)
where
    T: Sync,
    R: Send,
    I: Fn() -> S + Sync,
    F: Fn(&mut S, usize, &T) -> R + Sync,
    C: FnMut(usize, &T, R),
{
    par_for_each_init_with(test_par(), items, init, f, consume);
}

/// The scheduling state shared by the workers and the consumer.
struct Sched {
    /// The next item a worker may claim.
    next: usize,
    /// Items the consumer has taken (`0..consumed`).
    consumed: usize,
    /// Stop claiming (the consumer panicked or re-raises a worker's panic).
    abort: bool,
}

type Outcome<R> = Result<R, Box<dyn Any + Send>>;

/// [`par_for_each_init`] with the worker count given (tests of this module; callers that must
/// not follow `$POB_TEST_PAR`).
pub fn par_for_each_init_with<T, S, R, I, F, C>(
    workers: usize,
    items: &[T],
    init: I,
    f: F,
    consume: C,
) where
    T: Sync,
    R: Send,
    I: Fn() -> S + Sync,
    F: Fn(&mut S, usize, &T) -> R + Sync,
    C: FnMut(usize, &T, R),
{
    par_for_each_windowed(workers, window(workers), items, init, f, consume);
}

/// [`par_for_each`] for results that are large (e.g. a build's parsed dumps): at most
/// `in_flight` items are claimed and not yet consumed at any time (the one `consume` is on
/// included), on `min($POB_TEST_PAR, in_flight)` workers. `in_flight = 2` overlaps loading the
/// next item with consuming the current one while holding at most two in memory.
pub fn par_for_each_bounded<T, R, F, C>(items: &[T], in_flight: usize, f: F, consume: C)
where
    T: Sync,
    R: Send,
    F: Fn(usize, &T) -> R + Sync,
    C: FnMut(usize, &T, R),
{
    let in_flight = in_flight.max(1);
    par_for_each_windowed(
        test_par().min(in_flight),
        in_flight,
        items,
        || (),
        |(), i, t| f(i, t),
        consume,
    );
}

/// The pool: `workers` threads, a worker claims item `i` only while `i < consumed + ahead`.
fn par_for_each_windowed<T, S, R, I, F, C>(
    workers: usize,
    ahead: usize,
    items: &[T],
    init: I,
    f: F,
    mut consume: C,
) where
    T: Sync,
    R: Send,
    I: Fn() -> S + Sync,
    F: Fn(&mut S, usize, &T) -> R + Sync,
    C: FnMut(usize, &T, R),
{
    let workers = workers.max(1).min(items.len());
    // one worker is the serial loop on the calling thread (`POB_TEST_PAR=1`: the pre-TP1 run)
    if workers <= 1 {
        let mut s = init();
        for (i, t) in items.iter().enumerate() {
            let r = f(&mut s, i, t);
            consume(i, t, r);
        }
        return;
    }
    let ahead = ahead.max(1);
    let sched = Mutex::new(Sched {
        next: 0,
        consumed: 0,
        abort: false,
    });
    let cv = Condvar::new();
    let lock = || sched.lock().unwrap_or_else(PoisonError::into_inner);
    let (tx, rx) = mpsc::channel::<(usize, Outcome<R>)>();
    let stack = worker_stack();
    std::thread::scope(|scope| {
        for w in 0..workers {
            let tx = tx.clone();
            let (sched, cv, init, f) = (&sched, &cv, &init, &f);
            std::thread::Builder::new()
                .name(format!("par-{w}"))
                .stack_size(stack)
                .spawn_scoped(scope, move || {
                    let lock = || sched.lock().unwrap_or_else(PoisonError::into_inner);
                    let mut state: Option<S> = None;
                    loop {
                        // claim the next item inside the window, or stop
                        let i = {
                            let mut g = lock();
                            loop {
                                if g.abort || g.next >= items.len() {
                                    return;
                                }
                                if g.next < g.consumed + ahead {
                                    break;
                                }
                                g = cv.wait(g).unwrap_or_else(PoisonError::into_inner);
                            }
                            g.next += 1;
                            g.next - 1
                        };
                        let r = catch_unwind(AssertUnwindSafe(|| {
                            let s = state.get_or_insert_with(init);
                            f(s, i, &items[i])
                        }));
                        let failed = r.is_err();
                        if tx.send((i, r)).is_err() {
                            return;
                        }
                        if failed {
                            // the worker's state may be inconsistent after a panic
                            state = None;
                        }
                    }
                })
                .unwrap_or_else(|e| {
                    // stop the workers already running (they would wait for a consumer)
                    lock().abort = true;
                    cv.notify_all();
                    panic!("par: spawning worker {w}: {e}")
                });
        }
        drop(tx);
        let stop = || {
            lock().abort = true;
            cv.notify_all();
        };
        let mut pending: BTreeMap<usize, Outcome<R>> = BTreeMap::new();
        let run = catch_unwind(AssertUnwindSafe(|| {
            for (i, item) in items.iter().enumerate() {
                let r = loop {
                    if let Some(r) = pending.remove(&i) {
                        break r;
                    }
                    match rx.recv() {
                        Ok((k, r)) => {
                            pending.insert(k, r);
                        }
                        // every worker exited without sending item i: only after an abort
                        Err(mpsc::RecvError) => panic!("par: item {i} was never run"),
                    }
                };
                match r {
                    Ok(r) => consume(i, item, r),
                    Err(p) => resume_unwind(p),
                }
                lock().consumed = i + 1;
                cv.notify_all();
            }
        }));
        if let Err(p) = run {
            stop();
            // drop what is buffered and let the workers finish their current item
            pending.clear();
            drop(rx);
            resume_unwind(p);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Results come back in input order with any worker count, and the reorder window holds.
    #[test]
    fn ordered_and_bounded() {
        let items: Vec<usize> = (0..40).collect();
        for workers in [1, 2, 3, 8] {
            let consumed = AtomicUsize::new(0);
            let max_ahead = AtomicUsize::new(0);
            let mut seen = Vec::new();
            par_for_each_init_with(
                workers,
                &items,
                || 0usize,
                |n, i, x| {
                    *n += 1;
                    let ahead = i.saturating_sub(consumed.load(Ordering::SeqCst));
                    max_ahead.fetch_max(ahead, Ordering::SeqCst);
                    // later items finish first
                    std::thread::sleep(std::time::Duration::from_micros(((40 - i) * 50) as u64));
                    x * 10
                },
                |i, x, r| {
                    assert_eq!(r, x * 10);
                    seen.push(i);
                    consumed.store(i + 1, Ordering::SeqCst);
                },
            );
            assert_eq!(seen, items, "workers={workers}");
            assert!(
                max_ahead.load(Ordering::SeqCst) < window(workers.min(items.len())),
                "workers={workers}: {} ahead",
                max_ahead.load(Ordering::SeqCst)
            );
        }
        assert_eq!(par_map(&items, |x| x + 1), (1..41).collect::<Vec<_>>());
    }

    /// A panic in `f` surfaces at its item, in input order: the items before it were consumed.
    #[test]
    fn worker_panic_is_reraised_in_order() {
        let items: Vec<usize> = (0..20).collect();
        let mut seen = Vec::new();
        let r = catch_unwind(AssertUnwindSafe(|| {
            par_for_each_init_with(
                3,
                &items,
                || (),
                |(), i, _| {
                    assert!(i != 7 && i != 12, "boom {i}");
                    i
                },
                |i, _, _| seen.push(i),
            );
        }));
        let p = r.expect_err("must panic");
        let msg = p
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_default();
        assert!(msg.contains("boom 7"), "{msg}");
        assert_eq!(seen, (0..7).collect::<Vec<_>>());
    }

    /// A panic in `consume` stops the workers (no hang) and is re-raised.
    #[test]
    fn consumer_panic_stops_workers() {
        let items: Vec<usize> = (0..100).collect();
        let ran = AtomicUsize::new(0);
        let r = catch_unwind(AssertUnwindSafe(|| {
            par_for_each_init_with(
                3,
                &items,
                || (),
                |(), _, _| {
                    ran.fetch_add(1, Ordering::SeqCst);
                },
                |i, _, ()| assert!(i < 5, "stop at {i}"),
            );
        }));
        assert!(r.is_err());
        assert!(ran.load(Ordering::SeqCst) < 5 + 2 * window(3), "workers kept going");
    }

    /// `par_for_each_windowed` with a window of 2: never more than two items claimed and not
    /// consumed, results in order.
    #[test]
    fn bounded_in_flight() {
        let items: Vec<usize> = (0..25).collect();
        let claimed = AtomicUsize::new(0);
        let consumed = AtomicUsize::new(0);
        let max = AtomicUsize::new(0);
        let mut seen = Vec::new();
        par_for_each_windowed(
            2,
            2,
            &items,
            || (),
            |(), i, _| {
                let c = claimed.fetch_add(1, Ordering::SeqCst) + 1;
                max.fetch_max(c - consumed.load(Ordering::SeqCst), Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_micros(((i % 3) * 200) as u64));
                i
            },
            |i, _, r| {
                assert_eq!(i, r);
                seen.push(i);
                consumed.fetch_add(1, Ordering::SeqCst);
            },
        );
        assert_eq!(seen, items);
        assert!(max.load(Ordering::SeqCst) <= 2, "{} in flight", max.load(Ordering::SeqCst));
    }

    /// `init` runs once per worker, on that worker.
    #[test]
    fn init_per_worker() {
        let inits = AtomicUsize::new(0);
        let items: Vec<usize> = (0..30).collect();
        let mut total = 0;
        par_for_each_init_with(
            3,
            &items,
            || {
                inits.fetch_add(1, Ordering::SeqCst);
                std::thread::current().id()
            },
            |id, _, _| {
                assert_eq!(*id, std::thread::current().id());
                1
            },
            |_, _, r| total += r,
        );
        assert_eq!(total, 30);
        assert!(inits.load(Ordering::SeqCst) <= 3);
    }
}
