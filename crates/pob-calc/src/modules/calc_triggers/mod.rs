//! CalcTriggers.lua: the `TriggerConfig` type (calc doc §8.1). Owner: U0
//! (frozen). The handlers are TR-A's (`handlers`), the `configTable` arms and
//! `calcs.triggers` TR-B's (`config_table`).
#![allow(non_snake_case)]

use crate::prelude::*;

pub mod config_table;
pub mod handlers;

pub use config_table::triggers;
pub use handlers::{calcMultiSpellRotationImpact, isTriggered};

/// A `function(env, skill)` predicate of a trigger config (`triggerSkillCond`,
/// `triggeredSkillCond`). Only its truthiness is used.
pub type SkillCondFn = Rc<dyn Fn(&EnvRef, &ActiveSkillRef) -> bool>;

/// A `comparer = function(env, uuid, source, triggerRate)` of a trigger config: `source` and
/// `triggerRate` may be nil (findTriggerSkill's first candidate, CalcTriggers.lua:83), and the
/// arms' comparers close over locals (CalcTriggers.lua:1090, 1104, 1318, 1354, 1482; CCR-0087 §2).
pub type ComparerFn = Rc<dyn Fn(&EnvRef, Name, Option<&ActiveSkillRef>, Option<f64>) -> bool>;

/// What `configTable[key](env)` returns (CalcTriggers.lua 902-1584) and
/// `defaultTriggerHandler` reads. Keys the handler reads that are not typed
/// are in `extra` (TR-A census, CCR-0087 §4).
#[derive(Default)]
pub struct TriggerConfig {
    pub source: Option<ActiveSkillRef>,
    pub actor: Option<ActorRef>,
    pub triggerName: Option<Name>,
    pub triggerOnUse: bool,
    pub useCastRate: bool,
    pub triggerChance: Option<f64>,
    pub assumingEveryHitKills: bool,
    /// `sourceWeapon = true` (CalcTriggers.lua 1537, 1580); only tested (403-405, 447). CCR-0087 §3.
    pub sourceWeapon: bool,
    pub triggerSkillCond: Option<SkillCondFn>,
    /// CalcTriggers.lua 408-419, 797-814 (CCR-0087 §1).
    pub triggeredSkillCond: Option<SkillCondFn>,
    pub customHandler: Option<fn(&EnvRef, &mut TriggerConfig) -> Option<Table>>,
    /// CCR-0087 §2.
    pub comparer: Option<ComparerFn>,
    pub extra: Table,
}
