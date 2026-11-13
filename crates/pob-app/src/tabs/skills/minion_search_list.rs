//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/MinionSearchListControl.lua 1-71
//! (`newClass("MinionSearchListControl", "MinionListControl")`: the Spectre Library's source
//! list with a search field). Owner: U31.
//!
//! @pob-file Classes/MinionSearchListControl.lua @16de4b82d
//!
//! The struct embeds [`MinionListControl`] (which embeds `ListControl`); the subclass overrides
//! none of the base's ListControl hooks, so it uses the same hook table. Its only constructor is
//! Build.lua:1537 (`OpenSpectreLibrary`), through `ShellHooks::minion_search_list_control`
//! ([`spectre_library_source`]).
//!
//! Notes:
//! - `self.unfilteredList = copyTable(list)` and `self.list = self.unfilteredList` (L16, L69): the
//!   Lua list then aliases the copy. Nothing writes the list of this control while it aliases
//!   (`isMutable` is false, L17; `OnSelDelete` does nothing with a `dest`; no list has this one as
//!   a drag target), so the port stores a copy of the unfiltered ids and assigns a new
//!   `ListData::Local` built from it.
//! - L31 (`dest` nil) indexes `self.controls.add`, which only exists with a `dest`: the Lua
//!   error is kept (no PoB caller passes a nil `dest`).
//! - ORDER: L61 `pairs(self.unfilteredList)` walks a sequence (the array part, ascending).
//! - DIVERGENCE(wasm-pcall): as in ItemDBControl (CCR-0107 §3), `PCall(string.matchOrPattern,
//!   ..)` cannot catch on wasm32 (panic=abort).
//!
//! Props with side effects (ui doc §7.3): none.
#![allow(non_snake_case)]

use std::rc::Rc;

use pob_lua::pattern;
use pob_ui_core::controls::list::{ListControl, ListData, ListValue};
use pob_ui_core::controls::{DropDownControl, DropItem, EditArgs, EditControl};
use pob_lua::string::matchOrPattern;
use pob_ui_core::prelude::*;

use super::minion_list::{name_value, value_name, MinionListControl, HOOKS};
use crate::model::{self, Name};

// @pob-skip Classes/MinionSearchListControl.lua:1-13 reason="header comment, the table/string locals and newClass (the struct and Widget impl below)"

/// `local err, match = PCall(string.matchOrPattern, s, p)`; `not err and match`.
#[cfg(not(target_arch = "wasm32"))]
fn pcall_match(s: &str, p: &str) -> bool {
    pob_lua::error::catch(|| matchOrPattern(s, p)).unwrap_or(false)
}

/// DIVERGENCE(wasm-pcall): module notes.
#[cfg(target_arch = "wasm32")]
fn pcall_match(s: &str, p: &str) -> bool {
    matchOrPattern(s, p)
}

/// `MinionSearchListControl`: a [`MinionListControl`] with a search field and a filter mode.
pub struct MinionSearchListControl {
    pub minion: MinionListControl,
    /// `self.unfilteredList` (module notes)
    pub unfiltered_list: Vec<Name>,
}

impl MinionSearchListControl {
    /// lua: MinionSearchListControl.lua:14-35 `MinionSearchListClass:MinionSearchListControl(
    /// anchor, rect, data, list, dest)` (`data` is the global data)
    /// @pob Classes/MinionSearchListControl.lua:14-35 h=3d55a553
    pub fn new(
        anchor: Anchor,
        r: [Prop<f64>; 4],
        list: Vec<Name>,
        dest: Option<CtrlId>,
    ) -> New<MinionSearchListControl> {
        // L15 (the fields; the base constructor's children run as the first inits)
        let values: Vec<ListValue> = list.iter().copied().map(name_value).collect();
        let mut minion = MinionListControl::state(&HOOKS, ListData::Local(values), dest);
        // L17
        minion.list.is_mutable = false;
        New::new(
            Base::new(anchor, r),
            MinionSearchListControl {
                minion,
                // L16
                unfiltered_list: list,
            },
        )
        .host()
        .init(|cx, me| ListControl::init(cx, me.0))
        .init(|cx, me| MinionListControl::ctor_tail(cx, me.0))
        .init(move |cx, me| {
            let me = me.0;
            // L19-21
            cx.add(
                me,
                "searchText",
                EditControl::new(
                    Anchor::new(BottomLeft, me, TopLeft),
                    rect(0, -2, 203, 18),
                    EditArgs {
                        init: Some(String::new()),
                        prompt: Some("Search".to_owned()),
                        filter: Some("%c".to_owned()),
                        limit: Some(100),
                        change_func: Some(Rc::new(move |cx, buf| {
                            // L20
                            let dd = cx.child_as::<DropDownControl>(me, "searchModeDropDown");
                            let mode = cx.get(dd).sel_index;
                            Self::ListFilterChanged(cx, me, buf, mode);
                        })),
                        clearable: true,
                        ..EditArgs::default()
                    },
                ),
            );
            let search_text = cx.child_as::<EditControl>(me, "searchText");
            // L23-25
            cx.add(
                me,
                "searchModeDropDown",
                DropDownControl::new(
                    Anchor::new(Left, search_text, Right),
                    rect(2, 0, 60, 18),
                    vec![DropItem::str("Names"), DropItem::str("Skills"), DropItem::str("Both")],
                    Some(Rc::new(move |cx, index, _value| {
                        // L24
                        let st = cx.child_as::<EditControl>(me, "searchText");
                        let buf = cx.get(st).buf.clone();
                        Self::ListFilterChanged(cx, me, &buf, index);
                    })),
                    None,
                    false,
                ),
            );
            // L27
            cx.w_mut::<ListControl>(me).label_position_offset = [0.0, -20.0];
            // L28
            if dest.is_some() {
                // L29
                let add = cx
                    .child(me, "add")
                    .unwrap_or_else(|| lua_error!("attempt to index field 'add' (a nil value)"));
                let y = cx.base(add).y.raw();
                cx.base_mut(add).y = Prop::Val(y - 20.0);
            } else {
                // L31 (`self.controls.add` is nil without a dest: module notes)
                lua_error!("attempt to index field 'add' (a nil value)");
            }
            // L34 return self
        })
    }

    /// lua: MinionSearchListControl.lua:37-55 `MinionSearchListClass:DoesEntryMatchFilters(
    /// searchStr, minionId, filterMode)`
    /// @pob Classes/MinionSearchListControl.lua:37-55 h=f9f9b848
    pub fn DoesEntryMatchFilters(searchStr: &str, minionId: Name, filterMode: usize) -> bool {
        let data = model::data();
        // L38
        if filterMode == 1 || filterMode == 3 {
            // L39
            let minion = data
                .minions
                .get(&minionId)
                .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
            let name = pob_lua::string::lower(minion.name.as_str());
            // L40
            if pcall_match(&name, searchStr) {
                // L41
                return true;
            }
        }
        // L44
        if filterMode == 2 || filterMode == 3 {
            // L45
            let minion = data
                .minions
                .get(&minionId)
                .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
            for skillId in &minion.skillList {
                // L46
                if let Some(skill) = data.skills.get(skillId) {
                    // L47
                    let name = pob_lua::string::lower(skill.name.as_str());
                    // L48
                    if pcall_match(&name, searchStr) {
                        // L49
                        return true;
                    }
                }
            }
        }
        // L54
        false
    }

    /// lua: MinionSearchListControl.lua:57-71 `MinionSearchListClass:ListFilterChanged(buf,
    /// filterMode)`
    /// @pob Classes/MinionSearchListControl.lua:57-71 h=42335b08
    pub fn ListFilterChanged(cx: &mut Cx, me: CtrlId, buf: &str, filterMode: usize) {
        // L58
        let searchStr = pattern::gsub(
            &pob_lua::string::lower(buf),
            "[%-%.%+%[%]%$%^%%%?%*]",
            "%%%0",
            None,
        )
        .0;
        // L59
        if pattern::r#match(&searchStr, "%S", None).is_some() {
            // L60
            let mut filteredList: Vec<ListValue> = Vec::new();
            // L61 (ORDER: module notes)
            let unfiltered = cx.w::<MinionSearchListControl>(me).unfiltered_list.clone();
            for minionId in unfiltered {
                // L62
                if Self::DoesEntryMatchFilters(&searchStr, minionId, filterMode) {
                    // L63
                    filteredList.push(name_value(minionId));
                }
            }
            // L66
            cx.w_mut::<ListControl>(me).list = ListData::Local(filteredList);
            // L67
            ListControl::select_index(cx, me, 1);
        } else {
            // L69 (module notes)
            let values: Vec<ListValue> = cx
                .w::<MinionSearchListControl>(me)
                .unfiltered_list
                .iter()
                .copied()
                .map(name_value)
                .collect();
            cx.w_mut::<ListControl>(me).list = ListData::Local(values);
        }
    }

    /// The ids currently listed (tests).
    pub fn listed(cx: &Cx, me: CtrlId) -> Vec<Name> {
        let n = ListControl::list_len(cx, me);
        (1..=n)
            .filter_map(|i| ListControl::list_get(cx, me, i))
            .map(|v| value_name(&v))
            .collect()
    }
}

impl Widget for MinionSearchListControl {
    const NAME: &'static str = "MinionSearchListControl";
    const CAPS: Caps = <MinionListControl as Widget>::CAPS;

    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        ListControl::draw(cx, me, vp, no_tooltip);
    }
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        ListControl::is_mouse_over(cx, me)
    }
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, double_click: bool) -> Focus {
        ListControl::on_key_down(cx, me, key, double_click)
    }
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        ListControl::on_key_up(cx, me, key)
    }
    fn duck(cx: &Cx, me: CtrlId, q: Duck) -> bool {
        ListControl::duck(cx, me, q)
    }
    fn can_receive_drag(cx: &Cx, me: CtrlId, ty: &str, v: &DragValue) -> bool {
        MinionListControl::CanReceiveDrag(cx, me, ty, v)
    }
    fn receive_drag(cx: &mut Cx, me: CtrlId, ty: &str, v: &DragValue, source: CtrlId) {
        MinionListControl::ReceiveDrag(cx, me, ty, v, source);
    }

    pob_ui_core::impl_parts!(MinionSearchListControl: minion => MinionListControl);
}

/// `controls.source = new("MinionSearchListControl"):MinionSearchListControl(nil, {139, 60,
/// 265, 230}, self.data, sourceList, controls.list)` (Build.lua:1537) in the Spectre Library
/// popup: the `ShellHooks::minion_search_list_control` entry (CCR-0104 §1).
pub fn spectre_library_source(cx: &mut Cx, popup: CtrlId, source_list: Vec<Name>, list: CtrlId) -> CtrlId {
    cx.add(
        popup,
        "source",
        MinionSearchListControl::new(Anchor::NONE, rect(139, 60, 265, 230), source_list, Some(list)),
    )
    .id()
}
