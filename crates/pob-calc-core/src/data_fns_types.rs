//! The types of the skill hook registry (calc doc §8, §8.1), moved here from
//! `data_fns/mod.rs` (owner U0) so the crates below `data_fns` can name them (CCR-0131;
//! docs/port/calc-split-plan.md B1): the probe's skill-hook replay names [`SkillHook`], and the
//! ConfigOptions / ModMap registries use [`FnRow`]. `data_fns/mod.rs` re-exports every item, so
//! `crate::data_fns::{SkillHook, SkillFunc, ExplosiveArrowFunc, SkillFuncImpl, FnRow}` are
//! unchanged.
#![allow(non_snake_case)]

use crate::prelude::*;

/// The skill hook fields of a granted effect.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SkillHook {
    initialFunc,
    preSkillTypeFunc,
    preDamageFunc,
    postCritFunc,
    preDotFunc,
    explosiveArrowFunc,
}

impl SkillHook {
    /// The hook of `grantedEffect[name]`.
    pub fn of(self, ge: &GrantedEffect) -> Option<&LuaFnRef> {
        match self {
            SkillHook::initialFunc => ge.initialFunc.as_ref(),
            SkillHook::preSkillTypeFunc => ge.preSkillTypeFunc.as_ref(),
            SkillHook::preDamageFunc => ge.preDamageFunc.as_ref(),
            SkillHook::postCritFunc => ge.postCritFunc.as_ref(),
            SkillHook::preDotFunc => ge.preDotFunc.as_ref(),
            SkillHook::explosiveArrowFunc => ge.explosiveArrowFunc.as_ref(),
        }
    }
}

/// `func(activeSkill, output, breakdown)`: `output`/`breakdown` are the offence
/// top-level ones (runSkillFunc, CalcOffence.lua:509-514).
pub type SkillFunc =
    fn(activeSkill: &ActiveSkillRef, output: &Output, breakdown: Option<&Breakdown>);
/// `explosiveArrowFunc(activeSkill, output, globalOutput, globalBreakdown, env)` (act_dex.lua:6699).
pub type ExplosiveArrowFunc = fn(&ActiveSkillRef, &Output, &Output, Option<&Breakdown>, &EnvRef);

#[derive(Clone, Copy)]
pub enum SkillFuncImpl {
    Std(SkillFunc),
    ExplosiveArrow(ExplosiveArrowFunc),
}

/// One registry row: the closure's id (closures.json), the sha256 of its
/// source lines (the drift check) and its Rust body.
pub struct FnRow<F: 'static> {
    pub id: &'static str,
    pub hash: &'static str,
    pub f: F,
}
