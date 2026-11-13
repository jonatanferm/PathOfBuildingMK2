//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/CompareTab.lua:2516-3161 (ComparePowerBuilder, RunComparePowerReport).
//! Owner: U48.
//!
//! @pob-file Classes/CompareTab.lua @16de4b82d
//!
//! The Compare tab's power report: `CompareTab:ComparePowerBuilder` (the coroutine that rates
//! every tree node, item, jewel, socket group, support gem and config option of the compared
//! build against the primary build with the misc calculator) and
//! `CompareTab:RunComparePowerReport` (the per-frame driver DrawSummary calls,
//! CompareTab.lua:3236, U49). The results are [`PowerResult`]s, stored as U46's
//! `init::State::comparePowerResults` (`Rc<Vec<Rc<PowerResult>>>` behind its `Rc<dyn Any>`,
//! CCR-0136 §2; read back with [`results`]). The list control is `power_list.rs`.
//!
//! # The coroutine
//!
//! `coroutine.create(function() self:ComparePowerBuilder(...) end)` is a `pob_ui_core`
//! [`Coroutine`] over the async [`CompareTab::ComparePowerBuilder`]; its body reaches `Cx`
//! through `co::with_cx` between yields and yields (`co::yield_now`) exactly where the Lua
//! does: after an item of work when `coroutine.running() and GetTime() - start > 100`
//! (`GetTime()` is `cx.sys.time_ms()`, ui doc §8 rule 10). The coroutine is stored in U46's
//! `comparePowerCoroutine` and resumed through `cx.lend` (ui doc §6.13): `RunComparePowerReport`
//! is the only code that resumes it, and the body never reads that field.
//!
//! # Seams
//!
//! * `self.calcs.getMiscCalculator(self.primaryBuild)` (CompareTab.lua:193, Calcs.lua:89-114) is
//!   the build's [`crate::calc::CalcFacade::misc_calculator`]; `calcFunc(override, useFullDPS)`
//!   is [`crate::calc::MiscCalc::call`].
//! * `calcsTab:CalculatePowerStat` / `data.powerStatList.GetFromOutput` are calc:PWR's
//!   (`pob_calc::modules::power`).
//! * `itemsTab:AddItem` / `DeleteItem` are U32's `ItemsTabHooks` (U34's ports);
//!   `slot:SetSelItemId` is U32's `items::slot::set_sel_item_id` (ItemSlotControl.lua:72-84).
//! * `self:GetJewelComparisonSlots`, `NormalizeConfigVals`, `FormatConfigValue` are U46's.
//! * `self:ShouldShowRing3` / `AddAbyssSockets` (CompareTab.lua:3668-3693) are U49's
//!   `CompareTab::ShouldShowRing3` / `AddAbyssSockets` (`draw_a.rs`), and
//!   `self:GetGemGrantedEffect` / `GetSocketGroupSignature` / `GetSocketGroupLabel`
//!   (CompareTab.lua:2466-2513) U47's (`layout.rs`); FUP7 replaced this file's private copies
//!   (CCR-0138 §1) with calls to them.
//!
//! # Lua semantics
//!
//! * `categories` is `self.comparePowerCategories` itself (the live table): each
//!   `categories.X` test reads U46's field when it runs.
//! * `self.comparePowerCompareId ~= compareEntry` compares the entry by identity; Lua keeps the
//!   entry alive through the field. This unit keeps the entry itself in
//!   [`State::comparePowerCompareId`] (CCR-0138 §3; U46's unused `Option<usize>` is gone, FUP7).
//! * `type(nodeId) == "number"`: every key of `spec.allocNodes` is a number (`LjIntMap`).
//! * `pNode.pathDist or 0`: `Cell<Option<f64>>`; `#(pNode.path or {})` the path's length.
//! * `cItem.raw` is always a string (truthy); `cItem.name or "Unknown"`: `name` is always set.
//! * `tonumber(rawNumStr) == 0`: `pob_lua::tonumber` (nil for "nan"/"inf", which is not 0).
//! * `formatImpact` returns five values; `local perPointStr = formatImpact(perPoint)` keeps the
//!   first.
//! * `pcall(function() return self.calcs.getMiscCalculator(...) end)`: a Lua error inside is
//!   caught (`pob_lua::error::catch`; on wasm32, which has no unwinding, it propagates) and its
//!   message printed with `ConPrintf`.
//! * `tempGem` (L2986-2997) is a plain table with ten fields; here a new `GemInstance` with
//!   those fields (`qualityId` lives in `extra`: the model has no typed field for it) and every
//!   other field at its default (nil).
//! * `pInput[varData.var] = cVal` with a nil `cVal` removes the key (`Table::set` of nil).
//! * `displayName:gsub(":$", "")` keeps gsub's first value; `stripColors` likewise.
//!
//! DIVERGENCE(web-pending): the facade's calls return `Poll::Pending` only on wasm (calc doc
//! D15). The first `getMiscCalculator` (L2521) waits for the worker by yielding the coroutine
//! and asking again; any later Pending (inside a temporary model change) raises a Lua error
//! ("the calc is not ready"), which ends the report as a failed resume does. The web build is
//! not a parity target (ui doc §13 item 6).
//!
//! ORDER: `pairs(compareNodes)` (L2543, L2664) visits `compareEntry.spec.allocNodes` in
//! `LjIntMap` order (LuaJIT's, CCR-0024 §5); the results are sorted by impact afterwards by the
//! list (`ReSort(3)`), so only rows of equal impact depend on it. `allocNodes` is filled from
//! `spec.nodes`, whose fill order is per-process random in PoB (STYLE.md §LjIntMap), so ties
//! are (b): the u48 scenarios show rows whose impacts are distinct or whose ties draw the same
//! text. The per-`modKey` `cache` (L2662) is a lookup only. `pairs(oldEquipped)` (L2773)
//! re-equips distinct abyssal slots (no `nodeId`: no cluster graph rebuild; order-independent,
//! (c)). `pSignatures`/`pSupportNames` are lookups only.
//!
//! Props with side effects (ui doc §7.3): none.
#![allow(non_snake_case)]

use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::task::Poll;

use pob_lua::format::FmtArg;
use pob_lua::{n, Name};
use pob_ui_core::coroutine::{co, Coroutine};
use pob_ui_core::luastr::format_num_sep_main;
use pob_ui_core::prelude::*;

use super::init::{at_num, st, st_mut, CompareEntryRef, CLUSTER_NODE_OFFSET};
use super::CompareTab;
use crate::app::CxApp;
use crate::build::display_stats;
use crate::calc::MiscCalc;
use crate::model::gamedata::schema::item_data::PowerStat;
use crate::model::{
    self, Field, GemInstance, GemInstanceRef, ItemRef, NodeRef, Output, Override, Table, Val,
};
use crate::tabs::items::slot::set_sel_item_id;
use crate::tabs::items::slot_helper::items_hooks;
// calc:PWR's module is not re-exported by model.rs (U15; CCR-0123 §9, CCR-0115)
use pob_calc::classes::item::new_Item; // CCR-0123 §9
use pob_calc::modules::power; // CCR-0123 §9

/// U48's part of the CompareTab object (ui doc §12.1).
#[derive(Default)]
pub struct State {
    /// `self.comparePowerCompareId` (the compare entry the report was made for; module notes,
    /// CCR-0138 §3)
    pub comparePowerCompareId: Option<CompareEntryRef>,
}

/// One result entry of `ComparePowerBuilder` (L2687-2700, L2788-2802, L2873-2886, L2937-2949,
/// L3018-3030, L3097-3109).
pub struct PowerResult {
    /// `category` ("Tree", "Item", "Skill gem", "Support gem", "Config")
    pub category: &'static str,
    pub categoryColor: String,
    pub nameColor: String,
    pub name: String,
    /// `nodeId` (tree entries)
    pub nodeId: Option<i64>,
    /// `itemObj` (item and jewel entries)
    pub itemObj: Option<ItemRef>,
    /// `slotName` (item entries; nil for jewels)
    pub slotName: Option<String>,
    pub impact: f64,
    pub impactStr: String,
    pub impactPercent: f64,
    pub combinedImpactStr: String,
    /// `pathDist`, `perPoint`, `perPointStr` (tree entries; nil otherwise)
    pub pathDist: Option<f64>,
    pub perPoint: Option<f64>,
    pub perPointStr: Option<String>,
}

/// `self.comparePowerResults` as this unit's entries (`None`: nil).
pub fn results(cx: &Cx, me: CtrlId) -> Option<Rc<Vec<Rc<PowerResult>>>> {
    let r = st(cx, me).comparePowerResults.clone()?;
    Some(
        r.downcast::<Vec<Rc<PowerResult>>>()
            .unwrap_or_else(|_| lua_error!("comparePowerResults is not a power report")),
    )
}

/// `self.comparePowerResults = results`
fn set_results(cx: &mut Cx, me: CtrlId, results: Vec<Rc<PowerResult>>) {
    let r: Rc<dyn Any> = Rc::new(results);
    st_mut(cx, me).comparePowerResults = Some(r);
}

// ---- small helpers (Rust-only) ----------------------------------------------------------------

/// The `displayStat` fields `formatImpact` reads.
struct DisplayFmt {
    fmt: Option<&'static str>,
    pc: bool,
    r#mod: bool,
    lowerIsBetter: bool,
}

impl DisplayFmt {
    /// `(displayStat.pc or displayStat.mod) and 100 or 1`
    fn mult(&self) -> f64 {
        if self.pc || self.r#mod {
            100.0
        } else {
            1.0
        }
    }
}

/// `GetTime()`
fn get_time(cx: &Cx) -> f64 {
    cx.sys.time_ms() as f64
}

/// `ConPrintf(fmt, ...)`
fn con_printf(cx: &mut Cx, fmt: &str, args: &[FmtArg<'_>]) {
    let s = pob_lua::lua_format(fmt, args);
    cx.sys.platform.con_print(&s);
}

/// `pcall(f)`: `Err(message)` for a Lua error raised inside (module notes).
fn pcall<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        pob_lua::error::catch(f).map_err(|e| e.0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        Ok(f())
    }
}

/// A facade result outside the first `getMiscCalculator` (DIVERGENCE(web-pending)).
fn ready<T>(p: Poll<T>) -> T {
    match p {
        Poll::Ready(v) => v,
        Poll::Pending => lua_error!("the calc is not ready"),
    }
}

/// `self.calcs.getMiscCalculator(self.primaryBuild)` (Poll on wasm only).
fn misc_calculator(cx: &mut Cx) -> Poll<(Rc<dyn MiscCalc>, Output)> {
    let m = cx.build().model.clone();
    cx.build_mut().calc.misc_calculator(&m)
}

/// `calcFunc(override, useFullDPS)`
fn call(calcFunc: &Rc<dyn MiscCalc>, ov: Override, useFullDPS: bool) -> Output {
    ready(calcFunc.call(Some(Rc::new(ov)), Some(useFullDPS)))
}

/// `{ [node] = true }` as an `addNodes` set.
fn node_set(node: &NodeRef) -> pob_lua::Shared<model::LjIntMap<NodeRef>> {
    let mut m = model::LjIntMap::new();
    m.set(node.id as i64, Some(node.clone()));
    pob_lua::Shared::new(m)
}

/// `items[id]` of a build's ItemsTab (`id` a Lua value: nil/false and non-numbers give nil).
fn item_at(build: &model::BuildRef, id: &Val) -> Option<ItemRef> {
    let id = id.opt_num()?;
    if id.fract() != 0.0 || id < 0.0 || id > f64::from(u32::MAX) {
        return None;
    }
    build.itemsTab.items.with(|m| m.get(&(id as u32)).cloned())
}

/// `itemsTab.slots[slotName]`
fn slot_at(build: &model::BuildRef, slotName: &str) -> Option<Table> {
    build
        .itemsTab
        .slots
        .with(|s| s.get(&Name::from(slotName)).cloned())
}

/// `slot and itemsTab.items[slot.selItemId]`
fn slot_item(build: &model::BuildRef, slotName: &str) -> Option<ItemRef> {
    slot_at(build, slotName).and_then(|s| item_at(build, &s.get(n!("selItemId"))))
}

/// `colorCodes[name]`
fn color(cx: &Cx, name: &str) -> String {
    cx.main.cc.get(name).cloned().unwrap_or_default()
}

/// `colorCodes[item.rarity] or colorCodes.NORMAL`
fn rarity_color(cx: &Cx, item: &ItemRef) -> String {
    let rarity = item.rarity.get();
    cx.main
        .cc
        .get(rarity.as_str())
        .cloned()
        .unwrap_or_else(|| cx.main.cc.NORMAL.clone())
}

/// `group.gemList or {}` (a snapshot: the loops below do not change the list they iterate).
fn gems_of(group: &model::SocketGroupRef) -> Vec<GemInstanceRef> {
    group.gemList.get().snapshot()
}

// ---- ComparePowerBuilder ----------------------------------------------------------------------

/// `local baseSlots = { ... }` with Ring 3 and the abyssal sockets (L2553-2559, L2718-2724).
fn base_slots(cx: &Cx, compareEntry: &CompareEntryRef) -> Vec<String> {
    let mut baseSlots: Vec<String> = [
        "Weapon 1",
        "Weapon 2",
        "Helmet",
        "Body Armour",
        "Gloves",
        "Boots",
        "Amulet",
        "Ring 1",
        "Ring 2",
        "Belt",
        "Flask 1",
        "Flask 2",
        "Flask 3",
        "Flask 4",
        "Flask 5",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    if CompareTab::ShouldShowRing3(cx, compareEntry) {
        // t_insert(baseSlots, 10, "Ring 3")
        baseSlots.insert(9, "Ring 3".to_owned());
    }
    // we only use jewel slots if both sides have them available
    CompareTab::AddAbyssSockets(cx, compareEntry, &mut baseSlots, true);
    baseSlots
}

/// The support gem names of a group (L2585-2592, L2971-2978).
fn support_names(group: &model::SocketGroupRef) -> HashSet<String> {
    let mut pSupportNames = HashSet::new();
    for gem in gems_of(group) {
        let ge = CompareTab::GetGemGrantedEffect(&gem);
        if let Some(ge) = ge.filter(|ge| ge.support) {
            // `ge.name or gem.nameSpec` (`ge.name` is always set)
            pSupportNames.insert(ge.name.to_string());
        }
    }
    pSupportNames
}

/// `local function formatImpact(impact)` (L2627-2654): `str, displayVal, combinedStr, percent,
/// isZero`.
fn formatImpact(
    cx: &Cx,
    displayStat: &DisplayFmt,
    baseStatValue: f64,
    impact: f64,
) -> (String, f64, String, f64, bool) {
    // L2628
    let displayVal = impact * displayStat.mult();
    // L2629
    let Some(fmt) = displayStat.fmt else {
        lua_error!("attempt to concatenate field 'fmt' (a nil value)")
    };
    let rawNumStr = pob_lua::lua_format(&format!("%{fmt}"), &[FmtArg::from(displayVal)]);
    // L2630
    let isZero = pob_lua::tonumber(&rawNumStr) == Some(0.0);
    // L2631
    let numStr = format_num_sep_main(&cx.main, &rawNumStr);

    // Determine color
    // L2634
    let isPositive = (displayVal > 0.0 && !displayStat.lowerIsBetter)
        || (displayVal < 0.0 && displayStat.lowerIsBetter);
    // L2635
    let isNegative = (displayVal < 0.0 && !displayStat.lowerIsBetter)
        || (displayVal > 0.0 && displayStat.lowerIsBetter);
    // L2636
    let color = if isPositive {
        cx.main.cc.POSITIVE.clone()
    } else if isNegative {
        cx.main.cc.NEGATIVE.clone()
    } else {
        "^7".to_owned()
    };
    // L2637
    let sign = if displayVal > 0.0 { "+" } else { "" };
    // L2638
    let str = format!("{color}{sign}{numStr}");

    // Compute percentage change
    // L2641
    let mut percent = 0.0;
    // L2642
    if baseStatValue != 0.0 {
        // L2643
        percent = (impact / baseStatValue.abs()) * 100.0;
    }

    // Build combined string: "+1,234.5 (+4.3%)"
    // L2647
    let mut combinedStr = str.clone();
    // L2648
    if percent != 0.0 {
        // L2649
        let pctStr = pob_lua::lua_format("%+.1f%%", &[FmtArg::from(percent)]);
        // L2650
        combinedStr = format!("{str} ^7({color}{pctStr}^7)");
    }

    // L2653
    (str, displayVal, combinedStr, percent, isZero)
}

/// `processed = processed + 1; if coroutine.running() and GetTime() - start > 100 then
/// self.comparePowerProgress = m_floor(processed / total * 100); coroutine.yield(); start =
/// GetTime() end` (L2703-2708 and the five copies below).
async fn step(me: CtrlId, processed: &mut f64, total: f64, start: &mut f64) {
    *processed += 1.0;
    let processed = *processed;
    let start_v = *start;
    let yield_now = co::with_cx(|cx| {
        if co::running() && get_time(cx) - start_v > 100.0 {
            st_mut(cx, me).comparePowerProgress = (processed / total * 100.0).floor();
            true
        } else {
            false
        }
    });
    if yield_now {
        co::yield_now().await;
        *start = co::with_cx(|cx| get_time(cx));
    }
}

/// A result entry with the impact fields of `formatImpact` and no tree fields.
#[allow(clippy::too_many_arguments)]
fn entry(
    category: &'static str,
    categoryColor: String,
    nameColor: String,
    name: String,
    itemObj: Option<ItemRef>,
    slotName: Option<String>,
    f: (String, f64, String, f64, bool),
) -> Rc<PowerResult> {
    let (impactStr, impactVal, combinedImpactStr, impactPercent, _) = f;
    Rc::new(PowerResult {
        category,
        categoryColor,
        nameColor,
        name,
        nodeId: None,
        itemObj,
        slotName,
        impact: impactVal,
        impactStr,
        impactPercent,
        combinedImpactStr,
        pathDist: None,
        perPoint: None,
        perPointStr: None,
    })
}

impl CompareTab {
    /// Coroutine: calculate power of compared build elements against primary build
    /// lua: CompareTab.lua:2515-3126 `CompareTabClass:ComparePowerBuilder(compareEntry, powerStat,
    /// categories)` (`categories` is `self.comparePowerCategories`: module notes)
    /// @pob Classes/CompareTab.lua:2516-3126 h=3443146f
    #[allow(clippy::too_many_lines)]
    pub async fn ComparePowerBuilder(
        me: CtrlId,
        compareEntry: CompareEntryRef,
        powerStat: &'static PowerStat,
    ) {
        let cmp = compareEntry.model.clone();
        let cats = move |cx: &Cx| st(cx, me).comparePowerCategories;
        let statTable = power::powerStat_table(powerStat);
        // L2517
        let mut results: Vec<Rc<PowerResult>> = Vec::new();
        // L2518
        let useFullDPS = powerStat.stat.is_some_and(|s| s == n!("FullDPS"));

        // Get calculator for primary build
        // L2521 (DIVERGENCE(web-pending))
        let (calcFunc, calcBase) = loop {
            if let Poll::Ready(r) = co::with_cx(misc_calculator) {
                break r;
            }
            co::yield_now().await;
        };

        // Find display stat for formatting
        // L2524
        let mut displayStat: Option<DisplayFmt> = None;
        // L2525
        for ds in display_stats::displayStats().iter() {
            // L2526
            if ds
                .stat
                .is_some_and(|d| powerStat.stat.is_some_and(|s| s.as_str() == d))
            {
                // L2527
                displayStat = Some(DisplayFmt {
                    fmt: ds.fmt,
                    pc: ds.pc,
                    r#mod: ds.r#mod,
                    lowerIsBetter: ds.lowerIsBetter,
                });
                // L2528
                break;
            }
        }
        // L2531-2533
        let displayStat = displayStat.unwrap_or(DisplayFmt {
            fmt: Some(".1f"),
            pc: false,
            r#mod: false,
            lowerIsBetter: false,
        });

        // L2535
        let mut total = 0.0;
        // L2536
        let mut processed = 0.0;
        // L2537
        let mut start = co::with_cx(|cx| get_time(cx));

        // Count total work items for progress
        co::with_cx(|cx| {
            let p = cx.build().model.clone();
            let c = cats(cx);
            // L2540
            if c.treeNodes {
                // L2541
                let compareNodes = cmp.spec.get().allocNodes.with(model::LjIntMap::pairs);
                // L2542
                let primarySpec = p.spec.get();
                // L2543 ORDER: see the module notes
                for (nodeId, _node) in compareNodes {
                    // L2544
                    if (nodeId as f64) < CLUSTER_NODE_OFFSET
                        && !primarySpec.allocNodes.with(|a| a.contains_key(nodeId))
                    {
                        // L2545
                        let pNode = primarySpec.nodes.with(|m| m.get(nodeId));
                        // L2546
                        if pNode.is_some_and(|pNode| {
                            let ty = pNode.r#type.get();
                            (ty == n!("Normal") || ty == n!("Notable") || ty == n!("Keystone"))
                                && pNode.ascendancyName.get().is_none()
                        }) {
                            // L2547
                            total += 1.0;
                        }
                    }
                }
            }
            // L2552
            if c.items {
                // L2553-2559
                let baseSlots = base_slots(cx, &compareEntry);

                // L2561
                for slotName in &baseSlots {
                    // L2562-2563
                    if slot_item(&cmp, slotName).is_some() {
                        // L2565
                        total += 1.0;
                    }
                }
                // Count jewels for progress tracking
                // L2569
                let jewelSlots = CompareTab::GetJewelComparisonSlots(cx, &compareEntry);
                // L2570
                for jEntry in &jewelSlots {
                    // L2571
                    if jEntry.cItem.is_some() {
                        // L2572
                        total += 1.0;
                    }
                }
            }
            // L2576
            if c.skillGems {
                // L2577-2578
                total += cmp.skillsTab.socketGroupList.get().len() as f64;
            }
            // L2580
            if c.supportGems {
                // L2581
                let cMainGroup = at_num(
                    &cmp.skillsTab.socketGroupList.get().snapshot(),
                    cmp.mainSocketGroup.get(),
                );
                // L2582
                let pMainGroup = at_num(
                    &p.skillsTab.socketGroupList.get().snapshot(),
                    p.mainSocketGroup.get(),
                );
                // L2583
                if let (Some(cMainGroup), Some(pMainGroup)) = (cMainGroup, pMainGroup) {
                    // Count support gems in compared build's main group not in primary's main group
                    // L2585-2592
                    let pSupportNames = support_names(&pMainGroup);
                    // L2593
                    for gem in gems_of(&cMainGroup) {
                        // L2594
                        let ge = CompareTab::GetGemGrantedEffect(&gem);
                        // L2595
                        if let Some(ge) = ge.filter(|ge| ge.support) {
                            // L2596
                            let name = ge.name.to_string();
                            // L2597
                            if !pSupportNames.contains(&name) {
                                // L2598
                                total += 1.0;
                            }
                        }
                    }
                }
            }
            // L2604
            if c.config {
                // L2605
                let pInput = p.configTab.input.clone();
                // L2606
                let cInput = cmp.configTab.input.clone();
                // L2607
                for varData in st(cx, me).configOptions {
                    // L2608
                    if let (Some(var), true, true) = (
                        varData.var,
                        varData.apply.is_some(),
                        !varData.r#type.is_some_and(|t| t == n!("text")),
                    ) {
                        // L2609
                        let (pVal, cVal) = CompareTab::NormalizeConfigVals(
                            varData,
                            pInput.get(var),
                            cInput.get(var),
                        );
                        // L2610
                        if !pVal.raw_eq(&cVal) {
                            // L2611
                            total += 1.0;
                        }
                    }
                }
            }
        });

        // L2617
        if total == 0.0 {
            co::with_cx(|cx| {
                // L2618
                set_results(cx, me, results);
                // L2619
                st_mut(cx, me).comparePowerProgress = 100.0;
            });
            // L2620
            return;
        }

        // Get baseline stat value for percentage calculation
        // L2624
        let baseStatValue = power::GetFromOutput(&calcBase, &statTable, false);

        // ==========================================
        // Tree Nodes
        // ==========================================
        // L2659
        if co::with_cx(|cx| cats(cx).treeNodes) {
            // L2660
            let compareNodes = cmp.spec.get().allocNodes.with(model::LjIntMap::pairs);
            // L2662
            let mut cache: HashMap<String, Output> = HashMap::new();

            // L2664 ORDER: see the module notes
            for (nodeId, _) in compareNodes {
                let counted = co::with_cx(|cx| {
                    let p = cx.build().model.clone();
                    let primarySpec = p.spec.get();
                    // L2665
                    if (nodeId as f64) >= CLUSTER_NODE_OFFSET
                        || primarySpec.allocNodes.with(|a| a.contains_key(nodeId))
                    {
                        return false;
                    }
                    // L2666
                    let Some(pNode) = primarySpec.nodes.with(|m| m.get(nodeId)) else {
                        return false;
                    };
                    let ty = pNode.r#type.get();
                    let modKey = pNode.modKey.get().to_string();
                    // L2667-2668
                    if !((ty == n!("Normal") || ty == n!("Notable") || ty == n!("Keystone"))
                        && pNode.ascendancyName.get().is_none()
                        && !modKey.is_empty())
                    {
                        return false;
                    }
                    // L2669-2675
                    let output = if let Some(o) = cache.get(&modKey) {
                        // L2671
                        o.clone()
                    } else {
                        // L2673
                        let o = call(
                            &calcFunc,
                            Override {
                                addNodes: Field::new(Some(node_set(&pNode))),
                                ..Override::default()
                            },
                            useFullDPS,
                        );
                        // L2674
                        cache.insert(modKey, o.clone());
                        o
                    };
                    // L2676
                    let impact = power::CalculatePowerStat(&statTable, &output, &calcBase);
                    // L2677
                    let mut pathDist = pNode.pathDist.get().unwrap_or(0.0);
                    // L2678
                    if pathDist == 0.0 {
                        // L2679
                        pathDist = pNode.path.get().map_or(0, |p| p.len()) as f64;
                        // L2680
                        if pathDist == 0.0 {
                            pathDist = 1.0;
                        }
                    }
                    // L2682
                    let perPoint = impact / pathDist;
                    // L2683
                    let (impactStr, impactVal, combinedImpactStr, impactPercent, impactIsZero) =
                        formatImpact(cx, &displayStat, baseStatValue, impact);
                    // L2684
                    let (perPointStr, ..) = formatImpact(cx, &displayStat, baseStatValue, perPoint);

                    // L2686
                    if !impactIsZero {
                        // L2687-2700
                        results.push(Rc::new(PowerResult {
                            category: "Tree",
                            categoryColor: "^7".to_owned(),
                            nameColor: "^7".to_owned(),
                            name: pNode.dn.get().to_string(),
                            nodeId: Some(nodeId),
                            itemObj: None,
                            slotName: None,
                            impact: impactVal,
                            impactStr,
                            impactPercent,
                            combinedImpactStr,
                            pathDist: Some(pathDist),
                            perPoint: Some(perPoint * displayStat.mult()),
                            perPointStr: Some(perPointStr),
                        }));
                    }
                    true
                });
                if counted {
                    // L2703-2708
                    step(me, &mut processed, total, &mut start).await;
                }
            }
        }

        // ==========================================
        // Items
        // ==========================================
        // L2717
        if co::with_cx(|cx| cats(cx).items) {
            // L2718-2724
            let baseSlots = co::with_cx(|cx| base_slots(cx, &compareEntry));

            // L2726
            for slotName in &baseSlots {
                co::with_cx(|cx| {
                    Self::rate_item(
                        cx,
                        &compareEntry,
                        slotName,
                        &calcFunc,
                        &calcBase,
                        &statTable,
                        useFullDPS,
                        &displayStat,
                        baseStatValue,
                        &mut results,
                    )
                });
                // L2805-2810
                step(me, &mut processed, total, &mut start).await;
            }
        }

        // ==========================================
        // Jewels (included as items)
        // ==========================================
        // L2817
        if co::with_cx(|cx| cats(cx).items) {
            // Build list of jewel socket info in the primary build for fallback testing
            // Each entry has { slotName, nodeId, node, allocated }
            let (primaryJewelSockets, jewelSlots) = co::with_cx(|cx| {
                let p = cx.build().model.clone();
                // L2820
                let pSpec = p.spec.get();
                // L2821
                let mut primaryJewelSockets: Vec<(String, NodeRef, bool)> = Vec::new();
                // L2822
                for slot in p.itemsTab.orderedSlots.snapshot() {
                    // L2823
                    if let Some(nodeId) = slot.get(n!("nodeId")).opt_num() {
                        let nodeId = nodeId as i64;
                        // L2824
                        let node = pSpec.nodes.with(|m| m.get(nodeId));
                        // L2825
                        let allocated = pSpec.allocNodes.with(|a| a.contains_key(nodeId));
                        // L2826
                        if let Some(node) = node {
                            // L2827-2832
                            let slotName = slot
                                .str(n!("slotName"))
                                .map(|s| s.to_string())
                                .unwrap_or_default();
                            primaryJewelSockets.push((slotName, node, allocated));
                        }
                    }
                }
                // L2837
                (
                    primaryJewelSockets,
                    CompareTab::GetJewelComparisonSlots(cx, &compareEntry),
                )
            });

            // L2838
            for jEntry in &jewelSlots {
                co::with_cx(|cx| {
                    // L2839
                    let Some(cItem) = jEntry.cItem.clone() else {
                        return;
                    };
                    if jEntry
                        .pItem
                        .as_ref()
                        .is_some_and(|p| p.name.get() == cItem.name.get())
                    {
                        return;
                    }
                    // L2840
                    let newItem = new_Item(Some(&cItem.raw.get()), None, false);
                    // L2841
                    newItem.NormaliseQuality();

                    // L2843
                    let mut bestImpactVal: Option<f64> = None;
                    // L2844
                    let mut bestSlotLabel = jEntry.label.clone();

                    // L2846
                    if jEntry.pNodeAllocated {
                        // Socket is allocated in primary build, test directly in that socket
                        // L2848
                        let output = call(
                            &calcFunc,
                            Override {
                                repSlotName: Field::new(Some(Name::from(
                                    jEntry.cSlotName.as_str(),
                                ))),
                                repItem: Field::new(Some(newItem.clone())),
                                ..Override::default()
                            },
                            useFullDPS,
                        );
                        // L2849
                        bestImpactVal =
                            Some(power::CalculatePowerStat(&statTable, &output, &calcBase));
                    } else {
                        // Socket is NOT allocated in primary build; try the jewel in every
                        // jewel socket on the primary build's tree, temporarily allocating
                        // unallocated sockets via addNodes so CalcSetup doesn't skip them
                        // L2854
                        for (slotName, node, allocated) in &primaryJewelSockets {
                            // L2855
                            let mut ov = Override {
                                repSlotName: Field::new(Some(Name::from(slotName.as_str()))),
                                repItem: Field::new(Some(newItem.clone())),
                                ..Override::default()
                            };
                            // L2856
                            if !allocated {
                                // L2857
                                ov.addNodes = Field::new(Some(node_set(node)));
                            }
                            // L2859
                            let output = call(&calcFunc, ov, useFullDPS);
                            // L2860
                            let impact = power::CalculatePowerStat(&statTable, &output, &calcBase);
                            // L2861
                            if bestImpactVal.map_or(true, |b| impact > b) {
                                // L2862
                                bestImpactVal = Some(impact);
                                // L2863
                                bestSlotLabel = format!("{} (best socket)", jEntry.label);
                            }
                        }
                    }

                    // L2868
                    if let Some(best) = bestImpactVal {
                        // L2869
                        let f = formatImpact(cx, &displayStat, baseStatValue, best);
                        // L2870
                        if !f.4 {
                            // L2871
                            let rarityColor = rarity_color(cx, &cItem);
                            // L2873-2886
                            results.push(entry(
                                "Item",
                                rarityColor.clone(),
                                rarityColor,
                                format!("{}, {bestSlotLabel}", cItem.name.get()),
                                Some(newItem),
                                None,
                                f,
                            ));
                        }
                    }
                });
                // L2890-2895
                step(me, &mut processed, total, &mut start).await;
            }
        }

        // ==========================================
        // Skill Gems (socket groups)
        // ==========================================
        // L2902
        if co::with_cx(|cx| cats(cx).skillGems) {
            // L2903
            let cGroups = cmp.skillsTab.socketGroupList.get().snapshot();
            // L2904 (the list itself: `t_insert`/`t_remove` below change it)
            let pGroupsField = co::with_cx(|cx| cx.build().model.clone());
            let pGroups = pGroupsField.skillsTab.socketGroupList.get();

            // Build signature set for primary groups
            // L2907
            let mut pSignatures: HashSet<String> = HashSet::new();
            // L2908
            for group in pGroups.snapshot() {
                // L2909
                pSignatures.insert(CompareTab::GetSocketGroupSignature(&group));
            }

            // L2912
            for cGroup in &cGroups {
                co::with_cx(|cx| {
                    let p = cx.build().model.clone();
                    // L2913
                    let sig = CompareTab::GetSocketGroupSignature(cGroup);
                    // L2914
                    if sig.is_empty() || pSignatures.contains(&sig) {
                        return;
                    }
                    // Temporarily add this socket group to primary build and recalculate
                    // L2916
                    pGroups.push(cGroup.clone());
                    // L2917
                    p.buildFlag.set(true);

                    // Get a fresh calculator with the added group (pcall to guarantee cleanup)
                    // L2920-2922
                    let r = pcall(|| ready(misc_calculator(cx)));

                    // Always remove the temporarily added group
                    // L2925
                    pGroups.remove(None);
                    // L2926
                    p.buildFlag.set(true);

                    match r {
                        // L2928
                        Err(e) => {
                            // gemCalcFunc contains the error message on failure; skip this group
                            // L2930
                            con_printf(cx, "Compare power (gem): %s", &[FmtArg::from(e.as_str())]);
                        }
                        Ok((_gemCalcFunc, gemCalcBase)) => {
                            // L2932
                            let impact =
                                power::CalculatePowerStat(&statTable, &gemCalcBase, &calcBase);
                            // L2933
                            let f = formatImpact(cx, &displayStat, baseStatValue, impact);
                            // L2934
                            if !f.4 {
                                // L2935
                                let label = CompareTab::GetSocketGroupLabel(cGroup);
                                let gem = color(cx, "GEM");
                                // L2937-2949
                                results.push(entry(
                                    "Skill gem",
                                    gem.clone(),
                                    gem,
                                    label,
                                    None,
                                    None,
                                    f,
                                ));
                            }
                        }
                    }
                });
                // L2953-2958
                step(me, &mut processed, total, &mut start).await;
            }
        }

        // ==========================================
        // Support Gems (from compared build's active skill)
        // ==========================================
        // L2965
        if co::with_cx(|cx| cats(cx).supportGems) {
            let p = co::with_cx(|cx| cx.build().model.clone());
            // L2966
            let cMainGroup = at_num(
                &cmp.skillsTab.socketGroupList.get().snapshot(),
                cmp.mainSocketGroup.get(),
            );
            // L2967
            let pMainGroup = at_num(
                &p.skillsTab.socketGroupList.get().snapshot(),
                p.mainSocketGroup.get(),
            );

            // L2969
            if let (Some(cMainGroup), Some(pMainGroup)) = (cMainGroup, pMainGroup) {
                // Collect support gem names already in primary build's main group
                // L2971-2978
                let pSupportNames = support_names(&pMainGroup);

                // L2980
                for cGem in gems_of(&cMainGroup) {
                    // L2981
                    let cGrantedEffect = CompareTab::GetGemGrantedEffect(&cGem);
                    // L2982
                    let Some(cGrantedEffect) = cGrantedEffect.filter(|ge| ge.support) else {
                        continue;
                    };
                    // L2983
                    let name = cGrantedEffect.name.to_string();
                    // L2984
                    if pSupportNames.contains(&name) {
                        continue;
                    }
                    co::with_cx(|cx| {
                        // Create a temporary copy of this support gem
                        // L2986-2997
                        let tempGem = GemInstance {
                            nameSpec: Field::new(cGem.nameSpec.get()),
                            level: std::cell::Cell::new(cGem.level.get()),
                            quality: std::cell::Cell::new(cGem.quality.get()),
                            enabled: std::cell::Cell::new(cGem.enabled.get()),
                            grantedEffect: Field::new(cGem.grantedEffect.get()),
                            gemData: Field::new(cGem.gemData.get()),
                            count: std::cell::Cell::new(cGem.count.get()),
                            enableGlobal1: std::cell::Cell::new(cGem.enableGlobal1.get()),
                            enableGlobal2: std::cell::Cell::new(cGem.enableGlobal2.get()),
                            ..GemInstance::default()
                        };
                        // `qualityId = cGem.qualityId` (module notes)
                        tempGem
                            .extra
                            .set(n!("qualityId"), cGem.extra.get(n!("qualityId")));
                        let gemList = pMainGroup.gemList.get();

                        // Temporarily add to primary build's main socket group
                        // L3000
                        gemList.push(Rc::new(tempGem));
                        // L3001
                        p.buildFlag.set(true);

                        // L3003-3005
                        let r = pcall(|| ready(misc_calculator(cx)));

                        // Always remove the temporarily added gem
                        // L3008
                        gemList.remove(None);
                        // L3009
                        p.buildFlag.set(true);

                        match r {
                            // L3011
                            Err(e) => {
                                // L3012
                                con_printf(
                                    cx,
                                    "Compare power (support gem): %s",
                                    &[FmtArg::from(e.as_str())],
                                );
                            }
                            Ok((_sgCalcFunc, sgCalcBase)) => {
                                // L3014
                                let impact =
                                    power::CalculatePowerStat(&statTable, &sgCalcBase, &calcBase);
                                // L3015
                                let f = formatImpact(cx, &displayStat, baseStatValue, impact);

                                // L3017
                                if !f.4 {
                                    let gem = color(cx, "GEM");
                                    // L3018-3030
                                    results.push(entry(
                                        "Support gem",
                                        gem.clone(),
                                        gem,
                                        name.clone(),
                                        None,
                                        None,
                                        f,
                                    ));
                                }
                            }
                        }
                    });
                    // L3033-3038
                    step(me, &mut processed, total, &mut start).await;
                }
            }
        }

        // ==========================================
        // Config Options
        // ==========================================
        // L3048
        if co::with_cx(|cx| cats(cx).config) {
            let (p, configOptions) =
                co::with_cx(|cx| (cx.build().model.clone(), st(cx, me).configOptions));
            // L3049
            let pInput = p.configTab.input.clone();
            // L3050
            let cInput = cmp.configTab.input.clone();

            // L3052-3054
            let stripColors = |s: &str| -> String {
                let (s, _) = pob_lua::pattern::gsub(s, "%^%x", "", None);
                let (s, _) = pob_lua::pattern::gsub(&s, "%^x%x%x%x%x%x%x", "", None);
                s
            };

            // L3056
            for varData in configOptions {
                // L3057
                let Some(var) = varData.var else { continue };
                if varData.apply.is_none() || varData.r#type.is_some_and(|t| t == n!("text")) {
                    continue;
                }
                // L3058
                let pVal = pInput.get(var);
                // L3059
                let cVal = cInput.get(var);
                // L3060
                let (pNorm, cNorm) =
                    CompareTab::NormalizeConfigVals(varData, pVal.clone(), cVal.clone());

                // L3062
                if pNorm.raw_eq(&cNorm) {
                    continue;
                }
                co::with_cx(|cx| {
                    // Save original value
                    // L3064
                    let savedVal = pInput.get(var);

                    // Apply compare build's config value
                    // L3067
                    pInput.set(var, cVal.clone());

                    // Rebuild and calculate (pcall to guarantee restore on error)
                    // L3070-3074
                    let r = pcall(|| {
                        // L3071
                        p.configTab.BuildModList(&p);
                        // L3072
                        p.buildFlag.set(true);
                        // L3073
                        ready(misc_calculator(cx))
                    });

                    // Always restore original value
                    // L3077
                    pInput.set(var, savedVal);
                    // L3078
                    p.configTab.BuildModList(&p);
                    // L3079
                    p.buildFlag.set(true);

                    match r {
                        // L3081
                        Err(e) => {
                            // cfgCalcFunc contains the error message on failure; skip this config
                            // L3083
                            con_printf(
                                cx,
                                "Compare power (config): %s",
                                &[FmtArg::from(e.as_str())],
                            );
                        }
                        Ok((_cfgCalcFunc, cfgCalcBase)) => {
                            // L3085
                            let impact =
                                power::CalculatePowerStat(&statTable, &cfgCalcBase, &calcBase);
                            // L3086
                            let f = formatImpact(cx, &displayStat, baseStatValue, impact);

                            // Only include configs with non-zero impact
                            // L3089
                            if !f.4 {
                                // Build display name with value change description
                                // L3091
                                let displayName = varData
                                    .label
                                    .as_deref()
                                    .map_or_else(|| var.to_string(), str::to_owned);
                                // L3092
                                let (displayName, _) =
                                    pob_lua::pattern::gsub(&displayName, ":$", "", None);

                                // L3094
                                let pDisplay =
                                    stripColors(&CompareTab::FormatConfigValue(cx, varData, &pVal));
                                // L3095
                                let cDisplay =
                                    stripColors(&CompareTab::FormatConfigValue(cx, varData, &cVal));

                                // L3097-3109
                                results.push(entry(
                                    "Config",
                                    color(cx, "FRACTURED"),
                                    "^7".to_owned(),
                                    format!("{displayName}  ({pDisplay} -> {cDisplay})"),
                                    None,
                                    None,
                                    f,
                                ));
                            }
                        }
                    }
                });
                // L3113-3118
                step(me, &mut processed, total, &mut start).await;
            }
        }

        co::with_cx(|cx| {
            // L3124
            set_results(cx, me, results);
            // L3125
            st_mut(cx, me).comparePowerProgress = 100.0;
        });
    }

    /// One slot of the Items section (L2727-2804).
    #[allow(clippy::too_many_arguments)]
    fn rate_item(
        cx: &mut Cx,
        compareEntry: &CompareEntryRef,
        slotName: &str,
        calcFunc: &Rc<dyn MiscCalc>,
        calcBase: &Output,
        statTable: &Table,
        useFullDPS: bool,
        displayStat: &DisplayFmt,
        baseStatValue: f64,
        results: &mut Vec<Rc<PowerResult>>,
    ) {
        let p = cx.build().model.clone();
        let cmp = &compareEntry.model;
        // L2727-2728
        let cItem = slot_item(cmp, slotName);
        // L2729-2730
        let pItem = slot_item(&p, slotName);
        // L2731
        let Some(cItem) = cItem else { return };
        if pItem.is_some_and(|pItem| pItem.name.get() == cItem.name.get()) {
            return;
        }
        // L2732
        let newItem = new_Item(Some(&cItem.raw.get()), None, false);
        // L2733
        newItem.NormaliseQuality();

        // if our comparison has abyssal jewels, but the primary build
        // doesn't, add those temporarily to the build to work around
        // calcFunc not being able to take in multiple items
        // L2738
        let mut cmpJewels: Vec<ItemRef> = Vec::new();
        // L2739 ORDER: see the module notes
        let mut oldEquipped: indexmap::IndexMap<String, f64> = indexmap::IndexMap::new();
        let h = items_hooks();
        let abyssalSocketCount = newItem.abyssalSocketCount.get();
        // L2740
        if abyssalSocketCount > 0.0 {
            // L2741
            let mut idx = 1.0;
            while idx <= abyssalSocketCount {
                // L2742
                let abyssSlotName = pob_lua::lua_format(
                    "%s Abyssal Socket %d",
                    &[FmtArg::from(slotName), FmtArg::from(idx)],
                );
                // L2743
                let cmpJewelSlot = slot_at(cmp, &abyssSlotName).unwrap_or_else(|| {
                    lua_error!("attempt to index local 'cmpJewelSlot' (a nil value)")
                });
                // save old id and unequip existing
                // L2745
                let primaryJewelSlot = slot_at(&p, &abyssSlotName).unwrap_or_else(|| {
                    lua_error!("attempt to index local 'primaryJewelSlot' (a nil value)")
                });
                // L2746
                oldEquipped.insert(
                    abyssSlotName.clone(),
                    primaryJewelSlot.get(n!("selItemId")).num(),
                );
                // L2747
                set_sel_item_id(&p, &primaryJewelSlot, 0.0);
                // L2748
                let cmpSel = cmpJewelSlot.get(n!("selItemId"));
                if cmpSel.num() > 0.0 {
                    // L2749
                    let cmpJewel = item_at(cmp, &cmpSel);
                    // due to a previous bug where jewel slots didn't
                    // get cleared when becoming inactive, so the item
                    // might not exist
                    // L2753
                    if let Some(cmpJewel) = cmpJewel {
                        // copy item
                        // L2755
                        let itemCopy = new_Item(Some(&cmpJewel.BuildRaw()), None, false);
                        // L2756
                        cmpJewels.push(itemCopy.clone());
                        // L2757
                        (h.add_item)(cx, &itemCopy, false, None);

                        // equip copied
                        // L2760
                        let slot = slot_at(&p, &abyssSlotName).unwrap_or_else(|| {
                            lua_error!("attempt to index field '?' (a nil value)")
                        });
                        let id = itemCopy.id.get().unwrap_or_else(|| {
                            lua_error!("bad argument (number expected, got nil)")
                        });
                        set_sel_item_id(&p, &slot, id);
                    }
                }
                idx += 1.0;
            }
        }

        // L2767
        let output = call(
            calcFunc,
            Override {
                repSlotName: Field::new(Some(Name::from(slotName))),
                repItem: Field::new(Some(newItem.clone())),
                ..Override::default()
            },
            useFullDPS,
        );
        // L2768
        let impact = power::CalculatePowerStat(statTable, &output, calcBase);
        // L2769
        let f = formatImpact(cx, displayStat, baseStatValue, impact);

        // restore abyss jewel state
        // L2772
        if newItem.abyssalSocketCount.get() > 0.0 {
            // L2773
            for (k, v) in &oldEquipped {
                // L2774
                let slot = slot_at(&p, k)
                    .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
                set_sel_item_id(&p, &slot, *v);
            }
            // L2776
            for item in &cmpJewels {
                // L2777
                (h.delete_item)(cx, item, false);
            }
        }

        // L2781
        if !f.4 {
            // Get rarity color for item name
            // L2783
            let rarityColor = rarity_color(cx, &cItem);

            // L2786-2787
            let n = cmpJewels.len();
            let abyssJewelText = if n > 0 {
                pob_lua::lua_format(
                    ", %d Jewel%s",
                    &[
                        FmtArg::from(n as f64),
                        FmtArg::from(if n > 1 { "s" } else { "" }),
                    ],
                )
            } else {
                String::new()
            };
            // L2788-2802
            results.push(entry(
                "Item",
                rarityColor.clone(),
                rarityColor,
                format!("{}, {slotName}{abyssJewelText}", cItem.name.get()),
                Some(newItem),
                Some(slotName.to_owned()),
                f,
            ));
        }
    }

    /// Drive the compare power report coroutine
    /// lua: CompareTab.lua:3128-3157 `CompareTabClass:RunComparePowerReport(compareEntry)`
    /// @pob Classes/CompareTab.lua:3128-3157 h=b5a33cc2
    pub fn RunComparePowerReport(cx: &mut Cx, me: CtrlId, compareEntry: &CompareEntryRef) {
        // Invalidate if compare entry changed
        // L3131
        let same = cx
            .w::<CompareTab>(me)
            .power
            .comparePowerCompareId
            .as_ref()
            .is_some_and(|e| Rc::ptr_eq(e, compareEntry));
        if !same {
            // L3132
            cx.w_mut::<CompareTab>(me).power.comparePowerCompareId = Some(compareEntry.clone());
            // L3133
            st_mut(cx, me).comparePowerDirty = true;
        }

        // Start new calculation if dirty
        // L3137
        if let (true, Some(stat)) = (st(cx, me).comparePowerDirty, st(cx, me).comparePowerStat) {
            let s = st_mut(cx, me);
            // L3138
            s.comparePowerDirty = false;
            // L3139
            s.comparePowerResults = None;
            // L3140
            s.comparePowerProgress = 0.0;
            // L3141
            s.comparePowerListSynced = false;
            // L3142-3144
            s.comparePowerCoroutine = Some(Coroutine::create(CompareTab::ComparePowerBuilder(
                me,
                compareEntry.clone(),
                stat,
            )));
        }

        // Resume coroutine
        // L3148
        if st(cx, me).comparePowerCoroutine.is_some() {
            // L3149
            let (res, errMsg, dead) = cx.lend::<CompareTab, Option<Coroutine>, _>(
                me,
                |w| &mut w.init.comparePowerCoroutine,
                |cx, slot| {
                    let co = slot.as_mut().expect("checked above");
                    #[cfg(not(target_arch = "wasm32"))]
                    let (res, err) = co.try_resume(cx);
                    #[cfg(target_arch = "wasm32")]
                    let (res, err) = (co.resume(cx), None::<String>);
                    (res, err, co.is_dead())
                },
            );
            // L3150
            if cx.app().launch.dev_mode && !res {
                // L3151
                lua_error!("{}", errMsg.unwrap_or_default());
            }
            // L3153
            if dead {
                // L3154
                st_mut(cx, me).comparePowerCoroutine = None;
            }
        }
    }
}
