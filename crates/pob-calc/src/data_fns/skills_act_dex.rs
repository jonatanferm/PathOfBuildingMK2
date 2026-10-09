//! @pob-file Data/Skills/act_dex.lua @16de4b82d
//! Skill hooks of Data/Skills/act_dex.lua (calc doc §8: `preDamageFunc`, `initialFunc`, `preSkillTypeFunc`,
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
    unported!("Data/Skills/act_dex.lua:1026-1030")
}

/// @pob Data/Skills/act_dex.lua:1114-1118 h=5843d6f7
/// `data.skills.BarrageAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn BarrageAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:1114-1118")
}

/// @pob Data/Skills/act_dex.lua:1374-1379 h=205f8789
/// `data.skills.BladeBlast.preDamageFunc` (preDamageFunc)
pub(crate) fn BladeBlast_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:1374-1379")
}

/// @pob Data/Skills/act_dex.lua:1466-1471 h=205f8789
/// `data.skills.BladeBlastAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn BladeBlastAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:1466-1471")
}

/// @pob Data/Skills/act_dex.lua:1906-1916 h=ec64b5fa
/// `data.skills.BladeFlurry.preDamageFunc` (preDamageFunc)
pub(crate) fn BladeFlurry_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:1906-1916")
}

/// @pob Data/Skills/act_dex.lua:2023-2033 h=ec64b5fa
/// `data.skills.BladeFlurryAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn BladeFlurryAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:2023-2033")
}

/// @pob Data/Skills/act_dex.lua:2209-2211 h=4b1cd804
/// `data.skills.BladeVortex.preDamageFunc` (preDamageFunc)
pub(crate) fn BladeVortex_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:2209-2211")
}

/// @pob Data/Skills/act_dex.lua:2428-2430 h=3c07e211
/// `data.skills.VaalBladeVortex.preDamageFunc` (preDamageFunc)
pub(crate) fn VaalBladeVortex_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:2428-2430")
}

/// @pob Data/Skills/act_dex.lua:2787-2789 h=256412e5
/// `data.skills.BladefallAltZ.preDamageFunc` (preDamageFunc)
pub(crate) fn BladefallAltZ_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:2787-2789")
}

/// @pob Data/Skills/act_dex.lua:2893-2897 h=5843d6f7
/// `data.skills.BlastRain.preDamageFunc` (preDamageFunc)
pub(crate) fn BlastRain_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:2893-2897")
}

/// @pob Data/Skills/act_dex.lua:3939-3944 h=346390c3
/// `data.skills.ChargedDash.preDamageFunc` (preDamageFunc)
pub(crate) fn ChargedDash_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:3939-3944")
}

/// @pob Data/Skills/act_dex.lua:4064-4069 h=b19041c5
/// `data.skills.ChargedDashAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn ChargedDashAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:4064-4069")
}

/// @pob Data/Skills/act_dex.lua:4355-4359 h=23b2c0f6
/// `data.skills.Cremation.preDamageFunc` (preDamageFunc)
pub(crate) fn Cremation_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:4355-4359")
}

/// @pob Data/Skills/act_dex.lua:4473-4477 h=23b2c0f6
/// `data.skills.CremationAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn CremationAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:4473-4477")
}

/// @pob Data/Skills/act_dex.lua:4592-4594 h=01934563
/// `data.skills.CremationAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn CremationAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:4592-4594")
}

/// @pob Data/Skills/act_dex.lua:4703-4716 h=2a8770a4
/// `data.skills.Cyclone.initialFunc` (initialFunc)
pub(crate) fn Cyclone_initialFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:4703-4716")
}

/// @pob Data/Skills/act_dex.lua:4809-4822 h=43245e25
/// `data.skills.CycloneAltX.initialFunc` (initialFunc)
pub(crate) fn CycloneAltX_initialFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:4809-4822")
}

/// @pob Data/Skills/act_dex.lua:4934-4947 h=2a8770a4
/// `data.skills.VaalCyclone.initialFunc` (initialFunc)
pub(crate) fn VaalCyclone_initialFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:4934-4947")
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
    unported!("Data/Skills/act_dex.lua:6699-6778")
}

/// @pob Data/Skills/act_dex.lua:7156-7221 h=bd9f4379
/// `data.skills.ExplosiveTrap.preDamageFunc` (preDamageFunc) (also: data.skills.ExplosiveTrapAltX.preDamageFunc)
pub(crate) fn ExplosiveTrap_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:7156-7221")
}

/// @pob Data/Skills/act_dex.lua:7691-7717 h=2a19e003
/// `data.skills.FlamethrowerTrap.preDamageFunc` (preDamageFunc)
pub(crate) fn FlamethrowerTrap_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:7691-7717")
}

/// @pob Data/Skills/act_dex.lua:7809-7827 h=db767574
/// `data.skills.FlamethrowerTrapAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn FlamethrowerTrapAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:7809-7827")
}

/// @pob Data/Skills/act_dex.lua:10234-10241 h=e693cb8c
/// `data.skills.LancingSteel.preDamageFunc` (preDamageFunc)
pub(crate) fn LancingSteel_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:10234-10241")
}

/// @pob Data/Skills/act_dex.lua:10337-10344 h=659b0081
/// `data.skills.LancingSteelAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn LancingSteelAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:10337-10344")
}

/// @pob Data/Skills/act_dex.lua:10590-10594 h=5843d6f7
/// `data.skills.VaalLightningArrow.preDamageFunc` (preDamageFunc)
pub(crate) fn VaalLightningArrow_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:10590-10594")
}

/// @pob Data/Skills/act_dex.lua:11747-11752 h=a9e26965
/// `data.skills.StormRain.preDamageFunc` (preDamageFunc)
pub(crate) fn StormRain_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:11747-11752")
}

/// @pob Data/Skills/act_dex.lua:11851-11856 h=00a16615
/// `data.skills.StormRainAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn StormRainAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:11851-11856")
}

/// @pob Data/Skills/act_dex.lua:11958-11964 h=0fc060cb
/// `data.skills.StormRainAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn StormRainAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:11958-11964")
}

/// @pob Data/Skills/act_dex.lua:12990-12992 h=2da665b7
/// `data.skills.ScourgeArrow.preDamageFunc` (preDamageFunc)
pub(crate) fn ScourgeArrow_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:12990-12992")
}

/// @pob Data/Skills/act_dex.lua:13381-13496 h=fa977e88
/// `data.skills.SeismicTrap.preDamageFunc` (preDamageFunc)
pub(crate) fn SeismicTrap_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:13381-13496")
}

/// @pob Data/Skills/act_dex.lua:13622-13671 h=b282a3c7
/// `data.skills.SeismicTrapAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn SeismicTrapAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:13622-13671")
}

/// @pob Data/Skills/act_dex.lua:13779-13787 h=b1779b2a
/// `data.skills.ShrapnelBallista.preDamageFunc` (preDamageFunc)
pub(crate) fn ShrapnelBallista_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:13779-13787")
}

/// @pob Data/Skills/act_dex.lua:13867-13875 h=b1779b2a
/// `data.skills.ShrapnelBallistaAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn ShrapnelBallistaAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:13867-13875")
}

/// @pob Data/Skills/act_dex.lua:14935-14937 h=1154af21
/// `data.skills.ToxicRain.preDamageFunc` (preDamageFunc)
pub(crate) fn ToxicRain_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:14935-14937")
}

/// @pob Data/Skills/act_dex.lua:15027-15029 h=1154af21
/// `data.skills.ToxicRainAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn ToxicRainAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:15027-15029")
}

/// @pob Data/Skills/act_dex.lua:15116-15118 h=1154af21
/// `data.skills.ToxicRainAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn ToxicRainAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:15116-15118")
}

/// @pob Data/Skills/act_dex.lua:16051-16062 h=20843b91
/// `data.skills.TornadoShot.preDamageFunc` (preDamageFunc)
pub(crate) fn TornadoShot_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:16051-16062")
}

/// @pob Data/Skills/act_dex.lua:17423-17428 h=cd98cc1d
/// `data.skills.PoisonousConcoction.preDamageFunc` (preDamageFunc)
pub(crate) fn PoisonousConcoction_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:17423-17428")
}

/// @pob Data/Skills/act_dex.lua:17524-17529 h=cd98cc1d
/// `data.skills.PoisonousConcoctionAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn PoisonousConcoctionAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:17524-17529")
}

/// @pob Data/Skills/act_dex.lua:18034-18036 h=55131ec4
/// `data.skills.Tornado.preDamageFunc` (preDamageFunc)
pub(crate) fn Tornado_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:18034-18036")
}

/// @pob Data/Skills/act_dex.lua:18127-18130 h=5c4f744e
/// `data.skills.TornadoAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn TornadoAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_dex.lua:18127-18130")
}
