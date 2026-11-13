//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/PowerReportListControl.lua 1-153.
//! Owner: U26.
//!
//! @pob-file Classes/PowerReportListControl.lua @16de4b82d
//!
//! `newClass("PowerReportListControl", "ListControl")`: the Tree tab's power report (TreeTab.lua
//! 279-288 constructs it as `treeTab.controls.powerReportList`). Its rows are the report entries
//! `TreeTab::BuildPowerReportList` (power.rs) builds ([`ReportEntry`]); `self.list` is the
//! filtered copy of `self.originalList` that `ReList` makes, sorted in place by `ReSort`.
//!
//! ListControl hooks: `GetRowValue`, `AddValueTooltip`, `OnSelClick`, `ReSort`.
//!
//! Lua semantics:
//! * `self.powerColumn` is the third `colList` entry itself (L18, L22): [`POWER_COLUMN`].
//! * `self.nodeSelectCallback(report)` (L130): the callback TreeTab passes (TreeTab.lua:279-287)
//!   reads only `selectedNode.x` / `selectedNode.y`, so it is U25's [`JumpFn`] and receives those
//!   two fields (`nil` positions are `NaN` in the model, CCR-0024 §2: `None`).
//! * `report.pathDist` is a number or the string `"Cluster"` ([`PathDist`]); `GetRowValue`'s
//!   `pathDist == 1000 and "Anoint"` and `ReSort`'s `== "Anoint"` tests are kept (no TreeTab report
//!   makes either).
//! * `GetRowValue` returns the number `report.pathDist` for column 4; ListControl draws it as
//!   `tostring` would (`pob_lua::tostring`).
//!
//! ORDER: no `pairs` loops (`ipairs(self.originalList)` keeps the report's order). Props with
//! side effects (ui doc §7.3): none.
#![allow(non_snake_case)]

use std::rc::Rc;

use pob_ui_core::controls::list::{ListColumn, ListControl, ListData, ListHooks, ListValue};
use pob_ui_core::controls::{CheckBoxControl, DropDownControl, DropItem};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::{Tooltip, TtKey, TtObj};

use super::core::JumpFn;
use crate::model::{LStr, Name, Shared, Table};

// @pob Classes/PowerReportListControl.lua:1-12 h=5936523d
// (header comment, the local aliases (pob_lua is used directly), newClass: the struct and the
// Widget impl below)

/// One entry of the report `TreeTab:BuildPowerReportList` returns (TreeTab.lua:1110-1124
/// `addReportEntry`): `{ name, power, powerStr, pathPower, pathPowerStr, allocated, id, x, y,
/// type, sd, pathDist }`.
pub struct ReportEntry {
    pub name: String,
    pub power: f64,
    pub powerStr: String,
    pub pathPower: f64,
    pub pathPowerStr: String,
    /// `allocated = isAlloc` (`node.alloc or grantedPassives[nodeId]`; nil and false are false)
    pub allocated: bool,
    pub id: f64,
    /// `node.x`, `node.y` (`nil` = `None`)
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub r#type: Name,
    /// `node.sd` (the node's own table, aliased)
    pub sd: Option<Shared<Vec<LStr>>>,
    pub pathDist: PathDist,
}

/// `report.pathDist`: a number of points, or a string.
#[derive(Clone, Debug, PartialEq)]
pub enum PathDist {
    Num(f64),
    Str(&'static str),
}

impl PathDist {
    /// `type(pathDist) == "number"`
    fn is_num(&self) -> bool {
        matches!(self, PathDist::Num(_))
    }
    /// `pathDist == s`
    fn is(&self, s: &str) -> bool {
        matches!(self, PathDist::Str(v) if *v == s)
    }
    /// `a.pathDist < b.pathDist` (Lua `<`: numbers or strings; mixed raises)
    fn lt(&self, o: &PathDist) -> bool {
        match (self, o) {
            (PathDist::Num(a), PathDist::Num(b)) => a < b,
            (PathDist::Str(a), PathDist::Str(b)) => a.as_bytes() < b.as_bytes(),
            _ => lua_error!("attempt to compare number with string"),
        }
    }
}

/// `self.powerColumn`: `colList[3]` (L18, L22).
pub const POWER_COLUMN: usize = 3;

/// `PowerReportListControl`
pub struct PowerReportListControl {
    pub list: ListControl,
    /// `self.nodeSelectCallback`
    pub node_select_callback: Option<JumpFn>,
    /// `self.showClusters`
    pub show_clusters: bool,
    /// `self.showMasteries`
    pub show_masteries: bool,
    /// `self.allocated`
    pub allocated: bool,
    /// `self.originalList` (nil until the first `SetReport`)
    pub original_list: Option<Rc<Vec<Rc<ReportEntry>>>>,
}

static HOOKS: ListHooks = ListHooks {
    get_row_value: PowerReportListControl::GetRowValue,
    add_value_tooltip: Some(PowerReportListControl::AddValueTooltip),
    on_sel_click: Some(PowerReportListControl::OnSelClick),
    re_sort: Some(PowerReportListControl::ReSort),
    ..ListHooks::BASE
};

/// The report entry of a row.
fn entry_of(v: &ListValue) -> Rc<ReportEntry> {
    v.rc::<ReportEntry>()
        .unwrap_or_else(|| lua_error!("PowerReportListControl: a row that is not a report entry"))
}

impl PowerReportListControl {
    /// lua: PowerReportListControl.lua:14-47 `PowerReportListClass:PowerReportListControl(anchor,
    /// rect, nodeSelectCallback)`
    /// @pob Classes/PowerReportListControl.lua:14-47 h=cc6d00f2
    pub fn new(
        anchor: Anchor,
        r: [Prop<f64>; 4],
        node_select_callback: Option<JumpFn>,
    ) -> New<PowerReportListControl> {
        // L17
        let width = r[2].raw();
        New::new(
            Base::new(anchor, r),
            PowerReportListControl {
                // L15
                list: ListControl::state(&HOOKS, 16.0, true, false, ListData::default(), false),
                // L27
                node_select_callback,
                // L28
                show_clusters: false,
                // L29
                show_masteries: true,
                // L30
                allocated: false,
                original_list: None,
            },
        )
        .host()
        // L15 (the ListControl constructor's scroll bars)
        .init(|cx, me| ListControl::init(cx, me.0))
        .init(move |cx, me| {
            let me = me.0;
            {
                let l = cx.w_mut::<ListControl>(me);
                // L18-25
                let col = |w: f64, label: &str, sortable: bool| {
                    ListColumn::new(Some(Prop::Val(w)), Some(label), sortable)
                };
                l.col_list = vec![
                    col(width * 0.15, "Type", true),
                    col(width * 0.45, "Node Name", false),
                    // L18 self.powerColumn
                    col(width * 0.16, "", true),
                    col(width * 0.05, "Points", true),
                    col(width * 0.16, "Per Point", true),
                ];
                // L26
                l.col_labels = true;
                // L31
                l.label = Some(Prop::Val("Building Tree...".to_owned()));
            }
            // L33-40
            let filter_select = cx
                .add(
                    me,
                    "filterSelect",
                    DropDownControl::new(
                        Anchor::new(BottomRight, me, TopRight),
                        rect(0, -2, 200, 20),
                        vec![
                            DropItem::str("Show Unallocated"),
                            DropItem::str("Show Unallocated & Clusters"),
                            DropItem::str("Show Allocated"),
                        ],
                        Some(Rc::new(
                            move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                                {
                                    let w = cx.w_mut::<PowerReportListControl>(me);
                                    // L36
                                    w.show_clusters = index == 2;
                                    // L37
                                    w.allocated = index == 3;
                                }
                                // L38
                                Self::ReList(cx, me);
                                // L39
                                Self::ReSort(cx, me, 3); // Sort by power
                            },
                        )),
                        None,
                        false,
                    ),
                )
                .id();
            // L41-45
            cx.add(
                me,
                "masteryCheck",
                CheckBoxControl::new(
                    Anchor::new(Right, filter_select, Left),
                    rect(-120, 0, 18, 0),
                    Some("Show Masteries:".to_owned()),
                    Some(Rc::new(move |cx: &mut Cx, state: bool| {
                        // L42
                        cx.w_mut::<PowerReportListControl>(me).show_masteries = state;
                        // L43
                        Self::ReList(cx, me);
                        // L44
                        Self::ReSort(cx, me, 3); // Sort by power
                    })),
                    None,
                    true,
                ),
            );
            // L46 return self
        })
    }

    /// lua: PowerReportListControl.lua:49-60 `PowerReportListClass:SetReport(stat, report)`
    /// @pob Classes/PowerReportListControl.lua:49-60 h=dc1c779c
    pub fn SetReport(
        cx: &mut Cx,
        me: CtrlId,
        stat: Option<&Table>,
        report: Option<Vec<Rc<ReportEntry>>>,
    ) {
        // L50
        let power_label = stat
            .and_then(|s| s.str(pob_lua::n!("label")))
            .map_or_else(String::new, |l| l.to_string());
        cx.w_mut::<ListControl>(me).col_list[POWER_COLUMN - 1].label =
            Some(Prop::Val(power_label.clone()));
        let has_report = report.is_some();
        // L51
        cx.w_mut::<PowerReportListControl>(me).original_list =
            Some(Rc::new(report.unwrap_or_default()));

        // L53
        let label = if stat.is_some_and(|s| s.truthy(pob_lua::n!("stat"))) {
            // L54
            if has_report {
                "Click to focus node on tree".to_owned()
            } else {
                "Building Tree...".to_owned()
            }
        } else {
            // L56
            format!("^7\"{power_label}\" not supported.  Select a specific stat from the dropdown.")
        };
        cx.w_mut::<ListControl>(me).label = Some(Prop::Val(label));

        // L59
        Self::ReList(cx, me);
    }

    /// lua: PowerReportListControl.lua:62-100 `PowerReportListClass:ReSort(colIndex)`
    /// @pob Classes/PowerReportListControl.lua:62-100 h=c805f597
    pub fn ReSort(cx: &mut Cx, me: CtrlId, colIndex: usize) {
        // Reverse power sort for allocated because it uses negative numbers
        // L64-66
        let allocated = cx.w::<PowerReportListControl>(me).allocated;
        let compare = move |a: f64, b: f64| if allocated { a < b } else { a > b };

        let ListData::Local(list) = &mut cx.w_mut::<ListControl>(me).list else {
            lua_error!("PowerReportListControl: self.list is not the control's own table")
        };
        // L68
        if colIndex == 1 {
            // L69-74
            pob_lua::sort::sort(list, |a, b| {
                let (a, b) = (entry_of(a), entry_of(b));
                // L70
                if a.r#type == b.r#type {
                    // L71
                    return compare(a.power, b.power);
                }
                // L73
                a.r#type.as_str().as_bytes() < b.r#type.as_str().as_bytes()
            });
        // L75
        } else if colIndex == 3 {
            // L76-78
            pob_lua::sort::sort(list, |a, b| {
                let (a, b) = (entry_of(a), entry_of(b));
                // L77
                compare(a.power, b.power)
            });
        // L79
        } else if colIndex == 4 {
            // L80-91
            pob_lua::sort::sort(list, |a, b| {
                let (a, b) = (entry_of(a), entry_of(b));
                // L81
                if a.pathDist.is("Anoint") || a.pathDist.is("Cluster") {
                    // L82
                    return false;
                }
                // L84
                if b.pathDist.is("Anoint") || b.pathDist.is("Cluster") {
                    // L85
                    return true;
                }
                // L87
                if a.pathDist == b.pathDist {
                    // L88
                    return compare(a.power, b.power);
                }
                // L90
                a.pathDist.lt(&b.pathDist)
            });
        // L92
        } else if colIndex == 5 {
            // L93-98
            pob_lua::sort::sort(list, |a, b| {
                let (a, b) = (entry_of(a), entry_of(b));
                // L94
                if a.pathPower == b.pathPower && a.pathDist.is_num() && b.pathDist.is_num() {
                    // L95
                    return a.pathDist.lt(&b.pathDist);
                }
                // L97
                compare(a.pathPower, b.pathPower)
            });
        }
    }

    /// lua: PowerReportListControl.lua:102-126 `PowerReportListClass:ReList()`
    /// @pob Classes/PowerReportListControl.lua:102-126 h=81117967
    pub fn ReList(cx: &mut Cx, me: CtrlId) {
        // L103
        cx.w_mut::<ListControl>(me).list = ListData::Local(Vec::new());
        let (original, show_clusters, allocated, show_masteries) = {
            let w = cx.w::<PowerReportListControl>(me);
            (
                w.original_list.clone(),
                w.show_clusters,
                w.allocated,
                w.show_masteries,
            )
        };
        // L104
        let Some(original) = original else {
            // L105
            return;
        };

        let mut list = Vec::new();
        // L108
        for item in original.iter() {
            // L109
            let mut insert = item.power > 0.0;
            // L110
            if !show_clusters && item.pathDist.is("Cluster") {
                // L111
                insert = false;
            }
            // L113
            if allocated {
                // L114
                insert = item.allocated;
            } else if item.allocated {
                // L116
                insert = false;
            }
            // L118
            if !show_masteries && item.r#type == pob_lua::n!("Mastery") {
                // L119
                insert = false;
            }

            // L122
            if insert {
                // L123
                list.push(ListValue::of(item.clone()));
            }
        }
        cx.w_mut::<ListControl>(me).list = ListData::Local(list);
    }

    /// lua: PowerReportListControl.lua:128-132 `PowerReportListClass:OnSelClick(index, report,
    /// doubleClick)`
    /// @pob Classes/PowerReportListControl.lua:128-132 h=632e006a
    fn OnSelClick(
        cx: &mut Cx,
        me: CtrlId,
        _index: usize,
        v: &ListValue,
        _double_click: bool,
    ) -> Option<bool> {
        // L129
        if let Some(f) = cx
            .w::<PowerReportListControl>(me)
            .node_select_callback
            .clone()
        {
            let report = entry_of(v);
            // L130
            f(cx, report.x, report.y);
        }
        None
    }

    /// lua: PowerReportListControl.lua:134-141 `PowerReportListClass:GetRowValue(column, index,
    /// report)`
    /// @pob Classes/PowerReportListControl.lua:134-141 h=e69a54e4
    fn GetRowValue(_cx: &Cx, _me: CtrlId, column: usize, _index: usize, v: &ListValue) -> String {
        let report = entry_of(v);
        // L135-140
        match column {
            1 => report.r#type.as_str().to_owned(),
            2 => report.name.clone(),
            3 => report.powerStr.clone(),
            4 => match &report.pathDist {
                PathDist::Num(n) if *n == 1000.0 => "Anoint".to_owned(),
                PathDist::Num(n) => pob_lua::tostring(*n),
                PathDist::Str(s) => (*s).to_owned(),
            },
            5 => report.pathPowerStr.clone(),
            _ => String::new(),
        }
    }

    /// lua: PowerReportListControl.lua:143-153 `PowerReportListClass:AddValueTooltip(tooltip, _,
    /// node)`
    /// @pob Classes/PowerReportListControl.lua:143-153 h=41d678b7
    fn AddValueTooltip(
        cx: &mut Cx,
        _me: CtrlId,
        tooltip: &mut Tooltip,
        _index: usize,
        v: &ListValue,
    ) {
        // L144
        if !cx.main.popups.is_empty() {
            // L145
            tooltip.clear(false);
            // L146
            return;
        }
        let node = entry_of(v);
        // L148
        if tooltip.check_for_update(cx, &[TtKey::Obj(TtObj(v.data.clone()))]) {
            if let Some(sd) = &node.sd {
                // L149
                for line in sd.snapshot() {
                    // L150
                    tooltip.add_line(cx, 16.0, &line);
                }
            }
        }
    }
}

// ---- the TreeTab seams (CCR-0123 §6) ---------------------------------------------------------

/// `TreeTabHooks::power_report_list`: `self.controls.powerReportList =
/// new("PowerReportListControl"):PowerReportListControl(anchor, rect, callback)` (TreeTab.lua:279).
pub fn ctor_hook(
    cx: &mut Cx,
    tree_tab: CtrlId,
    anchor: Anchor,
    r: [Prop<f64>; 4],
    jump: JumpFn,
) -> CtrlId {
    cx.add(
        tree_tab,
        "powerReportList",
        PowerReportListControl::new(anchor, r, Some(jump)),
    )
    .id()
}

/// `TreeTabHooks::power_report_list_set_label`: `self.controls.powerReportList.label = message`
/// (TreeTab.lua:300).
pub fn set_label_hook(cx: &mut Cx, list: CtrlId, message: &str) {
    cx.w_mut::<ListControl>(list).label = Some(Prop::Val(message.to_owned()));
}

impl Widget for PowerReportListControl {
    const NAME: &'static str = "PowerReportListControl";
    const CAPS: Caps = <ListControl as Widget>::CAPS;

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

    pob_ui_core::impl_parts!(PowerReportListControl: list => ListControl);
}
