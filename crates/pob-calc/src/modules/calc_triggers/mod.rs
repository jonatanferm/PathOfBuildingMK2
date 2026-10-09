//! CalcTriggers.lua: the `TriggerConfig` type (calc doc §8.1). Owner: U0
//! (frozen). The handlers are TR-A's (`handlers`), the `configTable` arms and
//! `calcs.triggers` TR-B's (`config_table`).
#![allow(non_snake_case)]

use crate::prelude::*;

pub mod config_table;
pub mod handlers;

pub use config_table::triggers;
pub use handlers::{calcMultiSpellRotationImpact, isTriggered};

/// What `configTable[key](env)` returns (CalcTriggers.lua 902-1584) and
/// `defaultTriggerHandler` reads. Keys the handler reads that are not typed
/// are in `extra` (TR-A census).
#[derive(Default)]
pub struct TriggerConfig {
    pub source: Option<ActiveSkillRef>,
    pub actor: Option<ActorRef>,
    pub triggerName: Option<Name>,
    pub triggerOnUse: bool,
    pub useCastRate: bool,
    pub triggerChance: Option<f64>,
    pub assumingEveryHitKills: bool,
    pub sourceWeapon: Option<Name>,
    pub triggerSkillCond: Option<Rc<dyn Fn(&EnvRef, &ActiveSkillRef) -> bool>>,
    pub customHandler: Option<fn(&EnvRef, &mut TriggerConfig) -> Option<Table>>,
    pub comparer: Option<fn(&EnvRef, Name, &ActiveSkillRef, f64) -> bool>,
    pub extra: Table,
}
