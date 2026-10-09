//! `Prop<T>`: a Lua field that may hold a value or a function (ui doc §6.2, D4). Complete in
//! UI-U0; owned by U4 afterwards.
//!
//! lua: Control.lua `ControlClass:GetProperty(name)` (a function-valued field is called with
//! `self`, anything else is returned as is).

use std::rc::Rc;

use crate::{cx::Cx, ids::CtrlId};

pub type PropFn<T> = Rc<dyn Fn(&Cx, CtrlId) -> T>;

/// A Lua field that may hold a value or a function (ControlClass:GetProperty).
pub enum Prop<T: 'static> {
    Val(T),
    Fn(PropFn<T>),
}

impl<T: Clone + 'static> Prop<T> {
    /// GetProperty: `me` is the control that owns the property (Lua passes `self`).
    #[inline]
    pub fn get(&self, cx: &Cx, me: CtrlId) -> T {
        match self {
            Prop::Val(v) => v.clone(),
            Prop::Fn(f) => f(cx, me),
        }
    }
    pub fn f(f: impl Fn(&Cx, CtrlId) -> T + 'static) -> Self {
        Prop::Fn(Rc::new(f))
    }
    pub fn is_fn(&self) -> bool {
        matches!(self, Prop::Fn(_))
    }
    /// Lua RAW field read (`ctrl.width` without GetProperty) where the field holds a value.
    /// A function there would be a Lua arithmetic/concat error: lua_error!.
    #[track_caller]
    pub fn raw(&self) -> T {
        match self {
            Prop::Val(v) => v.clone(),
            Prop::Fn(_) => crate::lua_error!("raw read of a function-valued property"),
        }
    }
}
impl<T: Clone> Clone for Prop<T> {
    fn clone(&self) -> Self {
        match self {
            Prop::Val(v) => Prop::Val(v.clone()),
            Prop::Fn(f) => Prop::Fn(f.clone()),
        }
    }
}
impl From<f64> for Prop<f64> {
    fn from(v: f64) -> Self {
        Prop::Val(v)
    }
}
impl From<i32> for Prop<f64> {
    fn from(v: i32) -> Self {
        Prop::Val(v as f64)
    }
}
impl From<bool> for Prop<bool> {
    fn from(v: bool) -> Self {
        Prop::Val(v)
    }
}
impl From<&str> for Prop<String> {
    fn from(v: &str) -> Self {
        Prop::Val(v.to_owned())
    }
}
impl From<String> for Prop<String> {
    fn from(v: String) -> Self {
        Prop::Val(v)
    }
}
impl From<&str> for Prop<Option<String>> {
    fn from(v: &str) -> Self {
        Prop::Val(Some(v.to_owned()))
    }
}
impl From<Option<String>> for Prop<Option<String>> {
    fn from(v: Option<String>) -> Self {
        Prop::Val(v)
    }
}
// Closures are wrapped explicitly with Prop::f(..). There is no blanket From<F>, to keep coherence and readability.
