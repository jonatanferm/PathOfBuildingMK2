//! @pob-file Modules/CalcDefence.lua @16de4b82d
//! defence segment DF-B (CalcDefence.lua 659-1210): armour/evasion/ES/ward, block, suppression.
//!
//! Owner: DF-B. The segment's callees that other units port (`calcs.actionSpeedMod`: PF,
//! `calcs.resistances` / `calcs.hitChance`: DF-A, `doActorLifeMana` /
//! `doActorLifeManaReservation`: PF) are called through thin wrappers that consult the
//! oracle-test seam [`replay`] (feature `probe`), the same pattern as `item_parse::replay`
//! (CCR-0025 §3): `crates/pob-oracle/tests/DF-B.rs` installs hooks that replay PoB's recorded
//! effects while those callees are unported. Without hooks (and in every non-probe build) the
//! wrappers call the real functions.
#![allow(non_snake_case)]
// literal Lua shapes (`local total` then assignment in both branches, `a and b or c` chains)
#![allow(
    clippy::needless_late_init,
    clippy::nonminimal_bool,
    clippy::if_same_then_else,
    clippy::too_many_lines,
    clippy::float_cmp,
    clippy::never_loop,
    clippy::too_many_arguments
)]

use super::DefenceCtx;
use crate::modules::calc_perform::{actor_a, actor_b};
use crate::modules::calc_tools as calcLib;
use crate::prelude::*;
use pob_gamedata::mod_tools;

/// `resistTypeList` (CalcDefence.lua:25, a module-level local of DF-A's header; a private copy
/// here so this segment does not depend on how DF-A exposes it).
const resistTypeList: [&str; 4] = ["Fire", "Cold", "Lightning", "Chaos"];

// ---- replay seam (feature `probe`) ----------------------------------------------------------

/// Oracle-test seam (feature `probe`): while the callees named in the module doc are unported,
/// `crates/pob-oracle/tests/DF-B.rs` installs hooks that replay PoB's recorded results. Unset
/// hooks call the real function.
#[cfg(feature = "probe")]
pub mod replay {
    use super::*;
    use std::cell::RefCell;

    pub type ActorFn = Rc<dyn Fn(&ActorRef)>;
    pub type ActionSpeedModFn = Rc<dyn Fn(&ActorRef) -> f64>;
    pub type ReservationFn = Rc<dyn Fn(&ActorRef, bool)>;
    pub type HitChanceFn = Rc<dyn Fn(f64, f64) -> f64>;

    #[derive(Default, Clone)]
    pub struct Hooks {
        pub actionSpeedMod: Option<ActionSpeedModFn>,
        pub resistances: Option<ActorFn>,
        pub doActorLifeMana: Option<ActorFn>,
        pub doActorLifeManaReservation: Option<ReservationFn>,
        pub hitChance: Option<HitChanceFn>,
    }

    thread_local! {
        static HOOKS: RefCell<Hooks> = RefCell::new(Hooks::default());
    }

    /// Install hooks for this thread.
    pub fn set(h: Hooks) {
        HOOKS.with(|x| *x.borrow_mut() = h);
    }

    /// Remove every hook.
    pub fn clear() {
        set(Hooks::default());
    }

    pub(super) fn get() -> Hooks {
        HOOKS.with(|x| x.borrow().clone())
    }
}

/// `calcs.actionSpeedMod(actor)` (CalcPerform.lua:1141).
fn calcs_actionSpeedMod(actor: &ActorRef) -> f64 {
    #[cfg(feature = "probe")]
    if let Some(f) = replay::get().actionSpeedMod {
        return f(actor);
    }
    actor_b::actionSpeedMod(actor)
}

/// `calcs.resistances(actor)` (CalcDefence.lua:512, DF-A).
fn calcs_resistances(actor: &ActorRef) {
    #[cfg(feature = "probe")]
    if let Some(f) = replay::get().resistances {
        return f(actor);
    }
    super::resistances(actor);
}

/// Global `doActorLifeMana(actor)` (CalcPerform.lua:69).
fn call_doActorLifeMana(actor: &ActorRef) {
    #[cfg(feature = "probe")]
    if let Some(f) = replay::get().doActorLifeMana {
        return f(actor);
    }
    actor_a::doActorLifeMana(actor);
}

/// Global `doActorLifeManaReservation(actor, addAura)` (CalcPerform.lua:551).
fn call_doActorLifeManaReservation(actor: &ActorRef, addAura: bool) {
    #[cfg(feature = "probe")]
    if let Some(f) = replay::get().doActorLifeManaReservation {
        return f(actor, addAura);
    }
    actor_a::doActorLifeManaReservation(actor, addAura);
}

/// `calcs.hitChance(evasion, accuracy)` (CalcDefence.lua:58, DF-A).
fn calcs_hitChance(evasion: f64, accuracy: f64) -> f64 {
    #[cfg(feature = "probe")]
    if let Some(f) = replay::get().hitChance {
        return f(evasion, accuracy);
    }
    super::hitChance(evasion, accuracy)
}

// ---- plumbing (Rust-only helpers; snake_case) ------------------------------------------------

/// `<x> or <d()>` on a `ModStore:Override` result used as a number.
fn override_or(v: Value, d: impl FnOnce() -> f64) -> f64 {
    if v.truthy() {
        v.num().unwrap_or_else(|| {
            lua_error!("attempt to perform arithmetic on a {} value", value_type(&v))
        })
    } else {
        d()
    }
}

/// Lua `type(v)` of a mod value.
fn value_type(v: &Value) -> &'static str {
    match v {
        Value::Nil => "nil",
        Value::Bool(_) => "boolean",
        Value::Num(_) => "number",
        Value::Str(_) => "string",
        _ => "table",
    }
}

/// A Lua value read from a calc table, passed on as a mod value (`NewMod(…, output.X, …)`).
fn mod_value(v: &Val) -> Value {
    match v {
        Val::Nil => Value::Nil,
        Val::Bool(b) => Value::Bool(*b),
        Val::Num(n) => Value::Num(*n),
        Val::Str(s) => Value::from(&**s),
        Val::Data(d) => d.clone(),
        Val::Mod(m) => Value::Mod(m.clone()),
        Val::Table(_) | Val::Obj(_) => {
            lua_error!("DF-B: a calc table passed as a mod value (not reachable in PoB)")
        }
    }
}

/// `modDB:NewMod(name, type, value, mod.source, flags, keywordFlags, ...)` keeping a nil
/// `mod.source` nil (`ModStore:NewMod` → `modLib.createMod`).
fn new_mod_from(
    modDB: &ModStore,
    name: Name,
    ty: ModType,
    value: Value,
    source: Option<&str>,
    flags: u32,
    keywordFlags: u32,
    tags: &[Tag],
) {
    modDB.AddMod(Arc::new(mod_tools::createMod(
        name,
        ty,
        value,
        source,
        flags,
        keywordFlags,
        tags,
    )));
}

/// `actor.itemList[slot]`.
fn item_in(actor: &ActorRef, slot: Name) -> Option<ItemRef> {
    actor.itemList.with(|l| l.get(&slot).cloned())
}

/// `item` field of a breakdown slot row (`actor.itemList[slot]`, nil when absent).
fn item_val(item: Option<ItemRef>) -> Val {
    item.map_or(Val::Nil, |i| Val::Obj(i as Rc<dyn std::any::Any>))
}

/// `(cond) and s_format(...)`: the string, or `false`.
fn and_str(cond: bool, s: impl FnOnce() -> String) -> Val {
    if cond {
        Val::from(s())
    } else {
        Val::Bool(false)
    }
}

/// `breakdown[name].slots` for `t_insert`.
fn slots_of(breakdown: &Breakdown, name: Name) -> Table {
    breakdown
        .get(name)
        .get(n!("slots"))
        .table()
        .unwrap_or_else(|| lua_error!("bad argument #1 to 'insert' (table expected, got nil)"))
}

/// `actor.parent.output` / `actor.partyMembers.output` (indexing nil raises).
fn other_output(a: Option<ActorRef>, field: &str) -> Output {
    match a {
        Some(a) => a.output.get(),
        None => lua_error!("attempt to index field '{}' (a nil value)", field),
    }
}

/// `env.minion` is `actor`.
fn is_env_minion(env: &EnvRef, actor: &ActorRef) -> bool {
    env.minion.get().is_some_and(|m| Rc::ptr_eq(&m, actor))
}

/// `env.minion` (indexing nil raises).
fn env_minion(env: &EnvRef) -> ActorRef {
    env.minion
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'minion' (a nil value)"))
}

/// `env.configInput.EHPUnluckyWorstOf and env.configInput.EHPUnluckyWorstOf ~= 1`
/// → `Some(-EHPUnluckyWorstOf / 2)`.
fn unlucky_worst_of(env: &EnvRef) -> Option<f64> {
    let v = env.configInput.get(n!("EHPUnluckyWorstOf"));
    if v.truthy() && !matches!(v, Val::Num(x) if x == 1.0) {
        Some(-v.num() / 2.0)
    } else {
        None
    }
}

/// @pob Modules/CalcDefence.lua:659-1210 h=c1300dd8
/// DF-B: CalcDefence.lua 659-1210.
pub(crate) fn seg(ctx: &mut DefenceCtx) -> Flow {
    let env = ctx.env.clone();
    let actor = ctx.actor.clone();
    let modDB = ctx.modDB.clone();
    let enemyDB = ctx.enemyDB.clone();
    let output = ctx.output.clone();
    let breakdown = ctx.breakdown.clone();
    let misc = &data().misc;

    // L660
    let condList = modDB.conditions.clone();
    let _ = &condList;

    // -- Action Speed
    // L663
    let v = calcs_actionSpeedMod(&actor);
    output.set(n!("ActionSpeedMod"), v);

    // L665
    calcs_resistances(&actor);
    // L666
    if env.minion.get().is_some()
        && modDB.Sum(ModType::BASE, None, nm!["ResistanceAddedToMinions"]) > 0.0
    {
        // L667
        for elem in resistTypeList {
            // L668
            let r#final = output.num(cat!(elem, "Resist"));
            // L669
            let v = m_floor(
                r#final * modDB.Sum(ModType::BASE, None, nm!["ResistanceAddedToMinions"]) / 100.0,
            );
            env_minion(&env).modDB.get().NewMod(
                cat!(elem, "Resist"),
                ModType::BASE,
                v,
                "Player",
                0,
                0,
                &[],
            );
        }
    }
    // -- Formless Inferno
    // L673
    if is_env_minion(&env, &actor) {
        // L674
        call_doActorLifeMana(&actor);
        // L675
        call_doActorLifeManaReservation(&actor, false);
    }

    // -- Block
    // L679
    let v = m_min(
        modDB.Sum(ModType::BASE, None, nm!["BlockChanceMax"]),
        misc.BlockChanceCap,
    );
    output.set(n!("BlockChanceMax"), v);
    // L680
    if modDB.Flag(None, nm!["MaximumBlockAttackChanceIsEqualToParent"]) {
        // L681
        let v = other_output(actor.parent(), "parent").get(n!("BlockChanceMax"));
        output.set(n!("BlockChanceMax"), v);
    } else if modDB.Flag(None, nm!["MaximumBlockAttackChanceIsEqualToPartyMember"]) {
        // L683
        let v = other_output(actor.partyMembers.get().upgrade(), "partyMembers")
            .get(n!("BlockChanceMax"));
        output.set(n!("BlockChanceMax"), v);
    }
    // L685
    output.set(n!("BlockChanceOverCap"), 0.0);
    // L686
    output.set(n!("SpellBlockChanceOverCap"), 0.0);
    // L687
    let mut baseBlockChance = 0.0;
    // L688
    if let Some(armourData) = item_in(&actor, n!("Weapon 2")).and_then(|i| i.armourData.get()) {
        // L689
        baseBlockChance += armourData.num(n!("BlockChance"));
    }
    // L692
    if let Some(armourData) = item_in(&actor, n!("Weapon 3")).and_then(|i| i.armourData.get()) {
        // L693
        baseBlockChance += armourData.num(n!("BlockChance"));
    }
    // L695
    output.set(n!("ShieldBlockChance"), baseBlockChance);
    // L696
    baseBlockChance = if !env.keystonesAdded.get().truthy(n!("Necromantic Aegis")) {
        override_or(modDB.Override(None, nm!["ReplaceShieldBlock"]), || baseBlockChance)
    } else {
        baseBlockChance
    };

    // -- Apply player block overrides if Necromantic Aegis allocated
    // L699
    baseBlockChance = if is_env_minion(&env, &actor)
        && env.keystonesAdded.get().truthy(n!("Necromantic Aegis"))
    {
        override_or(
            env.player.modDB.get().Override(None, nm!["ReplaceShieldBlock"]),
            || baseBlockChance,
        )
    } else {
        baseBlockChance
    };

    // L701
    if modDB.Flag(None, nm!["BlockAttackChanceIsEqualToParent"]) {
        // L702
        let v = m_min(
            other_output(actor.parent(), "parent").num(n!("BlockChance")),
            output.num(n!("BlockChanceMax")),
        );
        output.set(n!("BlockChance"), v);
    } else if modDB.Flag(None, nm!["BlockAttackChanceIsEqualToPartyMember"]) {
        // L704
        let v = m_min(
            other_output(actor.partyMembers.get().upgrade(), "partyMembers")
                .num(n!("BlockChance")),
            output.num(n!("BlockChanceMax")),
        );
        output.set(n!("BlockChance"), v);
    } else if modDB.Flag(None, nm!["MaxBlockIfNotBlockedRecently"]) {
        // L706
        let v = output.get(n!("BlockChanceMax"));
        output.set(n!("BlockChance"), v);
    } else {
        // L708
        let inc = modDB.Sum(ModType::INC, None, nm!["BlockChance"]);
        // L709
        let more = modDB.More(None, nm!["BlockChance"]);
        // L710
        let totalBlockChance = round(
            (baseBlockChance + modDB.Sum(ModType::BASE, None, nm!["BlockChance"]))
                * (1.0 + inc / 100.0)
                * more,
            None,
        );
        // L711
        let v = m_min(totalBlockChance, output.num(n!("BlockChanceMax")));
        output.set(n!("BlockChance"), v);
        // L712
        let v = m_max(0.0, totalBlockChance - output.num(n!("BlockChanceMax")));
        output.set(n!("BlockChanceOverCap"), v);
    }

    // L715
    let v = m_min(
        output.num(n!("BlockChance"))
            + modDB.Sum(ModType::BASE, None, nm!["ProjectileBlockChance"])
                * calcLib::r#mod(&modDB, None, nm!["BlockChance"]),
        output.num(n!("BlockChanceMax")),
    );
    output.set(n!("ProjectileBlockChance"), v);
    // L716
    if modDB.Flag(None, nm!["SpellBlockChanceMaxIsBlockChanceMax"]) {
        // L717
        let v = output.get(n!("BlockChanceMax"));
        output.set(n!("SpellBlockChanceMax"), v);
    } else {
        // L719
        let v = m_min(
            modDB.Sum(ModType::BASE, None, nm!["SpellBlockChanceMax"]),
            misc.BlockChanceCap,
        );
        output.set(n!("SpellBlockChanceMax"), v);
    }
    // L721
    if modDB.Flag(None, nm!["MaxSpellBlockIfNotBlockedRecently"]) {
        // L722
        let v = output.get(n!("SpellBlockChanceMax"));
        output.set(n!("SpellBlockChance"), v);
        // L723
        let v = output.get(n!("SpellBlockChanceMax"));
        output.set(n!("SpellProjectileBlockChance"), v);
    } else if modDB.Flag(None, nm!["SpellBlockChanceIsBlockChance"]) {
        // L725
        let v = output.get(n!("BlockChance"));
        output.set(n!("SpellBlockChance"), v);
        // L726
        let v = output.get(n!("ProjectileBlockChance"));
        output.set(n!("SpellProjectileBlockChance"), v);
        // L727
        let v = output.get(n!("BlockChanceOverCap"));
        output.set(n!("SpellBlockChanceOverCap"), v);
    } else {
        // L729
        let inc = modDB.Sum(ModType::INC, None, nm!["BlockChance"]);
        // L730
        let more = modDB.More(None, nm!["BlockChance"]);
        // L731
        let totalSpellBlockChance = round(
            modDB.Sum(ModType::BASE, None, nm!["SpellBlockChance"]) * (1.0 + inc / 100.0) * more,
            None,
        );
        // L732
        let v = m_min(totalSpellBlockChance, output.num(n!("SpellBlockChanceMax")));
        output.set(n!("SpellBlockChance"), v);
        // L733
        let v = m_max(
            0.0,
            totalSpellBlockChance - output.num(n!("SpellBlockChanceMax")),
        );
        output.set(n!("SpellBlockChanceOverCap"), v);
        // L734
        let v = m_max(
            m_min(
                output.num(n!("SpellBlockChance"))
                    + modDB.Sum(ModType::BASE, None, nm!["ProjectileSpellBlockChance"])
                        * calcLib::r#mod(&modDB, None, nm!["SpellBlockChance"]),
                output.num(n!("SpellBlockChanceMax")),
            ),
            0.0,
        );
        output.set(n!("SpellProjectileBlockChance"), v);
    }
    // L736
    if let Some(breakdown) = &breakdown {
        // L737
        let t = tbl![
            format!("Base: {}%", tostring_num(baseBlockChance)),
            format!("Max: {}%", tostring_num(output.num(n!("BlockChanceMax")))),
            format!(
                "Total: {}%",
                tostring_num(output.num(n!("BlockChance")) + output.num(n!("BlockChanceOverCap")))
            ),
        ];
        breakdown.set(n!("BlockChance"), t);
        // L742
        let t = tbl![
            format!("Max: {}%", tostring_num(output.num(n!("SpellBlockChanceMax")))),
            format!(
                "Total: {}%",
                tostring_num(
                    output.num(n!("SpellBlockChance")) + output.num(n!("SpellBlockChanceOverCap"))
                )
            ),
        ];
        breakdown.set(n!("SpellBlockChance"), t);
    }
    // L747
    if modDB.Flag(None, nm!["CannotBlockAttacks"]) {
        // L748
        output.set(n!("BlockChance"), 0.0);
        // L749
        output.set(n!("ProjectileBlockChance"), 0.0);
    }
    // L751
    if modDB.Flag(None, nm!["CannotBlockSpells"]) {
        // L752
        output.set(n!("SpellBlockChance"), 0.0);
        // L753
        output.set(n!("SpellProjectileBlockChance"), 0.0);
    }
    // L755
    for blockType in [
        n!("BlockChance"),
        n!("ProjectileBlockChance"),
        n!("SpellBlockChance"),
        n!("SpellProjectileBlockChance"),
    ] {
        // L756
        let v: Val = if env.mode_effective.get() {
            Val::from(m_max(
                output.num(blockType) - enemyDB.Sum(ModType::BASE, None, nm!["reduceEnemyBlock"]),
                0.0,
            ))
        } else {
            output.get(blockType)
        };
        output.set(cat!("Effective", blockType), v);
        // L757
        let mut blockRolls = 0.0;
        // L758
        if env.mode_effective.get() {
            // L759
            if modDB.Flag(None, &[cat!(blockType, "IsLucky")]) {
                // L760
                blockRolls += 1.0;
            }
            // L762
            if modDB.Flag(None, &[cat!(blockType, "IsUnlucky")]) {
                // L763
                blockRolls -= 1.0;
            }
            // L765
            if modDB.Flag(None, nm!["ExtremeLuck"]) {
                // L766
                blockRolls *= 2.0;
            }
        }
        // -- unlucky config to lower the value of block, dodge, evade etc for ehp
        // L770
        if let Some(r) = unlucky_worst_of(&env) {
            // L771
            blockRolls = r;
        }
        // L773
        if blockRolls != 0.0 {
            // L774
            let blockChance = output.num(cat!("Effective", blockType)) / 100.0;
            // L775
            if modDB.Flag(None, nm!["Unexciting"]) {
                // -- Unexciting rolls three times and keeps the median result -> 3p^2 - 2p^3
                // L777
                let v = (3.0 * lpow(blockChance, 2.0) - 2.0 * lpow(blockChance, 3.0)) * 100.0;
                output.set(cat!("Effective", blockType), v);
            } else if blockRolls > 0.0 {
                // L779
                let v = (1.0 - lpow(1.0 - blockChance, blockRolls + 1.0)) * 100.0;
                output.set(cat!("Effective", blockType), v);
            } else {
                // L781
                let v = lpow(blockChance, m_abs(blockRolls))
                    * output.num(cat!("Effective", blockType));
                output.set(cat!("Effective", blockType), v);
            }
        }
    }
    // L785
    let v = (output.num(n!("EffectiveBlockChance"))
        + output.num(n!("EffectiveProjectileBlockChance"))
        + output.num(n!("EffectiveSpellBlockChance"))
        + output.num(n!("EffectiveSpellProjectileBlockChance")))
        / 4.0;
    output.set(n!("EffectiveAverageBlockChance"), v);
    // L786
    let v = 100.0 - modDB.Sum(ModType::BASE, None, nm!["BlockEffect"]);
    output.set(n!("BlockEffect"), v);
    // L787
    if output.num(n!("BlockEffect")) != 0.0 {
        // L788
        output.set(n!("ShowBlockEffect"), true);
        // L789
        let v = 100.0 - output.num(n!("BlockEffect"));
        output.set(n!("DamageTakenOnBlock"), v);
    }

    // L792
    if modDB.Flag(None, nm!["ArmourAppliesToEnergyShieldRecharge"]) {
        // -- Armour to ES Recharge conversion from Armour and Energy Shield Mastery
        // L794
        let multiplier = modDB
            .Max(None, nm!["ImprovedArmourAppliesToEnergyShieldRecharge"])
            .unwrap_or(100.0)
            / 100.0;
        // L795
        for value in modDB.Tabulate(
            Some(ModType::INC),
            None,
            nm!["Armour", "ArmourAndEvasion", "Defences"],
        ) {
            // L796
            let r#mod = value.r#mod.clone();
            // L797
            let modifiers = calcLib::getConvertedModTags(&r#mod, multiplier, false);
            // L798
            let mv = r#mod.value.num().unwrap_or_else(|| {
                lua_error!(
                    "attempt to perform arithmetic on field 'value' (a {} value)",
                    value_type(&r#mod.value)
                )
            });
            new_mod_from(
                &modDB,
                n!("EnergyShieldRecharge"),
                ModType::INC,
                Value::Num(m_floor(mv * multiplier)),
                r#mod.source.as_deref(),
                r#mod.flags,
                r#mod.keywordFlags,
                &modifiers,
            );
        }
    }

    // L802
    if modDB.Flag(None, nm!["ArmourIncreasedByUncappedFireRes"]) {
        // L803
        for value in modDB.Tabulate(Some(ModType::FLAG), None, nm!["ArmourIncreasedByUncappedFireRes"]) {
            // L804
            let r#mod = value.r#mod.clone();
            // L805
            new_mod_from(
                &modDB,
                n!("Armour"),
                ModType::INC,
                mod_value(&output.get(n!("FireResistTotal"))),
                r#mod.source.as_deref(),
                0,
                0,
                &[],
            );
            // L806
            break;
        }
    }
    // L809
    if modDB.Flag(None, nm!["ArmourIncreasedByOvercappedFireRes"]) {
        // L810
        for value in modDB.Tabulate(Some(ModType::FLAG), None, nm!["ArmourIncreasedByOvercappedFireRes"]) {
            // L811
            let r#mod = value.r#mod.clone();
            // L812
            new_mod_from(
                &modDB,
                n!("Armour"),
                ModType::INC,
                mod_value(&output.get(n!("FireResistOverCap"))),
                r#mod.source.as_deref(),
                0,
                0,
                &[],
            );
            // L813
            break;
        }
    }
    // L816
    if modDB.Flag(None, nm!["EvasionRatingIncreasedByUncappedColdRes"]) {
        // L817
        for value in modDB.Tabulate(Some(ModType::FLAG), None, nm!["EvasionRatingIncreasedByUncappedColdRes"]) {
            // L818
            let r#mod = value.r#mod.clone();
            // L819
            new_mod_from(
                &modDB,
                n!("Evasion"),
                ModType::INC,
                mod_value(&output.get(n!("ColdResistTotal"))),
                r#mod.source.as_deref(),
                0,
                0,
                &[],
            );
            // L820
            break;
        }
    }
    // L823
    if modDB.Flag(None, nm!["EvasionRatingIncreasedByOvercappedColdRes"]) {
        // L824
        for value in modDB.Tabulate(Some(ModType::FLAG), None, nm!["EvasionRatingIncreasedByOvercappedColdRes"]) {
            // L825
            let r#mod = value.r#mod.clone();
            // L826
            new_mod_from(
                &modDB,
                n!("Evasion"),
                ModType::INC,
                mod_value(&output.get(n!("ColdResistOverCap"))),
                r#mod.source.as_deref(),
                0,
                0,
                &[],
            );
            // L827
            break;
        }
    }
    // L830
    if modDB.Flag(None, nm!["EnergyShieldIncreasedByChanceToBlockSpellDamage"]) {
        // L831
        for value in modDB.Tabulate(Some(ModType::FLAG), None, nm!["EnergyShieldIncreasedByChanceToBlockSpellDamage"]) {
            // L832
            let r#mod = value.r#mod.clone();
            // L833
            new_mod_from(
                &modDB,
                n!("EnergyShield"),
                ModType::INC,
                mod_value(&output.get(n!("SpellBlockChance"))),
                r#mod.source.as_deref(),
                0,
                0,
                &[],
            );
            // L834
            break;
        }
    }
    // L837
    if modDB.Flag(None, nm!["EnergyShieldIncreasedByChaosResistance"]) {
        // L838
        for value in modDB.Tabulate(Some(ModType::FLAG), None, nm!["EnergyShieldIncreasedByChaosResistance"]) {
            // L839
            let r#mod = value.r#mod.clone();
            // L840
            new_mod_from(
                &modDB,
                n!("EnergyShield"),
                ModType::INC,
                mod_value(&output.get(n!("ChaosResist"))),
                r#mod.source.as_deref(),
                0,
                0,
                &[],
            );
            // L841
            break;
        }
    }
    // -- Primary defences: Energy shield, evasion and armour
    // L845
    {
        // L846
        let ironReflexes = modDB.Flag(None, nm!["IronReflexes"]);
        // L847
        let mut ward = 0.0;
        // L848
        let mut energyShield = 0.0;
        // L849
        let mut armour = 0.0;
        // L850
        let mut evasion = 0.0;
        // L851
        if let Some(breakdown) = &breakdown {
            // L852
            breakdown.set(n!("Ward"), tbl! { slots = Table::new() });
            // L853
            breakdown.set(n!("EnergyShield"), tbl! { slots = Table::new() });
            // L854
            breakdown.set(n!("Armour"), tbl! { slots = Table::new() });
            // L855
            breakdown.set(n!("Evasion"), tbl! { slots = Table::new() });
        }
        // L857
        let mut energyShieldBase: f64;
        let mut armourBase: f64;
        let mut evasionBase: f64;
        let mut wardBase: f64;
        // L858
        let mut gearWard = 0.0;
        // L859
        let mut gearEnergyShield = 0.0;
        // L860
        let mut gearArmour = 0.0;
        // L861
        let mut gearEvasion = 0.0;
        // L862 `wipeTable(tempTable1)`: the module-level scratch table, wiped; only
        // `slotName` is ever set on it.
        let slotCfg = ModCfg::default();
        // L863 `pairs` over an array literal: positional order.
        for slot in [
            n!("Helmet"),
            n!("Gloves"),
            n!("Boots"),
            n!("Body Armour"),
            n!("Weapon 2"),
            n!("Weapon 3"),
        ] {
            // L864
            let armourData = item_in(&actor, slot).and_then(|i| i.armourData.get());
            // L865
            if let Some(armourData) = armourData {
                // L866
                slotCfg.slotName.set(Some(slot));
                // L867
                energyShieldBase = if !modDB.Flag(None, &[cat!("GainNoEnergyShieldFrom", slot)]) {
                    armourData.num_or(n!("EnergyShield"), 0.0)
                } else {
                    0.0
                };
                // L868
                armourBase = if !modDB.Flag(None, &[cat!("GainNoArmourFrom", slot)]) {
                    armourData.num_or(n!("Armour"), 0.0)
                } else {
                    0.0
                };
                // L869
                evasionBase = if !(modDB.Flag(None, &[cat!("GainNoEvasionFrom", slot)])
                    || (modDB.Flag(None, &[cat!("GainNoArmourFrom", slot)]) && ironReflexes))
                {
                    armourData.num_or(n!("Evasion"), 0.0)
                } else {
                    0.0
                };
                // L870
                wardBase = if !modDB.Flag(None, &[cat!("GainNoWardFrom", slot)]) {
                    armourData.num_or(n!("Ward"), 0.0)
                } else {
                    0.0
                };
                // L871
                if slot == n!("Body Armour")
                    && modDB.Flag(None, nm!["ConvertBodyArmourArmourEvasionToWard"])
                {
                    // L872
                    let conversion = m_min(
                        modDB.Sum(ModType::BASE, None, nm!["BodyArmourArmourEvasionToWardPercent"])
                            / 100.0,
                        1.0,
                    );
                    // L873
                    let convertedArmour = armourBase * conversion;
                    // L874
                    let convertedEvasion = evasionBase * conversion;
                    // L875
                    armourBase -= convertedArmour;
                    // L876
                    evasionBase -= convertedEvasion;
                    // L877
                    wardBase += convertedEvasion + convertedArmour;
                }
                // L879
                if wardBase > 0.0 {
                    // L880
                    if modDB.Flag(None, nm!["EnergyShieldToWard"]) {
                        // L881
                        let inc = modDB.Sum(
                            ModType::INC,
                            Some(&slotCfg),
                            nm!["Ward", "Defences", "EnergyShield"],
                        );
                        // L882
                        let more = modDB.More(Some(&slotCfg), nm!["Ward", "Defences"]);
                        // L883
                        ward += wardBase * (1.0 + inc / 100.0) * more;
                        // L884
                        gearWard += wardBase;
                        // L885
                        if let Some(breakdown) = &breakdown {
                            // L886
                            let row = tbl! {
                                base = wardBase,
                                inc = and_str(inc != 0.0, || lfmt!(" x %.2f", 1.0 + inc / 100.0)),
                                more = and_str(more != 1.0, || lfmt!(" x %.2f", more)),
                                total = lfmt!("%.2f", wardBase * (1.0 + inc / 100.0) * more),
                                source = slot,
                                item = item_val(item_in(&actor, slot)),
                            };
                            slots_of(breakdown, n!("Ward")).push(row);
                        }
                    } else {
                        // L896
                        ward += wardBase * calcLib::r#mod(&modDB, Some(&slotCfg), nm!["Ward", "Defences"]);
                        // L897
                        gearWard += wardBase;
                        // L898
                        if let Some(breakdown) = &breakdown {
                            // L899
                            breakdown.slot(
                                slot.as_str(),
                                None,
                                Some(&slotCfg),
                                wardBase,
                                None,
                                nm!["Ward", "Defences"],
                            );
                        }
                    }
                }
                // L903
                if energyShieldBase > 0.0 {
                    // L904
                    if modDB.Flag(None, nm!["EnergyShieldToWard"]) {
                        // L905
                        let more = modDB.More(Some(&slotCfg), nm!["EnergyShield", "Defences"]);
                        // L906
                        energyShield += energyShieldBase * more;
                        // L907
                        gearEnergyShield += energyShieldBase;
                        // L908
                        if let Some(breakdown) = &breakdown {
                            // L909
                            let row = tbl! {
                                base = energyShieldBase,
                                more = and_str(more != 1.0, || lfmt!(" x %.2f", more)),
                                total = lfmt!("%.2f", energyShieldBase * more),
                                source = slot,
                                item = item_val(item_in(&actor, slot)),
                            };
                            slots_of(breakdown, n!("EnergyShield")).push(row);
                        }
                    } else if !modDB.Flag(None, nm!["ConvertArmourESToLife"]) {
                        // L918
                        energyShield += energyShieldBase
                            * calcLib::r#mod(
                                &modDB,
                                Some(&slotCfg),
                                &[n!("EnergyShield"), n!("Defences"), cat!(slot, "ESAndArmour")],
                            );
                        // L919
                        gearEnergyShield += energyShieldBase;
                        // L920
                        if let Some(breakdown) = &breakdown {
                            // L921
                            breakdown.slot(
                                slot.as_str(),
                                None,
                                Some(&slotCfg),
                                energyShieldBase,
                                None,
                                &[n!("EnergyShield"), n!("Defences"), cat!(slot, "ESAndArmour")],
                            );
                        }
                    }
                }
                // L925
                if armourBase > 0.0 {
                    // L926
                    armour += armourBase
                        * calcLib::r#mod(
                            &modDB,
                            Some(&slotCfg),
                            &[
                                n!("Armour"),
                                n!("ArmourAndEvasion"),
                                n!("Defences"),
                                cat!(slot, "ESAndArmour"),
                            ],
                        );
                    // L927
                    gearArmour += armourBase;
                    // L928
                    if let Some(breakdown) = &breakdown {
                        // L929
                        breakdown.slot(
                            slot.as_str(),
                            None,
                            Some(&slotCfg),
                            armourBase,
                            None,
                            &[
                                n!("Armour"),
                                n!("ArmourAndEvasion"),
                                n!("Defences"),
                                cat!(slot, "ESAndArmour"),
                            ],
                        );
                    }
                }
                // L932
                if evasionBase > 0.0 {
                    // L933
                    gearEvasion += evasionBase;
                    // L934
                    if let Some(breakdown) = &breakdown {
                        // L935
                        breakdown.slot(
                            slot.as_str(),
                            None,
                            Some(&slotCfg),
                            evasionBase,
                            None,
                            nm!["Evasion", "ArmourAndEvasion", "Defences"],
                        );
                    }
                    // L937
                    if ironReflexes {
                        // L938
                        armour += evasionBase
                            * calcLib::r#mod(
                                &modDB,
                                Some(&slotCfg),
                                nm!["Armour", "Evasion", "ArmourAndEvasion", "Defences"],
                            );
                    } else {
                        // L940
                        evasion += evasionBase
                            * calcLib::r#mod(
                                &modDB,
                                Some(&slotCfg),
                                nm!["Evasion", "ArmourAndEvasion", "Defences"],
                            );
                    }
                }
            }
        }
        // L945
        wardBase = modDB.Sum(ModType::BASE, None, nm!["Ward"]);

        // L947
        if wardBase > 0.0 {
            // L948
            if modDB.Flag(None, nm!["EnergyShieldToWard"]) {
                // L949
                let inc = modDB.Sum(ModType::INC, None, nm!["Ward", "Defences", "EnergyShield"]);
                // L950
                let more = modDB.More(None, nm!["Ward", "Defences"]);
                // L951
                ward += wardBase * (1.0 + inc / 100.0) * more;
                // L952
                if let Some(breakdown) = &breakdown {
                    // L953
                    let row = tbl! {
                        base = wardBase,
                        inc = and_str(inc != 0.0, || lfmt!(" x %.2f", 1.0 + inc / 100.0)),
                        more = and_str(more != 1.0, || lfmt!(" x %.2f", more)),
                        total = lfmt!("%.2f", wardBase * (1.0 + inc / 100.0) * more),
                        source = "Global",
                        item = item_val(item_in(&actor, n!("Global"))),
                    };
                    slots_of(breakdown, n!("Ward")).push(row);
                }
            } else {
                // L963
                ward += wardBase * calcLib::r#mod(&modDB, None, nm!["Ward", "Defences"]);
                // L964
                if let Some(breakdown) = &breakdown {
                    // L965
                    breakdown.slot("Global", None, None, wardBase, None, nm!["Ward", "Defences"]);
                }
            }
        }
        // L969
        energyShieldBase = modDB.Sum(ModType::BASE, None, nm!["EnergyShield"]);
        // L970
        if energyShieldBase > 0.0 {
            // L971
            if modDB.Flag(None, nm!["EnergyShieldToWard"]) {
                // L972
                energyShield += energyShieldBase * modDB.More(None, nm!["EnergyShield", "Defences"]);
            } else {
                // L974
                energyShield += energyShieldBase
                    * calcLib::r#mod(&modDB, None, nm!["EnergyShield", "Defences"]);
            }
            // L976
            if let Some(breakdown) = &breakdown {
                // L977
                let inc = modDB.Sum(ModType::INC, None, nm!["Defences", "EnergyShield"]);
                // L978
                let more = modDB.More(None, nm!["EnergyShield", "Defences"]);
                // L979
                let row = tbl! {
                    base = energyShieldBase,
                    inc = and_str(inc != 0.0, || lfmt!(" x %.2f", 1.0 + inc / 100.0)),
                    more = and_str(more != 1.0, || lfmt!(" x %.2f", more)),
                    total = lfmt!("%.2f", energyShieldBase * (1.0 + inc / 100.0) * more),
                    source = "Global",
                    item = item_val(item_in(&actor, n!("Global"))),
                };
                slots_of(breakdown, n!("EnergyShield")).push(row);
            }
        }
        // L989
        armourBase = modDB.Sum(ModType::BASE, None, nm!["Armour", "ArmourAndEvasion"]);
        // L990
        if armourBase > 0.0 {
            // L991
            armour += armourBase
                * calcLib::r#mod(&modDB, None, nm!["Armour", "ArmourAndEvasion", "Defences"]);
            // L992
            if let Some(breakdown) = &breakdown {
                // L993
                breakdown.slot(
                    "Global",
                    None,
                    None,
                    armourBase,
                    None,
                    nm!["Armour", "ArmourAndEvasion", "Defences"],
                );
            }
        }
        // L996
        evasionBase = modDB.Sum(ModType::BASE, None, nm!["Evasion", "ArmourAndEvasion"]);
        // L997
        if evasionBase > 0.0 {
            // L998
            if ironReflexes {
                // L999
                armour += evasionBase
                    * calcLib::r#mod(
                        &modDB,
                        None,
                        nm!["Armour", "Evasion", "ArmourAndEvasion", "Defences"],
                    );
                // L1000
                if let Some(breakdown) = &breakdown {
                    // L1001
                    breakdown.slot(
                        "Conversion",
                        Some("Evasion to Armour"),
                        None,
                        evasionBase,
                        None,
                        nm!["Armour", "Evasion", "ArmourAndEvasion", "Defences"],
                    );
                }
            } else {
                // L1004
                evasion += evasionBase
                    * calcLib::r#mod(&modDB, None, nm!["Evasion", "ArmourAndEvasion", "Defences"]);
                // L1005
                if let Some(breakdown) = &breakdown {
                    // L1006
                    breakdown.slot(
                        "Global",
                        None,
                        None,
                        evasionBase,
                        None,
                        nm!["Evasion", "ArmourAndEvasion", "Defences"],
                    );
                }
            }
        }
        // L1010
        let convManaToArmour = modDB.Sum(ModType::BASE, None, nm!["ManaConvertToArmour"]);
        // L1011
        if convManaToArmour > 0.0 {
            // L1012
            armourBase = 2.0 * modDB.Sum(ModType::BASE, None, nm!["Mana"]) * convManaToArmour / 100.0;
            // L1013
            let total = armourBase
                * calcLib::r#mod(
                    &modDB,
                    None,
                    nm!["Mana", "Armour", "ArmourAndEvasion", "Defences"],
                );
            // L1014
            armour += total;
            // L1015
            if let Some(breakdown) = &breakdown {
                // L1016
                breakdown.slot(
                    "Conversion",
                    Some("Mana to Armour"),
                    None,
                    armourBase,
                    Some(total),
                    nm!["Armour", "ArmourAndEvasion", "Defences", "Mana"],
                );
            }
        }
        // L1019
        let convManaToES = modDB.Sum(ModType::BASE, None, nm!["ManaGainAsEnergyShield"]);
        // L1020
        if convManaToES > 0.0 {
            // L1021
            energyShieldBase = modDB.Sum(ModType::BASE, None, nm!["Mana"]) * convManaToES / 100.0;
            // L1022
            energyShield += energyShieldBase
                * calcLib::r#mod(&modDB, None, nm!["Mana", "EnergyShield", "Defences"]);
            // L1023
            if let Some(breakdown) = &breakdown {
                // L1024
                breakdown.slot(
                    "Conversion",
                    Some("Mana to Energy Shield"),
                    None,
                    energyShieldBase,
                    None,
                    nm!["EnergyShield", "Defences", "Mana"],
                );
            }
        }
        // L1027
        let convLifeToArmour = modDB.Sum(ModType::BASE, None, nm!["LifeGainAsArmour"]);
        // L1028
        if convLifeToArmour > 0.0 {
            // L1029
            armourBase = modDB.Sum(ModType::BASE, None, nm!["Life"]) * convLifeToArmour / 100.0;
            // L1030
            let total;
            // L1031
            if modDB.Flag(None, nm!["ChaosInoculation"]) {
                // L1032
                total = 1.0;
            } else {
                // L1034
                total = armourBase
                    * calcLib::r#mod(
                        &modDB,
                        None,
                        nm!["Life", "Armour", "ArmourAndEvasion", "Defences"],
                    );
            }
            // L1036
            armour += total;
            // L1037
            if let Some(breakdown) = &breakdown {
                // L1038
                breakdown.slot(
                    "Conversion",
                    Some("Life to Armour"),
                    None,
                    armourBase,
                    Some(total),
                    nm!["Armour", "ArmourAndEvasion", "Defences", "Life"],
                );
            }
        }
        // L1041
        let convLifeToES = modDB.Sum(
            ModType::BASE,
            None,
            nm!["LifeConvertToEnergyShield", "LifeGainAsEnergyShield"],
        );
        // L1042
        if convLifeToES > 0.0 {
            // L1043
            energyShieldBase = modDB.Sum(ModType::BASE, None, nm!["Life"]) * convLifeToES / 100.0;
            // L1044
            let total;
            // L1045
            if modDB.Flag(None, nm!["ChaosInoculation"]) {
                // L1046
                total = 1.0;
            } else {
                // L1048
                total = energyShieldBase
                    * calcLib::r#mod(&modDB, None, nm!["Life", "EnergyShield", "Defences"]);
            }
            // L1050
            energyShield += total;
            // L1051
            if let Some(breakdown) = &breakdown {
                // L1052
                breakdown.slot(
                    "Conversion",
                    Some("Life to Energy Shield"),
                    None,
                    energyShieldBase,
                    Some(total),
                    nm!["EnergyShield", "Defences", "Life"],
                );
            }
        }
        // L1055
        let convEvasionToArmour = modDB.Sum(ModType::BASE, None, nm!["EvasionGainAsArmour"]);
        // L1056
        if convEvasionToArmour > 0.0 {
            // L1057
            armourBase = (modDB.Sum(ModType::BASE, None, nm!["Evasion", "ArmourAndEvasion"])
                + gearEvasion)
                * convEvasionToArmour
                / 100.0;
            // L1058
            let total = armourBase
                * calcLib::r#mod(
                    &modDB,
                    None,
                    nm!["Evasion", "Armour", "ArmourAndEvasion", "Defences"],
                );
            // L1059
            armour += total;
            // L1060
            if let Some(breakdown) = &breakdown {
                // L1061
                breakdown.slot(
                    "Conversion",
                    Some("Evasion to Armour"),
                    None,
                    armourBase,
                    Some(total),
                    nm!["Armour", "ArmourAndEvasion", "Defences", "Evasion"],
                );
            }
        }
        let _ = (energyShieldBase, armourBase, evasionBase);
        // L1064
        let ov = modDB.Override(None, nm!["EnergyShield"]);
        let v: Val = if ov.truthy() {
            Val::from(ov)
        } else {
            Val::from(m_max(round(energyShield, None), 0.0))
        };
        output.set(n!("EnergyShield"), v);
        // L1065
        output.set(n!("Armour"), m_max(round(armour, None), 0.0));
        // L1066
        output.set(n!("Evasion"), m_max(round(evasion, None), 0.0));
        // L1067
        let v = m_max(
            round(evasion * calcLib::r#mod(&modDB, None, nm!["MeleeEvasion"]), None),
            0.0,
        );
        output.set(n!("MeleeEvasion"), v);
        // L1068
        let v = m_max(
            round(
                evasion * calcLib::r#mod(&modDB, None, nm!["ProjectileEvasion"]),
                None,
            ),
            0.0,
        );
        output.set(n!("ProjectileEvasion"), v);
        // L1069
        let v = m_min(output.num(n!("Armour")), output.num(n!("Evasion")));
        output.set(n!("LowestOfArmourAndEvasion"), v);
        // L1070
        output.set(n!("Ward"), m_max(m_floor(ward), 0.0));
        // L1071
        output.set(n!("Gear:Ward"), gearWard);
        // L1072
        output.set(n!("Gear:EnergyShield"), gearEnergyShield);
        // L1073
        output.set(n!("Gear:Armour"), gearArmour);
        // L1074
        output.set(n!("Gear:Evasion"), gearEvasion);
        // L1075
        let v: Val = if modDB.Flag(None, nm!["ArmourESRecoveryCap"])
            && output.num(n!("Armour")) < output.num(n!("EnergyShield"))
        {
            Val::Bool(true)
        } else if modDB.Flag(None, nm!["EvasionESRecoveryCap"])
            && output.num(n!("Evasion")) < output.num(n!("EnergyShield"))
        {
            Val::Bool(true)
        } else {
            env.configInput.get(n!("conditionLowEnergyShield"))
        };
        output.set(n!("CappingES"), v);

        // L1077
        if output.truthy(n!("CappingES")) {
            // L1078
            let v: Val = if modDB.Flag(None, nm!["ArmourESRecoveryCap"])
                && modDB.Flag(None, nm!["EvasionESRecoveryCap"])
            {
                Val::from(m_min(output.num(n!("Armour")), output.num(n!("Evasion"))))
            } else if modDB.Flag(None, nm!["ArmourESRecoveryCap"]) {
                output.get(n!("Armour"))
            } else if modDB.Flag(None, nm!["EvasionESRecoveryCap"]) {
                output.get(n!("Evasion"))
            } else {
                lor(output.get(n!("EnergyShield")), || Val::from(0.0))
            };
            output.set(n!("EnergyShieldRecoveryCap"), v);
            // L1079
            let v: Val = if env.configInput.truthy(n!("conditionLowEnergyShield")) {
                Val::from(m_min(
                    output.num(n!("EnergyShield")) * misc.LowPoolThreshold,
                    output.num(n!("EnergyShieldRecoveryCap")),
                ))
            } else {
                output.get(n!("EnergyShieldRecoveryCap"))
            };
            output.set(n!("EnergyShieldRecoveryCap"), v);
        } else {
            // L1081
            let v = lor(output.get(n!("EnergyShield")), || Val::from(0.0));
            output.set(n!("EnergyShieldRecoveryCap"), v);
        }

        // L1084
        if modDB.Flag(None, nm!["CannotEvade"]) || enemyDB.Flag(None, nm!["CannotBeEvaded"]) {
            // L1085
            output.set(n!("EvadeChance"), 0.0);
            // L1086
            output.set(n!("MeleeEvadeChance"), 0.0);
            // L1087
            output.set(n!("ProjectileEvadeChance"), 0.0);
        } else if modDB.Flag(None, nm!["AlwaysEvade"]) {
            // L1089
            output.set(n!("EvadeChance"), 100.0);
            // L1090
            output.set(n!("MeleeEvadeChance"), 100.0);
            // L1091
            output.set(n!("ProjectileEvadeChance"), 100.0);
        } else {
            // L1093
            let enemyAccuracy = round(calcLib::val(&enemyDB, n!("Accuracy"), None), None);
            // L1094
            let evadeChance = modDB.Sum(ModType::BASE, None, nm!["EvadeChance"]);
            // L1095
            let hitChance = calcLib::r#mod(&enemyDB, None, nm!["HitChance"]);
            // L1096
            let mut evadeStat = output.num(n!("Evasion"));
            // L1097
            let mut meleeEvadeStat = output.num(n!("MeleeEvasion"));
            // L1098
            let mut projectileEvadeStat = output.num(n!("ProjectileEvasion"));
            // L1099
            if modDB.Flag(None, nm!["EvadeChanceBasedOnWard"]) {
                // L1100
                let ov = modDB.Override(None, nm!["EvadeChanceBasedOnWardPercent"]);
                let multiplier = ov.num().unwrap_or_else(|| {
                    lua_error!("attempt to perform arithmetic on a {} value", value_type(&ov))
                }) / 100.0;
                // L1101
                evadeStat = output.num(n!("Ward")) * multiplier;
                // L1102
                meleeEvadeStat = evadeStat;
                // L1103
                projectileEvadeStat = evadeStat;
            }
            // L1105
            let v = 100.0 - (calcs_hitChance(evadeStat, enemyAccuracy) - evadeChance) * hitChance;
            output.set(n!("EvadeChance"), v);
            // L1106
            let v = m_max(
                0.0,
                m_min(
                    misc.EvadeChanceCap,
                    (100.0
                        - (calcs_hitChance(meleeEvadeStat, enemyAccuracy) - evadeChance)
                            * hitChance)
                        * calcLib::r#mod(&modDB, None, nm!["EvadeChance", "MeleeEvadeChance"]),
                ),
            );
            output.set(n!("MeleeEvadeChance"), v);
            // L1107
            let v = m_max(
                0.0,
                m_min(
                    misc.EvadeChanceCap,
                    (100.0
                        - (calcs_hitChance(projectileEvadeStat, enemyAccuracy) - evadeChance)
                            * hitChance)
                        * calcLib::r#mod(&modDB, None, nm!["EvadeChance", "ProjectileEvadeChance"]),
                ),
            );
            output.set(n!("ProjectileEvadeChance"), v);
            // -- Condition for displaying evade chance only if melee or projectile evade chance have the same values
            // L1109
            if output.num(n!("MeleeEvadeChance")) != output.num(n!("ProjectileEvadeChance")) {
                // L1110
                output.set(n!("splitEvade"), true);
            } else {
                // L1112
                let v = output.get(n!("MeleeEvadeChance"));
                output.set(n!("EvadeChance"), v);
                // L1113
                output.set(n!("noSplitEvade"), true);
            }
            // L1115
            if let Some(breakdown) = &breakdown {
                let overridden = if env.configInput.truthy(n!("enemyLevel")) {
                    "overridden from"
                } else {
                    "can be overridden in"
                };
                // L1116
                let t = tbl![
                    lfmt!(
                        "Enemy level: %d ^8(%s the Configuration tab)",
                        env.enemyLevel.get(),
                        overridden
                    ),
                    lfmt!("Average enemy accuracy: %d", enemyAccuracy),
                    lfmt!(
                        "Approximate evade chance: %d%%",
                        output.num(n!("EvadeChance"))
                    ),
                ];
                breakdown.set(n!("EvadeChance"), t);
                // L1121
                let t = tbl![
                    lfmt!(
                        "Enemy level: %d ^8(%s the Configuration tab)",
                        env.enemyLevel.get(),
                        overridden
                    ),
                    lfmt!("Average enemy accuracy: %d", enemyAccuracy),
                    lfmt!("Effective Evasion: %d", meleeEvadeStat),
                    lfmt!(
                        "Approximate melee evade chance: %d%%",
                        output.num(n!("MeleeEvadeChance"))
                    ),
                ];
                breakdown.set(n!("MeleeEvadeChance"), t);
                // L1127
                let t = tbl![
                    lfmt!(
                        "Enemy level: %d ^8(%s the Configuration tab)",
                        env.enemyLevel.get(),
                        overridden
                    ),
                    lfmt!("Average enemy accuracy: %d", enemyAccuracy),
                    lfmt!("Effective Evasion: %d", projectileEvadeStat),
                    lfmt!(
                        "Approximate projectile evade chance: %d%%",
                        output.num(n!("ProjectileEvadeChance"))
                    ),
                ];
                breakdown.set(n!("ProjectileEvadeChance"), t);
            }
        }
    }

    // L1137
    let spellSuppressionChance = modDB.Sum(ModType::BASE, None, nm!["SpellSuppressionChance"]);
    ctx.spellSuppressionChance = Some(spellSuppressionChance);
    // L1138
    let totalSpellSuppressionChance =
        override_or(modDB.Override(None, nm!["SpellSuppressionChance"]), || {
            spellSuppressionChance
        });

    // -- Dodge
    // -- Acrobatics Spell Suppression to Spell Dodge Chance conversion.
    // L1142
    if modDB.Flag(None, nm!["ConvertSpellSuppressionToSpellDodge"]) {
        // L1143
        modDB.NewMod(
            n!("SpellDodgeChance"),
            ModType::BASE,
            spellSuppressionChance / 2.0,
            "Acrobatics",
            0,
            0,
            &[],
        );
    }

    // L1146
    let v = m_min(totalSpellSuppressionChance, misc.SuppressionChanceCap);
    output.set(n!("SpellSuppressionChance"), v);
    // L1147
    let v = m_max(
        misc.SuppressionEffect + modDB.Sum(ModType::BASE, None, nm!["SpellSuppressionEffect"]),
        0.0,
    );
    output.set(n!("SpellSuppressionEffect"), v);

    // L1149
    let v: Val = if enemyDB.Flag(None, nm!["CannotBeSuppressed"]) {
        Val::from(0.0)
    } else {
        output.get(n!("SpellSuppressionChance"))
    };
    output.set(n!("EffectiveSpellSuppressionChance"), v);
    // L1150
    let mut suppressRolls = 0.0;
    // L1151
    if env.mode_effective.get() {
        // L1152
        if modDB.Flag(None, nm!["SpellSuppressionChanceIsLucky"]) {
            // L1153
            suppressRolls += 1.0;
        }
        // L1155
        if modDB.Flag(None, nm!["SpellSuppressionChanceIsUnlucky"]) {
            // L1156
            suppressRolls -= 1.0;
        }
        // L1158
        if modDB.Flag(None, nm!["ExtremeLuck"]) {
            // L1159
            suppressRolls *= 2.0;
        }
    }
    // -- unlucky config to lower the value of block, dodge, evade etc for ehp
    // L1163
    if let Some(r) = unlucky_worst_of(&env) {
        // L1164
        suppressRolls = r;
    }
    // L1166
    if suppressRolls != 0.0 {
        // L1167
        let suppressChance = output.num(n!("EffectiveSpellSuppressionChance")) / 100.0;
        // L1168
        if modDB.Flag(None, nm!["Unexciting"]) {
            // -- Unexciting rolls three times and keeps the median result -> 3p^2 - 2p^3
            // L1170
            let v = (3.0 * lpow(suppressChance, 2.0) - 2.0 * lpow(suppressChance, 3.0)) * 100.0;
            output.set(n!("EffectiveSpellSuppressionChance"), v);
        } else if suppressRolls > 0.0 {
            // L1172
            let v = (1.0 - lpow(1.0 - suppressChance, suppressRolls + 1.0)) * 100.0;
            output.set(n!("EffectiveSpellSuppressionChance"), v);
        } else {
            // L1174
            let v = lpow(suppressChance, m_abs(suppressRolls))
                * output.num(n!("EffectiveSpellSuppressionChance"));
            output.set(n!("EffectiveSpellSuppressionChance"), v);
        }
    }

    // L1178
    let v = m_max(0.0, totalSpellSuppressionChance - misc.SuppressionChanceCap);
    output.set(n!("SpellSuppressionChanceOverCap"), v);

    // -- Dodge
    // L1181
    let totalAttackDodgeChance = modDB.Sum(ModType::BASE, None, nm!["AttackDodgeChance"]);
    // L1182
    let totalSpellDodgeChance = modDB.Sum(ModType::BASE, None, nm!["SpellDodgeChance"]);
    // L1183
    let attackDodgeChanceMax = misc.DodgeChanceCap;
    // L1184
    let spellDodgeChanceMax = override_or(modDB.Override(None, nm!["SpellDodgeChanceMax"]), || {
        modDB.Sum(ModType::BASE, None, nm!["SpellDodgeChanceMax"])
    });
    // L1185 (`Sum` never returns nil, so the `or 0` is dead)
    let enemyReduceDodgeChance = enemyDB.Sum(ModType::BASE, None, nm!["reduceEnemyDodge"]);

    // L1187
    output.set(
        n!("AttackDodgeChance"),
        m_min(totalAttackDodgeChance, attackDodgeChanceMax),
    );
    // L1188
    let v = if enemyDB.Flag(None, nm!["CannotBeDodged"]) {
        0.0
    } else {
        m_min(
            m_max(totalAttackDodgeChance - enemyReduceDodgeChance, 0.0),
            attackDodgeChanceMax,
        )
    };
    output.set(n!("EffectiveAttackDodgeChance"), v);
    // L1189
    output.set(
        n!("SpellDodgeChance"),
        m_min(totalSpellDodgeChance, spellDodgeChanceMax),
    );
    // L1190
    let v = if enemyDB.Flag(None, nm!["CannotBeDodged"]) {
        0.0
    } else {
        m_min(
            m_max(totalSpellDodgeChance - enemyReduceDodgeChance, 0.0),
            spellDodgeChanceMax,
        )
    };
    output.set(n!("EffectiveSpellDodgeChance"), v);
    // L1191
    if env.mode_effective.get() && modDB.Flag(None, nm!["DodgeChanceIsUnlucky"]) {
        // L1192
        let v = output.num(n!("EffectiveAttackDodgeChance")) / 100.0
            * output.num(n!("EffectiveAttackDodgeChance"));
        output.set(n!("EffectiveAttackDodgeChance"), v);
        // L1193
        let v = output.num(n!("EffectiveSpellDodgeChance")) / 100.0
            * output.num(n!("EffectiveSpellDodgeChance"));
        output.set(n!("EffectiveSpellDodgeChance"), v);
    }
    // L1195
    output.set(
        n!("AttackDodgeChanceOverCap"),
        m_max(0.0, totalAttackDodgeChance - attackDodgeChanceMax),
    );
    // L1196
    output.set(
        n!("SpellDodgeChanceOverCap"),
        m_max(0.0, totalSpellDodgeChance - spellDodgeChanceMax),
    );

    // L1198
    if let Some(breakdown) = &breakdown {
        // L1199
        let t = tbl![
            format!("Base: {}%", tostring_num(totalAttackDodgeChance)),
            format!("Max: {}%", tostring_num(attackDodgeChanceMax)),
            format!(
                "Total: {}%",
                tostring_num(
                    output.num(n!("AttackDodgeChance"))
                        + output.num(n!("AttackDodgeChanceOverCap"))
                )
            ),
        ];
        breakdown.set(n!("AttackDodgeChance"), t);
        // L1204
        let t = tbl![
            format!("Base: {}%", tostring_num(totalSpellDodgeChance)),
            format!("Max: {}%", tostring_num(spellDodgeChanceMax)),
            format!(
                "Total: {}%",
                tostring_num(
                    output.num(n!("SpellDodgeChance")) + output.num(n!("SpellDodgeChanceOverCap"))
                )
            ),
        ];
        breakdown.set(n!("SpellDodgeChance"), t);
    }
    Flow::Normal
}
