//! @pob-file Data/Skills/other.lua @16de4b82d
//! @pob-file Data/Skills/minion.lua @16de4b82d
//! @pob-file Data/Skills/spectre.lua @16de4b82d
//! @pob-file Data/Skills/sup_str.lua @16de4b82d
//! Skill hooks of Data/Skills/other.lua, Data/Skills/minion.lua, Data/Skills/spectre.lua, Data/Skills/sup_str.lua (calc doc §8: `preDamageFunc`, `initialFunc`, `preSkillTypeFunc`,
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
    // L670
    if output.num(n!("LifeReservedPercent")) >= 100.0 {
        // L671
        return;
    }
    let skillData = activeSkill.skillData.get();
    let skillModList = activeSkill.skillModList.get();
    // L673
    let lifeReservedPercent = skillData.num_or(n!("LifeReservedPercent"), 3.0);
    // L674
    let lifeReserved = skillData.num_or(n!("LifeReservedBase"), m_huge);
    // L675
    skillModList.NewMod(
        n!("Multiplier:ChannelledLifeReservedPercentPerStage"),
        ModType::BASE,
        lifeReservedPercent,
        "Blood Sacrament",
        0,
        0,
        &[],
    );
    // L676
    skillModList.NewMod(
        n!("Multiplier:ChannelledLifeReservedPerStage"),
        ModType::BASE,
        lifeReserved,
        "Blood Sacrament",
        0,
        0,
        &[],
    );
}

/// @pob Data/Skills/other.lua:1282-1288 h=1642217a
/// `data.skills.DeathWish.preDamageFunc` (preDamageFunc)
pub(crate) fn DeathWish_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L1283
    if activeSkill.skillPart.get() == Some(2.0) {
        // L1284
        let skillData = activeSkill.skillData.get();
        // L1285
        let v = output.num(n!("Life")) * skillData.num(n!("selfFireExplosionLifeMultiplier"));
        skillData.set(n!("FireBonusMin"), v);
        // L1286
        let v = output.num(n!("Life")) * skillData.num(n!("selfFireExplosionLifeMultiplier"));
        skillData.set(n!("FireBonusMax"), v);
    }
}

/// @pob Data/Skills/other.lua:2258-2260 h=40093632
/// `data.skills.HeraldOfTheBreach.preDamageFunc` (preDamageFunc)
pub(crate) fn HeraldOfTheBreach_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L2259
    let v = skillData.num(n!("repeatFrequency"))
        / (1.0
            + activeSkill.skillModList.get().Sum(
                ModType::INC,
                Some(&*activeSkill.skillCfg.get()),
                nm!["PulseFrequencyPerPressure"],
            ) / 100.0);
    skillData.set(n!("hitTimeOverride"), v);
}

/// @pob Data/Skills/other.lua:2596-2598 h=0a437538
/// `data.skills.LightningBolt.preDamageFunc` (preDamageFunc)
pub(crate) fn LightningBolt_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L2597
    let v = output.get(n!("Cooldown"));
    activeSkill.skillData.get().set(n!("hitTimeOverride"), v);
}

/// @pob Data/Skills/other.lua:6204-6252 h=e2a13f5d
/// `data.skills.EnemyExplode.preDamageFunc` (preDamageFunc)
pub(crate) fn EnemyExplode_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    let skillModList = activeSkill.skillModList.get();
    let skillCfg = activeSkill.skillCfg.get();
    // L6205
    output.set(n!("ExplodeChance"), 0.0);
    // L6206
    if activeSkill.skillPart.get() != Some(3.0) {
        // L6207
        let allExplodeMods =
            skillModList.Tabulate(Some(ModType::LIST), Some(&*skillCfg), nm!["ExplodeMod"]);
        // L6208
        for_ipairs!((_, explodeMod) in allExplodeMods => {
            // L6209
            let activeEffectSource = explode_source_name(activeSkill);
            // L6210
            if explodeMod.r#mod.source.as_deref() == Some(&*activeEffectSource) {
                // L6211
                let explodeMod = explodeMod.value.clone();
                let ty = value_field(&explodeMod, n!("type"));
                // L6212
                if ty.str().as_deref() == Some("RandomElement") {
                    let amount = value_field(&explodeMod, n!("amount")).num();
                    // L6213
                    skillData.set(n!("FireEffectiveExplodePercentage"), amount / 3.0);
                    // L6214
                    skillData.set(n!("ColdEffectiveExplodePercentage"), amount / 3.0);
                    // L6215
                    skillData.set(n!("LightningEffectiveExplodePercentage"), amount / 3.0);
                } else {
                    // L6217
                    let k = cat!(concat_str(&ty), "EffectiveExplodePercentage");
                    skillData.set(k, value_field(&explodeMod, n!("amount")));
                }
                // L6219
                let v = if activeSkill.skillPart.get() == Some(2.0) {
                    Val::from(1.0)
                } else {
                    value_field(&explodeMod, n!("chance"))
                };
                output.set(n!("ExplodeChance"), v);
            }
        });
    } else {
        // L6223 (insertion order; see the LUA-ORDER notes below)
        let mut typeAmountChances: Vec<(Name, Vec<(f64, f64)>)> = Vec::new();
        // L6224
        let explodeModList = skillModList.List(Some(&*skillCfg), nm!["ExplodeMod"]);
        // L6225
        for_ipairs!((_, explodeMod) in explodeModList => {
            let ty = Name::from(&*concat_str(&value_field(&explodeMod, n!("type"))));
            let amount = value_field(&explodeMod, n!("amount")).num();
            // L6226-6228
            let pos = match typeAmountChances.iter().position(|(t, _)| *t == ty) {
                Some(p) => p,
                None => {
                    typeAmountChances.push((ty, Vec::new()));
                    typeAmountChances.len() - 1
                }
            };
            let amountChance = &mut typeAmountChances[pos].1;
            // L6227
            match amountChance.iter().position(|(a, _)| *a == amount) {
                Some(p) => {
                    let c = amountChance[p].1 + value_field(&explodeMod, n!("chance")).num();
                    amountChance[p].1 = c;
                }
                None => {
                    let c = 0.0 + value_field(&explodeMod, n!("chance")).num();
                    amountChance.push((amount, c));
                }
            }
        });
        // L6230
        // LUA-ORDER: string keys (pairs_inventory other.lua:6230), insertion order; only float
        // rounding of the sums depends on it.
        for (r#type, amountChance) in &typeAmountChances {
            // L6231
            let mut physExplodeChance = 0.0;
            // L6232
            // LUA-ORDER: numeric keys (pairs_inventory other.lua:6232), insertion order; only float
            // rounding of the sums depends on it.
            for &(amount, chance) in amountChance {
                // L6233
                let amountXChance = amount * chance;
                // L6234
                if r#type.as_str() == "RandomElement" {
                    // L6235
                    let v = skillData.num_or(n!("FireEffectiveExplodePercentage"), 0.0)
                        + amountXChance / 3.0;
                    skillData.set(n!("FireEffectiveExplodePercentage"), v);
                    // L6236
                    let v = skillData.num_or(n!("ColdEffectiveExplodePercentage"), 0.0)
                        + amountXChance / 3.0;
                    skillData.set(n!("ColdEffectiveExplodePercentage"), v);
                    // L6237
                    let v = skillData.num_or(n!("LightningEffectiveExplodePercentage"), 0.0)
                        + amountXChance / 3.0;
                    skillData.set(n!("LightningEffectiveExplodePercentage"), v);
                } else {
                    // L6239
                    let k = cat!(*r#type, "EffectiveExplodePercentage");
                    let v = skillData.num_or(k, 0.0) + amountXChance;
                    skillData.set(k, v);
                }
                // L6241
                if r#type.as_str() == "Physical" {
                    // L6242
                    physExplodeChance = 1.0 - ((1.0 - physExplodeChance) * (1.0 - chance));
                }
                // L6244
                let v = 1.0 - ((1.0 - output.num(n!("ExplodeChance"))) * (1.0 - chance));
                output.set(n!("ExplodeChance"), v);
            }
            // L6246
            if r#type.as_str() == "Physical" && physExplodeChance != 0.0 {
                // L6247 (`NewMod` without source, flags or tags: `self:AddMod(mod_createMod(...))`)
                skillModList.AddMod(Arc::new(pob_gamedata::mod_tools::createMod(
                    n!("CalcArmourAsThoughDealing"),
                    ModType::MORE,
                    100.0 / m_min(physExplodeChance, 1.0) - 100.0,
                    None,
                    0,
                    0,
                    &[],
                )));
            }
        }
    }
    // L6251
    let v = m_min(output.num(n!("ExplodeChance")) * 100.0, 100.0);
    output.set(n!("ExplodeChance"), v);
}

/// @pob Data/Skills/minion.lua:756-758 h=83a41d9a
/// `data.skills.GAZombieCorpseGroundImpact.preDamageFunc` (preDamageFunc)
pub(crate) fn GAZombieCorpseGroundImpact_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L757
    let summonSkill = activeSkill
        .summonSkill()
        .unwrap_or_else(|| lua_error!("attempt to index field 'summonSkill' (a nil value)"));
    let v = 1.0 / summonSkill.skillData.get().num_or(n!("summonSpeed"), 1.0);
    activeSkill.skillData.get().set(n!("hitTimeOverride"), v);
}

/// @pob Data/Skills/minion.lua:1756-1758 h=83a41d9a
/// `data.skills.MinionInstability.preDamageFunc` (preDamageFunc)
pub(crate) fn MinionInstability_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L757
    let summonSkill = activeSkill
        .summonSkill()
        .unwrap_or_else(|| lua_error!("attempt to index field 'summonSkill' (a nil value)"));
    let v = 1.0 / summonSkill.skillData.get().num_or(n!("summonSpeed"), 1.0);
    activeSkill.skillData.get().set(n!("hitTimeOverride"), v);
}

/// @pob Data/Skills/spectre.lua:7277-7279 h=5645a921
/// `data.skills.AzmeriHydraBarrage.preDamageFunc` (preDamageFunc)
pub(crate) fn AzmeriHydraBarrage_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L7278
    activeSkill
        .skillData
        .get()
        .set(n!("dpsMultiplier"), output.get(n!("ProjectileCount")));
}

/// @pob Data/Skills/sup_str.lua:2540-2548 h=ecbae34c
/// `data.skills.AvengingFlame.preDamageFunc` (preDamageFunc)
pub(crate) fn AvengingFlame_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L2541
    let uuid = skillData.get(n!("triggerSourceUUID"));
    // L2542
    let cache = if uuid.truthy() {
        let uuid = Name::from(&*concat_str(&uuid));
        cachedData(Mode::MAIN, uuid).or_else(|| cachedData(Mode::CALCS, uuid))
    } else {
        None
    };
    // L2543
    let totemLife = match &cache {
        Some(cache) => {
            let v = cache.Env.player.output.get().get(n!("TotemLife"));
            v.num_or(0.0)
        }
        None => 0.0,
    };

    // L2545
    let add = totemLife * skillData.num(n!("lifeDealtAsFire")) / 100.0;
    // L2546
    let v = skillData.num_or(n!("FireMax"), 0.0) + add;
    skillData.set(n!("FireMax"), v);
    // L2547
    let v = skillData.num_or(n!("FireMin"), 0.0) + add;
    skillData.set(n!("FireMin"), v);
}

// ---- helpers ------------------------------------------------------------------------------

/// `v.<k>` on a mod value (`{ type = …, amount = …, chance = … }`); indexing a non-table raises.
fn value_field(v: &Value, k: Name) -> Val {
    match v {
        Value::Table(t) => Val::from(t.get(k).clone()),
        other => lua_error!(
            "attempt to index a {} value (field '{}')",
            other.lua_type(),
            k
        ),
    }
}

/// A string or number operand of `..` (anything else raises, as in Lua).
fn concat_str(v: &Val) -> String {
    match v {
        Val::Str(s) => s.to_string(),
        Val::Num(x) => tostring_num(*x),
        other => lua_error!("attempt to concatenate a {} value", other.lua_type()),
    }
}

/// `src.<k>` of an explode source: a tree node, an item or a plain table (CalcSetup.lua:838,
/// 1421-1423 fill `env.explodeSources`; 1541 stores one as `gemInstance.explodeSource`).
fn source_field(src: &Val, k: &str) -> Val {
    if let Some(item) = src.obj::<Item>() {
        return match k {
            "modSource" => item.modSource.get().map_or(Val::Nil, Val::from),
            "id" => Val::from(item.id.get()),
            _ => item.extra.get(Name::from(k)),
        };
    }
    if let Some(node) = src.obj::<Node>() {
        return match k {
            "id" => Val::from(node.id),
            _ => Val::Nil,
        };
    }
    src.get(Name::from(k))
}

/// `activeSkill.activeEffect.srcInstance.explodeSource.modSource or
/// "Tree:"..activeSkill.activeEffect.srcInstance.explodeSource.id` (other.lua:6209).
fn explode_source_name(activeSkill: &ActiveSkillRef) -> String {
    let srcInstance = activeSkill
        .activeEffect
        .get()
        .srcInstance
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'srcInstance' (a nil value)"));
    let explodeSource = srcInstance.extra.get(n!("explodeSource"));
    let modSource = source_field(&explodeSource, "modSource");
    if modSource.truthy() {
        concat_str(&modSource)
    } else {
        format!("Tree:{}", concat_str(&source_field(&explodeSource, "id")))
    }
}
