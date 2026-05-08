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
    #[serde(default)]
    pub bandit: Bandit,
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
    #[serde(default = "true_default")]
    pub enabled: bool,
}

fn true_default() -> bool {
    true
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
                            enabled: m.enabled,
                        })
                        .collect(),
                    main_active_skill_index: g.main_active_skill_index,
                    enabled: g.enabled,
                })
                .collect(),
            main_socket_group: c.main_socket_group,
            bandit: c.bandit,
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
                        enabled: gem.enabled,
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
            bandit: self.bandit,
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

/// Act 2 bandit reward — see `Data/Misc.lua` and `CalcSetup.lua` in PoB.
/// `KillAll` is the default; the other three each grant a small package of
/// stats. PoB stores this as a string attribute on `<Build bandit="…">`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Bandit {
    KillAll,
    Alira,
    Kraityn,
    Oak,
}

impl Default for Bandit {
    fn default() -> Self {
        Self::KillAll
    }
}

impl Bandit {
    /// PoB's serialised name on the `<Build bandit="…">` attribute and on
    /// CharacterSnapshot share codes.
    pub fn as_pob_name(self) -> &'static str {
        match self {
            Self::KillAll => "None",
            Self::Alira => "Alira",
            Self::Kraityn => "Kraityn",
            Self::Oak => "Oak",
        }
    }

    pub fn from_pob_name(name: &str) -> Option<Self> {
        match name {
            "None" | "Kill All" | "KillAll" | "" => Some(Self::KillAll),
            "Alira" => Some(Self::Alira),
            "Kraityn" => Some(Self::Kraityn),
            "Oak" => Some(Self::Oak),
            _ => None,
        }
    }
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
    /// Act 2 bandit choice. KillAll grants +2 passive points (tracked
    /// elsewhere); the named bandits each apply a small reward via
    /// `apply_bandit_mods` at compute time.
    pub bandit: Bandit,
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
    /// Enemy armour (used for physical-hit damage reduction via PoB's
    /// `armour / (armour + 5 × raw)` formula in CalcDefence.lua). Default 0
    /// to match PoB's bare target-dummy mode; common Pinnacle profile uses
    /// 36000.
    #[serde(default)]
    pub enemy_armour: u32,
    /// Defender-side avoidance against the player's hits. Each value is the
    /// percentage chance the enemy avoids/halves the hit. Mirrors PoB's
    /// `enemyBlockChance`, `enemyDodgeChance`, and `enemySuppressionChance`
    /// config inputs from the Enemy section of the Config tab.
    #[serde(default)]
    pub enemy_block_chance: u32,
    #[serde(default)]
    pub enemy_dodge_chance: u32,
    #[serde(default)]
    pub enemy_suppression_chance: u32,
    /// How many projectiles from a single cast/attack hit the target — PoB
    /// calls this "Projectiles hit target" and uses it for shotgun-style
    /// builds (Barrage, Tornado Shot focal-point). 0 / 1 = single hit (no
    /// shotgun); higher values multiply `MainSkillDPS` by `min(count,
    /// ProjectileCount)`.
    #[serde(default)]
    pub projectiles_hitting_target: u32,
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

    /// Effective enemy armour for damage-reduction calcs. When the user has
    /// set `enemy_armour` to a non-zero value, that value wins. Otherwise we
    /// mirror PoB's behaviour and pick the level-derived default from
    /// `MONSTER_ARMOUR_TABLE` (Data/Misc.lua) — that's what the PoB config
    /// panel shows as the placeholder when the user hasn't typed anything.
    pub fn effective_enemy_armour(&self) -> u32 {
        if self.enemy_armour > 0 {
            return self.enemy_armour;
        }
        let lvl = self.enemy_level.clamp(1, MONSTER_ARMOUR_TABLE.len() as u32);
        MONSTER_ARMOUR_TABLE[(lvl - 1) as usize]
    }
}

/// PoB `Data/Misc.lua:13` — base monster armour by level (1–100).
const MONSTER_ARMOUR_TABLE: [u32; 100] = [
    12, 15, 19, 23, 27, 32, 37, 43, 50, 57, 65, 74, 83, 94, 105, 118, 132, 147, 164, 182, 202, 224,
    248, 275, 303, 334, 368, 405, 445, 489, 537, 589, 646, 707, 774, 846, 925, 1010, 1103, 1204,
    1313, 1432, 1560, 1700, 1850, 2014, 2191, 2383, 2591, 2815, 3059, 3322, 3607, 3915, 4248, 4608,
    4997, 5418, 5873, 6365, 6896, 7469, 8089, 8757, 9480, 10259, 11101, 12009, 12989, 14047, 15188,
    16419, 17747, 19178, 20722, 22387, 24182, 26117, 28203, 30451, 32873, 35483, 38296, 41326,
    44591, 48107, 51894, 55973, 60365, 65095, 70188, 75670, 81573, 87926, 94765, 102125, 110047,
    118571, 127744, 137613,
];

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
            bandit: Bandit::default(),
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

    /// Count allocated nodes that belong to *some* ascendancy. PoB caps this at
    /// `tree.points.ascendancy_points` (8 by default, exposed in tree data).
    pub fn ascendancy_alloc_count(&self, tree: &PassiveTree) -> u32 {
        self.allocated
            .iter()
            .filter(|id| {
                tree.nodes
                    .get(id)
                    .and_then(|n| n.ascendancy_name.as_deref())
                    .is_some()
            })
            .count() as u32
    }

    /// True iff allocating one more ascendancy node would stay within the
    /// `tree.points.ascendancy_points` budget. UI gates clicks with this.
    pub fn can_allocate_ascendancy(&self, tree: &PassiveTree) -> bool {
        self.ascendancy_alloc_count(tree) < tree.points.ascendancy_points
    }
}
