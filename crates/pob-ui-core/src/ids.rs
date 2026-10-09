//! Ids, keys, viewport (ui doc §6.1). Complete in UI-U0; owned by U4 afterwards.

slotmap::new_key_type! {
    /// A control or control-like node (tab, mode, popup, anchor helper). Generation-checked.
    pub struct CtrlId;
    pub struct TabGroupId;
}

/// Typed handle: what a Lua field like `self.controls.edit` holds when the class is known. Copy; never owns.
pub struct Ctrl<W>(pub CtrlId, std::marker::PhantomData<fn() -> W>);

impl<W> Ctrl<W> {
    pub const fn from_raw(id: CtrlId) -> Self {
        Ctrl(id, std::marker::PhantomData)
    }
    pub const fn id(self) -> CtrlId {
        self.0
    }
    /// placeholder until wired; use panics
    pub fn null() -> Self {
        Ctrl(CtrlId::default(), std::marker::PhantomData)
    }
}
impl<W> Clone for Ctrl<W> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<W> Copy for Ctrl<W> {}
impl<W> PartialEq for Ctrl<W> {
    fn eq(&self, o: &Self) -> bool {
        self.0 == o.0
    }
}
impl<W> Eq for Ctrl<W> {}
impl<W> std::fmt::Debug for Ctrl<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Ctrl({:?})", self.0)
    }
}
impl<W> From<Ctrl<W>> for CtrlId {
    fn from(c: Ctrl<W>) -> CtrlId {
        c.0
    }
}

/// A key in a Lua `controls` table.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CtrlKey {
    Idx(i64),
    Name(std::borrow::Cow<'static, str>),
}
impl From<&'static str> for CtrlKey {
    fn from(s: &'static str) -> Self {
        CtrlKey::Name(s.into())
    }
}
impl From<String> for CtrlKey {
    fn from(s: String) -> Self {
        CtrlKey::Name(s.into())
    }
}
impl From<i64> for CtrlKey {
    fn from(i: i64) -> Self {
        CtrlKey::Idx(i)
    }
}

/// Lua viewPort table {x, y, width, height}.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ViewPort {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
