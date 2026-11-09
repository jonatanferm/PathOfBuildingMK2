//! @pob-file Modules/CalcPerform.lua @16de4b82d
//! perform segment PF-F (CalcPerform.lua 2997-3496): party buffs, curse application.
//!
//! Owner: PF-F. Ported from the U0 stub (signature frozen, calc doc §7.2).
//!
//! Representation notes (Rust-only plumbing, snake_case helpers at the end of the file):
//! * A Lua ModList that lives in a dynamic table (`buffs[name]`, `curse.modList`,
//!   `buff.modList` of a party buff) is `Val::Obj(ModStoreRef)` (how ORC-X injects `$store`
//!   tables and how PartyTab creates them), or a plain list of `Val::Mod`. [`mods_of`] reads
//!   either as the list `ipairs` sees.
//! * `mergeBuff`'s frozen `src` is a `&Table` (calc doc §7.2, actor_a.rs): a freshly built
//!   `srcList` ModList is handed over as the table of its mods ([`mod_table`]).
//! * `modList.notBuff` (CalcPerform.lua:3346) is a key on a ModList object, which the frozen
//!   `ModStore` cannot hold (CCR-0030 row 7, CCR-0076): [`notBuff`] reads it from a table-shaped buff
//!   and is false for a store (`// CCR-0030`).
#![allow(non_snake_case, clippy::all, clippy::pedantic)]

use super::actor_a::{
    determineCursePriority, doActorLifeMana, doActorLifeManaReservation, mergeBuff,
};
use super::actor_b::{addMinionModifiers, doActorCharges, doActorMisc};
use super::setup_flasks::mergeFlasks;
use super::PerformCtx;
use crate::modules::calc_active_skill::mergeSkillInstanceMods;
use crate::modules::calc_defence::helpers::{defenceForConditionals, resistances};
use crate::modules::calc_tools;
use crate::modules::mod_tools_env::mergeKeystones;
use crate::prelude::*;
use pob_gamedata::mod_tools::createMod;
use pob_gamedata::{tag, ValueTable};

/// @pob Modules/CalcPerform.lua:2997-3496 h=5d0affd9
/// PF-F: CalcPerform.lua 2997-3496 (party buffs, curse application).
pub(crate) fn seg(ctx: &mut PerformCtx) -> Flow {
    let env = ctx.env.clone();
    let modDB = ctx.modDB.clone();
    let enemyDB = ctx.enemyDB.clone();
    let output = ctx.output.clone();
    let buffs = ctx.buffs.clone();
    let guards = ctx.guards.clone();
    let minionBuffs = ctx.minionBuffs.clone();
    let debuffs = ctx.debuffs.clone();
    let curses = ctx.curses.clone();
    let minionCurses = ctx.minionCurses.clone();
    let buffExports = ctx.buffExports.clone();
    let guaranteedChillSkills = ctx.guaranteedChillSkills.clone();
    let nonUniqueFlasksApplyToMinion = ctx.nonUniqueFlasksApplyToMinion;
    let allyBuffs = || -> Table {
        ctx.allyBuffs
            .clone()
            .unwrap_or_else(|| lua_error!("attempt to index upvalue 'allyBuffs' (a nil value)"))
    };

    // L2997
    if allyBuffs().truthy(n!("otherEffects")) {
        // L2998: LUA-ORDER (string keys; each buff writes its own condition and buffs key)
        for (buffName, buff) in table_of(&allyBuffs().get(n!("otherEffects"))).pairs() {
            let buffName = key_str(buffName);
            // L2999
            modDB
                .conditions
                .set(cat!("AffectedBy", gsub_space(&buffName)), true);
            // L3000
            let inc = modDB.Sum(
                ModType::INC,
                None,
                nm!["BuffEffectOnSelf", "AuraEffectOnSelf"],
            );
            // L3001
            let more = modDB.More(None, nm!["BuffEffectOnSelf", "AuraEffectOnSelf"]);
            // L3002
            let srcList = ModStore::ModList(None);
            // L3003
            srcList.ScaleAddList(
                &mods_of(&buff.get(n!("modList"))),
                (buff.get(n!("effectMult")).num() + inc) / 100.0 * more,
                false,
            );
            // L3004
            mergeBuff(&mod_table(&srcList), &buffs, Name::from(&*buffName));
            // L3005
            if let Some(minion) = env.minion.get() {
                // L3006
                minion
                    .modDB
                    .get()
                    .conditions
                    .set(cat!("AffectedBy", gsub_space(&buffName)), true);
                // L3007
                let inc = minion.modDB.get().Sum(
                    ModType::INC,
                    None,
                    nm!["BuffEffectOnSelf", "AuraEffectOnSelf"],
                );
                // L3008
                let more = minion
                    .modDB
                    .get()
                    .More(None, nm!["BuffEffectOnSelf", "AuraEffectOnSelf"]);
                // L3009
                let srcList = ModStore::ModList(None);
                // L3010
                srcList.ScaleAddList(
                    &mods_of(&buff.get(n!("modList"))),
                    (buff.get(n!("effectMult")).num() + inc) / 100.0 * more,
                    false,
                );
                // L3011
                mergeBuff(&mod_table(&srcList), &minionBuffs, Name::from(&*buffName));
            }
        }
    }
    // L3015
    if allyBuffs().truthy(n!("Aura")) {
        // L3016: LUA-ORDER (string keys)
        for (auraName, aura) in table_of(&allyBuffs().get(n!("Aura"))).pairs() {
            let auraName = key_str(auraName);
            // L3017
            if &*auraName != "Vaal" {
                // L3018
                let auraNameCompressed = gsub_space(&auraName);
                // L3019
                if !modDB.Flag(None, nm!["AlliesAurasCannotAffectSelf"])
                    && !modDB
                        .conditions
                        .truthy(cat!("AffectedBy", auraNameCompressed))
                {
                    // L3020
                    modDB.conditions.set(n!("AffectedByAura"), true);
                    // L3021
                    modDB
                        .conditions
                        .set(cat!("AffectedBy", auraNameCompressed), true);
                    // L3022
                    let srcList = ModStore::ModList(None);
                    // L3023
                    srcList.ScaleAddList(
                        &mods_of(&aura.get(n!("modList"))),
                        aura.get(n!("effectMult")).num() / 100.0,
                        false,
                    );
                    // L3024
                    mergeBuff(&mod_table(&srcList), &buffs, Name::from(&*auraName));
                }
                // L3026
                if let Some(minion) = env.minion.get().filter(|m| {
                    !m.modDB
                        .get()
                        .conditions
                        .truthy(cat!("AffectedBy", auraNameCompressed))
                }) {
                    // L3027
                    minion
                        .modDB
                        .get()
                        .conditions
                        .set(n!("AffectedByAura"), true);
                    // L3028
                    minion
                        .modDB
                        .get()
                        .conditions
                        .set(cat!("AffectedBy", auraNameCompressed), true);
                    // L3029
                    let srcList = ModStore::ModList(None);
                    // L3030
                    srcList.ScaleAddList(
                        &mods_of(&aura.get(n!("modList"))),
                        aura.get(n!("effectMult")).num() / 100.0,
                        false,
                    );
                    // L3031
                    mergeBuff(&mod_table(&srcList), &minionBuffs, Name::from(&*auraName));
                }
            }
        }
        // L3035
        if table_of(&allyBuffs().get(n!("Aura"))).truthy(n!("Vaal")) {
            // L3036: LUA-ORDER (string keys)
            for (auraName, aura) in
                table_of(&table_of(&allyBuffs().get(n!("Aura"))).get(n!("Vaal"))).pairs()
            {
                let auraName = key_str(auraName);
                // L3037
                let auraNameCompressed = gsub_space(&auraName);
                // L3038
                if !modDB.Flag(None, nm!["AlliesAurasCannotAffectSelf"])
                    && !modDB
                        .conditions
                        .truthy(cat!("AffectedBy", auraNameCompressed))
                {
                    // L3039
                    modDB.conditions.set(n!("AffectedByAura"), true);
                    // L3040
                    modDB.conditions.set(
                        cat!(
                            "AffectedBy",
                            gsub_space(&pob_lua::string::sub(&auraName, 6, None))
                        ),
                        true,
                    );
                    // L3041
                    modDB
                        .conditions
                        .set(cat!("AffectedBy", auraNameCompressed), true);
                    // L3042
                    let srcList = ModStore::ModList(None);
                    // L3043
                    srcList.ScaleAddList(
                        &mods_of(&aura.get(n!("modList"))),
                        aura.get(n!("effectMult")).num() / 100.0,
                        false,
                    );
                    // L3044
                    mergeBuff(&mod_table(&srcList), &buffs, Name::from(&*auraName));
                }
                // L3046
                if let Some(minion) = env.minion.get().filter(|m| {
                    !m.modDB
                        .get()
                        .conditions
                        .truthy(cat!("AffectedBy", auraNameCompressed))
                }) {
                    // L3047
                    minion
                        .modDB
                        .get()
                        .conditions
                        .set(n!("AffectedByAura"), true);
                    // L3048
                    minion
                        .modDB
                        .get()
                        .conditions
                        .set(cat!("AffectedBy", auraNameCompressed), true);
                    // L3049
                    let srcList = ModStore::ModList(None);
                    // L3050
                    srcList.ScaleAddList(
                        &mods_of(&aura.get(n!("modList"))),
                        aura.get(n!("effectMult")).num() / 100.0,
                        false,
                    );
                    // L3051
                    mergeBuff(&mod_table(&srcList), &minionBuffs, Name::from(&*auraName));
                }
            }
        }
    }
    // L3056
    if allyBuffs().truthy(n!("AuraDebuff")) && env.mode_effective.get() {
        // L3057: LUA-ORDER (string keys)
        for (auraName, aura) in table_of(&allyBuffs().get(n!("AuraDebuff"))).pairs() {
            let auraName = key_str(auraName);
            // L3058
            if &*auraName != "Vaal" {
                // L3059
                let auraNameCompressed = gsub_space(&auraName);
                // L3060
                if !enemyDB
                    .conditions
                    .truthy(cat!("AffectedBy", auraNameCompressed))
                {
                    // L3061
                    enemyDB
                        .conditions
                        .set(cat!("AffectedBy", auraNameCompressed), true);
                    // L3062
                    modDB
                        .conditions
                        .set(cat!("AffectedBy", auraNameCompressed), true);
                    // L3063
                    let srcList = ModStore::ModList(None);
                    // L3064
                    srcList.ScaleAddList(
                        &mods_of(&aura.get(n!("modList"))),
                        aura.get(n!("effectMult")).num() / 100.0,
                        false,
                    );
                    // L3065
                    mergeBuff(&mod_table(&srcList), &debuffs, Name::from(&*auraName));
                }
            }
        }
        // L3069
        if table_of(&allyBuffs().get(n!("AuraDebuff"))).truthy(n!("Vaal")) {
            // L3070: LUA-ORDER (string keys)
            for (auraName, aura) in
                table_of(&table_of(&allyBuffs().get(n!("AuraDebuff"))).get(n!("Vaal"))).pairs()
            {
                let auraName = key_str(auraName);
                // L3071
                let auraNameCompressed = gsub_space(&auraName);
                // L3072
                if !enemyDB
                    .conditions
                    .truthy(cat!("AffectedBy", auraNameCompressed))
                {
                    // L3073
                    enemyDB
                        .conditions
                        .set(cat!("AffectedBy", auraNameCompressed), true);
                    // L3074
                    modDB
                        .conditions
                        .set(cat!("AffectedBy", auraNameCompressed), true);
                    // L3075
                    let srcList = ModStore::ModList(None);
                    // L3076
                    srcList.ScaleAddList(
                        &mods_of(&aura.get(n!("modList"))),
                        aura.get(n!("effectMult")).num() / 100.0,
                        false,
                    );
                    // L3077
                    mergeBuff(&mod_table(&srcList), &debuffs, Name::from(&*auraName));
                }
            }
        }
    }
    // L3082
    let partyWarcry = party_list(&env, n!("Warcry"));
    if partyWarcry.truthy() && partyWarcry.get(n!("Warcry")).truthy() {
        // L3083: LUA-ORDER (string keys)
        for (warcryName, warcry) in table_of(&partyWarcry.get(n!("Warcry"))).pairs() {
            let warcryName = key_str(warcryName);
            // L3084
            let warcryNameCompressed = gsub_space(&warcryName);
            // L3085
            if !modDB
                .conditions
                .truthy(cat!("AffectedBy", warcryNameCompressed))
            {
                // L3086
                modDB.conditions.set(n!("AffectedByWarcry"), true);
                // L3087
                modDB
                    .conditions
                    .set(cat!("AffectedBy", warcryNameCompressed), true);
                // L3088
                let srcList = ModStore::ModList(None);
                // L3089
                for_ipairs!((_i, warcryBuff) in mods_of(&warcry.get(n!("modList"))) => {
                    // L3090
                    srcList.ScaleAddList(
                        &[warcryBuff.clone()],
                        warcry.get(n!("effectMult")).num_or(100.0) / 100.0
                            * warcryPowerBonus(&warcryBuff).num_or(1.0),
                        false,
                    );
                });
                // L3092
                mergeBuff(&mod_table(&srcList), &buffs, Name::from(&*warcryName));
            }
            // L3094
            if let Some(minion) = env.minion.get().filter(|m| {
                !m.modDB
                    .get()
                    .conditions
                    .truthy(cat!("AffectedBy", warcryNameCompressed))
            }) {
                // L3095
                minion
                    .modDB
                    .get()
                    .conditions
                    .set(n!("AffectedByWarcry"), true);
                // L3096
                minion
                    .modDB
                    .get()
                    .conditions
                    .set(cat!("AffectedBy", warcryNameCompressed), true);
                // L3097
                let srcList = ModStore::ModList(None);
                // L3098
                for_ipairs!((_i, warcryBuff) in mods_of(&warcry.get(n!("modList"))) => {
                    // L3099
                    srcList.ScaleAddList(
                        &[warcryBuff.clone()],
                        warcry.get(n!("effectMult")).num_or(100.0) / 100.0
                            * warcryPowerBonus(&warcryBuff).num_or(1.0),
                        false,
                    );
                });
                // L3101
                mergeBuff(&mod_table(&srcList), &minionBuffs, Name::from(&*warcryName));
            }
        }
    }
    // L3105
    let partyLink = party_list(&env, n!("Link"));
    if partyLink.truthy() && partyLink.get(n!("Link")).truthy() {
        // L3106: LUA-ORDER (string keys)
        for (linkName, link) in table_of(&partyLink.get(n!("Link"))).pairs() {
            let linkName = key_str(linkName);
            // L3107
            let linkNameCompressed = gsub_space(&linkName);
            // L3108
            if !modDB
                .conditions
                .truthy(cat!("AffectedBy", linkNameCompressed))
            {
                // L3109
                modDB.conditions.set(n!("AffectedByLink"), true);
                // L3110
                modDB
                    .conditions
                    .set(cat!("AffectedBy", linkNameCompressed), true);
                // L3111
                let srcList = ModStore::ModList(None);
                // L3112
                srcList.ScaleAddList(
                    &mods_of(&link.get(n!("modList"))),
                    link.get(n!("effectMult")).num_or(100.0) / 100.0,
                    false,
                );
                // L3113
                mergeBuff(&mod_table(&srcList), &buffs, Name::from(&*linkName));
            }
        }
    }

    // L3118
    if env.mode_combat.get() {
        // This needs to be done in 2 steps to account for effects affecting life recovery from flasks
        // For example Sorrow of the Divine and buffs (like flask recovery watchers eye)
        // L3121
        let flasks = env.flasks.get().with(|m| m.clone());
        mergeFlasks(ctx, &flasks, true, nonUniqueFlasksApplyToMinion);
    }

    // Process stats from alternate charges
    // L3125
    if env.mode_combat.get() {
        // L3126
        if modDB.Flag(None, nm!["UseEnduranceCharges"])
            && modDB.Flag(None, nm!["EnduranceChargesConvertToBrutalCharges"])
        {
            // L3127
            let tripleDmgChancePerEndurance =
                modDB.Sum(ModType::BASE, None, nm!["PerBrutalTripleDamageChance"]);
            // L3128: LUA-QUIRK: the tag is the first vararg, so createMod takes it as a tag and
            // the mod has no source (ModTools.lua:38: `source` is only a string first vararg).
            modDB.AddMod(Arc::new(createMod(
                n!("TripleDamageChance"),
                ModType::BASE,
                tripleDmgChancePerEndurance,
                None,
                0,
                0,
                &[tag!({ type = Multiplier, var = "BrutalCharge" })],
            )));
        }
        // L3130
        if modDB.Flag(None, nm!["UseFrenzyCharges"])
            && modDB.Flag(None, nm!["FrenzyChargesConvertToAfflictionCharges"])
        {
            // L3131
            let dmgPerAffliction =
                modDB.Sum(ModType::BASE, None, nm!["PerAfflictionAilmentDamage"]);
            // L3132
            let effectPerAffliction =
                modDB.Sum(ModType::BASE, None, nm!["PerAfflictionNonDamageEffect"]);
            // L3133
            modDB.NewMod(
                n!("Damage"),
                ModType::MORE,
                dmgPerAffliction,
                "Affliction Charges",
                0,
                KeywordFlag::Ailment,
                &[tag!({ type = Multiplier, var = "AfflictionCharge" })],
            );
            // L3134-3139
            for name in [
                n!("EnemyChillEffect"),
                n!("EnemyShockEffect"),
                n!("EnemyFreezeEffect"),
                n!("EnemyScorchEffect"),
                n!("EnemyBrittleEffect"),
                n!("EnemySapEffect"),
            ] {
                modDB.NewMod(
                    name,
                    ModType::MORE,
                    effectPerAffliction,
                    "Affliction Charges",
                    0,
                    0,
                    &[tag!({ type = Multiplier, var = "AfflictionCharge" })],
                );
            }
        }
    }

    // Check for extra curses
    // L3144: `pairs({[curses] = modDB, [minionCurses] = env.minion and env.minion.modDB})`.
    // LUA-ORDER: table-identity keys (random in PoB); insertion order here (§3.4). The two
    // entries write disjoint state (their own dest list and their own modDB).
    let mut curseDests: Vec<(Table, ModDB)> = vec![(curses.clone(), modDB.clone())];
    if let Some(minion) = env.minion.get() {
        curseDests.push((minionCurses.clone(), minion.modDB.get()));
    }
    for (dest, modDB) in curseDests {
        // L3145
        if env.mode_combat.get() && modDB.Flag(None, nm!["Condition:OnConsecratedGround"]) {
            // L3146
            let effect = 1.0 + modDB.Sum(ModType::INC, None, nm!["ConsecratedGroundEffect"]) / 100.0;
            // L3147
            modDB.NewMod(
                n!("CurseEffectOnSelf"),
                ModType::INC,
                -50.0 * effect,
                "Consecrated Ground",
                0,
                0,
                &[],
            );
        }
        // L3149
        for_ipairs!((_i, value) in modDB.List(None, nm!["ExtraCurse"]) => {
            // L3150
            let gemModList = ModStore::ModList(None);
            // L3151
            let grantedEffect = value_name(value.get(n!("skillId")))
                .and_then(|id| env.data.skills.get(&id).cloned());
            // L3152
            if let Some(grantedEffect) = grantedEffect {
                // L3153-3157
                mergeSkillInstanceMods(
                    &env,
                    &gemModList,
                    &SkillEffect::new(
                        grantedEffect.clone(),
                        value.get(n!("level")).num().unwrap_or_else(|| {
                            lua_error!("ExtraCurse value without a numeric level")
                        }),
                        0.0,
                    ),
                    None,
                );
                // L3158
                let mut curseModList: Vec<ModRef> = Vec::new();
                // L3159
                for_ipairs!((_j, r#mod) in gemModList.ipairs() => {
                    // L3160
                    for tag in r#mod.tags.iter() {
                        // L3161
                        if tag.r#type == TagType::GlobalEffect
                            && tag.effectType == Some(n!("Curse"))
                        {
                            // L3162
                            curseModList.push(r#mod.clone());
                            // L3163
                            break;
                        }
                    }
                });
                // L3167
                if value.get(n!("applyToPlayer")).truthy() {
                    // Sources for curses on the player don't usually respect any kind of limit, so there's little point bothering with slots
                    // L3169
                    if modDB.Sum(ModType::BASE, None, nm!["AvoidCurse"]) < 100.0 {
                        // L3170
                        modDB.conditions.set(n!("Cursed"), true);
                        // L3171
                        let v = modDB.multipliers.num0(n!("CurseOnSelf")) + 1.0;
                        modDB.multipliers.set(n!("CurseOnSelf"), v);
                        // L3172
                        modDB.conditions.set(
                            cat!("AffectedBy", gsub_space(grantedEffect.name.as_str())),
                            true,
                        );
                        // L3173
                        let cfg = ModCfg::new();
                        cfg.skillName.set(Some(grantedEffect.name));
                        let cfg = &*cfg;
                        // L3174
                        let inc = modDB.Sum(ModType::INC, Some(cfg), nm!["CurseEffectOnSelf"])
                            + gemModList.Sum(ModType::INC, None, nm!["CurseEffectAgainstPlayer"]);
                        // L3175
                        let more = modDB.More(Some(cfg), nm!["CurseEffectOnSelf"])
                            * gemModList.More(None, nm!["CurseEffectAgainstPlayer"]);
                        // L3176
                        modDB.ScaleAddList(
                            &curseModList,
                            m_max((1.0 + inc / 100.0) * more, 0.0),
                            false,
                        );
                    }
                } else if !enemyDB.Flag(None, nm!["Hexproof"])
                    || modDB.Flag(None, nm!["CursesIgnoreHexproof"])
                {
                    // L3179
                    let curse = tbl! {
                        name = grantedEffect.name,
                        fromPlayer = dest.ptr_eq(&curses),
                        priority = determineCursePriority(grantedEffect.name, None),
                    };
                    // L3184
                    curse.set(n!("modList"), store_val(ModStore::ModList(None)));
                    // L3185
                    let scale = (1.0 + enemyDB.Sum(ModType::INC, None, nm!["CurseEffectOnSelf"]) / 100.0)
                        * enemyDB.More(None, nm!["CurseEffectOnSelf"]);
                    store_of(&curse.get(n!("modList"))).ScaleAddList(&curseModList, scale, false);
                    // L3186
                    dest.push(curse);
                }
            }
        });
    }
    // L3191
    let allyCurses = Table::new();
    // L3192
    let mut allyPartyCurses = party_list(&env, n!("Curse"));
    // L3193
    if allyPartyCurses.get(n!("Curse")).truthy() {
        // L3194
        allyCurses.set(n!("limit"), allyPartyCurses.get(n!("limit")));
    } else {
        // L3196
        allyPartyCurses = Val::Table(tbl! { Curse = Table::new() });
    }
    // L3198: LUA-ORDER (string keys; the order of allyCurses decides curse slots)
    for (curseName, curse) in table_of(&allyPartyCurses.get(n!("Curse"))).pairs() {
        let curseName = key_str(curseName);
        // L3199
        let newCurse = tbl! {
            name = Name::from(&*curseName),
            priority = 0.0,
            modList = store_val(ModStore::ModList(None)),
        };
        // L3204
        let mut mult = curse.get(n!("effectMult")).num() / 100.0;
        // L3205
        if curse.get(n!("isMark")).truthy() {
            // L3206
            newCurse.set(n!("isMark"), true);
        } else {
            // L3208
            mult = mult * enemyDB.More(None, nm!["CurseEffectOnSelf"]);
        }
        // L3210
        store_of(&newCurse.get(n!("modList"))).ScaleAddList(
            &mods_of(&curse.get(n!("modList"))),
            mult,
            false,
        );
        // L3211
        allyCurses.push(newCurse);
    }

    // Set curse limit
    // L3216
    let EnemyCurseLimit = if modDB.Flag(None, nm!["CurseLimitIsMaximumPowerCharges"])
        && output.truthy(n!("PowerChargesMax"))
    {
        output.get(n!("PowerChargesMax"))
    } else {
        Val::Num(modDB.Sum(ModType::BASE, None, nm!["EnemyCurseLimit"]))
    };
    output.set(n!("EnemyCurseLimit"), EnemyCurseLimit);
    // L3217
    curses.set(n!("limit"), output.get(n!("EnemyCurseLimit")));
    // L3218
    buffExports.set(n!("CurseLimit"), curses.get(n!("limit")));
    // Assign curses to slots
    // L3220
    let curseSlots = Table::new();
    // L3221
    env.curseSlots.set(curseSlots.clone());
    // Currently assume only 1 mark is possible
    // L3223
    let mut markSlotted = false;
    // L3224
    for_ipairs!((_s, source) in vec![curses.clone(), minionCurses.clone(), allyCurses.clone()] => {
        // L3225
        for_ipairs!((_c, curse) in source => {
            let curse = table_of(&curse);
            // Calculate curses that ignore hex limit after
            // L3227
            if !curse.truthy(n!("ignoreHexLimit")) && !curse.truthy(n!("socketedCursesHexLimit")) {
                // L3228
                let mut slot: Option<i64> = None;
                // L3229
                let mut skipAddingCurse = false;
                // Check if we need to disable a certain curse aura.
                // L3231
                for_ipairs!((_a, activeSkill) in env.player.activeSkillList.get() => {
                    // L3232
                    let buff1 = activeSkill.buffList.get().get(1);
                    if buff1.is_some()
                        && curse.get(n!("name")).raw_eq(&buff1.as_ref().expect("buffList[1]").get(n!("name")))
                        && activeSkill.skillTypes.get().with(|t| t.has(SkillType::Aura))
                    {
                        // L3233
                        if modDB.Flag(None, nm!["SelfAurasOnlyAffectYou"]) || {
                            let skillCfg = main_skill(&env.player).skillCfg.get();
                            activeSkill
                                .skillModList
                                .get()
                                .Flag(Some(&*skillCfg), nm!["SelfAurasAffectYouAndLinkedTarget"])
                        } {
                            // L3234
                            skipAddingCurse = true;
                        }
                        // L3236
                        break;
                    }
                });
                // L3239
                let limit = source.get(n!("limit"));
                let limit = limit.opt_num().unwrap_or_else(|| {
                    lua_error!("'for' limit must be a number")
                });
                let mut i = 1.0;
                while i <= limit {
                    let ii = i as i64;
                    // Prevent multiple marks from being considered
                    // L3241
                    if curse.truthy(n!("isMark")) {
                        // L3242
                        if markSlotted {
                            // L3243
                            slot = None;
                            // L3244
                            break;
                        }
                    }
                    // L3247
                    if !curseSlots.truthy(ii) {
                        // L3248
                        slot = Some(ii);
                        // L3249
                        break;
                    } else if table_of(&curseSlots.get(ii))
                        .get(n!("name"))
                        .raw_eq(&curse.get(n!("name")))
                    {
                        // L3251
                        if table_of(&curseSlots.get(ii)).num(n!("priority")) < curse.num(n!("priority")) {
                            // L3252
                            slot = Some(ii);
                        } else {
                            // L3254
                            slot = None;
                        }
                        // L3256
                        break;
                    } else if table_of(&curseSlots.get(ii)).num(n!("priority")) < curse.num(n!("priority")) {
                        // L3258
                        slot = Some(ii);
                    }
                    i += 1.0;
                }
                // L3261
                if let Some(slot) = slot {
                    // L3262
                    if curseSlots.truthy(slot) && table_of(&curseSlots.get(slot)).truthy(n!("isMark")) {
                        // L3263
                        markSlotted = false;
                    }
                    // L3265
                    if skipAddingCurse == false {
                        // L3266
                        curseSlots.set(slot, curse.clone());
                    }
                    // L3268
                    if curse.truthy(n!("isMark")) {
                        // L3269
                        markSlotted = true;
                    }
                }
            }
        });
    });

    // L3276
    for_ipairs!((_s, source) in vec![curses.clone(), minionCurses.clone()] => {
        // L3277
        for_ipairs!((_c, curse) in source => {
            let curse = table_of(&curse);
            // L3278
            if curse.truthy(n!("ignoreHexLimit")) {
                // L3279
                let mut skipAddingCurse = false;
                // L3280
                let n = curseSlots.len() as i64;
                let mut i = 1;
                while i <= n {
                    // L3281
                    if table_of(&curseSlots.get(i)).get(n!("name")).raw_eq(&curse.get(n!("name"))) {
                        // if curse is higher priority, replace current curse with it, otherwise if same or lower priority skip it entirely
                        // L3283
                        if table_of(&curseSlots.get(i)).num(n!("priority")) < curse.num(n!("priority")) {
                            // L3284
                            curseSlots.set(i, curse.clone());
                        }
                        // L3286
                        skipAddingCurse = true;
                        // L3287
                        break;
                    }
                    i += 1;
                }
                // L3290
                if !skipAddingCurse {
                    // L3291
                    curseSlots.set(curseSlots.len() as i64 + 1, curse.clone());
                }
            }
            // L3294
            if curse.truthy(n!("socketedCursesHexLimit")) {
                // L3295
                let socketedCursesHexLimitValue =
                    modDB.Sum(ModType::BASE, None, nm!["SocketedCursesHexLimitValue"]);
                // L3296
                let mut skipAddingCurse = false;
                // L3297
                let n = curseSlots.len() as i64;
                let mut i = 1;
                while i <= n {
                    // L3298
                    if table_of(&curseSlots.get(i)).get(n!("name")).raw_eq(&curse.get(n!("name"))) {
                        // if curse is higher priority, replace current curse with it, otherwise if same or lower priority skip it entirely
                        // L3300
                        if table_of(&curseSlots.get(i)).num(n!("priority")) < curse.num(n!("priority")) {
                            // L3301
                            curseSlots.set(i, curse.clone());
                        }
                        // L3303
                        skipAddingCurse = true;
                        // L3304
                        break;
                    }
                    // L3306
                    if i as f64 >= socketedCursesHexLimitValue {
                        // L3307
                        skipAddingCurse = true;
                    }
                    i += 1;
                }
                // L3310
                if !skipAddingCurse {
                    // L3311
                    curseSlots.set(curseSlots.len() as i64 + 1, curse.clone());
                }
            }
        });
    });

    // Process guard buffs
    // L3318
    let mut guardSlots: Vec<(Name, Val)> = Vec::new();
    // L3319
    let mut nonVaal = false;
    // L3320: LUA-ORDER (string keys; with several guards the first match wins)
    for (name, modList) in guards.pairs() {
        let name = Name::from(&*key_str(name));
        // L3321
        if name.as_str() == "Vaal Molten Shell" {
            // L3322
            guardSlots.clear();
            // L3323
            nonVaal = false;
            // L3324
            guardSlots.push((name, modList));
            // L3325
            break;
        } else if pob_lua::lmatch(name.as_str(), "^Vaal", None).is_some() {
            // L3327
            guardSlots.push((name, modList));
        } else if !nonVaal {
            // L3329
            guardSlots.push((name, modList));
            // L3330
            nonVaal = true;
        }
    }
    // L3333
    if nonVaal {
        // L3334
        modDB
            .conditions
            .set(n!("AffectedByNonVaalGuardSkill"), true);
    }
    // L3336
    for (guard_name, guard_modList) in &guardSlots {
        // L3337
        modDB.conditions.set(n!("AffectedByGuardSkill"), true);
        // L3338
        modDB
            .conditions
            .set(cat!("AffectedBy", gsub_space(guard_name.as_str())), true);
        // L3339
        mergeBuff(&mods_table(&mods_of(guard_modList)), &buffs, *guard_name);
    }
    // L3341
    let GuardSkillActive = lor(modDB.conditions.get(n!("AffectedByGuardSkill")), || {
        Val::Bool(false)
    });
    output.set(n!("GuardSkillActive"), GuardSkillActive);

    // Apply buff/debuff modifiers
    // L3344: LUA-ORDER (string keys; AddList order per mod name)
    for (_k, modList) in buffs.pairs() {
        // L3345
        modDB.AddList(&mods_of(&modList));
        // L3346
        if !notBuff(&modList) {
            // L3347
            let v = modDB.multipliers.num0(n!("BuffOnSelf")) + 1.0;
            modDB.multipliers.set(n!("BuffOnSelf"), v);
        }
        // L3349
        if let Some(minion) = env.minion.get() {
            // L3350
            let skillCfg = main_skill(&env.player).skillCfg.get();
            addMinionModifiers(&as_mod_list(&modList), Some(&*skillCfg), &minion);
        }
    }
    // L3353
    if let Some(minion) = env.minion.get() {
        // L3354: LUA-ORDER (string keys)
        for (_k, modList) in minionBuffs.pairs() {
            // L3355
            minion.modDB.get().AddList(&mods_of(&modList));
        }
    }
    // L3358: LUA-ORDER (string keys)
    for (_k, modList) in debuffs.pairs() {
        // L3359
        enemyDB.AddList(&mods_of(&modList));
    }
    // L3361
    modDB
        .multipliers
        .set(n!("CurseOnEnemy"), curseSlots.len() as f64);
    // L3362
    for_ipairs!((_i, slot) in curseSlots => {
        let slot = table_of(&slot);
        // L3363
        enemyDB.conditions.set(n!("Cursed"), true);
        // L3364
        if slot.truthy(n!("isMark")) {
            // L3365
            enemyDB.conditions.set(n!("Marked"), true);
        }
        // L3367
        if slot.truthy(n!("modList")) {
            // L3368
            enemyDB.AddList(&mods_of(&slot.get(n!("modList"))));
        }
        // L3370
        if slot.truthy(n!("buffModList")) {
            // L3371
            modDB.AddList(&mods_of(&slot.get(n!("buffModList"))));
        }
        // L3373
        if slot.truthy(n!("minionBuffModList")) {
            // L3374
            let minion = env.minion.get().unwrap_or_else(|| {
                lua_error!("attempt to index field 'minion' (a nil value)")
            });
            minion
                .modDB
                .get()
                .AddList(&mods_of(&slot.get(n!("minionBuffModList"))));
        }
    });

    // Calculate guaranteed chills after buffs so chilling areas benefit from active buff modifiers.
    // L3379
    for_ipairs!((_i, activeSkill) in guaranteedChillSkills => {
        let activeSkill = activeSkill
            .obj::<ActiveSkill>()
            .unwrap_or_else(|| lua_error!("guaranteedChillSkills entry is not an active skill"));
        // L3380
        let chillDefault = env
            .data
            .nonDamagingAilment
            .get(&n!("Chill"))
            .and_then(|a| a.default)
            .unwrap_or_else(|| lua_error!("attempt to perform arithmetic on field 'default' (a nil value)"));
        let effect = chillDefault
            * calc_tools::r#mod(
                &activeSkill.skillModList.get(),
                Some(&*activeSkill.skillCfg.get()),
                nm!["EnemyChillEffect"],
            );
        // L3381
        modDB.NewMod(
            n!("ChillOverride"),
            ModType::BASE,
            effect,
            activeSkill.activeEffect.get().grantedEffect.get().name.as_str(),
            0,
            0,
            &[],
        );
    });

    // Fix the configured impale stacks on the enemy
    // 		If the config is missing (blank), then use the maximum number of stacks
    //		If the config is larger than the maximum number of stacks, replace it with the correct maximum
    // L3387
    let maxImpaleStacks = modDB.Sum(ModType::BASE, None, nm!["ImpaleStacksMax"])
        * (1.0 + modDB.Sum(ModType::BASE, None, nm!["ImpaleAdditionalDurationChance"]) / 100.0);
    // L3388
    if !enemyDB.HasMod(ModType::BASE, None, nm!["Multiplier:ImpaleStacks"]) {
        // L3389
        enemyDB.NewMod(
            n!("Multiplier:ImpaleStacks"),
            ModType::BASE,
            maxImpaleStacks,
            "Config",
            0,
            0,
            &[tag!({ type = Condition, var = "Combat" })],
        );
    } else if enemyDB.Sum(ModType::BASE, None, nm!["Multiplier:ImpaleStacks"]) > maxImpaleStacks {
        // L3391
        enemyDB.ReplaceMod(
            n!("Multiplier:ImpaleStacks"),
            ModType::BASE,
            maxImpaleStacks,
            "Config",
            0,
            0,
            &[tag!({ type = Condition, var = "Combat" })],
        );
    }

    // Foulborn Choir of the Storm, needs to be after main auras (in case purity of lightning/elements auras) but before extra auras (Radiant Faith)
    // L3395
    if modDB.Flag(None, nm!["ManaIncreasedByOvercappedLightningRes"]) {
        // Calculate resistances for ManaIncreasedByOvercappedLightningRes without mutating conversion mods on the player ModDB.
        // L3397-3402
        let tempResistActor = Actor::new();
        tempResistActor
            .modDB
            .set(ModStore::ModDB(Some(modDB.clone())));
        tempResistActor.output.set(output.clone());
        tempResistActor
            .activeSkillList
            .set(env.player.activeSkillList.get());
        tempResistActor.enemy.set(env.player.enemy.get());
        // L3403
        tempResistActor
            .player
            .set(Rc::downgrade(&tempResistActor));
        // L3404
        tempResistActor
            .modDB
            .get()
            .actor
            .set(Rc::downgrade(&tempResistActor));
        // L3405
        resistances(&tempResistActor);
        // Set the life/mana reservations again as we now have increased mana from overcapped lightning resistance
        // L3407
        doActorLifeMana(&env.player);
        // L3408
        doActorLifeManaReservation(&env.player, true);
    }

    // L3411
    if modDB.Flag(
        Some(&*main_skill(&env.player).skillCfg.get()),
        nm!["Condition:CanInflictHallowingFlame"],
    ) {
        // L3412
        let ov = modDB.Override(None, nm!["HallowingFlameMagnitude"]);
        let magnitude = if ov.truthy() {
            Val::from_value(&ov).num()
        } else {
            modDB.Sum(ModType::INC, None, nm!["HallowingFlameMagnitude"])
        };
        // L3413
        let val = m_floor(25.0 * (1.0 + magnitude / 100.0)); // Hallowing flame grants Attack Hits against you gain 25% of Physical Damage as Extra Fire Damage
        // L3414
        let mut payload = ValueTable::new();
        payload.set(
            n!("mod"),
            Value::from(createMod(
                n!("PhysicalDamageGainAsFire"),
                ModType::BASE,
                val,
                Some("Hallowing Flame"),
                0,
                0,
                &[
                    tag!({ type = GlobalEffect, effectType = "Global", unscalable = true }),
                    tag!({ type = ActorCondition, actor = "enemy", var = "HallowingFlame" }),
                    tag!({ type = Multiplier, var = "HallowingFlame", actor = "enemy" }),
                ],
            )),
        );
        // LUA-QUIRK: no argument follows the payload, so createMod gives the mod no source
        // (ModStore.lua:110 → ModTools.lua:38); `NewMod` here always passes one.
        modDB.AddMod(Arc::new(createMod(
            n!("ExtraAura"),
            ModType::LIST,
            Value::from(payload),
            None,
            0,
            0,
            &[],
        )));
    }

    // Check for extra auras
    // L3418
    let buffExportsAura = buffExports
        .table(n!("Aura"))
        .unwrap_or_else(|| lua_error!("attempt to index field 'Aura' (a nil value)"));
    buffExportsAura.set(
        n!("extraAura"),
        tbl! { effectMult = 1.0, modList = store_val(ModStore::ModList(None)) },
    );
    // L3419
    for_ipairs!((_i, value) in modDB.List(None, nm!["ExtraAura"]) => {
        let valueMod: Option<ModRef> = value.get(n!("mod")).as_mod().cloned();
        // L3420
        let modList: Vec<ModRef> = valueMod.iter().cloned().collect();
        // L3421
        if !value.get(n!("onlyAllies")).truthy()
            && !(value.get(n!("fromAllies")).truthy()
                && modDB.Flag(None, nm!["AlliesAurasCannotAffectSelf"]))
        {
            // L3422
            let inc = modDB.Sum(
                ModType::INC,
                None,
                nm!["BuffEffectOnSelf", "AuraEffectOnSelf"],
            );
            // L3423
            let more = modDB.More(None, nm!["BuffEffectOnSelf", "AuraEffectOnSelf"]);
            // L3424
            modDB.ScaleAddList(&modList, (1.0 + inc / 100.0) * more, false);
            // L3425
            if !value.get(n!("notBuff")).truthy() {
                // L3426
                let v = modDB.multipliers.num0(n!("BuffOnSelf")) + 1.0;
                modDB.multipliers.set(n!("BuffOnSelf"), v);
            }
        }
        // L3429
        if value.get(n!("fromAllies")).truthy()
            || !modDB.Flag(None, nm!["SelfAurasCannotAffectAllies"])
        {
            // L3430
            if let Some(minion) = env.minion.get() {
                // L3431
                let inc = minion.modDB.get().Sum(
                    ModType::INC,
                    None,
                    nm!["BuffEffectOnSelf", "AuraEffectOnSelf"],
                );
                // L3432
                let more = minion
                    .modDB
                    .get()
                    .More(None, nm!["BuffEffectOnSelf", "AuraEffectOnSelf"]);
                // L3433
                minion
                    .modDB
                    .get()
                    .ScaleAddList(&modList, (1.0 + inc / 100.0) * more, false);
            }
            // L3435
            let valueMod = valueMod.clone().unwrap_or_else(|| {
                lua_error!("attempt to index local 'mod' (a nil value)")
            });
            store_of(
                &table_of(&buffExportsAura.get(n!("extraAura"))).get(n!("modList")),
            )
            .AddMod(valueMod.clone());
            // L3436 (`value.mod.name` is always set)
            let totemModBlacklist = valueMod.name.as_str() == "Speed"
                || valueMod.name.as_str() == "CritMultiplier"
                || valueMod.name.as_str() == "CritChance";
            // L3437
            if main_skill(&env.player).skillFlags.get().truthy(n!("totem")) && !totemModBlacklist {
                // L3438
                let mut totemMod: Mod = (*valueMod).clone();
                // L3439
                let (mut totemModName, matches) =
                    pob_lua::gsub(totemMod.name.as_str(), "Condition:", "Condition:Totem", None);
                // L3440
                if matches < 1 {
                    // L3441
                    totemModName = format!("Totem{}", totemMod.name.as_str());
                }
                // L3443
                totemMod.name = Name::from(totemModName.as_str());
                // L3444
                modDB.AddMod(Arc::new(totemMod));
            }
        }
    });
    // L3448
    if allyBuffs().truthy(n!("extraAura")) {
        // L3449: LUA-ORDER (string keys)
        for (_k, buff) in table_of(&allyBuffs().get(n!("extraAura"))).pairs() {
            // L3450
            let modList = mods_of(&buff.get(n!("modList")));
            // L3451
            let inc = modDB.Sum(
                ModType::INC,
                None,
                nm!["BuffEffectOnSelf", "AuraEffectOnSelf"],
            );
            // L3452
            let more = modDB.More(None, nm!["BuffEffectOnSelf", "AuraEffectOnSelf"]);
            // L3453
            modDB.ScaleAddList(&modList, (1.0 + inc / 100.0) * more, false);
            // L3454
            if let Some(minion) = env.minion.get() {
                // L3455
                let inc = minion.modDB.get().Sum(
                    ModType::INC,
                    None,
                    nm!["BuffEffectOnSelf", "AuraEffectOnSelf"],
                );
                // L3456
                let more = minion
                    .modDB
                    .get()
                    .More(None, nm!["BuffEffectOnSelf", "AuraEffectOnSelf"]);
                // L3457
                minion
                    .modDB
                    .get()
                    .ScaleAddList(&modList, (1.0 + inc / 100.0) * more, false);
            }
        }
    }

    // Check for modifiers to apply to actors affected by player auras or curses
    // L3463
    for_ipairs!((_i, _value) in modDB.List(None, nm!["AffectedByAuraMod"]) => {
        // L3464: LUA-GLOBAL-NIL: `affectedByAura` is an undeclared global (calc doc §7.3).
        lua_error!("bad argument #1 to 'pairs' (table expected, got nil)");
    });

    // Merge keystones again to catch any that were added by buffs
    // L3470
    mergeKeystones(&env, &env.modDB.get());

    // Special handling for Dancing Dervish
    // L3473
    if modDB.Flag(None, nm!["DisableWeapons"]) {
        // L3474
        let classId = env.classId.get();
        let unarmed = env
            .data
            .unarmedWeaponData
            .get(&(classId as i64))
            .filter(|_| classId.fract() == 0.0)
            .unwrap_or_else(|| lua_error!("bad argument #1 to 'pairs' (table expected, got nil)"));
        let weaponData1 = Table::new();
        weaponData1.set(n!("type"), unarmed.r#type);
        weaponData1.set(n!("AttackRate"), unarmed.AttackRate);
        weaponData1.set(n!("CritChance"), unarmed.CritChance);
        weaponData1.set(n!("PhysicalMin"), unarmed.PhysicalMin);
        weaponData1.set(n!("PhysicalMax"), unarmed.PhysicalMax);
        env.player.weaponData1.set(weaponData1);
        // L3475
        modDB.conditions.set(n!("Unarmed"), true);
        // L3476
        let Gloves = env.player.extra.get(n!("Gloves"));
        if !Gloves.truthy() || Gloves.str().is_some_and(|s| &*s == "None") {
            // L3477
            modDB.conditions.set(n!("Unencumbered"), true);
        }
    } else if let Some(weaponModList1) = env.weaponModList1.get() {
        // L3480
        modDB.AddList(&weaponModList1.ipairs());
    }

    // Process prerequisites for conditionals
    // L3484
    defenceForConditionals(&env, &env.player);
    // L3485
    if let Some(minion) = env.minion.get() {
        // L3486
        defenceForConditionals(&env, &minion);
    }

    // Process misc buffs/modifiers
    // L3490
    doActorCharges(&env, &env.player);
    // L3491
    doActorMisc(&env, &env.player);
    // L3492
    if let Some(minion) = env.minion.get() {
        // L3493
        doActorCharges(&env, &minion);
        // L3494
        doActorMisc(&env, &minion);
    }

    Flow::Normal
}

// ---- Rust-only plumbing -------------------------------------------------------------------------

/// A table value (`for k, v in pairs(x)` / `x[k]` on a table that must exist).
fn table_of(v: &Val) -> Table {
    v.table().unwrap_or_else(|| {
        lua_error!("attempt to index a {} value", v.lua_type())
    })
}

/// A string key of `pairs` (the tables iterated here are string-keyed).
fn key_str(k: Key) -> LStr {
    match k {
        Key::S(s) => LStr::from(s.as_str()),
        Key::I(i) => LStr::from(i.to_string().as_str()),
        Key::B(b) => lua_error!("attempt to concatenate a boolean value ({b})"),
    }
}

/// `s:gsub(" ", "")` (first result).
fn gsub_space(s: &str) -> Name {
    Name::from(pob_lua::gsub(s, " ", "", None).0.as_str())
}

/// The list `ipairs` sees of a Lua ModList or a plain list of mods.
fn mods_of(v: &Val) -> Vec<ModRef> {
    if let Some(store) = v.obj::<ModStore>() {
        return store.ipairs();
    }
    match v {
        Val::Table(t) => t
            .ipairs()
            .into_iter()
            .map(|(_, m)| match m {
                Val::Mod(m) => m,
                other => lua_error!("a mod list entry is a {} value", other.lua_type()),
            })
            .collect(),
        Val::Data(d) => d
            .as_table()
            .map(|t| {
                t.array
                    .iter()
                    .map(|m| {
                        m.as_mod()
                            .cloned()
                            .unwrap_or_else(|| lua_error!("a mod list entry is not a mod"))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        other => lua_error!(
            "bad argument #1 to 'ipairs' (table expected, got {})",
            other.lua_type()
        ),
    }
}

/// A ModList object as a dynamic-table value.
fn store_val(l: ModList) -> Val {
    Val::Obj(l as Rc<dyn std::any::Any>)
}

/// The ModList object a field holds.
fn store_of(v: &Val) -> ModList {
    v.obj::<ModStore>()
        .unwrap_or_else(|| lua_error!("attempt to call method on a {} value", v.lua_type()))
}

/// A list of mods as the table `mergeBuff` iterates (frozen `src: &Table`).
fn mods_table(mods: &[ModRef]) -> Table {
    let t = Table::new();
    for m in mods {
        t.push(Val::Mod(m.clone()));
    }
    t
}

/// A ModList handed to `mergeBuff` as its frozen `src: &Table`.
fn mod_table(l: &ModList) -> Table {
    mods_table(&l.ipairs())
}

/// A buff value as a ModList for `addMinionModifiers` (a table-shaped buff is copied into a
/// fresh ModList with the same mods; `List` over it is the same).
fn as_mod_list(v: &Val) -> ModList {
    if let Some(store) = v.obj::<ModStore>() {
        return store;
    }
    let l = ModStore::ModList(None);
    l.AddList(&mods_of(v));
    l
}

/// `modList.notBuff` (CalcPerform.lua:3346). CCR-0030 row 7 / CCR-0076: a ModList object cannot carry
/// the key in the frozen types, so a store reads as nil.
fn notBuff(v: &Val) -> bool {
    match v {
        Val::Table(t) => t.truthy(n!("notBuff")),
        _ => false, // CCR-0030, CCR-0076
    }
}

/// `env.partyMembers[key]` (`partyTab.actor.Aura/Curse/Warcry/Link`, kept in `actor.extra`).
fn party_list(env: &EnvRef, key: Name) -> Val {
    let pm = env
        .partyMembers
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'partyMembers' (a nil value)"));
    pm.extra.get(key)
}

/// `env.player.mainSkill` (indexing nil raises).
fn main_skill(player: &ActorRef) -> ActiveSkillRef {
    player
        .mainSkill
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"))
}

/// `warcryBuff[1].warcryPowerBonus` (the first tag of the mod; a missing tag raises).
fn warcryPowerBonus(warcryBuff: &ModRef) -> Val {
    let tag = warcryBuff
        .tags
        .first()
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
    tag.extra_get(n!("warcryPowerBonus"))
        .map_or(Val::Nil, Val::from_value)
}

/// A string-valued `Value` as a name (`value.skillId`).
fn value_name(v: &Value) -> Option<Name> {
    v.as_str().map(Name::from)
}
