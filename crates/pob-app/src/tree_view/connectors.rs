//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/PassiveTree.lua:909-1040 (BuildConnector, BuildArc).
//! Owner: U23. Also the connector loop of the constructor (PassiveTree.lua:626-645) and the
//! connector polygons of the spec's cluster subgraphs (PassiveSpec.lua:2021-2025, whose
//! `linkNodes` calls the calc's PS-C records as node pairs, CCR-0072 §3).
//!
//! @pob-file Classes/PassiveTree.lua @16de4b82d
//! @pob-file Classes/PassiveSpec.lua @16de4b82d
//!
//! A connector is `{ ascendancyName, nodeId1, nodeId2, type, vert = { Normal, Intermediate,
//! Active }, c = { } }`: `c[1..8]` are written by every draw (screen space), `c[9..16]` are the
//! texture coordinates set here. A line connector's `vert[state]` is the connector itself
//! (`connector[1..8]`), the same eight numbers for each state.
//!
//! ORDER: `pairs(self.nodes)` (:628) builds `self.connectors` in the model's `tree.nodes` order;
//! PoB's order of a 3.10+ tree is random per process (CCR-0024 §5). The connectors are drawn in
//! that order on one sub-layer (PassiveTreeView.lua:745-747), which the U23 test compares as a
//! multiset (ui doc §8 rule 6 (b)). `pairs(node.out)` walks a sequence (ascending).

use std::cell::RefCell;
use std::rc::Rc;

use pob_lua::math::m_max;
use pob_lua::{n, Name};
use pob_ui_core::prelude::*;

use super::assets::TreeAssets;
use crate::model::{Node, NodeRef, PassiveTreeRef, Table, Val};

/// One connector (module notes).
#[derive(Clone, Default)]
pub struct Connector {
    /// `connector.ascendancyName`
    pub ascendancy_name: Option<Name>,
    /// `connector.nodeId1` / `nodeId2`
    pub node_id1: f64,
    pub node_id2: f64,
    /// `connector.type`: `"LineConnector"` or `"Orbit"..node1.o`
    pub r#type: String,
    /// `connector.vert[state]` for Normal, Intermediate, Active
    pub vert: [[f64; 8]; 3],
    /// `connector.c[1..16]`
    pub c: RefCell<[f64; 16]>,
}

/// The connector states (`vert` keys).
pub const STATES: [&str; 3] = ["Normal", "Intermediate", "Active"];

impl Connector {
    /// `connector.vert[state]`
    pub fn vert(&self, state: &str) -> &[f64; 8] {
        &self.vert[STATES.iter().position(|s| *s == state).unwrap_or(0)]
    }
}

/// `m_pi`
const M_PI: f64 = std::f64::consts::PI;

#[inline]
fn m_sin(x: f64) -> f64 {
    x.sin()
}
#[inline]
fn m_cos(x: f64) -> f64 {
    x.cos()
}
#[inline]
fn m_tan(x: f64) -> f64 {
    x.tan()
}
#[inline]
fn m_sqrt(x: f64) -> f64 {
    x.sqrt()
}

/// `node.group.x` / `.y` (a nil group or coordinate raises, as Lua's arithmetic does).
fn group_xy(node: &Node) -> (f64, f64) {
    let g = node
        .group
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'group' (a nil value)"));
    (g.num(n!("x")), g.num(n!("y")))
}

/// A number field that Lua uses in arithmetic (`NaN` is the model's nil, CCR-0024 §2).
#[track_caller]
fn arith(x: f64, what: &str) -> f64 {
    if x.is_nan() {
        lua_error!("attempt to perform arithmetic on field '{what}' (a nil value)");
    }
    x
}

/// `tree.assets[name]`'s `width`/`height` (a missing asset raises at the index).
fn art(assets: &TreeAssets, name: &str) -> (f64, f64) {
    let a = assets
        .asset(name)
        .unwrap_or_else(|| lua_error!("attempt to index local 'art' (a nil value): {name}"));
    (a.width.get(), a.height.get())
}

/// lua: PassiveTree.lua:937-990 `PassiveTreeClass:BuildConnector(node1, node2)`
/// @pob Classes/PassiveTree.lua:937-990 h=82b6ce4a
pub fn build_connector(assets: &TreeAssets, node1: &NodeRef, node2: &NodeRef) -> Vec<Rc<Connector>> {
    let (mut node1, mut node2) = (node1.clone(), node2.clone());
    // L938-944
    let mut connector = Connector {
        ascendancy_name: node1.ascendancyName.get(),
        node_id1: node1.id,
        node_id2: node2.id,
        ..Connector::default()
    };
    // L945 if node1.g == node2.g and node1.o == node2.o then
    if node1.extra.get(n!("g")).raw_eq(&node2.extra.get(n!("g"))) && node1.o.get() == node2.o.get() {
        // Nodes are in the same orbit of the same group
        // Calculate the starting angle (node1.angle) and arc angle
        // L948
        if arith(node1.angle.get(), "angle") > arith(node2.angle.get(), "angle") {
            // L949
            std::mem::swap(&mut node1, &mut node2);
        }
        // L951
        let mut arc_angle = node2.angle.get() - node1.angle.get();
        // L952
        if arc_angle >= M_PI {
            // L953
            std::mem::swap(&mut node1, &mut node2);
            // L954
            arc_angle = M_PI * 2.0 - arc_angle;
        }
        // L956
        if arc_angle <= M_PI {
            // Angle is less than 180 degrees, draw an arc
            // If our arc is greater than 90 degrees, we will need 2 arcs because our orbit assets are at most 90 degree arcs see below
            // The calling class already handles adding a second connector object in the return table if provided and omits it if nil
            // Establish a nil secondConnector to populate in the case that we need a second arc (>90 degree orbit)
            // L961
            let mut second_connector: Option<Connector> = None;
            // L962
            if arc_angle > (M_PI / 2.0) {
                // Angle is greater than 90 degrees.
                // The default behavior for a given arcAngle is to place the arc at the center point between two nodes and clip the excess
                // If we need a second arc of any size, we should shift the arcAngle to 25% of the distance between the nodes instead of 50%
                // L966
                arc_angle /= 2.0;
                // clone the original connector table to ensure same functionality for both of the necessary connectors
                // L968
                let mut second = connector.clone();
                // And then ask the BuildArc function to create a connector that is a mirror of the provided arcAngle
                // Provide the second connector as a parameter to store the mirrored arc
                // L971
                build_arc(assets, arc_angle, &node1, &mut second, true);
                second_connector = Some(second);
            }
            // generate the primary arc -- this arcAngle may have been modified if we have determined that a second arc is necessary for this orbit
            // L974
            build_arc(assets, arc_angle, &node1, &mut connector, false);
            // L975
            let mut out = vec![Rc::new(connector)];
            if let Some(s) = second_connector {
                out.push(Rc::new(s));
            }
            return out;
        }
    }

    // Generate a straight line
    // L980
    connector.r#type = "LineConnector".to_owned();
    // L981
    let (art_w, art_h) = art(assets, "LineConnectorNormal");
    // L982
    let (v_x, v_y) = (
        arith(node2.x.get(), "x") - arith(node1.x.get(), "x"),
        arith(node2.y.get(), "y") - arith(node1.y.get(), "y"),
    );
    // L983
    let dist = m_sqrt(v_x * v_x + v_y * v_y);
    // L984
    let scale = art_h * 1.33 / dist;
    // L985
    let (n_x, n_y) = (v_x * scale, v_y * scale);
    // L986
    let end_s = dist / (art_w * 1.33);
    let (x1, y1, x2, y2) = (node1.x.get(), node1.y.get(), node2.x.get(), node2.y.get());
    // L987-990
    let v = [
        x1 - n_y,
        y1 + n_x,
        x1 + n_y,
        y1 - n_x,
        x2 + n_y,
        y2 - n_x,
        x2 - n_y,
        y2 + n_x,
    ];
    {
        let mut c = connector.c.borrow_mut();
        // L991-994
        c[8] = 0.0;
        c[9] = 1.0;
        c[10] = 0.0;
        c[11] = 0.0;
        c[12] = end_s;
        c[13] = 0.0;
        c[14] = end_s;
        c[15] = 1.0;
    }
    // L995 connector.vert = { Normal = connector, Intermediate = connector, Active = connector }
    connector.vert = [v, v, v];
    // L996
    vec![Rc::new(connector)]
}

/// lua: PassiveTree.lua:999-1040 `PassiveTreeClass:BuildArc(arcAngle, node1, connector, isMirroredArc)`
/// @pob Classes/PassiveTree.lua:999-1040 h=5b3e4cce
pub fn build_arc(
    assets: &TreeAssets,
    arc_angle: f64,
    node1: &Node,
    connector: &mut Connector,
    is_mirrored_arc: bool,
) {
    // L1000 connector.type = "Orbit" .. node1.o
    connector.r#type = format!("Orbit{}", pob_lua::tostring(arith(node1.o.get(), "o")));
    // This is an arc texture mapped onto a kite-shaped quad
    // Calculate how much the arc needs to be clipped by
    // Both ends of the arc will be clipped by this amount, so 90 degree arc angle = no clipping and 30 degree arc angle = 75 degrees of clipping
    // The clipping is accomplished by effectively moving the bottom left and top right corners of the arc texture towards the top left corner
    // The arc texture only shows 90 degrees of an arc, but some arcs must go for more than 90 degrees
    // Fortunately there's nowhere on the tree where we can't just show the middle 90 degrees and rely on the node artwork to cover the gaps :)
    // L1007
    let clip_angle = M_PI / 4.0 - arc_angle / 2.0;
    // L1008
    let p = 1.0 - m_max(m_tan(clip_angle), 0.0);
    // L1009
    let mut angle = node1.angle.get() - clip_angle;
    // L1010
    if is_mirrored_arc {
        // The center of the mirrored angle should be positioned at 75% of the way between nodes.
        // L1012
        angle += arc_angle;
    }
    // L1014 connector.vert = { }
    // L1015
    let (gx, gy) = group_xy(node1);
    for (si, state) in STATES.iter().enumerate() {
        // The different line states have differently-sized artwork, so the vertex coords must be calculated separately for each one
        // L1017
        let (art_w, _) = art(assets, &format!("{}{state}", connector.r#type));
        // L1018
        let size = art_w * 2.0 * 1.33;
        // L1019
        let (o_x, o_y) = (
            size * m_sqrt(2.0) * m_sin(angle + M_PI / 4.0),
            size * m_sqrt(2.0) * -m_cos(angle + M_PI / 4.0),
        );
        // L1020
        let (c_x, c_y) = (gx + o_x, gy + o_y);
        // L1021-1025
        let mut vert = [
            gx,
            gy,
            c_x + (size * m_sin(angle) - o_x) * p,
            c_y + (size * -m_cos(angle) - o_y) * p,
            c_x,
            c_y,
            c_x + (size * m_cos(angle) - o_x) * p,
            c_y + (size * m_sin(angle) - o_y) * p,
        ];
        // L1026
        if is_mirrored_arc {
            // Flip the quad's non-origin, non-center vertexes when drawing a mirrored arc so that the arc actually mirrored
            // This is required to prevent the connection of the 2 arcs appear to have a 'seam'
            // L1029-1031
            let (temp1, temp2) = (vert[2], vert[3]);
            vert[2] = vert[6];
            vert[3] = vert[7];
            vert[6] = temp1;
            vert[7] = temp2;
        }
        // L1033
        connector.vert[si] = vert;
    }
    let mut c = connector.c.borrow_mut();
    // L1035-1038
    c[8] = 1.0;
    c[9] = 1.0;
    c[10] = 0.0;
    c[11] = p;
    c[12] = 0.0;
    c[13] = 0.0;
    c[14] = p;
    c[15] = 0.0;
}

/// `type(otherId) == "string" and tonumber(otherId) or otherId`
fn link_id(v: &Val) -> Option<f64> {
    match v {
        Val::Num(x) => Some(*x),
        Val::Str(s) => pob_lua::tonumber(s),
        _ => None,
    }
}

/// lua: PassiveTree.lua:626-645 (pregenerate the polygons for the node connector lines; the
/// `linkedId`/`linked` lines are calc:PT's)
/// @pob Classes/PassiveTree.lua:626-627 h=acb7fdb8
pub fn build_tree_connectors(assets: &TreeAssets, tree: &PassiveTreeRef) -> Vec<Rc<Connector>> {
    // L627 self.connectors = { }
    let mut connectors = Vec::new();
    // PassiveTree.lua:520/544 `nodeMap[node.id] = node`
    let node_map: std::collections::HashMap<i64, NodeRef> = tree
        .nodes
        .pairs()
        .into_iter()
        .filter(|(_, n)| !n.id.is_nan())
        .map(|(_, n)| (n.id as i64, n))
        .collect();
    // @pob Classes/PassiveTree.lua:635-645 h=8a23ddb2
    // L628 (ORDER: module notes)
    for (_, node) in tree.nodes.pairs() {
        // L629
        let Some(out) = node.extra.get(n!("out")).table() else {
            continue;
        };
        for (_, other_id) in out.pairs() {
            // L630-632
            let Some(other_id) = link_id(&other_id) else {
                continue;
            };
            // L633 local other = nodeMap[otherId] (`nodeMap` is keyed by `node.id`: 2_6's
            // `tree.nodes` is a positional list, PassiveTree.lua:544)
            let other = node_map
                .get(&(other_id as i64))
                .cloned()
                .unwrap_or_else(|| lua_error!("attempt to index local 'other' (a nil value)"));
            // L635-639
            let node_group_proxy = node
                .group
                .get()
                .unwrap_or_else(|| lua_error!("attempt to index field 'group' (a nil value)"))
                .truthy(n!("isProxy"));
            let (nt, ot) = (node.r#type.get(), other.r#type.get());
            if &*nt != "ClassStart"
                && &*ot != "ClassStart"
                && &*nt != "Mastery"
                && &*ot != "Mastery"
                && node.ascendancyName.get() == other.ascendancyName.get()
                && !node.extra.truthy(n!("isProxy"))
                && !other.extra.truthy(n!("isProxy"))
                && !node_group_proxy
                && !node_group_proxy
            {
                // L640
                let built = build_connector(assets, &node, &other);
                // L641-644
                connectors.extend(built);
            }
        }
    }
    connectors
}

/// The connector polygons of one cluster subgraph: `subGraph.connectors` holds the node pairs
/// PS-C's `linkNodes` recorded, in call order (CCR-0072 §3); PoB builds the polygons there
/// (PassiveSpec.lua:2021-2025).
/// @pob Classes/PassiveSpec.lua:2021-2025 h=d719263c
pub fn build_subgraph_connectors(assets: &TreeAssets, sub_graph: &Table) -> Vec<Rc<Connector>> {
    let mut out = Vec::new();
    let Some(pairs) = sub_graph.table(n!("connectors")) else {
        return out;
    };
    for (_, pair) in pairs.ipairs() {
        let Some(pair) = pair.table() else { continue };
        let (Some(node1), Some(node2)) = (
            pair.get(1.0).obj::<Node>(),
            pair.get(2.0).obj::<Node>(),
        ) else {
            continue;
        };
        // L2022 local connectors = self.tree:BuildConnector(node1, node2)
        // L2023-2026
        out.extend(build_connector(assets, &node1, &node2));
    }
    out
}
