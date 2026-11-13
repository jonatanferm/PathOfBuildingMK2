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

pub use crate::data_fns_types::{ExplosiveArrowFunc, FnRow, SkillFunc, SkillFuncImpl, SkillHook};

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
