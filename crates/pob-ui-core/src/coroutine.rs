//! coroutine.create/resume/status/yield as `async` blocks polled once per resume (ui doc §6.13,
//! D10). Owner: U4. UI-U0 stub: final signatures.
//!
//! lua: Common.lua:40-56 (the coroutine wrapper), CalcsTab.lua:475-495 (coroutine use). The body
//! reaches `Cx` only through `co::with_cx`, a scoped thread-local (crate `scoped-tls-hkt`) set during
//! `resume`.

use std::future::Future;
use std::pin::Pin;

use crate::cx::Cx;

/// A Lua coroutine: `None` once dead.
#[derive(Default)]
pub struct Coroutine {
    fut: Option<Pin<Box<dyn Future<Output = ()>>>>,
}

impl Coroutine {
    pub fn create(f: impl Future<Output = ()> + 'static) -> Self {
        Coroutine {
            fut: Some(Box::pin(f)),
        }
    }
    /// coroutine.resume: one poll with Waker::noop(), with `cx` installed in the scoped TLS for the poll.
    /// Returns false once dead. A panic inside propagates (Lua: resume returns false, errMsg; the
    /// callers in PoB rethrow/print it; U4 maps this to lua_error with the message).
    pub fn resume(&mut self, cx: &mut Cx) -> bool {
        let _ = cx;
        todo!("U4: coroutine.resume")
    }
    pub fn is_dead(&self) -> bool {
        self.fut.is_none()
    }
}

pub mod co {
    /// Access Cx inside a coroutine body, between yields. Nested use panics (re-entrancy guard).
    pub fn with_cx<R>(f: impl FnOnce(&mut super::Cx) -> R) -> R {
        let _ = f;
        todo!("U4: co::with_cx (scoped-tls-hkt)")
    }
    /// coroutine.yield(): Pending exactly once.
    pub async fn yield_now() {
        todo!("U4: co::yield_now")
    }
}
