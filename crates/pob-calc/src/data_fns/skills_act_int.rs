//! @pob-file Data/Skills/act_int.lua @16de4b82d
//! Skill hooks of Data/Skills/act_int.lua (calc doc §8: `preDamageFunc`, `initialFunc`, `preSkillTypeFunc`,
//! `postCritFunc`, `explosiveArrowFunc`).
//!
//! Owner: SK-A. Stub created by U0: signatures are final (frozen; change through a CCR),
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
        id: "data.skills.BrandSupport.preDamageFunc",
        hash: "48907eac3b6310cb369ef87ebddefa1a756bd1661da336193b0eaae2df3d17c6",
        f: SkillFuncImpl::Std(BrandSupport_preDamageFunc),
    },
    FnRow {
        id: "data.skills.ArmageddonBrand.preDamageFunc",
        hash: "48907eac3b6310cb369ef87ebddefa1a756bd1661da336193b0eaae2df3d17c6",
        f: SkillFuncImpl::Std(ArmageddonBrand_preDamageFunc),
    },
    FnRow {
        id: "data.skills.ArmageddonBrandAltY.preDamageFunc",
        hash: "48907eac3b6310cb369ef87ebddefa1a756bd1661da336193b0eaae2df3d17c6",
        f: SkillFuncImpl::Std(ArmageddonBrandAltY_preDamageFunc),
    },
    FnRow {
        id: "data.skills.BallLightning.preDamageFunc",
        hash: "dcb69ca3835a1191fe7c523895792508563f293fe432f147c747c60c6fef139e",
        f: SkillFuncImpl::Std(BallLightning_preDamageFunc),
    },
    FnRow {
        id: "data.skills.BallLightningAltX.preDamageFunc",
        hash: "89c427cc7e2f34a3c66e83beae5543c68e59253f28cdcf5b0553160143440188",
        f: SkillFuncImpl::Std(BallLightningAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.BallLightningAltY.preDamageFunc",
        hash: "cba9cc4100f856123d3325e56396627834bba054a40ccf8b836925fe7c4adccf",
        f: SkillFuncImpl::Std(BallLightningAltY_preDamageFunc),
    },
    FnRow {
        id: "data.skills.Bane.preSkillTypeFunc",
        hash: "fba3ceee7fea2793a2d314ed507e81d59d47f9c9e2b09dcbeb6e02863adfaca2",
        f: SkillFuncImpl::Std(Bane_preSkillTypeFunc),
    },
    FnRow {
        id: "data.skills.BaneAltX.preSkillTypeFunc",
        hash: "fba3ceee7fea2793a2d314ed507e81d59d47f9c9e2b09dcbeb6e02863adfaca2",
        f: SkillFuncImpl::Std(BaneAltX_preSkillTypeFunc),
    },
    FnRow {
        id: "data.skills.BlazingSalvo.preDamageFunc",
        hash: "320418560c45149dd3f3e44b313abef1f0ce7ff4911d65edd1e021ac929f8d7d",
        f: SkillFuncImpl::Std(BlazingSalvo_preDamageFunc),
    },
    FnRow {
        id: "data.skills.Bodyswap.preDamageFunc",
        hash: "400f194111cbbdd3fa5526b6026439fa4da0643ee4126c8ea0c343b2bcdb7cc4",
        f: SkillFuncImpl::Std(Bodyswap_preDamageFunc),
    },
    FnRow {
        id: "data.skills.BodyswapAltX.preDamageFunc",
        hash: "400f194111cbbdd3fa5526b6026439fa4da0643ee4126c8ea0c343b2bcdb7cc4",
        f: SkillFuncImpl::Std(BodyswapAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.DarkPact.preDamageFunc",
        hash: "54cbc3a2f6744bc90aa534629f69e10239109e56e9631bea72eda678c3db98f5",
        f: SkillFuncImpl::Std(DarkPact_preDamageFunc),
    },
    FnRow {
        id: "data.skills.DarkPactAltX.preDamageFunc",
        hash: "74942ee31d9fdf61da9a7d9ed8d6e73c1005d4b108bd08775d0363449bf0ec60",
        f: SkillFuncImpl::Std(DarkPactAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.DivineIre.preDamageFunc",
        hash: "f9c1b539df20a924f9fd319a7addab2918186e24162e68189e7b42de466ae7f8",
        f: SkillFuncImpl::Std(DivineIre_preDamageFunc),
    },
    FnRow {
        id: "data.skills.DivineIreAltX.preDamageFunc",
        hash: "25a809cb4fe5f0739cf8d3284c70e8dbdc2a3eef31f1ae187eab371f088f2ce8",
        f: SkillFuncImpl::Std(DivineIreAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.DivineIreAltY.preDamageFunc",
        hash: "7a4ba81a52f2a63bb6f18cda7ea66454dd7fb1ae2bc33adc50260434d39485f5",
        f: SkillFuncImpl::Std(DivineIreAltY_preDamageFunc),
    },
    FnRow {
        id: "data.skills.EyeOfWinter.preDamageFunc",
        hash: "0a1071f37209aa0bde8bc91a7ab1bb25d949adf9554f2fc4afb04d0d4e96aa5f",
        f: SkillFuncImpl::Std(EyeOfWinter_preDamageFunc),
    },
    FnRow {
        id: "data.skills.EyeOfWinterAltX.preDamageFunc",
        hash: "505fe42709ddbbd02464f56c05a1886d33b00377fb63f80c40d793817ff14d9d",
        f: SkillFuncImpl::Std(EyeOfWinterAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.EyeOfWinterAltY.preDamageFunc",
        hash: "cde5535b049367c04282e6d0f8c0ff11409e8ec2ea4f6a93691d0875a0fb29a7",
        f: SkillFuncImpl::Std(EyeOfWinterAltY_preDamageFunc),
    },
    FnRow {
        id: "data.skills.Flameblast.preDamageFunc",
        hash: "d91f3f655684a8ae1337eac08c1fc1191699d5fc5e89d66303764d6fe9b31e36",
        f: SkillFuncImpl::Std(Flameblast_preDamageFunc),
    },
    FnRow {
        id: "data.skills.FlameblastAltX.preDamageFunc",
        hash: "f96210fe53a013ffa40a757544f282747f78617d8da9a269ffc278ee9ee89556",
        f: SkillFuncImpl::Std(FlameblastAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.FlameblastAltY.preDamageFunc",
        hash: "251dfcbb9351c5830cd56f1e801ead6f33c4e6cf877fd6ff6a074bf6cabc2117",
        f: SkillFuncImpl::Std(FlameblastAltY_preDamageFunc),
    },
    FnRow {
        id: "data.skills.ForbiddenRite.preDamageFunc",
        hash: "8457f4596105fea7e4ec5004af033184441926d8a9011141ff1701f05563ba0d",
        f: SkillFuncImpl::Std(ForbiddenRite_preDamageFunc),
    },
    FnRow {
        id: "data.skills.ForbiddenRiteAltX.preDamageFunc",
        hash: "b3b9441a39c2cafc17cd4c2c2b55e57b984501ce03215e2da96de822e277fee6",
        f: SkillFuncImpl::Std(ForbiddenRiteAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.FreezingPulse.preDamageFunc",
        hash: "06ba986fa54db9309760acd69ad078489a2c6dfe3714aa04413a79384595c94a",
        f: SkillFuncImpl::Std(FreezingPulse_preDamageFunc),
    },
    FnRow {
        id: "data.skills.FrostBombAltY.preDamageFunc",
        hash: "2ca978412425995c8196876064185d0aa15abe800fd24b4d2e08ce4aa5b9e1fb",
        f: SkillFuncImpl::Std(FrostBombAltY_preDamageFunc),
    },
    FnRow {
        id: "data.skills.GalvanicField.preDamageFunc",
        hash: "c46c160cb7ed4e7493e4c23dcc87fb5820553396bc2c9f1589a0b1e5dd39eac6",
        f: SkillFuncImpl::Std(GalvanicField_preDamageFunc),
    },
    FnRow {
        id: "data.skills.GalvanicFieldAltX.preDamageFunc",
        hash: "c46c160cb7ed4e7493e4c23dcc87fb5820553396bc2c9f1589a0b1e5dd39eac6",
        f: SkillFuncImpl::Std(GalvanicFieldAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.Hydrosphere.preDamageFunc",
        hash: "49c97908eb081a0b6d019e28727e7a2f3fc6451068ce7dfcbfd29fb38b31e919",
        f: SkillFuncImpl::Std(Hydrosphere_preDamageFunc),
    },
    FnRow {
        id: "data.skills.HeraldOfThunder.preDamageFunc",
        hash: "3398c8cc4a2056f42ebb76d7a3099234dae24766f46b624cbb9208f2fe618343",
        f: SkillFuncImpl::Std(HeraldOfThunder_preDamageFunc),
    },
    FnRow {
        id: "data.skills.IceSpear.preDamageFunc",
        hash: "125b4c43fa4c1ca7013b5cae25905d7bd974e9f2b827a0b6e4c9bd1fce943279",
        f: SkillFuncImpl::Std(IceSpear_preDamageFunc),
    },
    FnRow {
        id: "data.skills.IceSpearAltX.preDamageFunc",
        hash: "125b4c43fa4c1ca7013b5cae25905d7bd974e9f2b827a0b6e4c9bd1fce943279",
        f: SkillFuncImpl::Std(IceSpearAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.Incinerate.preDamageFunc",
        hash: "d6cb072ea9bff8bb8feca577ce5f6bbd96fdcae3dbc5e27c8740f86aef289f11",
        f: SkillFuncImpl::Std(Incinerate_preDamageFunc),
    },
    FnRow {
        id: "data.skills.IncinerateAltX.preDamageFunc",
        hash: "12a8c701214603e8d50ce7f526488d83fd8c6ad500ff34e4c3d7fa4b8997d9e4",
        f: SkillFuncImpl::Std(IncinerateAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.IncinerateAltY.preDamageFunc",
        hash: "b61aa681cbe1828858a0c35fdfad00007843ae8c0fac92c0902fb7b526bfd1dd",
        f: SkillFuncImpl::Std(IncinerateAltY_preDamageFunc),
    },
    FnRow {
        id: "data.skills.KineticFusillade.preDamageFunc",
        hash: "7894a531ba5645f60951d837ce03583965a144c4ef3f22c9e8e69d26f816367a",
        f: SkillFuncImpl::Std(KineticFusillade_preDamageFunc),
    },
    FnRow {
        id: "data.skills.KineticFusillade.postCritFunc",
        hash: "84ce184bd3aec16164374c36bedcfed0b740bad90f48d0de247dfc004c135304",
        f: SkillFuncImpl::Std(KineticFusillade_postCritFunc),
    },
    FnRow {
        id: "data.skills.KineticFusilladeAltX.preDamageFunc",
        hash: "ff8dd4f936006f9cd573b6c8cac571d1bbbf4b3e206a59c1f7576c9e208b1e3c",
        f: SkillFuncImpl::Std(KineticFusilladeAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.KineticFusilladeAltX.postCritFunc",
        hash: "84ce184bd3aec16164374c36bedcfed0b740bad90f48d0de247dfc004c135304",
        f: SkillFuncImpl::Std(KineticFusilladeAltX_postCritFunc),
    },
    FnRow {
        id: "data.skills.LightningSpireTrap.preDamageFunc",
        hash: "d7337e59d5c21fa35d46d46e596f8b5884ac6223f66675756b9e367f0ebf1ea8",
        f: SkillFuncImpl::Std(LightningSpireTrap_preDamageFunc),
    },
    FnRow {
        id: "data.skills.LightningSpireTrapAltX.preDamageFunc",
        hash: "028ba06841b97aabc71d46f9e1889238deb8252d56389e4d5b41adc296ab402f",
        f: SkillFuncImpl::Std(LightningSpireTrapAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.LightningSpireTrapAltY.preDamageFunc",
        hash: "ef24fbb9e65b31ff10dda30110edff92b135ceffd54395d20f91c0e5198ffaa1",
        f: SkillFuncImpl::Std(LightningSpireTrapAltY_preDamageFunc),
    },
    FnRow {
        id: "data.skills.LightningTendrilsAltX.preDamageFunc",
        hash: "b646128c0d7652d7854f4684bbe6be3c545a3739c7803643487b494bb9be0dad",
        f: SkillFuncImpl::Std(LightningTendrilsAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.LightningTendrilsAltX.postCritFunc",
        hash: "1d9effc02375c0875ba97ed557181296c4fb298a2994b5923f902c3162a2c0f5",
        f: SkillFuncImpl::Std(LightningTendrilsAltX_postCritFunc),
    },
    FnRow {
        id: "data.skills.Manabond.preDamageFunc",
        hash: "bb99045da4e3a30496e00a37107e19a7760dc43a6c9020f4c6e1ca2909836783",
        f: SkillFuncImpl::Std(Manabond_preDamageFunc),
    },
    FnRow {
        id: "data.skills.OrbOfStorms.preDamageFunc",
        hash: "c16984d1b006d2c64d68d32bf1b6b473c2897c50d2af1f7fd72f7629c53afc60",
        f: SkillFuncImpl::Std(OrbOfStorms_preDamageFunc),
    },
    FnRow {
        id: "data.skills.PenanceBrand.preDamageFunc",
        hash: "d4facd0432a4e07b57b66a47fd452a9a425261e509f7f3dd98c8f732947a8476",
        f: SkillFuncImpl::Std(PenanceBrand_preDamageFunc),
    },
    FnRow {
        id: "data.skills.PenanceBrandAltX.preDamageFunc",
        hash: "48907eac3b6310cb369ef87ebddefa1a756bd1661da336193b0eaae2df3d17c6",
        f: SkillFuncImpl::Std(PenanceBrandAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.PenanceBrandAltY.preDamageFunc",
        hash: "48907eac3b6310cb369ef87ebddefa1a756bd1661da336193b0eaae2df3d17c6",
        f: SkillFuncImpl::Std(PenanceBrandAltY_preDamageFunc),
    },
    FnRow {
        id: "data.skills.RighteousFire.preDamageFunc",
        hash: "47f63f828c7e4767e698ee0c010aa605ced004ea8c0a7b486d7aaa75b3c82225",
        f: SkillFuncImpl::Std(RighteousFire_preDamageFunc),
    },
    FnRow {
        id: "data.skills.RighteousFireAltX.preDamageFunc",
        hash: "12dd2781a200d312c2fb21b2cc18b0478792af63dd2581d67a2ad6d207ee775b",
        f: SkillFuncImpl::Std(RighteousFireAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.VaalRighteousFire.preDamageFunc",
        hash: "00b0c5422a4ecb05b7f078f5c7b1d3407624dea66fa3ae4b73a5d88da4db2e78",
        f: SkillFuncImpl::Std(VaalRighteousFire_preDamageFunc),
    },
    FnRow {
        id: "data.skills.Spark.preDamageFunc",
        hash: "3eda1a3a1b56279d7349ca9f484f7889c383b67346dcbb78c9cd19d9ce3f887e",
        f: SkillFuncImpl::Std(Spark_preDamageFunc),
    },
    FnRow {
        id: "data.skills.SparkAltX.preDamageFunc",
        hash: "6ba236c5d8dff7afa2ebf95e25c04b32216a8b592594ca233b11303f93b36096",
        f: SkillFuncImpl::Std(SparkAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.SparkAltY.preDamageFunc",
        hash: "3eda1a3a1b56279d7349ca9f484f7889c383b67346dcbb78c9cd19d9ce3f887e",
        f: SkillFuncImpl::Std(SparkAltY_preDamageFunc),
    },
    FnRow {
        id: "data.skills.StormBrand.preDamageFunc",
        hash: "48907eac3b6310cb369ef87ebddefa1a756bd1661da336193b0eaae2df3d17c6",
        f: SkillFuncImpl::Std(StormBrand_preDamageFunc),
    },
    FnRow {
        id: "data.skills.StormBrandAltX.preDamageFunc",
        hash: "48907eac3b6310cb369ef87ebddefa1a756bd1661da336193b0eaae2df3d17c6",
        f: SkillFuncImpl::Std(StormBrandAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.StormBurst.preDamageFunc",
        hash: "3cec102ee122f79dbd58a5023871dadda66a8edf257e1c49be4461d59f0dad94",
        f: SkillFuncImpl::Std(StormBurst_preDamageFunc),
    },
    FnRow {
        id: "data.skills.StormBurstAltX.preDamageFunc",
        hash: "41a3cdc39bf20760e409a1933f6e52f7c5cb66a52b830d12a785239596d3fce0",
        f: SkillFuncImpl::Std(StormBurstAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.VoidSphere.preDamageFunc",
        hash: "0fe6add8a42008e102b8e4143c3e5813ba32f1a423d639b6750f727f8b791a51",
        f: SkillFuncImpl::Std(VoidSphere_preDamageFunc),
    },
    FnRow {
        id: "data.skills.VoidSphereAltX.preDamageFunc",
        hash: "0fe6add8a42008e102b8e4143c3e5813ba32f1a423d639b6750f727f8b791a51",
        f: SkillFuncImpl::Std(VoidSphereAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.VoltaxicBurst.preDamageFunc",
        hash: "7491b90332deaa6b87be2e67058f16c82da7744dd32c0cee0f3d94e161959927",
        f: SkillFuncImpl::Std(VoltaxicBurst_preDamageFunc),
    },
    FnRow {
        id: "data.skills.Vortex.preDamageFunc",
        hash: "bcb877cd38f680213da072ab311e27cfa9cb87d1faa786617e844738aad9537e",
        f: SkillFuncImpl::Std(Vortex_preDamageFunc),
    },
    FnRow {
        id: "data.skills.VortexAltX.preDamageFunc",
        hash: "bcb877cd38f680213da072ab311e27cfa9cb87d1faa786617e844738aad9537e",
        f: SkillFuncImpl::Std(VortexAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.WaveOfConviction.preDamageFunc",
        hash: "d4d6595d68b37dd6fa2c629d0136a52a372db77a256afb356d3486a592f28e4b",
        f: SkillFuncImpl::Std(WaveOfConviction_preDamageFunc),
    },
    FnRow {
        id: "data.skills.WaveOfConvictionAltY.preDamageFunc",
        hash: "b68cf4116a44f96ab8e673e4803e33a87a20f04fd98e48b36662ec499174edac",
        f: SkillFuncImpl::Std(WaveOfConvictionAltY_preDamageFunc),
    },
    FnRow {
        id: "data.skills.WinterOrb.preDamageFunc",
        hash: "ffece993f623ae6cf65174c43bc24ec134484057f5836476506381d9332d10a5",
        f: SkillFuncImpl::Std(WinterOrb_preDamageFunc),
    },
    FnRow {
        id: "data.skills.WintertideBrand.preDamageFunc",
        hash: "a78ef7b6cb1c7bef2be331229a14f7fd2121b407f0f0e1b3c64454d8c1723e35",
        f: SkillFuncImpl::Std(WintertideBrand_preDamageFunc),
    },
];

/// @pob Data/Skills/act_int.lua:552-554 h=557be66a
/// `data.skills.BrandSupport.preDamageFunc` (preDamageFunc)
pub(crate) fn BrandSupport_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:552-554")
}

/// @pob Data/Skills/act_int.lua:715-717 h=557be66a
/// `data.skills.ArmageddonBrand.preDamageFunc` (preDamageFunc)
pub(crate) fn ArmageddonBrand_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:715-717")
}

/// @pob Data/Skills/act_int.lua:895-897 h=557be66a
/// `data.skills.ArmageddonBrandAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn ArmageddonBrandAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:895-897")
}

/// @pob Data/Skills/act_int.lua:1086-1167 h=e40e47ed
/// `data.skills.BallLightning.preDamageFunc` (preDamageFunc)
pub(crate) fn BallLightning_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:1086-1167")
}

/// @pob Data/Skills/act_int.lua:1252-1275 h=cecfa725
/// `data.skills.BallLightningAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn BallLightningAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:1252-1275")
}

/// @pob Data/Skills/act_int.lua:1375-1393 h=98a21d3a
/// `data.skills.BallLightningAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn BallLightningAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:1375-1393")
}

/// @pob Data/Skills/act_int.lua:1470-1481 h=e0d40709
/// `data.skills.Bane.preSkillTypeFunc` (preSkillTypeFunc)
pub(crate) fn Bane_preSkillTypeFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:1470-1481")
}

/// @pob Data/Skills/act_int.lua:1647-1658 h=e0d40709
/// `data.skills.BaneAltX.preSkillTypeFunc` (preSkillTypeFunc)
pub(crate) fn BaneAltX_preSkillTypeFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:1647-1658")
}

/// @pob Data/Skills/act_int.lua:1823-1827 h=d4206938
/// `data.skills.BlazingSalvo.preDamageFunc` (preDamageFunc)
pub(crate) fn BlazingSalvo_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:1823-1827")
}

/// @pob Data/Skills/act_int.lua:2298-2309 h=026d581b
/// `data.skills.Bodyswap.preDamageFunc` (preDamageFunc)
pub(crate) fn Bodyswap_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:2298-2309")
}

/// @pob Data/Skills/act_int.lua:2413-2424 h=026d581b
/// `data.skills.BodyswapAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn BodyswapAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:2413-2424")
}

/// @pob Data/Skills/act_int.lua:3889-3903 h=ccc82361
/// `data.skills.DarkPact.preDamageFunc` (preDamageFunc)
pub(crate) fn DarkPact_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:3889-3903")
}

/// @pob Data/Skills/act_int.lua:4006-4022 h=b3f64de4
/// `data.skills.DarkPactAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn DarkPactAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:4006-4022")
}

/// @pob Data/Skills/act_int.lua:4602-4606 h=d7eac377
/// `data.skills.DivineIre.preDamageFunc` (preDamageFunc)
pub(crate) fn DivineIre_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:4602-4606")
}

/// @pob Data/Skills/act_int.lua:4712-4716 h=9ea5c810
/// `data.skills.DivineIreAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn DivineIreAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:4712-4716")
}

/// @pob Data/Skills/act_int.lua:4823-4825 h=6b2c2529
/// `data.skills.DivineIreAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn DivineIreAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:4823-4825")
}

/// @pob Data/Skills/act_int.lua:5518-5520 h=01b5264c
/// `data.skills.EyeOfWinter.preDamageFunc` (preDamageFunc)
pub(crate) fn EyeOfWinter_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:5518-5520")
}

/// @pob Data/Skills/act_int.lua:5611-5613 h=eea88caf
/// `data.skills.EyeOfWinterAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn EyeOfWinterAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:5611-5613")
}

/// @pob Data/Skills/act_int.lua:5704-5706 h=a10d566f
/// `data.skills.EyeOfWinterAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn EyeOfWinterAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:5704-5706")
}

/// @pob Data/Skills/act_int.lua:6800-6802 h=68deb3ee
/// `data.skills.Flameblast.preDamageFunc` (preDamageFunc)
pub(crate) fn Flameblast_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:6800-6802")
}

/// @pob Data/Skills/act_int.lua:6905-6907 h=78160b6c
/// `data.skills.FlameblastAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn FlameblastAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:6905-6907")
}

/// @pob Data/Skills/act_int.lua:7008-7010 h=a159b7f4
/// `data.skills.FlameblastAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn FlameblastAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:7008-7010")
}

/// @pob Data/Skills/act_int.lua:7390-7434 h=f7f48a4b
/// `data.skills.ForbiddenRite.preDamageFunc` (preDamageFunc)
pub(crate) fn ForbiddenRite_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:7390-7434")
}

/// @pob Data/Skills/act_int.lua:7549-7589 h=58a90593
/// `data.skills.ForbiddenRiteAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn ForbiddenRiteAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:7549-7589")
}

/// @pob Data/Skills/act_int.lua:7686-7689 h=55c95af4
/// `data.skills.FreezingPulse.preDamageFunc` (preDamageFunc)
pub(crate) fn FreezingPulse_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:7686-7689")
}

/// @pob Data/Skills/act_int.lua:7933-7936 h=46e40c24
/// `data.skills.FrostBombAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn FrostBombAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:7933-7936")
}

/// @pob Data/Skills/act_int.lua:8543-8545 h=852e25cb
/// `data.skills.GalvanicField.preDamageFunc` (preDamageFunc)
pub(crate) fn GalvanicField_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:8543-8545")
}

/// @pob Data/Skills/act_int.lua:8663-8665 h=852e25cb
/// `data.skills.GalvanicFieldAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn GalvanicFieldAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:8663-8665")
}

/// @pob Data/Skills/act_int.lua:8972-8976 h=59e1d7b4
/// `data.skills.Hydrosphere.preDamageFunc` (preDamageFunc)
pub(crate) fn Hydrosphere_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:8972-8976")
}

/// @pob Data/Skills/act_int.lua:9383-9385 h=7b53bbcc
/// `data.skills.HeraldOfThunder.preDamageFunc` (preDamageFunc)
pub(crate) fn HeraldOfThunder_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:9383-9385")
}

/// @pob Data/Skills/act_int.lua:9805-9809 h=81d50744
/// `data.skills.IceSpear.preDamageFunc` (preDamageFunc)
pub(crate) fn IceSpear_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:9805-9809")
}

/// @pob Data/Skills/act_int.lua:9915-9919 h=81d50744
/// `data.skills.IceSpearAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn IceSpearAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:9915-9919")
}

/// @pob Data/Skills/act_int.lua:10284-10288 h=9fd10d63
/// `data.skills.Incinerate.preDamageFunc` (preDamageFunc)
pub(crate) fn Incinerate_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:10284-10288")
}

/// @pob Data/Skills/act_int.lua:10422-10426 h=d0206498
/// `data.skills.IncinerateAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn IncinerateAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:10422-10426")
}

/// @pob Data/Skills/act_int.lua:10561-10565 h=a72c7761
/// `data.skills.IncinerateAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn IncinerateAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:10561-10565")
}

/// @pob Data/Skills/act_int.lua:11009-11045 h=bc5e2942
/// `data.skills.KineticFusillade.preDamageFunc` (preDamageFunc)
pub(crate) fn KineticFusillade_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:11009-11045")
}

/// @pob Data/Skills/act_int.lua:11046-11123 h=65b58eeb
/// `data.skills.KineticFusillade.postCritFunc` (postCritFunc)
pub(crate) fn KineticFusillade_postCritFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:11046-11123")
}

/// @pob Data/Skills/act_int.lua:11227-11263 h=440601e4
/// `data.skills.KineticFusilladeAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn KineticFusilladeAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:11227-11263")
}

/// @pob Data/Skills/act_int.lua:11264-11341 h=65b58eeb
/// `data.skills.KineticFusilladeAltX.postCritFunc` (postCritFunc)
pub(crate) fn KineticFusilladeAltX_postCritFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:11264-11341")
}

/// @pob Data/Skills/act_int.lua:11667-11788 h=e3fdb3c5
/// `data.skills.LightningSpireTrap.preDamageFunc` (preDamageFunc)
pub(crate) fn LightningSpireTrap_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:11667-11788")
}

/// @pob Data/Skills/act_int.lua:11912-12028 h=e0102961
/// `data.skills.LightningSpireTrapAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn LightningSpireTrapAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:11912-12028")
}

/// @pob Data/Skills/act_int.lua:12150-12260 h=73550c96
/// `data.skills.LightningSpireTrapAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn LightningSpireTrapAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:12150-12260")
}

/// @pob Data/Skills/act_int.lua:12642-12650 h=faf8e954
/// `data.skills.LightningTendrilsAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn LightningTendrilsAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:12642-12650")
}

/// @pob Data/Skills/act_int.lua:12651-12662 h=890dcd7f
/// `data.skills.LightningTendrilsAltX.postCritFunc` (postCritFunc)
pub(crate) fn LightningTendrilsAltX_postCritFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:12651-12662")
}

/// @pob Data/Skills/act_int.lua:13527-13532 h=048c8a3b
/// `data.skills.Manabond.preDamageFunc` (preDamageFunc)
pub(crate) fn Manabond_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:13527-13532")
}

/// @pob Data/Skills/act_int.lua:13621-13623 h=ee56aaa4
/// `data.skills.OrbOfStorms.preDamageFunc` (preDamageFunc)
pub(crate) fn OrbOfStorms_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:13621-13623")
}

/// @pob Data/Skills/act_int.lua:13809-13811 h=6c38aaad
/// `data.skills.PenanceBrand.preDamageFunc` (preDamageFunc)
pub(crate) fn PenanceBrand_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:13809-13811")
}

/// @pob Data/Skills/act_int.lua:13901-13903 h=557be66a
/// `data.skills.PenanceBrandAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn PenanceBrandAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:13901-13903")
}

/// @pob Data/Skills/act_int.lua:14015-14017 h=557be66a
/// `data.skills.PenanceBrandAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn PenanceBrandAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:14015-14017")
}

/// @pob Data/Skills/act_int.lua:15428-15434 h=5a3564ba
/// `data.skills.RighteousFire.preDamageFunc` (preDamageFunc)
pub(crate) fn RighteousFire_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:15428-15434")
}

/// @pob Data/Skills/act_int.lua:15533-15537 h=7fbb670a
/// `data.skills.RighteousFireAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn RighteousFireAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:15533-15537")
}

/// @pob Data/Skills/act_int.lua:15625-15631 h=294abdee
/// `data.skills.VaalRighteousFire.preDamageFunc` (preDamageFunc)
pub(crate) fn VaalRighteousFire_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:15625-15631")
}

/// @pob Data/Skills/act_int.lua:16721-16727 h=c11e51ab
/// `data.skills.Spark.preDamageFunc` (preDamageFunc)
pub(crate) fn Spark_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:16721-16727")
}

/// @pob Data/Skills/act_int.lua:16811-16817 h=c11e51ab
/// `data.skills.SparkAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn SparkAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:16811-16817")
}

/// @pob Data/Skills/act_int.lua:16902-16908 h=c11e51ab
/// `data.skills.SparkAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn SparkAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:16902-16908")
}

/// @pob Data/Skills/act_int.lua:17373-17375 h=557be66a
/// `data.skills.StormBrand.preDamageFunc` (preDamageFunc)
pub(crate) fn StormBrand_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:17373-17375")
}

/// @pob Data/Skills/act_int.lua:17470-17472 h=557be66a
/// `data.skills.StormBrandAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn StormBrandAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:17470-17472")
}

/// @pob Data/Skills/act_int.lua:18036-18048 h=f573ec31
/// `data.skills.StormBurst.preDamageFunc` (preDamageFunc)
pub(crate) fn StormBurst_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:18036-18048")
}

/// @pob Data/Skills/act_int.lua:18151-18157 h=3ffac2b8
/// `data.skills.StormBurstAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn StormBurstAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:18151-18157")
}

/// @pob Data/Skills/act_int.lua:20267-20269 h=1b32abd3
/// `data.skills.VoidSphere.preDamageFunc` (preDamageFunc)
pub(crate) fn VoidSphere_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:20267-20269")
}

/// @pob Data/Skills/act_int.lua:20356-20358 h=1b32abd3
/// `data.skills.VoidSphereAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn VoidSphereAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:20356-20358")
}

/// @pob Data/Skills/act_int.lua:20445-20448 h=119adb7b
/// `data.skills.VoltaxicBurst.preDamageFunc` (preDamageFunc)
pub(crate) fn VoltaxicBurst_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:20445-20448")
}

/// @pob Data/Skills/act_int.lua:20542-20544 h=0a437538
/// `data.skills.Vortex.preDamageFunc` (preDamageFunc)
pub(crate) fn Vortex_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:20542-20544")
}

/// @pob Data/Skills/act_int.lua:20629-20631 h=0a437538
/// `data.skills.VortexAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn VortexAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:20629-20631")
}

/// @pob Data/Skills/act_int.lua:20795-20800 h=916a8a0d
/// `data.skills.WaveOfConviction.preDamageFunc` (preDamageFunc)
pub(crate) fn WaveOfConviction_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:20795-20800")
}

/// @pob Data/Skills/act_int.lua:20889-20894 h=16bf7003
/// `data.skills.WaveOfConvictionAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn WaveOfConvictionAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:20889-20894")
}

/// @pob Data/Skills/act_int.lua:20983-20987 h=406f34ad
/// `data.skills.WinterOrb.preDamageFunc` (preDamageFunc)
pub(crate) fn WinterOrb_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:20983-20987")
}

/// @pob Data/Skills/act_int.lua:21107-21131 h=c10af8bf
/// `data.skills.WintertideBrand.preDamageFunc` (preDamageFunc)
pub(crate) fn WintertideBrand_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Data/Skills/act_int.lua:21107-21131")
}
