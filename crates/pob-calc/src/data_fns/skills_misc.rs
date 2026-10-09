//! @pob-file Data/Skills/other.lua @16de4b82d
//! @pob-file Data/Skills/minion.lua @16de4b82d
//! @pob-file Data/Skills/spectre.lua @16de4b82d
//! @pob-file Data/Skills/sup_str.lua @16de4b82d
//! Skill hooks of Data/Skills/other.lua, Data/Skills/minion.lua, Data/Skills/spectre.lua, Data/Skills/sup_str.lua (calc doc §8: `preDamageFunc`, `initialFunc`, `preSkillTypeFunc`,
//! `postCritFunc`, `explosiveArrowFunc`).
//!
//! Owner: SK-B. Stub created by U0: signatures are final (frozen; change through a CCR),
//! bodies are `unported!` until the owner ports them (calc doc D9, §10.2).
#![allow(
    non_snake_case,
    unused_variables,
    unused_imports,
    dead_code,
    clippy::all,
    clippy::pedantic
)]

use crate::prelude::*;

use super::{FnRow, SkillFuncImpl};

/// Every skill hook of this file: id (closures.json), sha256 of its source lines, Rust body.
pub static ROWS: &[FnRow<SkillFuncImpl>] = &[
    FnRow {
        id: "data.skills.BloodSacramentUnique.initialFunc",
        hash: "f8f41b09ba743ff19d8b31e7eb52f055e786e2ceddf542d5883aa858d865c970",
        f: SkillFuncImpl::Std(BloodSacramentUnique_initialFunc),
    },
    FnRow {
        id: "data.skills.DeathWish.preDamageFunc",
        hash: "30eefc9e444f08bc31fd1c9f01b822f3d1824a5d1c08de98d748550188dd96ba",
        f: SkillFuncImpl::Std(DeathWish_preDamageFunc),
    },
    FnRow {
        id: "data.skills.HeraldOfTheBreach.preDamageFunc",
        hash: "80f82d62eb8f1acd1ab5c0fc61896af3a6cb3d1531a582e287fd1982e7237541",
        f: SkillFuncImpl::Std(HeraldOfTheBreach_preDamageFunc),
    },
    FnRow {
        id: "data.skills.LightningBolt.preDamageFunc",
        hash: "bcb877cd38f680213da072ab311e27cfa9cb87d1faa786617e844738aad9537e",
        f: SkillFuncImpl::Std(LightningBolt_preDamageFunc),
    },
    FnRow {
        id: "data.skills.EnemyExplode.preDamageFunc",
        hash: "7e9905c62fd2940c2fa3a44316dc64e87f0c8502f602d0c16b061cbb04e58913",
        f: SkillFuncImpl::Std(EnemyExplode_preDamageFunc),
    },
    FnRow {
        id: "data.skills.GAZombieCorpseGroundImpact.preDamageFunc",
        hash: "bc3a99f2a20c2aa3d459f84684ca97c7f18ea23622b4c852bfdd0a67f9443ecb",
        f: SkillFuncImpl::Std(GAZombieCorpseGroundImpact_preDamageFunc),
    },
    FnRow {
        id: "data.skills.MinionInstability.preDamageFunc",
        hash: "bc3a99f2a20c2aa3d459f84684ca97c7f18ea23622b4c852bfdd0a67f9443ecb",
        f: SkillFuncImpl::Std(MinionInstability_preDamageFunc),
    },
    FnRow {
        id: "data.skills.AzmeriHydraBarrage.preDamageFunc",
        hash: "03e59e6faf7109e4a7a2a36ece5042d3240adfa86c9ca60ff39b899a3d699a99",
        f: SkillFuncImpl::Std(AzmeriHydraBarrage_preDamageFunc),
    },
    FnRow {
        id: "data.skills.AvengingFlame.preDamageFunc",
        hash: "faaad88be39c14af61f127e760f1f689f9d4aae599fe769597b20a36b665670a",
        f: SkillFuncImpl::Std(AvengingFlame_preDamageFunc),
    },
];

/// @pob Data/Skills/other.lua:669-677 h=26bda14d
/// `data.skills.BloodSacramentUnique.initialFunc` (initialFunc)
pub(crate) fn BloodSacramentUnique_initialFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/other.lua:669-677")
}

/// @pob Data/Skills/other.lua:1282-1288 h=1642217a
/// `data.skills.DeathWish.preDamageFunc` (preDamageFunc)
pub(crate) fn DeathWish_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/other.lua:1282-1288")
}

/// @pob Data/Skills/other.lua:2258-2260 h=40093632
/// `data.skills.HeraldOfTheBreach.preDamageFunc` (preDamageFunc)
pub(crate) fn HeraldOfTheBreach_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/other.lua:2258-2260")
}

/// @pob Data/Skills/other.lua:2596-2598 h=0a437538
/// `data.skills.LightningBolt.preDamageFunc` (preDamageFunc)
pub(crate) fn LightningBolt_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/other.lua:2596-2598")
}

/// @pob Data/Skills/other.lua:6204-6252 h=e2a13f5d
/// `data.skills.EnemyExplode.preDamageFunc` (preDamageFunc)
pub(crate) fn EnemyExplode_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/other.lua:6204-6252")
}

/// @pob Data/Skills/minion.lua:756-758 h=83a41d9a
/// `data.skills.GAZombieCorpseGroundImpact.preDamageFunc` (preDamageFunc)
pub(crate) fn GAZombieCorpseGroundImpact_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/minion.lua:756-758")
}

/// @pob Data/Skills/minion.lua:1756-1758 h=83a41d9a
/// `data.skills.MinionInstability.preDamageFunc` (preDamageFunc)
pub(crate) fn MinionInstability_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/minion.lua:1756-1758")
}

/// @pob Data/Skills/spectre.lua:7277-7279 h=5645a921
/// `data.skills.AzmeriHydraBarrage.preDamageFunc` (preDamageFunc)
pub(crate) fn AzmeriHydraBarrage_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/spectre.lua:7277-7279")
}

/// @pob Data/Skills/sup_str.lua:2540-2548 h=ecbae34c
/// `data.skills.AvengingFlame.preDamageFunc` (preDamageFunc)
pub(crate) fn AvengingFlame_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/sup_str.lua:2540-2548")
}
