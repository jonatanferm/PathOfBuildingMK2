//! The control arena (ui doc §6.5, D3, D7, D8). Owner: U4. UI-U0 stub: final signatures.
//!
//! lua: Common.lua:158-230 (`new` creates the object table; the port stamps a creation sequence
//! number for the canonical child order, D7), ControlHost.lua:9-13 (`self.controls = {}`).

#![allow(dead_code)] // UI-U0 stub: the private fields are U4's bodies' state.

use std::collections::BTreeMap;

use crate::ids::{CtrlId, CtrlKey, TabGroupId};
use crate::layout::Base;
use crate::widget::{Overrides, VTable, Widget};

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

impl Ui {
    // Field-level accessors, usable while another field of Cx (e.g. cx.sg) is borrowed mutably.

    /// via vtable `part`; panics: dead id / wrong type (names W::NAME)
    #[track_caller]
    pub fn w<W: Widget>(&self, id: impl Into<CtrlId>) -> &W {
        let _ = id.into();
        todo!("U4: Ui::w")
    }
    #[track_caller]
    pub fn w_mut<W: Widget>(&mut self, id: impl Into<CtrlId>) -> &mut W {
        let _ = id.into();
        todo!("U4: Ui::w_mut")
    }
    pub fn try_w<W: Widget>(&self, id: CtrlId) -> Option<&W> {
        let _ = id;
        todo!("U4: Ui::try_w")
    }
    /// Lua `ctrl.ListControl` is-a check
    pub fn is<W: Widget>(&self, id: CtrlId) -> bool {
        let _ = id;
        todo!("U4: Ui::is")
    }
    #[track_caller]
    pub fn base(&self, id: impl Into<CtrlId>) -> &Base {
        let _ = id.into();
        todo!("U4: Ui::base")
    }
    #[track_caller]
    pub fn base_mut(&mut self, id: impl Into<CtrlId>) -> &mut Base {
        let _ = id.into();
        todo!("U4: Ui::base_mut")
    }
    pub fn is_alive(&self, id: CtrlId) -> bool {
        let _ = id;
        todo!("U4: Ui::is_alive")
    }
    pub fn class_name(&self, id: CtrlId) -> &'static str {
        let _ = id;
        todo!("U4: Ui::class_name")
    }
    #[track_caller]
    pub fn node(&self, id: CtrlId) -> &Node {
        let _ = id;
        todo!("U4: Ui::node")
    }
    #[track_caller]
    pub fn node_mut(&mut self, id: CtrlId) -> &mut Node {
        let _ = id;
        todo!("U4: Ui::node_mut")
    }
}
