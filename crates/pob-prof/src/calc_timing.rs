//! Recalc latency as the app sees it: a [`CalcFacade`] that times the Rust calc it wraps.
//! Owner: PROF1.
//!
//! The app's only seam to the calc is `CalcFacade` (ui doc §10.4), injected through
//! `PobApp::with_calc`. [`TimedCalc`] forwards every call to `RustCalc` and adds its wall time to
//! a thread-local [`CalcCounters`]: full recalcs (`calcsTab:BuildOutput()`, Build.lua:1258-1266),
//! `GetMiscCalculator` calls, and the calls of the misc calculator closures it hands out (the
//! tooltips' stat differences, the gem and config previews). The power report's calc calls run
//! inside pob-calc (`CalcsTab:BuildPower`, once per tree draw) and are not seen here; they show in
//! the frame times of the steps that run it.

use std::cell::Cell;
use std::rc::Rc;
use std::task::Poll;
use std::time::Instant;

use pob_app::calc::facade::{BuildRef, CalcFacade, MiscCalc, Output, Override};

/// Cumulative calc counters of this thread (nanoseconds).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CalcCounters {
    pub full_n: u64,
    pub full_ns: u64,
    pub misc_get_n: u64,
    pub misc_get_ns: u64,
    pub misc_call_n: u64,
    pub misc_call_ns: u64,
}

impl CalcCounters {
    pub fn since(&self, e: &CalcCounters) -> CalcCounters {
        CalcCounters {
            full_n: self.full_n - e.full_n,
            full_ns: self.full_ns - e.full_ns,
            misc_get_n: self.misc_get_n - e.misc_get_n,
            misc_get_ns: self.misc_get_ns - e.misc_get_ns,
            misc_call_n: self.misc_call_n - e.misc_call_n,
            misc_call_ns: self.misc_call_ns - e.misc_call_ns,
        }
    }
}

thread_local! {
    static COUNTERS: Cell<CalcCounters> = const { Cell::new(CalcCounters {
        full_n: 0, full_ns: 0, misc_get_n: 0, misc_get_ns: 0, misc_call_n: 0, misc_call_ns: 0,
    }) };
    /// Each full recalc's duration (ns), drained by the driver.
    static FULL_LOG: std::cell::RefCell<Vec<u64>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// This thread's counters now.
pub fn counters() -> CalcCounters {
    COUNTERS.with(Cell::get)
}

/// The durations of the full recalcs since the last call (ns).
pub fn take_full_log() -> Vec<u64> {
    FULL_LOG.with(|l| std::mem::take(&mut *l.borrow_mut()))
}

/// Put back counters and a full-recalc log taken earlier ([`counters`], [`take_full_log`]): the
/// calc work in between leaves the books (the driver's untimed guards and snapshots).
pub fn restore(c: CalcCounters, log: Vec<u64>) {
    COUNTERS.with(|v| v.set(c));
    FULL_LOG.with(|l| *l.borrow_mut() = log);
}

fn add(f: impl FnOnce(&mut CalcCounters)) {
    COUNTERS.with(|c| {
        let mut v = c.get();
        f(&mut v);
        c.set(v);
    });
}

fn ns(t0: Instant) -> u64 {
    t0.elapsed().as_nanos() as u64
}

/// The timing facade around another calc (the app's `RustCalc`).
pub struct TimedCalc(pub Box<dyn CalcFacade>);

impl CalcFacade for TimedCalc {
    fn build_output(&mut self, build: &BuildRef) -> Poll<()> {
        let t0 = Instant::now();
        let r = self.0.build_output(build);
        let d = ns(t0);
        add(|c| {
            c.full_n += 1;
            c.full_ns += d;
        });
        FULL_LOG.with(|l| l.borrow_mut().push(d));
        r
    }

    fn misc_calculator(&mut self, build: &BuildRef) -> Poll<(Rc<dyn MiscCalc>, Output)> {
        let t0 = Instant::now();
        let r = self.0.misc_calculator(build);
        let d = ns(t0);
        add(|c| {
            c.misc_get_n += 1;
            c.misc_get_ns += d;
        });
        r.map(|(f, base)| (Rc::new(TimedMisc(f)) as Rc<dyn MiscCalc>, base))
    }
}

/// A misc calculator closure that times its calls.
pub struct TimedMisc(pub Rc<dyn MiscCalc>);

impl MiscCalc for TimedMisc {
    fn call(&self, ov: Option<Rc<Override>>, use_full_dps: Option<bool>) -> Poll<Output> {
        let t0 = Instant::now();
        let r = self.0.call(ov, use_full_dps);
        let d = ns(t0);
        add(|c| {
            c.misc_call_n += 1;
            c.misc_call_ns += d;
        });
        r
    }
}

/// The calc factory for `PobApp::with_calc`: the app's `RustCalc` behind [`TimedCalc`].
pub fn timed_factory() -> Rc<dyn Fn() -> Box<dyn CalcFacade>> {
    Rc::new(|| Box::new(TimedCalc(Box::new(pob_app::calc::RustCalc))))
}
