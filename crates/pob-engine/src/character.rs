//! `Character` — the user's configuration. Phase 2 minimal version: class, level, and
//! allocated tree node ids.

use std::collections::HashSet;

use ahash::HashMap;
use pob_data::{Class, ItemSet, NodeId, PassiveTree};
use serde::{Deserialize, Serialize};

use crate::skill::MainSkill;

// Re-export so character.rs is the canonical Character module.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CharacterSnapshot {
    pub class: String,
    pub ascendancy: Option<String>,
    pub level: u32,
    pub allocated: Vec<NodeId>,
    pub items: ItemSet,
    pub main_skill_id: Option<String>,
    pub main_skill_level: u32,
    pub main_skill_quality: u32,
    pub config: ConfigState,
    pub notes: String,
    #[serde(default)]
    pub mastery_selections: Vec<(NodeId, u32)>,
    #[serde(default)]
    pub skill_groups: Vec<SocketGroupSnapshot>,
    #[serde(default = "one")]
    pub main_socket_group: u32,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SocketGroupSnapshot {
    pub label: String,
    pub gems: Vec<GemSnapshot>,
    #[serde(default = "one")]
    pub main_active_skill_index: u32,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GemSnapshot {
    pub skill_id: String,
    #[serde(default)]
    pub level: u32,
    #[serde(default)]
    pub quality: u32,
}

impl CharacterSnapshot {
    pub fn from_character(c: &Character) -> Self {
        Self {
            class: c.class.0.clone(),
            ascendancy: c.ascendancy.clone(),
            level: c.level,
            allocated: c.allocated.iter().copied().collect(),
            items: c.items.clone(),
            main_skill_id: c.main_skill.as_ref().map(|m| m.skill_id.clone()),
            main_skill_level: c.main_skill.as_ref().map(|m| m.level).unwrap_or(20),
            main_skill_quality: c.main_skill.as_ref().map(|m| m.quality).unwrap_or(0),
            config: c.config.clone(),
            notes: c.notes.clone(),
            mastery_selections: c.mastery_selections.iter().map(|(k, v)| (*k, *v)).collect(),
            skill_groups: c
                .skill_groups
                .iter()
                .map(|g| SocketGroupSnapshot {
                    label: g.label.clone(),
                    gems: g
                        .gems
                        .iter()
                        .map(|m| GemSnapshot {
                            skill_id: m.skill_id.clone(),
                            level: m.level,
                            quality: m.quality,
                        })
                        .collect(),
                    main_active_skill_index: g.main_active_skill_index,
                    enabled: g.enabled,
                })
                .collect(),
            main_socket_group: c.main_socket_group,
        }
    }
    pub fn into_character(self) -> Character {
        let groups: Vec<SocketGroup> = self
            .skill_groups
            .into_iter()
            .map(|g| SocketGroup {
                label: g.label,
                gems: g
                    .gems
                    .into_iter()
                    .map(|gem| MainSkill {
                        skill_id: gem.skill_id,
                        level: gem.level.max(1),
                        quality: gem.quality,
                        enabled: true,
                    })
                    .collect(),
                main_active_skill_index: g.main_active_skill_index.max(1),
                enabled: g.enabled,
            })
            .collect();
        Character {
            class: ClassRef(self.class),
            ascendancy: self.ascendancy,
            level: self.level,
            allocated: self.allocated.into_iter().collect(),
            items: self.items,
            main_skill: self.main_skill_id.map(|id| MainSkill {
                skill_id: id,
                level: self.main_skill_level,
                quality: self.main_skill_quality,
                enabled: true,
            }),
            skill_groups: groups,
            main_socket_group: self.main_socket_group,
            config: self.config,
            notes: self.notes,
            mastery_selections: self.mastery_selections.into_iter().collect(),
        }
    }
}

/// Reference to a class within a `PassiveTree`. Either the index (faster, fragile across
/// tree versions) or the name (slower, version-portable). We canonicalise on name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClassRef(pub String);

impl ClassRef {
    pub fn marauder() -> Self { Self("Marauder".into()) }
    pub fn ranger() -> Self { Self("Ranger".into()) }
    pub fn witch() -> Self { Self("Witch".into()) }
    pub fn duelist() -> Self { Self("Duelist".into()) }
    pub fn templar() -> Self { Self("Templar".into()) }
    pub fn shadow() -> Self { Self("Shadow".into()) }
    pub fn scion() -> Self { Self("Scion".into()) }
}

/// A single PoB-style "socket group" — a set of linked gems that share buffs
/// from supports inside the same group. Phase 5 minimum: track the gem list
/// and which one is the active skill the engine should target. Support-gem
/// effect propagation is not yet wired through the calc layer.
#[derive(Debug, Clone, Default)]
pub struct SocketGroup {
    pub label: String,
    /// Gems socketed in this group (main + supports). Index into here +1
    /// matches PoB's `mainActiveSkill` attribute.
    pub gems: Vec<crate::skill::MainSkill>,
    /// 1-based index of the active skill within `gems` (PoB convention).
    pub main_active_skill_index: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Character {
    pub class: ClassRef,
    pub ascendancy: Option<String>,
    pub level: u32,
    pub allocated: HashSet<NodeId>,
    pub items: ItemSet,
    pub main_skill: Option<MainSkill>,
    /// All socket groups defined on the character. The "main" group is whichever
    /// `mainSocketGroup` index points at — that group's `main_active_skill_index`
    /// gem becomes `main_skill`.
    pub skill_groups: Vec<SocketGroup>,
    /// 1-based index of the active socket group (PoB's `mainSocketGroup`).
    pub main_socket_group: u32,
    pub config: ConfigState,
    pub notes: String,
    /// Selected mastery effect per mastery node id. PoB stores this as
    /// `PassiveSpec.masterySelections`. Without an entry, an allocated mastery node
    /// contributes no stats (because the user hasn't picked an effect).
    pub mastery_selections: HashMap<NodeId, u32>,
}

/// Encounter / condition configuration. Mirrors PoB's Config tab:
/// enemy stats + conditional toggles + buff toggles.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigState {
    pub enemy_level: u32,
    pub enemy_fire_resist: i32,
    pub enemy_cold_resist: i32,
    pub enemy_lightning_resist: i32,
    pub enemy_chaos_resist: i32,
    /// Enemy evasion (used for player attack hit-chance calc). Default 1500 for an
    /// iLvl-84 monster — matches PoB's standard map mob.
    #[serde(default)]
    pub enemy_evasion: u32,
    /// Player-side condition toggles (`FullLife`, `LowLife`, …) — applied to
    /// `EvalState.conditions` at perform time so tagged mods activate.
    pub conditions: HashMap<String, bool>,
    /// Multiplier counters (`PowerCharge` count, `FrenzyCharge` count, …).
    pub multipliers: HashMap<String, f64>,
}

impl ConfigState {
    pub fn default_with_enemy() -> Self {
        Self {
            enemy_level: 84,
            enemy_evasion: 1500,
            ..Self::default()
        }
    }
}

impl Default for ClassRef {
    fn default() -> Self {
        Self::scion()
    }
}

impl Character {
    pub fn new(class: ClassRef, level: u32) -> Self {
        Self {
            class,
            ascendancy: None,
            level,
            allocated: HashSet::new(),
            items: ItemSet::new(),
            main_skill: None,
            skill_groups: Vec::new(),
            main_socket_group: 1,
            config: ConfigState::default_with_enemy(),
            notes: String::new(),
            mastery_selections: HashMap::default(),
        }
    }

    /// Refresh `main_skill` from `skill_groups[main_socket_group-1].gems[active-1]`.
    /// Call after editing socket groups or pointing main_socket_group at a new
    /// group; the calc layer reads `main_skill` directly.
    pub fn sync_main_skill(&mut self) {
        let group_idx = self
            .main_socket_group
            .saturating_sub(1)
            .min(self.skill_groups.len() as u32) as usize;
        let Some(group) = self.skill_groups.get(group_idx) else {
            return;
        };
        if !group.enabled {
            return;
        }
        let gem_idx = (group.main_active_skill_index.saturating_sub(1) as usize)
            .min(group.gems.len().saturating_sub(1));
        if let Some(g) = group.gems.get(gem_idx) {
            self.main_skill = Some(g.clone());
        }
    }

    pub fn allocate(&mut self, node: NodeId) {
        self.allocated.insert(node);
    }

    /// Find the `Class` definition in the tree.
    pub fn resolve_class<'a>(&self, tree: &'a PassiveTree) -> Option<&'a Class> {
        tree.classes.iter().find(|c| c.name == self.class.0)
    }
}
