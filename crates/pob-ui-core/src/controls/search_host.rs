//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/SearchHost.lua 1-153
//! (a mixin class: its state is a field of the host widget). Owner: U13. UI-U0 stub.

use std::rc::Rc;

use pob_sg::Key;

use crate::prelude::*;

/// `listAccessor()` / `valueAccessor(value)` (SearchHost.lua:10).
pub type ListAccessor = Rc<dyn Fn(&Cx, CtrlId) -> Vec<String>>;

#[derive(Default)]
pub struct SearchHost {
    pub list_accessor: Option<ListAccessor>,
    pub ignore_order: bool,
    pub search_term: String,
    pub match_count: usize,
    pub last_search_time: Option<i64>,
}

impl SearchHost {
    /// lua: SearchHost.lua:10-92 SearchHost(listAccessor, valueAccessor, ignoreOrder)
    pub fn new(list_accessor: ListAccessor, ignore_order: bool) -> SearchHost {
        let _ = (list_accessor, ignore_order);
        todo!("U13: SearchHost.lua:10-92")
    }
    /// lua: SearchHost.lua:94-96
    pub fn is_search_active(&self) -> bool {
        todo!("U13: SearchHost:IsSearchActive")
    }
    /// lua: SearchHost.lua:98-110
    pub fn on_search_char(
        cx: &mut Cx,
        me: CtrlId,
        get: fn(&mut Cx, CtrlId) -> &mut SearchHost,
        ch: Key,
    ) -> bool {
        let _ = (cx, me, get, ch);
        todo!("U13: SearchHost:OnSearchChar")
    }
    /// lua: SearchHost.lua:112-121
    pub fn on_search_key_down(
        cx: &mut Cx,
        me: CtrlId,
        get: fn(&mut Cx, CtrlId) -> &mut SearchHost,
        key: Key,
    ) -> bool {
        let _ = (cx, me, get, key);
        todo!("U13: SearchHost:OnSearchKeyDown")
    }
    /// lua: SearchHost.lua:133-135
    pub fn get_match_count(&self) -> usize {
        todo!("U13: SearchHost:GetMatchCount")
    }
    /// lua: SearchHost.lua:144-148
    pub fn reset_search(&mut self) {
        todo!("U13: SearchHost:ResetSearch")
    }
    /// lua: SearchHost.lua:150-153
    pub fn get_search_term_pretty(&self) -> String {
        todo!("U13: SearchHost:GetSearchTermPretty")
    }
}
