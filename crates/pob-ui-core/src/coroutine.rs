//! coroutine.create/resume/status/yield as `async` blocks polled once per resume (ui doc §6.13,
//! D10). Owner: U4.
//!
//! lua: Common.lua:46-57 (the coroutine wrapper: every coroutine is registered in a weak table that
//! SimpleGraphic reads through `coroutine._list()` for frame pacing), CalcsTab.lua:475-495 (the
//! canonical use: create on a flag, resume once per frame, drop it when `status == "dead"`).
//!
//! @pob-file Modules/Common.lua @16de4b82d
//!
//! The body reaches `Cx` only through `co::with_cx`, a scoped thread-local (crate
//! `scoped-tls-hkt`, no `unsafe`) installed for the duration of each poll. A coroutine stored in
//! widget state is resumed through `cx.lend` (§6.13), so the future never aliases `Cx`.
//!
//! Lua mapping:
//! - `co = coroutine.create(f)`: `Coroutine::create(async move { .. })`.
//! - `local res, errMsg = coroutine.resume(co, ...)`: `co.try_resume(cx)` returns `(res, errMsg)`
//!   exactly (a Lua error inside the body is caught, the coroutine is dead, `res` is false).
//!   `co.resume(cx)` is the plain form: a Lua error propagates as the `lua_error!` panic it is
//!   (callers in dev mode rethrow it: `if launch.devMode and not res then error(errMsg) end`).
//! - `coroutine.status(co) == "dead"`: `co.is_dead()`.
//! - `coroutine.yield()`: `co::yield_now().await`. `coroutine.running()` (truthy inside a body):
//!   `co::running()`.
//! - `coroutine._list()` (SG frame pacing: any coroutine not dead): `co::any_alive()`.
//!
//! DIVERGENCE(args): `coroutine.resume(co, self)` passes `self` as the body's first argument; an
//! `async` body captures what it needs (`me: CtrlId`) instead.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Wake, Waker};

use crate::cx::Cx;

scoped_tls_hkt::scoped_thread_local!(static mut CX: Cx);

thread_local! {
    /// Coroutines created and not yet dead (Common.lua:48 `active_coroutines`, minus the dead ones
    /// SG skips).
    static ALIVE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Depth of `Coroutine::resume` calls on the stack (`coroutine.running()`).
    static RUNNING: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A coroutine's body, counted in `ALIVE` until it finishes, fails or is dropped.
struct Body(Pin<Box<dyn Future<Output = ()>>>);

impl Body {
    fn new(f: Pin<Box<dyn Future<Output = ()>>>) -> Body {
        ALIVE.with(|a| a.set(a.get() + 1));
        Body(f)
    }
}

impl Drop for Body {
    fn drop(&mut self) {
        ALIVE.with(|a| a.set(a.get() - 1));
    }
}

/// `Waker::noop()` (std 1.85) without raising the workspace MSRV.
struct NoopWake;
impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
}

/// A Lua coroutine: dead once its body has returned or raised an error.
pub struct Coroutine {
    fut: Option<Body>,
}

impl Default for Coroutine {
    /// A dead coroutine (what a lent slot reads as while its coroutine runs, §6.13).
    fn default() -> Self {
        Coroutine { fut: None }
    }
}

impl Coroutine {
    /// lua: Common.lua:49-53 coroutine.create (wrapped: the coroutine is registered as active)
    /// @pob Modules/Common.lua:46-57 h=ee32faea
    pub fn create(f: impl Future<Output = ()> + 'static) -> Self {
        Coroutine {
            fut: Some(Body::new(Box::pin(f))),
        }
    }
    /// coroutine.resume: one poll with Waker::noop(), with `cx` installed in the scoped TLS for the poll.
    /// Returns false once dead. A panic inside propagates (Lua: resume returns false, errMsg; the
    /// callers in PoB rethrow/print it; U4 maps this to lua_error with the message).
    ///
    /// The return value is Lua's `res`: true when the body ran (up to its next yield or to its
    /// end), false when the coroutine was already dead ("cannot resume dead coroutine"). After a
    /// panic the coroutine is dead.
    pub fn resume(&mut self, cx: &mut Cx) -> bool {
        if self.fut.is_none() {
            return false;
        }
        let waker = Waker::from(Arc::new(NoopWake));
        let mut ctx = Context::from_waker(&waker);
        // While the body runs `coroutine.running()` is truthy; on unwind the coroutine is dead.
        struct Running<'a> {
            fut: &'a mut Option<Body>,
            ok: bool,
        }
        impl Drop for Running<'_> {
            fn drop(&mut self) {
                RUNNING.with(|r| r.set(r.get() - 1));
                if !self.ok {
                    *self.fut = None;
                }
            }
        }
        RUNNING.with(|r| r.set(r.get() + 1));
        let mut run = Running {
            fut: &mut self.fut,
            ok: false,
        };
        let poll = {
            let body = &mut run.fut.as_mut().expect("checked above").0;
            // A body may resume another coroutine from inside `with_cx` (the slot is empty there).
            CX.set(cx, || body.as_mut().poll(&mut ctx))
        };
        run.ok = true;
        if poll.is_ready() {
            *run.fut = None;
        }
        true
    }
    /// `local res, errMsg = coroutine.resume(co)`: like [`Coroutine::resume`], but a Lua error
    /// raised inside the body is returned as `(false, Some(msg))` and leaves the coroutine dead.
    /// A resume of a dead coroutine is `(false, Some("cannot resume dead coroutine"))`. Panics that
    /// are not Lua errors keep unwinding. Desktop only (wasm32 is panic=abort).
    /// CCR-0023 (U4 addition).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn try_resume(&mut self, cx: &mut Cx) -> (bool, Option<String>) {
        if self.is_dead() {
            return (false, Some("cannot resume dead coroutine".to_owned()));
        }
        match pob_lua::error::catch(|| self.resume(cx)) {
            Ok(res) => (res, None),
            Err(e) => (false, Some(e.0)),
        }
    }
    /// coroutine.status(co) == "dead"
    pub fn is_dead(&self) -> bool {
        self.fut.is_none()
    }
}

pub mod co {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    /// Access Cx inside a coroutine body, between yields. Nested use panics (re-entrancy guard).
    pub fn with_cx<R>(f: impl FnOnce(&mut super::Cx) -> R) -> R {
        if !super::CX.is_set() {
            crate::lua_error!(
                "co::with_cx outside a coroutine resume, or nested inside another with_cx"
            );
        }
        super::CX.with(f)
    }
    /// coroutine.yield(): Pending exactly once.
    pub async fn yield_now() {
        YieldNow(false).await
    }
    /// `coroutine.running()` used as a condition (CalcsTab.lua:514, Common.lua:646): true while a
    /// coroutine body is being polled. CCR-0023 (U4 addition).
    pub fn running() -> bool {
        super::RUNNING.with(std::cell::Cell::get) > 0
    }
    /// SG frame pacing over `coroutine._list()` (Common.lua:55-57): some coroutine is not dead.
    /// CCR-0023 (U4 addition).
    pub fn any_alive() -> bool {
        super::ALIVE.with(std::cell::Cell::get) > 0
    }

    struct YieldNow(bool);
    impl Future for YieldNow {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
            if self.0 {
                Poll::Ready(())
            } else {
                self.0 = true;
                Poll::Pending
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Coroutine resume/yield/dead with `with_cx` (ui doc §12.3 U4), mirroring the CalcsTab.lua:475-495
    //! BuildPower driver.
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    #[test]
    fn resume_yield_dead_with_cx() {
        let mut cx = Cx::headless();
        let log = Rc::new(RefCell::new(Vec::<String>::new()));
        let l = log.clone();
        assert!(!co::any_alive());
        let mut c = Coroutine::create(async move {
            l.borrow_mut().push("start".into());
            for i in 0..3 {
                co::with_cx(|cx| {
                    cx.main.screen_w += 1.0;
                    assert!(co::running());
                });
                assert!(co::running());
                l.borrow_mut().push(format!("step {i}"));
                co::yield_now().await;
            }
            l.borrow_mut().push("end".into());
        });
        // Nothing runs before the first resume (coroutine.create does not start the body).
        assert!(log.borrow().is_empty());
        assert!(co::any_alive());
        assert!(!co::running());
        let mut resumes = 0;
        while !c.is_dead() {
            assert!(c.resume(&mut cx));
            resumes += 1;
            assert_eq!(cx.main.screen_w, f64::from(resumes.min(3)));
        }
        assert_eq!(resumes, 4);
        assert_eq!(
            *log.borrow(),
            vec!["start", "step 0", "step 1", "step 2", "end"]
        );
        assert!(!co::any_alive());
        // Lua: coroutine.resume on a dead coroutine returns false.
        assert!(!c.resume(&mut cx));
        assert_eq!(
            c.try_resume(&mut cx),
            (false, Some("cannot resume dead coroutine".to_owned()))
        );
        assert!(Coroutine::default().is_dead());
    }

    #[test]
    fn errors_inside_kill_the_coroutine() {
        let mut cx = Cx::headless();
        let mut c = Coroutine::create(async {
            co::yield_now().await;
            crate::lua_error!("boom {}", 1);
        });
        assert_eq!(c.try_resume(&mut cx), (true, None));
        assert_eq!(c.try_resume(&mut cx), (false, Some("boom 1".to_owned())));
        assert!(c.is_dead());
        assert!(!co::any_alive());
        // The plain resume propagates the Lua error and also leaves the coroutine dead.
        let mut c = Coroutine::create(async {
            crate::lua_error!("bang");
        });
        let r = pob_lua::error::catch(|| c.resume(&mut cx));
        assert_eq!(r.unwrap_err().0, "bang");
        assert!(c.is_dead());
    }

    #[test]
    fn with_cx_guards() {
        let mut cx = Cx::headless();
        // Outside a resume.
        assert!(pob_lua::error::catch(|| co::with_cx(|_| ())).is_err());
        // Nested inside another with_cx.
        let mut c = Coroutine::create(async {
            co::with_cx(|_| co::with_cx(|_| ()));
        });
        assert!(pob_lua::error::catch(|| c.resume(&mut cx)).is_err());
        assert!(c.is_dead());
        // Dropping an alive coroutine unregisters it.
        let c = Coroutine::create(async { co::yield_now().await });
        assert!(co::any_alive());
        drop(c);
        assert!(!co::any_alive());
    }

    #[test]
    fn lent_from_widget_state() {
        use crate::layout::{rect, Anchor, Base};
        use crate::widget::{Caps, New, Widget};
        #[derive(Default)]
        struct Tab {
            power_builder: Coroutine,
            power_build_flag: bool,
            done: u32,
        }
        impl Widget for Tab {
            const NAME: &'static str = "CalcsTab";
            const CAPS: Caps = Caps::empty();
        }
        let mut cx = Cx::headless();
        let t = cx.add_free(
            None,
            New::new(
                Base::new(Anchor::NONE, rect(0, 0, 0, 0)),
                Tab {
                    power_build_flag: true,
                    ..Tab::default()
                },
            ),
        );
        let me = t.id();
        // lua: CalcsTab.lua:475-495 BuildPower, once per frame
        let build_power = |cx: &mut Cx| {
            if cx.w::<Tab>(me).power_build_flag {
                cx.w_mut::<Tab>(me).power_build_flag = false;
                cx.w_mut::<Tab>(me).power_builder = Coroutine::create(async move {
                    for _ in 0..2 {
                        co::with_cx(|cx| {
                            // the slot reads as a dead coroutine while it runs (§6.13)
                            assert!(cx.w::<Tab>(me).power_builder.is_dead());
                            cx.w_mut::<Tab>(me).done += 1;
                        });
                        co::yield_now().await;
                    }
                });
            }
            if !cx.w::<Tab>(me).power_builder.is_dead() {
                cx.lend(
                    me,
                    |w: &mut Tab| &mut w.power_builder,
                    |cx, co| co.resume(cx),
                );
                // the dead coroutine is simply left in the slot (`self.powerBuilder = nil`)
            }
        };
        for _ in 0..5 {
            build_power(&mut cx);
        }
        assert_eq!(cx.get(t).done, 2);
        assert!(cx.get(t).power_builder.is_dead());
    }
}
