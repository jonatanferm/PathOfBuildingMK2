//! @pob-file Data/Skills/act_dex.lua @16de4b82d
//! Skill hooks of Data/Skills/act_dex.lua (calc doc §8: `preDamageFunc`, `initialFunc`, `preSkillTypeFunc`,
//! `postCritFunc`, `explosiveArrowFunc`).
//!
//! Owner: SK-B. Signatures are U0's (frozen; change through a CCR); the bodies are ported
//! statement by statement from the Lua closures (calc doc §3, §8). Rust-only plumbing (field
//! access that raises Lua's index-of-nil error, `x or y` on mod values) is in the private
//! helpers at the end of the file.
#![allow(
    non_snake_case,
    unused_variables,
    unused_imports,
    dead_code,
    unused_assignments,
    clippy::all,
    clippy::pedantic
)]

use crate::prelude::*;

use super::{FnRow, SkillFuncImpl};
use crate::modules::calc_offence::calcSkillDuration;
use pob_gamedata::tag;

/// Every skill hook of this file: id (closures.json), sha256 of its source lines, Rust body.
pub static ROWS: &[FnRow<SkillFuncImpl>] = &[
    FnRow {
        id: "data.skills.Barrage.preDamageFunc",
        hash: "659342bba434fa868ca4478a8ef9e416bb275f9da00e180d681b210af698ca1b",
        f: SkillFuncImpl::Std(Barrage_preDamageFunc),
    },
    FnRow {
        id: "data.skills.BarrageAltX.preDamageFunc",
        hash: "659342bba434fa868ca4478a8ef9e416bb275f9da00e180d681b210af698ca1b",
        f: SkillFuncImpl::Std(BarrageAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.BladeBlast.preDamageFunc",
        hash: "5b44bc750e2f5bb739428f164abf9c3a1177a4a0b152d5521a449d49dd43762d",
        f: SkillFuncImpl::Std(BladeBlast_preDamageFunc),
    },
    FnRow {
        id: "data.skills.BladeBlastAltX.preDamageFunc",
        hash: "5b44bc750e2f5bb739428f164abf9c3a1177a4a0b152d5521a449d49dd43762d",
        f: SkillFuncImpl::Std(BladeBlastAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.BladeFlurry.preDamageFunc",
        hash: "6fd38a79cb34281f091e02470a78846b6094afd6c97b288d5b65ebcc48ac918a",
        f: SkillFuncImpl::Std(BladeFlurry_preDamageFunc),
    },
    FnRow {
        id: "data.skills.BladeFlurryAltX.preDamageFunc",
        hash: "6fd38a79cb34281f091e02470a78846b6094afd6c97b288d5b65ebcc48ac918a",
        f: SkillFuncImpl::Std(BladeFlurryAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.BladeVortex.preDamageFunc",
        hash: "6d53d97ad73356d74c3ea60e46f26bfa2f76739c0ba480bae189da0fa2e82ba2",
        f: SkillFuncImpl::Std(BladeVortex_preDamageFunc),
    },
    FnRow {
        id: "data.skills.VaalBladeVortex.preDamageFunc",
        hash: "155ceb3b36862cb3a1fd5c046642cb437f354b1d9f612577a85e49a4608edd9d",
        f: SkillFuncImpl::Std(VaalBladeVortex_preDamageFunc),
    },
    FnRow {
        id: "data.skills.BladefallAltZ.preDamageFunc",
        hash: "52cad43550ec10e9b9a2108ca0e71eb5c44f50096eb28273e8cb5a3f9d208893",
        f: SkillFuncImpl::Std(BladefallAltZ_preDamageFunc),
    },
    FnRow {
        id: "data.skills.BlastRain.preDamageFunc",
        hash: "659342bba434fa868ca4478a8ef9e416bb275f9da00e180d681b210af698ca1b",
        f: SkillFuncImpl::Std(BlastRain_preDamageFunc),
    },
    FnRow {
        id: "data.skills.ChargedDash.preDamageFunc",
        hash: "cbca4be3fb48ac1298af025e78cbe71d9b1da8b29161ee6f4d756bee523e1cf3",
        f: SkillFuncImpl::Std(ChargedDash_preDamageFunc),
    },
    FnRow {
        id: "data.skills.ChargedDashAltX.preDamageFunc",
        hash: "4cfa3dd9c3ae1b1b8d266348465c2fbf8ccabc90fca8b3ad8d339b607a1af06e",
        f: SkillFuncImpl::Std(ChargedDashAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.Cremation.preDamageFunc",
        hash: "882fada5a9bfe6cb9fea48dbebacee399971e50d9abb7b1018570a1689744a52",
        f: SkillFuncImpl::Std(Cremation_preDamageFunc),
    },
    FnRow {
        id: "data.skills.CremationAltX.preDamageFunc",
        hash: "882fada5a9bfe6cb9fea48dbebacee399971e50d9abb7b1018570a1689744a52",
        f: SkillFuncImpl::Std(CremationAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.CremationAltY.preDamageFunc",
        hash: "bd9b0643b619e82acac920480884590d96cb2cdbb4df6e037e96d72f5e6bccfc",
        f: SkillFuncImpl::Std(CremationAltY_preDamageFunc),
    },
    FnRow {
        id: "data.skills.Cyclone.initialFunc",
        hash: "a17ea50f272107e6da0976d59b0338a9c02946a975073f5d1bf3dac7ee02727f",
        f: SkillFuncImpl::Std(Cyclone_initialFunc),
    },
    FnRow {
        id: "data.skills.CycloneAltX.initialFunc",
        hash: "41ca5d9b2ee830be0c3c078a5e0c456f366600e6cb5dcce235ab941836797fd3",
        f: SkillFuncImpl::Std(CycloneAltX_initialFunc),
    },
    FnRow {
        id: "data.skills.VaalCyclone.initialFunc",
        hash: "a17ea50f272107e6da0976d59b0338a9c02946a975073f5d1bf3dac7ee02727f",
        f: SkillFuncImpl::Std(VaalCyclone_initialFunc),
    },
    FnRow {
        id: "data.skills.ExplosiveArrow.explosiveArrowFunc",
        hash: "fcb10aac6ef439a9aeda38d6950f55f88ee90e682f5888b661c270c448f78105",
        f: SkillFuncImpl::ExplosiveArrow(ExplosiveArrow_explosiveArrowFunc),
    },
    FnRow {
        id: "data.skills.ExplosiveTrap.preDamageFunc",
        hash: "d3c4aeaf80b651612cead32b56b5483cc01d01b91446d8cd1fb324ead204a510",
        f: SkillFuncImpl::Std(ExplosiveTrap_preDamageFunc),
    },
    FnRow {
        id: "data.skills.FlamethrowerTrap.preDamageFunc",
        hash: "6cdc3e1700dfe5b0913fd558ea38bb7f67196eb5f76e944bbd6acc5b667d854e",
        f: SkillFuncImpl::Std(FlamethrowerTrap_preDamageFunc),
    },
    FnRow {
        id: "data.skills.FlamethrowerTrapAltX.preDamageFunc",
        hash: "5fba2e220c7694bd1b0e58ea4828866dbe8953c59861746ec69d13a68e266d95",
        f: SkillFuncImpl::Std(FlamethrowerTrapAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.LancingSteel.preDamageFunc",
        hash: "78ba4c2f5a3bc6abc7c60ced2ec7a1ef808108754657bbf0ace863c3a0cc87d2",
        f: SkillFuncImpl::Std(LancingSteel_preDamageFunc),
    },
    FnRow {
        id: "data.skills.LancingSteelAltX.preDamageFunc",
        hash: "224eb660840035750561b12ee3d7e1b4598b6cf3c992dd97dae3a67aee13f50b",
        f: SkillFuncImpl::Std(LancingSteelAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.VaalLightningArrow.preDamageFunc",
        hash: "659342bba434fa868ca4478a8ef9e416bb275f9da00e180d681b210af698ca1b",
        f: SkillFuncImpl::Std(VaalLightningArrow_preDamageFunc),
    },
    FnRow {
        id: "data.skills.StormRain.preDamageFunc",
        hash: "b2496d03fd13d5fb90bf078a953fe41c6913b0099fbd2b3def79e79b04ab5066",
        f: SkillFuncImpl::Std(StormRain_preDamageFunc),
    },
    FnRow {
        id: "data.skills.StormRainAltX.preDamageFunc",
        hash: "6e93f9cb7d7c788e8ac3129cbfbd179fde996179284eda25b965c5a6c6a76d98",
        f: SkillFuncImpl::Std(StormRainAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.StormRainAltY.preDamageFunc",
        hash: "c1234ad1dc0fa6652c350cf0b5c135a3642bc721047bc832b318b62505e30859",
        f: SkillFuncImpl::Std(StormRainAltY_preDamageFunc),
    },
    FnRow {
        id: "data.skills.ScourgeArrow.preDamageFunc",
        hash: "f15cf01aee5a4b61d1d80cffe7e621ee4a0d31b1d680b1e54bf7a2d91ea8fa16",
        f: SkillFuncImpl::Std(ScourgeArrow_preDamageFunc),
    },
    FnRow {
        id: "data.skills.SeismicTrap.preDamageFunc",
        hash: "bcab0d881fb82cd7cab01ece5aa881433d7694c83538ffb0c4af3399d14a8b6e",
        f: SkillFuncImpl::Std(SeismicTrap_preDamageFunc),
    },
    FnRow {
        id: "data.skills.SeismicTrapAltX.preDamageFunc",
        hash: "9a6bb655aa4aa81df0ceeaa8b090d8fa397c8bddff395a52765e38189680357a",
        f: SkillFuncImpl::Std(SeismicTrapAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.ShrapnelBallista.preDamageFunc",
        hash: "a5e0e6526ed2da2039a231401f2141ba83c32b20a773365c8e069c59945796a1",
        f: SkillFuncImpl::Std(ShrapnelBallista_preDamageFunc),
    },
    FnRow {
        id: "data.skills.ShrapnelBallistaAltX.preDamageFunc",
        hash: "a5e0e6526ed2da2039a231401f2141ba83c32b20a773365c8e069c59945796a1",
        f: SkillFuncImpl::Std(ShrapnelBallistaAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.ToxicRain.preDamageFunc",
        hash: "a607479cf006b5e38a533faf7759bf4c720bf6f0dcd325bbf27566aaedd92ca2",
        f: SkillFuncImpl::Std(ToxicRain_preDamageFunc),
    },
    FnRow {
        id: "data.skills.ToxicRainAltX.preDamageFunc",
        hash: "a607479cf006b5e38a533faf7759bf4c720bf6f0dcd325bbf27566aaedd92ca2",
        f: SkillFuncImpl::Std(ToxicRainAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.ToxicRainAltY.preDamageFunc",
        hash: "a607479cf006b5e38a533faf7759bf4c720bf6f0dcd325bbf27566aaedd92ca2",
        f: SkillFuncImpl::Std(ToxicRainAltY_preDamageFunc),
    },
    FnRow {
        id: "data.skills.TornadoShot.preDamageFunc",
        hash: "d13aed30ef6bb210c32448cd93f0332c18e4892227ad62bde08547881701e2a0",
        f: SkillFuncImpl::Std(TornadoShot_preDamageFunc),
    },
    FnRow {
        id: "data.skills.PoisonousConcoction.preDamageFunc",
        hash: "558a8faca69370fdd764591fcaa080a6035da9236776ff310c8c3ac990b7009e",
        f: SkillFuncImpl::Std(PoisonousConcoction_preDamageFunc),
    },
    FnRow {
        id: "data.skills.PoisonousConcoctionAltX.preDamageFunc",
        hash: "558a8faca69370fdd764591fcaa080a6035da9236776ff310c8c3ac990b7009e",
        f: SkillFuncImpl::Std(PoisonousConcoctionAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.Tornado.preDamageFunc",
        hash: "91c37fecdf57afb92b32b98c4ed67f684fcdc47584e97d79c02c5e259873c87b",
        f: SkillFuncImpl::Std(Tornado_preDamageFunc),
    },
    FnRow {
        id: "data.skills.TornadoAltY.preDamageFunc",
        hash: "62e81aaabd74e61697a16e66ec3c776e095d5b638e71b186bc42ba5b00b3eec2",
        f: SkillFuncImpl::Std(TornadoAltY_preDamageFunc),
    },
];

/// @pob Data/Skills/act_dex.lua:1026-1030 h=5843d6f7
/// `data.skills.Barrage.preDamageFunc` (preDamageFunc)
pub(crate) fn Barrage_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L1027
    if activeSkill.skillPart.get() == Some(2.0) {
        // L1028
        activeSkill
            .skillData
            .get()
            .set(n!("dpsMultiplier"), output.get(n!("ProjectileCount")));
    }
}

/// @pob Data/Skills/act_dex.lua:1114-1118 h=5843d6f7
/// `data.skills.BarrageAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn BarrageAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L1027
    if activeSkill.skillPart.get() == Some(2.0) {
        // L1028
        activeSkill
            .skillData
            .get()
            .set(n!("dpsMultiplier"), output.get(n!("ProjectileCount")));
    }
}

/// @pob Data/Skills/act_dex.lua:1374-1379 h=205f8789
/// `data.skills.BladeBlast.preDamageFunc` (preDamageFunc)
pub(crate) fn BladeBlast_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L1375
    let v = skillData.num_or(n!("dpsMultiplier"), 1.0) * skillData.num(n!("dpsBaseMultiplier"));
    skillData.set(n!("dpsMultiplier"), v);
    // L1376
    if activeSkill.skillPart.get() == Some(2.0) {
        // L1377
        skillData.set(n!("hitTimeOverride"), 1.0);
    }
}

/// @pob Data/Skills/act_dex.lua:1466-1471 h=205f8789
/// `data.skills.BladeBlastAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn BladeBlastAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L1375
    let v = skillData.num_or(n!("dpsMultiplier"), 1.0) * skillData.num(n!("dpsBaseMultiplier"));
    skillData.set(n!("dpsMultiplier"), v);
    // L1376
    if activeSkill.skillPart.get() == Some(2.0) {
        // L1377
        skillData.set(n!("hitTimeOverride"), 1.0);
    }
}

/// @pob Data/Skills/act_dex.lua:1906-1916 h=ec64b5fa
/// `data.skills.BladeFlurry.preDamageFunc` (preDamageFunc)
pub(crate) fn BladeFlurry_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L1907
    if activeSkill.skillPart.get() == Some(2.0) && skillData.num(n!("numStages")) > 0.0 {
        // L1908
        let numStages = skillData.num(n!("numStages"));
        // L1909
        let mut channelMulti = 0.0;
        // L1910
        let mut i = 1.0;
        while i <= numStages {
            // L1911
            channelMulti = channelMulti + (0.8 + (0.2 * i));
            i += 1.0;
        }
        // L1913
        channelMulti = channelMulti / (0.8 + (0.2 * numStages));
        // L1914
        skillData.set(n!("dpsMultiplier"), channelMulti / numStages + 1.0);
    }
}

/// @pob Data/Skills/act_dex.lua:2023-2033 h=ec64b5fa
/// `data.skills.BladeFlurryAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn BladeFlurryAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L1907
    if activeSkill.skillPart.get() == Some(2.0) && skillData.num(n!("numStages")) > 0.0 {
        // L1908
        let numStages = skillData.num(n!("numStages"));
        // L1909
        let mut channelMulti = 0.0;
        // L1910
        let mut i = 1.0;
        while i <= numStages {
            // L1911
            channelMulti = channelMulti + (0.8 + (0.2 * i));
            i += 1.0;
        }
        // L1913
        channelMulti = channelMulti / (0.8 + (0.2 * numStages));
        // L1914
        skillData.set(n!("dpsMultiplier"), channelMulti / numStages + 1.0);
    }
}

/// @pob Data/Skills/act_dex.lua:2209-2211 h=4b1cd804
/// `data.skills.BladeVortex.preDamageFunc` (preDamageFunc)
pub(crate) fn BladeVortex_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    let skillCfg = activeSkill.skillCfg.get();
    // L2210
    let v = skillData.num(n!("hitFrequency"))
        / (1.0
            + activeSkill.skillModList.get().Sum(
                ModType::BASE,
                Some(&*skillCfg),
                nm!["Multiplier:BladeVortexBlade"],
            ) * skillData.num(n!("hitFrequencyPerBlade")));
    skillData.set(n!("hitTimeOverride"), v);
}

/// @pob Data/Skills/act_dex.lua:2428-2430 h=3c07e211
/// `data.skills.VaalBladeVortex.preDamageFunc` (preDamageFunc)
pub(crate) fn VaalBladeVortex_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L2429
    let v = skillData.num(n!("hitFrequency"))
        / (1.0
            + skillData.num(n!("VaalBladeVortexBlade"))
                * skillData.num_or(n!("hitFrequencyPerBlade"), 0.0));
    skillData.set(n!("hitTimeOverride"), v);
}

/// @pob Data/Skills/act_dex.lua:2787-2789 h=256412e5
/// `data.skills.BladefallAltZ.preDamageFunc` (preDamageFunc)
pub(crate) fn BladefallAltZ_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L2788
    let v =
        skillData.num(n!("hitFrequency")) / (1.0 + skillData.num(n!("incVolleyFrequency")) / 100.0);
    skillData.set(n!("hitTimeOverride"), v);
}

/// @pob Data/Skills/act_dex.lua:2893-2897 h=5843d6f7
/// `data.skills.BlastRain.preDamageFunc` (preDamageFunc)
pub(crate) fn BlastRain_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L1027
    if activeSkill.skillPart.get() == Some(2.0) {
        // L1028
        activeSkill
            .skillData
            .get()
            .set(n!("dpsMultiplier"), output.get(n!("ProjectileCount")));
    }
}

/// @pob Data/Skills/act_dex.lua:3939-3944 h=346390c3
/// `data.skills.ChargedDash.preDamageFunc` (preDamageFunc)
pub(crate) fn ChargedDash_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L3940
    if activeSkill.skillPart.get() == Some(3.0) {
        let skillModList = activeSkill.skillModList.get();
        let skillCfg = activeSkill.skillCfg.get();
        // L3941
        let finalWaveDamageModifier = skillModList.Sum(
            ModType::INC,
            Some(&*skillCfg),
            nm!["chargedDashFinalDamageModifier"],
        );
        // L3942
        skillModList.NewMod(
            n!("Damage"),
            ModType::MORE,
            finalWaveDamageModifier,
            "Skill:ChargedDash",
            ModFlag::Attack,
            0,
            &[release_damage_tag()],
        );
    }
}

/// @pob Data/Skills/act_dex.lua:4064-4069 h=b19041c5
/// `data.skills.ChargedDashAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn ChargedDashAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L4065
    if activeSkill.skillPart.get() == Some(3.0) {
        let skillModList = activeSkill.skillModList.get();
        let skillCfg = activeSkill.skillCfg.get();
        // L4066
        let finalWaveDamageModifier = skillModList.Sum(
            ModType::INC,
            Some(&*skillCfg),
            nm!["chargedDashFinalDamageModifier"],
        );
        // L4067
        skillModList.NewMod(
            n!("Damage"),
            ModType::MORE,
            finalWaveDamageModifier,
            "Skill:ChargedDashAltX",
            ModFlag::Attack,
            0,
            &[release_damage_tag()],
        );
    }
}

/// @pob Data/Skills/act_dex.lua:4355-4359 h=23b2c0f6
/// `data.skills.Cremation.preDamageFunc` (preDamageFunc)
pub(crate) fn Cremation_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L4356
    if activeSkill.skillPart.get() == Some(1.0) {
        let skillData = activeSkill.skillData.get();
        // L4357
        let v = skillData.num(n!("cremationFireRate"))
            / (1.0 + skillData.num_or(n!("cremationFireRateIncrease"), 0.0));
        skillData.set(n!("hitTimeOverride"), v);
    }
}

/// @pob Data/Skills/act_dex.lua:4473-4477 h=23b2c0f6
/// `data.skills.CremationAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn CremationAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L4356
    if activeSkill.skillPart.get() == Some(1.0) {
        let skillData = activeSkill.skillData.get();
        // L4357
        let v = skillData.num(n!("cremationFireRate"))
            / (1.0 + skillData.num_or(n!("cremationFireRateIncrease"), 0.0));
        skillData.set(n!("hitTimeOverride"), v);
    }
}

/// @pob Data/Skills/act_dex.lua:4592-4594 h=01934563
/// `data.skills.CremationAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn CremationAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L4593
    let v = skillData.num(n!("cremationFireRate"))
        / (1.0 + skillData.num_or(n!("cremationFireRateIncrease"), 0.0));
    skillData.set(n!("hitTimeOverride"), v);
}

/// @pob Data/Skills/act_dex.lua:4703-4716 h=2a8770a4
/// `data.skills.Cyclone.initialFunc` (initialFunc)
pub(crate) fn Cyclone_initialFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillModList = activeSkill.skillModList.get();
    let skillFlags = activeSkill.skillFlags.get();
    let actor = actor_of(activeSkill);
    // L4704
    let mut range = 0.0;
    // L4705
    if skillFlags.truthy(n!("weapon1Attack")) && actor.weaponData1.get().truthy(n!("range")) {
        let weapon1Cfg = activeSkill.weapon1Cfg.get();
        // L4706
        let weapon1RangeBonus = skillModList.Sum(
            ModType::BASE,
            weapon1Cfg.as_deref(),
            nm!["MeleeWeaponRange"],
        ) + 10.0
            * skillModList.Sum(
                ModType::BASE,
                weapon1Cfg.as_deref(),
                nm!["MeleeWeaponRangeMetre"],
            )
            + actor.weaponData1.get().num(n!("rangeBonus"));
        // L4707
        if skillFlags.truthy(n!("weapon2Attack")) && actor.weaponData2.get().truthy(n!("range")) {
            // dual wield average
            let weapon2Cfg = activeSkill.weapon2Cfg.get();
            // L4708
            range = (weapon1RangeBonus
                + skillModList.Sum(
                    ModType::BASE,
                    weapon2Cfg.as_deref(),
                    nm!["MeleeWeaponRange"],
                )
                + 10.0
                    * skillModList.Sum(
                        ModType::BASE,
                        weapon2Cfg.as_deref(),
                        nm!["MeleeWeaponRangeMetre"],
                    )
                + actor.weaponData2.get().num(n!("rangeBonus")))
                / 2.0;
        } else {
            // primary hand attack
            // L4710
            range = weapon1RangeBonus;
        }
    } else {
        // unarmed
        let skillCfg = activeSkill.skillCfg.get();
        // L4713
        range = skillModList.Sum(ModType::BASE, Some(&*skillCfg), nm!["UnarmedRange"])
            + 10.0 * skillModList.Sum(ModType::BASE, Some(&*skillCfg), nm!["UnarmedRangeMetre"]);
    }
    // L4715
    skillModList.NewMod(
        n!("Multiplier:AdditionalMeleeRange"),
        ModType::BASE,
        range,
        "Skill:Cyclone",
        0,
        0,
        &[],
    );
}

/// @pob Data/Skills/act_dex.lua:4809-4822 h=43245e25
/// `data.skills.CycloneAltX.initialFunc` (initialFunc)
pub(crate) fn CycloneAltX_initialFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillModList = activeSkill.skillModList.get();
    let skillFlags = activeSkill.skillFlags.get();
    let actor = actor_of(activeSkill);
    // L4810
    let mut range = 0.0;
    // L4811
    if skillFlags.truthy(n!("weapon1Attack")) && actor.weaponData1.get().truthy(n!("range")) {
        let weapon1Cfg = activeSkill.weapon1Cfg.get();
        // L4812
        let weapon1RangeBonus = skillModList.Sum(
            ModType::BASE,
            weapon1Cfg.as_deref(),
            nm!["MeleeWeaponRange"],
        ) + 10.0
            * skillModList.Sum(
                ModType::BASE,
                weapon1Cfg.as_deref(),
                nm!["MeleeWeaponRangeMetre"],
            )
            + actor.weaponData1.get().num(n!("rangeBonus"));
        // L4813
        if skillFlags.truthy(n!("weapon2Attack")) && actor.weaponData2.get().truthy(n!("range")) {
            // dual wield average
            let weapon2Cfg = activeSkill.weapon2Cfg.get();
            // L4814
            range = (weapon1RangeBonus
                + skillModList.Sum(
                    ModType::BASE,
                    weapon2Cfg.as_deref(),
                    nm!["MeleeWeaponRange"],
                )
                + 10.0
                    * skillModList.Sum(
                        ModType::BASE,
                        weapon2Cfg.as_deref(),
                        nm!["MeleeWeaponRangeMetre"],
                    )
                + actor.weaponData2.get().num(n!("rangeBonus")))
                / 2.0;
        } else {
            // primary hand attack
            // L4816
            range = weapon1RangeBonus;
        }
    } else {
        // unarmed
        let skillCfg = activeSkill.skillCfg.get();
        // L4819
        range = skillModList.Sum(ModType::BASE, Some(&*skillCfg), nm!["UnarmedRange"])
            + 10.0 * skillModList.Sum(ModType::BASE, Some(&*skillCfg), nm!["UnarmedRangeMetre"]);
    }
    // L4821
    skillModList.NewMod(
        n!("Multiplier:AdditionalMeleeRange"),
        ModType::BASE,
        range,
        "Skill:CycloneAltX",
        0,
        0,
        &[],
    );
}

/// @pob Data/Skills/act_dex.lua:4934-4947 h=2a8770a4
/// `data.skills.VaalCyclone.initialFunc` (initialFunc)
pub(crate) fn VaalCyclone_initialFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillModList = activeSkill.skillModList.get();
    let skillFlags = activeSkill.skillFlags.get();
    let actor = actor_of(activeSkill);
    // L4935
    let mut range = 0.0;
    // L4936
    if skillFlags.truthy(n!("weapon1Attack")) && actor.weaponData1.get().truthy(n!("range")) {
        let weapon1Cfg = activeSkill.weapon1Cfg.get();
        // L4937
        let weapon1RangeBonus = skillModList.Sum(
            ModType::BASE,
            weapon1Cfg.as_deref(),
            nm!["MeleeWeaponRange"],
        ) + 10.0
            * skillModList.Sum(
                ModType::BASE,
                weapon1Cfg.as_deref(),
                nm!["MeleeWeaponRangeMetre"],
            )
            + actor.weaponData1.get().num(n!("rangeBonus"));
        // L4938
        if skillFlags.truthy(n!("weapon2Attack")) && actor.weaponData2.get().truthy(n!("range")) {
            // dual wield average
            let weapon2Cfg = activeSkill.weapon2Cfg.get();
            // L4939
            range = (weapon1RangeBonus
                + skillModList.Sum(
                    ModType::BASE,
                    weapon2Cfg.as_deref(),
                    nm!["MeleeWeaponRange"],
                )
                + 10.0
                    * skillModList.Sum(
                        ModType::BASE,
                        weapon2Cfg.as_deref(),
                        nm!["MeleeWeaponRangeMetre"],
                    )
                + actor.weaponData2.get().num(n!("rangeBonus")))
                / 2.0;
        } else {
            // primary hand attack
            // L4941
            range = weapon1RangeBonus;
        }
    } else {
        // unarmed
        let skillCfg = activeSkill.skillCfg.get();
        // L4944
        range = skillModList.Sum(ModType::BASE, Some(&*skillCfg), nm!["UnarmedRange"])
            + 10.0 * skillModList.Sum(ModType::BASE, Some(&*skillCfg), nm!["UnarmedRangeMetre"]);
    }
    // L4946
    skillModList.NewMod(
        n!("Multiplier:AdditionalMeleeRange"),
        ModType::BASE,
        range,
        "Skill:Cyclone",
        0,
        0,
        &[],
    );
}

/// @pob Data/Skills/act_dex.lua:6699-6778 h=c70d10ad
/// `data.skills.ExplosiveArrow.explosiveArrowFunc` (explosiveArrowFunc)
pub(crate) fn ExplosiveArrow_explosiveArrowFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    globalOutput: &Output,
    globalBreakdown: Option<&Breakdown>,
    env: &EnvRef,
) {
    // L6700-6701 (t_insert, s_format)

    // L6703
    let skillPart = activeSkill.skillPart.get();
    if skillPart != Some(1.0) && skillPart != Some(2.0) {
        // This doesn't apply to the "Arrow" skill part. That works like a normal skill.
        // L6705
        return;
    }

    // L6708
    let modDB = env.modDB.get();
    // L6709
    let enemyDB = actor_of(activeSkill)
        .enemy()
        .unwrap_or_else(|| lua_error!("attempt to index field 'enemy' (a nil value)"))
        .modDB
        .get();
    // L6710
    let skillModList = activeSkill.skillModList.get();
    let skillCfg = activeSkill.skillCfg.get();
    let skillData = activeSkill.skillData.get();
    let skillFlags = activeSkill.skillFlags.get();
    // L6711
    let duration = calcSkillDuration(&skillModList, Some(&*skillCfg), &skillData, env, &enemyDB);
    // L6712
    let fuseLimit = skillModList.Sum(
        ModType::BASE,
        Some(&*skillCfg),
        nm!["ExplosiveArrowMaxFuseCount"],
    );
    // L6713
    let mut activeTotems = Val::Nil;
    // L6714
    if skillFlags.truthy(n!("totem")) {
        // L6715
        activeTotems = lor(
            Val::from(modDB.Override(None, nm!["TotemsSummoned"])),
            || {
                Val::from(skillModList.Sum(
                    ModType::BASE,
                    Some(&*skillCfg),
                    nm!["ActiveTotemLimit", "ActiveBallistaLimit"],
                ))
            },
        );
    }

    // L6718
    let mut barrageProjectiles: Option<f64> = None;
    // L6719
    if skillModList.Flag(None, nm!["SequentialProjectiles"])
        && !skillModList.Flag(None, nm!["OneShotProj"])
        && !skillModList.Flag(None, nm!["NoAdditionalProjectiles"])
        && !skillModList.Flag(None, nm!["TriggeredBySnipe"])
    {
        // L6720
        let bp = skillModList.Sum(ModType::BASE, Some(&*skillCfg), nm!["ProjectileCount"]);
        barrageProjectiles = Some(bp);
        // L6721
        // cancel out the normal dps multiplier from barrage that applies to most other skills
        let v = skillData.num(n!("dpsMultiplier")) / bp;
        skillData.set(n!("dpsMultiplier"), v);
    }

    // L6724
    let mut fuseApplicationRate = (output.num(n!("HitChance")) / 100.0)
        * globalOutput.num(n!("Speed"))
        * skillData.num(n!("dpsMultiplier"))
        * barrageProjectiles.unwrap_or(1.0);
    // L6725
    let initialApplicationRate = fuseApplicationRate;
    // L6726
    if skillFlags.truthy(n!("totem")) {
        // L6727
        fuseApplicationRate = fuseApplicationRate * activeTotems.num();
    }

    // Calculate the max number of fuses you can sustain
    // Does not take into account mines or traps
    // L6732
    if skillPart == Some(2.0) {
        // L6733
        let maximum = m_min(m_floor(fuseApplicationRate * duration) + 1.0, fuseLimit);
        // L6734
        skillModList.NewMod(
            n!("Multiplier:ExplosiveArrowStage"),
            ModType::BASE,
            maximum,
            "Base",
            0,
            0,
            &[],
        );
        // L6735
        skillModList.NewMod(
            n!("Multiplier:ExplosiveArrowStageAfterFirst"),
            ModType::BASE,
            maximum - 1.0,
            "Base",
            0,
            0,
            &[],
        );
        // L6736
        globalOutput.set(n!("MaxExplosiveArrowFuseCalculated"), maximum);
    } else {
        // L6738
        globalOutput.set(n!("MaxExplosiveArrowFuseCalculated"), Val::Nil);
    }

    // Calculate explosion rate
    // L6742
    let timeToMaxFuses = fuseLimit / fuseApplicationRate;
    // L6743
    if skillPart == Some(2.0)
        || (skillPart == Some(1.0)
            && activeSkill.activeStageCount.get().unwrap_or(0.0) + 1.0 >= fuseLimit)
    {
        // L6744
        globalOutput.set(n!("HitTime"), m_min(duration, timeToMaxFuses));
    } else {
        // Number of fuses is less than the limit, so the entire fuse duration applies
        // L6747
        globalOutput.set(n!("HitTime"), duration);
    }

    // L6750
    let v = 1.0 / globalOutput.num(n!("HitTime"));
    globalOutput.set(n!("HitSpeed"), v);

    // L6752
    if let Some(globalBreakdown) = globalBreakdown {
        if globalOutput.truthy(n!("MaxExplosiveArrowFuseCalculated")) {
            // L6753
            let fuse = Table::new();
            globalBreakdown.set(n!("MaxExplosiveArrowFuseCalculated"), fuse.clone());
            // L6754
            fuse.push(lfmt!(
                "%.2f ^8(attack speed)",
                globalOutput.num(n!("Speed"))
            ));
            // L6755
            if output.num(n!("HitChance")) < 100.0 {
                // L6756
                fuse.push(lfmt!(
                    "x %.2f ^8(hit chance)",
                    output.num(n!("HitChance")) / 100.0
                ));
            }
            // L6758
            fuse.push(lfmt!(
                "x %.2f ^8(projectiles)",
                barrageProjectiles.unwrap_or(1.0)
            ));
            // L6759
            if skillFlags.truthy(n!("totem")) {
                // L6760
                fuse.push(lfmt!("= %.2f ^8(fuse rate)", initialApplicationRate));
                // L6761
                fuse.push(lfmt!("x %d ^8(active totems)", activeTotems.num()));
                // L6762
                fuse.push(lfmt!("= %.2f ^8(fuse rate)", fuseApplicationRate));
            } else {
                // L6764
                fuse.push(lfmt!("= %.2f ^8(fuse rate)", fuseApplicationRate));
            }
            // L6766
            fuse.push(lfmt!("x %.2f ^8(duration)", duration));
            // L6767
            fuse.push(lfmt!("+ 1 ^8(initial hit)"));
            // L6768
            fuse.push(lfmt!("= %.2f", (fuseApplicationRate * duration) + 1.0));
            // L6769
            fuse.push(lfmt!(
                "= %d ^8(rounded down, capped at max)",
                globalOutput.num(n!("MaxExplosiveArrowFuseCalculated"))
            ));

            // L6771
            let eps = Table::new();
            globalBreakdown.set(n!("ExplosionsPerSecond"), eps.clone());
            // L6772
            eps.push(lfmt!("1 ^8(second)"));
            // L6773
            eps.push(lfmt!(
                " / %d ^8(max fuses)",
                globalOutput.num(n!("MaxExplosiveArrowFuseCalculated"))
            ));
            // L6774
            eps.push(lfmt!(" / %.2f ^8(fuse rate)", fuseApplicationRate));
            // L6775
            eps.push(lfmt!(
                "= %.2f ^8(explosions/s)",
                globalOutput.num(n!("HitSpeed"))
            ));
        }
    }
}

/// @pob Data/Skills/act_dex.lua:7156-7221 h=bd9f4379
/// `data.skills.ExplosiveTrap.preDamageFunc` (preDamageFunc) (also: data.skills.ExplosiveTrapAltX.preDamageFunc)
pub(crate) fn ExplosiveTrap_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L7157
    let skillCfg = activeSkill.skillCfg.get();
    // L7158
    let skillData = activeSkill.skillData.get();
    // L7159
    let skillPart = activeSkill.skillPart.get();
    // L7160
    let skillModList = activeSkill.skillModList.get();
    // L7161-7162 (t_insert, s_format)

    // L7164-7168: `hitChance` is the file-level helper `hitChance`
    // L7169
    let enemyRadius = override_or_sum(&skillModList, &skillCfg, n!("EnemyRadius"));
    // L7170
    let fullRadius = output.num(n!("AreaOfEffectRadiusSecondary"));
    // L7171
    let mut overlapChance = 0.0;
    // L7172
    let marginWidth = skillData.num(n!("radiusTertiaryBaseMargin")) * 2.0 + 1.0;
    // L7173
    let occurrences = output
        .table(n!("AreaOfEffectRadiusTertiaryOccurrences"))
        .unwrap_or_else(|| lua_error!("bad argument #1 to 'pairs' (table expected, got nil)"));
    // LUA-ORDER: integer radius keys (pairs_inventory act_dex.lua:7173); the sum only depends
    // on the order through float rounding. The table is walked in its Rust order (OF-A builds
    // it, CalcOffence.lua:442).
    for (smallRadius, occurrenceCount) in occurrences.pairs() {
        // L7174
        overlapChance = overlapChance
            + hitChance(enemyRadius, key_num(smallRadius), fullRadius) * occurrenceCount.num()
                / marginWidth;
    }
    // L7176
    output.set(n!("OverlapChance"), overlapChance * 100.0);
    // L7177
    let smallExplosionsPerTrap =
        skillModList.Sum(ModType::BASE, Some(&*skillCfg), nm!["SmallExplosions"]);
    // L7178
    output.set(n!("SmallExplosionsPerTrap"), smallExplosionsPerTrap);
    // L7179
    if let Some(breakdown) = breakdown {
        // L7180
        let oc = Table::new();
        breakdown.set(n!("OverlapChance"), oc.clone());
        // L7181
        oc.push("Chance for individual small explosion to land within range to damage enemy:");
        // L7182
        oc.push("^8= (area where a small explosion can spawn to damage enemy) / (total area)");
        // L7183
        oc.push("^8= (^7tertiary radius^8 + ^7enemy radius^8 - 1) ^ 2 / ^7secondary radius^8 ^ 2");
        // L7184
        oc.push(
            "^8Result is the weighted sum of overlap chances for each possible tertiary radius",
        );
        // L7185
        let mut radii: Vec<f64> = Vec::new();
        // L7186
        let mut numRadii = 0usize;
        // L7187
        // LUA-ORDER: integer keys (pairs_inventory act_dex.lua:7187); sorted below.
        for (radius, _) in occurrences.pairs() {
            // L7188
            radii.push(key_num(radius));
            // L7189
            numRadii = numRadii + 1;
        }
        // L7191
        pob_lua::sort::sort(&mut radii, |a, b| a < b);
        // L7192
        for_ipairs!((i, smallRadius) in radii => {
            // L7193-7195
            let line = lfmt!(
                "^8(^7%d^8 +^7 %d^8 - 1) ^ 2 /^7 %d^8 ^ 2 *^7 %d/%d%s",
                smallRadius,
                enemyRadius,
                fullRadius,
                occurrences.num(smallRadius),
                marginWidth,
                if i == numRadii { "" } else { " ^8+" }
            );
            oc.push(line);
        });
        // L7197
        oc.push(lfmt!("^8=^7 %.3f^8%%", output.num(n!("OverlapChance"))));
    }
    // L7199
    let mut dpsMultiplier = 1.0;
    // L7200
    if skillPart == Some(2.0) {
        // L7201
        dpsMultiplier = 1.0 + smallExplosionsPerTrap * overlapChance;
        // L7202
        if let Some(breakdown) = breakdown {
            // L7203
            let sm = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), sm.clone());
            // L7204
            sm.push("DPS multiplier");
            // L7205
            sm.push("^8= 1 + ^7small explosions^8 * ^7overlap chance^8");
            // L7206
            sm.push(lfmt!(
                "^8= 1 +^7 %d^8 *^7 %.2f^8",
                smallExplosionsPerTrap,
                overlapChance
            ));
            // L7207
            sm.push(lfmt!("^8=^7 %.3f", dpsMultiplier));
        }
    } else if skillPart == Some(3.0) {
        // L7210
        dpsMultiplier = 1.0 + smallExplosionsPerTrap;
        // L7211
        if let Some(breakdown) = breakdown {
            // L7212
            let sm = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), sm.clone());
            // L7213
            sm.push("DPS multiplier");
            // L7214
            sm.push(lfmt!("^8= 1 +^7 %d (small explosions)", dpsMultiplier));
        }
    }
    // L7217
    if dpsMultiplier != 1.0 {
        // L7218
        let v = skillData.num_or(n!("dpsMultiplier"), 1.0) * dpsMultiplier;
        skillData.set(n!("dpsMultiplier"), v);
        // L7219
        let v = output.num_or(n!("SkillDPSMultiplier"), 1.0) * dpsMultiplier;
        output.set(n!("SkillDPSMultiplier"), v);
    }
}

/// @pob Data/Skills/act_dex.lua:7691-7717 h=2a19e003
/// `data.skills.FlamethrowerTrap.preDamageFunc` (preDamageFunc)
pub(crate) fn FlamethrowerTrap_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // Unknown stats provided by asking GGG
    // L7693-7694 (t_insert, s_format)

    let skillData = activeSkill.skillData.get();
    let skillPart = activeSkill.skillPart.get();
    // L7696
    let duration = output.num(n!("Duration"));
    // L7697
    let cooldown = output.num(n!("TrapCooldown"));
    // L7698
    let averageActiveTraps = duration / cooldown;
    // L7699
    output.set(n!("AverageActiveTraps"), averageActiveTraps);
    // L7700
    if skillPart == Some(2.0) || skillPart == Some(4.0) {
        // L7701
        skillData.set(n!("hitTimeOverride"), 0.3);
    } else {
        // L7703
        skillData.set(n!("hitTimeOverride"), 0.1);
    }

    // L7706
    if skillPart == Some(3.0) || skillPart == Some(4.0) {
        // L7707
        let v = skillData.num_or(n!("dpsMultiplier"), 1.0) * averageActiveTraps;
        skillData.set(n!("dpsMultiplier"), v);
    }

    // L7710
    if let Some(breakdown) = breakdown {
        // L7711
        let at = Table::new();
        breakdown.set(n!("AverageActiveTraps"), at.clone());
        // L7712
        at.push("Average active traps, not considering stored cooldown uses:");
        // L7713
        at.push(lfmt!("%.2f^8 (skill duration)", duration));
        // L7714
        at.push(lfmt!("/ %.2f^8 (cooldown)", cooldown));
        // L7715
        at.push(lfmt!("= %.2f traps", averageActiveTraps));
    }
}

/// @pob Data/Skills/act_dex.lua:7809-7827 h=db767574
/// `data.skills.FlamethrowerTrapAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn FlamethrowerTrapAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L7810-7811 (t_insert, s_format)

    let skillData = activeSkill.skillData.get();
    // L7813
    let duration = output.num(n!("Duration"));
    // L7814
    let cooldown = output.num(n!("TrapCooldown"));
    // L7815
    let averageActiveTraps = duration / cooldown;
    // L7816
    output.set(n!("AverageActiveTraps"), averageActiveTraps);
    // L7817
    skillData.set(n!("hitTimeOverride"), 0.1);
    // L7818
    let v = skillData.num_or(n!("dpsMultiplier"), 1.0) * averageActiveTraps;
    skillData.set(n!("dpsMultiplier"), v);

    // L7820
    if let Some(breakdown) = breakdown {
        // L7821
        let at = Table::new();
        breakdown.set(n!("AverageActiveTraps"), at.clone());
        // L7822
        at.push("Average active traps, not considering stored cooldown uses:");
        // L7823
        at.push(lfmt!("%.2f^8 (skill duration)", duration));
        // L7824
        at.push(lfmt!("/ %.2f^8 (cooldown)", cooldown));
        // L7825
        at.push(lfmt!("= %.2f traps", averageActiveTraps));
    }
}

/// @pob Data/Skills/act_dex.lua:10234-10241 h=e693cb8c
/// `data.skills.LancingSteel.preDamageFunc` (preDamageFunc)
pub(crate) fn LancingSteel_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L10235
    if activeSkill.skillPart.get() == Some(2.0) {
        let skillModList = activeSkill.skillModList.get();
        let skillCfg = activeSkill.skillCfg.get();
        // L10236
        let percentReducedProjectiles =
            (output.num(n!("ProjectileCount")) - 1.0) / output.num(n!("ProjectileCount"));
        // L10237
        let mult = (skillModList.More(Some(&*skillCfg), nm!["LancingSteelSubsequentDamage"]) - 1.0)
            * 100.0
            * percentReducedProjectiles;
        // L10238
        activeSkill
            .skillData
            .get()
            .set(n!("dpsMultiplier"), output.get(n!("ProjectileCount")));
        // L10239
        skillModList.NewMod(
            n!("Damage"),
            ModType::MORE,
            mult,
            "Skill:LancingSteel",
            0,
            0,
            &[],
        );
    }
}

/// @pob Data/Skills/act_dex.lua:10337-10344 h=659b0081
/// `data.skills.LancingSteelAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn LancingSteelAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L10338
    if activeSkill.skillPart.get() == Some(2.0) {
        let skillModList = activeSkill.skillModList.get();
        let skillCfg = activeSkill.skillCfg.get();
        // L10339
        let percentReducedProjectiles =
            (output.num(n!("ProjectileCount")) - 1.0) / output.num(n!("ProjectileCount"));
        // L10340
        let mult = (skillModList.More(Some(&*skillCfg), nm!["LancingSteelSubsequentDamage"]) - 1.0)
            * 100.0
            * percentReducedProjectiles;
        // L10341
        activeSkill
            .skillData
            .get()
            .set(n!("dpsMultiplier"), output.get(n!("ProjectileCount")));
        // L10342
        skillModList.NewMod(
            n!("Damage"),
            ModType::MORE,
            mult,
            "Skill:LancingSteelAltX",
            0,
            0,
            &[],
        );
    }
}

/// @pob Data/Skills/act_dex.lua:10590-10594 h=5843d6f7
/// `data.skills.VaalLightningArrow.preDamageFunc` (preDamageFunc)
pub(crate) fn VaalLightningArrow_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L1027
    if activeSkill.skillPart.get() == Some(2.0) {
        // L1028
        activeSkill
            .skillData
            .get()
            .set(n!("dpsMultiplier"), output.get(n!("ProjectileCount")));
    }
}

/// @pob Data/Skills/act_dex.lua:11747-11752 h=a9e26965
/// `data.skills.StormRain.preDamageFunc` (preDamageFunc)
pub(crate) fn StormRain_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L11748
    if activeSkill.skillPart.get() == Some(2.0) {
        let skillData = activeSkill.skillData.get();
        let skillCfg = activeSkill.skillCfg.get();
        let skillModList = activeSkill.skillModList.get();
        // L11749
        let v = skillData.num(n!("hitFrequency"))
            / (1.0
                + skillModList.Sum(
                    ModType::INC,
                    Some(&*skillCfg),
                    nm!["StormRainBeamFrequency"],
                ) / 100.0);
        skillData.set(n!("hitTimeOverride"), v);
        // L11750
        let v = lor(skillData.get(n!("beamOverlapMultiplier")), || {
            Val::from(1.0)
        });
        skillData.set(n!("dpsMultiplier"), v);
    }
}

/// @pob Data/Skills/act_dex.lua:11851-11856 h=00a16615
/// `data.skills.StormRainAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn StormRainAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L11852
    if activeSkill.skillPart.get() == Some(2.0) {
        let skillData = activeSkill.skillData.get();
        let skillCfg = activeSkill.skillCfg.get();
        let skillModList = activeSkill.skillModList.get();
        // L11853
        let v = skillData.num(n!("hitFrequency"))
            / (1.0
                + skillModList.Sum(
                    ModType::INC,
                    Some(&*skillCfg),
                    nm!["StormRainBeamFrequency"],
                ) / 100.0);
        skillData.set(n!("hitTimeOverride"), v);
        // L11854
        let v = m_min(
            skillData.num_or(n!("activeArrowMultiplier"), 1.0),
            skillModList.Sum(
                ModType::BASE,
                Some(&*skillCfg),
                nm!["StormRainAllowedStormArrows"],
            ),
        );
        skillData.set(n!("dpsMultiplier"), v);
    }
}

/// @pob Data/Skills/act_dex.lua:11958-11964 h=0fc060cb
/// `data.skills.StormRainAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn StormRainAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L11959
    if activeSkill.skillPart.get() == Some(2.0) {
        let skillData = activeSkill.skillData.get();
        let skillCfg = activeSkill.skillCfg.get();
        let skillModList = activeSkill.skillModList.get();
        // L11960
        let v = skillData.num(n!("hitFrequency"))
            / (1.0
                + skillModList.Sum(
                    ModType::INC,
                    Some(&*skillCfg),
                    nm!["StormRainBeamFrequency"],
                ) / 100.0);
        skillData.set(n!("hitTimeOverride"), v);
        // Max of 2 arrows, and each fires at each other, so 2 beams per tick
        // L11962
        let v = skillModList.Sum(
            ModType::BASE,
            Some(&*skillCfg),
            nm!["StormRainAllowedStormArrows"],
        );
        skillData.set(n!("dpsMultiplier"), v);
    }
}

/// @pob Data/Skills/act_dex.lua:12990-12992 h=2da665b7
/// `data.skills.ScourgeArrow.preDamageFunc` (preDamageFunc)
pub(crate) fn ScourgeArrow_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L12991
    // LUA-GLOBAL-NIL: `cfg` is an undeclared global here (calc doc §7.3), so the cfg is nil.
    //First stage takes 0.5x time to channel compared to subsequent stages
    let v = m_max(
        activeSkill.skillModList.get().Sum(
            ModType::BASE,
            None,
            nm!["Multiplier:ScourgeArrowStage"],
        ) - 0.5,
        0.5,
    );
    activeSkill.skillData.get().set(n!("hitTimeMultiplier"), v);
}

/// @pob Data/Skills/act_dex.lua:13381-13496 h=fa977e88
/// `data.skills.SeismicTrap.preDamageFunc` (preDamageFunc)
pub(crate) fn SeismicTrap_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L13382
    let skillCfg = activeSkill.skillCfg.get();
    // L13383
    let skillData = activeSkill.skillData.get();
    // L13384
    let skillPart = activeSkill.skillPart.get();
    // L13385
    let skillModList = activeSkill.skillModList.get();
    // L13386-13387 (t_insert, s_format)

    // L13389
    let baseInterval = skillData.num(n!("repeatInterval"));
    // L13390
    let incFrequency = 1.0
        + skillModList.Sum(
            ModType::INC,
            Some(&*skillCfg),
            nm!["TrapThrowingSpeed", "SeismicPulseFrequency"],
        ) / 100.0;
    // L13391
    let moreFrequency = skillModList.More(
        Some(&*skillCfg),
        nm!["TrapThrowingSpeed", "SeismicPulseFrequency"],
    );
    // L13392
    let wavePulseRate = incFrequency * moreFrequency / baseInterval;
    // L13393
    skillData.set(n!("hitTimeOverride"), 1.0 / wavePulseRate);
    // L13394
    output.set(n!("WavePulseRate"), wavePulseRate);
    // L13395
    let incDuration =
        1.0 + skillModList.Sum(ModType::INC, Some(&*skillCfg), nm!["Duration"]) / 100.0;
    // L13396
    let moreDuration = skillModList.More(Some(&*skillCfg), nm!["Duration"]);
    // L13397
    let duration = skillData.num(n!("duration")) * incDuration * moreDuration;
    // L13398
    let pulses = m_floor(duration * wavePulseRate);
    // L13399
    output.set(n!("PulsesPerTrap"), pulses);
    // L13400
    let effectiveDuration = pulses / wavePulseRate;
    // L13401
    let cooldown = output.num(n!("TrapCooldown"));
    // L13402
    let averageActiveTraps = effectiveDuration / cooldown;
    // L13403
    output.set(n!("AverageActiveTraps"), averageActiveTraps);
    // L13404-13408: `hitChance` is the file-level helper `hitChance`
    // L13409
    let enemyRadius = override_or_sum(&skillModList, &skillCfg, n!("EnemyRadius"));
    // L13410
    let waveRadius = output.num(n!("AreaOfEffectRadiusSecondary"));
    // L13411
    let fullRadius = output.num(n!("AreaOfEffectRadius"));
    // L13412
    let overlapChance = hitChance(enemyRadius, waveRadius, fullRadius);
    // L13413
    output.set(n!("OverlapChance"), overlapChance * 100.0);
    // L13414
    if let Some(breakdown) = breakdown {
        // L13415
        let oc = Table::new();
        breakdown.set(n!("OverlapChance"), oc.clone());
        // L13416
        oc.push("Chance for individual wave to land within range to damage enemy:");
        // L13417
        oc.push("^8= (area where wave can spawn to damage enemy) / (total area)");
        // L13418
        oc.push("^8= (^7secondary radius^8 + ^7enemy radius^8 - 1) ^ 2 / ^7radius^8 ^ 2");
        // L13419
        oc.push(lfmt!(
            "^8= (^7%d^8 +^7 %d^8 - 1) ^ 2 /^7 %d^8 ^ 2",
            waveRadius,
            enemyRadius,
            fullRadius
        ));
        // L13420
        oc.push(lfmt!("^8=^7 %.3f^8%%", overlapChance * 100.0));
        // L13421
        let wp = Table::new();
        breakdown.set(n!("WavePulseRate"), wp.clone());
        // L13422
        wp.push("Pulse rate:");
        // L13423
        wp.push(lfmt!("%.2f ^8(base pulse rate)", 1.0 / baseInterval));
        // L13424
        wp.push(lfmt!(
            "* %.2f ^8(increased/reduced pulse frequency)",
            incFrequency
        ));
        // L13425
        wp.push(lfmt!("* %.2f ^8(more/less pulse frequency)", moreFrequency));
        // L13426
        wp.push(lfmt!("= %.2f^8/s", wavePulseRate));
        // L13427
        let pp = Table::new();
        breakdown.set(n!("PulsesPerTrap"), pp.clone());
        // L13428
        pp.push("Pulses per trap:");
        // L13429
        pp.push(lfmt!("%.3f ^8(unrounded skill duration)", duration));
        // L13430
        pp.push(lfmt!("* %.2f ^8(pulse rate)", wavePulseRate));
        // L13431
        pp.push(lfmt!("= %.2f ^8pulses", duration * wavePulseRate));
        // L13432
        pp.push("^8rounded down");
        // L13433
        pp.push(lfmt!("= %d ^8pulses", pulses));
        // L13434-13437
        let a = m_ceil(
            100.0 * ((pulses + 1.0) * baseInterval / (duration * moreFrequency) - incFrequency),
        );
        let b = m_ceil(
            100.0
                * ((pulses + 1.0) / (wavePulseRate * skillData.num(n!("duration")) * moreDuration)
                    - incDuration),
        );
        pp.push(lfmt!(
            "^8Next breakpoint: %d%% increased Trap Throwing Speed / %d%% increased Duration",
            a,
            b
        ));
        // L13438-13441
        let a = -m_ceil(
            100.0 * (pulses * baseInterval / (duration * moreFrequency) - incFrequency) - 1.0,
        );
        let b = -m_ceil(
            100.0
                * (pulses / (wavePulseRate * skillData.num(n!("duration")) * moreDuration)
                    - incDuration)
                - 1.0,
        );
        pp.push(lfmt!(
            "^8Previous breakpoint: %d%% reduced Trap Throwing Speed / %d%% reduced Duration",
            a,
            b
        ));
        // L13442
        let at = Table::new();
        breakdown.set(n!("AverageActiveTraps"), at.clone());
        // L13443
        at.push("Average active traps, not considering stored cooldown uses:");
        // L13444
        at.push(lfmt!(
            "%.2f^8 /^7 %.2f^8 (pulses / pulse rate = effective skill duration)",
            pulses,
            wavePulseRate
        ));
        // L13445
        at.push(lfmt!("/ %.2f ^8(cooldown)", cooldown));
        // L13446
        at.push(lfmt!("= %.2f traps", averageActiveTraps));
    }
    // L13448
    let maxWaves = skillModList.Sum(ModType::BASE, Some(&*skillCfg), nm!["MaximumWaves"]);
    // L13449
    let mut dpsMultiplier = 1.0;
    // L13450
    if skillPart == Some(2.0) {
        // L13451
        dpsMultiplier = maxWaves * overlapChance;
        // L13452
        if let Some(breakdown) = breakdown {
            // L13453
            let sm = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), sm.clone());
            // L13454
            sm.push("DPS multiplier");
            // L13455
            sm.push("^8= ^7maximum waves^8 * ^7overlap chance^8");
            // L13456
            sm.push(lfmt!("^8=^7 %d^8 *^7 %.2f^8", maxWaves, overlapChance));
            // L13457
            sm.push(lfmt!("^8=^7 %.3f", dpsMultiplier));
        }
    } else if skillPart == Some(3.0) {
        // L13460
        dpsMultiplier = maxWaves;
        // L13461
        if let Some(breakdown) = breakdown {
            // L13462
            let sm = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), sm.clone());
            // L13463
            sm.push("DPS multiplier");
            // L13464
            sm.push(lfmt!("^8=^7 %d (maximum waves)", dpsMultiplier));
        }
    } else if skillPart == Some(4.0) {
        // L13467
        dpsMultiplier = averageActiveTraps;
        // L13468
        if let Some(breakdown) = breakdown {
            // L13469
            let sm = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), sm.clone());
            // L13470
            sm.push("DPS multiplier");
            // L13471
            sm.push(lfmt!("^8=^7 %.2f (average active traps)", dpsMultiplier));
        }
    } else if skillPart == Some(5.0) {
        // L13474
        dpsMultiplier = averageActiveTraps * maxWaves * overlapChance;
        // L13475
        if let Some(breakdown) = breakdown {
            // L13476
            let sm = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), sm.clone());
            // L13477
            sm.push("DPS multiplier");
            // L13478
            sm.push("^8= ^7average active traps^8 * ^7maximum waves^8 * ^7overlap chance^8");
            // L13479
            sm.push(lfmt!(
                "^8=^7 %.2f^8 *^7 %d^8 *^7 %.2f",
                averageActiveTraps,
                maxWaves,
                overlapChance
            ));
            // L13480
            sm.push(lfmt!("^8=^7 %.3f", dpsMultiplier));
        }
    } else if skillPart == Some(6.0) {
        // L13483
        dpsMultiplier = averageActiveTraps * maxWaves;
        // L13484
        if let Some(breakdown) = breakdown {
            // L13485
            let sm = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), sm.clone());
            // L13486
            sm.push("DPS multiplier");
            // L13487
            sm.push("^8= ^7average active traps^8 * ^7maximum waves");
            // L13488
            sm.push(lfmt!("^8=^7 %.2f^8 *^7 %d", averageActiveTraps, maxWaves));
            // L13489
            sm.push(lfmt!("^8=^7 %.3f", dpsMultiplier));
        }
    }
    // L13492
    if dpsMultiplier != 1.0 {
        // L13493
        let v = skillData.num_or(n!("dpsMultiplier"), 1.0) * dpsMultiplier;
        skillData.set(n!("dpsMultiplier"), v);
        // L13494
        let v = output.num_or(n!("SkillDPSMultiplier"), 1.0) * dpsMultiplier;
        output.set(n!("SkillDPSMultiplier"), v);
    }
}

/// @pob Data/Skills/act_dex.lua:13622-13671 h=b282a3c7
/// `data.skills.SeismicTrapAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn SeismicTrapAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L13623
    let skillCfg = activeSkill.skillCfg.get();
    // L13624
    let skillData = activeSkill.skillData.get();
    // L13625
    let skillPart = activeSkill.skillPart.get();
    // L13626
    let skillModList = activeSkill.skillModList.get();
    // L13627-13628 (t_insert, s_format)

    // L13630-13634: `hitChance` is the file-level helper `hitChance`
    // L13635
    let enemyRadius = override_or_sum(&skillModList, &skillCfg, n!("EnemyRadius"));
    // L13636
    let waveRadius = output.num(n!("AreaOfEffectRadiusSecondary"));
    // L13637
    let fullRadius = output.num(n!("AreaOfEffectRadius"));
    // L13638
    let overlapChance = hitChance(enemyRadius, waveRadius, fullRadius);
    // L13639
    output.set(n!("OverlapChance"), overlapChance * 100.0);
    // L13640
    if let Some(breakdown) = breakdown {
        // L13641
        let oc = Table::new();
        breakdown.set(n!("OverlapChance"), oc.clone());
        // L13642
        oc.push("Chance for individual wave to land within range to damage enemy:");
        // L13643
        oc.push("^8= (area where wave can spawn to damage enemy) / (total area)");
        // L13644
        oc.push("^8= (^7secondary radius^8 + ^7enemy radius^8 - 1) ^ 2 / ^7radius^8 ^ 2");
        // L13645
        oc.push(lfmt!(
            "^8= (^7%d^8 +^7 %d^8 - 1) ^ 2 /^7 %d^8 ^ 2",
            waveRadius,
            enemyRadius,
            fullRadius
        ));
        // L13646
        oc.push(lfmt!("^8=^7 %.3f^8%%", overlapChance * 100.0));
    }
    // L13648
    let maxWaves = skillModList.Sum(ModType::BASE, Some(&*skillCfg), nm!["MaximumWaves"]);
    // L13649
    let mut dpsMultiplier = 1.0;
    // L13650
    if skillPart == Some(2.0) {
        // L13651
        dpsMultiplier = maxWaves * overlapChance;
        // L13652
        if let Some(breakdown) = breakdown {
            // L13653
            let sm = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), sm.clone());
            // L13654
            sm.push("DPS multiplier");
            // L13655
            sm.push("^8= ^7maximum waves^8 * ^7overlap chance^8");
            // L13656
            sm.push(lfmt!("^8=^7 %d^8 *^7 %.2f^8", maxWaves, overlapChance));
            // L13657
            sm.push(lfmt!("^8=^7 %.3f", dpsMultiplier));
        }
    } else if skillPart == Some(3.0) {
        // L13660
        dpsMultiplier = maxWaves;
        // L13661
        if let Some(breakdown) = breakdown {
            // L13662
            let sm = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), sm.clone());
            // L13663
            sm.push("DPS multiplier");
            // L13664
            sm.push(lfmt!("^8=^7 %d (maximum waves)", dpsMultiplier));
        }
    }
    // L13667
    if dpsMultiplier != 1.0 {
        // L13668
        let v = skillData.num_or(n!("dpsMultiplier"), 1.0) * dpsMultiplier;
        skillData.set(n!("dpsMultiplier"), v);
        // L13669
        let v = output.num_or(n!("SkillDPSMultiplier"), 1.0) * dpsMultiplier;
        output.set(n!("SkillDPSMultiplier"), v);
    }
}

/// @pob Data/Skills/act_dex.lua:13779-13787 h=b1779b2a
/// `data.skills.ShrapnelBallista.preDamageFunc` (preDamageFunc)
pub(crate) fn ShrapnelBallista_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillModList = activeSkill.skillModList.get();
    let skillData = activeSkill.skillData.get();
    // L13780
    if !skillModList.Flag(None, nm!["SequentialProjectiles"]) {
        // L13781
        let overlap = lor(
            skillData.get(n!("ShrapnelBallistaProjectileOverlap")),
            || {
                if activeSkill
                    .skillTypes
                    .get()
                    .with(|t| t.has(SkillType::Rain))
                {
                    lor(output.get(n!("ProjectileCount")), || Val::from(1.0))
                } else {
                    Val::from(1.0)
                }
            },
        );
        let v = skillData.num_or(n!("dpsMultiplier"), 1.0)
            * m_min(overlap.num(), output.num(n!("ProjectileCount")));
        skillData.set(n!("dpsMultiplier"), v);
    }
    // L13783
    let splitCount = output.num_or(n!("SplitCount"), 0.0);
    // L13784
    if splitCount > 0.0 {
        // L13785
        skillModList.NewMod(
            n!("DPS"),
            ModType::MORE,
            splitCount * 100.0,
            "Split Return",
            0,
            0,
            &[tag!({ type = Condition, var = "ReturningProjectile" })],
        );
    }
}

/// @pob Data/Skills/act_dex.lua:13867-13875 h=b1779b2a
/// `data.skills.ShrapnelBallistaAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn ShrapnelBallistaAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillModList = activeSkill.skillModList.get();
    let skillData = activeSkill.skillData.get();
    // L13780
    if !skillModList.Flag(None, nm!["SequentialProjectiles"]) {
        // L13781
        let overlap = lor(
            skillData.get(n!("ShrapnelBallistaProjectileOverlap")),
            || {
                if activeSkill
                    .skillTypes
                    .get()
                    .with(|t| t.has(SkillType::Rain))
                {
                    lor(output.get(n!("ProjectileCount")), || Val::from(1.0))
                } else {
                    Val::from(1.0)
                }
            },
        );
        let v = skillData.num_or(n!("dpsMultiplier"), 1.0)
            * m_min(overlap.num(), output.num(n!("ProjectileCount")));
        skillData.set(n!("dpsMultiplier"), v);
    }
    // L13783
    let splitCount = output.num_or(n!("SplitCount"), 0.0);
    // L13784
    if splitCount > 0.0 {
        // L13785
        skillModList.NewMod(
            n!("DPS"),
            ModType::MORE,
            splitCount * 100.0,
            "Split Return",
            0,
            0,
            &[tag!({ type = Condition, var = "ReturningProjectile" })],
        );
    }
}

/// @pob Data/Skills/act_dex.lua:14935-14937 h=1154af21
/// `data.skills.ToxicRain.preDamageFunc` (preDamageFunc)
pub(crate) fn ToxicRain_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L14936
    let v = m_min(
        skillData.num_or(n!("podOverlapMultiplier"), 1.0),
        output.num(n!("ProjectileCount")),
    );
    skillData.set(n!("dpsMultiplier"), v);
}

/// @pob Data/Skills/act_dex.lua:15027-15029 h=1154af21
/// `data.skills.ToxicRainAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn ToxicRainAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L14936
    let v = m_min(
        skillData.num_or(n!("podOverlapMultiplier"), 1.0),
        output.num(n!("ProjectileCount")),
    );
    skillData.set(n!("dpsMultiplier"), v);
}

/// @pob Data/Skills/act_dex.lua:15116-15118 h=1154af21
/// `data.skills.ToxicRainAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn ToxicRainAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L14936
    let v = m_min(
        skillData.num_or(n!("podOverlapMultiplier"), 1.0),
        output.num(n!("ProjectileCount")),
    );
    skillData.set(n!("dpsMultiplier"), v);
}

/// @pob Data/Skills/act_dex.lua:16051-16062 h=20843b91
/// `data.skills.TornadoShot.preDamageFunc` (preDamageFunc)
pub(crate) fn TornadoShot_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L16052
    if activeSkill.skillPart.get() == Some(2.0) && output.num_or(n!("ReturnChance"), 0.0) == 0.0 {
        let skillModList = activeSkill.skillModList.get();
        let skillData = activeSkill.skillData.get();
        let skillCfg = activeSkill.skillCfg.get();
        // L16053
        let mut averageSecondaryProjectiles =
            output.num(n!("ProjectileCount")) + output.num_or(n!("SplitCount"), 0.0);
        // if barrage then only shoots 1 projectile at a time, but those can still split and still releases at least 1 secondary projectile
        // L16055
        if skillModList.Flag(None, nm!["SequentialProjectiles"])
            && !skillModList.Flag(None, nm!["OneShotProj"])
            && !skillModList.Flag(None, nm!["NoAdditionalProjectiles"])
            && !skillModList.Flag(None, nm!["SingleProjectile"])
            && !skillModList.Flag(None, nm!["TriggeredBySnipe"])
        {
            // L16056
            averageSecondaryProjectiles = 1.0 + output.num_or(n!("SplitCount"), 0.0);
        }
        // default to 20% per secondary projectile, so 60% base, and 80% with helm enchant
        // L16059
        let chanceForSecondaryProjectilesToHit = m_min(
            lor(skillData.get(n!("tornadoShotSecondaryHitChance")), || {
                Val::from(
                    20.0 * skillModList.Sum(
                        ModType::BASE,
                        Some(&*skillCfg),
                        nm!["tornadoShotSecondaryProjectiles"],
                    ),
                )
            })
            .num()
                / 100.0,
            1.0,
        );
        // L16060
        let v = skillData.num_or(n!("dpsMultiplier"), 1.0)
            * (1.0 + chanceForSecondaryProjectilesToHit * averageSecondaryProjectiles);
        skillData.set(n!("dpsMultiplier"), v);
    }
}

/// @pob Data/Skills/act_dex.lua:17423-17428 h=cd98cc1d
/// `data.skills.PoisonousConcoction.preDamageFunc` (preDamageFunc)
pub(crate) fn PoisonousConcoction_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillModList = activeSkill.skillModList.get();
    let skillCfg = activeSkill.skillCfg.get();
    // L17424 (`Sum(...) or 0`: Sum always returns a number)
    let multiplier = skillModList.Sum(
        ModType::BASE,
        Some(&*skillCfg),
        nm!["ChaosPerLifeFlaskPercent"],
    );
    // L17425
    let addedFromFlask = output.num_or(n!("LifeFlaskRecovery"), 0.0) * (multiplier / 100.0);
    // L17426
    skillModList.NewMod(
        n!("ChaosMin"),
        ModType::BASE,
        addedFromFlask,
        "Life Flask charges consumed",
        0,
        0,
        &[],
    );
    // L17427
    skillModList.NewMod(
        n!("ChaosMax"),
        ModType::BASE,
        addedFromFlask,
        "Life Flask charges consumed",
        0,
        0,
        &[],
    );
}

/// @pob Data/Skills/act_dex.lua:17524-17529 h=cd98cc1d
/// `data.skills.PoisonousConcoctionAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn PoisonousConcoctionAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillModList = activeSkill.skillModList.get();
    let skillCfg = activeSkill.skillCfg.get();
    // L17424 (`Sum(...) or 0`: Sum always returns a number)
    let multiplier = skillModList.Sum(
        ModType::BASE,
        Some(&*skillCfg),
        nm!["ChaosPerLifeFlaskPercent"],
    );
    // L17425
    let addedFromFlask = output.num_or(n!("LifeFlaskRecovery"), 0.0) * (multiplier / 100.0);
    // L17426
    skillModList.NewMod(
        n!("ChaosMin"),
        ModType::BASE,
        addedFromFlask,
        "Life Flask charges consumed",
        0,
        0,
        &[],
    );
    // L17427
    skillModList.NewMod(
        n!("ChaosMax"),
        ModType::BASE,
        addedFromFlask,
        "Life Flask charges consumed",
        0,
        0,
        &[],
    );
}

/// @pob Data/Skills/act_dex.lua:18034-18036 h=55131ec4
/// `data.skills.Tornado.preDamageFunc` (preDamageFunc)
pub(crate) fn Tornado_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L18035
    let v = skillData.get(n!("damageInterval"));
    skillData.set(n!("hitTimeOverride"), v);
}

/// @pob Data/Skills/act_dex.lua:18127-18130 h=5c4f744e
/// `data.skills.TornadoAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn TornadoAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L18128
    let v = skillData.get(n!("damageInterval"));
    skillData.set(n!("hitTimeOverride"), v);
    // L18129
    let v = skillData.num_or(n!("dpsMultiplier"), 1.0)
        * activeSkill.skillModList.get().Sum(
            ModType::BASE,
            Some(&*activeSkill.skillCfg.get()),
            nm!["Multiplier:TornadoofElementalTurbulenceStage"],
        );
    skillData.set(n!("dpsMultiplier"), v);
}

// ---- helpers ------------------------------------------------------------------------------

/// `activeSkill.actor` (a dangling back-pointer is Lua's index of a nil value).
fn actor_of(activeSkill: &ActiveSkillRef) -> ActorRef {
    activeSkill
        .actor()
        .unwrap_or_else(|| lua_error!("attempt to index field 'actor' (a nil value)"))
}

/// `skillModList:Override(skillCfg, name) or skillModList:Sum("BASE", skillCfg, name)`, used in
/// arithmetic (a non-number raises, as in Lua).
fn override_or_sum(skillModList: &ModList, skillCfg: &Cfg, name: Name) -> f64 {
    lor(
        Val::from(skillModList.Override(Some(&**skillCfg), &[name])),
        || Val::from(skillModList.Sum(ModType::BASE, Some(&**skillCfg), &[name])),
    )
    .num()
}

/// The local `hitChance(enemyRadius, areaDamageRadius, areaSpreadRadius)` that ExplosiveTrap
/// (act_dex.lua:7164-7168), SeismicTrap (13404-13408) and SeismicTrapAltX (13630-13634) each
/// declare with the same body (calc doc §8: local closures become private fns). Covered by the
/// enclosing hooks' `@pob` markers.
fn hitChance(enemyRadius: f64, areaDamageRadius: f64, areaSpreadRadius: f64) -> f64 {
    // not to be confused with attack hit chance
    // radius where area damage can land to hit the enemy;
    let damagingAreaRadius = areaDamageRadius + enemyRadius - 1.0;
    // -1 because of two assumptions: PoE coordinates are integers and damage is not registered if the two areas only share a point or vertex. If either is not correct, then -1 is not needed.
    m_min(
        damagingAreaRadius * damagingAreaRadius / (areaSpreadRadius * areaSpreadRadius),
        1.0,
    )
}

/// A `pairs` key used as a number (the radius keys of `AreaOfEffectRadiusTertiaryOccurrences`).
fn key_num(k: Key) -> f64 {
    match k {
        Key::I(i) => i as f64,
        Key::S(s) => Val::from(s).num(),
        Key::B(_) => lua_error!("attempt to perform arithmetic on a boolean value"),
        Key::F(f) => f.get(),
    }
}

/// `{ type = "Release Damage", skillPart = 3 }` (ChargedDash, act_dex.lua:3942/4067): a tag type
/// outside EvalMod's set (`TagType::Other`), which `tag!` cannot name.
fn release_damage_tag() -> Tag {
    Tag {
        r#type: TagType::Other(n!("Release Damage")),
        skillPart: Some(3.0),
        ..Default::default()
    }
}
