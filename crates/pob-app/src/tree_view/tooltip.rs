//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/PassiveTreeView.lua:1412-1856 (AddNodeTooltip, AddCompareNodeTooltip).
//! Owner: U24.
//!
//! @pob-file Classes/PassiveTreeView.lua @16de4b82d
//!
//! `build` (the Lua argument) is the open build, `cx.build().model` (see `names.rs`). The
//! methods are plugged into U23's [`TreeViewHooks`](super::TreeViewHooks) by
//! [`super::default_hooks`] (CCR-0115 A: "U24 may give `PassiveTreeView` the methods in its own
//! files and point the default at them"); `returnEarly` (TreeTab.lua:1560/1598/1732, U25) is the
//! last argument of [`PassiveTreeView::add_node_tooltip`].
//!
//! Calls into other units:
//! * `build.itemsTab:AddItemTooltip(tooltip, item, slot)` (U38) and the compare spec's
//!   `self.compareSpec.build.itemsTab:AddItemTooltip` go through U32's
//!   `ItemsTabHooks::add_item_tooltip` with the model build meant: `build` (the drawn build,
//!   `cx.build().model`) or `compareSpec.build` (the spec's `Weak` build). TreeTab's compare spec
//!   (TreeTab.lua:451) is a spec of the open build; CompareTab's (CompareTab.lua:3421, U46) is a
//!   comparison entry's, whose ItemsTab the hook selects (FUP6, CCR-0122 §2).
//! * `socket:IsEnabled()` (L1517): the socket is the ItemSlotControl whose model table
//!   `GetSocketAndJewelForNodeID` returns (calc:BM-A); its `enabled` (ItemSlotControl.lua:31-33,
//!   U32) is `#self.items > 1` over that table's `items`.
//! * `build.calcsTab:GetMiscCalculator(build)` is the facade's `misc_calculator`;
//!   `build:AddStatComparesToTooltip` is U21's `stats::add_stat_compares_to_tooltip`;
//!   `gemTooltip.AddGemTooltip` is U29's `gem_tooltip::add_gem_tooltip`.
//!
//! Lua semantics:
//! * `{ [node] = true }` override sets are id maps of the node (the calc's `Override.addNodes /
//!   removeNodes`, CCR-0082 §2). `{ removeNodes = { [node.id] = true } }` (L1706, a node granted
//!   by an item) is the id half of CCR-0105 §1: an empty node-keyed `removeNodes` (the Lua table
//!   exists, so CalcSetup.lua:714 takes the same branch) plus `override.extra["$removeNodeIds"]`,
//!   which CS2 reads at CalcSetup.lua:1393/1408.
//! * `node.intuitiveLeapLikesAffecting == 0` (L1727) compares a table with a number: always
//!   false, so the " or the nodes leading to it" suffix is never added (LUA-QUIRK, kept).
//!   `#node.intuitiveLeapLikesAffecting` (L1720, L1743) raises when the field is nil, as in Lua.
//! * `pathOutput` is nil when `pathLength <= 1` or on the reallocation branch (L1695-1697);
//!   L1721 then passes nil, which AddStatComparesToTooltip indexes (raises, as in Lua).
//! * `data.goldRespecPrices[build.characterLevel]` is nil outside 1..=100 (or for a non-integral
//!   level); the arithmetic on it raises, as in Lua.
//!
//! DIVERGENCE(wasm-calc): when the facade answers `Pending` (wasm only, ui doc §10.4), the stat
//! difference block shows only its separator and the Ctrl+D tip; the tooltip is rebuilt at the
//! next `CheckForUpdate` change. Desktop is always `Ready`.
//!
//! ORDER: L1689 `pairs(path)` fills a set (order-free); L1548 `pairs(node.mods[i].list)` walks a
//! sequence (the array part, in order); `isValueInTable` (L1603) only tests membership (c).

use std::rc::Rc;
use std::task::Poll;

use pob_lua::{lfmt, n, LjIntMap, Shared};
use pob_sg::Font;
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::Tooltip;

use super::compare::compare_jewels_equal;
use super::PassiveTreeView;
use crate::app::CxApp;
use crate::build::stats::add_stat_compares_to_tooltip;
use crate::controls::gem_tooltip::{
    add_gem_tooltip, GemTooltipOptions, TooltipGemData, TooltipGemInstance,
};
use crate::model::{Node, NodeRef, Output, Override, Table, Val};
use crate::tabs::items::slot_helper::items_hooks;

/// `#node.intuitiveLeapLikesAffecting` (nil raises).
fn intuitive_leap_len(node: &Node) -> usize {
    match node.extra.get(n!("intuitiveLeapLikesAffecting")) {
        Val::Table(t) => t.len(),
        other => lua_error!(
            "attempt to get length of field 'intuitiveLeapLikesAffecting' (a {} value)",
            other.lua_type()
        ),
    }
}

/// `node.<k>` as a list of strings (`reminderText`, `flavourText`), nil when absent.
fn str_list(node: &Node, k: &str) -> Option<Vec<String>> {
    node.extra.table(pob_lua::Name::from(k)).map(|t| {
        t.ipairs()
            .into_iter()
            .map(|(_, v)| match v {
                Val::Str(s) => s.to_string(),
                Val::Num(x) => pob_lua::tostring(x),
                other => lua_error!(
                    "attempt to concatenate local 'line' (a {} value)",
                    other.lua_type()
                ),
            })
            .collect()
    })
}

/// A node-keyed override set (`{ [node] = true }`, module notes).
fn node_set(nodes: &[NodeRef]) -> Shared<LjIntMap<NodeRef>> {
    let m = Shared::new(LjIntMap::new());
    for node in nodes {
        m.with_mut(|m| m.set(node.id as i64, Some(node.clone())));
    }
    m
}

/// `{ addNodes = set }`
fn add_nodes(set: Shared<LjIntMap<NodeRef>>) -> Rc<Override> {
    let ov = Override::default();
    ov.addNodes.set(Some(set));
    Rc::new(ov)
}

/// `{ removeNodes = set }`
fn remove_nodes(set: Shared<LjIntMap<NodeRef>>) -> Rc<Override> {
    let ov = Override::default();
    ov.removeNodes.set(Some(set));
    Rc::new(ov)
}

/// `{ removeNodes = { [nodeId] = true } }` (module notes; CCR-0105 §1).
fn remove_node_id(node_id: f64) -> Rc<Override> {
    let ov = Override::default();
    ov.removeNodes.set(Some(Shared::new(LjIntMap::new())));
    let ids = Table::new();
    ids.set(node_id, true);
    ov.extra.set(n!("$removeNodeIds"), ids);
    Rc::new(ov)
}

/// `calcFunc(override)` on the desktop facade (always Ready; module notes for wasm).
fn call(
    calc_func: &Rc<dyn crate::calc::facade::MiscCalc>,
    ov: Option<Rc<Override>>,
) -> Option<Output> {
    match calc_func.call(ov, None) {
        Poll::Ready(o) => Some(o),
        Poll::Pending => None,
    }
}

/// `socket:IsEnabled()` for a socket's slot table (module notes).
fn socket_is_enabled(socket: Option<&Table>) -> bool {
    let Some(socket) = socket else {
        lua_error!("attempt to index local 'socket' (a nil value)")
    };
    socket.table(n!("items")).map_or(0, |t| t.len()) > 1
}

impl PassiveTreeView {
    /// lua: PassiveTreeView.lua:1476-1770 `PassiveTreeView:AddNodeTooltip(tooltip, node, build,
    /// returnEarly)`
    /// @pob Classes/PassiveTreeView.lua:1476-1770 h=2bd0fcbf
    pub fn add_node_tooltip(
        &mut self,
        cx: &mut Cx,
        tooltip: &mut Tooltip,
        node: &NodeRef,
        return_early: bool,
    ) {
        let fs = Some(Font::Fontin);
        let build = cx.build().model.clone();
        let cc = cx.main.cc.clone();
        // L1477
        let font_size_big = if cx.main.show_flavour_text {
            18.0
        } else {
            16.0
        };
        // L1478
        self.skill_tooltip.clear(false);
        // L1479
        tooltip.center = true;
        // L1480
        tooltip.max_width = Some(800.0);
        // Appends the compare spec's jewel tooltip if it has a jewel in this allocated socket.
        // L1482-1494 local function addCompareJewelSection(socket, withLabel)
        let add_compare_jewel_section = |view: &PassiveTreeView,
                                         cx: &mut Cx,
                                         tooltip: &mut Tooltip,
                                         socket: Option<&Table>,
                                         with_label: bool|
         -> bool {
            // L1483
            let c_jewel = view
                .compare_spec
                .as_ref()
                .and_then(|_| view.get_compare_jewel(node.id));
            // L1484
            let c_allocated = view
                .compare_spec
                .as_ref()
                .is_some_and(|cs| cs.allocNodes.with(|m| m.get(node.id as i64)).is_some());
            // L1485
            let Some(c_jewel) = c_jewel.filter(|_| c_allocated) else {
                // L1486
                return false;
            };
            // L1488
            if with_label {
                // L1489
                tooltip.add_separator(cx, Some(14.0));
                // L1490
                tooltip.add_line(
                    cx,
                    14.0,
                    &format!("{}Compared build:", cx.main.cc.DEXTERITY),
                );
            }
            // L1492 (`self.compareSpec.build`: the compare spec's own build, FUP6)
            let c_build = view
                .compare_spec
                .as_ref()
                .and_then(|cs| cs.build.upgrade())
                .unwrap_or_else(|| lua_error!("attempt to index field 'build' (a nil value)"));
            let h = items_hooks().add_item_tooltip;
            h(cx, &c_build, tooltip, &c_jewel, socket, false);
            // L1493
            true
        };

        let ty = node.r#type.get();
        // Special case for sockets
        // L1497
        if &*ty == "Socket" && node.alloc.get() {
            // L1498
            let (socket, jewel) = build.itemsTab.GetSocketAndJewelForNodeID(node.id);
            // L1499
            let c_jewel = if self.compare_spec.is_some() {
                self.get_compare_jewel(node.id)
            } else {
                None
            };
            // L1500
            if let Some(jewel) = &jewel {
                // L1501
                let h = items_hooks().add_item_tooltip;
                h(cx, &build, tooltip, jewel, socket.as_ref(), false);
                // L1502
                if !compare_jewels_equal(Some(jewel), c_jewel.as_ref()) {
                    // L1503
                    add_compare_jewel_section(self, cx, tooltip, socket.as_ref(), true);
                }
            } else if !add_compare_jewel_section(self, cx, tooltip, socket.as_ref(), false) {
                // L1506
                self.add_node_name(cx, tooltip, node);
            }
            // L1508
            let c_alloc = self
                .compare_spec
                .as_ref()
                .is_some_and(|cs| cs.allocNodes.with(|m| m.get(node.id as i64)).is_some());
            if c_alloc && c_jewel.is_none() {
                // L1509
                tooltip.add_separator(cx, Some(14.0));
                // L1510
                tooltip.add_line(
                    cx,
                    16.0,
                    &format!("{}Compared build: ^7(empty socket)", cc.WARNING),
                );
            }
            // L1512
            if let Some(d) = node.distanceToClassStart.get().filter(|d| *d > 0.0) {
                // L1513
                tooltip.add_separator(cx, Some(14.0));
                // L1514
                tooltip.add_line(cx, 16.0, &lfmt!("^7Distance to start: %d", d));
            }
            // L1516
            tooltip.add_separator(cx, Some(14.0));
            // L1517
            if socket_is_enabled(socket.as_ref()) {
                // L1518
                tooltip.add_line(
                    cx,
                    14.0,
                    &format!(
                        "{}Tip: Right click this socket to go to the items page and choose the jewel for this socket.",
                        cc.TIP
                    ),
                );
            }
            // L1520
            tooltip.add_line(
                cx,
                14.0,
                &format!("{}Tip: Hold Shift or Ctrl to hide this tooltip.", cc.TIP),
            );
            // L1521
            return;
        }

        // For unallocated sockets, show compare build's jewel if it has one
        // L1525
        if &*ty == "Socket" && !node.alloc.get() {
            // L1526
            let (socket, _) = build.itemsTab.GetSocketAndJewelForNodeID(node.id);
            // L1527
            if add_compare_jewel_section(self, cx, tooltip, socket.as_ref(), false) {
                // L1528
                tooltip.add_line(
                    cx,
                    14.0,
                    &format!("{}Tip: Hold Shift or Ctrl to hide this tooltip.", cc.TIP),
                );
                // L1529
                return;
            }
        }

        // Node name
        // L1534
        self.add_node_name(cx, tooltip, node);
        // L1535
        tooltip.center = false;
        let dev_mode_alt = cx.app().launch.dev_mode_alt;
        // L1536
        if dev_mode_alt {
            // L1537
            let power = node.extra.get(n!("power"));
            if power.truthy() && power.get(n!("offence")).truthy() {
                // Power debugging info
                // L1539
                tooltip.add_line(
                    cx,
                    16.0,
                    &lfmt!(
                        "DPS power: %g   Defence power: %g",
                        power.get(n!("offence")).num(),
                        power.get(n!("defence")).num()
                    ),
                );
            }
        }

        // L1543-1566 local function addModInfoToTooltip(node, i, line)
        let add_mod_info_to_tooltip =
            |cx: &mut Cx, tooltip: &mut Tooltip, node: &Node, i: usize, line: String| {
                let mods = node.mods.get();
                // L1544
                let Some(m) = mods.with(|v| v.get(i - 1).cloned()) else {
                    return;
                };
                let mut line = line;
                let list = m.get(n!("list"));
                let extra = m.get(n!("extra"));
                // L1545
                if dev_mode_alt && list.truthy() {
                    // Modifier debugging info
                    // L1547
                    let mut mod_str: Option<String> = None;
                    // L1548 (ORDER: module notes)
                    let entries = list.table().map(|t| t.ipairs()).unwrap_or_default();
                    for (_, md) in entries {
                        let text = match &md {
                            Val::Mod(md) => crate::model::gamedata::mod_tools::formatMod(md),
                            other => lua_error!(
                                "modLib.formatMod: a {} value in node.mods[i].list",
                                other.lua_type()
                            ),
                        };
                        // L1549
                        mod_str = Some(format!(
                            "{}{text}",
                            mod_str.map_or_else(|| "^2".to_owned(), |s| format!("{s}, "))
                        ));
                    }
                    // L1551
                    if extra.truthy() {
                        // L1552
                        mod_str = Some(format!(
                            "{}{}{}",
                            mod_str.map_or_else(String::new, |s| format!("{s}  ")),
                            cx.main.cc.NEGATIVE,
                            concat_val(&extra)
                        ));
                    }
                    // L1554
                    if let Some(mod_str) = mod_str {
                        // L1555
                        line = format!("{line}  {mod_str}");
                    }
                }
                // L1558
                if line != " " && (extra.truthy() || !list.truthy()) {
                    // L1559
                    let mut line = format!("{}{line}", cx.main.cc.UNSUPPORTED);
                    // L1560
                    if cx.main.not_supported_mod_tooltips {
                        line = format!("{line}{}", cx.app().main.not_supported_tooltip_text);
                    }
                    // L1561
                    tooltip.add_line_ex(cx, font_size_big, &line, fs, None, None);
                } else {
                    // L1563
                    tooltip.add_line_ex(
                        cx,
                        font_size_big,
                        &format!("{}{line}", cx.main.cc.MAGIC),
                        fs,
                        None,
                        None,
                    );
                }
            };

        // If node is a Mastery node, check if compare tree is on
        // If so, check if the left hand tree is unallocated, but the right hand tree is allocated.
        // If so, set the node variable to be the node element from the right hand tree and change the Mastery color
        // Then continue processing as normal
        // L1572
        let mut mastery_color = String::new();
        // L1573
        let mut m_node: NodeRef = node.clone();
        // L1574
        let compare_spec_node = self
            .compare_spec
            .as_ref()
            .and_then(|cs| cs.nodes.with(|m| m.get(node.id as i64)));
        let compare_node = compare_spec_node.as_ref().is_some_and(|c| c.alloc.get());
        // L1575
        if &*ty == "Mastery" {
            // L1576
            if !node.alloc.get() && compare_node {
                // L1577
                m_node = compare_spec_node.clone().expect("compareNode");
                // L1578
                mastery_color = cc.DEXTERITY.clone();
            }
            // If allocated on Left, but not Right, match the colour of the other unallocated nodes.
            // L1581
            if self.compare_spec.is_some() && node.alloc.get() && !compare_node {
                // L1582
                mastery_color = cc.STRENGTH.clone();
            }
            // If both are allocated but are different text ...
            // L1585
            if node.alloc.get() && compare_node {
                // L1586
                m_node = compare_spec_node.clone().expect("compareNode");
                // L1587
                let first = |n: &Node| n.sd.get().with(|v| v.first().cloned());
                if first(node) != first(&m_node) {
                    // L1588
                    let sd = node.sd.get().snapshot();
                    for (i, line) in sd.iter().enumerate() {
                        // L1589
                        add_mod_info_to_tooltip(cx, tooltip, node, i + 1, format!("<- {line}"));
                        // L1590
                        mastery_color = format!("{}-> ", cc.DEXTERITY);
                    }
                }
            }
        }

        // L1596
        let is_runegraft = m_node
            .overrideType
            .get()
            .is_some_and(|t| &*t == "AlternateMastery");
        let m_sd = m_node.sd.get().snapshot();
        // L1597
        if !m_sd.is_empty() && m_node.allMasteryOptions.get() && !is_runegraft {
            // L1598
            tooltip.add_separator(cx, Some(14.0));
            // L1599
            tooltip.add_line(cx, 14.0, "^7Available Mastery node options are:");
            // L1600
            tooltip.add_line(cx, 6.0, "");
            // L1601
            let mut line_count = 0usize;
            let effects = m_node.masteryEffects.get().unwrap_or_else(|| {
                lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)")
            });
            let n_effects = effects.len();
            let spec = build.spec.get();
            // L1602
            for (n, effect) in effects.ipairs() {
                let effect_id = effect.get(n!("effect"));
                // L1603 (ORDER: module notes)
                let existing_mastery = spec
                    .masterySelections
                    .with(|m| m.pairs())
                    .iter()
                    .any(|(_, v)| effect_id.opt_num() == Some(*v));
                // L1604
                let effect_data = match effect_id.opt_num() {
                    Some(id) => spec.tree.get().masteryEffects.get(id),
                    None => Val::Nil,
                };
                // Must step through every mod, and filter out already allocated mods later
                // Otherwise red not supported text applies to the wrong indexes
                // L1607 (`tree.masteryEffects[id]` is the effect's node table, PassiveTree.lua:561,
                // so `effectData.sd` is that node's `sd`)
                let Some(effect_data) = effect_data.obj::<Node>() else {
                    lua_error!(
                        "attempt to index local 'effectData' (a {} value)",
                        effect_data.lua_type()
                    )
                };
                for line in effect_data.sd.get().snapshot() {
                    // L1608
                    line_count += 1;
                    // L1609
                    if !existing_mastery {
                        // L1610
                        add_mod_info_to_tooltip(cx, tooltip, &m_node, line_count, line.to_string());
                    }
                }
                // L1613
                if !existing_mastery && n < n_effects {
                    // L1614
                    tooltip.add_line(cx, 6.0, "");
                }
            }
            // L1617
            tooltip.add_separator(cx, Some(14.0));
        }

        // This stanza actives for both Mastery and non Mastery tooltips. Proof: add '"Blah "..' to addModInfoToTooltip
        // L1621
        if !m_sd.is_empty() && (!m_node.allMasteryOptions.get() || is_runegraft) {
            // L1622
            tooltip.add_line(cx, 16.0, "");
            // L1623
            for (i, line) in m_sd.iter().enumerate() {
                // L1624
                add_mod_info_to_tooltip(
                    cx,
                    tooltip,
                    &m_node,
                    i + 1,
                    format!("{mastery_color}{line}"),
                );
            }
            // add child tooltip for skills
            // L1627
            let list = m_node
                .finalModList
                .get()
                .unwrap_or_else(|| m_node.modList.get());
            for md in list.ipairs() {
                // L1628
                if md.name == n!("ExtraSkill") || md.name == n!("ExtraSupport") {
                    // L1629
                    let skill = match md.value.get(n!("skillId")) {
                        crate::model::gamedata::Value::Str(id) => {
                            build.data.skills.get(&pob_lua::Name::from(id)).cloned()
                        }
                        _ => None,
                    };
                    // L1630
                    if let Some(skill) = skill {
                        // L1631
                        let gem = build.data.gem_for_skill(skill.id).cloned();
                        // L1632
                        let mut options = GemTooltipOptions::default();
                        // L1633
                        let gem_data = if let Some(g) = gem {
                            TooltipGemData::Gem(g)
                        } else {
                            // L1635
                            options.skip_requirements = true;
                            // L1634
                            TooltipGemData::MadeUp(skill.clone())
                        };
                        // L1637-1642
                        let gem_inst = TooltipGemInstance {
                            gem_data,
                            level: md.value.get(n!("level")).num().unwrap_or(1.0),
                            quality: 0.0,
                            display_effect: None,
                        };
                        // L1643
                        add_gem_tooltip(
                            cx,
                            &mut self.skill_tooltip,
                            &build,
                            &gem_inst,
                            Some(options),
                        );
                    }
                }
            }
        }

        // Reminder text
        // L1651
        if let Some(lines) = str_list(node, "reminderText") {
            // L1652
            tooltip.add_separator(cx, Some(14.0));
            // L1653
            for line in lines {
                // L1654
                tooltip.add_line(cx, 14.0, &format!("^xA0A080{line}"));
            }
        }

        // Flavour text
        // L1659
        if let Some(lines) = str_list(node, "flavourText").filter(|_| cx.main.show_flavour_text) {
            // L1660
            tooltip.add_separator(cx, Some(14.0));
            // L1661
            for line in lines {
                // L1662
                tooltip.add_line_ex(
                    cx,
                    font_size_big,
                    &format!("{}{line}", cc.UNIQUE),
                    Some(Font::FontinItalic),
                    None,
                    None,
                );
            }
        }

        // L1666
        if return_early {
            // L1667
            return;
        }

        // Tattoo Editing
        // L1671-1676
        let dn = node.dn.get();
        let sd = node.sd.get().snapshot();
        let sd1_matches = || {
            let s = sd.first().map(|s| s.to_string()).unwrap_or_default();
            pob_lua::lmatch(&s, "+30 to Dexterity", None).is_some()
                || pob_lua::lmatch(&s, "+30 to Strength", None).is_some()
                || pob_lua::lmatch(&s, "+30 to Intelligence", None).is_some()
        };
        if (node.isTattoo.get()
            || (&*ty == "Normal"
                && (&*dn == "Strength" || &*dn == "Dexterity" || &*dn == "Intelligence"))
            || (&*ty == "Notable" && !sd.is_empty() && sd1_matches())
            || &*ty == "Keystone"
            || &*ty == "Mastery")
            && !node.extra.truthy(n!("expansionSkill"))
        {
            // L1677
            tooltip.add_separator(cx, Some(14.0));
            // L1678
            let node_edit_type = if &*ty == "Mastery" {
                "runegraft"
            } else {
                "tattoo"
            };
            // L1679
            tooltip.add_line(
                cx,
                14.0,
                &format!(
                    "{}Tip: Right click to edit the {node_edit_type} for this node",
                    cc.TIP
                ),
            );
        }

        // Mod differences
        // L1683
        if self.show_stat_differences {
            // L1684
            let misc = cx.build_mut().calc.misc_calculator(&build);
            // L1685
            tooltip.add_separator(cx, Some(14.0));
            if let Poll::Ready((calc_func, calc_base)) = misc {
                self.add_stat_differences(cx, tooltip, node, &calc_func, &calc_base);
            }
            // L1730
            tooltip.add_line(
                cx,
                14.0,
                &format!(
                    "{}Tip: Press Ctrl+D to disable the display of stat differences.",
                    cc.TIP
                ),
            );
        } else {
            // L1732
            tooltip.add_separator(cx, Some(14.0));
            // L1733
            tooltip.add_line(
                cx,
                14.0,
                &format!(
                    "{}Tip: Press Ctrl+D to enable the display of stat differences.",
                    cc.TIP
                ),
            );
        }

        // Pathing distance
        // L1737
        tooltip.add_separator(cx, Some(14.0));
        // L1738
        let path = node.path.get().map(|p| p.snapshot());
        if let Some(path) = path.filter(|p| !p.is_empty()) {
            // L1739
            let trace = self.trace_path.as_ref().map(|t| t.snapshot());
            let in_trace = self.trace_mode
                && trace
                    .as_ref()
                    .is_some_and(|t| t.iter().any(|x| Rc::ptr_eq(x, node)));
            if in_trace {
                // L1740
                let n = trace.as_ref().map_or(0, Vec::len);
                tooltip.add_line(cx, 14.0, &format!("^7{n} nodes in trace path"));
                // L1741
                tooltip.add_line(cx, 14.0, &cc.TIP);
            } else {
                // L1743
                let path_dist = match node.pathDist.get() {
                    Some(d) => pob_lua::tostring(d),
                    None => lua_error!("attempt to concatenate field 'pathDist' (a nil value)"),
                };
                let leap = if intuitive_leap_len(node) > 0 {
                    " ^8(Can be allocated without pathing to it)"
                } else {
                    ""
                };
                tooltip.add_line(cx, 14.0, &format!("^7{path_dist} points to node{leap}"));
                // L1744
                tooltip.add_line(cx, 14.0, &cc.TIP);
                // L1745
                if path.len() > 1 {
                    // Handy hint!
                    // L1747
                    tooltip.add_line(
                        cx,
                        14.0,
                        "Tip: To reach this node by a different path, hold Shift, then trace the path and click this node",
                    );
                }
            }
        }
        // L1751
        let level = build.characterLevel.get();
        let mut gold_cost = if level.fract() == 0.0 && level >= 1.0 {
            build.data.goldRespecPrices.get(level as usize - 1).copied()
        } else {
            None
        };
        // L1752
        if node.ascendancyName.get().is_some() {
            // L1753
            gold_cost = Some(arith(gold_cost) * 5.0);
        }
        let depends = node.depends.get().with(Vec::len);
        // L1755
        if depends > 1 {
            // L1756
            tooltip.add_separator(cx, Some(14.0));
            // L1757
            tooltip.add_line(
                cx,
                14.0,
                &format!("^7{depends} points gained from unallocating these nodes"),
            );
            // L1758
            let gold = pob_ui_core::luastr::format_num_sep_main(
                &cx.main,
                &pob_lua::tostring(depends as f64 * arith(gold_cost)),
            );
            tooltip.add_line(
                cx,
                14.0,
                &format!("^xFFD700{gold} Gold ^7required to unallocate these nodes"),
            );
            // L1759
            tooltip.add_line(cx, 14.0, &cc.TIP);
        } else if node.alloc.get() {
            // L1761
            let gold = pob_ui_core::luastr::format_num_sep_main(
                &cx.main,
                &pob_lua::tostring(depends as f64 * arith(gold_cost)),
            );
            tooltip.add_line(
                cx,
                14.0,
                &format!("^xFFD700{gold} Gold ^7required to unallocate this node"),
            );
            // L1762
            tooltip.add_line(cx, 14.0, &cc.TIP);
        }
        // L1764
        if &*ty == "Socket" {
            // L1765
            tooltip.add_line(
                cx,
                14.0,
                &format!("{}Tip: Hold Shift or Ctrl to hide this tooltip.", cc.TIP),
            );
        } else {
            // L1767
            tooltip.add_line(
                cx,
                14.0,
                &format!("{}Tip: Hold Ctrl to hide this tooltip.", cc.TIP),
            );
            // L1768
            tooltip.add_line(
                cx,
                14.0,
                &format!("{}Tip: Press Ctrl+C to copy this node's text.", cc.TIP),
            );
        }
    }

    /// L1686-1729: the body of the "Mod differences" block after `GetMiscCalculator` and the
    /// separator (split out for the `Pending` placeholder, module notes).
    fn add_stat_differences(
        &self,
        cx: &mut Cx,
        tooltip: &mut Tooltip,
        node: &NodeRef,
        calc_func: &Rc<dyn crate::calc::facade::MiscCalc>,
        calc_base: &Output,
    ) {
        let alloc = node.alloc.get();
        let ty = node.r#type.get();
        // L1686
        let path: Vec<NodeRef> = if alloc {
            node.depends.get().snapshot()
        } else if let Some(tp) = &self.trace_path {
            tp.snapshot()
        } else if let Some(p) = node.path.get() {
            p.snapshot()
        } else {
            Vec::new()
        };
        // L1687
        let path_length = path.len();
        // L1688-1691 (ORDER: module notes)
        let path_nodes = node_set(&path);
        // L1692
        let node_output: Option<Output>;
        let mut path_output: Option<Output> = None;
        // L1693
        let env = cx
            .build()
            .model
            .calcsTab
            .mainEnv
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'mainEnv' (a nil value)"));
        let is_granted = node.id.fract() == 0.0
            && env
                .grantedPassives
                .get()
                .with(|m| m.get(node.id as i64))
                .unwrap_or(false);
        // L1694
        let mut realloc = false;
        // L1695
        if alloc && &*ty == "Mastery" && !cx.main.popups.is_empty() {
            // L1696
            realloc = true;
            // L1697
            node_output = call(
                calc_func,
                Some(add_nodes(node_set(std::slice::from_ref(node)))),
            );
        } else if alloc {
            // Calculate the differences caused by deallocating this node and its dependent nodes
            // L1700
            node_output = call(
                calc_func,
                Some(remove_nodes(node_set(std::slice::from_ref(node)))),
            );
            // L1701
            if path_length > 1 {
                // L1702
                path_output = call(calc_func, Some(remove_nodes(path_nodes)));
            }
        } else if is_granted {
            // Calculate the differences caused by deallocating this node
            // L1706
            node_output = call(calc_func, Some(remove_node_id(node.id)));
        } else {
            // Calculated the differences caused by allocating this node and all nodes along the path to it
            // L1709
            if &*ty == "Mastery" && node.allMasteryOptions.get() {
                // L1710
                path_nodes.with_mut(|m| {
                    if m.get(node.id as i64).is_some_and(|x| Rc::ptr_eq(&x, node)) {
                        m.set(node.id as i64, None);
                    }
                });
                // L1711
                node_output = call(calc_func, None);
            } else {
                // L1713
                node_output = call(
                    calc_func,
                    Some(add_nodes(node_set(std::slice::from_ref(node)))),
                );
            }
            // L1715
            if path_length > 1 {
                // L1716
                path_output = call(calc_func, Some(add_nodes(path_nodes)));
            }
        }
        let output_or_raise = |o: Option<Output>| {
            o.unwrap_or_else(|| lua_error!("attempt to index local 'compareOutput' (a nil value)"))
        };
        // L1719
        let header = if realloc {
            "^7Reallocating this node will give you:"
        } else if alloc {
            "^7Unallocating this node will give you:"
        } else if is_granted {
            "^7This node is granted by an item. Removing it will give you:"
        } else {
            "^7Allocating this node will give you:"
        };
        let mut count = add_stat_compares_to_tooltip(
            cx,
            tooltip,
            calc_base,
            &output_or_raise(node_output),
            header,
            None,
        );
        // L1720
        if path_length > 1 && !is_granted && (intuitive_leap_len(node) == 0 || alloc) {
            // L1721
            let header = if alloc {
                "^7Unallocating this node and all nodes depending on it will give you:"
            } else {
                "^7Allocating this node and all nodes leading to it will give you:"
            };
            count += add_stat_compares_to_tooltip(
                cx,
                tooltip,
                calc_base,
                &output_or_raise(path_output),
                header,
                Some(path_length as f64),
            );
        }
        // L1723
        if count == 0.0 {
            // L1724
            if is_granted {
                // L1725
                tooltip.add_line(
                    cx,
                    14.0,
                    "^7This node is granted by an item. Removing it will cause no changes",
                );
            } else {
                // L1727 (`node.intuitiveLeapLikesAffecting == 0`: module notes)
                tooltip.add_line(
                    cx,
                    14.0,
                    &lfmt!(
                        "^7No changes from %s this node%s.",
                        if alloc { "unallocating" } else { "allocating" },
                        ""
                    ),
                );
            }
        }
    }

    /// lua: PassiveTreeView.lua:1772-1830 `PassiveTreeView:AddCompareNodeTooltip(tooltip, node,
    /// build)` (tooltip for compare-only nodes: nodes only in the compared build, e.g. cluster
    /// jewel subgraph nodes)
    /// @pob Classes/PassiveTreeView.lua:1772-1830 h=2302417b
    pub fn add_compare_node_tooltip(&mut self, cx: &mut Cx, tooltip: &mut Tooltip, node: &NodeRef) {
        let fs = Some(Font::Fontin);
        let cc = cx.main.cc.clone();
        // L1774
        let font_size_big = if cx.main.show_flavour_text {
            18.0
        } else {
            16.0
        };
        // L1775
        tooltip.center = true;
        // L1776
        tooltip.max_width = Some(800.0);

        let ty = node.r#type.get();
        // Special case for sockets with jewels
        // L1779
        if &*ty == "Socket" && node.alloc.get() {
            // L1780
            let c_jewel = self.get_compare_jewel(node.id);
            // L1781
            let Some(compare_spec) = self.compare_spec.as_ref() else {
                lua_error!("attempt to index field 'compareSpec' (a nil value)")
            };
            let c_build = compare_spec.build.upgrade();
            // L1782
            match c_jewel.zip(c_build) {
                Some((c_jewel, c_build)) => {
                    // L1783 (`cItemsTab`: the compare spec's own build's ItemsTab, FUP6)
                    let h = items_hooks().add_item_tooltip;
                    h(cx, &c_build, tooltip, &c_jewel, None, false);
                }
                None => {
                    // L1785
                    self.add_compare_node_name(cx, tooltip, node);
                }
            }
            // L1787
            tooltip.add_separator(cx, Some(14.0));
            // L1788
            tooltip.add_line(
                cx,
                14.0,
                &format!("{}This node is only in the compared build", cc.DEXTERITY),
            );
            // L1789
            return;
        }

        // Node name
        // L1793
        self.add_compare_node_name(cx, tooltip, node);
        // L1794
        tooltip.center = false;

        // Node mods
        // L1797
        let sd = node.sd.get().snapshot();
        if !sd.is_empty() {
            // L1798
            tooltip.add_line(cx, 16.0, "");
            let mods = node.mods.get().snapshot();
            // L1799
            for (i, line) in sd.iter().enumerate() {
                // L1800
                if let Some(m) = mods.get(i) {
                    // L1801
                    if &**line != " " && (m.truthy(n!("extra")) || !m.truthy(n!("list"))) {
                        // L1802
                        tooltip.add_line_ex(
                            cx,
                            font_size_big,
                            &format!("{}{line}", cc.UNSUPPORTED),
                            fs,
                            None,
                            None,
                        );
                    } else {
                        // L1804
                        tooltip.add_line_ex(
                            cx,
                            font_size_big,
                            &format!("{}{line}", cc.MAGIC),
                            fs,
                            None,
                            None,
                        );
                    }
                } else {
                    // L1807
                    tooltip.add_line_ex(
                        cx,
                        font_size_big,
                        &format!("{}{line}", cc.MAGIC),
                        fs,
                        None,
                        None,
                    );
                }
            }
        }

        // Reminder text
        // L1813
        if let Some(lines) = str_list(node, "reminderText") {
            // L1814
            tooltip.add_separator(cx, Some(14.0));
            // L1815
            for line in lines {
                // L1816
                tooltip.add_line(cx, 14.0, &format!("^xA0A080{line}"));
            }
        }

        // Flavour text
        // L1821
        if let Some(lines) = str_list(node, "flavourText").filter(|_| cx.main.show_flavour_text) {
            // L1822
            tooltip.add_separator(cx, Some(14.0));
            // L1823
            for line in lines {
                // L1824
                tooltip.add_line_ex(
                    cx,
                    font_size_big,
                    &format!("{}{line}", cc.UNIQUE),
                    Some(Font::FontinItalic),
                    None,
                    None,
                );
            }
        }

        // L1828
        tooltip.add_separator(cx, Some(14.0));
        // L1829
        tooltip.add_line(
            cx,
            14.0,
            &format!("{}This node is only in the compared build", cc.DEXTERITY),
        );
    }
}

/// `x * ...` on `goldCost` (nil raises).
fn arith(v: Option<f64>) -> f64 {
    v.unwrap_or_else(|| {
        lua_error!("attempt to perform arithmetic on local 'goldCost' (a nil value)")
    })
}

/// A value in a `..` concatenation (strings and numbers; others raise).
fn concat_val(v: &Val) -> String {
    match v {
        Val::Str(s) => s.to_string(),
        Val::Num(x) => pob_lua::tostring(*x),
        other => lua_error!("attempt to concatenate a {} value", other.lua_type()),
    }
}
