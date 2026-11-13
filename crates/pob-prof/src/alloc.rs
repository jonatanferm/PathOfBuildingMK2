//! A counting global allocator (allocation counts, bytes, live and peak heap). Owner: PROF1.
//!
//! The `pob-prof` binary installs [`Counting`] as its `#[global_allocator]` (unless built with
//! the `dhat-heap` feature, which installs dhat's instead). It wraps the system allocator and
//! counts **per thread**, in plain thread-local cells: the app, its calc and the driver all run on
//! the main thread, and per-thread counters cost a few loads and stores per allocation, where
//! shared atomics cost an atomic read-modify-write each (on aarch64 an out-of-line
//! `__aarch64_ldadd8_relax` call, 1.4 % of the samples of a recalc-heavy scenario in the first
//! version). Allocations of other threads (the wgpu backend's) are not counted. A block freed on
//! another thread than the one that allocated it lowers that thread's live size, so `live` can
//! drift; it is only used for the per-step peak.
//!
//! [`snapshot`] reads the calling thread's counters and [`reset_peak`] restarts its peak at the
//! current live size (the driver does that at every step boundary). `realloc` counts as one
//! allocation of the new size. `POB_PROF_ALLOC_COUNT=0` switches counting off (the check is one
//! relaxed load per call) to measure its overhead: docs/perf/PROFILING.md "Noise".
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

/// Counting on/off (`POB_PROF_ALLOC_COUNT=0` turns it off, to measure its overhead).
static ON: AtomicBool = AtomicBool::new(true);

/// Turn counting on or off for the whole process (call it before the measured work).
pub fn set_counting(on: bool) {
    ON.store(on, Relaxed);
}

thread_local! {
    // `const` initialisers without destructors: no lazy registration, no allocation, so they are
    // safe to touch from inside the allocator (also while a thread is being torn down).
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
    static BYTES: Cell<u64> = const { Cell::new(0) };
    static LIVE: Cell<i64> = const { Cell::new(0) };
    static PEAK: Cell<i64> = const { Cell::new(0) };
}

/// The counting allocator.
pub struct Counting;

fn on_alloc(size: usize) {
    if !ON.load(Relaxed) {
        return;
    }
    let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
    let _ = BYTES.try_with(|c| c.set(c.get() + size as u64));
    grow(size as i64);
}

fn grow(n: i64) {
    if !ON.load(Relaxed) {
        return;
    }
    let _ = LIVE.try_with(|l| {
        let v = l.get() + n;
        l.set(v);
        let _ = PEAK.try_with(|p| {
            if v > p.get() {
                p.set(v);
            }
        });
    });
}

// SAFETY: every method forwards to `System` with the caller's arguments unchanged; the counters
// are thread-local integers and never touch the memory.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarded as is (the caller upholds `GlobalAlloc::alloc`'s contract).
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            on_alloc(layout.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as `alloc`.
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            on_alloc(layout.size());
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: as `alloc`.
        unsafe { System.dealloc(ptr, layout) };
        grow(-(layout.size() as i64));
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: as `alloc`.
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() && ON.load(Relaxed) {
            let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
            let _ = BYTES.try_with(|c| c.set(c.get() + new_size as u64));
            grow(new_size as i64 - layout.size() as i64);
        }
        p
    }
}

/// The calling thread's counters at one point in time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AllocSnap {
    /// Allocations (and reallocations) so far.
    pub allocs: u64,
    /// Bytes requested so far.
    pub bytes: u64,
    /// Bytes live now (allocated minus freed on this thread).
    pub live: u64,
    /// The largest live size since the last [`reset_peak`].
    pub peak: u64,
}

impl AllocSnap {
    /// `self - earlier` for the cumulative counters (live/peak are `self`'s).
    pub fn since(&self, earlier: &AllocSnap) -> AllocSnap {
        AllocSnap {
            allocs: self.allocs.saturating_sub(earlier.allocs),
            bytes: self.bytes.saturating_sub(earlier.bytes),
            live: self.live,
            peak: self.peak,
        }
    }
}

/// Read the calling thread's counters (all zero when [`Counting`] is not the global allocator).
pub fn snapshot() -> AllocSnap {
    AllocSnap {
        allocs: ALLOCS.with(Cell::get),
        bytes: BYTES.with(Cell::get),
        live: LIVE.with(Cell::get).max(0) as u64,
        peak: PEAK.with(Cell::get).max(0) as u64,
    }
}

/// Restart the calling thread's peak at its current live size.
pub fn reset_peak() {
    let l = LIVE.with(Cell::get);
    PEAK.with(|p| p.set(l));
}

/// Whether the counters move (false when another allocator is installed).
pub fn active() -> bool {
    ALLOCS.with(Cell::get) > 0
}
