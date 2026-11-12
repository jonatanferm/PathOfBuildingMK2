//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/TreeTab.lua:1047-1169 (SetPowerCalc, BuildPowerReportList).
//! Owner: U26.
//!
//! @pob-file Classes/TreeTab.lua @16de4b82d
//!
//! The Tree tab's power report: `TreeTab:SetPowerCalc`, `TreeTab:BuildPowerReportList`, the
//! `TreeTabHooks` entries U25's constructor and power-builder callbacks call (CCR-0123 §5, §6),
//! and the per-frame `build.calcsTab:BuildPower()` call site (PassiveTreeView.lua:785,
//! [`build_power`]). The PowerBuilder itself (CalcsTab.lua:441-773) is calc:PWR's resumable
//! iterator (`pob_calc::modules::power`); this unit steps it exactly where PoB resumes the
//! coroutine: once per `PassiveTreeView:Draw` while the heat map is shown.
//!
//! How the stepping reaches `Cx` (CCR-0123 §5): calc:PWR calls the build's
//! `powerBuilderProgressCallback(percent)` / `powerBuilderCallback()` (model closures, no `Cx`)
//! and SimpleGraphic's `GetTime()` (a host hook). [`build_power`] runs `CalcsTab:BuildPower` inside
//! a `pob_ui_core` coroutine resume, so the closures it registers (TreeTab.lua:292-324, U25's
//! [`core::power_builder_progress_callback`] / [`core::power_builder_callback`]) and the clock
//! (`cx.sys.time_ms()`, ui doc §8 rule 10) reach `Cx` through `co::with_cx` at the very point PoB
//! calls them: no queue, no reordering.
//!
//! DIVERGENCE(callback-registration): Lua assigns `build.powerBuilderProgressCallback` and
//! `build.powerBuilderCallback` once, in the TreeTab constructor (L292, L314). The model's
//! closures need the tab's id and the coroutine's `Cx`, so [`build_power`] (re)registers them
//! right before each step; they are the only callers' callbacks (no other PoB code calls
//! `calcsTab:BuildPower`), and nothing reads them between the constructor and the first step.
//! DIVERGENCE(pacing): SimpleGraphic keeps drawing frames while any coroutine is alive
//! (`coroutine._list()`, Common.lua:46-57), `calcsTab.powerBuilder` included. calc:PWR's iterator
//! is not a `pob_ui_core` coroutine, so a never-resumed placeholder coroutine ([`State::pacer`])
//! stands for it in the alive count (`co::any_alive`, `PobApp::wants_frames`) while
//! `calcsTab.powerBuilder` is set.
//!
//! Lua semantics:
//! * `self.build.displayStats` is U21's `display_stats::displayStats()`; the formatting fields
//!   `BuildPowerReportList` reads (`fmt`, `pc`, `mod`, `lowerIsBetter`) are copied into
//!   [`DisplayFmt`] (the fallback `{ fmt = ".1f" }` has the others nil).
//! * `isAlloc` (`node.alloc or grantedPassives[nodeId]`) is only tested for truth: a `bool`.
//! * `node.power` of a spec node is always a table (PassiveSpec.lua:56; calc:PWR's
//!   `node_power`); a non-table raises Lua's index error.
//!
//! ORDER: `pairs(self.build.spec.nodes)` (L1127) and `pairs(self.build.spec.tree.clusterNodeMap)`
//! (L1150) fill the report, which is then sorted by power with `table.sort` (L1158-1166, not
//! stable). Entries of distinct power end in one order whatever the fill order; entries of equal
//! power keep an order that follows the fill order, which is per-process random in PoB
//! (`spec.nodes` is filled from `tree.nodes`, which held the string key `root`; `clusterNodeMap`
//! is string-keyed: ui doc §8 rule 6, calc doc §3.4). The port fills in `LjIntMap` /
//! insertion order. Resolution (b)-like: the u26 scenarios compare the visible rows where the
//! stand-in powers are distinct (tests/u26_power.rs module notes); a tie's row order is not
//! reproducible against PoB.
//!
//! Props with side effects (ui doc §7.3): none.
#![allow(non_snake_case)]

use std::rc::Rc;

use pob_lua::format::FmtArg;
use pob_lua::n;
use pob_ui_core::coroutine::{co, Coroutine};
use pob_ui_core::luastr::format_num_sep_main;
use pob_ui_core::prelude::*;

use super::core::{self, ctl, tab};
use super::power_report_list::{PathDist, PowerReportListControl, ReportEntry};
use super::TreeTab;
use crate::app::CxApp;
use crate::build::display_stats;
use crate::main_mode::toast;
use crate::model::{BuildRef, Key, LStr, Node, NodeRef, Table, Val};
// calc:PWR's module is not re-exported by model.rs (U15; CCR-0123 §9, CCR-0115)
use pob_calc::modules::power; // CCR-0123 §9

/// U26's part of the TreeTab object (ui doc §12.1).
#[derive(Default)]
pub struct State {
    /// Stands for `calcsTab.powerBuilder` in `coroutine._list()` (DIVERGENCE(pacing)): alive
    /// while the report is being built.
    pub pacer: Coroutine,
}

// ---- the report helpers ------------------------------------------------------------------------

/// The `displayStat` fields `BuildPowerReportList` reads.
struct DisplayFmt {
    fmt: Option<&'static str>,
    pc: bool,
    r#mod: bool,
    lowerIsBetter: bool,
}

/// `node.power` (a table on every spec node).
fn node_power(node: &Node) -> Table {
    match node.extra.get(n!("power")) {
        Val::Table(t) => t,
        other => lua_error!("attempt to index field 'power' (a {} value)", other.lua_type()),
    }
}

/// `x or 0` for a number field.
fn num_or0(v: Val) -> f64 {
    match v {
        Val::Nil | Val::Bool(false) => 0.0,
        v => v.num(),
    }
}

/// `node.x` / `node.y` (`nil` is `NaN`, CCR-0024 §2).
fn pos(v: f64) -> Option<f64> {
    (!v.is_nan()).then_some(v)
}

/// `t_concat(parts, sep)` over strings.
fn t_concat(parts: &[LStr], sep: &str) -> String {
    parts.iter().map(|p| &**p).collect::<Vec<&str>>().join(sep)
}

impl TreeTab {
    /// lua: TreeTab.lua:1047-1059 `TreeTabClass:SetPowerCalc(powerStat)`
    /// (`TreeTabHooks::set_power_calc`)
    /// @pob Classes/TreeTab.lua:1047-1059 h=5870f6c7
    pub fn SetPowerCalc(cx: &mut Cx, me: CtrlId, powerStat: Option<Table>) {
        let m = cx.build().model.clone();
        // L1048
        cx.w_mut::<TreeTab>(me).core.viewer.show_heat_map = true;
        // L1049
        m.buildFlag.set(true);
        // L1050
        m.calcsTab.powerBuildFlag.set(true);
        // L1051
        power::set_powerStat(&m, powerStat.clone());
        // L1052
        let list = ctl(cx, me, "powerReportList");
        PowerReportListControl::SetReport(cx, list, powerStat.as_ref(), None);
        // -- Remove old toast and clear dismissed state so toast can show for new power report
        // L1054
        if let Some(id) = cx.w::<TreeTab>(me).core.power_builder_toast_id.clone() {
            // L1055
            toast::clear_dismissed(cx, Some(&id));
            // L1056
            toast::remove(cx, &id, true);
            // L1057
            cx.w_mut::<TreeTab>(me).core.power_builder_toast_id = None;
        }
    }

    /// lua: TreeTab.lua:1061-1168 `TreeTabClass:BuildPowerReportList(currentStat)`
    /// @pob Classes/TreeTab.lua:1061-1168 h=623f2422
    pub fn BuildPowerReportList(cx: &Cx, _me: CtrlId, currentStat: Option<&Table>) -> Vec<Rc<ReportEntry>> {
        let m = cx.build().model.clone();
        // L1062
        let mut report: Vec<Rc<ReportEntry>> = Vec::new();

        // L1064
        let Some(stat) = currentStat
            .filter(|s| s.truthy(n!("stat")))
            .map(|s| s.get(n!("stat")))
        else {
            // L1065
            return report;
        };

        // -- locate formatting information for the type of heat map being used.
        // -- maybe a better place to find this? At the moment, it is the only place
        // -- in the code that has this information in a tidy place.
        // L1071
        let mut displayStat: Option<DisplayFmt> = None;

        // L1073
        for ds in display_stats::displayStats().iter() {
            // L1074
            if ds.stat.is_some_and(|d| stat.str().is_some_and(|s| *s == *d)) {
                // L1075
                displayStat = Some(DisplayFmt {
                    fmt: ds.fmt,
                    pc: ds.pc,
                    r#mod: ds.r#mod,
                    lowerIsBetter: ds.lowerIsBetter,
                });
                // L1076
                break;
            }
        }

        // -- not every heat map has an associated "stat" in the displayStats table
        // -- this is due to not every stat being displayed in the sidebar, I believe.
        // -- But, we do want to use the formatting knowledge stored in that table rather than duplicating it here.
        // -- If no corresponding stat is found, just default to a generic stat display (>0=good, one digit of precision).
        // L1084-1088
        let displayStat = displayStat.unwrap_or(DisplayFmt {
            fmt: Some(".1f"),
            pc: false,
            r#mod: false,
            lowerIsBetter: false,
        });
        // L1089
        let powerMultiplier = if displayStat.pc || displayStat.r#mod { 100.0 } else { 1.0 };
        // L1090-1098
        let formatPower = |power: f64| -> String {
            // L1091
            let Some(fmt) = displayStat.fmt else {
                lua_error!("attempt to concatenate field 'fmt' (a nil value)")
            };
            let powerStr = format_num_sep_main(
                &cx.main,
                &pob_lua::lua_format(&format!("%{fmt}"), &[FmtArg::from(power)]),
            );
            // L1092
            if (power > 0.0 && !displayStat.lowerIsBetter) || (power < 0.0 && displayStat.lowerIsBetter) {
                // L1093
                return format!("{}{powerStr}", cx.main.cc.POSITIVE);
            // L1094
            } else if (power < 0.0 && !displayStat.lowerIsBetter) || (power > 0.0 && displayStat.lowerIsBetter) {
                // L1095
                return format!("{}{powerStr}", cx.main.cc.NEGATIVE);
            }
            // L1097
            powerStr
        };
        // L1099-1104
        let getNodePathDist = |node: &Node, isAlloc: bool| -> f64 {
            // L1100
            if isAlloc {
                // L1101
                let n = node.depends.get().len();
                return if n == 0 { 1.0 } else { n as f64 };
            }
            // L1103
            let distance = node_power(node).get(n!("distance"));
            if distance.truthy() {
                return distance.num();
            }
            let n = node.path.get().map_or(0, |p| p.len());
            if n == 0 {
                1.0
            } else {
                n as f64
            }
        };
        // L1105-1120
        let mut addReportEntry = |node: &Node,
                                  name: String,
                                  nodePower: f64,
                                  pathPower: f64,
                                  pathDist: PathDist,
                                  isAlloc: bool,
                                  pathPowerStr: Option<&str>| {
            // L1106-1119
            report.push(Rc::new(ReportEntry {
                name,
                power: nodePower,
                powerStr: formatPower(nodePower),
                pathPower,
                pathPowerStr: pathPowerStr.map_or_else(|| formatPower(pathPower), str::to_owned),
                allocated: isAlloc,
                id: node.id,
                x: pos(node.x.get()),
                y: pos(node.y.get()),
                r#type: node.r#type.get(),
                sd: Some(node.sd.get()),
                pathDist,
            }));
        };

        let spec = m.spec.get();
        let mainEnv = m.calcsTab.mainEnv.get();
        let granted = |id: i64| -> bool {
            let Some(env) = &mainEnv else {
                lua_error!("attempt to index field 'mainEnv' (a nil value)")
            };
            env.grantedPassives.get().with(|g| g.get(id)).unwrap_or(false)
        };

        // -- search all nodes, ignoring ascendancies, sockets, etc.
        // L1123 ORDER: pairs(spec.nodes) (module docs: the report is sorted below)
        for (nodeId, node) in spec.nodes.with(pob_lua::LjIntMap::pairs) {
            // L1124
            let isAlloc = node.alloc.get() || granted(nodeId);
            let ty = node.r#type.get();
            // L1125
            if (ty == n!("Normal") || ty == n!("Keystone") || ty == n!("Notable"))
                && node.ascendancyName.get().is_none()
            {
                // L1126
                let pathDist = getNodePathDist(&node, isAlloc);
                let power = node_power(&node);
                // L1127
                let nodePower = num_or0(power.get(n!("singleStat"))) * powerMultiplier;
                // L1128
                let pathPower = num_or0(power.get(n!("pathPower"))) / pathDist * powerMultiplier;
                // L1129
                addReportEntry(
                    &node,
                    node.dn.get().to_string(),
                    nodePower,
                    pathPower,
                    PathDist::Num(pathDist),
                    isAlloc,
                    None,
                );
            // L1130
            } else if ty == n!("Mastery")
                && node_power(&node).truthy(n!("masteryEffects"))
                && node.ascendancyName.get().is_none()
            {
                // L1131
                let pathDist = getNodePathDist(&node, isAlloc);

                // L1133 (nothing writes the list inside the loop: ipairs over a snapshot)
                let masteryEffects = node.masteryEffects.get().unwrap_or_default();
                for (_, masteryEffect) in masteryEffects.ipairs() {
                    let effectId = masteryEffect.get(n!("effect"));
                    let tree = spec.tree.get();
                    // L1134 (`masteryEffects[nil]` is a read: nil)
                    let effect: Option<NodeRef> = match &effectId {
                        Val::Nil => None,
                        v => tree.masteryEffects.get(Key::from(v.num())).obj::<Node>(),
                    };
                    // L1135
                    let effectPower = match node_power(&node).table(n!("masteryEffects")) {
                        Some(me) => match &effectId {
                            Val::Nil => None,
                            v => me.table(Key::from(v.num())),
                        },
                        None => lua_error!("attempt to index field 'masteryEffects' (a nil value)"),
                    };
                    // L1136
                    if let (Some(effect), Some(effectPower)) = (effect, effectPower) {
                        // L1137
                        let effectLabelParts: Option<Vec<LStr>> =
                            if isAlloc && !node.allMasteryOptions.get() {
                                Some(node.sd.get().snapshot())
                            } else {
                                match effect.extra.get(n!("stats")) {
                                    Val::Nil | Val::Bool(false) => Some(effect.sd.get().snapshot()),
                                    Val::Table(t) => Some(
                                        t.ipairs()
                                            .into_iter()
                                            .map(|(i, v)| match v.str() {
                                                Some(s) => s,
                                                None => lua_error!(
                                                    "invalid value (at index {i}) in table for 'concat'"
                                                ),
                                            })
                                            .collect(),
                                    ),
                                    other => lua_error!(
                                        "bad argument #1 to 'concat' (table expected, got {})",
                                        other.lua_type()
                                    ),
                                }
                            };
                        // L1138
                        let name = match &effectLabelParts {
                            Some(parts) => format!("{}: {}", node.dn.get(), t_concat(parts, " / ")),
                            None => node.dn.get().to_string(),
                        };
                        // L1139
                        let nodePower = num_or0(effectPower.get(n!("singleStat"))) * powerMultiplier;
                        // L1140
                        let pp = match effectPower.get(n!("pathPower")) {
                            Val::Nil | Val::Bool(false) => num_or0(effectPower.get(n!("singleStat"))),
                            v => v.num(),
                        };
                        let pathPower = (pp / pathDist) * powerMultiplier;
                        // L1141
                        addReportEntry(&node, name, nodePower, pathPower, PathDist::Num(pathDist), isAlloc, None);
                    }
                }
            }
        }

        // -- search all cluster notables and add to the list
        // L1148 ORDER: pairs(clusterNodeMap) (string keys; module docs: the report is sorted below)
        let tree = spec.tree.get();
        for (_nodeName, node) in tree.clusterNodeMap.pairs() {
            let Some(node) = node.obj::<Node>() else {
                lua_error!("attempt to index local 'node' (a non-node value)")
            };
            // L1149
            let isAlloc = node.alloc.get();
            // L1150
            if !isAlloc {
                // L1151
                let nodePower = match node.extra.get(n!("power")) {
                    Val::Nil | Val::Bool(false) => 0.0,
                    Val::Table(p) => num_or0(p.get(n!("singleStat"))),
                    other => lua_error!("attempt to index field 'power' (a {} value)", other.lua_type()),
                } * powerMultiplier;
                // L1152
                addReportEntry(
                    &node,
                    node.dn.get().to_string(),
                    nodePower,
                    0.0,
                    PathDist::Str("Cluster"),
                    isAlloc,
                    Some("--"),
                );
            }
        }

        // -- sort it
        // L1157
        if displayStat.lowerIsBetter {
            // L1158-1160
            pob_lua::sort::sort(&mut report, |a, b| a.power < b.power);
        } else {
            // L1162-1164
            pob_lua::sort::sort(&mut report, |a, b| a.power > b.power);
        }

        // L1167
        report
    }
}

// ---- the TreeTab seams (CCR-0123 §6) -----------------------------------------------------------

/// `TreeTabHooks::power_report_set` (TreeTab.lua:316-317 in U25's `powerBuilderCallback`):
/// `local report = self:BuildPowerReportList(powerStat)` and
/// `self.controls.powerReportList:SetReport(powerStat, report)`.
pub fn power_report_set_hook(cx: &mut Cx, me: CtrlId, powerStat: Table) {
    // L316
    let report = TreeTab::BuildPowerReportList(cx, me, Some(&powerStat));
    // L317
    let list = ctl(cx, me, "powerReportList");
    PowerReportListControl::SetReport(cx, list, Some(&powerStat), Some(report));
}

// ---- the BuildPower call site (PassiveTreeView.lua:785) ---------------------------------------

/// SimpleGraphic's `GetTime()` while calc:PWR runs inside [`build_power`]'s resume:
/// `cx.sys.time_ms()` (ui doc §8 rule 10; the oracle's clock is virtual).
fn coroutine_get_time() -> f64 {
    co::with_cx(|cx| cx.sys.time_ms() as f64)
}

/// Restores calc:PWR's default clock when the step ends (also on a Lua error).
struct ClockGuard;
impl Drop for ClockGuard {
    fn drop(&mut self) {
        power::set_GetTime(None);
    }
}

/// The build's `powerBuilderProgressCallback` / `powerBuilderCallback` (TreeTab.lua:292-324,
/// U25's functions; DIVERGENCE(callback-registration)): called by calc:PWR inside the resume of
/// [`build_power`], they reach `Cx` through `co::with_cx`.
fn register_callbacks(m: &BuildRef, tree_tab: Option<CtrlId>) {
    let Some(me) = tree_tab else {
        power::set_powerBuilderProgressCallback(m, None);
        power::set_powerBuilderCallback(m, None);
        return;
    };
    // L292-312
    power::set_powerBuilderProgressCallback(
        m,
        Some(Rc::new(move |percent: f64| {
            co::with_cx(|cx| core::power_builder_progress_callback(cx, me, Some(percent)));
        })),
    );
    // L314-324
    power::set_powerBuilderCallback(
        m,
        Some(Rc::new(move || {
            co::with_cx(|cx| core::power_builder_callback(cx, me));
        })),
    );
}

/// `TreeViewHooks::build_power`: `build.calcsTab:BuildPower()` (PassiveTreeView.lua:785, once
/// per Draw while the heat map is shown): calc:PWR's `CalcsTab:BuildPower` (CalcsTab.lua:475-493:
/// start the report when `BuildOutput` flagged it, resume it once), run inside a coroutine resume
/// so its callbacks and clock reach `Cx` (module docs).
pub fn build_power(cx: &mut Cx) {
    let m = cx.build().model.clone();
    let tree_tab = tab(cx);
    register_callbacks(&m, tree_tab);
    let mut step = Coroutine::create({
        let m = m.clone();
        async move {
            power::BuildPower(&m);
        }
    });
    {
        power::set_GetTime(Some(coroutine_get_time));
        let _clock = ClockGuard;
        // the body never yields: one resume runs `CalcsTab:BuildPower` to its end; a Lua error
        // (dev mode, or a callback's) propagates as it does from PoB's call
        step.resume(cx);
    }
    // DIVERGENCE(pacing)
    if let Some(me) = tree_tab {
        let running = power::powerBuilderRunning(&m);
        let pacer = &mut cx.w_mut::<TreeTab>(me).power.pacer;
        if running {
            if pacer.is_dead() {
                *pacer = Coroutine::create(std::future::pending::<()>());
            }
        } else {
            *pacer = Coroutine::default();
        }
    }
}
