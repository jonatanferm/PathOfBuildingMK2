//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/Control.lua:151-178 (tab groups:
//! AddToTabGroup, TabAdvance) and Modules/Common.lua:569-575 (isValueInArray). Ui doc §6.6 "Tab
//! groups". Owner: U4.
//!
//! @pob-file Classes/Control.lua @16de4b82d
//!
//! A Lua `tabOrder` table (shared by every member) is `ui.tab_groups[g]`; `self.tabOrder` is
//! `Base::tab_order`. The arrays never have holes (`t_insert` appends), so `#tabOrder` is the
//! vector length.

use crate::cx::Cx;
use crate::ids::CtrlId;
use crate::widget::Caps;

impl Cx {
    /// lua: Control.lua:151-158 AddToTabGroup
    /// @pob Classes/Control.lua:151-158 h=f57bb67b
    pub fn add_to_tab_group(&mut self, id: impl Into<CtrlId>, master: impl Into<CtrlId>) {
        let (id, master) = (id.into(), master.into());
        let g = if let Some(g) = self.base(master).tab_order {
            // t_insert(master.tabOrder, self)
            self.ui.tab_groups[g].push(id);
            g
        } else {
            // master.tabOrder = { master, self }
            let g = self.ui.tab_groups.insert(vec![master, id]);
            self.base_mut(master).tab_order = Some(g);
            g
        };
        // self.tabOrder = master.tabOrder
        self.base_mut(id).tab_order = Some(g);
    }

    /// lua: Control.lua:160-178 TabAdvance
    /// @pob Classes/Control.lua:160-178 h=618513c4
    pub fn tab_advance(&self, id: CtrlId, step: i32) -> CtrlId {
        if let Some(g) = self.base(id).tab_order {
            let tab_order = &self.ui.tab_groups[g];
            let n = tab_order.len() as i64;
            if let Some(index) = is_value_in_array(tab_order, id) {
                let mut index = index as i64;
                loop {
                    index += i64::from(step);
                    if index > n {
                        index = 1;
                    } else if index < 1 {
                        index = n;
                    }
                    let c = tab_order[(index - 1) as usize];
                    if c == id || (self.has(c, Caps::KEY_DOWN) && self.is_shown(c)) {
                        return c;
                    }
                }
            }
        }
        id
    }
}

/// lua: Common.lua:569-575 isValueInArray (1-based index of the first `== val`, by identity)
/// @pob Modules/Common.lua:569-575 h=b0314d73
fn is_value_in_array(tbl: &[CtrlId], val: CtrlId) -> Option<usize> {
    for (i, &v) in tbl.iter().enumerate() {
        if val == v {
            return Some(i + 1);
        }
    }
    None
}
