use pob_engine::parse_mod_line;

fn main() {
    let lines = [
        "You can only have one Herald",
        "Cannot Recover Energy Shield to above Evasion Rating",
        "When your Hits Impale Enemies, also Impale other Enemies near them",
        "Skills that have dealt a Critical Strike in the past 8 seconds deal 40% more Elemental Damage with Hits and Ailments",
        "You count as Dual Wielding while you are Unencumbered",
        "20% increased Taunt Duration",
        "20% increased Stun Duration on Enemies",
        "+1 to maximum number of Summoned Totems",
        "10% reduced Mana Cost of Curse Skills",
        "3% reduced Mana Cost of Skills",
        "Spell Skills have 10% increased Area of Effect",
        "Melee Skills have 10% increased Area of Effect",
        "10% increased Damage with Attack Skills while Fortified",
        "Retaliation Skills have 8% increased Speed",
        "Warcry Skills have 15% increased Area of Effect",
        "+1 to maximum number of Summoned Totems",
        "12% increased Minion Duration",
        "+5 to Maximum Virulence",
        "30% increased Mine Duration",
        "+3 to Maximum Rage",
        "+10 to maximum Valour",
        "Gain 1 Rage on Melee Hit",
        "Gain 15 Life per Enemy Killed",
        "+0.1 metres to Melee Strike Range",
        "Effect of Buffs granted by your Golems",
        "10% increased Effect of Cold Ailments",
    ];
    for l in lines {
        let r = parse_mod_line(l);
        println!(
            "{:<70} → {}",
            l,
            r.map(|p| format!("{}={:?}", p.mod_.name, p.mod_.kind))
                .unwrap_or_else(|| "(unparsed)".to_owned())
        );
    }
}
