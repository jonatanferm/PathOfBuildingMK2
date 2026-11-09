//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/DropDownControl.lua 1-573
//! (`newClass("DropDownControl", "Control", "ControlHost", "TooltipHost", "SearchHost")`). Owner: U13.
//!
//! @pob-file Classes/DropDownControl.lua @16de4b82d
//!
//! # Hook set (frozen by U13 at the wave-2 gate, CCR-0086 A)
//!
//! Grepped over every DropDownControl use in `src/` (Export/ excluded): 157 `new("DropDownControl")`
//! sites in 21 files, two subclasses (`ItemSlotControl`, `TimelessJewelSocketControl`), and every
//! method or field assigned on a dropdown instance. The methods DropDownControl.lua calls on
//! `self` (`DropIndexToListIndex`, `ListIndexToDropIndex`, `GetDropCount`, `DrawSearchHighlights`,
//! `SetSel`, `ScrollSelIntoView`, `IsMouseOver`, `SetList`, `CheckDroppedWidth`, and the
//! SearchHost/TooltipHost/ControlHost/Control methods) are overridden by **no** subclass and **no**
//! instance, so the class-hook table is empty: `DropDownHooks` and the `hooks` field of the UI-U0
//! stub are gone. What the subclasses and instances do override are framework methods:
//!
//! - `ItemSlotControl` (ItemSlotControl.lua:125-185): `CanReceiveDrag`, `ReceiveDrag`, `Draw`,
//!   `OnKeyDown`, `OnHoverKeyUp`: its own `Widget` impl (the `Caps` it adds), calling the
//!   functions here for the super calls (`self.DropDownControl:Draw(viewPort)` =
//!   [`DropDownControl::draw`], `:OnKeyDown(key)` = [`DropDownControl::on_key_down`],
//!   `:GetHoverIndex()` = [`DropDownControl::get_hover_index`]).
//! - `TimelessJewelSocketControl` (TimelessJewelSocketControl.lua:25-58): `Draw` (same pattern).
//! - `build.controls.mainSkillMinion` (Build.lua:541-551): `CanReceiveDrag`/`ReceiveDrag` instance
//!   methods = `Overrides::{can_receive_drag, receive_drag}` (presence = `Caps::CAN_RECEIVE_DRAG`,
//!   ListControl.lua:164), and `tooltipFunc`.
//! - Instance fields read by this class: `tooltipFunc`/`tooltipText` (TooltipHost, 19 + 11 sites),
//!   `borderFunc` (ConfigTab.lua:697: [`DropDownControl::border_func`]), `otherDragSource`
//!   (written by a dragging ListControl: read through `list::other_drag_source`, CCR-0078 B),
//!   and the plain fields `selIndex`, `list`, `selFunc`, `dropped` (PassiveTreeView.lua:526),
//!   `enableDroppedWidth`, `enableChangeBoxWidth`, `maxDroppedWidth`.
//!
//! A subclass embeds `DropDownControl` as a field (`impl_parts!`), builds it with
//! [`DropDownControl::state`] and chains `.init(|cx, me| DropDownControl::init(cx, me.0))`.
//!
//! # Notes
//!
//! - `list[i]` is a [`DropItem`]; a Lua string entry has `label` = the string, no `detail` or
//!   `search_filter`, and `value` = `Rc<String>`. `DropItem::detail` is new (CCR-0086 B):
//!   L319/L393 read `listVal.detail` (ImportTab.lua:1071-1079 sets it).
//! - `selFunc(index, value)` gets `Option<&DropItem>` (CCR-0086 B): `self.list[newSel]` is nil
//!   when the list shrank under `selIndex` (ImportTab.lua:102-103 `SetList({})`) and the clamped
//!   `SetSel` lands on index 1 of an empty list.
//! - `SetSel` takes the Lua number ([`DropDownControl::set_sel_num`]: the class itself passes
//!   `ListIndexToDropIndex(..) - 1`, which can be 0); [`DropDownControl::set_sel`] is the
//!   `usize` convenience. `DropIndexToListIndex`/`ListIndexToDropIndex` return `Option<f64>`
//!   (nil); arithmetic on their nil (`ListIndexToDropIndex(selIndex, 1) - 2` for a `selIndex`
//!   out of range while a search is active) raises the Lua error.
//! - `IsMouseOver` returns two values: [`DropDownControl::is_mouse_over_comp`]; the `Widget` method
//!   returns the first. `self:IsMouseOver()` inside the class is that static call (no override
//!   exists, see above).
//! - `self.selControl` is the ControlHost field (`HostData::sel_control`), written raw as the Lua
//!   does (L445, L448, L475).
//! - `self.hoverSelDrop` is `mOver and ... and n` (false or a number): `Option<f64>`.
//! - DIVERGENCE(alias): `SetList` wipes the previous list table (`wipeTable(self.list)`, L530),
//!   which a caller that still references that table observes (TradeQuery.lua:102/:472 pass the
//!   same `leagueDropList` twice: the second call empties it). The Rust list is owned by the
//!   control, so a caller that aliases its list must reproduce that itself.
//! - DIVERGENCE(nil-label): a Lua table entry without `label` errors in `DrawString` (L397) and in
//!   the search value accessor (`StripEscapes(table)`, L33); `DropItem::label` always exists.
#![allow(clippy::many_single_char_names, clippy::too_many_lines)] // Lua names (x, y, ..), Draw is one Lua function

use std::any::Any;
use std::rc::Rc;

use pob_lua::math::{m_floor, m_max, m_min};
use pob_lua::string::StripEscapes;
use pob_sg::{Align, Font, Key};

use crate::controls::list::other_drag_source;
use crate::controls::scroll_bar::ScrollBarControl;
use crate::controls::search_host::{SearchHost, SearchInfo};
use crate::draw::{draw_arrow, ArrowDir};
use crate::luastr;
use crate::prelude::*;
use crate::tooltip::TooltipHost;

// @pob Classes/DropDownControl.lua:1-13 h=b8006036 (header, locals and newClass: the struct and Widget impl below)

/// One `list[i]` entry. A Lua string entry is a DropItem whose `value` is the `String`.
#[derive(Clone)]
pub struct DropItem {
    pub label: String,
    pub search_filter: Option<String>,
    /// `listVal.detail` (drawn right-aligned in the box and in the list; CCR-0086 B)
    pub detail: Option<String>,
    pub value: Rc<dyn Any>,
}

impl DropItem {
    /// A Lua string entry.
    pub fn str(s: impl Into<String>) -> DropItem {
        let s: String = s.into();
        DropItem {
            label: s.clone(),
            search_filter: None,
            detail: None,
            value: Rc::new(s),
        }
    }
    /// A Lua table entry `{ label = label, ... }` whose other fields are `value`.
    pub fn new(label: impl Into<String>, value: Rc<dyn Any>) -> DropItem {
        DropItem {
            label: label.into(),
            search_filter: None,
            detail: None,
            value,
        }
    }
    /// `searchFilter = ..`
    #[must_use]
    pub fn search_filter(mut self, f: impl Into<String>) -> DropItem {
        self.search_filter = Some(f.into());
        self
    }
    /// `detail = ..`
    #[must_use]
    pub fn detail(mut self, d: impl Into<String>) -> DropItem {
        self.detail = Some(d.into());
        self
    }
    /// The value as `T` (`String` for a string entry).
    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.value.downcast_ref::<T>()
    }
    /// The string of a Lua string entry.
    pub fn as_str(&self) -> Option<&str> {
        self.get::<String>().map(String::as_str)
    }
}

impl std::fmt::Debug for DropItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DropItem({:?})", self.label)
    }
}

/// The `mode` argument of tooltipFunc (DropDownControl.lua Draw: "BODY" | "OUT" | "HOVER").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropTipMode {
    Body,
    Out,
    Hover,
}

impl DropTipMode {
    /// The Lua string.
    pub const fn as_str(self) -> &'static str {
        match self {
            DropTipMode::Body => "BODY",
            DropTipMode::Out => "OUT",
            DropTipMode::Hover => "HOVER",
        }
    }
}

/// tooltipFunc(tooltip, mode, index, value)
pub type DropTipArgs = (DropTipMode, Option<usize>, Option<DropItem>);

/// `selFunc(index, value)`; `value` is `self.list[index]`, which may be nil (see the module docs).
pub type SelFunc = Rc<dyn Fn(&mut Cx, usize, Option<&DropItem>)>;

/// `self.borderFunc()` -> r, g, b (ConfigTab.lua:697).
pub type BorderFunc = Rc<dyn Fn(&Cx, CtrlId) -> (f64, f64, f64)>;

/// The second return value of `IsMouseOver` (`mOverComp`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropComp {
    Body,
    Drop,
}

pub struct DropDownControl {
    pub tt: TooltipHost<DropTipArgs>,
    pub search: SearchHost,
    pub list: Vec<DropItem>,
    /// `self.selIndex` (1-based)
    pub sel_index: usize,
    pub sel_func: Option<SelFunc>,
    pub dropped: bool,
    pub drop_up: bool,
    /// `self.dropHeight`
    pub drop_height: f64,
    /// `self.droppedWidth`
    pub dropped_width: f64,
    /// `self.maxDroppedWidth`
    pub max_dropped_width: f64,
    /// `self.enableDroppedWidth`
    pub enable_dropped_width: bool,
    /// `self.enableChangeBoxWidth`
    pub enable_change_box_width: bool,
    /// `self.hoverSelDrop` (false | number)
    pub hover_sel_drop: Option<f64>,
    /// `self.hoverSel` (nil | false | a valid list index)
    pub hover_sel: Option<usize>,
    /// `self.borderFunc`
    pub border_func: Option<BorderFunc>,
}

#[track_caller]
fn num(v: Option<f64>) -> f64 {
    v.unwrap_or_else(|| crate::lua_error!("attempt to perform arithmetic on a nil value"))
}

/// The value mapping function of the constructor (L24-L34).
fn value_accessor(list_val: &DropItem) -> String {
    // L25-L33: a table entry: searchFilter, else label; a string entry: the string (= label)
    StripEscapes(list_val.search_filter.as_deref().unwrap_or(&list_val.label))
}

fn search(cx: &mut Cx, me: CtrlId) -> &mut SearchHost {
    &mut cx.w_mut::<DropDownControl>(me).search
}

impl DropDownControl {
    /// The fields of `DropDownControl(anchor, rect, list, selFunc, tooltipText, ignoreSearchOrder)`;
    /// the rest of the constructor (the scroll bar, `SetList`, the widths) is [`Self::init`].
    pub fn state(
        list: Vec<DropItem>,
        sel_func: Option<SelFunc>,
        tooltip_text: Option<Prop<Option<String>>>,
        ignore_search_order: bool,
    ) -> DropDownControl {
        DropDownControl {
            // L17
            tt: TooltipHost::new(tooltip_text),
            // L18-L36
            search: SearchHost::new(
                // L19 list to filter; L23 value mapping function
                Rc::new(|cx, me| {
                    cx.w::<DropDownControl>(me)
                        .list
                        .iter()
                        .map(value_accessor)
                        .collect()
                }),
                // L35
                ignore_search_order,
            ),
            // set by SetList in init (L45)
            list,
            // L46
            sel_index: 1,
            // L47
            sel_func,
            dropped: false,
            drop_up: false,
            // L44
            drop_height: 0.0,
            // L49/L51 in init
            dropped_width: 0.0,
            max_dropped_width: 0.0,
            // L53
            enable_dropped_width: false,
            // L55
            enable_change_box_width: false,
            hover_sel_drop: None,
            hover_sel: None,
            border_func: None,
        }
    }

    /// lua: DropDownControl.lua:14-58 DropDownControl(anchor, rect, list, selFunc, tooltipText, ignoreSearchOrder)
    /// @pob Classes/DropDownControl.lua:14-58 h=18f8810a
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        list: Vec<DropItem>,
        sel_func: Option<SelFunc>,
        tooltip_text: Option<Prop<Option<String>>>,
        ignore_search_order: bool,
    ) -> New<DropDownControl> {
        // L15 Control; L16 ControlHost
        New::new(
            Base::new(anchor, rect),
            DropDownControl::state(list, sel_func, tooltip_text, ignore_search_order),
        )
        .host()
        .init(|cx, me| DropDownControl::init(cx, me.0))
    }

    /// The constructor tail (L37-L55), after the node exists: `me` is the dropdown (or the
    /// subclass node that embeds it).
    pub fn init(cx: &mut Cx, me: CtrlId) {
        // L37 (raw `self.height`)
        let height = cx.base(me).height.raw();
        cx.add(
            me,
            "scrollBar",
            ScrollBarControl::new(
                Anchor::new(TopRight, me, TopRight),
                rect(-1, 0, 18, 0),
                Some((height - 4.0) * 4.0),
                None,
                false,
            )
            // L38
            .height(Prop::f(move |cx, _| {
                // L39
                cx.w::<DropDownControl>(me).drop_height + 2.0
            }))
            // L41
            .shown(Prop::f(move |cx, sb| {
                // L42
                cx.w::<DropDownControl>(me).dropped && ScrollBarControl::enabled_raw(cx, sb)
            })),
        );
        // L44
        cx.w_mut::<Self>(me).drop_height = 0.0;
        // L45 SetList(list or { })
        let list = std::mem::take(&mut cx.w_mut::<Self>(me).list);
        Self::set_list(cx, me, list);
        // L46-L47 (state)
        let width = cx.base(me).width.raw();
        let w = cx.w_mut::<Self>(me);
        w.sel_index = 1;
        // L48 Current value of the width of the dropped component
        // L49
        w.dropped_width = width;
        // L50 Set by the parent control. The maximum width of the dropped component will go to.
        // L51
        w.max_dropped_width = m_max(width, 300.0);
        // L52 Set by the parent control. Activates the auto width of the dropped component.
        // L53
        w.enable_dropped_width = false;
        // L54 Set by the parent control. Activates the auto width of the box component.
        // L55
        w.enable_change_box_width = false;
        // L56 -- self.tag = "-"
    }

    fn scroll_bar(cx: &Cx, me: CtrlId) -> CtrlId {
        cx.child_as::<ScrollBarControl>(me, "scrollBar").id()
    }

    /// lua: DropDownControl.lua:60-79 DropIndexToListIndex(dropIndex)
    /// @pob Classes/DropDownControl.lua:60-79 h=4a52b639
    pub fn drop_index_to_list_index(cx: &Cx, me: CtrlId, drop_index: Option<f64>) -> Option<f64> {
        // L60 maps the actual dropdown row index (after eventual filtering) to the original (unfiltered) list index
        let w = cx.w::<Self>(me);
        // L62 1:1
        // L63
        if !w.search.is_search_active() {
            // L64
            return drop_index;
        }
        // L66 out of bounds
        // L67
        let mut drop_index = match drop_index {
            Some(d) if d > 0.0 && d <= Self::get_drop_count(cx, me) as f64 => d,
            // L68
            _ => return None,
        };
        // L70 actual mapping
        // L71
        for (list_index, info) in w.search.search_infos.iter().enumerate() {
            // L72
            if info.matches {
                // L73
                drop_index -= 1.0;
                // L74
                if drop_index <= 0.0 {
                    // L75
                    return Some((list_index + 1) as f64);
                }
            }
        }
        None
    }

    /// lua: DropDownControl.lua:81-103 ListIndexToDropIndex(listIndex, default)
    /// @pob Classes/DropDownControl.lua:81-103 h=663bbd88
    pub fn list_index_to_drop_index(
        cx: &Cx,
        me: CtrlId,
        list_index: f64,
        default: Option<f64>,
    ) -> Option<f64> {
        // L81 maps the original (unfiltered) list index to the actual dropdown row index (after eventual filtering)
        let w = cx.w::<Self>(me);
        // L83 1:1
        // L84
        if !w.search.is_search_active() {
            // L85
            return Some(list_index);
        }
        // L87 out of bounds
        // L88
        if list_index <= 0.0 || list_index > w.list.len() as f64 {
            // L89
            return None;
        }
        // L91 actual mapping
        // L92
        let mut drop_index = 0.0;
        // L93
        for (i, info) in w.search.search_infos.iter().enumerate() {
            // L94
            if info.matches {
                // L95
                drop_index += 1.0;
                // L96
                if list_index == (i + 1) as f64 {
                    // L97
                    return Some(drop_index);
                }
            }
        }
        // L101 given listIndex is currently filtered out
        // L102
        default
    }

    /// lua: DropDownControl.lua:105-111
    /// @pob Classes/DropDownControl.lua:105-111 h=ba4a7869
    pub fn get_drop_count(cx: &Cx, me: CtrlId) -> usize {
        let w = cx.w::<Self>(me);
        // L106
        if w.search.is_search_active() {
            // L107
            w.search.get_match_count()
        } else {
            // L109
            w.list.len()
        }
    }

    /// lua: DropDownControl.lua:113-133 DrawSearchHighlights(label, searchInfo, x, y, width, height)
    /// @pob Classes/DropDownControl.lua:113-133 h=2e39fb66
    pub fn draw_search_highlights(
        cx: &mut Cx,
        label: &str,
        search_info: Option<&SearchInfo>,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    ) {
        let _ = width;
        // L114
        if let Some(search_info) = search_info.filter(|i| i.matches) {
            // L115
            let mut start_x;
            // L116
            let mut end_x = 0.0;
            // L117
            let mut last = 0.0;
            // L118
            cx.sg.set_draw_color_a(1.0, 1.0, 0.0, 0.2);
            // L119
            let stripped_label = StripEscapes(label);
            // L120
            for range in &search_info.ranges {
                let (from, to) = (range.from as f64, range.to as f64);
                // L121
                if from - last - 1.0 > 0.0 {
                    // L122
                    start_x = cx.sg.draw_string_width(
                        height,
                        Font::Var,
                        &luastr::sub(&stripped_label, last + 1.0, Some(from - 1.0)),
                    ) + x
                        + end_x;
                } else {
                    // L124
                    start_x = end_x;
                }
                // L126
                end_x = cx.sg.draw_string_width(
                    height,
                    Font::Var,
                    &luastr::sub(&stripped_label, from, Some(to)),
                ) + x
                    + start_x;
                // L127
                last = to;
                // L129
                cx.sg.draw_image(None, start_x, y, end_x - start_x, height);
            }
            // L131
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
        }
    }

    /// lua: DropDownControl.lua:136-150 SelByValue(value, key): `pred` is `listVal[key] == value`
    /// for a table entry, `listVal == value` for a string entry.
    /// @pob Classes/DropDownControl.lua:136-150 h=43686afa
    pub fn sel_by_value(cx: &mut Cx, me: CtrlId, pred: &dyn Fn(&DropItem) -> bool) {
        // L137
        let found = cx.w::<Self>(me).list.iter().position(pred);
        // L138-L147
        if let Some(index) = found {
            // L140 / L145
            cx.w_mut::<Self>(me).sel_index = index + 1;
            // L141 / L146 return
        }
    }

    /// lua: DropDownControl.lua:152-154 GetSelValueByKey(key): `self.list[self.selIndex][key]`
    /// (`key` reads the field; a nil entry is the Lua index error).
    /// @pob Classes/DropDownControl.lua:152-154 h=4996fead
    pub fn get_sel_value_by_key<R>(cx: &Cx, me: CtrlId, key: impl FnOnce(&DropItem) -> R) -> R {
        let w = cx.w::<Self>(me);
        // L153
        match w.sel_index.checked_sub(1).and_then(|i| w.list.get(i)) {
            Some(v) => key(v),
            None => crate::lua_error!("attempt to index a nil value"),
        }
    }

    /// lua: DropDownControl.lua:156-158
    /// @pob Classes/DropDownControl.lua:156-158 h=dfaefbde
    pub fn get_sel_value(cx: &Cx, me: CtrlId) -> Option<DropItem> {
        let w = cx.w::<Self>(me);
        // L157
        w.sel_index
            .checked_sub(1)
            .and_then(|i| w.list.get(i))
            .cloned()
    }

    /// `SetSel(newSel, noCallSelFunc)` with an index (see [`Self::set_sel_num`]).
    pub fn set_sel(cx: &mut Cx, me: CtrlId, new_sel: usize, no_call_sel_func: bool) {
        Self::set_sel_num(cx, me, new_sel as f64, no_call_sel_func);
    }

    /// lua: DropDownControl.lua:160-169 SetSel(newSel, noCallSelFunc) with the Lua number.
    /// @pob Classes/DropDownControl.lua:160-169 h=0296a684
    pub fn set_sel_num(cx: &mut Cx, me: CtrlId, new_sel: f64, no_call_sel_func: bool) {
        // L161
        let new_sel = m_max(1.0, m_min(Self::get_drop_count(cx, me) as f64, new_sel));
        // L162
        let new_sel = Self::drop_index_to_list_index(cx, me, Some(new_sel));
        // L163
        if let Some(new_sel) = new_sel.filter(|&n| n != cx.w::<Self>(me).sel_index as f64) {
            let new_sel = new_sel as usize;
            let w = cx.w_mut::<Self>(me);
            // L164
            w.sel_index = new_sel;
            // L165
            if let Some(f) = w.sel_func.clone().filter(|_| !no_call_sel_func) {
                // L166
                let value = w.list.get(new_sel - 1).cloned();
                f(cx, new_sel, value.as_ref());
            }
        }
    }

    /// lua: DropDownControl.lua:171-176
    /// @pob Classes/DropDownControl.lua:171-176 h=d12cd5e8
    pub fn scroll_sel_into_view(cx: &mut Cx, me: CtrlId) {
        // L172
        let (_width, height) = cx.size(me);
        // L173
        let scroll_bar = Self::scroll_bar(cx, me);
        // L174
        let (count, drop_height) = (
            Self::get_drop_count(cx, me) as f64,
            cx.w::<Self>(me).drop_height,
        );
        ScrollBarControl::set_content_dimension(
            cx,
            scroll_bar,
            (height - 4.0) * count,
            drop_height,
        );
        // L175
        let sel_index = cx.w::<Self>(me).sel_index as f64;
        let drop_index = num(Self::list_index_to_drop_index(cx, me, sel_index, Some(1.0)));
        ScrollBarControl::scroll_into_view(
            cx,
            scroll_bar,
            (drop_index - 2.0) * (height - 4.0),
            3.0 * (height - 4.0),
        );
    }

    /// lua: DropDownControl.lua:178-210 IsMouseOver() (both return values)
    /// @pob Classes/DropDownControl.lua:178-210 h=be38d2ee
    pub fn is_mouse_over_comp(cx: &Cx, me: CtrlId) -> (bool, Option<DropComp>) {
        // L179
        if !cx.is_shown(me) {
            // L180
            return (false, None);
        }
        // L182
        if cx.get_mouse_over_control(me).is_some() {
            // L183
            return (true, None);
        }
        // L185
        let (x, y) = cx.pos(me);
        // L186
        let (mut width, height) = cx.size(me);
        // L187
        let (cursor_x, cursor_y) = cx.sys.cursor_pos();
        let w = cx.w::<Self>(me);
        // L188
        let drop_extra = if w.dropped { w.drop_height + 2.0 } else { 0.0 };
        // L189
        let m_over;
        // L191
        if w.dropped {
            // L192
            width = m_max(width, w.dropped_width);
            // L193
            if w.drop_up {
                // L194
                m_over = cursor_x >= x
                    && cursor_y >= y - drop_extra
                    && cursor_x < x + width
                    && cursor_y < y + height;
            } else {
                // L196
                m_over = cursor_x >= x
                    && cursor_y >= y
                    && cursor_x < x + width
                    && cursor_y < y + height + drop_extra;
            }
        } else {
            // L199
            m_over =
                cursor_x >= x && cursor_y >= y && cursor_x < x + width && cursor_y < y + height;
        }
        // L201
        let mut m_over_comp = None;
        // L202
        if m_over {
            // L203
            if cursor_y >= y && cursor_y < y + height {
                // L204
                m_over_comp = Some(DropComp::Body);
            } else {
                // L206
                m_over_comp = Some(DropComp::Drop);
            }
        }
        // L209
        (m_over, m_over_comp)
    }

    /// lua: DropDownControl.lua:212-425 Draw(viewPort, noTooltip)
    /// @pob Classes/DropDownControl.lua:212-425 h=1b5b4e7b
    pub fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        // L213
        let (x, y) = cx.pos(me);
        // L214
        let (mut width, height) = cx.size(me);
        // L215
        let enabled = cx.is_enabled(me);
        // L216
        let scroll_bar = Self::scroll_bar(cx, me);
        // L217
        let line_height = height - 4.0;
        // L218
        let list_len = cx.w::<Self>(me).list.len() as f64;
        cx.w_mut::<Self>(me).drop_height = line_height * m_min(list_len, 20.0);
        // L219
        cx.base_mut(scroll_bar).y = Prop::Val(height + 1.0);
        // L220
        let drop_height = cx.w::<Self>(me).drop_height;
        if y + height + drop_height + 4.0 <= vp.y + vp.height {
            // L221 Drop fits below body
            // L222
            cx.w_mut::<Self>(me).drop_up = false;
        } else {
            // L224
            let lines_above = m_floor((y - vp.y - 4.0) / line_height);
            // L225
            let lines_below = m_floor((vp.y + vp.height - y - height - 4.0) / line_height);
            // L226
            if lines_above > lines_below {
                // L227 There's more room above the body than below
                let w = cx.w_mut::<Self>(me);
                // L228
                w.drop_up = true;
                // L229
                if y - vp.y < w.drop_height + 4.0 {
                    // L230 Still doesn't fit, so clip it
                    // L231
                    w.drop_height = line_height * lines_above;
                }
                // L233
                let drop_height = w.drop_height;
                cx.base_mut(scroll_bar).y = Prop::Val(-drop_height - 3.0);
            } else {
                // L235 Doesn't fit below body, so clip it
                let w = cx.w_mut::<Self>(me);
                // L236
                w.drop_up = false;
                // L237
                w.drop_height = line_height * lines_below;
            }
        }

        // L241
        if cx.w::<Self>(me).search.is_search_active() && !cx.w::<Self>(me).dropped {
            // L242
            cx.w_mut::<Self>(me).search.reset_search();
        }

        // L245 fit dropHeight to filtered content but keep initial orientation
        // L246
        let count = Self::get_drop_count(cx, me) as f64;
        {
            let w = cx.w_mut::<Self>(me);
            w.drop_height = m_max(m_min(w.drop_height, count * line_height), line_height);
        }

        // L248
        let (m_over, m_over_comp) = Self::is_mouse_over_comp(cx, me);
        let (dropped, drop_up, drop_height, dropped_width, border_func) = {
            let w = cx.w::<Self>(me);
            (
                w.dropped,
                w.drop_up,
                w.drop_height,
                w.dropped_width,
                w.border_func.clone(),
            )
        };
        // L249
        let drop_extra = drop_height + 4.0;
        // L250
        ScrollBarControl::set_content_dimension(cx, scroll_bar, line_height * count, drop_height);
        // L251
        let drop_y = if drop_up { y - drop_extra } else { y + height };
        // L252
        if !enabled {
            // L253
            cx.sg.set_draw_color(0.33, 0.33, 0.33);
        } else if m_over || dropped {
            // L255
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
        } else if let Some(f) = border_func {
            // L257
            let (r, g, b) = f(cx, me);
            // L258
            cx.sg.set_draw_color(r, g, b);
        } else {
            // L260
            cx.sg.set_draw_color(0.5, 0.5, 0.5);
        }
        // L262
        cx.sg.draw_image(None, x, y, width, height);
        // L263
        if dropped {
            // L264
            cx.sg.set_draw_layer(None, Some(5));
            // L265
            cx.sg.draw_image(None, x, drop_y, dropped_width, drop_extra);
            // L266
            cx.sg.set_draw_layer(None, Some(0));
        }
        // L268
        if !enabled || dropped {
            // L269
            cx.sg.set_draw_color(0.0, 0.0, 0.0);
        } else if m_over {
            // L271
            cx.sg.set_draw_color(0.33, 0.33, 0.33);
        } else {
            // L273
            cx.sg.set_draw_color(0.0, 0.0, 0.0);
        }
        // L275
        cx.sg
            .draw_image(None, x + 1.0, y + 1.0, width - 2.0, height - 2.0);
        // L276
        if !enabled {
            // L277
            cx.sg.set_draw_color(0.33, 0.33, 0.33);
        } else if m_over || dropped {
            // L279
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
        } else {
            // L281
            cx.sg.set_draw_color(0.5, 0.5, 0.5);
        }
        // L283
        draw_arrow(
            cx,
            x + width - height / 2.0,
            y + height / 2.0,
            height / 2.0,
            height / 2.0,
            ArrowDir::Down,
        );
        // L284
        if dropped {
            // L285
            cx.sg.set_draw_layer(None, Some(5));
            // L286
            cx.sg.set_draw_color(0.0, 0.0, 0.0);
            // L287
            cx.sg.draw_image(
                None,
                x + 1.0,
                drop_y + 1.0,
                dropped_width - 2.0,
                drop_extra - 2.0,
            );
            // L288
            cx.sg.set_draw_layer(None, Some(0));
        }
        // L290 CCR-0078: `self.otherDragSource` through the side table
        if other_drag_source(cx, me).is_some() {
            // L291
            cx.sg.set_draw_color_a(0.0, 1.0, 0.0, 0.25);
            // L292
            cx.sg.draw_image(None, x, y, width, height);
        }

        // L295 draw dropdown bar
        // L296
        if enabled {
            // L297
            if (m_over || dropped) && m_over_comp != Some(DropComp::Drop) && !no_tooltip {
                // L298
                cx.sg.set_draw_layer(None, Some(100));
                // L299-L303
                let (sel_index, sel_value) = {
                    let w = cx.w::<Self>(me);
                    (
                        w.sel_index,
                        w.sel_index
                            .checked_sub(1)
                            .and_then(|i| w.list.get(i))
                            .cloned(),
                    )
                };
                TooltipHost::draw_tooltip::<Self>(
                    cx,
                    me,
                    |w| &mut w.tt,
                    x,
                    y - if dropped && drop_up { drop_extra } else { 0.0 },
                    width,
                    height + if dropped { drop_extra } else { 0.0 },
                    vp,
                    (
                        if m_over {
                            DropTipMode::Body
                        } else {
                            DropTipMode::Out
                        },
                        Some(sel_index),
                        sel_value,
                    ),
                );
                // L304
                cx.sg.set_draw_layer(None, Some(0));
            }
            // L306
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
        } else {
            // L308
            cx.sg.set_draw_color(0.66, 0.66, 0.66);
        }
        // L310 draw selected label or search term
        // L311-L312 (assigned below)
        let sel_label: String;
        let mut sel_detail: Option<String> = None;
        // L313
        if cx.w::<Self>(me).search.is_search_active() {
            // L314
            sel_label = format!(
                "Search: {}",
                cx.w::<Self>(me).search.get_search_term_pretty()
            );
        } else {
            let w = cx.w::<Self>(me);
            // L316
            let sel_item = w.sel_index.checked_sub(1).and_then(|i| w.list.get(i));
            // L317-L322: a table entry gives its label and detail, a string entry itself
            // (`selLabel or ""` at L325 covers a nil entry)
            sel_label = sel_item.map(|i| i.label.clone()).unwrap_or_default();
            sel_detail = sel_item.and_then(|i| i.detail.clone());
        }
        // L324
        cx.sg
            .set_viewport(x + 2.0, y + 2.0, width - height, line_height);
        // L325
        cx.sg
            .draw_string(0.0, 0.0, Align::Left, line_height, Font::Var, &sel_label);
        // L326
        if let Some(sel_detail) = sel_detail {
            // L327
            let dx = cx.sg.draw_string_width(line_height, Font::Var, &sel_detail);
            // L328
            if !enabled || dropped {
                // L329
                cx.sg.set_draw_color(0.0, 0.0, 0.0);
            } else if m_over {
                // L331
                cx.sg.set_draw_color(0.33, 0.33, 0.33);
            } else {
                // L333
                cx.sg.set_draw_color(0.0, 0.0, 0.0);
            }
            // L335
            cx.sg
                .draw_image(None, width - dx - 4.0 - 22.0, 0.0, width - 4.0, line_height);
            // L336
            if enabled {
                cx.sg.set_draw_color(1.0, 1.0, 1.0);
            } else {
                // L339
                cx.sg.set_draw_color(0.66, 0.66, 0.66);
            }
            // L341
            cx.sg.draw_string(
                width - dx - 22.0,
                0.0,
                Align::Left,
                line_height,
                Font::Var,
                &sel_detail,
            );
        }
        // L343
        cx.sg.reset_viewport();

        // L345 draw dropped down part with items
        // L346
        if dropped {
            // L347
            cx.sg.set_draw_layer(None, Some(5));
            // L348
            cx.draw_controls(me, vp, None);
            // L349
            width = dropped_width;

            // L351 draw tooltip for hovered item
            // L352
            let (_cursor_x, cursor_y) = cx.sys.cursor_pos();
            // L353
            let offset = cx.w::<ScrollBarControl>(scroll_bar).offset;
            let hover_sel_drop = if m_over && !cx.is_mouse_over(scroll_bar) {
                Some(m_floor((cursor_y - drop_y + offset) / line_height) + 1.0)
            } else {
                None
            };
            cx.w_mut::<Self>(me).hover_sel_drop = hover_sel_drop;
            // L354
            let hover_sel = Self::drop_index_to_list_index(cx, me, hover_sel_drop);
            // L355-L357: `self.list[self.hoverSel]` exists only for an integer in 1..=#list
            let hover_sel = hover_sel
                .filter(|&h| h >= 1.0 && h <= list_len && h.fract() == 0.0)
                .map(|h| h as usize);
            cx.w_mut::<Self>(me).hover_sel = hover_sel;
            // L358
            if let Some(hs) = hover_sel.filter(|_| !no_tooltip) {
                // L359
                cx.sg.set_draw_layer(None, Some(100));
                // L360-L364
                let value = cx.w::<Self>(me).list.get(hs - 1).cloned();
                TooltipHost::draw_tooltip::<Self>(
                    cx,
                    me,
                    |w| &mut w.tt,
                    x,
                    drop_y + 2.0 + (num(hover_sel_drop) - 1.0) * line_height - offset,
                    width,
                    line_height,
                    vp,
                    (DropTipMode::Hover, Some(hs), value),
                );
                // L365
                cx.sg.set_draw_layer(None, Some(5));
            }

            // L368 draw dropdown items
            // L369
            let sb_enabled = ScrollBarControl::enabled_raw(cx, scroll_bar);
            cx.sg.set_viewport(
                x + 2.0,
                drop_y + 2.0,
                if sb_enabled {
                    width - 22.0
                } else {
                    width - 4.0
                },
                drop_height,
            );
            // L370
            let mut drop_index = 0.0;
            // L371 (the list is not changed by the loop body)
            let (list, search_active, search_infos, sel_index) = {
                let w = cx.w::<Self>(me);
                (
                    w.list.clone(),
                    w.search.is_search_active(),
                    w.search.search_infos.clone(),
                    w.sel_index,
                )
            };
            for (i, list_val) in list.iter().enumerate() {
                let index = i + 1;
                // L372
                let search_info = search_infos.get(i);
                // L373 skip filtered out items if search is active
                // L374
                if !search_active || search_info.is_some_and(|s| s.matches) {
                    // L375
                    drop_index += 1.0;
                    // L376
                    let y = (drop_index - 1.0) * line_height - offset;
                    // L377 highlight background if hovered
                    // L378
                    if Some(index) == hover_sel {
                        // L379
                        cx.sg.set_draw_color(0.33, 0.33, 0.33);
                        // L380
                        cx.sg.draw_image(None, 0.0, y, width - 4.0, line_height);
                    }
                    // L382 highlight font color if hovered or selected
                    // L383
                    if Some(index) == hover_sel || index == sel_index {
                        cx.sg.set_draw_color(1.0, 1.0, 1.0);
                    } else {
                        // L386
                        cx.sg.set_draw_color(0.66, 0.66, 0.66);
                    }
                    // L388 draw actual item label with search match highlight if available
                    // L389-L396
                    let label = &list_val.label;
                    let detail = &list_val.detail;
                    // L397
                    cx.sg
                        .draw_string(0.0, y, Align::Left, line_height, Font::Var, label);
                    // L398
                    if let Some(detail) = detail {
                        // L399 local detail = listVal.detail
                        // L400
                        let dx = cx.sg.draw_string_width(line_height, Font::Var, detail);
                        // L401
                        if Some(index) == hover_sel {
                            // L402
                            cx.sg.set_draw_color(0.33, 0.33, 0.33);
                        } else {
                            // L404
                            cx.sg.set_draw_color(0.0, 0.0, 0.0);
                        }
                        // L406
                        cx.sg.draw_image(
                            None,
                            width - dx - 8.0 - 22.0,
                            y,
                            width - 4.0,
                            line_height,
                        );
                        // L407 highlight font color if hovered or selected
                        // L408
                        if Some(index) == hover_sel || index == sel_index {
                            cx.sg.set_draw_color(1.0, 1.0, 1.0);
                        } else {
                            // L411
                            cx.sg.set_draw_color(0.66, 0.66, 0.66);
                        }
                        // L413
                        cx.sg.draw_string(
                            width - dx - 4.0 - 22.0,
                            y,
                            Align::Left,
                            line_height,
                            Font::Var,
                            detail,
                        );
                    }
                    // L415
                    Self::draw_search_highlights(
                        cx,
                        label,
                        search_info,
                        0.0,
                        y,
                        width - 4.0,
                        line_height,
                    );
                }
            }
            // L418
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
            // L419
            let w = cx.w::<Self>(me);
            if w.search.is_search_active() && w.search.get_match_count() == 0 {
                // L420
                cx.sg.draw_string(
                    0.0,
                    0.0,
                    Align::Left,
                    line_height,
                    Font::Var,
                    "<No matches>",
                );
            }
            // L422
            cx.sg.reset_viewport();
            // L423
            cx.sg.set_draw_layer(None, Some(0));
        }
    }

    /// lua: DropDownControl.lua:427-432 OnChar(key)
    /// @pob Classes/DropDownControl.lua:427-432 h=d7fa463c
    pub fn on_char(cx: &mut Cx, me: CtrlId, ch: Key) -> Focus {
        // L428
        if !cx.is_shown(me) || !cx.is_enabled(me) || !cx.w::<Self>(me).dropped {
            // L429
            return None;
        }
        // L431 (OnSearchChar returns self)
        SearchHost::on_search_char(cx, me, search, ch).then_some(me)
    }

    /// lua: DropDownControl.lua:434-464 OnKeyDown(key)
    /// @pob Classes/DropDownControl.lua:434-464 h=232d8f6e
    pub fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        // L435
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L436
            return None;
        }
        // L438
        if cx.w::<Self>(me).dropped {
            // L439
            if SearchHost::on_search_key_down(cx, me, search, key) {
                // L440
                return Some(me);
            }
        }
        // L443
        let m_over_control = cx.get_mouse_over_control(me);
        // L444
        if let Some(m) = m_over_control.filter(|&m| cx.has(m, Caps::KEY_DOWN)) {
            // L445 (raw field write, no SetFocus)
            cx.host_mut(me).sel_control = Some(m);
            // L446 `mOverControl:OnKeyDown(key)`: the double-click flag is not passed
            return cx.on_key_down(m, key, false).map(|_| me);
        }
        // L448
        cx.host_mut(me).sel_control = None;
        // L450
        if key == Key::LeftButton || key == Key::RightButton {
            // L451
            let (m_over, m_over_comp) = Self::is_mouse_over_comp(cx, me);
            let dropped = cx.w::<Self>(me).dropped;
            // L452
            if !m_over || (dropped && m_over_comp == Some(DropComp::Body)) {
                // L453
                cx.w_mut::<Self>(me).dropped = false;
                // L454
                return Some(me);
            }
            // L456
            if !dropped {
                // L457
                cx.w_mut::<Self>(me).dropped = true;
                // L458
                Self::scroll_sel_into_view(cx, me);
            }
        } else if key == Key::Escape {
            // L461
            cx.w_mut::<Self>(me).dropped = false;
        }
        // L463
        cx.w::<Self>(me).dropped.then_some(me)
    }

    /// lua: DropDownControl.lua:466-522 OnKeyUp(key)
    /// @pob Classes/DropDownControl.lua:466-522 h=ca2cceee
    pub fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        // L467
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L468
            return None;
        }
        // L470
        if let Some(sel) = cx.host(me).sel_control {
            // L471
            let new_sel = cx.on_key_up(sel, key);
            // L472
            if new_sel.is_some() {
                // L473
                return Some(me);
            }
            // L475
            cx.host_mut(me).sel_control = None;
            // L477
            return Some(me);
        }
        let scroll_bar = Self::scroll_bar(cx, me);
        // L479
        if key == Key::LeftButton || key == Key::RightButton {
            // L480
            let (m_over, m_over_comp) = Self::is_mouse_over_comp(cx, me);
            // L481
            if !m_over {
                // L482
                cx.w_mut::<Self>(me).dropped = false;
            } else if m_over_comp == Some(DropComp::Drop) {
                // L484
                let (_x, y) = cx.pos(me);
                // L485
                let (_width, height) = cx.size(me);
                // L486
                let (_cursor_x, cursor_y) = cx.sys.cursor_pos();
                let (drop_height, drop_up) = {
                    let w = cx.w::<Self>(me);
                    (w.drop_height, w.drop_up)
                };
                // L487
                let drop_extra = drop_height + 4.0;
                // L488
                let drop_y = if drop_up { y - drop_extra } else { y + height };
                // L489
                let offset = cx.w::<ScrollBarControl>(scroll_bar).offset;
                Self::set_sel_num(
                    cx,
                    me,
                    m_floor((cursor_y - drop_y + offset) / (height - 4.0)) + 1.0,
                    false,
                );
                // L490
                cx.w_mut::<Self>(me).dropped = false;
            }
        } else if ScrollBarControl::is_scroll_down_key(cx, scroll_bar, key) {
            // L493
            if cx.w::<Self>(me).dropped && ScrollBarControl::enabled_raw(cx, scroll_bar) {
                // L494
                ScrollBarControl::scroll(cx, scroll_bar, 1.0);
            } else {
                // L496
                if cx.main.disable_scroll_control_interaction && key == Key::WheelDown {
                    // L497
                    return None;
                }
                // L499
                let sel = cx.w::<Self>(me).sel_index as f64;
                let d = num(Self::list_index_to_drop_index(cx, me, sel, Some(0.0)));
                Self::set_sel_num(cx, me, d + 1.0, false);
            }
            // L501
            return Some(me);
        } else if key == Key::Down {
            // L503
            let sel = cx.w::<Self>(me).sel_index as f64;
            let d = num(Self::list_index_to_drop_index(cx, me, sel, Some(0.0)));
            Self::set_sel_num(cx, me, d + 1.0, false);
            // L504
            Self::scroll_sel_into_view(cx, me);
            // L505
            return Some(me);
        } else if ScrollBarControl::is_scroll_up_key(cx, scroll_bar, key) {
            // L507
            if cx.w::<Self>(me).dropped && ScrollBarControl::enabled_raw(cx, scroll_bar) {
                // L508
                ScrollBarControl::scroll(cx, scroll_bar, -1.0);
            } else {
                // L510
                if cx.main.disable_scroll_control_interaction && key == Key::WheelUp {
                    // L511
                    return None;
                }
                // L513
                let sel = cx.w::<Self>(me).sel_index as f64;
                let d = num(Self::list_index_to_drop_index(cx, me, sel, Some(0.0)));
                Self::set_sel_num(cx, me, d - 1.0, false);
            }
            // L515
            return Some(me);
        } else if key == Key::Up {
            // L517
            let sel = cx.w::<Self>(me).sel_index as f64;
            let d = num(Self::list_index_to_drop_index(cx, me, sel, Some(0.0)));
            Self::set_sel_num(cx, me, d - 1.0, false);
            // L518
            Self::scroll_sel_into_view(cx, me);
            // L519
            return Some(me);
        }
        // L521
        cx.w::<Self>(me).dropped.then_some(me)
    }

    /// lua: DropDownControl.lua:524-526 GetHoverIndex(): `self.hoverSel or self.selIndex`
    /// @pob Classes/DropDownControl.lua:524-526 h=c579ad5f
    pub fn get_hover_index(cx: &Cx, me: CtrlId) -> usize {
        let w = cx.w::<Self>(me);
        // L525
        w.hover_sel.unwrap_or(w.sel_index)
    }

    /// lua: DropDownControl.lua:528-535 SetList(textList)
    /// @pob Classes/DropDownControl.lua:528-535 h=a9c76437
    pub fn set_list(cx: &mut Cx, me: CtrlId, list: Vec<DropItem>) {
        // L529 (textList is never nil here)
        // L530 wipeTable(self.list): the old list is dropped (DIVERGENCE(alias), module docs)
        // L531
        cx.w_mut::<Self>(me).list = list;
        // L532 check width on new list
        // L533
        let enable = cx.w::<Self>(me).enable_dropped_width;
        Self::check_dropped_width(cx, me, enable);
    }

    /// lua: DropDownControl.lua:537-573 CheckDroppedWidth(enable)
    /// @pob Classes/DropDownControl.lua:537-573 h=fe15ccc7
    pub fn check_dropped_width(cx: &mut Cx, me: CtrlId, enable: bool) {
        let scroll_bar = Self::scroll_bar(cx, me);
        // L538
        cx.w_mut::<Self>(me).enable_dropped_width = enable;
        // L539 (self.list always exists)
        if cx.w::<Self>(me).enable_dropped_width {
            // L540
            let mut scroll_width = 0.0;
            // L541
            if cx.w::<Self>(me).dropped && ScrollBarControl::enabled_raw(cx, scroll_bar) {
                // L542
                scroll_width = cx.base(scroll_bar).width.raw();
            }
            // L544
            let line_height = cx.base(me).height.raw() - 4.0;

            // L546 do not be smaller than the created width
            // L547
            let mut d_width = cx.base(me).width.raw();
            // L548
            for line in &cx.w::<Self>(me).list {
                // L549-L551 (`line.label or ""`; a string entry is its label)
                // L552 +10 to stop clipping
                // L553
                d_width = m_max(
                    d_width,
                    cx.sg.draw_string_width(line_height, Font::Var, &line.label) + 10.0,
                );
            }
            // L555 no greater than self.maxDroppedWidth
            // L556
            let w = cx.w_mut::<Self>(me);
            w.dropped_width = m_min(d_width + scroll_width, w.max_dropped_width);
            // L557
            if w.enable_change_box_width {
                // L558-L561
                let line = w
                    .sel_index
                    .checked_sub(1)
                    .and_then(|i| w.list.get(i))
                    .map(|l| l.label.clone());
                // L562 add 20 to account for the 'down arrow' in the box
                // L563-L564
                let box_width =
                    cx.sg
                        .draw_string_width(line_height, Font::Var, line.as_deref().unwrap_or(""))
                        + 20.0;
                // L565
                cx.base_mut(me).width = Prop::Val(m_max(m_min(box_width, 390.0), 190.0));
            }

            // L568
            let x = cx.w::<Self>(me).dropped_width - cx.base(me).width.raw() - 1.0;
            cx.base_mut(scroll_bar).x = Prop::Val(x);
        } else {
            // L570
            cx.w_mut::<Self>(me).dropped_width = cx.base(me).width.raw();
            // L571
            cx.base_mut(scroll_bar).x = Prop::Val(-1.0);
        }
    }
}

impl Widget for DropDownControl {
    const NAME: &'static str = "DropDownControl";
    const CAPS: Caps = Caps::DRAW
        .union(Caps::IS_MOUSE_OVER)
        .union(Caps::KEY_DOWN)
        .union(Caps::KEY_UP)
        .union(Caps::CHAR);

    /// lua: DropDownControl.lua:178-210 (first return value)
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        Self::is_mouse_over_comp(cx, me).0
    }
    /// lua: DropDownControl.lua:212-425
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        DropDownControl::draw(cx, me, vp, no_tooltip);
    }
    /// lua: DropDownControl.lua:427-432
    fn on_char(cx: &mut Cx, me: CtrlId, ch: Key) -> Focus {
        DropDownControl::on_char(cx, me, ch)
    }
    /// lua: DropDownControl.lua:434-464 (OnKeyDown(key): no double-click argument)
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, _double_click: bool) -> Focus {
        DropDownControl::on_key_down(cx, me, key)
    }
    /// lua: DropDownControl.lua:466-522
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        DropDownControl::on_key_up(cx, me, key)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    type Calls = Rc<RefCell<Vec<(usize, Option<String>)>>>;

    fn items(n: usize) -> Vec<DropItem> {
        (1..=n)
            .map(|i| DropItem::str(format!("Item {i}")))
            .collect()
    }

    fn dropdown(cx: &mut Cx, list: Vec<DropItem>) -> (CtrlId, Calls) {
        let calls = Calls::default();
        let c = calls.clone();
        let f: SelFunc = Rc::new(move |_cx, i, v| {
            c.borrow_mut().push((i, v.map(|v| v.label.clone())));
        });
        let id = cx
            .add_free(
                None,
                DropDownControl::new(
                    Anchor::NONE,
                    rect(10, 10, 150, 20),
                    list,
                    Some(f),
                    None,
                    false,
                ),
            )
            .id();
        (id, calls)
    }

    #[test]
    fn constructor_widths_and_scroll_bar() {
        let mut cx = Cx::headless();
        let (dd, _) = dropdown(&mut cx, items(3));
        let w = cx.w::<DropDownControl>(dd);
        assert_eq!(
            (w.sel_index, w.dropped_width, w.max_dropped_width),
            (1, 150.0, 300.0)
        );
        assert!(!w.enable_dropped_width && !w.enable_change_box_width);
        let sb = cx.child_as::<ScrollBarControl>(dd, "scrollBar");
        // step = (height - 4) * 4; x = -1 (CheckDroppedWidth(nil) in SetList)
        assert_eq!(cx.get(sb).step, 64.0);
        assert_eq!(cx.base(sb).x.raw(), -1.0);
        // shown only while dropped and enabled
        assert!(!cx.is_shown(sb.id()));
    }

    #[test]
    fn set_sel_clamps_and_calls_sel_func() {
        let mut cx = Cx::headless();
        let (dd, calls) = dropdown(&mut cx, items(3));
        DropDownControl::set_sel_num(&mut cx, dd, 7.0, false);
        assert_eq!(cx.w::<DropDownControl>(dd).sel_index, 3);
        // same index: no call
        DropDownControl::set_sel(&mut cx, dd, 3, false);
        // noCallSelFunc
        DropDownControl::set_sel_num(&mut cx, dd, -2.0, true);
        assert_eq!(cx.w::<DropDownControl>(dd).sel_index, 1);
        assert_eq!(*calls.borrow(), vec![(3, Some("Item 3".to_owned()))]);
        // the list shrank under selIndex: index 1 of an empty list, value nil
        DropDownControl::set_sel(&mut cx, dd, 3, true);
        DropDownControl::set_list(&mut cx, dd, Vec::new());
        DropDownControl::set_sel(&mut cx, dd, 2, false);
        assert_eq!(cx.w::<DropDownControl>(dd).sel_index, 1);
        assert_eq!(calls.borrow().last(), Some(&(1, None)));
        assert!(DropDownControl::get_sel_value(&cx, dd).is_none());
    }

    #[test]
    fn keys_step_the_selection() {
        let mut cx = Cx::headless();
        let (dd, calls) = dropdown(&mut cx, items(3));
        assert_eq!(DropDownControl::on_key_up(&mut cx, dd, Key::Down), Some(dd));
        assert_eq!(
            DropDownControl::on_key_up(&mut cx, dd, Key::WheelDown),
            Some(dd)
        );
        assert_eq!(
            DropDownControl::on_key_up(&mut cx, dd, Key::PageDown),
            Some(dd)
        );
        assert_eq!(cx.w::<DropDownControl>(dd).sel_index, 3);
        assert_eq!(DropDownControl::on_key_up(&mut cx, dd, Key::Up), Some(dd));
        assert_eq!(cx.w::<DropDownControl>(dd).sel_index, 2);
        // main.disableScrollControlInteraction blocks the wheel only
        cx.main.disable_scroll_control_interaction = true;
        assert_eq!(DropDownControl::on_key_up(&mut cx, dd, Key::WheelUp), None);
        assert_eq!(
            DropDownControl::on_key_up(&mut cx, dd, Key::PageUp),
            Some(dd)
        );
        assert_eq!(cx.w::<DropDownControl>(dd).sel_index, 1);
        let got: Vec<usize> = calls.borrow().iter().map(|c| c.0).collect();
        assert_eq!(got, vec![2, 3, 2, 1]);
        // other keys: `self.dropped and self`
        assert_eq!(
            DropDownControl::on_key_up(&mut cx, dd, Key::Char(b'a')),
            None
        );
        // ESCAPE closes
        cx.w_mut::<DropDownControl>(dd).dropped = true;
        assert_eq!(DropDownControl::on_key_down(&mut cx, dd, Key::Escape), None);
        assert!(!cx.w::<DropDownControl>(dd).dropped);
    }

    #[test]
    fn search_maps_drop_rows_to_list_indices() {
        let mut cx = Cx::headless();
        let list = vec![
            DropItem::str("^1Fire"),
            DropItem::new("Ice", Rc::new(())).search_filter("Cold Frost"),
            DropItem::str("Fireball"),
            DropItem::str("Lightning"),
        ];
        let (dd, calls) = dropdown(&mut cx, list);
        // typing needs the list dropped
        assert_eq!(DropDownControl::on_char(&mut cx, dd, Key::Char(b'f')), None);
        cx.w_mut::<DropDownControl>(dd).dropped = true;
        for ch in b"fir" {
            assert_eq!(
                DropDownControl::on_char(&mut cx, dd, Key::Char(*ch)),
                Some(dd)
            );
        }
        assert_eq!(DropDownControl::get_drop_count(&cx, dd), 2);
        let d2l = |cx: &Cx, d| DropDownControl::drop_index_to_list_index(cx, dd, d);
        assert_eq!(d2l(&cx, Some(1.0)), Some(1.0));
        assert_eq!(d2l(&cx, Some(2.0)), Some(3.0));
        assert_eq!(d2l(&cx, Some(3.0)), None);
        assert_eq!(d2l(&cx, None), None);
        let l2d = |cx: &Cx, l, d| DropDownControl::list_index_to_drop_index(cx, dd, l, d);
        assert_eq!(l2d(&cx, 3.0, None), Some(2.0));
        assert_eq!(l2d(&cx, 2.0, Some(0.0)), Some(0.0));
        assert_eq!(l2d(&cx, 5.0, Some(0.0)), None);
        // DOWN from list index 1 (drop row 1) selects drop row 2 = list index 3
        DropDownControl::on_key_up(&mut cx, dd, Key::Down);
        assert_eq!(cx.w::<DropDownControl>(dd).sel_index, 3);
        // the search filter, not the label, is searched
        DropDownControl::on_key_down(&mut cx, dd, Key::Escape);
        assert!(!cx.w::<DropDownControl>(dd).search.is_search_active());
        assert!(cx.w::<DropDownControl>(dd).dropped);
        for ch in b"frost" {
            DropDownControl::on_char(&mut cx, dd, Key::Char(*ch));
        }
        assert_eq!(DropDownControl::get_drop_count(&cx, dd), 1);
        DropDownControl::set_sel(&mut cx, dd, 1, false);
        assert_eq!(cx.w::<DropDownControl>(dd).sel_index, 2);
        assert_eq!(calls.borrow().last(), Some(&(2, Some("Ice".to_owned()))));
        // inactive search: 1:1
        cx.w_mut::<DropDownControl>(dd).search.reset_search();
        assert_eq!(d2l(&cx, Some(-3.0)), Some(-3.0));
        assert_eq!(l2d(&cx, 9.0, None), Some(9.0));
    }

    #[test]
    fn sel_by_value_and_hover_index() {
        let mut cx = Cx::headless();
        let (dd, calls) = dropdown(&mut cx, items(4));
        DropDownControl::sel_by_value(&mut cx, dd, &|v| v.as_str() == Some("Item 3"));
        assert_eq!(cx.w::<DropDownControl>(dd).sel_index, 3);
        // no match keeps selIndex
        DropDownControl::sel_by_value(&mut cx, dd, &|v| v.as_str() == Some("nope"));
        assert_eq!(cx.w::<DropDownControl>(dd).sel_index, 3);
        assert!(calls.borrow().is_empty());
        assert_eq!(DropDownControl::get_hover_index(&cx, dd), 3);
        cx.w_mut::<DropDownControl>(dd).hover_sel = Some(2);
        assert_eq!(DropDownControl::get_hover_index(&cx, dd), 2);
        assert_eq!(
            DropDownControl::get_sel_value_by_key(&cx, dd, |v| v.label.clone()),
            "Item 3"
        );
    }

    #[test]
    fn check_dropped_width_change_box() {
        let mut cx = Cx::headless();
        let (dd, _) = dropdown(&mut cx, items(2));
        cx.w_mut::<DropDownControl>(dd).enable_change_box_width = true;
        DropDownControl::check_dropped_width(&mut cx, dd, true);
        // the box width is clamped to 190..=390; the scroll bar sits at the dropped edge
        assert_eq!(cx.base(dd).width.raw(), 190.0);
        let dropped = cx.w::<DropDownControl>(dd).dropped_width;
        assert!(cx.w::<DropDownControl>(dd).enable_dropped_width);
        let sb = cx.child_as::<ScrollBarControl>(dd, "scrollBar");
        assert_eq!(cx.base(sb).x.raw(), dropped - 190.0 - 1.0);
        DropDownControl::check_dropped_width(&mut cx, dd, false);
        assert_eq!(cx.w::<DropDownControl>(dd).dropped_width, 190.0);
        assert_eq!(cx.base(sb).x.raw(), -1.0);
    }
}
