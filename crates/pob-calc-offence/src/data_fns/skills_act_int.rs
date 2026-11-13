//! @pob-file Data/Skills/act_int.lua @16de4b82d
//! Skill hooks of Data/Skills/act_int.lua (calc doc §8: `preDamageFunc`, `initialFunc`, `preSkillTypeFunc`,
//! `postCritFunc`, `explosiveArrowFunc`).
//!
//! Owner: SK-A. Signatures and the registry rows are U0's (frozen; change through a CCR);
//! the bodies are hand-ported from the closures, one Rust fn per Lua closure (AltX/AltY copies
//! are ported as their own fns, each against its own lines).
//!
//! Every hook receives `(activeSkill, output, breakdown)` from `runSkillFunc`
//! (CalcOffence.lua:509-514); closures declared as `function(activeSkill, output)` ignore the
//! third argument. Reads of `skillData`/`output` keys used in arithmetic go through
//! `Table::num` (a nil raises, as the Lua arithmetic does); plain copies (`x = y.k`) copy the
//! `Val`, nil included.
#![allow(
    non_snake_case,
    unused_variables,
    unused_imports,
    unused_assignments,
    unused_mut,
    unused_parens,
    dead_code,
    clippy::all,
    clippy::pedantic
)]

use crate::prelude::*;

use super::{FnRow, SkillFuncImpl};
use crate::lua_table::TableField;
use crate::modules::calc_offence::calcSkillDuration;
use crate::modules::calc_tools as calcLib;
use pob_gamedata::ModType::{BASE, INC, MORE};
use pob_gamedata::{tag, Ramp};
use pob_lua::bit::bor;

// ---- plumbing ---------------------------------------------------------------------------------

/// `a.socketGroup == b.socketGroup`: table identity, and `nil == nil`.
fn same_socket_group(a: &Option<SocketGroupRef>, b: &Option<SocketGroupRef>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// Lua `v ~= <number>` negated: `v == n` without coercion (nil and strings are unequal).
fn num_eq(v: &Val, n: f64) -> bool {
    matches!(v, Val::Num(x) if *x == n)
}

/// `t_insert(v, …)`'s first argument: a table, else Lua's "bad argument #1 to 'insert'".
fn table_of(v: &Val) -> Table {
    v.table().unwrap_or_else(|| {
        lua_error!(
            "bad argument #1 to 'insert' (table expected, got {})",
            v.lua_type()
        )
    })
}

/// `{ type = "DistanceRamp", ramp = { {d0, m0}, {d1, m1}, … } }`.
fn distance_ramp(points: &[[f64; 2]]) -> Tag {
    Tag {
        r#type: TagType::DistanceRamp,
        ramp: Some(Ramp::Points(points.into())),
        ..Default::default()
    }
}

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
    // L553
    let hitTimeOverride = activeSkill.skillData.get().num(n!("repeatFrequency"))
        / (1.0
            + activeSkill.skillModList.get().Sum(
                INC,
                Some(&activeSkill.skillCfg.get()),
                nm!["Speed", "BrandActivationFrequency"],
            ) / 100.0)
        / activeSkill.skillModList.get().More(
            Some(&activeSkill.skillCfg.get()),
            nm!["BrandActivationFrequency"],
        );
    activeSkill
        .skillData
        .get()
        .set(n!("hitTimeOverride"), hitTimeOverride);
}

/// @pob Data/Skills/act_int.lua:715-717 h=557be66a
/// `data.skills.ArmageddonBrand.preDamageFunc` (preDamageFunc)
pub(crate) fn ArmageddonBrand_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L716
    let hitTimeOverride = activeSkill.skillData.get().num(n!("repeatFrequency"))
        / (1.0
            + activeSkill.skillModList.get().Sum(
                INC,
                Some(&activeSkill.skillCfg.get()),
                nm!["Speed", "BrandActivationFrequency"],
            ) / 100.0)
        / activeSkill.skillModList.get().More(
            Some(&activeSkill.skillCfg.get()),
            nm!["BrandActivationFrequency"],
        );
    activeSkill
        .skillData
        .get()
        .set(n!("hitTimeOverride"), hitTimeOverride);
}

/// @pob Data/Skills/act_int.lua:895-897 h=557be66a
/// `data.skills.ArmageddonBrandAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn ArmageddonBrandAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L896
    let hitTimeOverride = activeSkill.skillData.get().num(n!("repeatFrequency"))
        / (1.0
            + activeSkill.skillModList.get().Sum(
                INC,
                Some(&activeSkill.skillCfg.get()),
                nm!["Speed", "BrandActivationFrequency"],
            ) / 100.0)
        / activeSkill.skillModList.get().More(
            Some(&activeSkill.skillCfg.get()),
            nm!["BrandActivationFrequency"],
        );
    activeSkill
        .skillData
        .get()
        .set(n!("hitTimeOverride"), hitTimeOverride);
}

/// @pob Data/Skills/act_int.lua:1086-1167 h=e40e47ed
/// `data.skills.BallLightning.preDamageFunc` (preDamageFunc)
pub(crate) fn BallLightning_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L1087
    let skillCfg = activeSkill.skillCfg.get();
    // L1088
    let skillData = activeSkill.skillData.get();
    // L1089
    let skillFlags = activeSkill.skillFlags.get();
    // L1090
    let skillModList = activeSkill.skillModList.get();
    // L1091-1092: `t_insert` / `s_format` are `Table::push` / `lfmt!`

    // L1094
    let mut dpsMultiplier = 0.0;
    // L1095
    if activeSkill.skillPart.get() == Some(1.0) {
        // Compute DPS changes as if we get exactly 1 strike per ball.
        // L1097
        dpsMultiplier = 1.0;
    } else if activeSkill.skillPart.get() == Some(2.0) {
        // Compute DPS changes accounting for all strikes in range.

        // What's the bolt strike proc rate? Note that the interval is not
        // considered to be a cooldown, so it is unaffected by CDR mods.
        // L1103
        let secsPerStrike = skillData.get(n!("strikeInterval"));
        // How many total bolt strikes proc per ball, ignoring whether the
        // enemy is in range? We assume that the first strike is at the end
        // of the first interval, based on Kitava self-poison testing (no
        // recorded examples of getting 14 self poison stacks with multiple
        // people testing).
        // L1109
        let durationSecs = skillData.get(n!("duration"));
        // L1110
        let maxStrikes = m_floor(durationSecs.num() / secsPerStrike.num());
        // How fast does the ball travel?
        // L1112
        let baseBallDistPerSec = skillData.get(n!("projectileSpeed"));
        // L1113
        let (incSpeedMult, moreSpeedMult) =
            calcLib::mods(&skillModList, Some(&skillCfg), nm!["ProjectileSpeed"]);
        // L1114
        let netSpeedMult = incSpeedMult * moreSpeedMult;
        // L1115
        let ballDistPerSec = baseBallDistPerSec.num() * netSpeedMult;
        // L1116
        let ballDistPerStrike = ballDistPerSec * secsPerStrike.num();
        // How many times does the ball proc a bolt strike while it is in
        // range of the enemy?
        // L1119
        let enemyRadius = lor(
            Val::from_value(&skillModList.Override(Some(&skillCfg), nm!["EnemyRadius"])),
            || Val::Num(skillModList.Sum(BASE, Some(&skillCfg), nm!["EnemyRadius"])),
        );
        // L1120
        let baseStrikeRadius = output.get(n!("AreaOfEffectRadius"));
        // A bolt can hit when its area overlaps any part of the enemy's collision circle.
        // L1122
        let strikeRadius = baseStrikeRadius.num() + enemyRadius.num();
        // L1123
        let mut castDist = 0.0;
        // L1124
        if let Some(skillDist) = skillCfg.skillDist.get() {
            // Advanced users can specify exactly the standoff distance
            // they'll use against single-target bosses.
            // L1127
            castDist = skillDist;
        } else if skillFlags.truthy(n!("triggered")) {
            // Cyclone is the most common trigger skill, and players who
            // aren't min-maxing their playstyle will tend to just
            // cyclone back and forth across the boss instead of
            // hovering at the optimal range. For simplicity, let's
            // assume they tend to be an average of 1 normal bolt strike
            // radius away as they do this.
            // L1135
            castDist = m_floor(baseStrikeRadius.num() / 2.0);
        } else {
            // Be nice and assume hand-casters are at the optimal
            // distance for normal bolt strikes.
            // L1139
            castDist = baseStrikeRadius.num();
        }
        // L1141
        let firstStrikeIdxThatHits = m_max(
            1.0, // 1 not 0 here: strike seems to happen at the end of the interval, not start
            m_ceil((castDist - strikeRadius) / ballDistPerStrike),
        );
        // L1144
        let mut lastStrikeIdxThatHits = m_floor(
            m_min(data().misc.ProjectileDistanceCap, castDist + strikeRadius) / ballDistPerStrike,
        );
        // L1145
        let numStrikes = m_max(
            0.0,
            m_min(
                maxStrikes,
                lastStrikeIdxThatHits + 1.0 - firstStrikeIdxThatHits,
            ),
        );
        // L1146
        lastStrikeIdxThatHits = firstStrikeIdxThatHits + numStrikes - 1.0;

        // L1148
        dpsMultiplier = numStrikes;

        // L1150
        if let Some(breakdown) = breakdown {
            // L1151
            let breakdownHits = Table::new();
            // L1152
            breakdownHits.push(lfmt!(
                "^8Balls travel at^7 %.2f^8 units/sec.",
                ballDistPerSec
            ));
            // L1153
            breakdownHits.push(lfmt!(
                "^8Lightning bolts strike all nearby enemies every^7 %.2f^8 seconds (^7%.2f^8 strikes/sec).",
                secsPerStrike.num(),
                1.0 / secsPerStrike.num()
            ));
            // L1154
            breakdownHits.push(lfmt!(
                "^8Balls travel^7 %.2f^8 units between each bolt strike.",
                ballDistPerStrike
            ));
            // L1155
            breakdownHits.push(lfmt!(
                "^8Assumes balls are cast^7 %d^8 units from the enemy.",
                castDist
            ));
            // L1156
            breakdownHits.push(lfmt!(
                "^8Balls can strike enemies up to^7 %d^8 units away from themselves.",
                strikeRadius
            ));
            // L1157
            breakdownHits.push(lfmt!(
                "^8The first strike is at^7 %.2f^8 seconds after it is cast, when the ball is^7 %d^8 units from the cast point.",
                firstStrikeIdxThatHits * secsPerStrike.num(),
                firstStrikeIdxThatHits * ballDistPerStrike
            ));
            // L1158
            breakdownHits.push(lfmt!(
                "^8The last strike is at^7 %.2f^8 seconds after it is cast, when the ball is^7 %d^8 units from the cast point.",
                lastStrikeIdxThatHits * secsPerStrike.num(),
                lastStrikeIdxThatHits * ballDistPerStrike
            ));
            // L1159
            output.set(n!("NormalHitsPerCast"), numStrikes);
            // L1160
            breakdown.set(n!("NormalHitsPerCast"), breakdownHits);
        }
    }
    // L1163
    if dpsMultiplier != 1.0 {
        // L1164
        let v = skillData.num_or(n!("dpsMultiplier"), 1.0) * dpsMultiplier;
        skillData.set(n!("dpsMultiplier"), v);
        // L1165
        let v = output.num_or(n!("SkillDPSMultiplier"), 1.0) * dpsMultiplier;
        output.set(n!("SkillDPSMultiplier"), v);
    }
}

/// @pob Data/Skills/act_int.lua:1252-1275 h=cecfa725
/// `data.skills.BallLightningAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn BallLightningAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L1253: `s_format` is `lfmt!`
    // L1254
    let mut dpsMultiplier = 1.0;
    // L1255
    if activeSkill.skillPart.get() == Some(2.0) || activeSkill.skillPart.get() == Some(3.0) {
        // L1257
        let skillData = activeSkill.skillData.get();
        // L1258
        let secsPerStrike = skillData.get(n!("strikeInterval"));
        // L1259
        let durationSecs = skillData.get(n!("duration"));
        // L1260
        let numStrikes = m_floor(durationSecs.num() / secsPerStrike.num());

        // L1262
        dpsMultiplier = numStrikes;
        // L1263
        if dpsMultiplier != 1.0 {
            // L1264
            let v = skillData.num_or(n!("dpsMultiplier"), 1.0) * dpsMultiplier;
            skillData.set(n!("dpsMultiplier"), v);
            // L1265
            let v = output.num_or(n!("SkillDPSMultiplier"), 1.0) * dpsMultiplier;
            output.set(n!("SkillDPSMultiplier"), v);
        }
        // L1267
        output.set(n!("NormalHitsPerCast"), numStrikes);
        // L1268
        if let Some(breakdown) = breakdown {
            // L1269
            let t = Table::from_ctor(vec![
                TableField::item(
                    false,
                    lfmt!(
                        "^8Lightning bolts strike all nearby enemies every^7 %.2f^8 seconds (^7%.2f^8 strikes/sec).",
                        secsPerStrike.num(),
                        1.0 / secsPerStrike.num()
                    ),
                ),
                TableField::item(
                    false,
                    lfmt!(
                        "^8Balls lasts for ^7%d^8 seconds for a total of ^7%d^8 strikes.",
                        durationSecs.num(),
                        numStrikes
                    ),
                ),
            ]);
            breakdown.set(n!("NormalHitsPerCast"), t);
        }
    }
}

/// @pob Data/Skills/act_int.lua:1375-1393 h=98a21d3a
/// `data.skills.BallLightningAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn BallLightningAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L1376: `s_format` is `lfmt!`

    // L1378
    let skillData = activeSkill.skillData.get();
    // L1379
    let secsPerStrike = skillData.get(n!("strikeInterval"));
    // L1380
    let durationSecs = skillData.get(n!("duration"));
    // L1381
    let numStrikes = m_floor(durationSecs.num() / secsPerStrike.num());

    // L1383
    let v = skillData.num_or(n!("dpsMultiplier"), 1.0) * numStrikes;
    skillData.set(n!("dpsMultiplier"), v);
    // L1384
    output.set(n!("NormalHitsPerCast"), numStrikes);
    // L1385
    let v = output.num_or(n!("SkillDPSMultiplier"), 1.0) * numStrikes;
    output.set(n!("SkillDPSMultiplier"), v);

    // L1387
    if let Some(breakdown) = breakdown {
        // L1388
        let t = Table::from_ctor(vec![
            TableField::item(
                false,
                lfmt!(
                    "^8Lightning bolts strike all nearby enemies every^7 %.2f^8 seconds (^7%.2f^8 strikes/sec).",
                    secsPerStrike.num(),
                    1.0 / secsPerStrike.num()
                ),
            ),
            TableField::item(
                false,
                lfmt!(
                    "^8Balls lasts for ^7%d^8 seconds for a total of ^7%d^8 strikes.",
                    durationSecs.num(),
                    numStrikes
                ),
            ),
        ]);
        breakdown.set(n!("NormalHitsPerCast"), t);
    }
}

/// @pob Data/Skills/act_int.lua:1470-1481 h=e0d40709
/// `data.skills.Bane.preSkillTypeFunc` (preSkillTypeFunc)
pub(crate) fn Bane_preSkillTypeFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L1471
    let mut curseCount = 0.0;
    // L1472
    let actor = activeSkill
        .actor()
        .unwrap_or_else(|| lua_error!("attempt to index field 'actor' (a nil value)"));
    for_ipairs!((_, skill) in actor.activeSkillList.get() => {
        // L1473
        if same_socket_group(&skill.socketGroup.get(), &activeSkill.socketGroup.get())
            && skill
                .skillModList
                .get()
                .GetCondition(n!("AppliedByBane"), None, false)
        {
            // L1474
            curseCount += 1.0;
            // L1475
            if matches!(output.get(n!("EnemyCurseLimit")), Val::Num(x) if x == curseCount) {
                // L1476
                break;
            }
        }
    });
    // L1480
    activeSkill.skillModList.get().NewMod(
        n!("Multiplier:CurseApplied"),
        BASE,
        curseCount,
        "Base",
        0,
        0,
        &[],
    );
}

/// @pob Data/Skills/act_int.lua:1647-1658 h=e0d40709
/// `data.skills.BaneAltX.preSkillTypeFunc` (preSkillTypeFunc)
pub(crate) fn BaneAltX_preSkillTypeFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L1648
    let mut curseCount = 0.0;
    // L1649
    let actor = activeSkill
        .actor()
        .unwrap_or_else(|| lua_error!("attempt to index field 'actor' (a nil value)"));
    for_ipairs!((_, skill) in actor.activeSkillList.get() => {
        // L1650
        if same_socket_group(&skill.socketGroup.get(), &activeSkill.socketGroup.get())
            && skill
                .skillModList
                .get()
                .GetCondition(n!("AppliedByBane"), None, false)
        {
            // L1651
            curseCount += 1.0;
            // L1652
            if matches!(output.get(n!("EnemyCurseLimit")), Val::Num(x) if x == curseCount) {
                // L1653
                break;
            }
        }
    });
    // L1657
    activeSkill.skillModList.get().NewMod(
        n!("Multiplier:CurseApplied"),
        BASE,
        curseCount,
        "Base",
        0,
        0,
        &[],
    );
}

/// @pob Data/Skills/act_int.lua:1823-1827 h=d4206938
/// `data.skills.BlazingSalvo.preDamageFunc` (preDamageFunc)
pub(crate) fn BlazingSalvo_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L1824
    if activeSkill.skillPart.get() == Some(2.0) {
        // L1825
        let skillData = activeSkill.skillData.get();
        let v = skillData.num_or(n!("dpsMultiplier"), 1.0) * output.num(n!("ProjectileCount"));
        skillData.set(n!("dpsMultiplier"), v);
    }
}

/// @pob Data/Skills/act_int.lua:2298-2309 h=026d581b
/// `data.skills.Bodyswap.preDamageFunc` (preDamageFunc)
pub(crate) fn Bodyswap_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L2299
    if activeSkill.skillPart.get() == Some(1.0) {
        // L2300
        let skillData = activeSkill.skillData.get();
        // L2301
        if activeSkill.skillFlags.get().truthy(n!("totem")) {
            // L2302
            let v =
                output.num(n!("TotemLife")) * skillData.num(n!("selfFireExplosionLifeMultiplier"));
            skillData.set(n!("FireBonusMin"), v);
            // L2303
            let v =
                output.num(n!("TotemLife")) * skillData.num(n!("selfFireExplosionLifeMultiplier"));
            skillData.set(n!("FireBonusMax"), v);
        } else {
            // L2305
            let v = output.num(n!("Life")) * skillData.num(n!("selfFireExplosionLifeMultiplier"));
            skillData.set(n!("FireBonusMin"), v);
            // L2306
            let v = output.num(n!("Life")) * skillData.num(n!("selfFireExplosionLifeMultiplier"));
            skillData.set(n!("FireBonusMax"), v);
        }
    }
}

/// @pob Data/Skills/act_int.lua:2413-2424 h=026d581b
/// `data.skills.BodyswapAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn BodyswapAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L2414
    if activeSkill.skillPart.get() == Some(1.0) {
        // L2415
        let skillData = activeSkill.skillData.get();
        // L2416
        if activeSkill.skillFlags.get().truthy(n!("totem")) {
            // L2417
            let v =
                output.num(n!("TotemLife")) * skillData.num(n!("selfFireExplosionLifeMultiplier"));
            skillData.set(n!("FireBonusMin"), v);
            // L2418
            let v =
                output.num(n!("TotemLife")) * skillData.num(n!("selfFireExplosionLifeMultiplier"));
            skillData.set(n!("FireBonusMax"), v);
        } else {
            // L2420
            let v = output.num(n!("Life")) * skillData.num(n!("selfFireExplosionLifeMultiplier"));
            skillData.set(n!("FireBonusMin"), v);
            // L2421
            let v = output.num(n!("Life")) * skillData.num(n!("selfFireExplosionLifeMultiplier"));
            skillData.set(n!("FireBonusMax"), v);
        }
    }
}

/// @pob Data/Skills/act_int.lua:3889-3903 h=ccc82361
/// `data.skills.DarkPact.preDamageFunc` (preDamageFunc)
pub(crate) fn DarkPact_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L3890
    let life: Val;
    // L3891
    if activeSkill.skillPart.get() == Some(1.0) {
        // L3892
        if activeSkill.skillFlags.get().truthy(n!("totem")) {
            // L3893
            life = output.get(n!("TotemLife"));
        } else {
            // L3895
            life = output.get(n!("Life"));
        }
    } else {
        // L3898
        life = Val::Num(activeSkill.skillData.get().num_or(n!("skeletonLife"), 0.0));
    }
    // L3900
    let add = life.num() * activeSkill.skillData.get().num(n!("lifeDealtAsChaos")) / 100.0;
    // L3901
    let skillData = activeSkill.skillData.get();
    let v = skillData.num(n!("ChaosMin")) + add;
    skillData.set(n!("ChaosMin"), v);
    // L3902
    let v = skillData.num(n!("ChaosMax")) + add;
    skillData.set(n!("ChaosMax"), v);
}

/// @pob Data/Skills/act_int.lua:4006-4022 h=b3f64de4
/// `data.skills.DarkPactAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn DarkPactAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L4007
    let life: Val;
    // L4008
    if activeSkill.skillFlags.get().truthy(n!("totem")) {
        // L4009
        life = output.get(n!("TotemLife"));
    } else {
        // L4011
        life = output.get(n!("Life"));
    }
    // L4013
    let skillData = activeSkill.skillData.get();
    let add = life.num() * skillData.num(n!("percentageLifeSacrificed")) / 100.0
        * (skillData.num(n!("percentageSacrificedDealtAsChaos")) / 100.0);
    // L4014
    if activeSkill.skillPart.get() == Some(2.0) {
        // L4015
        let v = skillData.num(n!("ChaosMin")) + m_floor(add);
        skillData.set(n!("ChaosMin"), v);
        // L4016
        let v = skillData.num(n!("ChaosMax")) + m_floor(add);
        skillData.set(n!("ChaosMax"), v);
    } else {
        // L4018
        let avgCastsTillRuin =
            7.0 / (1.0 + m_min(100.0, skillData.num_or(n!("additionalRuinChance"), 0.0)) / 100.0);
        // L4019
        let v = skillData.num(n!("ChaosMin")) + m_floor(add / avgCastsTillRuin);
        skillData.set(n!("ChaosMin"), v);
        // L4020
        let v = skillData.num(n!("ChaosMax")) + m_floor(add / avgCastsTillRuin);
        skillData.set(n!("ChaosMax"), v);
    }
}

/// @pob Data/Skills/act_int.lua:4602-4606 h=d7eac377
/// `data.skills.DivineIre.preDamageFunc` (preDamageFunc)
pub(crate) fn DivineIre_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L4603
    if activeSkill.skillPart.get() == Some(2.0) {
        // L4604
        let v = activeSkill.skillModList.get().Sum(
            BASE,
            Some(&activeSkill.skillCfg.get()),
            nm!["Multiplier:DivineIreStage"],
        );
        activeSkill.skillData.get().set(n!("hitTimeMultiplier"), v);
    }
}

/// @pob Data/Skills/act_int.lua:4712-4716 h=9ea5c810
/// `data.skills.DivineIreAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn DivineIreAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L4713
    if activeSkill.skillPart.get() == Some(2.0) {
        // L4714
        let v = activeSkill.skillModList.get().Sum(
            BASE,
            Some(&activeSkill.skillCfg.get()),
            nm!["Multiplier:DivineIreofHolyLightningStage"],
        );
        activeSkill.skillData.get().set(n!("hitTimeMultiplier"), v);
    }
}

/// @pob Data/Skills/act_int.lua:4823-4825 h=6b2c2529
/// `data.skills.DivineIreAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn DivineIreAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L4824
    let v = activeSkill.skillModList.get().Sum(
        BASE,
        Some(&activeSkill.skillCfg.get()),
        nm!["Multiplier:DivineIreofDisintegrationStage"],
    );
    activeSkill.skillData.get().set(n!("hitTimeMultiplier"), v);
}

/// @pob Data/Skills/act_int.lua:5518-5520 h=01b5264c
/// `data.skills.EyeOfWinter.preDamageFunc` (preDamageFunc)
pub(crate) fn EyeOfWinter_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L5519
    let v = activeSkill.skillModList.get().Sum(
        BASE,
        Some(&activeSkill.skillCfg.get()),
        nm!["EyeOfWinterRamp"],
    );
    activeSkill.skillModList.get().NewMod(
        n!("Damage"),
        MORE,
        v,
        "Skill:EyeOfWinter",
        0,
        0,
        &[distance_ramp(&[
            [0.0, 0.0],
            [60.0 * output.num(n!("ProjectileSpeedMod")), 1.0],
        ])],
    );
}

/// @pob Data/Skills/act_int.lua:5611-5613 h=eea88caf
/// `data.skills.EyeOfWinterAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn EyeOfWinterAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L5612
    let v = activeSkill.skillModList.get().Sum(
        BASE,
        Some(&activeSkill.skillCfg.get()),
        nm!["EyeOfWinterRamp"],
    );
    activeSkill.skillModList.get().NewMod(
        n!("Damage"),
        MORE,
        v,
        "Skill:EyeOfWinterAltX",
        0,
        0,
        &[distance_ramp(&[
            [0.0, 0.0],
            [60.0 * output.num(n!("ProjectileSpeedMod")), 1.0],
        ])],
    );
}

/// @pob Data/Skills/act_int.lua:5704-5706 h=a10d566f
/// `data.skills.EyeOfWinterAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn EyeOfWinterAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L5705
    let v = activeSkill.skillModList.get().Sum(
        BASE,
        Some(&activeSkill.skillCfg.get()),
        nm!["EyeOfWinterRamp"],
    );
    activeSkill.skillModList.get().NewMod(
        n!("Damage"),
        MORE,
        v,
        "Skill:EyeOfWinterAltY",
        0,
        0,
        &[distance_ramp(&[
            [0.0, 0.0],
            [60.0 * output.num(n!("ProjectileSpeedMod")), 1.0],
        ])],
    );
}

/// @pob Data/Skills/act_int.lua:6800-6802 h=68deb3ee
/// `data.skills.Flameblast.preDamageFunc` (preDamageFunc)
pub(crate) fn Flameblast_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L6801
    let v = m_max(
        activeSkill.skillModList.get().Sum(
            BASE,
            Some(&activeSkill.skillCfg.get()),
            nm!["Multiplier:FlameblastStage"],
        ) - activeSkill.skillModList.get().Sum(
            BASE,
            Some(&activeSkill.skillCfg.get()),
            nm!["Multiplier:FlameblastMinimumStage"],
        ),
        1.0,
    );
    activeSkill.skillData.get().set(n!("hitTimeMultiplier"), v);
}

/// @pob Data/Skills/act_int.lua:6905-6907 h=78160b6c
/// `data.skills.FlameblastAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn FlameblastAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L6906
    let v = m_max(
        activeSkill.skillModList.get().Sum(
            BASE,
            Some(&activeSkill.skillCfg.get()),
            nm!["Multiplier:FlameblastofCelerityStage"],
        ) - activeSkill.skillModList.get().Sum(
            BASE,
            Some(&activeSkill.skillCfg.get()),
            nm!["Multiplier:FlameblastMinimumStage"],
        ),
        1.0,
    );
    activeSkill.skillData.get().set(n!("hitTimeMultiplier"), v);
}

/// @pob Data/Skills/act_int.lua:7008-7010 h=a159b7f4
/// `data.skills.FlameblastAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn FlameblastAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L7009
    let v = m_max(
        activeSkill.skillModList.get().Sum(
            BASE,
            Some(&activeSkill.skillCfg.get()),
            nm!["Multiplier:FlameblastofContractionStage"],
        ) - activeSkill.skillModList.get().Sum(
            BASE,
            Some(&activeSkill.skillCfg.get()),
            nm!["Multiplier:FlameblastMinimumStage"],
        ),
        1.0,
    );
    activeSkill.skillData.get().set(n!("hitTimeMultiplier"), v);
}

/// @pob Data/Skills/act_int.lua:7390-7434 h=f7f48a4b
/// `data.skills.ForbiddenRite.preDamageFunc` (preDamageFunc)
pub(crate) fn ForbiddenRite_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L7391
    let add: f64;
    // L7392-7393: `t_insert` / `s_format` are `Table::push` / `lfmt!`
    // L7394
    let basetakenFlat = activeSkill.skillModList.get().Sum(
        BASE,
        None,
        nm![
            "DamageTaken",
            "ChaosDamageTaken",
            "DamageTakenWhenHit",
            "ChaosDamageTakenWhenHit"
        ],
    );
    // L7395
    let baseTakenInc = activeSkill.skillModList.get().Sum(
        INC,
        None,
        nm![
            "DamageTaken",
            "ChaosDamageTaken",
            "DamageTakenWhenHit",
            "ChaosDamageTakenWhenHit"
        ],
    );
    // L7396
    let baseTakenMore = activeSkill.skillModList.get().More(
        None,
        nm![
            "DamageTaken",
            "ChaosDamageTaken",
            "DamageTakenWhenHit",
            "ChaosDamageTakenWhenHit"
        ],
    );
    // L7397
    let chaosDamageTaken = m_max((1.0 + baseTakenInc / 100.0) * baseTakenMore, 0.0);
    // L7398
    let chaosFlat = floor(round(basetakenFlat * chaosDamageTaken, None), Some(0));
    // L7399
    let life: Val;
    let energyShield: Val;
    let chaosResistance: Val;
    let SelfDamageTakenLife: f64;
    let SelfDamageTakenES: f64;
    // L7400
    if activeSkill.skillFlags.get().truthy(n!("totem")) {
        // L7401
        life = output.get(n!("TotemLife"));
        // L7402
        energyShield = output.get(n!("TotemEnergyShield"));
        // L7403
        chaosResistance = output.get(n!("TotemChaosResist"));
    } else {
        // L7405
        life = output.get(n!("Life"));
        // L7406
        energyShield = output.get(n!("EnergyShield"));
        // L7407
        chaosResistance = output.get(n!("ChaosResist"));
    }
    let skillData = activeSkill.skillData.get();
    // L7409
    add = life.num() * skillData.num(n!("lifeDealtAsChaos"))
        + energyShield.num() * skillData.num(n!("energyShieldDealtAsChaos"));
    // L7410
    SelfDamageTakenLife = floor(
        round(life.num() * skillData.num(n!("SelfDamageTakenLife")), None)
            * (100.0 - chaosResistance.num())
            / 100.0
            * chaosDamageTaken,
        None,
    );
    // L7411
    SelfDamageTakenES = floor(
        round(
            energyShield.num() * skillData.num(n!("SelfDamageTakenES")),
            None,
        ) * (100.0 - chaosResistance.num())
            / 100.0
            * chaosDamageTaken,
        None,
    );
    // L7412
    let v = skillData.num(n!("ChaosMin")) + add;
    skillData.set(n!("ChaosMin"), v);
    // L7413
    let v = skillData.num(n!("ChaosMax")) + add;
    skillData.set(n!("ChaosMax"), v);
    // L7414
    if activeSkill.skillPart.get() == Some(2.0) {
        // L7415
        let v =
            skillData.num_or(n!("dpsMultiplier"), 1.0) * (output.num(n!("ProjectileCount")) + 1.0);
        skillData.set(n!("dpsMultiplier"), v);
    }
    // L7417
    output.set(
        n!("FRDamageTaken"),
        SelfDamageTakenLife + SelfDamageTakenES + chaosFlat,
    );
    // L7418
    if let Some(breakdown) = breakdown {
        // L7419
        let FRDamageTaken = Table::new();
        // L7420
        FRDamageTaken.push(lfmt!(
            "Damage Taken per cast from Forbidden Rite: %d",
            output.num(n!("FRDamageTaken"))
        ));
        // L7421
        FRDamageTaken.push(lfmt!(
            "^8=^7 %d^8 (Life) *^7 %d%%^8 (of Life taken as Chaos Damage)",
            life.num(),
            skillData.num(n!("SelfDamageTakenLife")) * 100.0
        ));
        // L7422
        if !num_eq(&energyShield, 0.0) {
            // L7423
            FRDamageTaken.push(lfmt!(
                "^8+^7 %d^8 (ES) *^7 %d%%^8 (of ES taken as Chaos Damage)",
                energyShield.num(),
                skillData.num(n!("SelfDamageTakenES")) * 100.0
            ));
        }
        // L7425
        FRDamageTaken.push(lfmt!(
            "^8=^7 %d^8 (Chaos Damage) *^7 %d%%^8 (Chaos Resistance)",
            life.num() * skillData.num(n!("SelfDamageTakenLife"))
                + energyShield.num() * skillData.num(n!("SelfDamageTakenES")),
            chaosResistance.num()
        ));
        // L7426
        if chaosFlat != 0.0 {
            // L7427
            FRDamageTaken.push(lfmt!("^8 -^7 %d^8 (Flat Damage reduction)", -basetakenFlat));
        }
        // L7429
        if chaosDamageTaken != 1.0 {
            // L7430
            FRDamageTaken.push(lfmt!(
                "^8 *^7 %.2f^8 (Damage taken Multiplier)",
                chaosDamageTaken
            ));
        }
        // L7432
        breakdown.set(n!("FRDamageTaken"), FRDamageTaken);
    }
}

/// @pob Data/Skills/act_int.lua:7549-7589 h=58a90593
/// `data.skills.ForbiddenRiteAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn ForbiddenRiteAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L7550
    let add: f64;
    // L7551-7552: `t_insert` / `s_format` are `Table::push` / `lfmt!`
    // L7553
    let basetakenFlat = activeSkill.skillModList.get().Sum(
        BASE,
        None,
        nm![
            "DamageTaken",
            "ChaosDamageTaken",
            "DamageTakenWhenHit",
            "ChaosDamageTakenWhenHit"
        ],
    );
    // L7554
    let baseTakenInc = activeSkill.skillModList.get().Sum(
        INC,
        None,
        nm![
            "DamageTaken",
            "ChaosDamageTaken",
            "DamageTakenWhenHit",
            "ChaosDamageTakenWhenHit"
        ],
    );
    // L7555
    let baseTakenMore = activeSkill.skillModList.get().More(
        None,
        nm![
            "DamageTaken",
            "ChaosDamageTaken",
            "DamageTakenWhenHit",
            "ChaosDamageTakenWhenHit"
        ],
    );
    // L7556
    let chaosDamageTaken = m_max((1.0 + baseTakenInc / 100.0) * baseTakenMore, 0.0);
    // L7557
    let chaosFlat = floor(round(basetakenFlat * chaosDamageTaken, None), Some(0));
    // L7558
    let energyShield: Val;
    let chaosResistance: Val;
    let SelfDamageTakenES: f64;
    // L7559
    if activeSkill.skillFlags.get().truthy(n!("totem")) {
        // L7560
        energyShield = output.get(n!("TotemEnergyShield"));
        // L7561
        chaosResistance = output.get(n!("TotemChaosResist"));
    } else {
        // L7563
        energyShield = output.get(n!("EnergyShield"));
        // L7564
        chaosResistance = output.get(n!("ChaosResist"));
    }
    let skillData = activeSkill.skillData.get();
    // L7566
    add = energyShield.num() * skillData.num(n!("energyShieldDealtAsChaos"));
    // L7567
    SelfDamageTakenES = floor(
        round(
            energyShield.num() * skillData.num(n!("SelfDamageTakenES")),
            None,
        ) * (100.0 - chaosResistance.num())
            / 100.0
            * chaosDamageTaken,
        None,
    );
    // L7568
    let v = skillData.num(n!("ChaosMin")) + add;
    skillData.set(n!("ChaosMin"), v);
    // L7569
    let v = skillData.num(n!("ChaosMax")) + add;
    skillData.set(n!("ChaosMax"), v);
    // L7570
    if activeSkill.skillPart.get() == Some(2.0) {
        // L7571
        let v =
            skillData.num_or(n!("dpsMultiplier"), 1.0) * (output.num(n!("ProjectileCount")) + 1.0);
        skillData.set(n!("dpsMultiplier"), v);
    }
    // L7573
    output.set(n!("FRDamageTaken"), SelfDamageTakenES + chaosFlat);
    // L7574
    if let Some(breakdown) = breakdown {
        // L7575
        let FRDamageTaken = Table::new();
        // L7576
        FRDamageTaken.push(lfmt!(
            "Damage Taken per cast from Forbidden Rite: %d",
            output.num(n!("FRDamageTaken"))
        ));
        // L7577
        if !num_eq(&energyShield, 0.0) {
            // L7578
            FRDamageTaken.push(lfmt!(
                "^8+^7 %d^8 (ES) *^7 %d%%^8 (of ES taken as Chaos Damage)",
                energyShield.num(),
                skillData.num(n!("SelfDamageTakenES")) * 100.0
            ));
        }
        // L7580
        FRDamageTaken.push(lfmt!(
            "^8=^7 %d^8 (Chaos Damage) *^7 %d%%^8 (Chaos Resistance)",
            energyShield.num() * skillData.num(n!("SelfDamageTakenES")),
            chaosResistance.num()
        ));
        // L7581
        if chaosFlat != 0.0 {
            // L7582
            FRDamageTaken.push(lfmt!("^8 -^7 %d^8 (Flat Damage reduction)", -basetakenFlat));
        }
        // L7584
        if chaosDamageTaken != 1.0 {
            // L7585
            FRDamageTaken.push(lfmt!(
                "^8 *^7 %.2f^8 (Damage taken Multiplier)",
                chaosDamageTaken
            ));
        }
        // L7587
        breakdown.set(n!("FRDamageTaken"), FRDamageTaken);
    }
}

/// @pob Data/Skills/act_int.lua:7686-7689 h=55c95af4
/// `data.skills.FreezingPulse.preDamageFunc` (preDamageFunc)
pub(crate) fn FreezingPulse_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L7687
    activeSkill.skillModList.get().NewMod(
        n!("Damage"),
        MORE,
        -50.0,
        "Skill:FreezingPulse",
        0,
        0,
        &[distance_ramp(&[
            [0.0, 0.0],
            [60.0 * output.num(n!("ProjectileSpeedMod")), 1.0],
        ])],
    );
    // L7688
    activeSkill.skillModList.get().NewMod(
        n!("EnemyFreezeChance"),
        BASE,
        25.0,
        "Skill:FreezingPulse",
        0,
        0,
        &[distance_ramp(&[
            [0.0, 1.0],
            [15.0 * output.num(n!("ProjectileSpeedMod")), 0.0],
        ])],
    );
}

/// @pob Data/Skills/act_int.lua:7933-7936 h=46e40c24
/// `data.skills.FrostBombAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn FrostBombAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L7934
    let duration = m_floor(
        activeSkill.skillData.get().num(n!("duration")) * output.num(n!("DurationMod")) * 10.0,
    );
    // L7935
    activeSkill.skillModList.get().NewMod(
        n!("Multiplier:100msFrostBombDuration"),
        BASE,
        duration,
        "Skill:FrostBombAltY",
        0,
        0,
        &[],
    );
}

/// @pob Data/Skills/act_int.lua:8543-8545 h=852e25cb
/// `data.skills.GalvanicField.preDamageFunc` (preDamageFunc)
pub(crate) fn GalvanicField_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L8544
    let skillData = activeSkill.skillData.get();
    let v = skillData.get(n!("repeatFrequency"));
    skillData.set(n!("hitTimeOverride"), v);
}

/// @pob Data/Skills/act_int.lua:8663-8665 h=852e25cb
/// `data.skills.GalvanicFieldAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn GalvanicFieldAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L8664
    let skillData = activeSkill.skillData.get();
    let v = skillData.get(n!("repeatFrequency"));
    skillData.set(n!("hitTimeOverride"), v);
}

/// @pob Data/Skills/act_int.lua:8972-8976 h=59e1d7b4
/// `data.skills.Hydrosphere.preDamageFunc` (preDamageFunc)
pub(crate) fn Hydrosphere_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L8973
    if activeSkill.skillPart.get() == Some(1.0)
        || activeSkill.skillPart.get() == Some(2.0)
        || activeSkill.skillPart.get() == Some(3.0)
    {
        // L8974
        let v = activeSkill.skillData.get().num(n!("repeatFrequency"))
            / (1.0
                + activeSkill.skillModList.get().Sum(
                    INC,
                    Some(&activeSkill.skillCfg.get()),
                    nm!["HydroSphereFrequency"],
                ) / 100.0);
        activeSkill.skillData.get().set(n!("hitTimeOverride"), v);
    }
}

/// @pob Data/Skills/act_int.lua:9383-9385 h=7b53bbcc
/// `data.skills.HeraldOfThunder.preDamageFunc` (preDamageFunc)
pub(crate) fn HeraldOfThunder_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L9384
    let v = activeSkill.skillData.get().num(n!("repeatFrequency"))
        / (1.0
            + activeSkill.skillModList.get().Sum(
                INC,
                Some(&activeSkill.skillCfg.get()),
                nm!["HeraldStormFrequency"],
            ) / 100.0);
    activeSkill.skillData.get().set(n!("hitTimeOverride"), v);
}

/// @pob Data/Skills/act_int.lua:9805-9809 h=81d50744
/// `data.skills.IceSpear.preDamageFunc` (preDamageFunc)
pub(crate) fn IceSpear_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L9806
    if activeSkill.skillPart.get() == Some(3.0) || activeSkill.skillPart.get() == Some(4.0) {
        // L9807
        let skillData = activeSkill.skillData.get();
        let v = skillData.num_or(n!("dpsMultiplier"), 1.0) * output.num(n!("ProjectileCount"));
        skillData.set(n!("dpsMultiplier"), v);
    }
}

/// @pob Data/Skills/act_int.lua:9915-9919 h=81d50744
/// `data.skills.IceSpearAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn IceSpearAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L9916
    if activeSkill.skillPart.get() == Some(3.0) || activeSkill.skillPart.get() == Some(4.0) {
        // L9917
        let skillData = activeSkill.skillData.get();
        let v = skillData.num_or(n!("dpsMultiplier"), 1.0) * output.num(n!("ProjectileCount"));
        skillData.set(n!("dpsMultiplier"), v);
    }
}

/// @pob Data/Skills/act_int.lua:10284-10288 h=9fd10d63
/// `data.skills.Incinerate.preDamageFunc` (preDamageFunc)
pub(crate) fn Incinerate_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L10285
    if activeSkill.skillPart.get() == Some(2.0) {
        // L10286
        let v = m_max(
            activeSkill.skillModList.get().Sum(
                BASE,
                Some(&activeSkill.skillCfg.get()),
                nm!["Multiplier:IncinerateStage"],
            ) - activeSkill.skillModList.get().Sum(
                BASE,
                Some(&activeSkill.skillCfg.get()),
                nm!["Multiplier:IncinerateMinimumStage"],
            ) - 0.4175,
            0.5825,
        ); //First stage takes 0.5825x time to channel compared to subsequent stages
        activeSkill.skillData.get().set(n!("hitTimeMultiplier"), v);
    }
}

/// @pob Data/Skills/act_int.lua:10422-10426 h=d0206498
/// `data.skills.IncinerateAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn IncinerateAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L10423
    if activeSkill.skillPart.get() == Some(2.0) {
        // L10424
        let v = m_max(
            activeSkill.skillModList.get().Sum(
                BASE,
                Some(&activeSkill.skillCfg.get()),
                nm!["Multiplier:IncinerateofExpanseStage"],
            ) - activeSkill.skillModList.get().Sum(
                BASE,
                Some(&activeSkill.skillCfg.get()),
                nm!["Multiplier:IncinerateMinimumStage"],
            ) - 0.4175,
            0.5825,
        ); //First stage takes 0.5825x time to channel compared to subsequent stages
        activeSkill.skillData.get().set(n!("hitTimeMultiplier"), v);
    }
}

/// @pob Data/Skills/act_int.lua:10561-10565 h=a72c7761
/// `data.skills.IncinerateAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn IncinerateAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L10562
    if activeSkill.skillPart.get() == Some(2.0) {
        // L10563
        let v = m_max(
            activeSkill.skillModList.get().Sum(
                BASE,
                Some(&activeSkill.skillCfg.get()),
                nm!["Multiplier:IncinerateofVentingStage"],
            ) - activeSkill.skillModList.get().Sum(
                BASE,
                Some(&activeSkill.skillCfg.get()),
                nm!["Multiplier:IncinerateMinimumStage"],
            ) - 0.4175,
            0.5825,
        ); //First stage takes 0.5825x time to channel compared to subsequent stages
        activeSkill.skillData.get().set(n!("hitTimeMultiplier"), v);
    }
}

/// @pob Data/Skills/act_int.lua:11009-11045 h=bc5e2942
/// `data.skills.KineticFusillade.preDamageFunc` (preDamageFunc)
pub(crate) fn KineticFusillade_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L11010
    let skillData = activeSkill.skillData.get();
    // L11011-11012: `t_insert` / `s_format` are `Table::push` / `lfmt!`

    // L11014
    if activeSkill.skillPart.get() == Some(1.0) {
        // Set base dpsMultiplier for projectile count
        // L11016
        activeSkill
            .skillData
            .get()
            .set(n!("dpsMultiplier"), output.get(n!("ProjectileCount")));

        // Calculate average damage scaling for sequential projectiles
        // Each projectile does more damage based on how many came before it
        // L11020
        let moreDamagePerProj = skillData.num_or(n!("damagePerProjectile"), 0.0);
        // L11021
        if moreDamagePerProj != 0.0 && output.num(n!("ProjectileCount")) > 1.0 {
            // Average multiplier: sum of (0, X, 2X, 3X, ..., (n-1)X) / n
            // This equals: X * (0 + 1 + 2 + ... + (n-1)) / n = X * n(n-1)/2 / n = X * (n-1)/2
            // L11024
            let avgMoreMult = moreDamagePerProj * (output.num(n!("ProjectileCount")) - 1.0) / 2.0;
            // L11025
            activeSkill.skillModList.get().NewMod(
                n!("Damage"),
                MORE,
                avgMoreMult,
                "Skill:KineticFusillade",
                0,
                0,
                &[],
            );

            // Store the average multiplier for display
            // L11028
            output.set(n!("KineticFusilladeAvgMoreMult"), avgMoreMult);

            // L11030
            if let Some(breakdown) = breakdown {
                // L11031
                let breakdownSequential = Table::new();
                // L11032
                breakdownSequential.push(lfmt!(
                    "^8Each projectile deals^7 %d%%^8 more damage per previous projectile",
                    moreDamagePerProj
                ));
                // L11033
                breakdownSequential.push(lfmt!(
                    "^8With^7 %d^8 projectiles, damage progression is:^7",
                    output.num(n!("ProjectileCount"))
                ));
                // L11034
                let limit = output.num(n!("ProjectileCount"));
                let mut i = 1.0;
                while i <= limit {
                    // L11035
                    let projMult = moreDamagePerProj * (i - 1.0);
                    // L11036
                    breakdownSequential.push(lfmt!(
                        "  ^8Projectile %d:^7 %d%%^8 more damage",
                        i,
                        projMult
                    ));
                    i += 1.0;
                }
                // L11038
                breakdownSequential.push(lfmt!("^8Average more multiplier:^7 %.1f%%", avgMoreMult));
                // L11039
                breakdown.set(
                    n!("KineticFusilladeSequentialBreakdown"),
                    breakdownSequential,
                );
                // L11040
                let ProjectileCount = lor(breakdown.get(n!("ProjectileCount")), || {
                    Val::Table(Table::new())
                });
                breakdown.set(n!("ProjectileCount"), ProjectileCount);
                // L11041
                let max = Val::from_value(&activeSkill.skillModList.get().Override(
                    Some(&activeSkill.skillCfg.get()),
                    nm!["ProjectileCountMaximum"],
                ));
                let s = lfmt!(
                    "^8Maximum number of Kinetic Fusilade projectiles:^7 %d",
                    max.opt_num()
                );
                table_of(&breakdown.get(n!("ProjectileCount"))).push(s);
            }
        }
    }
}

/// @pob Data/Skills/act_int.lua:11046-11123 h=65b58eeb
/// `data.skills.KineticFusillade.postCritFunc` (postCritFunc)
pub(crate) fn KineticFusillade_postCritFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L11047
    let skillData = activeSkill.skillData.get();
    // L11048-11049: `t_insert` / `s_format` are `Table::push` / `lfmt!`

    // L11051
    let baseDelayBetweenProjectiles = skillData.get(n!("delayPerProjectile"));
    // L11052
    let mut projectileCount = Val::Num(1.0);

    // L11054
    if activeSkill.skillPart.get() == Some(1.0) {
        // L11055
        projectileCount = output.get(n!("ProjectileCount"));
    }

    // Calculate effective attack rate accounting for delayed projectile firing
    // Projectiles orbit for base_skill_effect_duration before firing
    // Recasting resets the timer, so attacking too fast wastes potential damage
    // Formula: totalTime = (hoverDelay + delayBetweenProj * nProj) * durationMod
    // L11062
    let hoverDelay = skillData.get(n!("duration"));
    // L11063
    let durationMod = output.get(n!("DurationMod"));
    // L11064
    let baseTimeForAllProjectiles =
        baseDelayBetweenProjectiles.num() * (projectileCount.num() - 1.0);
    // L11065
    let effectiveDelay = (hoverDelay.num() + baseTimeForAllProjectiles) * durationMod.num();
    // Testing in game showed playing in Lockstep rounded the duration to server ticks but Predictive did not
    // L11067
    let effectiveDelayRounded =
        m_ceil(effectiveDelay / data().misc.ServerTickTime) * data().misc.ServerTickTime;
    // L11068
    let maxEffectiveAPS = 1.0 / effectiveDelayRounded;
    // L11069
    let maxEffectivePredictiveAPS = 1.0 / effectiveDelay;
    // L11070
    let currentAPS = output.get(n!("Speed"));

    // L11072
    output.set(n!("KineticFusilladeMaxEffectiveAPS"), maxEffectiveAPS);

    // L11074
    if let Some(breakdown) = breakdown {
        // L11075
        let breakdownAPS = Table::new();
        // L11076
        breakdownAPS.push(lfmt!("^8Base hover delay:^7 %.1fs", hoverDelay.num()));
        // L11077
        breakdownAPS.push(lfmt!(
            "^8Base delay between projectiles:^7 %.2fs",
            baseDelayBetweenProjectiles.num()
        ));
        // L11078
        breakdownAPS.push(lfmt!(
            "^8Base time for^7 %d added ^8projectiles:^7 %.2fs x %d = %.1fs",
            (projectileCount.num() - 1.0),
            baseDelayBetweenProjectiles.num(),
            (projectileCount.num() - 1.0),
            baseTimeForAllProjectiles
        ));
        // L11079
        breakdownAPS.push(lfmt!("^8Duration modifier:^7 %.4f", durationMod.num()));
        // L11080
        breakdownAPS.push("");
        // L11081
        breakdownAPS.push(lfmt!("^6Lockstep"));
        // L11082
        breakdownAPS.push(lfmt!(
            "^8Effective delay:^7 (%.1f + %.2f) x %.4f = %.4fs",
            hoverDelay.num(),
            baseTimeForAllProjectiles,
            durationMod.num(),
            effectiveDelay
        ));
        // L11083
        breakdownAPS.push(lfmt!(
            "^8Rounded up to nearest server tick^7 = %.4fs",
            effectiveDelayRounded
        ));
        // L11084
        breakdownAPS.push(lfmt!(
            "^8Max effective attack rate:^7 1 / %.4f = %.2f",
            effectiveDelayRounded,
            maxEffectiveAPS
        ));
        // L11085
        if currentAPS.truthy() {
            // L11086
            let currentInc = activeSkill.skillModList.get().Sum(
                INC,
                Some(&activeSkill.skillCfg.get()),
                nm!["Speed"],
            );
            // L11087
            let neededInc =
                ((maxEffectiveAPS / currentAPS.num()) * (1.0 + currentInc / 100.0) - 1.0) * 100.0;
            // L11088
            let additionalInc = neededInc - currentInc;
            // L11089
            let neededPredictiveInc =
                ((maxEffectivePredictiveAPS / currentAPS.num()) * (1.0 + currentInc / 100.0) - 1.0)
                    * 100.0;
            // L11090
            let additionalPredictiveInc = neededPredictiveInc - currentInc;
            // L11091
            if currentAPS.num() > maxEffectiveAPS {
                // L11092
                breakdownAPS.push(lfmt!(
                    "^1Current attack rate (%.2f) exceeds max effective rate!",
                    currentAPS.num()
                ));
                // L11093
                breakdownAPS.push(lfmt!(
                    "^1Reduce attack speed by at least^7 %d%% ^1",
                    m_ceil(-additionalInc)
                ));
                // L11094
                breakdownAPS.push(lfmt!(
                    "^1DPS is reduced by %.1f%%",
                    (1.0 - maxEffectiveAPS / currentAPS.num()) * 100.0
                ));
            } else {
                // L11096
                breakdownAPS.push(lfmt!(
                    "^2Current attack rate (%.2f) is within effective limits",
                    currentAPS.num()
                ));
                // L11097
                breakdownAPS.push(lfmt!(
                    "^2You can add up to^7 %d%% ^2increased attack speed",
                    m_floor(additionalInc)
                ));
            }
            // L11099
            breakdownAPS.push("");
            // L11100
            breakdownAPS.push(lfmt!("^6Predictive"));
            // L11101
            breakdownAPS.push(lfmt!(
                "^8Effective delay:^7 (%.1f + %.2f) x %.4f = %.4fs",
                hoverDelay.num(),
                baseTimeForAllProjectiles,
                durationMod.num(),
                effectiveDelay
            ));
            // L11102
            breakdownAPS.push(lfmt!(
                "^8Max effective attack rate:^7 1 / %.4f = %.2f",
                effectiveDelay,
                maxEffectivePredictiveAPS
            ));
            // L11103
            if currentAPS.num() > maxEffectivePredictiveAPS {
                // L11104
                breakdownAPS.push(lfmt!(
                    "^1Current attack rate (%.2f) exceeds max effective rate!",
                    currentAPS.num()
                ));
                // L11105
                breakdownAPS.push(lfmt!(
                    "^1Reduce attack speed by at least^7 %d%% ^1",
                    m_ceil(-additionalPredictiveInc)
                ));
                // L11106
                breakdownAPS.push(lfmt!(
                    "^1DPS is reduced by %.1f%%",
                    (1.0 - maxEffectivePredictiveAPS / currentAPS.num()) * 100.0
                ));
            } else {
                // L11108
                breakdownAPS.push(lfmt!(
                    "^2Current attack rate (%.2f) is within effective limits",
                    currentAPS.num()
                ));
                // L11109
                breakdownAPS.push(lfmt!(
                    "^2You can add up to^7 %d%% ^2increased attack speed",
                    m_floor(additionalPredictiveInc)
                ));
            }
        }
        // L11112
        breakdown.set(n!("KineticFusilladeMaxEffectiveAPS"), breakdownAPS);
    }

    // Adjust dpsMultiplier if attacking too fast (only for "All Projectiles" mode)
    // L11116
    if activeSkill.skillPart.get() == Some(1.0) {
        // L11117
        if currentAPS.truthy() && currentAPS.num() > maxEffectiveAPS {
            // L11118
            let efficiencyRatio = maxEffectiveAPS / currentAPS.num();
            // L11119
            let originalMultiplier = lor(skillData.get(n!("dpsMultiplier")), || {
                output.get(n!("ProjectileCount"))
            });
            // L11120
            skillData.set(
                n!("dpsMultiplier"),
                originalMultiplier.num() * efficiencyRatio,
            );
        }
    }
}

/// @pob Data/Skills/act_int.lua:11227-11263 h=440601e4
/// `data.skills.KineticFusilladeAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn KineticFusilladeAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L11228
    let skillData = activeSkill.skillData.get();
    // L11229-11230: `t_insert` / `s_format` are `Table::push` / `lfmt!`

    // L11232
    if activeSkill.skillPart.get() == Some(1.0) {
        // Set base dpsMultiplier for projectile count
        // L11234
        activeSkill
            .skillData
            .get()
            .set(n!("dpsMultiplier"), output.get(n!("ProjectileCount")));

        // Calculate average damage scaling for sequential projectiles
        // Each projectile does more damage based on how many came before it
        // L11238
        let moreDamagePerProj = skillData.num_or(n!("damagePerProjectile"), 0.0);
        // L11239
        if moreDamagePerProj != 0.0 && output.num(n!("ProjectileCount")) > 1.0 {
            // Average multiplier: sum of (0, X, 2X, 3X, ..., (n-1)X) / n
            // This equals: X * (0 + 1 + 2 + ... + (n-1)) / n = X * n(n-1)/2 / n = X * (n-1)/2
            // L11242
            let avgMoreMult = moreDamagePerProj * (output.num(n!("ProjectileCount")) - 1.0) / 2.0;
            // L11243
            activeSkill.skillModList.get().NewMod(
                n!("Damage"),
                MORE,
                avgMoreMult,
                "Skill:KineticFusilladeAltX",
                0,
                0,
                &[],
            );

            // Store the average multiplier for display
            // L11246
            output.set(n!("KineticFusilladeAvgMoreMult"), avgMoreMult);

            // L11248
            if let Some(breakdown) = breakdown {
                // L11249
                let breakdownSequential = Table::new();
                // L11250
                breakdownSequential.push(lfmt!(
                    "^8Each projectile deals^7 %d%%^8 more damage per previous projectile",
                    moreDamagePerProj
                ));
                // L11251
                breakdownSequential.push(lfmt!(
                    "^8With^7 %d^8 projectiles, damage progression is:^7",
                    output.num(n!("ProjectileCount"))
                ));
                // L11252
                let limit = output.num(n!("ProjectileCount"));
                let mut i = 1.0;
                while i <= limit {
                    // L11253
                    let projMult = moreDamagePerProj * (i - 1.0);
                    // L11254
                    breakdownSequential.push(lfmt!(
                        "  ^8Projectile %d:^7 %d%%^8 more damage",
                        i,
                        projMult
                    ));
                    i += 1.0;
                }
                // L11256
                breakdownSequential.push(lfmt!("^8Average more multiplier:^7 %.1f%%", avgMoreMult));
                // L11257
                breakdown.set(
                    n!("KineticFusilladeSequentialBreakdown"),
                    breakdownSequential,
                );
                // L11258
                let ProjectileCount = lor(breakdown.get(n!("ProjectileCount")), || {
                    Val::Table(Table::new())
                });
                breakdown.set(n!("ProjectileCount"), ProjectileCount);
                // L11259
                let max = Val::from_value(&activeSkill.skillModList.get().Override(
                    Some(&activeSkill.skillCfg.get()),
                    nm!["ProjectileCountMaximum"],
                ));
                let s = lfmt!(
                    "^8Maximum number of Kinetic Fusilade projectiles:^7 %d",
                    max.opt_num()
                );
                table_of(&breakdown.get(n!("ProjectileCount"))).push(s);
            }
        }
    }
}

/// @pob Data/Skills/act_int.lua:11264-11341 h=65b58eeb
/// `data.skills.KineticFusilladeAltX.postCritFunc` (postCritFunc)
pub(crate) fn KineticFusilladeAltX_postCritFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L11265
    let skillData = activeSkill.skillData.get();
    // L11266-11267: `t_insert` / `s_format` are `Table::push` / `lfmt!`

    // L11269
    let baseDelayBetweenProjectiles = skillData.get(n!("delayPerProjectile"));
    // L11270
    let mut projectileCount = Val::Num(1.0);

    // L11272
    if activeSkill.skillPart.get() == Some(1.0) {
        // L11273
        projectileCount = output.get(n!("ProjectileCount"));
    }

    // Calculate effective attack rate accounting for delayed projectile firing
    // Projectiles orbit for base_skill_effect_duration before firing
    // Recasting resets the timer, so attacking too fast wastes potential damage
    // Formula: totalTime = (hoverDelay + delayBetweenProj * nProj) * durationMod
    // L11280
    let hoverDelay = skillData.get(n!("duration"));
    // L11281
    let durationMod = output.get(n!("DurationMod"));
    // L11282
    let baseTimeForAllProjectiles =
        baseDelayBetweenProjectiles.num() * (projectileCount.num() - 1.0);
    // L11283
    let effectiveDelay = (hoverDelay.num() + baseTimeForAllProjectiles) * durationMod.num();
    // Testing in game showed playing in Lockstep rounded the duration to server ticks but Predictive did not
    // L11285
    let effectiveDelayRounded =
        m_ceil(effectiveDelay / data().misc.ServerTickTime) * data().misc.ServerTickTime;
    // L11286
    let maxEffectiveAPS = 1.0 / effectiveDelayRounded;
    // L11287
    let maxEffectivePredictiveAPS = 1.0 / effectiveDelay;
    // L11288
    let currentAPS = output.get(n!("Speed"));

    // L11290
    output.set(n!("KineticFusilladeMaxEffectiveAPS"), maxEffectiveAPS);

    // L11292
    if let Some(breakdown) = breakdown {
        // L11293
        let breakdownAPS = Table::new();
        // L11294
        breakdownAPS.push(lfmt!("^8Base hover delay:^7 %.1fs", hoverDelay.num()));
        // L11295
        breakdownAPS.push(lfmt!(
            "^8Base delay between projectiles:^7 %.2fs",
            baseDelayBetweenProjectiles.num()
        ));
        // L11296
        breakdownAPS.push(lfmt!(
            "^8Base time for^7 %d added ^8projectiles:^7 %.2fs x %d = %.1fs",
            (projectileCount.num() - 1.0),
            baseDelayBetweenProjectiles.num(),
            (projectileCount.num() - 1.0),
            baseTimeForAllProjectiles
        ));
        // L11297
        breakdownAPS.push(lfmt!("^8Duration modifier:^7 %.4f", durationMod.num()));
        // L11298
        breakdownAPS.push("");
        // L11299
        breakdownAPS.push(lfmt!("^6Lockstep"));
        // L11300
        breakdownAPS.push(lfmt!(
            "^8Effective delay:^7 (%.1f + %.2f) x %.4f = %.4fs",
            hoverDelay.num(),
            baseTimeForAllProjectiles,
            durationMod.num(),
            effectiveDelay
        ));
        // L11301
        breakdownAPS.push(lfmt!(
            "^8Rounded up to nearest server tick^7 = %.4fs",
            effectiveDelayRounded
        ));
        // L11302
        breakdownAPS.push(lfmt!(
            "^8Max effective attack rate:^7 1 / %.4f = %.2f",
            effectiveDelayRounded,
            maxEffectiveAPS
        ));
        // L11303
        if currentAPS.truthy() {
            // L11304
            let currentInc = activeSkill.skillModList.get().Sum(
                INC,
                Some(&activeSkill.skillCfg.get()),
                nm!["Speed"],
            );
            // L11305
            let neededInc =
                ((maxEffectiveAPS / currentAPS.num()) * (1.0 + currentInc / 100.0) - 1.0) * 100.0;
            // L11306
            let additionalInc = neededInc - currentInc;
            // L11307
            let neededPredictiveInc =
                ((maxEffectivePredictiveAPS / currentAPS.num()) * (1.0 + currentInc / 100.0) - 1.0)
                    * 100.0;
            // L11308
            let additionalPredictiveInc = neededPredictiveInc - currentInc;
            // L11309
            if currentAPS.num() > maxEffectiveAPS {
                // L11310
                breakdownAPS.push(lfmt!(
                    "^1Current attack rate (%.2f) exceeds max effective rate!",
                    currentAPS.num()
                ));
                // L11311
                breakdownAPS.push(lfmt!(
                    "^1Reduce attack speed by at least^7 %d%% ^1",
                    m_ceil(-additionalInc)
                ));
                // L11312
                breakdownAPS.push(lfmt!(
                    "^1DPS is reduced by %.1f%%",
                    (1.0 - maxEffectiveAPS / currentAPS.num()) * 100.0
                ));
            } else {
                // L11314
                breakdownAPS.push(lfmt!(
                    "^2Current attack rate (%.2f) is within effective limits",
                    currentAPS.num()
                ));
                // L11315
                breakdownAPS.push(lfmt!(
                    "^2You can add up to^7 %d%% ^2increased attack speed",
                    m_floor(additionalInc)
                ));
            }
            // L11317
            breakdownAPS.push("");
            // L11318
            breakdownAPS.push(lfmt!("^6Predictive"));
            // L11319
            breakdownAPS.push(lfmt!(
                "^8Effective delay:^7 (%.1f + %.2f) x %.4f = %.4fs",
                hoverDelay.num(),
                baseTimeForAllProjectiles,
                durationMod.num(),
                effectiveDelay
            ));
            // L11320
            breakdownAPS.push(lfmt!(
                "^8Max effective attack rate:^7 1 / %.4f = %.2f",
                effectiveDelay,
                maxEffectivePredictiveAPS
            ));
            // L11321
            if currentAPS.num() > maxEffectivePredictiveAPS {
                // L11322
                breakdownAPS.push(lfmt!(
                    "^1Current attack rate (%.2f) exceeds max effective rate!",
                    currentAPS.num()
                ));
                // L11323
                breakdownAPS.push(lfmt!(
                    "^1Reduce attack speed by at least^7 %d%% ^1",
                    m_ceil(-additionalPredictiveInc)
                ));
                // L11324
                breakdownAPS.push(lfmt!(
                    "^1DPS is reduced by %.1f%%",
                    (1.0 - maxEffectivePredictiveAPS / currentAPS.num()) * 100.0
                ));
            } else {
                // L11326
                breakdownAPS.push(lfmt!(
                    "^2Current attack rate (%.2f) is within effective limits",
                    currentAPS.num()
                ));
                // L11327
                breakdownAPS.push(lfmt!(
                    "^2You can add up to^7 %d%% ^2increased attack speed",
                    m_floor(additionalPredictiveInc)
                ));
            }
        }
        // L11330
        breakdown.set(n!("KineticFusilladeMaxEffectiveAPS"), breakdownAPS);
    }

    // Adjust dpsMultiplier if attacking too fast (only for "All Projectiles" mode)
    // L11334
    if activeSkill.skillPart.get() == Some(1.0) {
        // L11335
        if currentAPS.truthy() && currentAPS.num() > maxEffectiveAPS {
            // L11336
            let efficiencyRatio = maxEffectiveAPS / currentAPS.num();
            // L11337
            let originalMultiplier = lor(skillData.get(n!("dpsMultiplier")), || {
                output.get(n!("ProjectileCount"))
            });
            // L11338
            skillData.set(
                n!("dpsMultiplier"),
                originalMultiplier.num() * efficiencyRatio,
            );
        }
    }
}

/// @pob Data/Skills/act_int.lua:11667-11788 h=e3fdb3c5
/// `data.skills.LightningSpireTrap.preDamageFunc` (preDamageFunc)
pub(crate) fn LightningSpireTrap_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L11668
    let skillCfg = activeSkill.skillCfg.get();
    // L11669
    let skillData = activeSkill.skillData.get();
    // L11670
    let skillPart = activeSkill.skillPart.get();
    // L11671
    let skillModList = activeSkill.skillModList.get();
    // L11672-11673: `t_insert` / `s_format` are `Table::push` / `lfmt!`

    // seemingly the only mechanical difference with seismic trap - this one does not scale it's total radius with AoE modifiers
    // L11676
    output.set(n!("AreaOfEffectRadius"), skillData.get(n!("radius")));
    // L11677
    if let Some(breakdown) = breakdown {
        // L11678
        breakdown.set(
            n!("AreaOfEffectRadius"),
            Table::from_ctor(vec![TableField::item(
                true,
                "Targeting area of this skill is not affected by Area of Effect modifiers.",
            )]),
        );
    }

    // L11681
    let baseInterval = skillData.get(n!("repeatInterval"));
    // L11682
    let incFrequency =
        (1.0 + skillModList.Sum(INC, Some(&skillCfg), nm!["TrapThrowingSpeed"]) / 100.0);
    // L11683
    let moreFrequency = skillModList.More(Some(&skillCfg), nm!["TrapThrowingSpeed"]);
    // L11684
    let wavePulseRate = incFrequency * moreFrequency / baseInterval.num();
    // L11685
    skillData.set(n!("hitTimeOverride"), 1.0 / wavePulseRate);
    // L11686
    output.set(n!("WavePulseRate"), wavePulseRate);
    // L11687
    let incDuration = (1.0 + skillModList.Sum(INC, Some(&skillCfg), nm!["Duration"]) / 100.0);
    // L11688
    let moreDuration = skillModList.More(Some(&skillCfg), nm!["Duration"]);
    // L11689
    let duration = skillData.num(n!("duration")) * incDuration * moreDuration;
    // L11690
    let pulses = m_floor(duration * wavePulseRate);
    // L11691
    output.set(n!("PulsesPerTrap"), pulses);
    // L11692
    let effectiveDuration = pulses / wavePulseRate;
    // L11693
    let cooldown = output.get(n!("TrapCooldown"));
    // L11694
    let averageActiveTraps = effectiveDuration / cooldown.num();
    // L11695
    output.set(n!("AverageActiveTraps"), averageActiveTraps);
    // L11696
    // not to be confused with attack hit chance
    let hitChance = |enemyRadius: f64, areaDamageRadius: f64, areaSpreadRadius: f64| -> f64 {
        // L11697
        let damagingAreaRadius = areaDamageRadius + enemyRadius - 1.0; // radius where area damage can land to hit the enemy;
                                                                       // -1 because of two assumptions: PoE coordinates are integers and damage is not registered if the two areas only share a point or vertex. If either is not correct, then -1 is not needed.
                                                                       // L11699
        m_min(
            damagingAreaRadius * damagingAreaRadius / (areaSpreadRadius * areaSpreadRadius),
            1.0,
        )
    };
    // L11701
    let enemyRadius = lor(
        Val::from_value(&skillModList.Override(Some(&skillCfg), nm!["EnemyRadius"])),
        || Val::Num(skillModList.Sum(BASE, Some(&skillCfg), nm!["EnemyRadius"])),
    );
    // L11702
    let waveRadius = output.get(n!("AreaOfEffectRadiusSecondary"));
    // L11703
    let fullRadius = output.get(n!("AreaOfEffectRadius"));
    // L11704
    let overlapChance = hitChance(enemyRadius.num(), waveRadius.num(), fullRadius.num());
    // L11705
    output.set(n!("OverlapChance"), overlapChance * 100.0);
    // L11706
    if let Some(breakdown) = breakdown {
        // L11707
        let OverlapChance = Table::new();
        breakdown.set(n!("OverlapChance"), OverlapChance.clone());
        // L11708
        OverlapChance.push("Chance for individual wave to land within range to damage enemy:");
        // L11709
        OverlapChance.push("^8= (area where wave can spawn to damage enemy) / (total area)");
        // L11710
        OverlapChance
            .push("^8= (^7secondary radius^8 + ^7enemy radius^8 - 1) ^ 2 / ^7radius^8 ^ 2");
        // L11711
        OverlapChance.push(lfmt!(
            "^8= (^7%d^8 +^7 %d^8 - 1) ^ 2 /^7 %d^8 ^ 2",
            waveRadius.num(),
            enemyRadius.num(),
            fullRadius.num()
        ));
        // L11712
        OverlapChance.push(lfmt!("^8=^7 %.3f^8%%", overlapChance * 100.0));
        // L11713
        let WavePulseRate = Table::new();
        breakdown.set(n!("WavePulseRate"), WavePulseRate.clone());
        // L11714
        WavePulseRate.push("Pulse rate:");
        // L11715
        WavePulseRate.push(lfmt!("%.2f ^8(base pulse rate)", 1.0 / baseInterval.num()));
        // L11716
        WavePulseRate.push(lfmt!(
            "* %.2f ^8(increased/reduced pulse frequency)",
            incFrequency
        ));
        // L11717
        WavePulseRate.push(lfmt!("* %.2f ^8(more/less pulse frequency)", moreFrequency));
        // L11718
        WavePulseRate.push(lfmt!("= %.2f^8/s", wavePulseRate));
        // L11719
        let PulsesPerTrap = Table::new();
        breakdown.set(n!("PulsesPerTrap"), PulsesPerTrap.clone());
        // L11720
        PulsesPerTrap.push("Pulses per trap:");
        // L11721
        PulsesPerTrap.push(lfmt!("%.3f ^8(unrounded skill duration)", duration));
        // L11722
        PulsesPerTrap.push(lfmt!("* %.2f ^8(pulse rate)", wavePulseRate));
        // L11723
        PulsesPerTrap.push(lfmt!("= %.2f ^8pulses", duration * wavePulseRate));
        // L11724
        PulsesPerTrap.push("^8rounded down");
        // L11725
        PulsesPerTrap.push(lfmt!("= %d ^8pulses", pulses));
        // L11726
        PulsesPerTrap.push(lfmt!(
            "^8Next breakpoint: %d%% increased Trap Throwing Speed / %d%% increased Duration",
            m_ceil(
                100.0
                    * ((pulses + 1.0) * baseInterval.num() / (duration * moreFrequency)
                        - incFrequency)
            ),
            m_ceil(
                100.0
                    * ((pulses + 1.0)
                        / (wavePulseRate * skillData.num(n!("duration")) * moreDuration)
                        - incDuration)
            )
        ));
        // L11730
        PulsesPerTrap.push(lfmt!(
            "^8Previous breakpoint: %d%% reduced Trap Throwing Speed / %d%% reduced Duration",
            -m_ceil(
                100.0 * (pulses * baseInterval.num() / (duration * moreFrequency) - incFrequency)
                    - 1.0
            ),
            -m_ceil(
                100.0
                    * (pulses / (wavePulseRate * skillData.num(n!("duration")) * moreDuration)
                        - incDuration)
                    - 1.0
            )
        ));
        // L11734
        let AverageActiveTraps = Table::new();
        breakdown.set(n!("AverageActiveTraps"), AverageActiveTraps.clone());
        // L11735
        AverageActiveTraps.push("Average active traps, not considering stored cooldown uses:");
        // L11736
        AverageActiveTraps.push(lfmt!(
            "%.2f^8 /^7 %.2f^8 (pulses / pulse rate = effective skill duration)",
            pulses,
            wavePulseRate
        ));
        // L11737
        AverageActiveTraps.push(lfmt!("/ %.2f ^8(cooldown)", cooldown.num()));
        // L11738
        AverageActiveTraps.push(lfmt!("= %.2f traps", averageActiveTraps));
    }
    // L11740
    let maxWaves = skillModList.Sum(BASE, Some(&skillCfg), nm!["MaximumWaves"]);
    // L11741
    let mut dpsMultiplier = 1.0;
    // L11742
    if skillPart == Some(2.0) {
        // L11743
        dpsMultiplier = maxWaves * overlapChance;
        // L11744
        if let Some(breakdown) = breakdown {
            // L11745
            let SkillDPSMultiplier = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), SkillDPSMultiplier.clone());
            // L11746
            SkillDPSMultiplier.push("DPS multiplier");
            // L11747
            SkillDPSMultiplier.push("^8= ^7maximum waves^8 * ^7overlap chance^8");
            // L11748
            SkillDPSMultiplier.push(lfmt!("^8=^7 %d^8 *^7 %.2f^8", maxWaves, overlapChance));
            // L11749
            SkillDPSMultiplier.push(lfmt!("^8=^7 %.3f", dpsMultiplier));
        }
    } else if skillPart == Some(3.0) {
        // L11752
        dpsMultiplier = maxWaves;
        // L11753
        if let Some(breakdown) = breakdown {
            // L11754
            let SkillDPSMultiplier = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), SkillDPSMultiplier.clone());
            // L11755
            SkillDPSMultiplier.push("DPS multiplier");
            // L11756
            SkillDPSMultiplier.push(lfmt!("^8=^7 %d (maximum waves)", dpsMultiplier));
        }
    } else if skillPart == Some(4.0) {
        // L11759
        dpsMultiplier = averageActiveTraps;
        // L11760
        if let Some(breakdown) = breakdown {
            // L11761
            let SkillDPSMultiplier = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), SkillDPSMultiplier.clone());
            // L11762
            SkillDPSMultiplier.push("DPS multiplier");
            // L11763
            SkillDPSMultiplier.push(lfmt!("^8=^7 %.2f (average active traps)", dpsMultiplier));
        }
    } else if skillPart == Some(5.0) {
        // L11766
        dpsMultiplier = averageActiveTraps * maxWaves * overlapChance;
        // L11767
        if let Some(breakdown) = breakdown {
            // L11768
            let SkillDPSMultiplier = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), SkillDPSMultiplier.clone());
            // L11769
            SkillDPSMultiplier.push("DPS multiplier");
            // L11770
            SkillDPSMultiplier
                .push("^8= ^7average active traps^8 * ^7maximum waves^8 * ^7overlap chance^8");
            // L11771
            SkillDPSMultiplier.push(lfmt!(
                "^8=^7 %.2f^8 *^7 %d^8 *^7 %.2f",
                averageActiveTraps,
                maxWaves,
                overlapChance
            ));
            // L11772
            SkillDPSMultiplier.push(lfmt!("^8=^7 %.3f", dpsMultiplier));
        }
    } else if skillPart == Some(6.0) {
        // L11775
        dpsMultiplier = averageActiveTraps * maxWaves;
        // L11776
        if let Some(breakdown) = breakdown {
            // L11777
            let SkillDPSMultiplier = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), SkillDPSMultiplier.clone());
            // L11778
            SkillDPSMultiplier.push("DPS multiplier");
            // L11779
            SkillDPSMultiplier.push("^8= ^7average active traps^8 * ^7maximum waves");
            // L11780
            SkillDPSMultiplier.push(lfmt!("^8=^7 %.2f^8 *^7 %d", averageActiveTraps, maxWaves));
            // L11781
            SkillDPSMultiplier.push(lfmt!("^8=^7 %.3f", dpsMultiplier));
        }
    }
    // L11784
    if dpsMultiplier != 1.0 {
        // L11785
        let v = skillData.num_or(n!("dpsMultiplier"), 1.0) * dpsMultiplier;
        skillData.set(n!("dpsMultiplier"), v);
        // L11786
        let v = output.num_or(n!("SkillDPSMultiplier"), 1.0) * dpsMultiplier;
        output.set(n!("SkillDPSMultiplier"), v);
    }
}

/// @pob Data/Skills/act_int.lua:11912-12028 h=e0102961
/// `data.skills.LightningSpireTrapAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn LightningSpireTrapAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L11913
    let skillCfg = activeSkill.skillCfg.get();
    // L11914
    let skillData = activeSkill.skillData.get();
    // L11915
    let skillPart = activeSkill.skillPart.get();
    // L11916
    let skillModList = activeSkill.skillModList.get();
    // L11917-11918: `t_insert` / `s_format` are `Table::push` / `lfmt!`

    // seemingly the only mechanical difference with seismic trap - this one does not scale it's total radius with AoE modifiers
    // L11921
    output.set(n!("AreaOfEffectRadius"), skillData.get(n!("radius")));
    // L11922
    if let Some(breakdown) = breakdown {
        // L11923
        breakdown.set(
            n!("AreaOfEffectRadius"),
            Table::from_ctor(vec![TableField::item(
                true,
                "Targeting area of this skill is not affected by Area of Effect modifiers.",
            )]),
        );
    }

    // L11926
    let baseInterval = skillData.get(n!("repeatInterval"));
    // L11927
    let wavePulseRate = 1.0 / baseInterval.num();
    // L11928
    skillData.set(n!("hitTimeOverride"), 1.0 / wavePulseRate);
    // L11929
    let incDuration = (1.0 + skillModList.Sum(INC, Some(&skillCfg), nm!["Duration"]) / 100.0);
    // L11930
    let moreDuration = skillModList.More(Some(&skillCfg), nm!["Duration"]);
    // L11931
    let duration = skillData.num(n!("duration")) * incDuration * moreDuration;
    // L11932
    let pulses = m_floor(duration * wavePulseRate);
    // L11933
    output.set(n!("PulsesPerTrap"), pulses);
    // L11934
    let actionSpeedMod = 1.0 + skillModList.Sum(INC, Some(&skillCfg), nm!["ActionSpeed"]) / 100.0;
    // L11935
    let baseSpeed = 1.0 / skillModList.Sum(BASE, Some(&skillCfg), nm!["TrapThrowingTime"]);
    // L11936
    let mut throwSpeed = baseSpeed
        * calcLib::r#mod(&skillModList, Some(&skillCfg), nm!["TrapThrowingSpeed"])
        * actionSpeedMod;
    // L11937
    throwSpeed = m_min(throwSpeed, data().misc.ServerTickRate);
    // L11938
    let throwTime = 1.0 / throwSpeed;

    // L11940
    let effectiveDuration = pulses / wavePulseRate;
    //local cooldown = output.TrapCooldown or 1
    // L11942
    let averageActiveTraps = effectiveDuration / throwTime;
    // L11943
    output.set(n!("AverageActiveTraps"), averageActiveTraps);
    // L11944
    // not to be confused with attack hit chance
    let hitChance = |enemyRadius: f64, areaDamageRadius: f64, areaSpreadRadius: f64| -> f64 {
        // L11945
        let damagingAreaRadius = areaDamageRadius + enemyRadius - 1.0; // radius where area damage can land to hit the enemy;
                                                                       // -1 because of two assumptions: PoE coordinates are integers and damage is not registered if the two areas only share a point or vertex. If either is not correct, then -1 is not needed.
                                                                       // L11947
        m_min(
            damagingAreaRadius * damagingAreaRadius / (areaSpreadRadius * areaSpreadRadius),
            1.0,
        )
    };
    // L11949
    let enemyRadius = lor(
        Val::from_value(&skillModList.Override(Some(&skillCfg), nm!["EnemyRadius"])),
        || Val::Num(skillModList.Sum(BASE, Some(&skillCfg), nm!["EnemyRadius"])),
    );
    // L11950
    let waveRadius = output.get(n!("AreaOfEffectRadiusSecondary"));
    // L11951
    let fullRadius = output.get(n!("AreaOfEffectRadius"));
    // L11952
    let overlapChance = hitChance(enemyRadius.num(), waveRadius.num(), fullRadius.num());
    // L11953
    output.set(n!("OverlapChance"), overlapChance * 100.0);
    // L11954
    if let Some(breakdown) = breakdown {
        // L11955
        let OverlapChance = Table::new();
        breakdown.set(n!("OverlapChance"), OverlapChance.clone());
        // L11956
        OverlapChance.push("Chance for individual wave to land within range to damage enemy:");
        // L11957
        OverlapChance.push("^8= (area where wave can spawn to damage enemy) / (total area)");
        // L11958
        OverlapChance
            .push("^8= (^7secondary radius^8 + ^7enemy radius^8 - 1) ^ 2 / ^7radius^8 ^ 2");
        // L11959
        OverlapChance.push(lfmt!(
            "^8= (^7%d^8 +^7 %d^8 - 1) ^ 2 /^7 %d^8 ^ 2",
            waveRadius.num(),
            enemyRadius.num(),
            fullRadius.num()
        ));
        // L11960
        OverlapChance.push(lfmt!("^8=^7 %.3f^8%%", overlapChance * 100.0));
        // L11961
        let PulsesPerTrap = Table::new();
        breakdown.set(n!("PulsesPerTrap"), PulsesPerTrap.clone());
        // L11962
        PulsesPerTrap.push("Pulses per trap:");
        // L11963
        PulsesPerTrap.push(lfmt!("%.3f ^8(unrounded skill duration)", duration));
        // L11964
        PulsesPerTrap.push(lfmt!("* %.2f ^8(pulse rate)", wavePulseRate));
        // L11965
        PulsesPerTrap.push(lfmt!("= %.2f ^8pulses", duration * wavePulseRate));
        // L11966
        PulsesPerTrap.push("^8rounded down");
        // L11967
        PulsesPerTrap.push(lfmt!("= %d ^8pulses", pulses));
        // L11968
        PulsesPerTrap.push(lfmt!(
            "^8Next breakpoint: %d%% increased Duration",
            m_ceil(
                100.0
                    * ((pulses + 1.0)
                        / (wavePulseRate * skillData.num(n!("duration")) * moreDuration)
                        - incDuration)
            )
        ));
        // L11971
        PulsesPerTrap.push(lfmt!(
            "^8Previous breakpoint: %d%% reduced Duration",
            -m_ceil(
                100.0
                    * (pulses / (wavePulseRate * skillData.num(n!("duration")) * moreDuration)
                        - incDuration)
                    - 1.0
            )
        ));
        // L11974
        let AverageActiveTraps = Table::new();
        breakdown.set(n!("AverageActiveTraps"), AverageActiveTraps.clone());
        // L11975
        AverageActiveTraps.push("Average active traps:");
        // L11976
        AverageActiveTraps.push(lfmt!(
            "%.2f^8 /^7 %.2f^8 (pulses / pulse rate = effective skill duration)",
            pulses,
            wavePulseRate
        ));
        // L11977
        AverageActiveTraps.push(lfmt!("/ %.2f ^8(throw time)", throwTime));
        // L11978
        AverageActiveTraps.push(lfmt!("= %.2f traps", averageActiveTraps));
    }
    // L11980
    let maxWaves = skillModList.Sum(BASE, Some(&skillCfg), nm!["MaximumWaves"]);
    // L11981
    let mut dpsMultiplier = 1.0;
    // L11982
    if skillPart == Some(2.0) {
        // L11983
        dpsMultiplier = maxWaves * overlapChance;
        // L11984
        if let Some(breakdown) = breakdown {
            // L11985
            let SkillDPSMultiplier = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), SkillDPSMultiplier.clone());
            // L11986
            SkillDPSMultiplier.push("DPS multiplier");
            // L11987
            SkillDPSMultiplier.push("^8= ^7maximum waves^8 * ^7overlap chance^8");
            // L11988
            SkillDPSMultiplier.push(lfmt!("^8=^7 %d^8 *^7 %.2f^8", maxWaves, overlapChance));
            // L11989
            SkillDPSMultiplier.push(lfmt!("^8=^7 %.3f", dpsMultiplier));
        }
    } else if skillPart == Some(3.0) {
        // L11992
        dpsMultiplier = maxWaves;
        // L11993
        if let Some(breakdown) = breakdown {
            // L11994
            let SkillDPSMultiplier = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), SkillDPSMultiplier.clone());
            // L11995
            SkillDPSMultiplier.push("DPS multiplier");
            // L11996
            SkillDPSMultiplier.push(lfmt!("^8=^7 %d (maximum waves)", dpsMultiplier));
        }
    } else if skillPart == Some(4.0) {
        // L11999
        dpsMultiplier = averageActiveTraps;
        // L12000
        if let Some(breakdown) = breakdown {
            // L12001
            let SkillDPSMultiplier = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), SkillDPSMultiplier.clone());
            // L12002
            SkillDPSMultiplier.push("DPS multiplier");
            // L12003
            SkillDPSMultiplier.push(lfmt!("^8=^7 %.2f (average active traps)", dpsMultiplier));
        }
    } else if skillPart == Some(5.0) {
        // L12006
        dpsMultiplier = averageActiveTraps * maxWaves * overlapChance;
        // L12007
        if let Some(breakdown) = breakdown {
            // L12008
            let SkillDPSMultiplier = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), SkillDPSMultiplier.clone());
            // L12009
            SkillDPSMultiplier.push("DPS multiplier");
            // L12010
            SkillDPSMultiplier
                .push("^8= ^7average active traps^8 * ^7maximum waves^8 * ^7overlap chance^8");
            // L12011
            SkillDPSMultiplier.push(lfmt!(
                "^8=^7 %.2f^8 *^7 %d^8 *^7 %.2f",
                averageActiveTraps,
                maxWaves,
                overlapChance
            ));
            // L12012
            SkillDPSMultiplier.push(lfmt!("^8=^7 %.3f", dpsMultiplier));
        }
    } else if skillPart == Some(6.0) {
        // L12015
        dpsMultiplier = averageActiveTraps * maxWaves;
        // L12016
        if let Some(breakdown) = breakdown {
            // L12017
            let SkillDPSMultiplier = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), SkillDPSMultiplier.clone());
            // L12018
            SkillDPSMultiplier.push("DPS multiplier");
            // L12019
            SkillDPSMultiplier.push("^8= ^7average active traps^8 * ^7maximum waves");
            // L12020
            SkillDPSMultiplier.push(lfmt!("^8=^7 %.2f^8 *^7 %d", averageActiveTraps, maxWaves));
            // L12021
            SkillDPSMultiplier.push(lfmt!("^8=^7 %.3f", dpsMultiplier));
        }
    }
    // L12024
    if dpsMultiplier != 1.0 {
        // L12025
        let v = skillData.num_or(n!("dpsMultiplier"), 1.0) * dpsMultiplier;
        skillData.set(n!("dpsMultiplier"), v);
        // L12026
        let v = output.num_or(n!("SkillDPSMultiplier"), 1.0) * dpsMultiplier;
        output.set(n!("SkillDPSMultiplier"), v);
    }
}

/// @pob Data/Skills/act_int.lua:12150-12260 h=73550c96
/// `data.skills.LightningSpireTrapAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn LightningSpireTrapAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L12151
    let skillCfg = activeSkill.skillCfg.get();
    // L12152
    let skillData = activeSkill.skillData.get();
    // L12153
    let skillPart = activeSkill.skillPart.get();
    // L12154
    let skillModList = activeSkill.skillModList.get();
    // L12155-12156: `t_insert` / `s_format` are `Table::push` / `lfmt!`

    // seemingly the only mechanical difference with seismic trap - this one does not scale it's total radius with AoE modifiers
    // L12159
    output.set(n!("AreaOfEffectRadius"), skillData.get(n!("radius")));
    // L12160
    if let Some(breakdown) = breakdown {
        // L12161
        breakdown.set(
            n!("AreaOfEffectRadius"),
            Table::from_ctor(vec![TableField::item(
                true,
                "Targeting area of this skill is not affected by Area of Effect modifiers.",
            )]),
        );
    }

    // L12164
    let baseInterval = skillData.get(n!("repeatInterval"));
    // L12165
    let wavePulseRate = 1.0 / baseInterval.num();
    // L12166
    skillData.set(n!("hitTimeOverride"), 1.0 / wavePulseRate);
    // L12167
    let incDuration = (1.0 + skillModList.Sum(INC, Some(&skillCfg), nm!["Duration"]) / 100.0);
    // L12168
    let moreDuration = skillModList.More(Some(&skillCfg), nm!["Duration"]);
    // L12169
    let duration = skillData.num(n!("duration")) * incDuration * moreDuration;
    // L12170
    let pulses = m_floor(duration * wavePulseRate);
    // L12171
    output.set(n!("PulsesPerTrap"), pulses);
    // L12172
    let effectiveDuration = pulses / wavePulseRate;
    // L12173
    let cooldown = output.get(n!("TrapCooldown"));
    // L12174
    let averageActiveTraps = effectiveDuration / cooldown.num();
    // L12175
    output.set(n!("AverageActiveTraps"), averageActiveTraps);
    // L12176
    // not to be confused with attack hit chance
    let hitChance = |enemyRadius: f64, areaDamageRadius: f64, areaSpreadRadius: f64| -> f64 {
        // L12177
        let damagingAreaRadius = areaDamageRadius + enemyRadius - 1.0; // radius where area damage can land to hit the enemy;
                                                                       // -1 because of two assumptions: PoE coordinates are integers and damage is not registered if the two areas only share a point or vertex. If either is not correct, then -1 is not needed.
                                                                       // L12179
        m_min(
            damagingAreaRadius * damagingAreaRadius / (areaSpreadRadius * areaSpreadRadius),
            1.0,
        )
    };
    // L12181
    let enemyRadius = lor(
        Val::from_value(&skillModList.Override(Some(&skillCfg), nm!["EnemyRadius"])),
        || Val::Num(skillModList.Sum(BASE, Some(&skillCfg), nm!["EnemyRadius"])),
    );
    // L12182
    let waveRadius = output.get(n!("AreaOfEffectRadiusSecondary"));
    // L12183
    let fullRadius = output.get(n!("AreaOfEffectRadius"));
    // L12184
    let overlapChance = hitChance(enemyRadius.num(), waveRadius.num(), fullRadius.num());
    // L12185
    output.set(n!("OverlapChance"), overlapChance * 100.0);
    // L12186
    if let Some(breakdown) = breakdown {
        // L12187
        let OverlapChance = Table::new();
        breakdown.set(n!("OverlapChance"), OverlapChance.clone());
        // L12188
        OverlapChance.push("Chance for individual wave to land within range to damage enemy:");
        // L12189
        OverlapChance.push("^8= (area where wave can spawn to damage enemy) / (total area)");
        // L12190
        OverlapChance
            .push("^8= (^7secondary radius^8 + ^7enemy radius^8 - 1) ^ 2 / ^7radius^8 ^ 2");
        // L12191
        OverlapChance.push(lfmt!(
            "^8= (^7%d^8 +^7 %d^8 - 1) ^ 2 /^7 %d^8 ^ 2",
            waveRadius.num(),
            enemyRadius.num(),
            fullRadius.num()
        ));
        // L12192
        OverlapChance.push(lfmt!("^8=^7 %.3f^8%%", overlapChance * 100.0));
        // L12193
        let PulsesPerTrap = Table::new();
        breakdown.set(n!("PulsesPerTrap"), PulsesPerTrap.clone());
        // L12194
        PulsesPerTrap.push("Pulses per trap:");
        // L12195
        PulsesPerTrap.push(lfmt!("%.3f ^8(unrounded skill duration)", duration));
        // L12196
        PulsesPerTrap.push(lfmt!("* %.2f ^8(pulse rate)", wavePulseRate));
        // L12197
        PulsesPerTrap.push(lfmt!("= %.2f ^8pulses", duration * wavePulseRate));
        // L12198
        PulsesPerTrap.push("^8rounded down");
        // L12199
        PulsesPerTrap.push(lfmt!("= %d ^8pulses", pulses));
        // L12200
        PulsesPerTrap.push(lfmt!(
            "^8Next breakpoint: %d%% increased Duration",
            m_ceil(
                100.0
                    * ((pulses + 1.0)
                        / (wavePulseRate * skillData.num(n!("duration")) * moreDuration)
                        - incDuration)
            )
        ));
        // L12203
        PulsesPerTrap.push(lfmt!(
            "^8Previous breakpoint: %d%% reduced Duration",
            -m_ceil(
                100.0
                    * (pulses / (wavePulseRate * skillData.num(n!("duration")) * moreDuration)
                        - incDuration)
                    - 1.0
            )
        ));
        // L12206
        let AverageActiveTraps = Table::new();
        breakdown.set(n!("AverageActiveTraps"), AverageActiveTraps.clone());
        // L12207
        AverageActiveTraps.push("Average active traps, not considering stored cooldown uses:");
        // L12208
        AverageActiveTraps.push(lfmt!(
            "%.2f^8 /^7 %.2f^8 (pulses / pulse rate = effective skill duration)",
            pulses,
            wavePulseRate
        ));
        // L12209
        AverageActiveTraps.push(lfmt!("/ %.2f ^8(cooldown)", cooldown.num()));
        // L12210
        AverageActiveTraps.push(lfmt!("= %.2f traps", averageActiveTraps));
    }
    // L12212
    let maxWaves = skillModList.Sum(BASE, Some(&skillCfg), nm!["MaximumWaves"]);
    // L12213
    let mut dpsMultiplier = 1.0;
    // L12214
    if skillPart == Some(2.0) {
        // L12215
        dpsMultiplier = maxWaves * overlapChance;
        // L12216
        if let Some(breakdown) = breakdown {
            // L12217
            let SkillDPSMultiplier = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), SkillDPSMultiplier.clone());
            // L12218
            SkillDPSMultiplier.push("DPS multiplier");
            // L12219
            SkillDPSMultiplier.push("^8= ^7maximum waves^8 * ^7overlap chance^8");
            // L12220
            SkillDPSMultiplier.push(lfmt!("^8=^7 %d^8 *^7 %.2f^8", maxWaves, overlapChance));
            // L12221
            SkillDPSMultiplier.push(lfmt!("^8=^7 %.3f", dpsMultiplier));
        }
    } else if skillPart == Some(3.0) {
        // L12224
        dpsMultiplier = maxWaves;
        // L12225
        if let Some(breakdown) = breakdown {
            // L12226
            let SkillDPSMultiplier = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), SkillDPSMultiplier.clone());
            // L12227
            SkillDPSMultiplier.push("DPS multiplier");
            // L12228
            SkillDPSMultiplier.push(lfmt!("^8=^7 %d (maximum waves)", dpsMultiplier));
        }
    } else if skillPart == Some(4.0) {
        // L12231
        dpsMultiplier = averageActiveTraps;
        // L12232
        if let Some(breakdown) = breakdown {
            // L12233
            let SkillDPSMultiplier = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), SkillDPSMultiplier.clone());
            // L12234
            SkillDPSMultiplier.push("DPS multiplier");
            // L12235
            SkillDPSMultiplier.push(lfmt!("^8=^7 %.2f (average active traps)", dpsMultiplier));
        }
    } else if skillPart == Some(5.0) {
        // L12238
        dpsMultiplier = averageActiveTraps * maxWaves * overlapChance;
        // L12239
        if let Some(breakdown) = breakdown {
            // L12240
            let SkillDPSMultiplier = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), SkillDPSMultiplier.clone());
            // L12241
            SkillDPSMultiplier.push("DPS multiplier");
            // L12242
            SkillDPSMultiplier
                .push("^8= ^7average active traps^8 * ^7maximum waves^8 * ^7overlap chance^8");
            // L12243
            SkillDPSMultiplier.push(lfmt!(
                "^8=^7 %.2f^8 *^7 %d^8 *^7 %.2f",
                averageActiveTraps,
                maxWaves,
                overlapChance
            ));
            // L12244
            SkillDPSMultiplier.push(lfmt!("^8=^7 %.3f", dpsMultiplier));
        }
    } else if skillPart == Some(6.0) {
        // L12247
        dpsMultiplier = averageActiveTraps * maxWaves;
        // L12248
        if let Some(breakdown) = breakdown {
            // L12249
            let SkillDPSMultiplier = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), SkillDPSMultiplier.clone());
            // L12250
            SkillDPSMultiplier.push("DPS multiplier");
            // L12251
            SkillDPSMultiplier.push("^8= ^7average active traps^8 * ^7maximum waves");
            // L12252
            SkillDPSMultiplier.push(lfmt!("^8=^7 %.2f^8 *^7 %d", averageActiveTraps, maxWaves));
            // L12253
            SkillDPSMultiplier.push(lfmt!("^8=^7 %.3f", dpsMultiplier));
        }
    }
    // L12256
    if dpsMultiplier != 1.0 {
        // L12257
        let v = skillData.num_or(n!("dpsMultiplier"), 1.0) * dpsMultiplier;
        skillData.set(n!("dpsMultiplier"), v);
        // L12258
        let v = output.num_or(n!("SkillDPSMultiplier"), 1.0) * dpsMultiplier;
        output.set(n!("SkillDPSMultiplier"), v);
    }
}

/// @pob Data/Skills/act_int.lua:12642-12650 h=faf8e954
/// `data.skills.LightningTendrilsAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn LightningTendrilsAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // DPS multiplier applied to the skills to reflect the DPS from each skill part
    // L12644
    let interval = activeSkill.skillData.get().get(n!("pulseInterval"));
    // L12645
    if activeSkill.skillPart.get() == Some(2.0) {
        // L12646
        activeSkill.skillModList.get().NewMod(
            n!("DPS"),
            MORE,
            -(1.0 / interval.num()) * 100.0,
            "Normal pulse",
            0,
            0,
            &[tag!({ type = SkillPart, skillPart = 2.0 })],
        );
    } else if activeSkill.skillPart.get() == Some(3.0) {
        // L12648
        activeSkill.skillModList.get().NewMod(
            n!("DPS"),
            MORE,
            -(interval.num() - 1.0) / interval.num() * 100.0,
            "Stronger pulse",
            0,
            0,
            &[tag!({ type = SkillPart, skillPart = 3.0 })],
        );
    }
}

/// @pob Data/Skills/act_int.lua:12651-12662 h=890dcd7f
/// `data.skills.LightningTendrilsAltX.postCritFunc` (postCritFunc)
pub(crate) fn LightningTendrilsAltX_postCritFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // Formula to find a effective damage multiplier to take into account the 500% more damage on every 5th hit
    // L12653
    if activeSkill.skillPart.get() == Some(1.0) {
        let skillData = activeSkill.skillData.get();
        // L12654
        let interval = skillData.num(n!("pulseInterval"));
        // L12655
        let pulseDamage = skillData.num(n!("pulseDamage")) / 100.0;
        // L12656
        let critChance = output.num(n!("PreEffectiveCritChance")) / 100.0;
        // L12657
        let effectiveCritChance = output.num(n!("CritChance")) / 100.0;
        // L12658
        let critMulti = output.num(n!("CritMultiplier"));
        // L12659
        let averageMore = 100.0
            * (((interval - 1.0) * (1.0 + critChance * (critMulti - 1.0))
                + (1.0 + pulseDamage) * critMulti)
                / (interval * ((1.0 - effectiveCritChance) + critMulti * effectiveCritChance))
                - 1.0);
        // L12660: `nil` flags are skipped by createMod (flags 0), so the keyword flags are
        // the 3rd vararg and the tag the 4th (ModTools.lua:42-48).
        activeSkill.skillModList.get().NewMod(
            n!("Damage"),
            MORE,
            averageMore,
            "Average Pulse Damage",
            0,
            bor(KeywordFlag::Hit, KeywordFlag::Ailment),
            &[tag!({ type = SkillPart, skillPart = 1.0 })],
        );
    }
}

/// @pob Data/Skills/act_int.lua:13527-13532 h=048c8a3b
/// `data.skills.Manabond.preDamageFunc` (preDamageFunc)
pub(crate) fn Manabond_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L13528
    let missingUnreservedManaPercentage = activeSkill
        .skillData
        .get()
        .num_or(n!("ManabondMissingUnreservedManaPercentage"), 100.0);
    // L13529
    let manaGainedAsBaseLightningDamage = m_floor(
        (activeSkill
            .skillData
            .get()
            .num(n!("ManabondMissingManaGainPercent"))
            / 100.0)
            * (missingUnreservedManaPercentage / 100.0)
            * output.num_or(n!("ManaUnreserved"), 0.0),
    );
    // L13530
    activeSkill.skillModList.get().NewMod(
        n!("LightningMin"),
        BASE,
        manaGainedAsBaseLightningDamage,
        "Manabond gain % missing unreserved mana as base lightning damage",
        0,
        0,
        &[],
    );
    // L13531
    activeSkill.skillModList.get().NewMod(
        n!("LightningMax"),
        BASE,
        manaGainedAsBaseLightningDamage,
        "Manabond gain % missing unreserved mana as base lightning damage",
        0,
        0,
        &[],
    );
}

/// @pob Data/Skills/act_int.lua:13621-13623 h=ee56aaa4
/// `data.skills.OrbOfStorms.preDamageFunc` (preDamageFunc)
pub(crate) fn OrbOfStorms_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L13622
    let v = activeSkill.skillData.get().num(n!("hitFrequency"))
        / calcLib::r#mod(
            &activeSkill.skillModList.get(),
            Some(&activeSkill.skillCfg.get()),
            nm!["Speed"],
        );
    activeSkill.skillData.get().set(n!("hitTimeOverride"), v);
}

/// @pob Data/Skills/act_int.lua:13809-13811 h=6c38aaad
/// `data.skills.PenanceBrand.preDamageFunc` (preDamageFunc)
pub(crate) fn PenanceBrand_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L13810
    let v = activeSkill.skillData.get().num(n!("repeatFrequency"))
        * activeSkill.skillModList.get().Sum(
            BASE,
            Some(&activeSkill.skillCfg.get()),
            nm!["Multiplier:PenanceBrandMaxEnergy"],
        )
        / (1.0
            + activeSkill.skillModList.get().Sum(
                INC,
                Some(&activeSkill.skillCfg.get()),
                nm!["Speed", "BrandActivationFrequency"],
            ) / 100.0)
        / activeSkill.skillModList.get().More(
            Some(&activeSkill.skillCfg.get()),
            nm!["BrandActivationFrequency"],
        );
    activeSkill.skillData.get().set(n!("hitTimeOverride"), v);
}

/// @pob Data/Skills/act_int.lua:13901-13903 h=557be66a
/// `data.skills.PenanceBrandAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn PenanceBrandAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L13902
    let hitTimeOverride = activeSkill.skillData.get().num(n!("repeatFrequency"))
        / (1.0
            + activeSkill.skillModList.get().Sum(
                INC,
                Some(&activeSkill.skillCfg.get()),
                nm!["Speed", "BrandActivationFrequency"],
            ) / 100.0)
        / activeSkill.skillModList.get().More(
            Some(&activeSkill.skillCfg.get()),
            nm!["BrandActivationFrequency"],
        );
    activeSkill
        .skillData
        .get()
        .set(n!("hitTimeOverride"), hitTimeOverride);
}

/// @pob Data/Skills/act_int.lua:14015-14017 h=557be66a
/// `data.skills.PenanceBrandAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn PenanceBrandAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L14016
    let hitTimeOverride = activeSkill.skillData.get().num(n!("repeatFrequency"))
        / (1.0
            + activeSkill.skillModList.get().Sum(
                INC,
                Some(&activeSkill.skillCfg.get()),
                nm!["Speed", "BrandActivationFrequency"],
            ) / 100.0)
        / activeSkill.skillModList.get().More(
            Some(&activeSkill.skillCfg.get()),
            nm!["BrandActivationFrequency"],
        );
    activeSkill
        .skillData
        .get()
        .set(n!("hitTimeOverride"), hitTimeOverride);
}

/// @pob Data/Skills/act_int.lua:15428-15434 h=5a3564ba
/// `data.skills.RighteousFire.preDamageFunc` (preDamageFunc)
pub(crate) fn RighteousFire_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L15429
    if activeSkill.skillFlags.get().truthy(n!("totem")) && output.num(n!("TotemLife")) > 1.0 {
        // L15430
        let v = output.num(n!("TotemLife")) * skillData.num(n!("RFLifeMultiplier"))
            + output.num(n!("TotemEnergyShield")) * skillData.num(n!("RFESMultiplier"));
        skillData.set(n!("FireDot"), v);
    } else if output.num(n!("LifeUnreserved")) > 1.0 {
        // L15432
        let v = output.num(n!("Life")) * skillData.num(n!("RFLifeMultiplier"))
            + output.num(n!("EnergyShield")) * skillData.num(n!("RFESMultiplier"));
        skillData.set(n!("FireDot"), v);
    }
}

/// @pob Data/Skills/act_int.lua:15533-15537 h=7fbb670a
/// `data.skills.RighteousFireAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn RighteousFireAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L15534
    if output.num(n!("LifeUnreserved")) > 1.0 {
        // L15535
        let skillData = activeSkill.skillData.get();
        let v = output.num(n!("Mana")) * skillData.num(n!("RFManaMultiplier"));
        skillData.set(n!("FireDot"), v);
    }
}

/// @pob Data/Skills/act_int.lua:15625-15631 h=294abdee
/// `data.skills.VaalRighteousFire.preDamageFunc` (preDamageFunc)
pub(crate) fn VaalRighteousFire_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L15626
    if activeSkill.skillFlags.get().truthy(n!("totem")) {
        // L15627
        let v = output.num(n!("TotemLife"))
            * skillData.num(n!("percentSacrificed"))
            * skillData.num(n!("RFMultiplier"));
        skillData.set(n!("FireDot"), v);
    } else {
        // L15629
        let v = (output.num(n!("Life")) + output.num(n!("EnergyShield")))
            * skillData.num(n!("percentSacrificed"))
            * skillData.num(n!("RFMultiplier"));
        skillData.set(n!("FireDot"), v);
    }
}

/// @pob Data/Skills/act_int.lua:16721-16727 h=c11e51ab
/// `data.skills.Spark.preDamageFunc` (preDamageFunc)
pub(crate) fn Spark_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L16722
    let skillData = activeSkill.skillData.get();
    // L16723
    if activeSkill.skillPart.get() == Some(2.0) {
        // L16724
        let v = (skillData.num_or(n!("dpsMultiplier"), 1.0))
            * (1.0 + m_floor(output.num(n!("Duration")) / 0.66));
        skillData.set(n!("dpsMultiplier"), v);
        // L16725
        output.set(n!("SkillDPSMultiplier"), skillData.get(n!("dpsMultiplier")));
    }
}

/// @pob Data/Skills/act_int.lua:16811-16817 h=c11e51ab
/// `data.skills.SparkAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn SparkAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L16812
    let skillData = activeSkill.skillData.get();
    // L16813
    if activeSkill.skillPart.get() == Some(2.0) {
        // L16814
        let v = (skillData.num_or(n!("dpsMultiplier"), 1.0))
            * (1.0 + m_floor(output.num(n!("Duration")) / 0.66));
        skillData.set(n!("dpsMultiplier"), v);
        // L16815
        output.set(n!("SkillDPSMultiplier"), skillData.get(n!("dpsMultiplier")));
    }
}

/// @pob Data/Skills/act_int.lua:16902-16908 h=c11e51ab
/// `data.skills.SparkAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn SparkAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L16903
    let skillData = activeSkill.skillData.get();
    // L16904
    if activeSkill.skillPart.get() == Some(2.0) {
        // L16905
        let v = (skillData.num_or(n!("dpsMultiplier"), 1.0))
            * (1.0 + m_floor(output.num(n!("Duration")) / 0.66));
        skillData.set(n!("dpsMultiplier"), v);
        // L16906
        output.set(n!("SkillDPSMultiplier"), skillData.get(n!("dpsMultiplier")));
    }
}

/// @pob Data/Skills/act_int.lua:17373-17375 h=557be66a
/// `data.skills.StormBrand.preDamageFunc` (preDamageFunc)
pub(crate) fn StormBrand_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L17374
    let hitTimeOverride = activeSkill.skillData.get().num(n!("repeatFrequency"))
        / (1.0
            + activeSkill.skillModList.get().Sum(
                INC,
                Some(&activeSkill.skillCfg.get()),
                nm!["Speed", "BrandActivationFrequency"],
            ) / 100.0)
        / activeSkill.skillModList.get().More(
            Some(&activeSkill.skillCfg.get()),
            nm!["BrandActivationFrequency"],
        );
    activeSkill
        .skillData
        .get()
        .set(n!("hitTimeOverride"), hitTimeOverride);
}

/// @pob Data/Skills/act_int.lua:17470-17472 h=557be66a
/// `data.skills.StormBrandAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn StormBrandAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L17471
    let hitTimeOverride = activeSkill.skillData.get().num(n!("repeatFrequency"))
        / (1.0
            + activeSkill.skillModList.get().Sum(
                INC,
                Some(&activeSkill.skillCfg.get()),
                nm!["Speed", "BrandActivationFrequency"],
            ) / 100.0)
        / activeSkill.skillModList.get().More(
            Some(&activeSkill.skillCfg.get()),
            nm!["BrandActivationFrequency"],
        );
    activeSkill
        .skillData
        .get()
        .set(n!("hitTimeOverride"), hitTimeOverride);
}

/// @pob Data/Skills/act_int.lua:18036-18048 h=f573ec31
/// `data.skills.StormBurst.preDamageFunc` (preDamageFunc)
pub(crate) fn StormBurst_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L18037
    if activeSkill.skillPart.get() == Some(2.0) {
        // L18038
        let duration = skillData.num(n!("duration")) * output.num(n!("DurationMod"));
        // duration * 10 / (jump * 10), instead of duration / jump to avoid floating point issues
        // L18040
        let jumpPeriod = skillData.num(n!("repeatFrequency")) * 10.0;
        // additional 1 tick upon spawn of orb
        // L18042
        let v = skillData.num_or(n!("dpsMultiplier"), 1.0)
            * (1.0 + m_floor(duration * 10.0 / jumpPeriod));
        skillData.set(n!("dpsMultiplier"), v);
    } else if activeSkill.skillPart.get() == Some(3.0) {
        // L18044
        let remainingJumps = m_floor(
            (skillData.num(n!("duration")) * output.num(n!("DurationMod")) + 0.001)
                / skillData.num(n!("repeatFrequency")),
        );
        // Tested in-game and the skill grants more damage only after you have at least 0.8s duration
        // L18046
        activeSkill.skillModList.get().NewMod(
            n!("Damage"),
            MORE,
            m_max(
                skillData.num(n!("remainingDurationDamage")) * remainingJumps - 100.0,
                0.0,
            ),
            "Skill:StormBurst",
            0,
            0,
            &[],
        );
    }
}

/// @pob Data/Skills/act_int.lua:18151-18157 h=3ffac2b8
/// `data.skills.StormBurstAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn StormBurstAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L18152
    if activeSkill.skillPart.get() == Some(2.0) {
        // L18153
        let remainingJumps = m_floor(
            (skillData.num(n!("duration")) * output.num(n!("DurationMod")) + 0.001)
                / skillData.num(n!("repeatFrequency")),
        );
        // Tested in-game and the skill grants more damage only after you have at least 0.8s duration
        // L18155
        activeSkill.skillModList.get().NewMod(
            n!("Damage"),
            MORE,
            m_max(
                skillData.num(n!("remainingDurationDamage")) * remainingJumps - 100.0,
                0.0,
            ),
            "Skill:StormBurst",
            0,
            0,
            &[],
        );
    }
}

/// @pob Data/Skills/act_int.lua:20267-20269 h=1b32abd3
/// `data.skills.VoidSphere.preDamageFunc` (preDamageFunc)
pub(crate) fn VoidSphere_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L20268
    let v = activeSkill.skillData.get().num(n!("repeatFrequency"))
        / (1.0
            + activeSkill.skillModList.get().Sum(
                INC,
                Some(&activeSkill.skillCfg.get()),
                nm!["VoidSphereFrequency"],
            ) / 100.0);
    activeSkill.skillData.get().set(n!("hitTimeOverride"), v);
}

/// @pob Data/Skills/act_int.lua:20356-20358 h=1b32abd3
/// `data.skills.VoidSphereAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn VoidSphereAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L20357
    let v = activeSkill.skillData.get().num(n!("repeatFrequency"))
        / (1.0
            + activeSkill.skillModList.get().Sum(
                INC,
                Some(&activeSkill.skillCfg.get()),
                nm!["VoidSphereFrequency"],
            ) / 100.0);
    activeSkill.skillData.get().set(n!("hitTimeOverride"), v);
}

/// @pob Data/Skills/act_int.lua:20445-20448 h=119adb7b
/// `data.skills.VoltaxicBurst.preDamageFunc` (preDamageFunc)
pub(crate) fn VoltaxicBurst_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L20446
    let duration = m_floor(
        activeSkill.skillData.get().num(n!("duration")) * output.num(n!("DurationMod")) * 10.0,
    );
    // L20447
    let v = activeSkill.skillModList.get().Sum(
        INC,
        Some(&activeSkill.skillCfg.get()),
        nm!["VoltaxicDurationIncDamage"],
    ) * duration;
    activeSkill
        .skillModList
        .get()
        .NewMod(n!("Damage"), INC, v, "Skill:VoltaxicBurst", 0, 0, &[]);
}

/// @pob Data/Skills/act_int.lua:20542-20544 h=0a437538
/// `data.skills.Vortex.preDamageFunc` (preDamageFunc)
pub(crate) fn Vortex_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L20543
    activeSkill
        .skillData
        .get()
        .set(n!("hitTimeOverride"), output.get(n!("Cooldown")));
}

/// @pob Data/Skills/act_int.lua:20629-20631 h=0a437538
/// `data.skills.VortexAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn VortexAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L20630
    activeSkill
        .skillData
        .get()
        .set(n!("hitTimeOverride"), output.get(n!("Cooldown")));
}

/// @pob Data/Skills/act_int.lua:20795-20800 h=916a8a0d
/// `data.skills.WaveOfConviction.preDamageFunc` (preDamageFunc)
pub(crate) fn WaveOfConviction_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L20796
    if skillData.truthy(n!("duration")) {
        // L20797
        let duration = m_floor(
            m_ceil(skillData.num(n!("duration")) * data().misc.ServerTickRate)
                / data().misc.ServerTickRate
                * output.num(n!("DurationMod"))
                * 10.0,
        );
        // L20798: `NewMod(…, "<src>", 0, { tag })`: the tag table is the 3rd vararg, so
        // keywordFlags stays 0 (ModTools.lua:42-48).
        let v = activeSkill.skillModList.get().Sum(
            INC,
            Some(&activeSkill.skillCfg.get()),
            nm!["WaveOfConvictionDurationDotMulti"],
        ) * duration
            / 100.0;
        activeSkill.skillModList.get().NewMod(
            n!("DotMultiplier"),
            BASE,
            v,
            "Skill:WaveOfConviction",
            0,
            0,
            &[tag!({ type = Multiplier, var = "WoCDurationExpired" })],
        );
    }
}

/// @pob Data/Skills/act_int.lua:20889-20894 h=16bf7003
/// `data.skills.WaveOfConvictionAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn WaveOfConvictionAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L20890
    if skillData.truthy(n!("duration")) {
        // L20891
        let duration = m_floor(
            m_ceil(skillData.num(n!("duration")) * data().misc.ServerTickRate)
                / data().misc.ServerTickRate
                * output.num(n!("DurationMod"))
                * 10.0,
        );
        // L20892: `NewMod(…, "<src>", 0, { tag })`: the tag table is the 3rd vararg, so
        // keywordFlags stays 0 (ModTools.lua:42-48).
        let v = activeSkill.skillModList.get().Sum(
            INC,
            Some(&activeSkill.skillCfg.get()),
            nm!["WaveOfConvictionDurationDotMulti"],
        ) * duration
            / 100.0;
        activeSkill.skillModList.get().NewMod(
            n!("DotMultiplier"),
            BASE,
            v,
            "Skill:WaveOfConvictionAltY",
            0,
            0,
            &[tag!({ type = Multiplier, var = "WoCDurationExpired" })],
        );
    }
}

/// @pob Data/Skills/act_int.lua:20983-20987 h=406f34ad
/// `data.skills.WinterOrb.preDamageFunc` (preDamageFunc)
pub(crate) fn WinterOrb_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L20984
    let rateMod = 1.0
        + activeSkill.skillModList.get().Sum(
            INC,
            Some(&activeSkill.skillCfg.get()),
            nm!["HitRate", "Speed"],
        ) / 100.0;
    // L20985
    let mult = activeSkill
        .skillModList
        .get()
        .More(Some(&activeSkill.skillCfg.get()), nm!["HitRate"]);
    // L20986
    let v = activeSkill.skillData.get().num(n!("repeatFrequency")) / rateMod / mult;
    activeSkill.skillData.get().set(n!("hitTimeOverride"), v);
}

/// @pob Data/Skills/act_int.lua:21107-21131 h=c10af8bf
/// `data.skills.WintertideBrand.preDamageFunc` (preDamageFunc)
pub(crate) fn WintertideBrand_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L21108
    activeSkill.skillData.get().set(
        n!("countsAttachedBrandsInDamage"),
        activeSkill.skillPart.get() == Some(1.0),
    );
    // L21109
    let hitTimeOverride = activeSkill.skillData.get().num(n!("repeatFrequency"))
        / (1.0
            + activeSkill.skillModList.get().Sum(
                INC,
                Some(&activeSkill.skillCfg.get()),
                nm!["Speed", "BrandActivationFrequency"],
            ) / 100.0)
        / activeSkill.skillModList.get().More(
            Some(&activeSkill.skillCfg.get()),
            nm!["BrandActivationFrequency"],
        );
    activeSkill
        .skillData
        .get()
        .set(n!("hitTimeOverride"), hitTimeOverride);
    // L21110
    if activeSkill.skillPart.get() == Some(1.0) {
        let skillModList = activeSkill.skillModList.get();
        let skillCfg = activeSkill.skillCfg.get();
        let skillData = activeSkill.skillData.get();
        // L21111
        let skillMaxStages = skillModList.Sum(
            BASE,
            Some(&skillCfg),
            nm!["Multiplier:WintertideBrandMaxStages"],
        );
        // L21112
        let enemy = activeSkill
            .actor()
            .and_then(|a| a.enemy())
            .unwrap_or_else(|| lua_error!("attempt to index field 'enemy' (a nil value)"));
        let debuffDurationMult = 1.0
            / m_max(
                data().misc.BuffExpirationSlowCap,
                calcLib::r#mod(&enemy.modDB.get(), Some(&skillCfg), nm!["BuffExpireFaster"]),
            );
        // L21113
        // (`{}`: an env table without `mode_effective` and no enemyDB, CCR-0046)
        let duration = calcSkillDuration(&skillModList, Some(&skillCfg), &skillData, None, None)
            * debuffDurationMult;
        // L21114
        let maxStages = m_min(
            m_floor(duration / skillData.num(n!("hitTimeOverride"))),
            skillMaxStages,
        );
        // L21115
        let timeToReachMaxStages = maxStages * skillData.num(n!("hitTimeOverride"));
        // L21116
        let timeAtMaxStages = m_max(duration - timeToReachMaxStages, 0.0);
        // L21117
        let damagePerStage = skillModList.Sum(
            BASE,
            Some(&skillCfg),
            nm!["Multiplier:WintertideBrandDamagePerStage"],
        );
        // Each activation adds one stage; average the completed stages over the attached duration
        // L21119
        let averageStages = (maxStages * (maxStages - 1.0) / 2.0
            * skillData.num(n!("hitTimeOverride"))
            + maxStages * timeAtMaxStages)
            / duration;
        // L21120
        let averageDamageMultiplier = averageStages * damagePerStage;
        // L21121
        let endDurationBase = skillData.num_or(n!("durationTertiary"), 0.0)
            + skillModList.Sum(BASE, Some(&skillCfg), nm!["Duration", "TertiaryDuration"]);
        // L21122
        let endDurationMod = m_max(
            calcLib::r#mod(
                &skillModList,
                Some(&skillCfg),
                nm!["Duration", "TertiaryDuration"],
            ),
            0.0,
        );
        // L21123
        let endDuration = m_ceil(
            endDurationBase * endDurationMod * debuffDurationMult * data().misc.ServerTickRate,
        ) / data().misc.ServerTickRate;
        // L21124
        let endUptime = m_min(endDuration / duration, 1.0);
        // L21125
        let attachedBrandCount = skillData.get(n!("attachedBrandCount"));
        // Attached Wintertide debuffs stack, but only the strongest maximum-stage Wintertide's End debuff deals damage
        // L21127
        let endDamageMultiplier = maxStages * damagePerStage;
        // L21128
        let dpsMultiplier = attachedBrandCount.num() * (100.0 + averageDamageMultiplier)
            + endUptime * (100.0 + endDamageMultiplier)
            - 100.0;
        // L21129
        skillModList.NewMod(
            n!("Damage"),
            MORE,
            dpsMultiplier,
            "Wintertide Brand Average Multiplier",
            0,
            0,
            &[],
        );
    }
}
