//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/Control.lua:151-178 (tab groups:
//! AddToTabGroup, TabAdvance). Ui doc §6.6 "Tab groups". Owner: U4. UI-U0 stub.
//!
//! `add_to_tab_group(master)` creates `[master, self]` the first time; `tab_advance` wraps around
//! and accepts a member that is `self`, or that has `KEY_DOWN` and is shown.

use crate::cx::Cx;
use crate::ids::CtrlId;

impl Cx {
    /// lua: Control.lua:151-158 AddToTabGroup
    pub fn add_to_tab_group(&mut self, id: impl Into<CtrlId>, master: impl Into<CtrlId>) {
        let _ = (id.into(), master.into());
        todo!("U4: Control.lua:151-158 AddToTabGroup")
    }

    /// lua: Control.lua:160-178 TabAdvance
    pub fn tab_advance(&self, id: CtrlId, step: i32) -> CtrlId {
        let _ = (id, step);
        todo!("U4: Control.lua:160-178 TabAdvance")
    }
}
