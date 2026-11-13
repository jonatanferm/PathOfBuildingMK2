//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/CalcSectionControl.lua 1-550.
//! Owner: U39.
//!
//! @pob-file Classes/CalcSectionControl.lua @16de4b82d
//!
//! One section of the Calcs tab (`newClass("CalcSectionControl", "Control", "ControlHost")`).
//! The section data are calc [`Table`]s (from `sections_data`, or the tab's SkillSelect section),
//! annotated in place as the Lua does: `colData.calcSection` (a [`CalcSectionRef`]),
//! `colData.xOffset/yOffset/width/height/x/y`, `rowData.enabled`. A cell's `control` is a
//! [`CellControl`] holding the control's id.
//!
//! Model half (CCR-0080 §1, ui doc §10.2): calc:BM-A's `CalcsTab::ctor_model` ported
//! CalcSectionControl.lua:20-49 for the model (the section `id` and each subsection's `id` and
//! `collapsed`, which `CalcsTab:Load/Save` read). The tab hands this constructor the model's
//! subsection tables (with the module's `label`, `defaultCollapsed` and `data` copied into them),
//! so `subSec.collapsed` is one value for the UI and for Load/Save, as it is in Lua. The UI
//! statements of 20-49 are here without a marker (the range is BM-A's); L34 and L49 are the model
//! half's and are not repeated (L49 would undo the collapsed states `Load` already applied: the
//! model loads the build before the UI constructors run).
//!
//! Lua semantics:
//! * `self.enabled` is the Control field (`IsEnabled`): `CheckFlag` returns true or nil, stored
//!   as `Prop::Val(bool)`. `rowData.enabled` keeps the Lua value (true, nil or false).
//! * `IsMouseOver` returns two values (`mOver, colData`): [`CalcSectionControl::is_mouse_over_comp`];
//!   the framework's `IsMouseOver` uses the first.
//! * `DrawContent` calls `self:DrawControls` once per subsection it visits (L476), as the Lua does.
//! * `for colour, colData in ipairs(rowData)` (L134, L173, L506): `colour` is the index (unused).
//!
//! DIVERGENCE: none. ORDER: no `pairs` loops (sequences only; the controls tables are the
//! framework's canonical order, D7).
//!
//! Props with side effects (ui doc §7.3): none (the shown/label functions only read).

// the Lua `if/else` shapes and boolean expressions are kept as written
#![allow(non_snake_case, clippy::redundant_else, clippy::nonminimal_bool)]

use std::rc::Rc;

use pob_lua::math::{m_max, m_min};
use pob_lua::n;
use pob_sg::{Align, Font};
use pob_ui_core::controls::ButtonControl;
use pob_ui_core::input::InputEvents;
use pob_ui_core::prelude::*;

use super::core as calcs;
use super::CalcsTab;
use crate::app::CxApp;
use crate::build::on_frame::{self, OverlayPane};
use crate::model::{Table, Val};
use pob_calc::actor::ActorRef;
// CCR-0103 §4: model.rs (U15) does not re-export the calc modules; CalcFormat is calc:CT's
use pob_calc::modules::calc_format::formatCalcStr;

// @pob Classes/CalcSectionControl.lua:1-11 h=7222f18c
// (`local t_insert, m_max, m_min`: Vec/`pob_lua::math`; `newClass("CalcSectionControl",
// "Control", "ControlHost")`: the `Widget` impl and `.host()` in the constructor)

/// `colData.calcSection` (CalcSectionControl.lua:38): the section a cell belongs to.
pub struct CalcSectionRef(pub CtrlId);

/// `colData.control` (CalcsTab.lua:46-140): a control placed in a cell.
pub struct CellControl(pub CtrlId);

/// `updateFunc(section)` (CalcsTab.lua:144-148; `self:updateFunc()`, L158).
pub type UpdateFunc = Rc<dyn Fn(&mut Cx, CtrlId)>;

/// The section as a pinned pane (`build.overlayPanes`, Build.lua:626; U20's [`OverlayPane`]).
pub struct SectionPane(pub CtrlId);

pub struct CalcSectionControl {
    /// `self.calcsTab`
    pub calcs_tab: CtrlId,
    /// `self.id`
    pub id: String,
    /// `self.group`
    pub group: f64,
    /// `self.colour`
    pub colour: String,
    /// `self.subSection` (the model's subsection tables, see the module doc)
    pub sub_section: Table,
    /// `self.flag`, `self.notFlag` (L29-30), as the object `CheckFlag(self)` reads
    pub flags: Table,
    /// `self.updateFunc`
    pub update_func: Option<UpdateFunc>,
    /// `self.hasControls`
    pub has_controls: bool,
    /// `self.widthCols` (CalcsTab.lua:372)
    pub width_cols: f64,
    /// `self.isOverlay`
    pub is_overlay: bool,
    /// `self.overlayX`
    pub overlay_x: f64,
    /// `self.overlayY`
    pub overlay_y: f64,
    /// `self.dragging`
    pub dragging: bool,
    /// `self.dragOffX`
    pub drag_off_x: f64,
    /// `self.dragOffY`
    pub drag_off_y: f64,
    /// `self.overlayRevision` (nil until the first DrawOverlay)
    pub overlay_revision: Option<f64>,
    /// `self.overlayBreakdownCell`
    pub overlay_breakdown_cell: bool,
    /// this section in `build.overlayPanes` (one object, compared by identity)
    pane: Rc<SectionPane>,
}

impl Widget for CalcSectionControl {
    const NAME: &'static str = "CalcSectionControl";
    const CAPS: Caps = Caps::DRAW
        .union(Caps::IS_MOUSE_OVER)
        .union(Caps::KEY_DOWN)
        .union(Caps::KEY_UP);
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        Self::draw(cx, me, vp, no_tooltip);
    }
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        Self::is_mouse_over_comp(cx, me).0
    }
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, double_click: bool) -> Focus {
        Self::on_key_down(cx, me, key, double_click)
    }
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        Self::on_key_up(cx, me, key)
    }
}

fn st(cx: &Cx, me: CtrlId) -> &CalcSectionControl {
    cx.w::<CalcSectionControl>(me)
}

fn st_mut(cx: &mut Cx, me: CtrlId) -> &mut CalcSectionControl {
    cx.w_mut::<CalcSectionControl>(me)
}

/// The raw field `self.enabled` (a boolean once the constructor ran).
fn enabled_raw(cx: &Cx, me: CtrlId) -> bool {
    cx.base(me).enabled.raw()
}

/// `subSection[i]` as a table (`ipairs` yields only non-nil values; a non-table is a Lua index
/// error at its first field read).
#[track_caller]
fn tbl(v: &Val) -> Table {
    v.table()
        .unwrap_or_else(|| lua_error!("attempt to index a {} value", v.lua_type()))
}

/// `self.subSection[1]`
#[track_caller]
fn first_sub(sub_section: &Table) -> Table {
    tbl(&sub_section.get(1))
}

/// `subSec.data`
#[track_caller]
fn sub_data(subSec: &Table) -> Table {
    tbl(&subSec.get(n!("data")))
}

/// `x .. y` operand (a string, or a number via `tostring`).
#[track_caller]
fn concat(v: &Val) -> String {
    match v {
        Val::Str(s) => s.to_string(),
        Val::Num(x) => pob_lua::tostring_num(*x),
        other => lua_error!("attempt to concatenate a {} value", other.lua_type()),
    }
}

/// `SetDrawColor(v)` for a colour escape string value (`rowData.bgCol or "^0"`).
fn set_color_val(cx: &mut Cx, v: &Val) {
    match v {
        Val::Str(s) => cx.sg.set_draw_color_esc(s),
        Val::Nil | Val::Bool(false) => cx.sg.set_draw_color_esc("^0"),
        other => lua_error!(
            "SetDrawColor() argument 1: expected string or number, got {}",
            other.lua_type()
        ),
    }
}

impl CalcSectionControl {
    /// lua: CalcSectionControl.lua:20-82 CalcSectionControl(calcsTab, width, id, group, colour,
    /// subSection, updateFunc). `width_cols` is CalcsTab.lua:372 (`section.widthCols = width`,
    /// set right after the constructor returns).
    /// @pob Classes/CalcSectionControl.lua:50-82 h=4d0c93ac
    pub fn new(
        calcs_tab: CtrlId,
        width: f64,
        id: &str,
        group: f64,
        colour: &str,
        sub_section: Table,
        update_func: Option<UpdateFunc>,
        width_cols: f64,
    ) -> New<CalcSectionControl> {
        // L29-30 (`subSection[1].data.flag`: a missing subsection is a Lua index error)
        let first = first_sub(&sub_section);
        let data = sub_data(&first);
        let flags = Table::new();
        flags.set(n!("flag"), data.get(n!("flag")));
        flags.set(n!("notFlag"), data.get(n!("notFlag")));
        // L21 self:Control(calcsTab, {0, 0, width, 0}) (`calcsTab[1]` is nil: no anchor)
        New::new(
            Base::new(Anchor::NONE, rect(0, 0, width, 0)),
            CalcSectionControl {
                // L23
                calcs_tab,
                // L24
                id: id.to_owned(),
                // L25
                group,
                // L26
                colour: colour.to_owned(),
                // L27 self.width = width (the Control field, set by L21 already)
                // L28
                sub_section,
                flags,
                // L31
                update_func,
                has_controls: false,
                width_cols,
                // L72
                is_overlay: false,
                // L73
                overlay_x: 320.0,
                // L74
                overlay_y: 50.0,
                // L75
                dragging: false,
                // L76
                drag_off_x: 0.0,
                // L77
                drag_off_y: 0.0,
                overlay_revision: None,
                overlay_breakdown_cell: false,
                pane: Rc::new(SectionPane(CtrlId::default())),
            },
        )
        // L22 self:ControlHost()
        .host()
        .init(|cx, me| {
            let me = me.id();
            st_mut(cx, me).pane = Rc::new(SectionPane(me));
            let sub_section = st(cx, me).sub_section.clone();
            // L33
            for (i, subSec) in sub_section.ipairs() {
                let subSec = tbl(&subSec);
                // L34 subSec.id = subSec.label:gsub("%W", "") (model half: calc:BM-A)

                // L36
                for (_, data) in sub_data(&subSec).ipairs() {
                    let data = tbl(&data);
                    // L37
                    for (_, colData) in data.ipairs() {
                        let colData = tbl(&colData);
                        // L38
                        colData.set(n!("calcSection"), Val::Obj(Rc::new(CalcSectionRef(me))));
                        // L39
                        if let Some(control) = colData.obj::<CellControl>(n!("control")) {
                            // L40
                            st_mut(cx, me).has_controls = true;
                            // Add control to the section's control list and set show/hide function
                            // L42
                            let name = concat(&colData.get(n!("controlName")));
                            cx.put(me, name, control.0);
                            // L43-45
                            let (sub_section, subSec, data) =
                                (sub_section.clone(), subSec.clone(), data.clone());
                            cx.base_mut(control.0).shown = Prop::f(move |cx, _| {
                                // L44
                                enabled_raw(cx, me)
                                    && !first_sub(&sub_section).truthy(n!("collapsed"))
                                    && !subSec.truthy(n!("collapsed"))
                                    && data.truthy(n!("enabled"))
                            });
                        }
                    }
                }
                // L49 subSec.collapsed = subSec.defaultCollapsed (model half: calc:BM-A)
                // L50-55
                let (sub_label, sub_click) = (subSec.clone(), subSec.clone());
                let toggle = cx
                    .add(
                        me,
                        format!("toggle{i}"),
                        ButtonControl::new(
                            Anchor::new(TopRight, me, TopRight),
                            rect(-3, -13 + (16 * i as i32), 16, 16),
                            Prop::f(move |_, _| {
                                // L51
                                if sub_label.truthy(n!("collapsed")) {
                                    "+".to_owned()
                                } else {
                                    "-".to_owned()
                                }
                            }),
                            move |cx| {
                                // L53
                                let c = sub_click.truthy(n!("collapsed"));
                                sub_click.set(n!("collapsed"), !c);
                                // L54
                                let tab = st(cx, me).calcs_tab;
                                CalcsTab::set_mod_flag_of(cx, tab, true);
                            },
                        ),
                    )
                    .id();
                // L56
                if i == 1 {
                    // L57-59
                    cx.base_mut(toggle).shown = Prop::f(move |cx, _| enabled_raw(cx, me));
                } else {
                    // L61-63
                    let sub_section = sub_section.clone();
                    cx.base_mut(toggle).shown = Prop::f(move |cx, _| {
                        enabled_raw(cx, me) && !first_sub(&sub_section).truthy(n!("collapsed"))
                    });
                }
            }
            // L66-68
            let pop_out = cx
                .add(
                    me,
                    "popOut",
                    ButtonControl::new(
                        Anchor::new(TopRight, me, TopRight),
                        rect(-22, 3, 16, 16),
                        "^",
                        move |cx| {
                            // L67
                            CalcSectionControl::toggle_overlay(cx, me);
                        },
                    ),
                )
                .id();
            // L69-71
            cx.base_mut(pop_out).shown = Prop::f(move |cx, _| {
                // L70
                let s = st(cx, me);
                enabled_raw(cx, me) && !s.is_overlay && !s.has_controls
            });
            // L78-80
            cx.base_mut(me).shown = Prop::f(move |cx, me| {
                // L79
                enabled_raw(cx, me) && !st(cx, me).is_overlay
            });
        })
    }

    /// lua: CalcSectionControl.lua:84-112 `IsMouseOver()`: `(mOver, colData)`.
    /// @pob Classes/CalcSectionControl.lua:84-112 h=2415e00d
    pub fn is_mouse_over_comp(cx: &Cx, me: CtrlId) -> (bool, Option<Table>) {
        // L85
        if !cx.is_shown(me) {
            // L86
            return (false, None);
        }
        // L88
        if cx.get_mouse_over_control(me).is_some() {
            // L89
            return (true, None);
        }
        // L91
        let mOver = cx.is_mouse_in_bounds(me);
        let sub_section = st(cx, me).sub_section.clone();
        // L92
        for (_, subSec) in sub_section.ipairs() {
            let subSec = tbl(&subSec);
            // L93
            if mOver && !subSec.truthy(n!("collapsed")) && enabled_raw(cx, me) {
                // Check if mouse is over one of the cells
                // L95
                let (cursorX, cursorY) = cx.sys.cursor_pos();
                // L96
                for (_, data) in sub_data(&subSec).ipairs() {
                    let data = tbl(&data);
                    // L97
                    if data.truthy(n!("enabled")) {
                        // L98
                        for (_, colData) in data.ipairs() {
                            let colData = tbl(&colData);
                            // L99
                            if let Some(control) = colData.obj::<CellControl>(n!("control")) {
                                // L100
                                if cx.is_mouse_over(control.0) {
                                    // L101
                                    return (mOver, Some(colData));
                                }
                            // L103
                            } else if cursorX >= colData.num(n!("x"))
                                && cursorY >= colData.num(n!("y"))
                                && cursorX < colData.num(n!("x")) + colData.num(n!("width"))
                                && cursorY < colData.num(n!("y")) + colData.num(n!("height"))
                            {
                                // L104
                                return (mOver, Some(colData));
                            }
                        }
                    }
                }
            }
        }
        // L111
        (mOver, None)
    }

    /// lua: CalcSectionControl.lua:114-163 `UpdateSize()`
    /// @pob Classes/CalcSectionControl.lua:114-163 h=9ee8ac11
    pub fn update_size(cx: &mut Cx, me: CtrlId) {
        let flags = st(cx, me).flags.clone();
        // L115
        let enabled = calcs::check_flag(cx, &flags, None, None);
        cx.base_mut(me).enabled = Prop::Val(enabled);
        // L116
        if !enabled {
            // L117
            cx.base_mut(me).height = Prop::Val(22.0);
            // L118
            return;
        }
        // L120
        let (_x, _y) = cx.pos(me);
        // L121
        let (width, _) = cx.size(me);
        // L122
        let mut height = 2.0;
        // L123
        cx.base_mut(me).enabled = Prop::Val(false);
        // L124
        let mut yOffset = 0.0;
        let sub_section = st(cx, me).sub_section.clone();
        // L125
        for (i, subSec) in sub_section.ipairs() {
            let subSec = tbl(&subSec);
            // L126
            let toggle = cx
                .child(me, format!("toggle{i}"))
                .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
            cx.base_mut(toggle).y = Prop::Val(yOffset + 3.0);
            // L127
            let mut tempHeight = 0.0;
            // L128
            yOffset += 22.0;
            let data = sub_data(&subSec);
            // L129
            for (_, rowData) in data.ipairs() {
                let rowData = tbl(&rowData);
                // L130
                let row_enabled = calcs::check_flag(cx, &rowData, None, None);
                rowData.set(
                    n!("enabled"),
                    if row_enabled {
                        Val::Bool(true)
                    } else {
                        Val::Nil
                    },
                );
                // L131
                if row_enabled {
                    // L132
                    cx.base_mut(me).enabled = Prop::Val(true);
                    // L133
                    let mut xOffset = 134.0;
                    // L134
                    for (_, colData) in rowData.ipairs() {
                        let colData = tbl(&colData);
                        // L135
                        colData.set(n!("xOffset"), xOffset);
                        // L136
                        colData.set(n!("yOffset"), yOffset);
                        // L137
                        let w = data.get(n!("colWidth"));
                        let w = if w.truthy() {
                            w
                        } else {
                            Val::Num(width - 136.0)
                        };
                        colData.set(n!("width"), w);
                        // L138
                        colData.set(n!("height"), 18.0);
                        // L139
                        xOffset += colData.num(n!("width"));
                    }
                    // L141
                    yOffset += 18.0;
                    // L142
                    height += 18.0;
                    // L143
                    tempHeight += 18.0;
                }
                // L145
                if subSec.truthy(n!("collapsed")) {
                    // L146
                    rowData.set(n!("enabled"), false);
                }
            }
            // L149
            if enabled_raw(cx, me) && !subSec.truthy(n!("collapsed")) {
                // L150
                height += 22.0;
            } else {
                // L152
                height = height - tempHeight + 20.0;
                // L153
                yOffset = yOffset - tempHeight - 2.0;
            }
        }
        cx.base_mut(me).height = Prop::Val(height);
        // L156
        if enabled_raw(cx, me) && !first_sub(&sub_section).truthy(n!("collapsed")) {
            // L157
            if let Some(f) = st(cx, me).update_func.clone() {
                // L158
                f(cx, me);
            }
        } else {
            // L161
            cx.base_mut(me).height = Prop::Val(22.0);
        }
    }

    /// lua: CalcSectionControl.lua:165-185 `UpdatePos()`
    /// @pob Classes/CalcSectionControl.lua:165-185 h=630d92b9
    pub fn update_pos(cx: &mut Cx, me: CtrlId) {
        // L166
        if !enabled_raw(cx, me) {
            // L167
            return;
        }
        // L169
        let (x, y) = cx.pos(me);
        let sub_section = st(cx, me).sub_section.clone();
        // L170
        for (_, subSec) in sub_section.ipairs() {
            let subSec = tbl(&subSec);
            // L171
            for (_, rowData) in sub_data(&subSec).ipairs() {
                let rowData = tbl(&rowData);
                // L172
                if rowData.truthy(n!("enabled")) {
                    // L173
                    for (_, colData) in rowData.ipairs() {
                        let colData = tbl(&colData);
                        // Update the real coordinates of this cell
                        // L175
                        colData.set(n!("x"), x + colData.num(n!("xOffset")));
                        // L176
                        colData.set(n!("y"), y + colData.num(n!("yOffset")));
                        // L177
                        if let Some(control) = colData.obj::<CellControl>(n!("control")) {
                            // L178
                            cx.base_mut(control.0).x = Prop::Val(colData.num(n!("x")) + 4.0);
                            // L179 (raw `colData.control.height`)
                            let h = cx.base(control.0).height.raw();
                            cx.base_mut(control.0).y =
                                Prop::Val(colData.num(n!("y")) + 9.0 - h / 2.0);
                        }
                    }
                }
            }
        }
    }

    /// The actor the section shows: `calcsTab.input.showMinion and calcsTab.calcsEnv.minion or
    /// calcsTab.calcsEnv.player` (L190, L388).
    fn actor(cx: &Cx) -> ActorRef {
        let env = calcs::calcs_env(cx)
            .unwrap_or_else(|| lua_error!("attempt to index field 'calcsEnv' (a nil value)"));
        calcs::input_actor(cx, &env)
    }

    /// lua: CalcSectionControl.lua:187-199 `Draw(viewPort, noTooltip)`
    /// @pob Classes/CalcSectionControl.lua:187-199 h=200cf474
    pub fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        // L188
        let (x, y) = cx.pos(me);
        // L189
        let (width, height) = cx.size(me);
        // L190
        let actor = Self::actor(cx);
        let colour = st(cx, me).colour.clone();
        // Draw border and background
        // L192
        cx.sg.set_draw_layer(None, Some(-10));
        // L193
        cx.sg.set_draw_color_esc(&colour);
        // L194
        cx.sg.draw_image(None, x, y, width, height);
        // L195
        cx.sg.set_draw_color(0.10, 0.10, 0.10);
        // L196
        cx.sg
            .draw_image(None, x + 2.0, y + 2.0, width - 4.0, height - 4.0);

        // L198
        Self::draw_content(cx, me, x, y, width, Some(&actor), vp, false, no_tooltip);
    }

    /// lua: CalcSectionControl.lua:201-221 `OnKeyDown(key, doubleClick)`
    /// @pob Classes/CalcSectionControl.lua:201-221 h=4a205bd3
    pub fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, _double_click: bool) -> Focus {
        // L202
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L203
            return None;
        }
        // L205
        let mOverControl = cx.get_mouse_over_control(me);
        // L206
        if let Some(m) = mOverControl.filter(|&m| cx.has(m, Caps::KEY_DOWN)) {
            // L207 (`OnKeyDown(key)`: no doubleClick)
            return cx.on_key_down(m, key, false);
        }
        // L209
        let (mOver, mOverComp) = Self::is_mouse_over_comp(cx, me);
        // L210
        if key.name().contains("BUTTON") {
            // L211
            if !mOver {
                // L212
                return None;
            }
            // L214
            if let Some(comp) = mOverComp {
                // Pin the stat breakdown
                let tab = st(cx, me).calcs_tab;
                // L216
                CalcsTab::set_display_stat(cx, tab, Some(comp), true);
                // L217
                return Some(CalcsTab::breakdown(cx, tab));
            }
        }
        // L220
        None
    }

    /// lua: CalcSectionControl.lua:223-232 `OnKeyUp(key)`
    /// @pob Classes/CalcSectionControl.lua:223-232 h=ffee2030
    pub fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        // L224
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L225
            return None;
        }
        // L227
        let mOverControl = cx.get_mouse_over_control(me);
        // L228
        if let Some(m) = mOverControl.filter(|&m| cx.has(m, Caps::KEY_UP)) {
            // L229
            return cx.on_key_up(m, key);
        }
        // L231
        None
    }

    /// `pane == self` in `build.overlayPanes` (identity).
    fn is_self(pane: &Rc<dyn OverlayPane>, mine: &Rc<SectionPane>) -> bool {
        std::ptr::addr_eq(Rc::as_ptr(pane), Rc::as_ptr(mine))
    }

    /// lua: CalcSectionControl.lua:234-251 `ToggleOverlay()`
    /// @pob Classes/CalcSectionControl.lua:234-251 h=30f0a6d5
    pub fn toggle_overlay(cx: &mut Cx, me: CtrlId) {
        // L235
        let o = !st(cx, me).is_overlay;
        st_mut(cx, me).is_overlay = o;
        // L236
        if o {
            // L237
            let (x, y) = cx.pos(me);
            // L238
            st_mut(cx, me).overlay_x = x;
            // L239
            st_mut(cx, me).overlay_y = y;
            // L240
            let pane: Rc<dyn OverlayPane> = st(cx, me).pane.clone();
            on_frame::push_overlay_pane(cx, pane);
        } else {
            let mine = st(cx, me).pane.clone();
            // L242
            let panes = on_frame::overlay_panes_mut(cx);
            // L243
            if let Some(i) = panes.iter().position(|p| Self::is_self(p, &mine)) {
                // L245
                panes.remove(i);
                // L246
            }
            // L249
            st_mut(cx, me).dragging = false;
        }
    }

    /// lua: CalcSectionControl.lua:253-262 `RaiseOverlay()`
    /// @pob Classes/CalcSectionControl.lua:253-262 h=ced4a0e8
    pub fn raise_overlay(cx: &mut Cx, me: CtrlId) {
        let mine = st(cx, me).pane.clone();
        // L254
        let panes = on_frame::overlay_panes_mut(cx);
        // L255
        if let Some(i) = panes.iter().position(|p| Self::is_self(p, &mine)) {
            // L257
            let p = panes.remove(i);
            // L258
            panes.push(p);
            // L259
        }
    }

    /// lua: CalcSectionControl.lua:264-270 `IsMouseInOverlay(cursorX, cursorY)`
    /// @pob Classes/CalcSectionControl.lua:264-270 h=e96c7d80
    pub fn is_mouse_in_overlay(cx: &Cx, me: CtrlId, cursorX: f64, cursorY: f64) -> bool {
        // L265
        if !st(cx, me).is_overlay || calcs::calcs_env(cx).is_none() {
            return false;
        }
        // L266
        let x = st(cx, me).overlay_x;
        // L267
        let y = st(cx, me).overlay_y;
        // L268 (raw `self.width`)
        let width = cx.base(me).width.raw();
        if cursorX < x || cursorX > x + width || cursorY < y {
            return false;
        }
        // L269
        cursorY < y + Self::get_overlay_height(cx, me)
    }

    /// lua: CalcSectionControl.lua:272-289 `GetOverlayHeight()`
    /// @pob Classes/CalcSectionControl.lua:272-289 h=256f9bb9
    pub fn get_overlay_height(cx: &Cx, me: CtrlId) -> f64 {
        // L273
        let mut height = 28.0;
        let flags = st(cx, me).flags.clone();
        // L274
        let enabled = calcs::calcs_env(cx).is_some() && calcs::check_flag(cx, &flags, None, None);
        let sub_section = st(cx, me).sub_section.clone();
        // L275
        for (i, subSec) in sub_section.ipairs() {
            let subSec = tbl(&subSec);
            // L276
            height += 22.0;
            // L277
            if !subSec.truthy(n!("collapsed")) && enabled {
                // L278
                for (_, rowData) in sub_data(&subSec).ipairs() {
                    let rowData = tbl(&rowData);
                    // L279
                    if calcs::check_flag(cx, &rowData, None, None) {
                        // L280
                        height += 18.0;
                    }
                }
                // L283
                height += 2.0;
            } else if i == 1 {
                // L284-285
                break;
            }
        }
        // L288
        height
    }

    /// lua: CalcSectionControl.lua:291-360 `HandleOverlayClick(key, cursorX, cursorY)`
    /// @pob Classes/CalcSectionControl.lua:291-360 h=2493b5a7
    pub fn handle_overlay_click(cx: &mut Cx, me: CtrlId, key: Key, cursorX: f64, cursorY: f64) {
        // L292
        if key != Key::LeftButton {
            return;
        }

        // L294
        Self::raise_overlay(cx, me);

        // L296
        let x = st(cx, me).overlay_x;
        // L297
        let y = st(cx, me).overlay_y;
        // L298 (raw `self.width`)
        let overlayWidth = cx.base(me).width.raw();

        // Check close button
        // L301
        let closeX = x + overlayWidth - 18.0;
        // L302
        let closeY = y + 2.0;
        // L303
        if cursorX >= closeX
            && cursorX <= closeX + 14.0
            && cursorY >= closeY
            && cursorY <= closeY + 16.0
        {
            // L304
            Self::toggle_overlay(cx, me);
            // L305
            return;
        }

        // L308
        let mut lineY = y + 26.0;
        // L309
        let mut primary = true;
        let sub_section = st(cx, me).sub_section.clone();
        let flags = st(cx, me).flags.clone();
        // L310
        for (_, subSec) in sub_section.ipairs() {
            let subSec = tbl(&subSec);
            // L311
            let secEnabled = calcs::check_flag(cx, &flags, None, None);

            // Check subsection toggle
            // L314
            if secEnabled {
                // L315
                let toggleX = x + overlayWidth - 18.0;
                // L316
                let toggleY = lineY + 3.0;
                // L317
                if cursorX >= toggleX
                    && cursorX <= toggleX + 14.0
                    && cursorY >= toggleY
                    && cursorY <= toggleY + 16.0
                {
                    // L318
                    let c = subSec.truthy(n!("collapsed"));
                    subSec.set(n!("collapsed"), !c);
                    // L319
                    st_mut(cx, me).overlay_revision = None;
                    // L320
                    let tab = st(cx, me).calcs_tab;
                    CalcsTab::set_mod_flag_of(cx, tab, true);
                    // L321
                    return;
                }
            }

            // L325
            if subSec.truthy(n!("collapsed")) || !secEnabled {
                // L326
                if primary {
                    // L327
                    break;
                } else {
                    // L329
                    lineY += 20.0;
                    // L330
                    primary = false;
                }
            } else {
                // L333
                lineY += 20.0;
                // L334
                primary = false;
                // L335
                for (_, rowData) in sub_data(&subSec).ipairs() {
                    let rowData = tbl(&rowData);
                    // L336
                    if calcs::check_flag(cx, &rowData, None, None) {
                        // L337
                        for (_, colData) in rowData.ipairs() {
                            let colData = tbl(&colData);
                            // L338
                            let xo = colData.get(n!("xOffset"));
                            let cellX = x + if xo.truthy() { xo.num() } else { 134.0 };
                            // L339
                            let w = colData.get(n!("width"));
                            let cellW = if w.truthy() {
                                w.num()
                            } else {
                                overlayWidth - 136.0
                            };
                            // L340
                            if cursorX >= cellX
                                && cursorX <= cellX + cellW
                                && cursorY >= lineY + 2.0
                                && cursorY <= lineY + 19.0
                            {
                                // L341
                                if colData.truthy(n!("format"))
                                    && calcs::check_flag(cx, &colData, None, None)
                                {
                                    // L342
                                    let tab = st(cx, me).calcs_tab;
                                    CalcsTab::set_display_stat(cx, tab, Some(colData), true);
                                }
                                // L344
                                return;
                            }
                        }
                        // L347
                        lineY += 18.0;
                    }
                }
                // L350
                lineY += 2.0;
            }
        }

        // Start dragging
        // L355
        if cursorY >= y && cursorY <= y + 24.0 {
            let s = st_mut(cx, me);
            // L356
            s.dragging = true;
            // L357
            s.drag_off_x = cursorX - s.overlay_x;
            // L358
            s.drag_off_y = cursorY - s.overlay_y;
        }
    }

    /// lua: CalcSectionControl.lua:362-366 `HandleOverlayRelease(key)`
    /// @pob Classes/CalcSectionControl.lua:362-366 h=cf7256af
    pub fn handle_overlay_release(cx: &mut Cx, me: CtrlId, key: Key) {
        // L363
        if key == Key::LeftButton {
            // L364
            st_mut(cx, me).dragging = false;
        }
    }

    /// lua: CalcSectionControl.lua:368-435 `DrawOverlay(viewPort, inputEvents)`
    /// @pob Classes/CalcSectionControl.lua:368-435 h=9822f9c8
    pub fn draw_overlay(cx: &mut Cx, me: CtrlId, vp: &ViewPort, _input_events: &mut InputEvents) {
        // L369
        let (cursorX, cursorY) = cx.sys.cursor_pos();
        // L370
        st_mut(cx, me).overlay_breakdown_cell = false;

        let rev = cx.build().model.outputRevision.get();
        // L372
        if st(cx, me).overlay_revision != Some(rev) {
            // L373
            Self::update_size(cx, me);
            // L374
            st_mut(cx, me).overlay_revision = Some(rev);
        }

        // L377
        if st(cx, me).dragging {
            let s = st_mut(cx, me);
            // L378
            s.overlay_x = cursorX - s.drag_off_x;
            // L379
            s.overlay_y = cursorY - s.drag_off_y;
        }

        // L382-383 (raw `self.width`)
        let width = cx.base(me).width.raw();
        {
            let s = st_mut(cx, me);
            s.overlay_x = m_max(vp.x, m_min(s.overlay_x, vp.x + vp.width - width));
            s.overlay_y = m_max(vp.y, m_min(s.overlay_y, vp.y + vp.height - 24.0));
        }

        // L385
        let x = st(cx, me).overlay_x;
        // L386
        let y = st(cx, me).overlay_y;
        // L387
        let overlayWidth = width;
        // L388
        let actor = calcs::calcs_env(cx).map(|env| calcs::input_actor(cx, &env));

        // Calculate content height
        // L391
        let totalHeight = Self::get_overlay_height(cx, me);

        // Ensure it's above most other controls
        // L394
        cx.sg.set_draw_layer(Some(10), None);

        // Draw background
        // L397
        cx.sg.set_draw_color_a(0.08, 0.08, 0.08, 0.95);
        // L398
        cx.sg.draw_image(None, x, y, overlayWidth, totalHeight);

        let colour = st(cx, me).colour.clone();
        // Draw border
        // L401
        cx.sg.set_draw_color_esc(&colour);
        // L402
        cx.sg.draw_image(None, x, y, overlayWidth, 1.0);
        // L403
        cx.sg
            .draw_image(None, x, y + totalHeight - 1.0, overlayWidth, 1.0);
        // L404
        cx.sg.draw_image(None, x, y, 1.0, totalHeight);
        // L405
        cx.sg
            .draw_image(None, x + overlayWidth - 1.0, y, 1.0, totalHeight);

        // Draw header
        // L408
        cx.sg.set_draw_color(0.18, 0.18, 0.18);
        // L409
        cx.sg
            .draw_image(None, x + 1.0, y + 1.0, overlayWidth - 2.0, 22.0);
        // L410
        cx.sg.set_draw_color(1.0, 1.0, 1.0);
        // L411
        let cbx = x + overlayWidth - 16.0;
        // L412
        let cby = y + 3.0;
        // L413
        cx.sg.draw_image_quad(
            None,
            [
                cbx + 3.0,
                cby + 5.0,
                cbx + 5.0,
                cby + 3.0,
                cbx + 13.0,
                cby + 11.0,
                cbx + 11.0,
                cby + 13.0,
            ],
        );
        // L414
        cx.sg.draw_image_quad(
            None,
            [
                cbx + 11.0,
                cby + 3.0,
                cbx + 13.0,
                cby + 5.0,
                cbx + 5.0,
                cby + 13.0,
                cbx + 3.0,
                cby + 11.0,
            ],
        );

        // Separator
        // L417
        cx.sg.set_draw_color_esc(&colour);
        // L418
        cx.sg
            .draw_image(None, x + 2.0, y + 24.0, overlayWidth - 4.0, 1.0);

        // Draw content
        // L421
        Self::draw_content(
            cx,
            me,
            x,
            y + 26.0,
            overlayWidth,
            actor.as_ref(),
            vp,
            true,
            false,
        );

        // Draw stat breakdown
        let tab = st(cx, me).calcs_tab;
        let (display_data, display_pinned) = CalcsTab::display(cx, tab);
        // L424
        if let Some(cd) = display_data.filter(|cd| {
            st(cx, me).overlay_breakdown_cell
                || (display_pinned
                    && cd
                        .obj::<CalcSectionRef>(n!("calcSection"))
                        .is_some_and(|s| s.0 == me))
        }) {
            // L425
            // L426
            let (origX, origY) = (cd.get(n!("x")), cd.get(n!("y")));
            // L427
            let xo = cd.get(n!("xOffset"));
            cd.set(n!("x"), x + if xo.truthy() { xo.num() } else { 134.0 });
            // L428
            let yo = cd.get(n!("yOffset"));
            cd.set(n!("y"), y + 26.0 + if yo.truthy() { yo.num() } else { 0.0 });
            // L429 (`Draw(viewPort)`: noTooltip nil)
            let bd = CalcsTab::breakdown(cx, tab);
            cx.draw(bd, vp, false);
            // L430
            cd.set(n!("x"), origX);
            cd.set(n!("y"), origY);
        }
        // L432
        st_mut(cx, me).overlay_breakdown_cell = false;

        // L434
        cx.sg.set_draw_layer(Some(0), None);
    }

    /// lua: CalcSectionControl.lua:437-551 `DrawContent(drawX, startLineY, drawWidth, actor,
    /// viewPort, isOverlay, noTooltip)`
    /// @pob Classes/CalcSectionControl.lua:437-551 h=f07ac0df
    #[allow(clippy::too_many_lines)]
    pub fn draw_content(
        cx: &mut Cx,
        me: CtrlId,
        drawX: f64,
        startLineY: f64,
        drawWidth: f64,
        actor: Option<&ActorRef>,
        vp: &ViewPort,
        isOverlay: bool,
        noTooltip: bool,
    ) {
        // L438
        let (cursorX, cursorY) = cx.sys.cursor_pos();
        // L439
        let mut lineY = startLineY;
        // L440
        let mut primary = true;
        let flags = st(cx, me).flags.clone();
        // L441 (`isOverlay and (actor and CheckFlag(self)) or self.enabled`)
        let enabled = if isOverlay && actor.is_some() && calcs::check_flag(cx, &flags, None, None) {
            true
        } else {
            enabled_raw(cx, me)
        };
        let tab = st(cx, me).calcs_tab;
        let colour = st(cx, me).colour.clone();
        let sub_section = st(cx, me).sub_section.clone();

        // L443
        for (_, subSec) in sub_section.ipairs() {
            let subSec = tbl(&subSec);
            let label = subSec.get(n!("label"));
            // `subSec.label or ""`
            let label_or_empty = if label.truthy() {
                concat(&label)
            } else {
                String::new()
            };
            // Top border
            // L445
            cx.sg.set_draw_color_esc(&colour);
            // L446
            cx.sg
                .draw_image(None, drawX + 2.0, lineY, drawWidth - 4.0, 2.0);

            // Label
            // L449
            if !enabled {
                // L450
                let lx = if isOverlay { drawX + 4.0 } else { drawX + 3.0 };
                // L451
                cx.sg.draw_string(
                    lx,
                    lineY + 3.0,
                    Align::Left,
                    16.0,
                    Font::VarBold,
                    &format!("^8{label_or_empty}"),
                );
            } else {
                // L453
                let lx = if isOverlay { drawX + 4.0 } else { drawX + 3.0 };
                // L454
                let mut textColor = "^7".to_owned();
                // L455 (`SearchMatch(subSec.label)`: `txt:lower()` of a nil label raises)
                if !isOverlay && CalcsTab::search_match(cx, tab, &concat(&label)) {
                    // L456
                    textColor = cx.main.cc.HIGHLIGHT.clone();
                }
                // L458
                cx.sg.draw_string(
                    lx,
                    lineY + 3.0,
                    Align::Left,
                    16.0,
                    Font::VarBold,
                    &format!("{textColor}{label_or_empty}:"),
                );
                let extra = sub_data(&subSec).get(n!("extra"));
                // L459
                if extra.truthy() {
                    // L460
                    let ex = lx
                        + cx.sg
                            .draw_string_width(16.0, Font::VarBold, &concat(&label))
                        + if isOverlay { 12.0 } else { 10.0 };
                    // L461
                    let text = formatCalcStr(&concat(&extra), actor, &Val::Nil);
                    cx.sg.draw_string(
                        ex,
                        lineY + 3.0,
                        Align::Left,
                        16.0,
                        Font::Var,
                        &format!("^7{text}"),
                    );
                }
            }

            // Bottom border
            // L466
            cx.sg.set_draw_color_esc(&colour);
            // L467
            cx.sg
                .draw_image(None, drawX + 2.0, lineY + 20.0, drawWidth - 4.0, 2.0);

            // Toggle
            // L470
            if isOverlay {
                // L471
                if enabled {
                    // L472
                    let sign = if subSec.truthy(n!("collapsed")) {
                        "+"
                    } else {
                        "-"
                    };
                    cx.sg.draw_string(
                        drawX + drawWidth - 16.0,
                        lineY + 3.0,
                        Align::Left,
                        14.0,
                        Font::Var,
                        &format!("^7{sign}"),
                    );
                }
            } else {
                // L475
                cx.sg.set_draw_layer(None, Some(0));
                // L476 (`noTooltip and self.calcsTab.selControl`)
                let sel = if noTooltip {
                    cx.host(tab).sel_control
                } else {
                    None
                };
                cx.draw_controls(me, vp, sel);
            }

            // L479
            if subSec.truthy(n!("collapsed")) || !enabled {
                // L480
                if primary {
                    // L481
                    return;
                } else {
                    // L483
                    lineY += 20.0;
                    // L484
                    primary = false;
                }
            } else {
                // L487
                lineY += 20.0;
                // L488
                primary = false;
                // L489
                let mut rows = 0;
                // L490
                for (_, rowData) in sub_data(&subSec).ipairs() {
                    let rowData = tbl(&rowData);
                    // L491
                    let row_shown = if isOverlay {
                        actor.is_some() && calcs::check_flag(cx, &rowData, None, None)
                    } else {
                        rowData.truthy(n!("enabled"))
                    };
                    if row_shown {
                        // L492
                        rows += 1;
                        let row_label = rowData.get(n!("label"));
                        // L493
                        if !isOverlay {
                            // L494
                            let rc = rowData.get(n!("color"));
                            let mut textColor = if rc.truthy() {
                                concat(&rc)
                            } else {
                                "^7".to_owned()
                            };
                            // L495
                            if row_label.truthy() {
                                // L496
                                set_color_val(cx, &rowData.get(n!("bgCol")));
                                // L497
                                cx.sg
                                    .draw_image(None, drawX + 2.0, lineY + 2.0, 130.0, 18.0);
                                let row_label = concat(&row_label);
                                // L498
                                if CalcsTab::search_match(cx, tab, &row_label) {
                                    // L499
                                    textColor = cx.main.cc.HIGHLIGHT.clone();
                                }
                                // L501
                                cx.sg.draw_string(
                                    drawX + 132.0,
                                    lineY + 2.0,
                                    Align::RightX,
                                    16.0,
                                    Font::Var,
                                    &format!("{textColor}{row_label}^7:"),
                                );
                            }
                        // L503
                        } else if row_label.truthy() {
                            // L504
                            cx.sg.draw_string(
                                drawX + 132.0,
                                lineY + 2.0,
                                Align::RightX,
                                16.0,
                                Font::Var,
                                &format!("^7{}^7:", concat(&row_label)),
                            );
                        }
                        // L506
                        for (_, colData) in rowData.ipairs() {
                            let colData = tbl(&colData);
                            // L507
                            let cellX = if isOverlay {
                                let xo = colData.get(n!("xOffset"));
                                drawX + if xo.truthy() { xo.num() } else { 134.0 }
                            } else {
                                colData.num(n!("x"))
                            };
                            // L508
                            let cellW = if isOverlay {
                                let w = colData.get(n!("width"));
                                if w.truthy() {
                                    w.num()
                                } else {
                                    drawWidth - 136.0
                                }
                            } else {
                                colData.num(n!("width"))
                            };
                            // L509
                            let cellY = if isOverlay {
                                lineY + 2.0
                            } else {
                                colData.num(n!("y"))
                            };
                            // L510
                            let h = colData.get(n!("height"));
                            let cellH = if h.truthy() { h.num() } else { 18.0 };

                            // L512
                            cx.sg.set_draw_color_esc(&colour);
                            // L513
                            cx.sg.draw_image(None, cellX, lineY + 2.0, 2.0, cellH);

                            let format = colData.get(n!("format"));
                            // L515
                            if format.truthy()
                                && (isOverlay || calcs::check_flag(cx, &colData, None, None))
                            {
                                // L516
                                if (isOverlay
                                    && actor.is_some()
                                    && calcs::check_flag(cx, &colData, None, None))
                                    || !isOverlay
                                {
                                    // L517
                                    if isOverlay {
                                        // L518
                                        if cursorY >= lineY + 2.0
                                            && cursorY < lineY + 20.0
                                            && cursorX >= cellX
                                            && cursorX < cellX + cellW
                                        {
                                            // L519
                                            CalcsTab::set_display_stat(
                                                cx,
                                                tab,
                                                Some(colData.clone()),
                                                false,
                                            );
                                            // L520
                                            st_mut(cx, me).overlay_breakdown_cell = true;
                                        }
                                    // L522
                                    } else if cursorY >= vp.y
                                        && cursorY < vp.y + vp.height
                                        && cursorX >= cellX
                                        && cursorY >= cellY
                                        && cursorX < cellX + cellW
                                        && cursorY < cellY + cellH
                                    {
                                        // L523
                                        CalcsTab::set_display_stat(
                                            cx,
                                            tab,
                                            Some(colData.clone()),
                                            false,
                                        );
                                    }

                                    let is_display = CalcsTab::display(cx, tab)
                                        .0
                                        .is_some_and(|d| d.ptr_eq(&colData));
                                    // L526
                                    if !isOverlay && is_display {
                                        // L527
                                        cx.sg.set_draw_color(0.25, 1.0, 0.25);
                                        // L528
                                        cx.sg.draw_image(
                                            None,
                                            cellX + 2.0,
                                            cellY,
                                            cellW - 2.0,
                                            cellH,
                                        );
                                        // L529
                                        set_color_val(cx, &rowData.get(n!("bgCol")));
                                        // L530
                                        cx.sg.draw_image(
                                            None,
                                            cellX + 3.0,
                                            cellY + 1.0,
                                            cellW - 4.0,
                                            cellH - 2.0,
                                        );
                                    } else {
                                        // L532
                                        set_color_val(cx, &rowData.get(n!("bgCol")));
                                        // L533
                                        cx.sg.draw_image(
                                            None,
                                            cellX + 2.0,
                                            lineY + 2.0,
                                            cellW - 2.0,
                                            18.0,
                                        );
                                    }

                                    // L536
                                    let ts = rowData.get(n!("textSize"));
                                    let textSize = if ts.truthy() { ts.num() } else { 14.0 };
                                    // L537
                                    cx.sg.set_viewport(
                                        cellX + 3.0,
                                        if isOverlay { lineY + 2.0 } else { cellY },
                                        cellW - 4.0,
                                        18.0,
                                    );
                                    // L538
                                    let text = formatCalcStr(
                                        &concat(&format),
                                        actor,
                                        &Val::Table(colData.clone()),
                                    );
                                    cx.sg.draw_string(
                                        1.0,
                                        9.0 - textSize / 2.0,
                                        Align::Left,
                                        textSize,
                                        Font::Var,
                                        &format!("^7{text}"),
                                    );
                                    // L539
                                    cx.sg.reset_viewport();
                                }
                            }
                        }
                        // L543
                        lineY += 18.0;
                    }
                }
                // L546
                if rows > 0 {
                    // L547
                    lineY += 2.0;
                }
            }
        }
    }
}

impl OverlayPane for SectionPane {
    fn is_overlay(&self, cx: &Cx) -> bool {
        // `pane.isOverlay` (a pane whose section was freed reads as not an overlay)
        cx.try_w::<CalcSectionControl>(self.0)
            .is_some_and(|s| s.is_overlay)
    }
    fn is_mouse_in_overlay(&self, cx: &Cx, x: f64, y: f64) -> bool {
        CalcSectionControl::is_mouse_in_overlay(cx, self.0, x, y)
    }
    fn handle_overlay_click(&self, cx: &mut Cx, key: Key, x: f64, y: f64) {
        CalcSectionControl::handle_overlay_click(cx, self.0, key, x, y);
    }
    fn handle_overlay_release(&self, cx: &mut Cx, key: Key) {
        CalcSectionControl::handle_overlay_release(cx, self.0, key);
    }
    fn draw_overlay(&self, cx: &mut Cx, vp: &ViewPort, input_events: &mut InputEvents) {
        CalcSectionControl::draw_overlay(cx, self.0, vp, input_events);
    }
}
