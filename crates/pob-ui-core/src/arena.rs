//! The control arena (ui doc §6.5, D3, D7, D8). Owner: U4.
//!
//! lua: Common.lua:158-230 (`new` creates the object table; the port stamps a creation sequence
//! number for the canonical child order, D7), ControlHost.lua:9-13 (`self.controls = {}`).
//!
//! Lua objects are tables kept alive by the garbage collector. Here every control (and every
//! control-like object: tabs, modes, popups, anchor helpers) is a [`Node`] in a generation-checked
//! slotmap. Freeing is deferred (D8): `Cx::bury` puts a node in the graveyard and
//! `Cx::end_frame_gc` drops it, together with everything it owns, after the frame. Using an id
//! after that panics with a Lua-style error that names the expected class (the equivalent of a
//! Lua nil-index error).
//!
//! DIVERGENCE(gc): Lua keeps an object alive while anything references it. The arena frees a
//! node when its owner frees it or when it is buried, even if a `selControl`, a tab group or a
//! widget field still holds its id; such a stale id panics on its next use (loudly, never
//! silently). The same holds for a second host: a node records one `parent`, so `end_frame_gc`
//! unlinks a freed node only from that host; if the node was also `put` into another host, that
//! host's `controls` keeps the dead id and its next `children()` panics. PoB keeps each control in
//! one `controls` table, so code that moves a control between hosts must `detach_child` it first.

use std::any::TypeId;
use std::collections::BTreeMap;

use crate::ids::{CtrlId, CtrlKey, TabGroupId};
use crate::layout::Base;
use crate::widget::{vtable_of, Overrides, VTable, Widget};

/// One control (or control-like object) in the arena.
pub struct Node {
    pub base: Base,
    /// ControlHost mixin
    pub host: Option<HostData>,
    pub ov: Overrides,
    pub(crate) vt: &'static VTable,
    /// the most-derived widget struct
    pub(crate) state: Box<dyn std::any::Any>,
    /// creation order (D7)
    pub(crate) seq: u64,
    /// host membership, for markers and removal
    pub(crate) parent: Option<(CtrlId, CtrlKey)>,
    /// freed with its owner
    pub(crate) owner: Option<CtrlId>,
    pub(crate) owned: Vec<CtrlId>,
    pub(crate) buried: bool,
}

/// A ControlHost's `controls` table and `selControl`.
pub struct HostData {
    pub(crate) map: BTreeMap<CtrlKey, CtrlId>,
    /// selControl (raw writes allowed: EditControl.lua:448 assigns it without SetFocus)
    pub sel_control: Option<CtrlId>,
}

impl HostData {
    /// ControlHost.lua:9-13 `self.controls = {}` (selControl starts nil).
    pub(crate) fn new() -> HostData {
        HostData {
            map: BTreeMap::new(),
            sel_control: None,
        }
    }
}

/// The arena.
pub struct Ui {
    pub(crate) nodes: slotmap::SlotMap<CtrlId, Node>,
    pub(crate) tab_groups: slotmap::SlotMap<TabGroupId, Vec<CtrlId>>,
    pub(crate) graveyard: Vec<CtrlId>,
    pub(crate) next_seq: u64,
}

impl Default for Ui {
    fn default() -> Self {
        Ui {
            nodes: slotmap::SlotMap::with_key(),
            tab_groups: slotmap::SlotMap::with_key(),
            graveyard: Vec::new(),
            next_seq: 0,
        }
    }
}

/// A Lua index of a dead object (D8): the id's node was freed at the end of an earlier frame.
#[track_caller]
fn dead(id: CtrlId, expected: &str) -> ! {
    crate::lua_error!(
        "attempt to index a dead control (expected {}) {:?}",
        expected,
        id
    )
}

/// A typed access to a control of another class (the Lua code would index a missing field).
#[track_caller]
fn wrong_type(id: CtrlId, expected: &str, actual: &str) -> ! {
    crate::lua_error!(
        "control {:?} is a {}, not a {} (attempt to index a nil value)",
        id,
        actual,
        expected
    )
}

impl Ui {
    // Field-level accessors, usable while another field of Cx (e.g. cx.sg) is borrowed mutably.

    /// via vtable `part`; panics: dead id / wrong type (names W::NAME)
    #[track_caller]
    pub fn w<W: Widget>(&self, id: impl Into<CtrlId>) -> &W {
        let id = id.into();
        let Some(n) = self.nodes.get(id) else {
            dead(id, W::NAME)
        };
        match (n.vt.part)(&*n.state, TypeId::of::<W>()).and_then(|a| a.downcast_ref::<W>()) {
            Some(w) => w,
            None => wrong_type(id, W::NAME, n.vt.name),
        }
    }
    #[track_caller]
    pub fn w_mut<W: Widget>(&mut self, id: impl Into<CtrlId>) -> &mut W {
        let id = id.into();
        let Some(n) = self.nodes.get_mut(id) else {
            dead(id, W::NAME)
        };
        let name = n.vt.name;
        match (n.vt.part_mut)(&mut *n.state, TypeId::of::<W>()).and_then(|a| a.downcast_mut::<W>())
        {
            Some(w) => w,
            None => wrong_type(id, W::NAME, name),
        }
    }
    pub fn try_w<W: Widget>(&self, id: CtrlId) -> Option<&W> {
        let n = self.nodes.get(id)?;
        (n.vt.part)(&*n.state, TypeId::of::<W>()).and_then(|a| a.downcast_ref::<W>())
    }
    /// Lua `ctrl.ListControl` is-a check
    pub fn is<W: Widget>(&self, id: CtrlId) -> bool {
        self.try_w::<W>(id).is_some()
    }
    #[track_caller]
    pub fn base(&self, id: impl Into<CtrlId>) -> &Base {
        &self.node(id.into()).base
    }
    #[track_caller]
    pub fn base_mut(&mut self, id: impl Into<CtrlId>) -> &mut Base {
        &mut self.node_mut(id.into()).base
    }
    pub fn is_alive(&self, id: CtrlId) -> bool {
        self.nodes.contains_key(id)
    }
    #[track_caller]
    pub fn class_name(&self, id: CtrlId) -> &'static str {
        self.node(id).vt.name
    }
    #[track_caller]
    pub fn node(&self, id: CtrlId) -> &Node {
        match self.nodes.get(id) {
            Some(n) => n,
            None => dead(id, "control"),
        }
    }
    #[track_caller]
    pub fn node_mut(&mut self, id: CtrlId) -> &mut Node {
        match self.nodes.get_mut(id) {
            Some(n) => n,
            None => dead(id, "control"),
        }
    }

    /// Common.lua:158-230 `new(className)` + the constructor's state: a fresh node with the next
    /// creation seq (D7). `owner` frees it with itself (D8).
    pub(crate) fn insert_node<W: Widget>(
        &mut self,
        base: Base,
        w: W,
        is_host: bool,
        owner: Option<CtrlId>,
    ) -> CtrlId {
        let seq = self.next_seq;
        self.next_seq += 1;
        let id = self.nodes.insert(Node {
            base,
            host: is_host.then(HostData::new),
            ov: Overrides::default(),
            vt: vtable_of::<W>(),
            state: Box::new(w),
            seq,
            parent: None,
            owner,
            owned: Vec::new(),
            buried: false,
        });
        if let Some(o) = owner {
            self.node_mut(o).owned.push(id);
        }
        id
    }
}
