//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ComparePowerReportListControl.lua 1-165.
//! Owner: U48.
//!
//! @pob-file Classes/ComparePowerReportListControl.lua @16de4b82d
//!
//! `newClass("ComparePowerReportListControl", "ListControl")`: the Compare tab's power report
//! list in the Summary view (CompareTab.lua:1037-1039 constructs it as
//! `controls.comparePowerReportList` and sets `.compareTab`; [`ctor_hook`] is that seam,
//! CCR-0136 §1). Its rows are the result entries `ComparePowerBuilder` makes
//! ([`super::power::PowerResult`]); `self.list` is a copy of `self.reportData` that `ReList`
//! makes, sorted in place by `ReSort`.
//!
//! ListControl hooks: `GetRowValue`, `AddValueTooltip`, `ReSort`; `Draw` is overridden.
//!
//! Lua semantics:
//! * `self.impactColumn` is the third `colList` entry itself (L17, L21): [`IMPACT_COLUMN`]; a
//!   write to `impactColumn.label` is a write to that column's label.
//! * `self.reportData` is the results list (`report or {}`); `ReList` copies its entries (the
//!   same entry objects, `Rc`s).
//! * `"Calculating... " .. progress .. "%"` concatenates a number with `tostring`.
//! * `a.category < b.category` compares strings bytewise (Lua `<` on strings, C locale).
//! * `tooltip:CheckForUpdate(node, IsKeyDown("SHIFT"), launch.devModeAlt, build.outputRevision)`
//!   keys the node by identity (`Rc<Node>`), the item likewise.
//!
//! ORDER: no `pairs` loops (`ipairs(self.reportData)` keeps the results' order). The sorts are
//! Lua 5.1's `auxsort` (`pob_lua::sort`, not stable): rows tied under a comparator keep an order
//! that follows the results' order (power.rs ORDER note).
//!
//! Props with side effects (ui doc §7.3): none.
#![allow(non_snake_case)]

use std::rc::Rc;

use pob_sg::{Align, Font};
use pob_ui_core::controls::list::{ListColumn, ListControl, ListData, ListHooks, ListValue};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::{Tooltip, TtKey, TtObj};

use super::power::PowerResult;
use crate::app::CxApp;
use crate::model::gamedata::schema::item_data::PowerStat;

// @pob Classes/ComparePowerReportListControl.lua:1-11 h=04e89997
// (header comment, the local aliases (pob_lua is used directly), newClass: the struct and the
// Widget impl below)

/// `self.impactColumn`: `colList[3]` (L17, L21).
pub const IMPACT_COLUMN: usize = 3;

/// The two status texts (L27, L39, L42).
const SELECT_METRIC: &str = "Select a metric above to generate the power report.";

/// `ComparePowerReportListControl`
pub struct ComparePowerReportListControl {
    pub list: ListControl,
    /// `self.statusText` (nil: `None`)
    pub status_text: Option<String>,
    /// `self.reportData` (nil until the first `SetReport`)
    pub report_data: Option<Rc<Vec<Rc<PowerResult>>>>,
    /// `self.lastTooltipIndex`
    pub last_tooltip_index: Option<usize>,
    /// `self.compareTab` (set by the CompareTab constructor, CompareTab.lua:1038)
    pub compare_tab: Option<CtrlId>,
}

static HOOKS: ListHooks = ListHooks {
    get_row_value: ComparePowerReportListControl::GetRowValue,
    add_value_tooltip: Some(ComparePowerReportListControl::AddValueTooltip),
    re_sort: Some(ComparePowerReportListControl::ReSort),
    ..ListHooks::BASE
};

/// The result entry of a row.
fn entry_of(v: &ListValue) -> Rc<PowerResult> {
    v.rc::<PowerResult>()
        .unwrap_or_else(|| lua_error!("ComparePowerReportListControl: a row that is not a result entry"))
}

impl ComparePowerReportListControl {
    /// lua: ComparePowerReportListControl.lua:13-29 `ComparePowerReportListClass:
    /// ComparePowerReportListControl(anchor, rect)`
    /// @pob Classes/ComparePowerReportListControl.lua:13-29 h=86c6b3fc
    pub fn new(anchor: Anchor, r: [Prop<f64>; 4]) -> New<ComparePowerReportListControl> {
        // L16
        let width = r[2].raw();
        New::new(
            Base::new(anchor, r),
            ComparePowerReportListControl {
                // L14 (`scroll = "VERTICAL"`: truthy, not horizontal)
                list: ListControl::state(&HOOKS, 18.0, true, false, ListData::default(), false),
                // L27
                status_text: Some(SELECT_METRIC.to_owned()),
                report_data: None,
                last_tooltip_index: None,
                compare_tab: None,
            },
        )
        .host()
        // L14 (the ListControl constructor's scroll bars)
        .init(|cx, me| ListControl::init(cx, me.0))
        .init(move |cx, me| {
            let l = cx.w_mut::<ListControl>(me.0);
            let col = |w: f64, label: &str, sortable: bool| ListColumn::new(Some(Prop::Val(w)), Some(label), sortable);
            // L17-24
            l.col_list = vec![
                col(width * 0.10, "Category", true),
                col(width * 0.44, "Name", false),
                // L17 self.impactColumn
                col(width * 0.22, "", true),
                col(width * 0.06, "Points", true),
                col(width * 0.14, "Per Point", true),
            ];
            // L25
            l.col_labels = true;
            // L26
            l.show_row_separators = true;
            // L28 return self
        })
    }

    /// `self.impactColumn.label = label`
    pub fn set_impact_label(cx: &mut Cx, me: CtrlId, label: &str) {
        cx.w_mut::<ListControl>(me).col_list[IMPACT_COLUMN - 1].label = Some(Prop::Val(label.to_owned()));
    }

    /// lua: ComparePowerReportListControl.lua:31-47 `ComparePowerReportListClass:SetReport(stat,
    /// report)`; `stat` is a `data.powerStatList` entry
    /// @pob Classes/ComparePowerReportListControl.lua:31-47 h=3cd701b6
    pub fn SetReport(cx: &mut Cx, me: CtrlId, stat: Option<&PowerStat>, report: Option<Rc<Vec<Rc<PowerResult>>>>) {
        // L32 (`stat.label` is always set)
        Self::set_impact_label(cx, me, stat.map_or("", |s| &*s.label));
        // L33
        let has_rows = report.as_ref().is_some_and(|r| !r.is_empty());
        cx.w_mut::<Self>(me).report_data = Some(report.unwrap_or_default());

        // L35
        let status = if stat.is_some_and(|s| s.stat.is_some()) {
            // L36
            if has_rows {
                // L37
                None
            } else {
                // L39
                Some("No differences found.".to_owned())
            }
        } else {
            // L42
            Some(SELECT_METRIC.to_owned())
        };
        cx.w_mut::<Self>(me).status_text = status;

        // L45
        Self::ReList(cx, me);
        // L46
        Self::ReSort(cx, me, 3);
    }

    /// lua: ComparePowerReportListControl.lua:49-54 `ComparePowerReportListClass:SetProgress(
    /// progress)`
    /// @pob Classes/ComparePowerReportListControl.lua:49-54 h=a378ab55
    pub fn SetProgress(cx: &mut Cx, me: CtrlId, progress: f64) {
        // L50
        if progress < 100.0 {
            // L51
            cx.w_mut::<Self>(me).status_text = Some(format!("Calculating... {}%", pob_lua::tostring_num(progress)));
            // L52
            cx.w_mut::<ListControl>(me).list = ListData::Local(Vec::new());
        }
    }

    /// lua: ComparePowerReportListControl.lua:56-72 `ComparePowerReportListClass:Draw(viewPort,
    /// noTooltip)`
    /// @pob Classes/ComparePowerReportListControl.lua:56-72 h=8a04858d
    fn draw_impl(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        // L57
        let hover = cx.w::<ListControl>(me).hover_index;
        if hover != cx.w::<Self>(me).last_tooltip_index {
            // L58
            cx.w_mut::<ListControl>(me).tooltip.update_params = None;
        }
        // L60
        cx.w_mut::<Self>(me).last_tooltip_index = hover;
        // L61
        ListControl::draw(cx, me, vp, no_tooltip);
        // Draw status text below column headers when the list is empty
        // L63
        // (`self.list` is always this control's own table)
        let empty = matches!(&cx.w::<ListControl>(me).list, ListData::Local(v) if v.is_empty());
        if let (true, Some(status)) = (empty, cx.w::<Self>(me).status_text.clone()) {
            // L64
            let (x, y) = cx.pos(me);
            // L65
            let (width, height) = cx.size(me);
            // Column headers are 18px tall, plus 2px border = start at y+20
            // L67
            cx.sg.set_viewport(x + 2.0, y + 20.0, width - 20.0, height - 22.0);
            // L68
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
            // L69
            cx.sg.draw_string(4.0, 4.0, Align::Left, 14.0, Font::Var, &status);
            // L70
            cx.sg.reset_viewport();
        }
    }

    /// lua: ComparePowerReportListControl.lua:74-104 `ComparePowerReportListClass:ReSort(
    /// colIndex)`
    /// @pob Classes/ComparePowerReportListControl.lua:74-104 h=8b84a4b8
    pub fn ReSort(cx: &mut Cx, me: CtrlId, colIndex: usize) {
        // L75
        let compare = |a: f64, b: f64| a > b;

        let ListData::Local(list) = &mut cx.w_mut::<ListControl>(me).list else {
            lua_error!("ComparePowerReportListControl: self.list is not the control's own table")
        };
        // L77
        if colIndex == 1 {
            // L78-83
            pob_lua::sort::sort(list, |a, b| {
                let (a, b) = (entry_of(a), entry_of(b));
                // L79
                if a.category == b.category {
                    // L80
                    return compare(a.impact.abs(), b.impact.abs());
                }
                // L82
                a.category.as_bytes() < b.category.as_bytes()
            });
        // L84
        } else if colIndex == 3 {
            // L85-87
            pob_lua::sort::sort(list, |a, b| {
                let (a, b) = (entry_of(a), entry_of(b));
                // L86
                compare(a.impact, b.impact)
            });
        // L88
        } else if colIndex == 4 {
            // L89-96
            pob_lua::sort::sort(list, |a, b| {
                let (a, b) = (entry_of(a), entry_of(b));
                // L90
                let aDist = a.pathDist.unwrap_or(99999.0);
                // L91
                let bDist = b.pathDist.unwrap_or(99999.0);
                // L92
                if aDist == bDist {
                    // L93
                    return compare(a.impact.abs(), b.impact.abs());
                }
                // L95
                aDist < bDist
            });
        // L97
        } else if colIndex == 5 {
            // L98-102
            pob_lua::sort::sort(list, |a, b| {
                let (a, b) = (entry_of(a), entry_of(b));
                // L99
                let aVal = a.perPoint.unwrap_or(-99999.0);
                // L100
                let bVal = b.perPoint.unwrap_or(-99999.0);
                // L101
                compare(aVal, bVal)
            });
        }
    }

    /// lua: ComparePowerReportListControl.lua:106-114 `ComparePowerReportListClass:ReList()`
    /// @pob Classes/ComparePowerReportListControl.lua:106-114 h=022e06f7
    pub fn ReList(cx: &mut Cx, me: CtrlId) {
        // L107
        cx.w_mut::<ListControl>(me).list = ListData::Local(Vec::new());
        // L108
        let Some(report) = cx.w::<Self>(me).report_data.clone() else {
            // L109
            return;
        };
        // L111
        let list: Vec<ListValue> = report.iter().map(|e| ListValue::of(e.clone())).collect();
        // L112
        cx.w_mut::<ListControl>(me).list = ListData::Local(list);
    }

    /// lua: ComparePowerReportListControl.lua:116-147 `ComparePowerReportListClass:
    /// AddValueTooltip(tooltip, index, entry)`
    /// @pob Classes/ComparePowerReportListControl.lua:116-147 h=39e83a36
    fn AddValueTooltip(cx: &mut Cx, me: CtrlId, tooltip: &mut Tooltip, _index: usize, v: &ListValue) {
        // L117
        if !cx.main.popups.is_empty() {
            // L118
            tooltip.clear(false);
            // L119
            return;
        }

        // L122 (`self.compareTab.primaryBuild`: the build of this app)
        if cx.w::<Self>(me).compare_tab.is_none() || cx.app().build.is_none() {
            // L124
            tooltip.clear(false);
            // L125
            return;
        }
        let build = cx.build().model.clone();
        let entry = entry_of(v);
        let shift = TtKey::Bool(cx.sys.is_key_down(pob_sg::Key::Shift));
        let dev_mode_alt = TtKey::Bool(cx.app().launch.dev_mode_alt);
        let rev = TtKey::Num(build.outputRevision.get());

        // L128
        if let (true, Some(nodeId)) = (entry.category == "Tree", entry.nodeId) {
            // L129
            let node = build.spec.get().nodes.with(|m| m.get(nodeId));
            // L130
            if let Some(node) = node {
                // L131
                let obj: Rc<dyn std::any::Any> = node.clone();
                if tooltip.check_for_update(cx, &[TtKey::Obj(TtObj(obj)), shift, dev_mode_alt, rev]) {
                    // L132-135 (`build.treeTab.viewer`)
                    if let Some(tree_tab) = crate::tabs::tree::core::tab(cx) {
                        let f = crate::tree_view::hooks().add_node_tooltip;
                        cx.lend::<crate::tabs::tree::TreeTab, _, _>(
                            tree_tab,
                            |w| &mut w.core.viewer,
                            |cx, viewer| f(cx, viewer, tooltip, &node),
                        );
                    }
                }
            } else {
                // L138
                tooltip.clear(false);
            }
        // L140
        } else if let (true, Some(item)) = (entry.category == "Item", entry.itemObj.clone()) {
            // L141
            let obj: Rc<dyn std::any::Any> = item.clone();
            if tooltip.check_for_update(cx, &[TtKey::Obj(TtObj(obj)), shift, dev_mode_alt, rev]) {
                // L142
                let h = crate::tabs::items::slot_helper::items_hooks();
                (h.add_item_tooltip)(cx, tooltip, &item, None, false);
            }
        } else {
            // L145
            tooltip.clear(false);
        }
    }

    /// lua: ComparePowerReportListControl.lua:149-165 `ComparePowerReportListClass:GetRowValue(
    /// column, index, entry)`
    /// @pob Classes/ComparePowerReportListControl.lua:149-165 h=6d9ba728
    fn GetRowValue(_cx: &Cx, _me: CtrlId, column: usize, _index: usize, v: &ListValue) -> String {
        let entry = entry_of(v);
        match column {
            // L151
            1 => format!("{}{}", entry.categoryColor, entry.category),
            // L153
            2 => format!("{}{}", entry.nameColor, entry.name),
            // L155 (`combinedImpactStr` is always set)
            3 => entry.combinedImpactStr.clone(),
            // L157-160
            4 => entry.pathDist.map_or_else(String::new, pob_lua::tostring_num),
            // L162
            5 => entry.perPointStr.clone().unwrap_or_default(),
            // L164
            _ => String::new(),
        }
    }
}

// ---- the CompareTab seam (CCR-0136 §1) -----------------------------------------------------------

/// `CompareHooks::power_report_list`: `self.controls.comparePowerReportList =
/// new("ComparePowerReportListControl"):ComparePowerReportListControl(nil, {0, 0, 750, 250})` and
/// `self.controls.comparePowerReportList.compareTab = self` (CompareTab.lua:1037-1038).
pub fn ctor_hook(cx: &mut Cx, compare_tab: CtrlId) -> CtrlId {
    // L1037
    let id = cx
        .add(
            compare_tab,
            "comparePowerReportList",
            ComparePowerReportListControl::new(Anchor::NONE, rect(0, 0, 750, 250)),
        )
        .id();
    // L1038
    cx.w_mut::<ComparePowerReportListControl>(id).compare_tab = Some(compare_tab);
    id
}

impl Widget for ComparePowerReportListControl {
    const NAME: &'static str = "ComparePowerReportListControl";
    const CAPS: Caps = <ListControl as Widget>::CAPS;

    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        Self::draw_impl(cx, me, vp, no_tooltip);
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

    pob_ui_core::impl_parts!(ComparePowerReportListControl: list => ListControl);
}
