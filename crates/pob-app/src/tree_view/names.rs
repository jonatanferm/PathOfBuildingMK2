//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/PassiveTreeView.lua:1412-1856 (AddNodeName, AddCompareNodeName).
//! Owner: U24.
//!
//! @pob-file Classes/PassiveTreeView.lua @16de4b82d
//!
//! `build` (the Lua argument) is the viewer's own build in every PoB caller (TreeTab, ItemsTab's
//! socket viewer, NotableDBControl, PassiveMasteryControl, ComparePowerReportListControl's
//! `primaryBuild`), which is the open build: `cx.build().model`.
//!
//! Lua semantics:
//! * `node.id` is concatenated with `..` (L1435): `tostring` of the number; a legion / tattoo
//!   node's string id (CCR-0024 §1, `node_id_val`) is concatenated as is.
//! * `node.id > 65535` (L1437) and the `band`/`b_rshift` decomposition (L1439-1442) take the
//!   number id; a string id raises at the comparison, as in Lua.
//! * `attribTotals[...] >= 40` (L1454-1462): a socket whose `nodesInRadius[2]` is empty leaves
//!   the totals nil and the comparison raises ("attempt to compare nil with number"), as in Lua.
//!
//! DIVERGENCE: none. ORDER: L1447 `pairs(node.nodesInRadius[2])` only sums integral attribute
//! values into three totals (order-free, (c)).

use pob_lua::bit::{band, rshift, tobit};
use pob_lua::{lfmt, n};
use pob_sg::Font;
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::Tooltip;

use super::PassiveTreeView;
use crate::app::CxApp;
use crate::model::{gamedata::ModType, Node, NodeRef, Val};

/// `tooltipMap[node.type] or "UNKNOWN"` (L1415-1422, L1834-1841).
fn tooltip_map(ty: &str) -> &'static str {
    match ty {
        "Normal" => "PASSIVE",
        "Notable" => "NOTABLE",
        "Socket" => "JEWEL",
        "Keystone" => "KEYSTONE",
        "Ascendancy" => "ASCENDANCY",
        "Mastery" => "MASTERY",
        _ => "UNKNOWN",
    }
}

/// `node.recipe` (a list of oil names, or nil).
pub(super) fn node_recipe(node: &Node) -> Option<Vec<String>> {
    node.extra.table(n!("recipe")).map(|t| {
        t.ipairs()
            .into_iter()
            .map(|(_, v)| match v.str() {
                Some(s) => s.to_string(),
                None => format!("{v:?}"),
            })
            .collect()
    })
}

/// `node.id` in a concatenation (module notes).
pub(super) fn node_id_concat(node: &Node) -> String {
    // CCR-0115: model.rs (U15) does not re-export `node_id_val`
    match pob_calc::classes::passive_tree::node_id_val(node) {
        Val::Num(x) => pob_lua::tostring(x),
        Val::Str(s) => s.to_string(),
        other => lua_error!(
            "attempt to concatenate field 'id' (a {} value)",
            other.lua_type()
        ),
    }
}

/// L1423-1429 / L1842-1848: the header from the node type.
fn set_header(tooltip: &mut Tooltip, node: &Node) {
    let ty = node.r#type.get();
    if &*ty == "Mastery" {
        tooltip.tooltip_header = Some(
            if node.alloc.get() {
                "MASTERYALLOC"
            } else {
                "MASTERY"
            }
            .to_owned(),
        );
    } else if (&*ty == "Notable" || &*ty == "Normal") && node.ascendancyName.get().is_some() {
        tooltip.tooltip_header = Some("ASCENDANCY".to_owned());
    } else {
        tooltip.tooltip_header = Some(tooltip_map(&ty).to_owned());
    }
}

impl PassiveTreeView {
    /// lua: PassiveTreeView.lua:1412-1470 `PassiveTreeView:AddNodeName(tooltip, node, build)`
    /// @pob Classes/PassiveTreeView.lua:1412-1470 h=5812283a
    pub fn add_node_name(&self, cx: &mut Cx, tooltip: &mut Tooltip, node: &NodeRef) {
        let fs = Some(Font::Fontin);
        // L1413
        let font_size_big = if cx.main.show_flavour_text {
            18.0
        } else {
            16.0
        };
        // L1414
        tooltip.set_recipe(node_recipe(node));
        // L1415-1429
        set_header(tooltip, node);
        // L1430
        let mut node_name = node.dn.get().to_string();
        // L1431
        if cx.main.show_flavour_text {
            // L1432
            node_name = format!("^xF8E6CA{}", node.dn.get());
        }
        let dev_mode_alt = cx.app().launch.dev_mode_alt;
        // L1434
        tooltip.center = true;
        // L1435
        let suffix = if dev_mode_alt {
            format!(" [{}]", node_id_concat(node))
        } else {
            String::new()
        };
        tooltip.add_line_ex(cx, 24.0, &format!("{node_name}{suffix}"), fs, None, None);
        // L1436
        tooltip.center = false;
        // L1437
        if dev_mode_alt && node_id_number(node) > 65535.0 {
            // Decompose cluster node Id
            let id = tobit(node.id) as u32;
            // L1439
            let index = band(id, 0xF);
            // L1440
            let size = band(rshift(id, 4), 0x3);
            // L1441
            let large = band(rshift(id, 6), 0x7);
            // L1442
            let medium = band(rshift(id, 9), 0x3);
            // L1443
            tooltip.add_line_ex(
                cx,
                font_size_big,
                &lfmt!(
                    "^7Cluster node index: %d, size: %d, large index: %d, medium index: %d",
                    index,
                    size,
                    large,
                    medium
                ),
                fs,
                None,
                None,
            );
        }
        let ty = node.r#type.get();
        // L1445
        if &*ty == "Socket" && node.nodesInRadius.get().is_some() {
            // L1446
            let (mut str_total, mut dex_total, mut int_total): (
                Option<f64>,
                Option<f64>,
                Option<f64>,
            ) = (None, None, None);
            // L1447 (ORDER: module notes; CCR-0115 `nodesInRadiusAt`)
            let in_radius = pob_calc::classes::passive_tree::nodesInRadiusAt(node, 2.0)
                .unwrap_or_else(|| {
                    lua_error!("bad argument #1 to 'pairs' (table expected, got nil)")
                });
            let spec = cx.build().model.spec.get();
            for (node_id, _) in in_radius.pairs() {
                // L1448
                let spec_node = spec.nodes.with(|m| m.get(node_id));
                let Some(spec_node) = spec_node else {
                    lua_error!("attempt to index local 'specNode' (a nil value)")
                };
                let Some(fml) = spec_node.finalModList.get() else {
                    lua_error!("attempt to index field 'finalModList' (a nil value)")
                };
                // L1449-1451
                for (attrib, total) in [
                    ("Str", &mut str_total),
                    ("Dex", &mut dex_total),
                    ("Int", &mut int_total),
                ] {
                    let v = fml.Sum(ModType::BASE, None, &[pob_lua::Name::from(attrib)]);
                    *total = Some(total.unwrap_or(0.0) + v);
                }
            }
            // L1453
            tooltip.add_separator(cx, Some(14.0));
            let ge40 = |t: Option<f64>| match t {
                Some(t) => t >= 40.0,
                None => lua_error!("attempt to compare nil with number"),
            };
            let cc = cx.main.cc.clone();
            // L1454
            if ge40(str_total) {
                // L1455
                tooltip.add_line_ex(
                    cx,
                    font_size_big,
                    &format!("^7Can support {}Strength ^7threshold jewels", cc.STRENGTH),
                    fs,
                    None,
                    None,
                );
            }
            // L1457
            if ge40(dex_total) {
                // L1458
                tooltip.add_line_ex(
                    cx,
                    font_size_big,
                    &format!("^7Can support {}Dexterity ^7threshold jewels", cc.DEXTERITY),
                    fs,
                    None,
                    None,
                );
            }
            // L1460
            if ge40(int_total) {
                // L1461
                tooltip.add_line_ex(
                    cx,
                    font_size_big,
                    &format!(
                        "^7Can support {}Intelligence ^7threshold jewels",
                        cc.INTELLIGENCE
                    ),
                    fs,
                    None,
                    None,
                );
            }
        }
        // L1464
        if &*ty == "Socket" && node.alloc.get() {
            // L1465
            if let Some(d) = node.distanceToClassStart.get().filter(|d| *d > 0.0) {
                // L1466
                tooltip.add_separator(cx, Some(14.0));
                // L1467
                tooltip.add_line_ex(
                    cx,
                    16.0,
                    &lfmt!("^7Distance to start: %d", d),
                    fs,
                    None,
                    None,
                );
            }
        }
    }

    /// lua: PassiveTreeView.lua:1832-1856 `PassiveTreeView:AddCompareNodeName(tooltip, node)`
    /// @pob Classes/PassiveTreeView.lua:1832-1856 h=9dc455b1
    pub fn add_compare_node_name(&self, cx: &mut Cx, tooltip: &mut Tooltip, node: &NodeRef) {
        // L1833
        tooltip.set_recipe(node_recipe(node));
        // L1834-1848
        set_header(tooltip, node);
        // L1849
        let mut node_name = node.dn.get().to_string();
        // L1850
        if cx.main.show_flavour_text {
            // L1851
            node_name = format!("^xF8E6CA{}", node.dn.get());
        }
        // L1853
        tooltip.center = true;
        // L1854
        let suffix = if cx.app().launch.dev_mode_alt {
            format!(" [{}]", node_id_concat(node))
        } else {
            String::new()
        };
        tooltip.add_line_ex(
            cx,
            24.0,
            &format!("{node_name}{suffix}"),
            Some(Font::Fontin),
            None,
            None,
        );
        // L1855
        tooltip.center = false;
    }
}

/// `node.id` as a number operand (L1437 `node.id > 65535`; a string id raises).
fn node_id_number(node: &Node) -> f64 {
    match pob_calc::classes::passive_tree::node_id_val(node) {
        Val::Num(x) => x,
        other => lua_error!("attempt to compare number with {}", other.lua_type()),
    }
}
