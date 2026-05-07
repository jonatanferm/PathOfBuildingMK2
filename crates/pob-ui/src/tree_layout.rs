//! Pure geometric layout for the passive tree.
//!
//! Mirrors `Classes/PassiveTree.lua:828-833` (node position) and `:CalcOrbitAngles`
//! (orbit-specific angle tables).

use ahash::HashMap;
use pob_data::{NodeId, PassiveTree};

#[derive(Debug, Clone, Copy)]
pub struct NodePos {
    pub x: f32,
    pub y: f32,
}

/// Compute the screen-space (tree-space, really) position of every node in `tree`.
/// Nodes that lack a group / orbit / orbit_index are placed at (0, 0) and the caller
/// can decide whether to skip them.
pub fn compute_node_positions(tree: &PassiveTree) -> HashMap<NodeId, NodePos> {
    // Pre-compute angle tables per orbit.
    let mut angle_tables: Vec<Vec<f32>> = Vec::with_capacity(tree.constants.skills_per_orbit.len());
    for &n in &tree.constants.skills_per_orbit {
        angle_tables.push(orbit_angles_rad(n));
    }

    let mut out: HashMap<NodeId, NodePos> = HashMap::default();
    for (id, node) in &tree.nodes {
        let Some(group_id) = node.group else {
            out.insert(*id, NodePos { x: 0.0, y: 0.0 });
            continue;
        };
        let Some(group) = tree.groups.get(&group_id) else {
            out.insert(*id, NodePos { x: 0.0, y: 0.0 });
            continue;
        };
        let orbit = node.orbit.unwrap_or(0) as usize;
        let orbit_index = node.orbit_index.unwrap_or(0) as usize;

        let radius = *tree.constants.orbit_radii.get(orbit).unwrap_or(&0) as f32;
        let table = angle_tables.get(orbit);
        let angle = table
            .and_then(|t| t.get(orbit_index).copied())
            .unwrap_or(0.0);

        let x = group.x + angle.sin() * radius;
        let y = group.y - angle.cos() * radius;
        out.insert(*id, NodePos { x, y });
    }
    out
}

pub(crate) fn orbit_angles_rad(nodes_in_orbit: u32) -> Vec<f32> {
    let degs: Vec<f32> = match nodes_in_orbit {
        16 => vec![
            0.0, 30.0, 45.0, 60.0, 90.0, 120.0, 135.0, 150.0, 180.0, 210.0, 225.0, 240.0,
            270.0, 300.0, 315.0, 330.0,
        ],
        40 => vec![
            0.0, 10.0, 20.0, 30.0, 40.0, 45.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0, 110.0,
            120.0, 130.0, 135.0, 140.0, 150.0, 160.0, 170.0, 180.0, 190.0, 200.0, 210.0,
            220.0, 225.0, 230.0, 240.0, 250.0, 260.0, 270.0, 280.0, 290.0, 300.0, 310.0,
            315.0, 320.0, 330.0, 340.0, 350.0,
        ],
        n if n > 0 => (0..n).map(|i| 360.0 * i as f32 / n as f32).collect(),
        _ => vec![0.0],
    };
    degs.into_iter().map(f32::to_radians).collect()
}
