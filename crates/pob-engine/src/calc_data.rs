//! The complete calc entry point: every data input the app has loaded,
//! bundled so the desktop UI and the `pob_diff` parity harness compute a
//! build identically.

use ahash::HashMap;
use pob_data::PassiveTree;

use crate::{
    apply_minion_outputs, compute_full_with_clusters_and_timeless_lut, identify_timeless_jewel,
    Character, ClusterContext, Env, Output, SkillRegistry, TimelessLutContext,
};

/// Borrowed view of the loaded game data. Any `None` field disables the
/// corresponding calc layer (e.g. no cluster data → cluster jewels inert).
#[derive(Clone, Copy, Default)]
pub struct CalcData<'a> {
    pub skills: Option<&'a SkillRegistry>,
    pub bases: Option<&'a pob_data::bases::ItemBaseSet>,
    pub cluster_jewels: Option<&'a pob_data::ClusterJewelData>,
    pub cluster_jewel_mods: Option<&'a pob_data::ClusterModSet>,
    pub timeless_jewels: Option<&'a pob_data::TimelessJewelData>,
    pub timeless_node_index: Option<&'a pob_data::TimelessNodeIndex>,
    pub legion_pool: Option<&'a pob_data::LegionPool>,
    /// Inflated timeless LUTs per 1-based jewel type; see
    /// [`timeless_lut_types_needed`] for which ones a character needs.
    pub timeless_luts: Option<&'a HashMap<u32, Vec<u8>>>,
    pub minions: Option<&'a pob_data::MinionData>,
}

/// Compute `character` with every calc layer the data supports: item bases,
/// cluster-jewel sub-graphs, timeless keystone + notable replacement, and
/// the summoned minion's stats.
pub fn compute_with_data(
    character: &Character,
    tree: &PassiveTree,
    data: &CalcData<'_>,
) -> (Output, Env) {
    let cluster_ctx = match (data.cluster_jewels, data.cluster_jewel_mods) {
        (Some(jewels), Some(mods)) => Some(ClusterContext::new(jewels, mods)),
        _ => None,
    };
    let lut_ctx = match (
        data.timeless_node_index,
        data.legion_pool,
        data.timeless_luts,
    ) {
        (Some(node_index), Some(pool), Some(luts)) if !luts.is_empty() => {
            Some(TimelessLutContext {
                node_index,
                pool,
                luts,
            })
        }
        _ => None,
    };
    let (mut output, env) = compute_full_with_clusters_and_timeless_lut(
        character,
        tree,
        data.skills,
        data.bases,
        cluster_ctx,
        data.timeless_jewels,
        lut_ctx.as_ref(),
    );
    if let (Some(minions), Some(skills)) = (data.minions, data.skills) {
        apply_minion_outputs(character, skills, minions, &env, &mut output);
    }
    (output, env)
}

/// 1-based timeless jewel types socketed on `character` — the LUTs a caller
/// must inflate into [`CalcData::timeless_luts`].
#[must_use]
pub fn timeless_lut_types_needed(character: &Character) -> Vec<u32> {
    let mut types: Vec<u32> = character
        .socketed_jewels
        .entries
        .iter()
        .filter_map(|(_, item)| {
            let jewel = identify_timeless_jewel(item)?;
            pob_data::jewel_type_for_base(&jewel.base_name)
        })
        .collect();
    types.sort_unstable();
    types.dedup();
    types
}
