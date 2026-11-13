//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/PassiveTreeView.lua:1-1411 (compare helpers).
//! Owner: U23: `isAbyssConquered` (L23-26), `GetCompareJewel` (L118-128),
//! `GetJewelSocketOverlay` (L131-159), `compareJewelsEqual` (L161-166) and
//! `GetCompareNodeColor` (L175-194).
//!
//! @pob-file Classes/PassiveTreeView.lua @16de4b82d
//!
//! A draw colour is what Lua passes to `SetDrawColor`: an escape string (`nodeDefaultColor`,
//! `"^xFFFFFF"`, `colorCodes.SPLITPERSONALITY`) or `r, g, b` ([`DrawColor`]).
//!
//! DIVERGENCE: none. ORDER: no `pairs` loops.

use pob_lua::n;
use pob_ui_core::prelude::*;

use super::PassiveTreeView;
use crate::app::CxApp;
use crate::model::{ItemRef, ItemsTab, Node, PassiveSpecRef};

/// The arguments of one `SetDrawColor` call.
#[derive(Clone, Debug, PartialEq)]
pub enum DrawColor {
    /// `SetDrawColor("^x..")`
    Esc(String),
    /// `SetDrawColor(r, g, b)`
    Rgb(f64, f64, f64),
}

impl DrawColor {
    /// `SetDrawColor(...)`
    #[track_caller]
    pub fn set(&self, cx: &mut Cx) {
        match self {
            DrawColor::Esc(s) => cx.sg.set_draw_color_esc(s),
            DrawColor::Rgb(r, g, b) => cx.sg.set_draw_color(*r, *g, *b),
        }
    }
}

/// lua: PassiveTreeView.lua:23-26 `isAbyssConquered(node)`
/// @pob Classes/PassiveTreeView.lua:23-26 h=0844141d
pub fn is_abyss_conquered(node: Option<&Node>) -> bool {
    // L24 local conqueror = node and node.conqueredBy and node.conqueredBy.conqueror
    let Some(conquered_by) = node.and_then(|n| n.conqueredBy.get()) else {
        return false;
    };
    let conqueror = conquered_by.get(n!("conqueror"));
    // L25 return conqueror and conqueror.type and conqueror.type:match("^abyss_")
    let ty = conqueror.get(n!("type"));
    match ty.str() {
        Some(t) => conqueror.truthy() && t.starts_with("abyss_"),
        None => false,
    }
}

/// `items[id]` of an items tab (`nil` for an unknown or non-integer id).
pub fn item_at(items_tab: &ItemsTab, id: f64) -> Option<ItemRef> {
    if id.fract() != 0.0 || id < 0.0 {
        return None;
    }
    items_tab.items.with(|m| m.get(&(id as u32)).cloned())
}

impl PassiveTreeView {
    /// Look up the jewel item socketed at a given node ID in a compare spec.
    /// Uses itemsTab.sockets (the slot controls) which stay in sync with the active item/tree set.
    /// lua: PassiveTreeView.lua:118-128 `PassiveTreeView:GetCompareJewel(nodeId)`
    /// @pob Classes/PassiveTreeView.lua:118-128 h=a5c38436
    pub fn get_compare_jewel(&self, node_id: f64) -> Option<ItemRef> {
        // L119
        let compare_spec = self.compare_spec.as_ref()?;
        // L120-121
        let c_build = compare_spec.build.upgrade()?;
        let c_items_tab = &c_build.itemsTab;
        // L122 (`itemsTab.sockets` always exists in the model)
        // L123
        let c_socket = if node_id.fract() == 0.0 {
            c_items_tab.sockets.with(|m| m.get(node_id as i64))
        } else {
            None
        };
        // L124
        if let Some(c_socket) = c_socket {
            let sel = c_socket.opt_num(n!("selItemId"));
            if let Some(sel) = sel.filter(|s| *s > 0.0) {
                // L125
                return item_at(c_items_tab, sel);
            }
        }
        // L127
        None
    }

    /// Returns the overlay asset name for a socketed jewel, or nil if no special overlay applies.
    /// lua: PassiveTreeView.lua:131-159 `PassiveTreeView:GetJewelSocketOverlay(jewel, isExpansion)`
    /// @pob Classes/PassiveTreeView.lua:131-159 h=68adf519
    pub fn get_jewel_socket_overlay(jewel: &ItemRef, is_expansion: bool) -> Option<&'static str> {
        let base_name = jewel.baseName.get();
        let base_name = base_name.as_deref();
        let sub_type = jewel
            .base
            .get()
            .and_then(|b| b.subType)
            .map(|s| s.to_string());
        let sub_type = sub_type.as_deref();
        let pick = |alt: &'static str, plain: &'static str| if is_expansion { alt } else { plain };
        // L132
        if base_name == Some("Crimson Jewel") {
            // L133
            Some(pick("JewelSocketActiveRedAlt", "JewelSocketActiveRed"))
        } else if base_name == Some("Viridian Jewel") {
            // L135
            Some(pick("JewelSocketActiveGreenAlt", "JewelSocketActiveGreen"))
        } else if base_name == Some("Cobalt Jewel") {
            // L137
            Some(pick("JewelSocketActiveBlueAlt", "JewelSocketActiveBlue"))
        } else if base_name == Some("Prismatic Jewel") {
            // L139
            Some(pick(
                "JewelSocketActivePrismaticAlt",
                "JewelSocketActivePrismatic",
            ))
        } else if sub_type == Some("Abyss") {
            // L141
            Some(pick("JewelSocketActiveAbyssAlt", "JewelSocketActiveAbyss"))
        } else if sub_type == Some("Charm") {
            // L143
            if base_name == Some("Ursine Charm") {
                // L144
                Some("CharmSocketActiveStr")
            } else if base_name == Some("Corvine Charm") {
                // L146
                Some("CharmSocketActiveInt")
            } else if base_name == Some("Lupine Charm") {
                // L148
                Some("CharmSocketActiveDex")
            } else {
                None
            }
        } else if base_name == Some("Timeless Jewel") {
            // L151
            Some(pick(
                "JewelSocketActiveLegionAlt",
                "JewelSocketActiveLegion",
            ))
        } else if base_name == Some("Large Cluster Jewel") {
            // L153
            Some("JewelSocketActiveAltPurple")
        } else if base_name == Some("Medium Cluster Jewel") {
            // L155
            Some("JewelSocketActiveAltBlue")
        } else if base_name == Some("Small Cluster Jewel") {
            // L157
            Some("JewelSocketActiveAltRed")
        } else {
            None
        }
    }

    /// Returns the draw color for a node when compare overlay is active.
    /// Handles diff coloring for allocated/unallocated, mastery changes, and jewel socket differences.
    /// lua: PassiveTreeView.lua:175-194 `PassiveTreeView:GetCompareNodeColor(node, compareNode, spec, build, nodeDefaultColor)`
    /// @pob Classes/PassiveTreeView.lua:175-194 h=5bc804cf
    pub fn get_compare_node_color(
        &self,
        cx: &Cx,
        node: &Node,
        compare_node: Option<&Node>,
        spec: &PassiveSpecRef,
        node_default_color: &DrawColor,
    ) -> DrawColor {
        // L176
        let Some(compare_node) = compare_node else {
            // L177
            return node_default_color.clone();
        };
        let ty = node.r#type.get();
        // L179
        if compare_node.alloc.get() && !node.alloc.get() {
            // L180
            return DrawColor::Rgb(0.0, 1.0, 0.0);
        } else if !compare_node.alloc.get() && node.alloc.get() {
            // L182
            return DrawColor::Rgb(1.0, 0.0, 0.0);
        } else if &*ty == "Mastery"
            && compare_node.alloc.get()
            && node.alloc.get()
            && !node.sd.get().ptr_eq(&compare_node.sd.get())
        {
            // L184
            return DrawColor::Rgb(0.0, 0.0, 1.0);
        } else if &*ty == "Socket" && compare_node.alloc.get() && node.alloc.get() {
            // L186
            let p_jewel_id = spec.jewels.with(|m| m.get(node.id as i64));
            // L187
            let p_jewel = p_jewel_id.and_then(|id| item_at(&cx.build().model.itemsTab, id));
            // L188
            let c_jewel = self.get_compare_jewel(node.id);
            // L189
            if !compare_jewels_equal(p_jewel.as_ref(), c_jewel.as_ref()) {
                // L190
                return DrawColor::Rgb(0.0, 0.0, 1.0);
            }
        }
        // L193
        node_default_color.clone()
    }
}

/// lua: PassiveTreeView.lua:161-166 `compareJewelsEqual(a, b)`
/// @pob Classes/PassiveTreeView.lua:161-166 h=016569e1
pub fn compare_jewels_equal(a: Option<&ItemRef>, b: Option<&ItemRef>) -> bool {
    match (a, b) {
        // L162-163 if not a or not b then return a == b end
        // L165
        (Some(a), Some(b)) => a.BuildRaw() == b.BuildRaw(),
        (None, None) => true,
        _ => false,
    }
}
