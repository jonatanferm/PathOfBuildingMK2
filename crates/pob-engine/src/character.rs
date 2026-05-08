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
    #[serde(default)]
    pub pantheon_major: MajorGod,
    #[serde(default)]
    pub pantheon_minor: MinorGod,
    /// Named item-set saves the user has stored. The active set is the
    /// `items` field on Character; this list keeps inactive copies that
    /// the user can swap in.
    #[serde(default)]
    pub item_sets: Vec<NamedItemSet>,
    /// Party members — group-play teammates whose auras / curses /
    /// banners propagate onto the player. Each member's `mod_lines`
    /// are parsed by `mod_parser` and added to the player's modDB
    /// during `init_env` (skipped when `enabled = false`).
    #[serde(default)]
    pub party_members: Vec<PartyMember>,
    /// Tattoo overrides per allocated passive node — `(NodeId, mod
    /// text)`. Used as a Vec in the snapshot for deterministic save
    /// ordering; converted to a HashMap on the Character.
    #[serde(default)]
    pub tattoo_overrides: Vec<(NodeId, String)>,
}

/// One stored item-loadout save. `items` is the same `ItemSet` the
/// active hand uses — switching the active set just clones it back
/// onto `Character::items`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NamedItemSet {
    pub name: String,
    pub items: ItemSet,
}

/// Party member — a teammate's projected buffs / curses / banners.
/// `mod_lines` is free-form text the user pastes (one line per mod);
/// each line goes through `mod_parser::parse_mod_line` at compute time
/// with `source = Source::Other("Party:<name>")`. Issue #97 also
/// supports auto-extraction: the user can paste a teammate's PoB
/// share code, and `extracted_auras` is populated from their aura /
/// curse / banner gems via `pob_engine::skill::aura_buff_mods`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PartyMember {
    /// Display name shown in the Party tab and used as the mod source
    /// tag (so the Calcs-tab breakdown can attribute buffs to a
    /// specific teammate).
    pub name: String,
    /// Newline-separated mod lines (e.g. `25% increased Damage`,
    /// `+15% to all Elemental Resistances`). Parsed through the same
    /// path as `ConfigState::custom_mods`.
    pub mod_lines: String,
    /// Issue #97: gems auto-extracted from a pasted teammate PoB
    /// build code. Each entry feeds `aura_buff_mods` at compute time
    /// and contributes mods alongside the manual `mod_lines`. Stored
    /// rather than re-derived per frame so saves preserve the import
    /// even after the source code is forgotten.
    #[serde(default)]
    pub extracted_auras: Vec<ExtractedAura>,
    /// Toggle to A/B with vs. without this teammate's contribution.
    /// Default true so adding a member immediately applies their
    /// buffs.
    #[serde(default = "true_default_party")]
    pub enabled: bool,
}

/// One aura/curse/banner gem auto-extracted from a teammate's pasted
/// PoB code. The triple `(skill_id, level, quality)` is enough to
/// recover the projected mods through `SkillRegistry::get` +
/// `aura_buff_mods`. Mirrors the bare minimum PoB persists per
/// teammate (`Classes/PartyTab.lua`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtractedAura {
    pub skill_id: String,
    #[serde(default = "default_aura_level")]
    pub level: u32,
    #[serde(default)]
    pub quality: u32,
    /// Mirror the gem's enabled state from the source build —
    /// disabled gems on the teammate are not extracted by default,
    /// but the user may toggle one off without re-importing.
    #[serde(default = "true_default_party")]
    pub enabled: bool,
    /// Issue #97 (slice 2): manual aura-effect % override applied to
    /// the projected mod values at compute time. PoB scales aura
    /// values by `(1 + AuraEffect%/100) × BuffEffect_more` from the
    /// teammate's items / supports (Generosity, Empower, etc.); we
    /// don't yet recompute the teammate's full state at extract
    /// time, so this field lets the user dial in the effective
    /// scaling by hand. 0 = use the gem's raw L/Q values (no
    /// scaling); 50 = +50% on every projected mod. Negative values
    /// are clamped at -100% (no projection at all).
    #[serde(default)]
    pub effect_pct: i32,
}

fn default_aura_level() -> u32 {
    20
}

fn true_default_party() -> bool {
    true
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
            main_skill_level: c.main_skill.as_ref().map_or(20, |m| m.level),
            main_skill_quality: c.main_skill.as_ref().map_or(0, |m| m.quality),
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
            pantheon_major: c.pantheon_major,
            pantheon_minor: c.pantheon_minor,
            item_sets: c.item_sets.clone(),
            party_members: c.party_members.clone(),
            tattoo_overrides: c
                .tattoo_overrides
                .iter()
                .map(|(k, v)| (*k, v.clone()))
                .collect(),
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
            pantheon_major: self.pantheon_major,
            pantheon_minor: self.pantheon_minor,
            item_sets: self.item_sets,
            party_members: self.party_members,
            tattoo_overrides: self.tattoo_overrides.into_iter().collect(),
        }
    }
}

/// Reference to a class within a `PassiveTree`. Either the index (faster, fragile across
/// tree versions) or the name (slower, version-portable). We canonicalise on name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClassRef(pub String);

impl ClassRef {
    pub fn marauder() -> Self {
        Self("Marauder".into())
    }
    pub fn ranger() -> Self {
        Self("Ranger".into())
    }
    pub fn witch() -> Self {
        Self("Witch".into())
    }
    pub fn duelist() -> Self {
        Self("Duelist".into())
    }
    pub fn templar() -> Self {
        Self("Templar".into())
    }
    pub fn shadow() -> Self {
        Self("Shadow".into())
    }
    pub fn scion() -> Self {
        Self("Scion".into())
    }
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Bandit {
    #[default]
    KillAll,
    Alira,
    Kraityn,
    Oak,
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

/// Endgame Pantheon — Major God selection. PoB stores this on the
/// `<Build pantheonMajorGod="…">` attribute. Each god's "Soul"
/// (level-1 effect) is the player-facing baseline mod.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum MajorGod {
    #[default]
    None,
    TheBrineKing,
    Arakaali,
    Solaris,
    Lunaris,
}

impl MajorGod {
    pub fn as_pob_name(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::TheBrineKing => "TheBrineKing",
            Self::Arakaali => "Arakaali",
            Self::Solaris => "Solaris",
            Self::Lunaris => "Lunaris",
        }
    }

    pub fn from_pob_name(name: &str) -> Option<Self> {
        match name {
            "None" | "" => Some(Self::None),
            "TheBrineKing" => Some(Self::TheBrineKing),
            "Arakaali" => Some(Self::Arakaali),
            "Solaris" => Some(Self::Solaris),
            "Lunaris" => Some(Self::Lunaris),
            _ => None,
        }
    }

    /// Display label for the UI dropdown. PoB's data file uses the
    /// internal id; this is the in-game name.
    pub fn display(self) -> &'static str {
        match self {
            Self::None => "No major god",
            Self::TheBrineKing => "Soul of the Brine King",
            Self::Arakaali => "Soul of Arakaali",
            Self::Solaris => "Soul of Solaris",
            Self::Lunaris => "Soul of Lunaris",
        }
    }
}

/// Endgame Pantheon — Minor God selection. PoB has 8 minor gods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum MinorGod {
    #[default]
    None,
    Abberath,
    Gruthkul,
    Yugul,
    Shakari,
    Tukohama,
    Ralakesh,
    Garukhan,
    Ryslatha,
}

impl MinorGod {
    pub fn as_pob_name(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Abberath => "Abberath",
            Self::Gruthkul => "Gruthkul",
            Self::Yugul => "Yugul",
            Self::Shakari => "Shakari",
            Self::Tukohama => "Tukohama",
            Self::Ralakesh => "Ralakesh",
            Self::Garukhan => "Garukhan",
            Self::Ryslatha => "Ryslatha",
        }
    }

    pub fn from_pob_name(name: &str) -> Option<Self> {
        match name {
            "None" | "" => Some(Self::None),
            "Abberath" => Some(Self::Abberath),
            "Gruthkul" => Some(Self::Gruthkul),
            "Yugul" => Some(Self::Yugul),
            "Shakari" => Some(Self::Shakari),
            "Tukohama" => Some(Self::Tukohama),
            "Ralakesh" => Some(Self::Ralakesh),
            "Garukhan" => Some(Self::Garukhan),
            "Ryslatha" => Some(Self::Ryslatha),
            _ => None,
        }
    }

    pub fn display(self) -> &'static str {
        match self {
            Self::None => "No minor god",
            Self::Abberath => "Soul of Abberath",
            Self::Gruthkul => "Soul of Gruthkul",
            Self::Yugul => "Soul of Yugul",
            Self::Shakari => "Soul of Shakari",
            Self::Tukohama => "Soul of Tukohama",
            Self::Ralakesh => "Soul of Ralakesh",
            Self::Garukhan => "Soul of Garukhan",
            Self::Ryslatha => "Soul of Ryslatha",
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
    /// Endgame Pantheon Major God. The chosen god's "Soul" (level-1
    /// effect) is injected into the player modDB at compute time via
    /// `apply_pantheon_mods`.
    pub pantheon_major: MajorGod,
    /// Endgame Pantheon Minor God.
    pub pantheon_minor: MinorGod,
    /// Stored item-loadout saves the user has named (e.g. "Mapping",
    /// "Bossing"). The active loadout is the `items` field above; this
    /// list keeps inactive copies that the user can swap in via
    /// `activate_item_set`.
    pub item_sets: Vec<NamedItemSet>,
    /// Party members — group-play teammates whose auras / curses /
    /// banners propagate onto the player. Each member's `mod_lines`
    /// are parsed by `mod_parser` and added to the player's modDB
    /// during `init_env_with_bases` (skipped when `enabled = false`).
    pub party_members: Vec<PartyMember>,
    /// Tattoo overrides per allocated passive node (3.22+). Each entry
    /// `node_id → mod text` replaces the node's canonical `stats` with
    /// the tattoo's mod lines during compute. Removing an entry restores
    /// the original node. Mirrors PoB's `PassiveSpec.tattooOverrides`.
    pub tattoo_overrides: HashMap<NodeId, String>,
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
    /// User-typed "Custom Modifiers" textarea content. Mirrors PoB's Config-tab
    /// custom-modifiers feature: each newline-separated line is parsed by
    /// `mod_parser` and added to the player modDB with `source = "Custom"`.
    /// Used for what-if testing without editing items / tree.
    #[serde(default)]
    pub custom_mods: String,
    /// Encounter preset that injects PoB's standard Boss / Pinnacle / Uber
    /// enemy modifiers (resist defaults, ailment threshold MORE,
    /// `Condition:RareOrUnique` and `Condition:PinnacleBoss` flags).
    /// Mirrors PoB's `enemyIsBoss` ConfigOption.
    #[serde(default)]
    pub enemy_boss: EnemyBoss,
    /// "Enemies hit by AoE" — for shotgun-overlap skills like Earthquake,
    /// Tectonic Slam, or Vaal Ground Slam, the player can hit a single
    /// enemy more than once per cast. PoB exposes this as a Config-tab
    /// slider (default 1). The engine multiplies the per-cast hit
    /// average for AoE-tagged skills by this value. 0 / 1 = single hit;
    /// higher values stack overlapping AoE hits on the same target.
    #[serde(default)]
    pub enemies_hit_by_aoe: u32,
    /// `WarcryPower` config input — drives skills / nodes that scale per
    /// warcry power (e.g. "X% increased Damage per 5 Warcry Power").
    /// PoB exposes a slider 0..100 with 20 as the default ("a small pack
    /// of monsters in front of the player"). When this is `None` the
    /// engine falls back to its existing 20-default.
    #[serde(default)]
    pub warcry_power: Option<u32>,
    /// Fraction of the player's attacks that are "exerted" by an active
    /// warcry. Each exerted attack receives the `ExertedAttackDamage`
    /// MORE bonus from warcry support gems. Default 0 means "no
    /// exertion is active" (equivalent to no warcry being cast). PoB
    /// computes this from `ExertedAttackCount / (ExertedAttackCount +
    /// attacks_between_cries)`; we expose the result directly.
    #[serde(default)]
    pub exerted_attack_uptime: f64,
    /// Issue #109: when `true` the calc engine treats the
    /// `Weapon1Swap` / `Weapon2Swap` pair as the live pair instead
    /// of `Weapon1` / `Weapon2`. Mirrors PoB's per-ItemSet
    /// `useSecondWeaponSet` attribute (lifted to a build-level
    /// toggle so MK2 doesn't need to mirror PoB's per-set storage
    /// scheme to round-trip the wire format). Defaults `false`
    /// — the live pair stays as the primary weapons.
    #[serde(default)]
    pub use_second_weapon_set: bool,
    /// Issue #83 (slice 2): "# of nearby Enemies" — drives the
    /// `Multiplier:NearbyEnemies` BASE that PoB exposes from
    /// `ConfigOptions.lua:1193-1199`. Used by mods like Lunaris's
    /// "1% phys reduction for each nearby Enemy" and (via the
    /// derived `OnlyOneNearbyEnemy` condition) Solaris's "while
    /// there is only one nearby Enemy". 0 = no nearby enemies, no
    /// mod injection.
    #[serde(default)]
    pub nearby_enemies: u32,
}

/// PoB's `enemyIsBoss` four-option preset. The serialised PoB-XML
/// attribute is "None" / "Boss" / "Pinnacle" / "Uber" — see
/// `as_pob_name` / `from_pob_name`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum EnemyBoss {
    #[default]
    None,
    Boss,
    Pinnacle,
    Uber,
}

impl EnemyBoss {
    pub fn as_pob_name(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Boss => "Boss",
            Self::Pinnacle => "Pinnacle",
            Self::Uber => "Uber",
        }
    }

    pub fn from_pob_name(name: &str) -> Option<Self> {
        match name {
            "None" | "" => Some(Self::None),
            "Boss" => Some(Self::Boss),
            "Pinnacle" => Some(Self::Pinnacle),
            "Uber" => Some(Self::Uber),
            _ => None,
        }
    }

    /// Default elemental resist (%) the preset implies. PoB's Pinnacle
    /// boss is 50% all-ele / 30% chaos; Boss is 40 / 25; None is 0.
    pub fn default_resists(self) -> (i32, i32, i32, i32) {
        // (fire, cold, lightning, chaos)
        match self {
            Self::None => (0, 0, 0, 0),
            Self::Boss => (40, 40, 40, 25),
            Self::Pinnacle | Self::Uber => (50, 50, 50, 30),
        }
    }

    /// Default elemental penetration (%) the preset implies. PoB's
    /// `pinnacleBossPen = 15 / 5 = 3` and `uberBossPen = 40 / 5 = 8`
    /// (in `Data.lua`); Boss has no implicit pen. Used by the calc as
    /// "enemy resists are reduced by N%" — applied to FireResistTotal /
    /// ColdResistTotal / LightningResistTotal at hit time.
    pub fn default_penetration(self) -> i32 {
        match self {
            Self::None | Self::Boss => 0,
            Self::Pinnacle => 3,
            Self::Uber => 8,
        }
    }

    /// Default armour for the preset, mirroring upstream PoB's
    /// `data.bossStats.PinnacleArmourMean` (computed in
    /// `Modules/Data.lua`). Standard map mob: 0 (we already default
    /// `enemy_armour` from level via `MONSTER_ARMOUR_TABLE`); Boss
    /// inherits the level-derived value; Pinnacle / Uber use higher
    /// fixed scales — PoB's averaged number is around 36000 for
    /// pinnacle bosses.
    pub fn default_armour(self) -> u32 {
        match self {
            Self::None | Self::Boss => 0, // 0 → fall back to level-derived default
            Self::Pinnacle | Self::Uber => 36_000,
        }
    }

    /// Default evasion for the preset. PoB's
    /// `data.bossStats.PinnacleEvasionMean` runs around 6000.
    pub fn default_evasion(self) -> u32 {
        match self {
            Self::None => 1500, // standard map mob, matches PoB default
            Self::Boss => 1500,
            Self::Pinnacle | Self::Uber => 6_000,
        }
    }

    /// PoB's `damageTaken` multiplier per preset, derived from
    /// `data.misc.{stdBossDPSMult, pinnacleBossDPSMult, uberBossDPSMult}`
    /// in `Modules/Data.lua`. Currently exposed as an output key so
    /// callers can show "ratio of monster damage taken" in the UI;
    /// the calc engine doesn't fold this into MainSkillDPS yet
    /// (PoB models monster damage scaling, not player DPS scaling).
    pub fn dps_taken_multiplier(self) -> f64 {
        match self {
            Self::None => 1.0,
            Self::Boss => 4.0 / 4.40,
            Self::Pinnacle => 8.0 / 4.40,
            Self::Uber => 10.0 / 4.25,
        }
    }
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
            pantheon_major: MajorGod::default(),
            pantheon_minor: MinorGod::default(),
            item_sets: Vec::new(),
            party_members: Vec::new(),
            tattoo_overrides: HashMap::default(),
        }
    }

    /// Save the current `items` map as a named entry in `item_sets`.
    /// Returns the index of the new entry. If a set with the given
    /// name already exists, overwrites it and returns its index.
    pub fn save_item_set(&mut self, name: impl Into<String>) -> usize {
        let name = name.into();
        let snapshot = NamedItemSet {
            name: name.clone(),
            items: self.items.clone(),
        };
        if let Some(idx) = self.item_sets.iter().position(|s| s.name == name) {
            self.item_sets[idx] = snapshot;
            idx
        } else {
            self.item_sets.push(snapshot);
            self.item_sets.len() - 1
        }
    }

    /// Make `item_sets[idx]` the active loadout. Returns true if the
    /// swap happened (idx in range), false otherwise. The previously
    /// active items are NOT auto-saved — call `save_item_set` first if
    /// you want to keep them.
    pub fn activate_item_set(&mut self, idx: usize) -> bool {
        let Some(set) = self.item_sets.get(idx) else {
            return false;
        };
        self.items = set.items.clone();
        true
    }

    /// Remove the named set from `item_sets`. Returns true if removed.
    pub fn delete_item_set(&mut self, idx: usize) -> bool {
        if idx < self.item_sets.len() {
            self.item_sets.remove(idx);
            true
        } else {
            false
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

    /// The set of nodes treated as already-reached when path-finding: the
    /// user's actual `allocated` set plus the synthetic class-start (and
    /// ascendancy-start) anchors. The anchors aren't real allocations — they
    /// don't cost a point and don't appear in `allocated` — but PoB lets you
    /// grow a path from the class start as if they were. Used by
    /// `allocate_path` and the UI hover preview so they agree on what
    /// "reachable" means.
    pub fn pathfind_seeds(&self, tree: &PassiveTree) -> std::collections::HashSet<NodeId> {
        let mut seeds: std::collections::HashSet<NodeId> = self.allocated.iter().copied().collect();
        for s in crate::pathfind::anchor_nodes(tree, &self.class.0, self.ascendancy.as_deref()) {
            seeds.insert(s);
        }
        seeds
    }

    /// Allocate `target` and every unallocated node on the shortest path
    /// connecting it to the existing allocation. Mirrors PoB's "click an
    /// outlying notable to jump there" behaviour: when you click a far
    /// node we also allocate the chain of points it takes to reach it.
    ///
    /// The class start (and chosen ascendancy start) act as virtual seeds —
    /// the first click on a freshly-rolled Marauder grows a path from the
    /// Marauder start, not an isolated island.
    ///
    /// Returns the list of node ids that were newly inserted (in path
    /// order, target last). Returns an empty `Vec` if `target` was
    /// already allocated. Returns `None` if `target` is unreachable from
    /// any seed; in that case nothing changes.
    ///
    /// When there are no seeds at all (no class set and nothing allocated —
    /// only happens in synthetic test trees) the method falls back to
    /// inserting just `target`.
    pub fn allocate_path(&mut self, tree: &PassiveTree, target: NodeId) -> Option<Vec<NodeId>> {
        if self.allocated.contains(&target) {
            return Some(Vec::new());
        }
        let seeds = self.pathfind_seeds(tree);
        let path = if seeds.is_empty() {
            vec![target]
        } else {
            crate::pathfind::shortest_path_from_allocated(tree, &seeds, target)?
        };
        // First entry is a seed (real allocation or virtual anchor) — or
        // `target` itself in the no-seeds fallback. Skip it so we only return
        // and insert newly-added ids. Path[1..] is guaranteed not to contain
        // anchors because BFS stops at the first seed it hits.
        let first_idx = usize::from(!seeds.is_empty());
        let added: Vec<NodeId> = path[first_idx..].to_vec();
        for id in &added {
            self.allocated.insert(*id);
        }
        Some(added)
    }

    /// Unallocate `node` and any nodes that are now orphaned — that is,
    /// no longer connected to the character's class start (or picked
    /// ascendancy start) through the remaining allocation. Returns the
    /// full set of removed node ids.
    ///
    /// Does nothing and returns an empty `Vec` if `node` wasn't allocated.
    pub fn unallocate(&mut self, tree: &PassiveTree, node: NodeId) -> Vec<NodeId> {
        if !self.allocated.remove(&node) {
            return Vec::new();
        }
        let mut removed = vec![node];
        let seeds = crate::pathfind::anchor_nodes(tree, &self.class.0, self.ascendancy.as_deref());
        if seeds.is_empty() {
            // No anchor — every node is technically orphaned, but blowing
            // away the whole allocation surprises callers (e.g. tests with
            // synthetic class names). Leave the rest in place.
            return removed;
        }
        let allocated_set: std::collections::HashSet<NodeId> =
            self.allocated.iter().copied().collect();
        let anchored = crate::pathfind::anchored_subset(tree, &allocated_set, &seeds);
        let orphans: Vec<NodeId> = self
            .allocated
            .iter()
            .copied()
            .filter(|id| !anchored.contains(id))
            .collect();
        for id in &orphans {
            self.allocated.remove(id);
        }
        removed.extend(orphans);
        removed
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
