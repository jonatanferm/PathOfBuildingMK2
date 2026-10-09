//! Closures stored in data: the skill hook registry (calc doc §8, §8.1, D7).
//! Owner: U0 (frozen frame). The bodies are SK-A (`skills_act_int`) and SK-B
//! (`skills_act_dex`, `skills_act_str`, `skills_misc`). The
//! `registry_complete` test (crates/pob-oracle/tests/U0.rs) checks that every
//! `closures.json` id of the registered families has exactly one row with the
//! same hash.
#![allow(non_snake_case)]

use std::sync::LazyLock;

use crate::prelude::*;

pub mod skills_act_dex;
pub mod skills_act_int;
pub mod skills_act_str;
pub mod skills_misc;

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

/// Every skill hook by id.
pub static SKILL_FUNCS: LazyLock<ahash::AHashMap<&'static str, &'static FnRow<SkillFuncImpl>>> =
    LazyLock::new(|| {
        skills_act_int::ROWS
            .iter()
            .chain(skills_act_dex::ROWS.iter())
            .chain(skills_act_str::ROWS.iter())
            .chain(skills_misc::ROWS.iter())
            .map(|r| (r.id, r))
            .collect()
    });

fn missing_hook(_: &ActiveSkillRef, _: &Output, _: Option<&Breakdown>) {
    unported!("Data/Skills (skill hook not in the registry)")
}

/// The body of a skill hook (unknown id → `unported!` no-op).
pub fn skillFunc(r: &LuaFnRef) -> SkillFuncImpl {
    match SKILL_FUNCS.get(r.id.as_str()) {
        Some(row) => row.f,
        None => SkillFuncImpl::Std(missing_hook),
    }
}
