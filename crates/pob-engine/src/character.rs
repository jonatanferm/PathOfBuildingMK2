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
        }
    }
    pub fn into_character(self) -> Character {
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
            }),
            config: self.config,
            notes: self.notes,
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

#[derive(Debug, Clone, Default)]
pub struct Character {
    pub class: ClassRef,
    pub ascendancy: Option<String>,
    pub level: u32,
    pub allocated: HashSet<NodeId>,
    pub items: ItemSet,
    pub main_skill: Option<MainSkill>,
    pub config: ConfigState,
    pub notes: String,
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
            config: ConfigState::default_with_enemy(),
            notes: String::new(),
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
