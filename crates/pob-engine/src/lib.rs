//! Path of Building calc engine — pure, Wasm-clean, no I/O.
//!
//! Rule of thumb: this crate must compile to `wasm32-unknown-unknown` without polyfills. That
//! means no `std::fs`, `std::env`, `std::process`, `std::net`, no `tokio`, no native windowing,
//! no `mlua`. `std::collections` and the rest of `core`/`alloc` are fine.
//!
//! See `docs/architecture-current.md` for the upstream Lua engine map.
//!
//! ## Module map
//!
//! - [`modifier`] — `Mod`, `ModValue`, `ModType`, `Tag`, `Source`. The data carrier.
//! - [`mod_db`] — `ModStore`, `ModDB`, `ModList`. Storage + query API
//!   (`Sum` / `More` / `Flag` / `Override` / `List`).
//! - [`mod_parser`] — English text → `Vec<Mod>`. A tiny subset of PoB's `ModParser.lua`
//!   for Phase 2; expanded in Phase 3.
//! - [`character`] — `Character` (class + level + allocated nodes), `Build` (the user's
//!   configuration). Runtime version of PoB's `Build`.
//! - [`env`] — `Env`, `Output`. Computation context.
//! - [`perform`] — top-level `compute(build) -> Output`. Calls the basic-stats passes.

pub mod breakdown;
pub mod character;
pub mod cluster_synth;
pub mod env;
pub mod ggg_import;
pub mod item_parser;
pub mod jewel_radius;
pub mod minion;
pub mod mod_db;
pub mod mod_parser;
pub mod modifier;
pub mod pathfind;
pub mod perform;
pub mod pob_export;
pub mod pob_import;
pub mod power;
pub mod share;
pub mod skill;
pub mod timeless;

pub use breakdown::{derive_for, Breakdown, BreakdownStep, ModSource, COVERED_KEYS};
pub use character::{Character, CharacterSnapshot, ClassRef, ConfigState};
pub use cluster_synth::{
    parse_cluster_jewel, synthesise_all, synthesise_for_socket, ClusterJewelSpec,
    ParsedClusterJewel,
};
pub use env::{Env, Output};
pub use ggg_import::{
    apply_passive_jewels as apply_ggg_passive_jewels, build_character as build_character_from_ggg,
    build_character_with_skills as build_character_from_ggg_with_skills,
    decode_mastery_effects as decode_ggg_mastery_effects, default_skill_id_from_type_line,
    encode_account_name as ggg_encode_account_name, get_characters_url as ggg_get_characters_url,
    get_items_url as ggg_get_items_url, get_passive_skills_url as ggg_get_passive_skills_url,
    parse_character_list as parse_ggg_character_list, parse_items as parse_ggg_items,
    parse_passive_skills as parse_ggg_passive_skills, CharacterList as GggCharacterList,
    CharacterSummary as GggCharacterSummary, GggImportError, ItemsResponse as GggItemsResponse,
    PassiveSkillsResponse as GggPassiveSkillsResponse,
};
pub use item_parser::{apply_item_set, apply_item_set_with_bases, parse_item, ItemApplyReport};
pub use jewel_radius::{
    allocated_nodes_in_radius, apply_non_radius_socketed_jewels, apply_radius_jewels,
    extend_anchored_with_intuitive_leap, identify_radius_jewel, intuitive_leap_radius_set,
    intuitive_leap_reachable, node_position, nodes_in_radius, HandlerKind, RadiusJewel,
    RadiusJewelReport, SocketedJewels,
};
pub use minion::{
    apply_minion_hit_chance, apply_minion_outputs, parse_minion_intrinsic_mods, select_minion_type,
    write_minion_outputs, MinionState,
};
pub use mod_db::{ModDB, ModList, ModStore};
pub use mod_parser::{parse_mod_line, ParsedMod};
pub use modifier::{Mod, ModType, ModValue, Source, Tag, TagKind};
pub use perform::{
    canonical_weapon_class, compute, compute_full, compute_full_with_clusters,
    compute_full_with_clusters_and_timeless, compute_full_with_env, ClusterContext,
};
pub use pob_export::{export_pob_code, export_pob_xml};
pub use pob_import::{import_pob_code, import_pob_xml, resolve_share_url, PobImportError};
pub use power::{
    format_top_contributors, rank_item_modlines, rank_node_additions, score_item_modline_removal,
    score_node_addition, score_node_removal, ItemModlineScore, NodeScore,
};
pub use share::{export_code, import_code, ShareError};
pub use skill::{skill_for_quality, skill_mods, MainSkill, QualityId, SkillRegistry};
pub use timeless::{
    apply_keystone_replacements, compute_keystone_replacements, conquered_keystone_set,
    identify_timeless_jewel, KeystoneReplacement, TimelessJewel,
};
