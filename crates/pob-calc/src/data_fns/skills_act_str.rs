//! @pob-file Data/Skills/act_str.lua @16de4b82d
//! Skill hooks of Data/Skills/act_str.lua (calc doc §8: `preDamageFunc`, `initialFunc`, `preSkillTypeFunc`,
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
        id: "data.skills.Earthquake.preDamageFunc",
        hash: "f83e1ae0bdaf20278a8f98f4c547f3f91a4fbe9bd6074350753a14d1bf68d7fd",
        f: SkillFuncImpl::Std(Earthquake_preDamageFunc),
    },
    FnRow {
        id: "data.skills.FrozenSweep.preDamageFunc",
        hash: "2723a80de969128512021a766ddcc18b794345df65a8d02b7f248081e64f7073",
        f: SkillFuncImpl::Std(FrozenSweep_preDamageFunc),
    },
    FnRow {
        id: "data.skills.FrozenSweepAltX.preDamageFunc",
        hash: "6e21ef23c19d262d7b7f44d7bd8cb3d6592b9f54e97155a45950adf17c31dc4d",
        f: SkillFuncImpl::Std(FrozenSweepAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.HeraldOfAsh.preDamageFunc",
        hash: "b2dc72cf68544399285372853b14f33a4ea9ffd7a65c100bcb6c05e8251636d2",
        f: SkillFuncImpl::Std(HeraldOfAsh_preDamageFunc),
    },
    FnRow {
        id: "data.skills.InfernalBlow.preDamageFunc",
        hash: "51d48b99032e2cafffa213dfd711a775ddfa0608c6f184c6aceb084e9c16c0d2",
        f: SkillFuncImpl::Std(InfernalBlow_preDamageFunc),
    },
    FnRow {
        id: "data.skills.InfernalBlowAltX.preDamageFunc",
        hash: "e7d573845e95ae52fe78cb4fbf8db7fffad8426a8b1326ae229343ccaf7dcff1",
        f: SkillFuncImpl::Std(InfernalBlowAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.MoltenShell.preDamageFunc",
        hash: "3a02403e817b375d2f14abed327b1bc6e16bec124cc220852b05eab7de952d01",
        f: SkillFuncImpl::Std(MoltenShell_preDamageFunc),
    },
    FnRow {
        id: "data.skills.VaalMoltenShell.preDamageFunc",
        hash: "87435ec8cd27eb64b0dde5d339e393bc127cbd034c313309a284591744107c8f",
        f: SkillFuncImpl::Std(VaalMoltenShell_preDamageFunc),
    },
    FnRow {
        id: "data.skills.MoltenStrike.preDamageFunc",
        hash: "a54f809c7509641eed7fbd55b08bbf94ea474af7f4161f5a52c61860da4c1c6f",
        f: SkillFuncImpl::Std(MoltenStrike_preDamageFunc),
    },
    FnRow {
        id: "data.skills.MoltenStrikeAltX.preDamageFunc",
        hash: "1aa115a4b356ed97dea4dc33d1b6a8d907005bac936b9870758a4a51ec7e5c69",
        f: SkillFuncImpl::Std(MoltenStrikeAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.Perforate.preDamageFunc",
        hash: "14405ec5edd00d5194d9589ea73b42a0096d422d8fb6bacc1ec9b4e4cb4ff8c6",
        f: SkillFuncImpl::Std(Perforate_preDamageFunc),
    },
    FnRow {
        id: "data.skills.PerforateAltX.preDamageFunc",
        hash: "14405ec5edd00d5194d9589ea73b42a0096d422d8fb6bacc1ec9b4e4cb4ff8c6",
        f: SkillFuncImpl::Std(PerforateAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.PerforateAltY.preDamageFunc",
        hash: "14405ec5edd00d5194d9589ea73b42a0096d422d8fb6bacc1ec9b4e4cb4ff8c6",
        f: SkillFuncImpl::Std(PerforateAltY_preDamageFunc),
    },
    FnRow {
        id: "data.skills.RageVortex.preSkillTypeFunc",
        hash: "3c8202d2061670642148688e8d08947e4e8a3124e115251dc7861020e5dc1146",
        f: SkillFuncImpl::Std(RageVortex_preSkillTypeFunc),
    },
    FnRow {
        id: "data.skills.StaticStrike.preDamageFunc",
        hash: "30f986590c9224a1fd7f329b289eca6159a4a7058fbb725a25311d8b767fdfcb",
        f: SkillFuncImpl::Std(StaticStrike_preDamageFunc),
    },
];

/// @pob Data/Skills/act_str.lua:3363-3366 h=d130e236
/// `data.skills.Earthquake.preDamageFunc` (preDamageFunc)
pub(crate) fn Earthquake_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_str.lua:3363-3366")
}

/// @pob Data/Skills/act_str.lua:4679-4713 h=050930d4
/// `data.skills.FrozenSweep.preDamageFunc` (preDamageFunc)
pub(crate) fn FrozenSweep_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_str.lua:4679-4713")
}

/// @pob Data/Skills/act_str.lua:4797-4831 h=56a1ceed
/// `data.skills.FrozenSweepAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn FrozenSweepAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_str.lua:4797-4831")
}

/// @pob Data/Skills/act_str.lua:5743-5745 h=1239adc2
/// `data.skills.HeraldOfAsh.preDamageFunc` (preDamageFunc)
pub(crate) fn HeraldOfAsh_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_str.lua:5743-5745")
}

/// @pob Data/Skills/act_str.lua:7115-7123 h=f991cf74
/// `data.skills.InfernalBlow.preDamageFunc` (preDamageFunc)
pub(crate) fn InfernalBlow_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_str.lua:7115-7123")
}

/// @pob Data/Skills/act_str.lua:7241-7249 h=53b6c5c2
/// `data.skills.InfernalBlowAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn InfernalBlowAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_str.lua:7241-7249")
}

/// @pob Data/Skills/act_str.lua:7599-7603 h=35162e38
/// `data.skills.MoltenShell.preDamageFunc` (preDamageFunc)
pub(crate) fn MoltenShell_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_str.lua:7599-7603")
}

/// @pob Data/Skills/act_str.lua:7701-7705 h=ec07950c
/// `data.skills.VaalMoltenShell.preDamageFunc` (preDamageFunc)
pub(crate) fn VaalMoltenShell_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_str.lua:7701-7705")
}

/// @pob Data/Skills/act_str.lua:7832-7882 h=23096fcc
/// `data.skills.MoltenStrike.preDamageFunc` (preDamageFunc)
pub(crate) fn MoltenStrike_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_str.lua:7832-7882")
}

/// @pob Data/Skills/act_str.lua:8011-8082 h=7f66fb3d
/// `data.skills.MoltenStrikeAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn MoltenStrikeAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_str.lua:8011-8082")
}

/// @pob Data/Skills/act_str.lua:8304-8306 h=8534ec2b
/// `data.skills.Perforate.preDamageFunc` (preDamageFunc)
pub(crate) fn Perforate_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_str.lua:8304-8306")
}

/// @pob Data/Skills/act_str.lua:8400-8402 h=8534ec2b
/// `data.skills.PerforateAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn PerforateAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_str.lua:8400-8402")
}

/// @pob Data/Skills/act_str.lua:8501-8503 h=8534ec2b
/// `data.skills.PerforateAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn PerforateAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_str.lua:8501-8503")
}

/// @pob Data/Skills/act_str.lua:9082-9091 h=e93d90b1
/// `data.skills.RageVortex.preSkillTypeFunc` (preSkillTypeFunc)
pub(crate) fn RageVortex_preSkillTypeFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_str.lua:9082-9091")
}

/// @pob Data/Skills/act_str.lua:10726-10730 h=a455bfbd
/// `data.skills.StaticStrike.preDamageFunc` (preDamageFunc)
pub(crate) fn StaticStrike_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_str.lua:10726-10730")
}
