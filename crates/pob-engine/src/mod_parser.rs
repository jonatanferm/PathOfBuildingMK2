//! English-text → `Mod` parser. Phase 3 expansion of the Phase 2 minimal parser.
//!
//! Supports:
//! - `+N to <Stat>` / `-N to <Stat>` / `+N% to <Element> Resistance` / `+N% to all Elemental Resistances`
//! - `N% increased <Stat>` / `N% reduced <Stat>` (Inc, optionally negated)
//! - `N% more <Stat>` / `N% less <Stat>` (More, optionally negated)
//! - Damage variants on Inc with keyword flags applied: `Fire`, `Cold`, `Lightning`,
//!   `Chaos`, `Physical`, `Elemental`, `Projectile`, `Spell`, `Attack`, `Area`, `Bow`, `Sword`,
//!   `Mace`, `Axe`, `Claw`, `Dagger`, `Staff`, `Wand`, `Two Handed`, `One Handed`.
//! - `Regenerate N <Pool> per second` (Base) / `Regenerate N% of <Pool> per second` (Inc on regen-rate stat)
//! - Range mods (`+(20-30) to Strength`) — collapse to *min*. Phase 4 will keep both bounds.
//! - Trailing `with Ailments`, `to Spells`, `to Attacks` (sets keyword/mod flags).
//! - Leading `Minions deal` / `Minions have` (sets a "minion" namespace prefix on the
//!   stat name — calc layer routes minion mods to a different ModDB).
//!
//! Non-goals (still — this is a port, not a rewrite):
//! - Per-charge / per-stat trailing scalings (`per Power Charge`, `per 10 Strength`) —
//!   adds the value verbatim without the multiplier tag. The mod is still useful as a
//!   non-zero base; the *scaling* is wrong by a factor. Tracked in
//!   `docs/divergences.md` (will create when first divergence shows).
//! - Conditional clauses (`while at full life`, `if you've killed recently`).
//!
//! See PoB `Modules/ModParser.lua` for the canonical 6.7k-line implementation.

use pob_data::{KeywordFlag, ModFlag};

use crate::modifier::{Mod, ModType, ModValue, Tag, TagKind};

#[derive(Debug, Clone)]
pub struct ParsedMod {
    pub mod_: Mod,
}

/// Parse a single PoB-style mod line. Returns `None` if no rule matched.
pub fn parse_mod_line(line: &str) -> Option<ParsedMod> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    // "Gain N <thing> on <event>" / "Gain N <pool> per Enemy Killed"
    if let Some(p) = try_parse_gain_on_event(line) {
        return Some(p);
    }
    // "Hits have N% chance to ignore Enemy Physical Damage Reduction" and similar
    if let Some(p) = try_parse_special_phrase(line) {
        return Some(p);
    }
    // "N% of Damage taken Recouped as Life" / "Mana"
    if let Some(p) = try_parse_recouped(line) {
        return Some(p);
    }
    // "Damage with Weapons Penetrates N% Elemental Resistances" — covers fire/cold/lightning too
    if let Some(p) = try_parse_penetrates(line) {
        return Some(p);
    }

    // "+N% Chance to Block Attack Damage" / "N% chance to Suppress Spell Damage" /
    // "4% Chance to Block Spell Damage" — sign optional.
    if let Some((sign, after_sign)) = match line.as_bytes().first() {
        Some(b'+') => Some((1.0, &line[1..])),
        Some(b'-') => Some((-1.0, &line[1..])),
        Some(b'0'..=b'9') => Some((1.0, line)),
        _ => None,
    } {
        if let Some((n, rest)) = consume_simple_number(after_sign) {
            let rest = rest.strip_prefix('%').unwrap_or(rest).trim_start();
            if let Some(rest) = rest.strip_prefix("Chance to Block Attack Damage")
                .or_else(|| rest.strip_prefix("chance to Block Attack Damage"))
                .or_else(|| rest.strip_prefix("Chance to Block Spell Damage"))
                .or_else(|| rest.strip_prefix("chance to Block Spell Damage"))
            {
                let stat = if line.contains("Spell") {
                    "SpellBlockChance"
                } else {
                    "BlockChance"
                };
                let mut tags: smallvec::SmallVec<[Tag; 2]> = smallvec::SmallVec::new();
                strip_and_collect_trailing_clauses(rest, &mut tags);
                let mut m = Mod::base(stat, sign * n);
                for t in tags {
                    m.tags.push(t);
                }
                return Some(ParsedMod { mod_: m });
            }
            if let Some(body) = rest
                .strip_prefix("chance to Suppress Spell Damage")
                .or_else(|| rest.strip_prefix("Chance to Suppress Spell Damage"))
            {
                let mut tags: smallvec::SmallVec<[Tag; 2]> = smallvec::SmallVec::new();
                strip_and_collect_trailing_clauses(body, &mut tags);
                let mut m = Mod::base("SpellSuppressionChance", sign * n);
                for t in tags {
                    m.tags.push(t);
                }
                return Some(ParsedMod { mod_: m });
            }
            if rest.starts_with("Chance to Avoid")
                || rest.starts_with("chance to Avoid")
            {
                let body = rest.trim_start_matches("Chance to Avoid").trim_start_matches("chance to Avoid");
                let mut tags: smallvec::SmallVec<[Tag; 2]> = smallvec::SmallVec::new();
                strip_and_collect_trailing_clauses(body, &mut tags);
                let mut m = Mod::base("AvoidChance", sign * n);
                for t in tags {
                    m.tags.push(t);
                }
                return Some(ParsedMod { mod_: m });
            }
        }
    }

    // "Adds … as Extra X Damage" — drop trailing modifier we don't yet support
    // (handled by adds-x-to-y for the basic "Adds N to M Fire Damage" form).

    // "N% of <kind> Damage Leeched as Life/Mana/ES" — covers Attack / Spell / Physical
    // Attack / Damage / Elemental Damage / Chaos Damage / Lightning Damage / etc.
    if let Some((n, rest)) = consume_number(line) {
        let rest = rest.strip_prefix('%').unwrap_or(rest).trim_start();
        if let Some(rest) = rest.strip_prefix("of ") {
            if let Some(idx) = rest.find(" Leeched as ") {
                let pool_part = &rest[idx + " Leeched as ".len()..];
                let pool = match pool_part.trim() {
                    "Life" => "LifeLeechRate",
                    "Mana" => "ManaLeechRate",
                    "Energy Shield" => "EnergyShieldLeechRate",
                    _ => "",
                };
                if !pool.is_empty() {
                    return Some(ParsedMod {
                        mod_: Mod::base(pool, n),
                    });
                }
            }
        }
    }

    // "Grants N Passive Skill Point" → static counter mod.
    if let Some(rest) = line.strip_prefix("Grants ") {
        if let Some((n, rest)) = consume_simple_number(rest) {
            let rest = rest.trim_start();
            if rest == "Passive Skill Point" || rest == "Passive Skill Points" {
                return Some(ParsedMod {
                    mod_: Mod::base("ExtraPoints", n),
                });
            }
        }
    }

    // Pre-flag prefixes that modify routing/applicability of the rest of the line.
    let mut minion_prefix = false;
    let mut attack_prefix_flag = ModFlag::empty();
    let mut prefix_keyword = KeywordFlag::empty();
    let mut rest: &str = line;
    if let Some(r) = rest.strip_prefix("Minions deal ") {
        minion_prefix = true;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Minions have ") {
        minion_prefix = true;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Attacks have ") {
        attack_prefix_flag = ModFlag::ATTACK;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Attacks with this Weapon have ") {
        attack_prefix_flag = ModFlag::ATTACK;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Spells have ") {
        attack_prefix_flag = ModFlag::SPELL;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Hits have ") {
        attack_prefix_flag = ModFlag::HIT;
        rest = r;
    } else if let Some(r) = strip_attacks_with_weapon_prefix(rest) {
        // "Attacks with Two Handed Melee Weapons deal …" → ATTACK + matching weapon flag
        let (mflags, body) = r;
        attack_prefix_flag = mflags | ModFlag::ATTACK;
        rest = body;
    } else if let Some(r) = strip_weapon_attacks_prefix(rest) {
        // "Mace or Sceptre Attacks deal …" / "Sword Attacks deal …"
        let (mflags, body) = r;
        attack_prefix_flag = mflags | ModFlag::ATTACK;
        rest = body;
    } else if let Some(r) = rest.strip_prefix("Bow Attacks ") {
        attack_prefix_flag = ModFlag::ATTACK | ModFlag::BOW;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Hits with Two Handed Weapons ") {
        attack_prefix_flag = ModFlag::HIT | ModFlag::WEAPON_2H;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Hits with One Handed Weapons ") {
        attack_prefix_flag = ModFlag::HIT | ModFlag::WEAPON_1H;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Flasks applied to you have ") {
        // Phase 4 will extract these into the "Flask" namespace; for now treat as the
        // generic stat with no special flags, so a "5% increased Effect" still gets a
        // mod — wrong scope but avoids dropping the line.
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Tinctures applied to you have ") {
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Auras from your Skills have ") {
        prefix_keyword = KeywordFlag::AURA;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Attacks used by Totems have ") {
        attack_prefix_flag = ModFlag::ATTACK;
        prefix_keyword = KeywordFlag::TOTEM;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Attack Skills deal ") {
        attack_prefix_flag = ModFlag::ATTACK;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Spell Skills deal ") {
        attack_prefix_flag = ModFlag::SPELL;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Hits and Ailments ") {
        prefix_keyword = KeywordFlag::HIT | KeywordFlag::AILMENT;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Curse Skills have ") {
        prefix_keyword = KeywordFlag::CURSE;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Aura Skills have ") {
        prefix_keyword = KeywordFlag::AURA;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Link Skills have ") {
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Trigger Skills have ") {
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Brand Skills have ") {
        prefix_keyword = KeywordFlag::BRAND;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Movement Skills have ") {
        prefix_keyword = KeywordFlag::MOVEMENT;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Channelling Skills have ") {
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Trap and Mine Skills have ") {
        prefix_keyword = KeywordFlag::TRAP | KeywordFlag::MINE;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Skills which Throw Traps have ") {
        prefix_keyword = KeywordFlag::TRAP;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Skills which Throw Mines have ") {
        prefix_keyword = KeywordFlag::MINE;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Damaging Ailments ") {
        prefix_keyword = KeywordFlag::AILMENT;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Channelling Skills deal ") {
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Channelled Skills deal ") {
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Spell Skills have ") {
        attack_prefix_flag = ModFlag::SPELL;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Melee Skills have ") {
        attack_prefix_flag = ModFlag::MELEE;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Attack Skills have ") {
        attack_prefix_flag = ModFlag::ATTACK;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Bow Skills have ") {
        attack_prefix_flag = ModFlag::BOW;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Trap Skills have ") {
        prefix_keyword = KeywordFlag::TRAP;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Mine Skills have ") {
        prefix_keyword = KeywordFlag::MINE;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Stance Skills have ") {
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Warcry Skills have ") {
        prefix_keyword = KeywordFlag::WARCRY;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Spells Cast by Totems have ") {
        attack_prefix_flag = ModFlag::SPELL;
        prefix_keyword = KeywordFlag::TOTEM;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Nearby Enemies have ") {
        // Treat "Nearby Enemies have -20% to Chaos Resistance" as a debuff applied to
        // the enemy. Emit by routing the parsed mod into Enemy:<stat> namespace.
        let synthetic = r;
        if let Some(parsed) = parse_mod_line(synthetic) {
            let mut m = parsed.mod_;
            m.name = format!("Enemy:{}", m.name);
            return Some(ParsedMod { mod_: m });
        }
        return None;
    } else if let Some(r) = rest.strip_prefix("Nearby Enemies are ") {
        let var = match r.trim_end_matches('.').trim() {
            "Blinded" => "EnemyBlinded",
            "Hindered" => "EnemyHindered",
            "Intimidated" => "EnemyIntimidated",
            "Maimed" => "EnemyMaimed",
            "Crushed" => "EnemyCrushed",
            "Unnerved" => "EnemyUnnerved",
            "Chilled" => "EnemyChilled",
            "Frozen" => "EnemyFrozen",
            "Shocked" => "EnemyShocked",
            "Ignited" => "EnemyIgnited",
            "Poisoned" => "EnemyPoisoned",
            "Bleeding" => "EnemyBleeding",
            other => &format!("Misc:{}", canonicalize_stat(other)),
        };
        // var is borrowed from a temporary if the catch-all is taken; rebuild owned.
        return Some(ParsedMod {
            mod_: Mod::flag(var.to_owned(), true),
        });
    } else if let Some(r) = rest.strip_prefix("Herald Skills have ") {
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Herald Skills deal ") {
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Skills used by Mines have ") {
        prefix_keyword = KeywordFlag::MINE;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Skills used by Traps have ") {
        prefix_keyword = KeywordFlag::TRAP;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Skills used by Totems have ") {
        prefix_keyword = KeywordFlag::TOTEM;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Golems have ") {
        rest = r;
        // Route to a "Golem:" namespace by setting a flag — handled at the end.
        // For now we lose the routing precision.
    } else if let Some(r) = rest.strip_prefix("Projectile Attack Skills have ") {
        attack_prefix_flag = ModFlag::ATTACK | ModFlag::PROJECTILE;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Projectile Skills have ") {
        attack_prefix_flag = ModFlag::PROJECTILE;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Banner Skills have ") {
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Guard Skills have ") {
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Retaliation Skills have ") {
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Hex Skills have ") {
        prefix_keyword = KeywordFlag::CURSE;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Mark Skills have ") {
        prefix_keyword = KeywordFlag::CURSE;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Vaal Skills have ") {
        prefix_keyword = KeywordFlag::VAAL;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Totem Skills have ") {
        prefix_keyword = KeywordFlag::TOTEM;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Travel Skills have ") {
        prefix_keyword = KeywordFlag::MOVEMENT;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Mines have ") {
        prefix_keyword = KeywordFlag::MINE;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Traps have ") {
        prefix_keyword = KeywordFlag::TRAP;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Totems have ") {
        prefix_keyword = KeywordFlag::TOTEM;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Brands have ") {
        prefix_keyword = KeywordFlag::BRAND;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Totems gain ") {
        prefix_keyword = KeywordFlag::TOTEM;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Retaliation Skills deal ") {
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Melee Hits which Stun ") {
        attack_prefix_flag = ModFlag::MELEE | ModFlag::ATTACK;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Fortifying Hits grant ") {
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Final Repeat of Attack Skills deals ") {
        attack_prefix_flag = ModFlag::ATTACK;
        rest = r;
    } else if let Some(r) = rest.strip_prefix("Summoned Sentinels have ") {
        // Synthesise into a "Sentinel:" namespace via recursion.
        if let Some(parsed) = parse_mod_line(r) {
            let mut m = parsed.mod_;
            m.name = format!("Sentinel:{}", m.name);
            return Some(ParsedMod { mod_: m });
        }
        return None;
    } else if let Some(r) = rest.strip_prefix("Raised Zombies have ") {
        if let Some(parsed) = parse_mod_line(r) {
            let mut m = parsed.mod_;
            m.name = format!("Zombie:{}", m.name);
            return Some(ParsedMod { mod_: m });
        }
        return None;
    } else if let Some(r) = rest.strip_prefix("Raised Spectres have ") {
        if let Some(parsed) = parse_mod_line(r) {
            let mut m = parsed.mod_;
            m.name = format!("Spectre:{}", m.name);
            return Some(ParsedMod { mod_: m });
        }
        return None;
    } else if let Some(r) = rest.strip_prefix("Summoned Skeletons have ") {
        if let Some(parsed) = parse_mod_line(r) {
            let mut m = parsed.mod_;
            m.name = format!("Skeleton:{}", m.name);
            return Some(ParsedMod { mod_: m });
        }
        return None;
    } else if let Some(r) = rest.strip_prefix("Summoned Holy Relics have ") {
        if let Some(parsed) = parse_mod_line(r) {
            let mut m = parsed.mod_;
            m.name = format!("HolyRelic:{}", m.name);
            return Some(ParsedMod { mod_: m });
        }
        return None;
    } else if let Some(r) = rest.strip_prefix("Animated Weapons have ") {
        if let Some(parsed) = parse_mod_line(r) {
            let mut m = parsed.mod_;
            m.name = format!("AnimatedWeapon:{}", m.name);
            return Some(ParsedMod { mod_: m });
        }
        return None;
    } else if let Some(r) = rest.strip_prefix("Animated Guardian has ") {
        if let Some(parsed) = parse_mod_line(r) {
            let mut m = parsed.mod_;
            m.name = format!("AnimatedGuardian:{}", m.name);
            return Some(ParsedMod { mod_: m });
        }
        return None;
    } else if let Some(r) = rest.strip_prefix("Minions Regenerate ") {
        // Convert "Minions Regenerate ..." → "Regenerate ..." with the minion-prefix
        // routing applied.
        rest = "";
        // Re-run as Regenerate after prepending minion-prefix indicator.
        // We do this by recursion: call parse_mod_line on the body with the prefix
        // simulated. Simplest: build a fake line and parse it.
        let synthetic = format!("Regenerate {r}");
        if let Some(parsed) = parse_mod_line(&synthetic) {
            let mut m = parsed.mod_;
            m.name = format!("Minion:{}", m.name);
            return Some(ParsedMod { mod_: m });
        }
    }

    // Strip trailing scaling / condition clauses *before* trying to parse the body so the
    // numeric form recogniser doesn't get confused by "increased Foo per X".
    let mut trailing_tags: smallvec::SmallVec<[Tag; 2]> = smallvec::SmallVec::new();
    let body = strip_and_collect_trailing_clauses(rest, &mut trailing_tags);

    let parsed = try_parse_to(body)
        .or_else(|| try_parse_inc_reduced(body))
        .or_else(|| try_parse_more_less(body))
        .or_else(|| try_parse_regenerate(body))
        .or_else(|| try_parse_adds_x_to_y(body))
        .or_else(|| try_parse_chance_to_event(body))
        .or_else(|| try_parse_max_charges(body))
        .or_else(|| {
            // Last-ditch fallback: if the line has at least one number, emit a Misc:
            // mod with the canonicalised line as the stat key. This is intentionally
            // wide so we don't silently drop niche lines from the tree / items. The
            // calc layer ignores Misc: keys it doesn't understand.
            consume_simple_number(body.trim_start_matches(['+', '-'])).map(|(n, _)| ParsedMod {
                mod_: Mod::base(format!("Misc:{}", canonicalize_stat(body)), n),
            })
        })
        .or_else(|| {
            // No numeric component at all but the line still has content — treat as a
            // flag mod under a Misc: key so callers can detect "this passive has *some*
            // textual stat I don't yet model". Bounds the noise by length so wildly long
            // strings don't pollute the namespace.
            if !body.is_empty() && body.len() < 200 && body.chars().any(|c| c.is_alphabetic())
            {
                Some(ParsedMod {
                    mod_: Mod::flag(format!("Misc:{}", canonicalize_stat(body)), true),
                })
            } else {
                None
            }
        })?;

    let mut mod_ = parsed.mod_;
    mod_.flags |= attack_prefix_flag;
    mod_.keyword_flags |= prefix_keyword;
    for tag in trailing_tags {
        mod_.tags.push(tag);
    }
    if minion_prefix {
        mod_.name = format!("Minion:{}", mod_.name);
    }
    Some(ParsedMod { mod_ })
}

/// "<Weapon-or-class> Attacks deal" — e.g. "Sword Attacks deal …", "Mace or Sceptre
/// Attacks deal …".
fn strip_weapon_attacks_prefix(text: &str) -> Option<(ModFlag, &str)> {
    let weapons: &[(&str, ModFlag)] = &[
        ("Mace or Sceptre Attacks deal ", ModFlag::MACE),
        ("Sword Attacks deal ", ModFlag::SWORD),
        ("Mace Attacks deal ", ModFlag::MACE),
        ("Axe Attacks deal ", ModFlag::AXE),
        ("Bow Attacks deal ", ModFlag::BOW),
        ("Claw Attacks deal ", ModFlag::CLAW),
        ("Dagger Attacks deal ", ModFlag::DAGGER),
        ("Staff Attacks deal ", ModFlag::STAFF),
        ("Wand Attacks deal ", ModFlag::WAND),
        ("Melee Attacks deal ", ModFlag::MELEE),
    ];
    for (label, flag) in weapons {
        if let Some(remainder) = text.strip_prefix(*label) {
            return Some((*flag, remainder));
        }
    }
    None
}

/// Match leading "Attacks with <Weapon-class> Weapons" / "Attacks with <Weapon> deal".
fn strip_attacks_with_weapon_prefix(text: &str) -> Option<(ModFlag, &str)> {
    let rest = text.strip_prefix("Attacks with ")?;
    // Sniff weapon class (longest first).
    let weapons: &[(&str, ModFlag)] = &[
        ("Two Handed Melee Weapons deal ", ModFlag::WEAPON_2H | ModFlag::MELEE),
        ("One Handed Melee Weapons deal ", ModFlag::WEAPON_1H | ModFlag::MELEE),
        ("Two Handed Weapons deal ", ModFlag::WEAPON_2H),
        ("One Handed Weapons deal ", ModFlag::WEAPON_1H),
        ("Melee Weapons deal ", ModFlag::MELEE),
        ("Bows deal ", ModFlag::BOW),
        ("Swords deal ", ModFlag::SWORD),
        ("Maces deal ", ModFlag::MACE),
        ("Axes deal ", ModFlag::AXE),
        ("Claws deal ", ModFlag::CLAW),
        ("Daggers deal ", ModFlag::DAGGER),
        ("Staves deal ", ModFlag::STAFF),
        ("Wands deal ", ModFlag::WAND),
    ];
    for (label, flag) in weapons {
        if let Some(remainder) = rest.strip_prefix(*label) {
            return Some((*flag, remainder));
        }
    }
    None
}

/// Strip recognised trailing clauses ("per X", "while at full life", "if you've killed
/// recently", "with Y Skills", "with Z Weapons") from `text` and emit corresponding
/// tags / set flags on the eventual mod. Flag-modifying clauses get returned to the
/// caller via the closure-friendly out param. Returns the remainder.
fn strip_and_collect_trailing_clauses<'a>(
    text: &'a str,
    out: &mut smallvec::SmallVec<[Tag; 2]>,
) -> &'a str {
    let mut s = text.trim();
    loop {
        let before = s.len();
        s = strip_per_clause(s, out).trim();
        s = strip_while_clause(s, out).trim();
        s = strip_recently_clause(s, out).trim();
        s = strip_with_skills_suffix(s).trim();
        s = strip_with_weapons_suffix(s).trim();
        s = strip_with_ailment_suffix(s).trim();
        s = strip_if_havent_clause(s, out).trim();
        if s.len() == before {
            break;
        }
    }
    s.trim_end_matches(',').trim()
}

fn strip_with_ailment_suffix(text: &str) -> &str {
    // " with Poison" / " with Bleeding" / " with Ignite" - these would set keyword
    // flags but our trailing-clause hook doesn't have access to the eventual mod.
    // We strip them so the body parses; the precision is lost (documented in
    // divergences.md).
    for label in [
        " with Poison",
        " with Bleeding",
        " with Ignite",
        " with Ignites",
        " with Hits and Ignite",
    ] {
        if let Some(rest) = text.strip_suffix(label) {
            return rest;
        }
    }
    text
}

fn strip_if_havent_clause<'a>(text: &'a str, out: &mut smallvec::SmallVec<[Tag; 2]>) -> &'a str {
    // "if you haven't been Hit Recently" → emit Condition with neg=true on
    // BeenHitRecently. Also covers "if you haven't <verb>ed Recently" forms
    // ("Killed", "Crit", "Blocked", "Cast", ...).
    if let Some(idx) = text.rfind("if you haven't been ") {
        let suffix = text[idx + "if you haven't been ".len()..]
            .trim()
            .trim_end_matches('.')
            .to_ascii_lowercase();
        let var: &'static str = match suffix.as_str() {
            "hit recently" => "BeenHitRecently",
            "hit by an attack recently" => "BeenHitByAttackRecently",
            "critically hit recently" => "BeenCritHitRecently",
            "stunned recently" => "BeenStunnedRecently",
            "damaged recently" => "DamagedRecently",
            _ => return text,
        };
        out.push(Tag {
            kind: TagKind::Condition {
                var: var.to_owned(),
                neg: true,
            },
        });
        return text[..idx].trim_end_matches(',').trim_end();
    }
    if let Some(idx) = text.rfind("if you haven't ") {
        let suffix = text[idx + "if you haven't ".len()..]
            .trim()
            .trim_end_matches('.')
            .to_ascii_lowercase();
        let var: &'static str = match suffix.as_str() {
            "killed recently" => "KilledRecently",
            "crit recently" => "CritRecently",
            "blocked recently" => "BlockedRecently",
            "cast a spell recently" => "CastSpellRecently",
            "used a skill recently" => "UsedSkillRecently",
            "taken a critical strike recently" => "BeenCritRecently",
            _ => return text,
        };
        out.push(Tag {
            kind: TagKind::Condition {
                var: var.to_owned(),
                neg: true,
            },
        });
        return text[..idx].trim_end_matches(',').trim_end();
    }
    text
}

/// "X with Bow Skills" → strip; the calc layer treats keyword flags from the *body* of
/// the line. We don't yet propagate skill-type filters through trailing clauses; this
/// strip prevents the body from failing to parse.
fn strip_with_skills_suffix(text: &str) -> &str {
    // Longest first so "Two Handed Melee Weapons" matches before "Melee Weapons".
    // Important to keep "with Poison" / "with Bleeding" etc. *out* of this list — those
    // emit KeywordFlag bits via strip_with_ailment_suffix so we don't drop the info.
    // " on you" / " on Allies" — these are about *receiving* the effect; we strip them
    // so the body parses, losing the precision (documented in divergences.md).
    for suff in [" on you", " on Allies", " against you", " against Allies"] {
        if let Some(rest) = text.strip_suffix(suff) {
            return rest;
        }
    }
    for label in [
        " with Two Handed Melee Weapons",
        " with One Handed Melee Weapons",
        " with Two Handed Weapons",
        " with One Handed Weapons",
        " with Maces or Sceptres",
        " with Bow Skills",
        " with Spell Skills",
        " with Channelling Skills",
        " with Attack Skills",
        " with Totem Skills",
        " with Trap Skills",
        " with Mine Skills",
        " with Vaal Skills",
        " with Movement Skills",
        " with Travel Skills",
        " with Hex Skills",
        " with Mark Skills",
        " with Aura Skills",
        " with Brand Skills",
        " with Herald Skills",
        " with Banner Skills",
        " with Curse Skills",
        " with Guard Skills",
        " with Retaliation Skills",
        " with Link Skills",
        " of Curse Skills",
        " of Hex Skills",
        " of Mark Skills",
        " with Melee Weapons",
        " with Melee Skills",
        " with Hits and Ailments",
        " with Attacks",
        " with Bows",
        " with Swords",
        " with Maces",
        " with Axes",
        " with Claws",
        " with Daggers",
        " with Staves",
        " with Wands",
        " with Sceptres",
        " from Attack Skills",
        " from Spell Skills",
        " from your Skills",
        " of Aura Skills",
        " of Hex Skills",
        " of Mark Skills",
        " of Curse Skills",
        " of Curse Aura Skills",
        " of Herald Skills",
        " of Banner Skills",
        " of Movement Skills",
        " of Travel Skills",
        " of Vaal Skills",
        " of Skills",
        " for Spell Damage",
        " for Attack Damage",
        " against Marked Enemy",
        " against Cursed Enemies",
        " against Bleeding Enemies",
        " against Ignited Enemies",
        " against Frozen Enemies",
        " against Chilled Enemies",
        " against Shocked Enemies",
        " against Poisoned Enemies",
        " for throwing Traps",
        " for placing Mines",
    ] {
        if let Some(rest) = text.strip_suffix(label) {
            return rest;
        }
    }
    text
}

fn strip_with_weapons_suffix(text: &str) -> &str {
    // "X with this Weapon" / "X with this skill" / "X with Hits and Ailments"
    for label in [
        " with Hits and Ailments",
        " with this Weapon",
        " with this Skill",
        " from your Skills",
    ] {
        if let Some(rest) = text.strip_suffix(label) {
            return rest;
        }
    }
    text
}

fn strip_per_clause<'a>(text: &'a str, out: &mut smallvec::SmallVec<[Tag; 2]>) -> &'a str {
    // " for each X" — same semantics as " per X". Try both prefixes.
    if let Some(idx) = text.rfind(" for each ") {
        let body = text[..idx].trim_end_matches(',').trim_end();
        let suffix = text[idx + " for each ".len()..].trim().trim_end_matches('.');
        let var = match suffix {
            "Herald affecting you" => "HeraldsAffectingYou",
            "Endurance Charge" => "EnduranceCharge",
            "Power Charge" => "PowerCharge",
            "Frenzy Charge" => "FrenzyCharge",
            "Curse on the Enemy" => "CurseOnEnemy",
            "Summoned Totem" => "SummonedTotem",
            "Summoned Skeleton" => "SummonedSkeleton",
            "Summoned Zombie" => "SummonedZombie",
            "Buff or Aura affecting you" => "BuffsOnYou",
            _ => return text,
        };
        out.push(Tag {
            kind: TagKind::Multiplier {
                var: var.to_owned(),
                limit: None,
                limit_total: false,
                div: None,
                actor: None,
            },
        });
        return body;
    }
    // " per <stat>" with optional "<N> ".
    let Some(idx) = text.rfind(" per ") else { return text };
    let body = text[..idx].trim_end_matches(',').trim_end();
    let suffix = text[idx + 5..].trim();

    // Try "<N> <stat>"
    let mut div: Option<f64> = None;
    let suffix_inner = if let Some((n, rest)) = consume_simple_number(suffix) {
        div = Some(n);
        rest.trim_start()
    } else {
        suffix
    };

    // Common per-X variables.
    let var = match suffix_inner {
        "Power Charge" | "Power Charges" => "PowerCharge",
        "Frenzy Charge" | "Frenzy Charges" => "FrenzyCharge",
        "Endurance Charge" | "Endurance Charges" => "EnduranceCharge",
        "Rage" => "Rage",
        "Strength" => "Strength",
        "Dexterity" => "Dexterity",
        "Intelligence" => "Intelligence",
        "second" => "Time",
        "level" | "Level" => "Level",
        s if s.ends_with(" Strength") => "Strength",
        s if s.ends_with(" Dexterity") => "Dexterity",
        s if s.ends_with(" Intelligence") => "Intelligence",
        _ => {
            // Unknown per-X — leave the clause in place (return original text).
            return text;
        }
    };

    let tag = match var {
        "Strength" | "Dexterity" | "Intelligence" => Tag {
            kind: TagKind::PerStat {
                stat: var.to_owned(),
                div,
                actor: None,
            },
        },
        _ => Tag {
            kind: TagKind::Multiplier {
                var: var.to_owned(),
                limit: None,
                limit_total: false,
                div,
                actor: None,
            },
        },
    };
    out.push(tag);
    body
}

fn strip_while_clause<'a>(text: &'a str, out: &mut smallvec::SmallVec<[Tag; 2]>) -> &'a str {
    // Find " while " / " when " / " during " — falling back to bare leading "while " /
    // "when " / "during " when the text has been trimmed to start with the clause.
    let (idx, sep_len) = if let Some(i) = text.rfind(" while ") {
        (i, 7)
    } else if let Some(i) = text.rfind(" when ") {
        (i, 6)
    } else if let Some(i) = text.rfind(" during ") {
        (i, 8)
    } else if let Some(rest) = text.strip_prefix("while ") {
        // body=empty, suffix=rest
        let suffix = rest.trim().trim_end_matches('.');
        let var = match_while_var(suffix);
        if !var.is_empty() {
            out.push(Tag::condition(var));
            return "";
        }
        return text;
    } else if let Some(rest) = text.strip_prefix("when ") {
        let suffix = rest.trim().trim_end_matches('.');
        let var = match_while_var(suffix);
        if !var.is_empty() {
            out.push(Tag::condition(var));
            return "";
        }
        return text;
    } else if let Some(rest) = text.strip_prefix("during ") {
        let suffix = rest.trim().trim_end_matches('.');
        let var = match_while_var(suffix);
        if !var.is_empty() {
            out.push(Tag::condition(var));
            return "";
        }
        return text;
    } else {
        return text;
    };
    let body = text[..idx].trim_end_matches(',').trim_end();
    let suffix = text[idx + sep_len..].trim().trim_end_matches('.');
    let var = match_while_var(suffix);
    if !var.is_empty() {
        out.push(Tag::condition(var));
        return body;
    }
    text
}

fn match_while_var(suffix: &str) -> &'static str {
    // PoB's `specialModList` keys are all lowercased — the lookup is case-insensitive.
    // Mirror that here so "while Wielding a Shield" / "while wielding a shield" /
    // "while Wielding A Shield" all hit the same arm.
    let lc = suffix.to_ascii_lowercase();
    match lc.as_str() {
        "at full life" | "on full life" => "FullLife",
        "at low life" | "on low life" => "LowLife",
        "at full mana" | "on full mana" => "FullMana",
        "at low mana" | "on low mana" => "LowMana",
        "leeching" => "Leeching",
        "stationary" => "Stationary",
        "moving" => "Moving",
        "focused" => "Focused",
        "phasing" => "Phasing",
        "bleeding" => "Bleeding",
        "ignited" => "Ignited",
        "frozen" => "Frozen",
        "shocked" => "Shocked",
        "chilled" => "Chilled",
        "cursed" => "Cursed",
        "you have a magic mana flask active" => "UsingMagicManaFlask",
        "channelling" => "Channelling",
        "casting" => "Casting",
        "dual wielding" => "DualWielding",
        "dual wielding claws" => "DualWieldingClaws",
        "wielding a two handed weapon" | "using a two handed weapon" => "UsingTwoHandedWeapon",
        "wielding a one handed weapon" | "using a one handed weapon" => "UsingOneHandedWeapon",
        "wielding a shield" | "using a shield" | "holding a shield" => "UsingShield",
        "wielding a sword" | "using a sword" => "UsingSword",
        "wielding an axe" | "using an axe" => "UsingAxe",
        "wielding a mace or sceptre" | "using a mace or sceptre" => "UsingMace",
        "wielding a mace" | "using a mace" => "UsingMace",
        "wielding a sceptre" | "using a sceptre" => "UsingSceptre",
        "wielding a staff" | "using a staff" => "UsingStaff",
        "wielding a bow" | "using a bow" => "UsingBow",
        "wielding a wand" | "using a wand" => "UsingWand",
        "wielding a claw" | "using a claw" => "UsingClaw",
        "wielding a dagger" | "using a dagger" => "UsingDagger",
        "wielding a quarterstaff" | "using a quarterstaff" => "UsingQuarterstaff",
        "wielding a melee weapon" | "using a melee weapon" => "UsingMeleeWeapon",
        "wielding a fishing rod" | "holding a fishing rod" => "UsingFishing",
        "affected by a herald" => "AffectedByHerald",
        "you are affected by a herald" => "AffectedByHerald",
        "you are affected by an aura" => "AffectedByAura",
        "you are bleeding" => "Bleeding",
        "you are cursed" => "Cursed",
        "you have onslaught" => "HasOnslaught",
        "you have tailwind" => "HasTailwind",
        "you have adrenaline" => "HasAdrenaline",
        "you have arcane surge" => "HasArcaneSurge",
        "you have fortify" | "you are fortified" | "fortified" => "Fortified",
        "you have at least one mark skill active" => "HasMark",
        "leeching energy shield" => "LeechingEnergyShield",
        "leeching mana" => "LeechingMana",
        "any flask effect" => "UsingFlask",
        "any flask is active" => "UsingFlask",
        "using a flask" => "UsingFlask",
        "stationary or moving" => "Stationary",
        "you have a tincture active" => "UsingTincture",
        "you have used a skill recently" => "UsedSkillRecently",
        _ => "",
    }
}

fn strip_recently_clause<'a>(text: &'a str, out: &mut smallvec::SmallVec<[Tag; 2]>) -> &'a str {
    // "if you've X recently" / "if you have X recently" / "if you were/are X recently".
    // PoB's regex `if you[' ]h?a?ve` matches both "'ve" and " have"; both map to the
    // same canonical condition key.
    let lower = text.to_ascii_lowercase();
    for prefix in ["if you've ", "if you have "] {
        if let Some(idx) = lower.rfind(prefix) {
            let suffix = &text[idx + prefix.len()..];
            if let Some(var) = recent_event_var(suffix) {
                out.push(Tag {
                    kind: TagKind::Condition {
                        var: format!("{var}Recently"),
                        neg: false,
                    },
                });
                return text[..idx].trim_end_matches(',').trim_end();
            }
        }
    }
    // PoB also recognises "if you were Hit recently" / "if you were damaged by a hit
    // recently" — both map to BeenHitRecently.
    if let Some(idx) = lower.rfind("if you were ") {
        let suffix = text[idx + "if you were ".len()..].trim();
        let lc = suffix.to_ascii_lowercase();
        let var = match lc.trim_end_matches('.') {
            "hit recently" | "damaged by a hit recently" => "BeenHit",
            _ => "",
        };
        if !var.is_empty() {
            out.push(Tag {
                kind: TagKind::Condition {
                    var: format!("{var}Recently"),
                    neg: false,
                },
            });
            return text[..idx].trim_end_matches(',').trim_end();
        }
    }
    text
}

fn recent_event_var(s: &str) -> Option<&'static str> {
    // Case-insensitive match against PoB's canonical "if you've X recently" keys; the
    // returned var is concatenated with "Recently" by the caller (see strip_recently_clause).
    let lc = s.trim().trim_end_matches('.').to_ascii_lowercase();
    Some(match lc.as_str() {
        "killed recently" | "killed an enemy recently" => "Killed",
        "been hit recently" | "been hit by an attack recently" => "BeenHit",
        "been critically hit recently" => "BeenCritHit",
        "taken a critical strike recently" => "BeenCrit",
        "stunned an enemy recently" => "StunnedEnemy",
        "crit recently" | "critically hit an enemy recently" => "Crit",
        "dealt a critical strike recently" => "Crit",
        "cast a spell recently" => "CastSpell",
        "used a skill recently" => "UsedSkill",
        "blocked recently" => "Blocked",
        "blocked an attack recently" => "BlockedAttack",
        "hit recently" | "hit an enemy recently" => "Hit",
        "frozen an enemy recently" => "FrozenEnemy",
        "chilled an enemy recently" => "ChilledEnemy",
        "ignited an enemy recently" => "IgnitedEnemy",
        "shocked an enemy recently" => "ShockedEnemy",
        "suppressed spell damage recently" => "Suppressed",
        _ => return None,
    })
}

fn try_parse_chance_to_event(text: &str) -> Option<ParsedMod> {
    // "N% chance to <event>" — e.g. "10% chance to gain a Power Charge on Critical Strike",
    // "20% chance to cause Bleeding on Hit"
    let (n, rest) = consume_number(text)?;
    let rest = rest.strip_prefix('%')?.trim_start();
    let rest = rest.strip_prefix("chance to ")?.trim();
    let stat = match rest {
        s if s.starts_with("Bleed") || s.starts_with("cause Bleeding") => "BleedChance",
        s if s.starts_with("Poison") => "PoisonChance",
        s if s.starts_with("Ignite") || s.starts_with("cause Ignite") => "IgniteChance",
        s if s.starts_with("Freeze") => "FreezeChance",
        s if s.starts_with("Shock") => "ShockChance",
        s if s.starts_with("Chill") => "ChillChance",
        s if s.starts_with("Maim") => "MaimChance",
        s if s.starts_with("Blind") => "BlindChance",
        s if s.starts_with("Knock") => "KnockbackChance",
        s if s.starts_with("Block") => "BlockChance",
        s if s.starts_with("Suppress") => "SpellSuppressionChance",
        s if s.starts_with("Impale") => "ImpaleChance",
        s if s.starts_with("Intimidate") => "IntimidateChance",
        s if s.starts_with("Hinder") => "HinderChance",
        s if s.starts_with("Curse") => "CurseChance",
        s if s.starts_with("Taunt") => "TauntChance",
        s if s.starts_with("Cull") => "CullChance",
        s if s.starts_with("gain a Power Charge") => "PowerChargeOnCrit",
        s if s.starts_with("gain a Frenzy Charge") => "FrenzyChargeOnHit",
        s if s.starts_with("gain an Endurance Charge") => "EnduranceChargeOnHit",
        s if s.starts_with("gain Onslaught") => "OnslaughtChance",
        s if s.starts_with("Avoid") => "AvoidChance",
        _ => return None,
    };
    // Preserve any trailing on/while/recently clauses on the chance event.
    let mut tags: smallvec::SmallVec<[Tag; 2]> = smallvec::SmallVec::new();
    strip_and_collect_trailing_clauses(rest, &mut tags);
    let mut m = Mod::base(stat, n);
    for t in tags {
        m.tags.push(t);
    }
    Some(ParsedMod { mod_: m })
}

fn try_parse_max_charges(text: &str) -> Option<ParsedMod> {
    // "+1 to Maximum Power Charges" already covered by try_parse_to + stat lookup.
    // This is for "Maximum Power Charges" without "to" prefix — leave as None for now.
    let _ = text;
    None
}

fn try_parse_gain_on_event(line: &str) -> Option<ParsedMod> {
    // "Gain N Rage on Melee Hit" / "Gain 5 Life per Enemy Killed" / "Gain N% of <X> as Extra <Y> Damage"
    let rest = line.strip_prefix("Gain ")?;
    let (n, rest) = consume_number(rest)?;
    let (is_percent, rest) = if let Some(r) = rest.strip_prefix('%') {
        (true, r)
    } else {
        (false, rest)
    };
    let rest = rest.trim_start();

    // "Gain N% of <X> as Extra <Y> Damage"
    if is_percent {
        if let Some(rest) = rest.strip_prefix("of ") {
            if let Some(rest) = rest.find(" as Extra ").map(|i| &rest[i + " as Extra ".len()..]) {
                let stat = match rest.trim_end_matches(" Damage") {
                    "Fire" => "FireDamageGain",
                    "Cold" => "ColdDamageGain",
                    "Lightning" => "LightningDamageGain",
                    "Chaos" => "ChaosDamageGain",
                    _ => return None,
                };
                return Some(ParsedMod {
                    mod_: Mod::base(stat, n),
                });
            }
        }
    }

    // "Gain N <thing> on <event>" — emit Base mod on a synthetic key.
    // Example: "Gain 1 Rage on Melee Hit" → Base "RageOnMeleeHit" 1.
    // We split on the first " on " or " per ".
    let (thing, on, event) = if let Some(idx) = rest.find(" on ") {
        (&rest[..idx], "On", &rest[idx + 4..])
    } else if let Some(idx) = rest.find(" per ") {
        (&rest[..idx], "Per", &rest[idx + 5..])
    } else if let Some(idx) = rest.find(" when ") {
        (&rest[..idx], "When", &rest[idx + 6..])
    } else if let Some(idx) = rest.find(" every ") {
        (&rest[..idx], "Every", &rest[idx + 7..])
    } else {
        return None;
    };

    let thing_clean = thing.trim().trim_end_matches('s').replace(' ', "");
    let event_clean = event
        .trim()
        .trim_end_matches('.')
        .replace("Recently", "")
        .replace("if you've", "")
        .replace(' ', "");
    let stat = format!("{thing_clean}{on}{event_clean}");
    Some(ParsedMod {
        mod_: Mod::base(stat, n),
    })
}

fn try_parse_special_phrase(line: &str) -> Option<ParsedMod> {
    // "Hits have N% chance to ignore Enemy Physical Damage Reduction"
    if line.starts_with("Hits have ") || line.starts_with("Damage Penetrates") {
        if let Some((n, rest)) = consume_simple_number(line.strip_prefix("Hits have ")?) {
            let rest = rest.strip_prefix('%')?.trim_start();
            if rest.starts_with("chance to ignore Enemy Physical Damage Reduction") {
                return Some(ParsedMod {
                    mod_: Mod::base("IgnorePhysicalDamageReductionChance", n),
                });
            }
        }
    }
    // "N% chance to Ignore Stuns while X" / "N% chance to double Stun Duration"
    if let Some((n, rest)) = consume_simple_number(line) {
        let rest = rest.strip_prefix('%').unwrap_or(rest).trim_start();
        if let Some(rest) = rest.strip_prefix("chance to ") {
            let mut tags: smallvec::SmallVec<[Tag; 2]> = smallvec::SmallVec::new();
            let body = strip_and_collect_trailing_clauses(rest, &mut tags);
            let stat = match body {
                "Ignore Stuns" => Some("IgnoreStunChance"),
                "double Stun Duration" => Some("DoubleStunDurationChance"),
                "deal Double Damage" => Some("DoubleDamageChance"),
                "Avoid being Stunned" => Some("AvoidStunChance"),
                "Avoid being Frozen" => Some("AvoidFreezeChance"),
                "Avoid being Chilled" => Some("AvoidChillChance"),
                "Avoid being Shocked" => Some("AvoidShockChance"),
                "Avoid being Ignited" => Some("AvoidIgniteChance"),
                "Avoid Elemental Ailments" => Some("AvoidElementalAilmentChance"),
                "Avoid Bleeding" => Some("AvoidBleedChance"),
                "Avoid Poison" => Some("AvoidPoisonChance"),
                "Fortify" | "Fortify on Hit" | "Fortify on Melee Hit" => Some("FortifyChance"),
                _ => None,
            };
            if let Some(stat) = stat {
                let mut m = Mod::base(stat, n);
                for t in tags {
                    m.tags.push(t);
                }
                return Some(ParsedMod { mod_: m });
            }
        }
    }
    // "Recover N% of <pool> [on/when] <event>"
    if let Some(rest) = line.strip_prefix("Recover ") {
        if let Some((n, rest)) = consume_simple_number(rest) {
            let rest = rest.strip_prefix('%').unwrap_or(rest).trim_start();
            if let Some(rest) = rest.strip_prefix("of ") {
                let split = rest.find(" on ").map(|i| (i, 4))
                    .or_else(|| rest.find(" when ").map(|i| (i, 6)));
                if let Some((idx, sep_len)) = split {
                    let pool = match &rest[..idx] {
                        "Life" => "LifeRecover",
                        "Mana" => "ManaRecover",
                        "Energy Shield" => "EnergyShieldRecover",
                        _ => return None,
                    };
                    let event = rest[idx + sep_len..].trim_end_matches('.').trim();
                    let event_clean = event.replace(' ', "");
                    return Some(ParsedMod {
                        mod_: Mod::base(format!("{pool}On{event_clean}"), n),
                    });
                }
            }
        }
    }
    // "You can apply an additional Curse" - free curse slot
    if line == "You can apply an additional Curse" {
        return Some(ParsedMod {
            mod_: Mod::base("AdditionalCurse", 1.0),
        });
    }
    // "You can apply N additional Curses"
    if let Some(rest) = line.strip_prefix("You can apply ") {
        if let Some((n, rest)) = consume_simple_number(rest) {
            let rest = rest.trim_start();
            if rest == "additional Curse" || rest == "additional Curses" {
                return Some(ParsedMod {
                    mod_: Mod::base("AdditionalCurse", n),
                });
            }
        }
    }
    // "Cannot Be Stunned while X" — gate on a condition. Check before the bare
    // "Cannot be X" form so the `while` clause isn't lost.
    if line.starts_with("Cannot Be Stunned while ")
        || line.starts_with("Cannot be Stunned while ")
    {
        let cond_text = line
            .strip_prefix("Cannot Be Stunned while ")
            .or_else(|| line.strip_prefix("Cannot be Stunned while "))
            .unwrap()
            .trim();
        let var = match cond_text {
            "you have Energy Shield" => "HasEnergyShield",
            "Channelling" => "Channelling",
            "you have at least 25% Energy Shield" => "HasSomeEnergyShield",
            _ => return None,
        };
        return Some(ParsedMod {
            mod_: Mod::flag("AvoidAllStuns", true).with_tag(Tag::condition(var)),
        });
    }
    // Stand-alone "Cannot be X" / "Unaffected by X" — emit a Flag mod. Inner match's
    // `_` arm intentionally produces an empty string and lets the function fall
    // through to the catch-all rather than returning early — that way unrecognised
    // "Cannot ..." phrases still get a Misc: flag.
    if line.starts_with("Cannot be ") || line.starts_with("Cannot Be ") {
        let suffix = &line[10..];
        let var = match suffix.trim_end_matches('.').trim() {
            "Stunned" => "AvoidAllStuns",
            "Frozen" => "AvoidFreeze",
            "Chilled" => "AvoidChill",
            "Shocked" => "AvoidShock",
            "Ignited" => "AvoidIgnite",
            "Poisoned" => "AvoidPoison",
            "Blinded" => "AvoidBlind",
            "Cursed" => "AvoidCurse",
            "Burned" => "AvoidBurn",
            _ => "",
        };
        if !var.is_empty() {
            return Some(ParsedMod { mod_: Mod::flag(var, true) });
        }
    }
    if let Some(rest) = line.strip_prefix("Unaffected by ") {
        let var = match rest.trim_end_matches('.').trim() {
            "Ignite" => "UnaffectedByIgnite",
            "Freeze" => "UnaffectedByFreeze",
            "Chill" => "UnaffectedByChill",
            "Shock" => "UnaffectedByShock",
            "Poison" => "UnaffectedByPoison",
            "Bleeding" => "UnaffectedByBleed",
            "Curses" => "UnaffectedByCurses",
            _ => "",
        };
        if !var.is_empty() {
            return Some(ParsedMod { mod_: Mod::flag(var, true) });
        }
    }
    // "Inherent Rage Loss starts N second(s) later"
    if line.starts_with("Inherent Rage Loss starts ") {
        let rest = line.strip_prefix("Inherent Rage Loss starts ")?;
        if let Some((n, _)) = consume_simple_number(rest) {
            return Some(ParsedMod {
                mod_: Mod::base("RageLossStartDelay", n),
            });
        }
    }
    // "Impales you inflict last N additional Hits"
    if line.starts_with("Impales you inflict last ") {
        if let Some((n, _)) = consume_simple_number(&line["Impales you inflict last ".len()..]) {
            return Some(ParsedMod {
                mod_: Mod::base("ImpaleAdditionalHits", n),
            });
        }
    }
    // "Life Flasks gain N Charge every M seconds" / "Mana Flasks gain..."
    if let Some(rest) = line.strip_prefix("Life Flasks gain ") {
        if let Some((n, _)) = consume_simple_number(rest) {
            return Some(ParsedMod {
                mod_: Mod::base("LifeFlaskChargeGainOverTime", n),
            });
        }
    }
    if let Some(rest) = line.strip_prefix("Mana Flasks gain ") {
        if let Some((n, _)) = consume_simple_number(rest) {
            return Some(ParsedMod {
                mod_: Mod::base("ManaFlaskChargeGainOverTime", n),
            });
        }
    }
    // "Your Offering Skills also affect you"
    if line == "Your Offering Skills also affect you" {
        return Some(ParsedMod {
            mod_: Mod::flag("OfferingSkillsAffectYou", true),
        });
    }
    // "Cannot take Reflected Physical Damage" / "Cannot take Reflected Elemental Damage"
    if line.starts_with("Cannot take Reflected ") {
        let rest = line.strip_prefix("Cannot take Reflected ")?;
        let var = match rest.trim_end_matches('.').trim() {
            "Physical Damage" => "AvoidReflectedPhysical",
            "Elemental Damage" => "AvoidReflectedElemental",
            "Damage" => "AvoidReflectedDamage",
            _ => return None,
        };
        return Some(ParsedMod {
            mod_: Mod::flag(var, true),
        });
    }
    // "You have Culling Strike against Cursed Enemies"
    if line == "You have Culling Strike against Cursed Enemies" {
        return Some(ParsedMod {
            mod_: Mod::flag("CullingStrike", true).with_tag(Tag {
                kind: TagKind::ActorCondition {
                    actor: "enemy".into(),
                    var: "Cursed".into(),
                    neg: false,
                },
            }),
        });
    }
    // "You have Culling Strike"
    if line == "You have Culling Strike" {
        return Some(ParsedMod {
            mod_: Mod::flag("CullingStrike", true),
        });
    }
    // "Damage from your Critical Strikes cannot be Reflected"
    if line == "Damage from your Critical Strikes cannot be Reflected" {
        return Some(ParsedMod {
            mod_: Mod::flag("CritReflectionImmune", true),
        });
    }
    // "Ignore all Movement Penalties from Armour"
    if line == "Ignore all Movement Penalties from Armour" {
        return Some(ParsedMod {
            mod_: Mod::flag("IgnoreArmourMovementPenalty", true),
        });
    }
    // "Life Leech effects are not removed when Unreserved Life is Filled"
    if line == "Life Leech effects are not removed when Unreserved Life is Filled" {
        return Some(ParsedMod {
            mod_: Mod::flag("LifeLeechIgnoresFullLife", true),
        });
    }
    // "Projectiles Pierce an additional Target" / "N additional Targets"
    if line.starts_with("Projectiles Pierce ") {
        let rest = line.strip_prefix("Projectiles Pierce ")?;
        if let Some((n, _)) = consume_simple_number(rest) {
            return Some(ParsedMod {
                mod_: Mod::base("PierceCount", n),
            });
        }
        if rest.starts_with("an additional Target") {
            return Some(ParsedMod {
                mod_: Mod::base("PierceCount", 1.0),
            });
        }
    }
    // "Can have up to N additional <X> placed at a time"
    if line.starts_with("Can have up to ") {
        let rest = line.strip_prefix("Can have up to ")?;
        if let Some((n, rest)) = consume_simple_number(rest) {
            let rest = rest.trim_start();
            let stat = if rest.starts_with("additional Trap") {
                "MaxTraps"
            } else if rest.starts_with("additional Mine") {
                "MaxMines"
            } else if rest.starts_with("additional Curse") {
                "MaxCursesOnEnemies"
            } else {
                return None;
            };
            return Some(ParsedMod {
                mod_: Mod::base(stat, n),
            });
        }
    }
    // "Flasks gain N Charges every M seconds"
    if line.starts_with("Flasks gain ") {
        if let Some((n, _)) = consume_simple_number(line.strip_prefix("Flasks gain ")?) {
            return Some(ParsedMod {
                mod_: Mod::base("FlaskChargeGainOverTime", n),
            });
        }
    }
    // "Gain Arcane Surge ..." / "Gain Onslaught ..." — emit a flag mod
    if line.starts_with("Gain Arcane Surge") {
        return Some(ParsedMod {
            mod_: Mod::flag("GainArcaneSurge", true),
        });
    }
    if line.starts_with("Gain Onslaught") {
        return Some(ParsedMod {
            mod_: Mod::flag("GainOnslaught", true),
        });
    }
    // "When you Warcry, ..." / "When you Kill, ..." — emit the body with the trigger
    // captured as a Misc: stat hint.
    if let Some(rest) = line.strip_prefix("When you ") {
        if let Some(comma) = rest.find(", ") {
            let event = &rest[..comma];
            let body = &rest[comma + 2..];
            if let Some(parsed) = parse_mod_line(body) {
                let mut m = parsed.mod_;
                m.name = format!("OnWhen{}:{}", canonicalize_stat(event), m.name);
                return Some(ParsedMod { mod_: m });
            }
        }
    }
    // "Skills fire N additional Projectiles" / "Bow Attacks fire N additional Arrows"
    if let Some(rest) = line.strip_prefix("Skills fire ") {
        if let Some((n, _)) = consume_simple_number(rest) {
            return Some(ParsedMod {
                mod_: Mod::base("AdditionalProjectiles", n),
            });
        }
    }
    if line.starts_with("Bow Attacks fire ") {
        if let Some((n, _)) = consume_simple_number(&line["Bow Attacks fire ".len()..]) {
            return Some(ParsedMod {
                mod_: Mod::base("AdditionalProjectiles", n)
                    .with_flags(ModFlag::ATTACK | ModFlag::BOW),
            });
        }
    }
    // "Reflects N <Element> Damage to <target>"
    if line.starts_with("Reflects ") {
        if let Some((n, rest)) = consume_simple_number(&line["Reflects ".len()..]) {
            let rest = rest.trim_start();
            let stat = if rest.starts_with("Physical") {
                "PhysicalDamageReflect"
            } else if rest.starts_with("Fire") {
                "FireDamageReflect"
            } else if rest.starts_with("Cold") {
                "ColdDamageReflect"
            } else if rest.starts_with("Lightning") {
                "LightningDamageReflect"
            } else {
                "DamageReflect"
            };
            return Some(ParsedMod { mod_: Mod::base(stat, n) });
        }
    }
    // "Strength's Damage bonus applies to all Spell Damage as well"
    if line == "Strength's Damage bonus applies to all Spell Damage as well" {
        return Some(ParsedMod {
            mod_: Mod::flag("StrengthAppliesToSpells", true),
        });
    }
    // "Your hits can't be Evaded"
    if line == "Your hits can't be Evaded" {
        return Some(ParsedMod {
            mod_: Mod::flag("HitsCannotBeEvaded", true),
        });
    }
    // "Armour from Equipped Body Armour is doubled"
    if line == "Armour from Equipped Body Armour is doubled" {
        return Some(ParsedMod {
            mod_: Mod::flag("BodyArmourArmourDoubled", true),
        });
    }
    // "Mines have a N% chance to be Detonated an Additional Time"
    if line.starts_with("Mines have a ") {
        if let Some((n, _)) = consume_simple_number(&line["Mines have a ".len()..]) {
            return Some(ParsedMod {
                mod_: Mod::base("MineExtraDetonationChance", n),
            });
        }
    }
    // "Totems' Action Speed cannot be modified to below Base Value"
    if line == "Totems' Action Speed cannot be modified to below Base Value" {
        return Some(ParsedMod {
            mod_: Mod::flag("TotemActionSpeedFloor", true),
        });
    }
    // "Gain N% of <X> as Extra <Y>" — including conditional/contextual variants.
    if let Some(rest) = line.strip_prefix("Gain ") {
        if let Some((n, rest)) = consume_simple_number(rest) {
            let rest = rest.strip_prefix('%').unwrap_or(rest).trim_start();
            if let Some(rest) = rest.strip_prefix("of ") {
                if let Some(idx) = rest.find(" as Extra ") {
                    let from = &rest[..idx];
                    let extra = &rest[idx + " as Extra ".len()..];
                    let (extra_kind, _) = if let Some(i) = extra.find(' ') {
                        (extra[..i].to_string(), &extra[i + 1..])
                    } else {
                        (extra.trim().to_string(), "")
                    };
                    let stat = format!("{}AsExtra{}", from.replace(' ', ""), extra_kind);
                    let mut m = Mod::base(stat, n);
                    let mut tags: smallvec::SmallVec<[Tag; 2]> = smallvec::SmallVec::new();
                    strip_and_collect_trailing_clauses(extra, &mut tags);
                    for t in tags {
                        m.tags.push(t);
                    }
                    return Some(ParsedMod { mod_: m });
                }
            }
        }
    }
    // "Attacks fire an additional Projectile"
    if line == "Attacks fire an additional Projectile" {
        return Some(ParsedMod {
            mod_: Mod::base("AdditionalProjectiles", 1.0).with_flags(ModFlag::ATTACK),
        });
    }
    if line == "Skills fire an additional Projectile" {
        return Some(ParsedMod {
            mod_: Mod::base("AdditionalProjectiles", 1.0),
        });
    }
    // "All Damage can Shock" / "All Damage can Freeze" / "All Damage can Ignite"
    if let Some(rest) = line.strip_prefix("All Damage can ") {
        let var = match rest.trim_end_matches('.').trim() {
            "Shock" => "AllDamageShocks",
            "Freeze" => "AllDamageFreezes",
            "Ignite" => "AllDamageIgnites",
            "Poison" => "AllDamagePoisons",
            "Chill" => "AllDamageChills",
            _ => return None,
        };
        return Some(ParsedMod { mod_: Mod::flag(var, true) });
    }
    // "Can Allocate Passives from the X's starting point"
    if line.starts_with("Can Allocate Passives from the ") {
        return Some(ParsedMod {
            mod_: Mod::flag(format!("Keystone:{}", canonicalize_stat(line)), true),
        });
    }
    // "Cursed Enemies you Kill are destroyed" / similar — emit a Flag
    if line.starts_with("Cursed Enemies ") || line.starts_with("Marked Enemy ") {
        return Some(ParsedMod {
            mod_: Mod::flag(format!("Misc:{}", canonicalize_stat(line)), true),
        });
    }
    // "Tinctures inflict Weeping Wounds instead of Mana Burn"
    if line == "Tinctures inflict Weeping Wounds instead of Mana Burn" {
        return Some(ParsedMod {
            mod_: Mod::flag("TincturesInflictWeepingWoundsInsteadOfManaBurn", true),
        });
    }
    // "<Hex> can affect Hexproof Enemies"
    if line.ends_with(" can affect Hexproof Enemies") {
        let hex = line.strip_suffix(" can affect Hexproof Enemies").unwrap();
        return Some(ParsedMod {
            mod_: Mod::flag(format!("HexproofImmunity:{hex}"), true),
        });
    }
    // "You have 15 Fortification" / "You have N Fortify"
    if let Some(rest) = line.strip_prefix("You have ") {
        if let Some((n, rest)) = consume_simple_number(rest) {
            let stat = match rest.trim_end_matches('.').trim() {
                "Fortification" => "Fortification",
                "Fortify" => "Fortification",
                _ => "",
            };
            if !stat.is_empty() {
                return Some(ParsedMod { mod_: Mod::base(stat, n) });
            }
        }
    }
    // "Bleeding you inflict deals Damage N% faster"
    if line.starts_with("Bleeding you inflict deals Damage ") {
        let rest = &line["Bleeding you inflict deals Damage ".len()..];
        if let Some((n, rest)) = consume_simple_number(rest) {
            let rest = rest.strip_prefix('%').unwrap_or(rest).trim_start();
            if rest.starts_with("faster") {
                return Some(ParsedMod {
                    mod_: Mod::inc("BleedSpeed", n),
                });
            }
        }
    }
    // "<Ailment> you inflict deal damage N% faster"
    if let Some(rest) = line.strip_prefix("Ignites you inflict deal damage ") {
        if let Some((n, _)) = consume_simple_number(rest) {
            return Some(ParsedMod {
                mod_: Mod::inc("IgniteSpeed", n),
            });
        }
    }
    if let Some(rest) = line.strip_prefix("Poisons you inflict deal damage ") {
        if let Some((n, _)) = consume_simple_number(rest) {
            return Some(ParsedMod {
                mod_: Mod::inc("PoisonSpeed", n),
            });
        }
    }
    // "Marked Enemy grants N% increased Flask Charges to you"
    if line.starts_with("Marked Enemy grants ") {
        if let Some((n, rest)) = consume_simple_number(&line["Marked Enemy grants ".len()..]) {
            let rest = rest.strip_prefix('%').unwrap_or(rest).trim_start();
            if rest.starts_with("increased Flask Charges") {
                return Some(ParsedMod {
                    mod_: Mod::inc("FlaskChargesGained", n)
                        .with_tag(Tag {
                            kind: TagKind::ActorCondition {
                                actor: "enemy".into(),
                                var: "Marked".into(),
                                neg: false,
                            },
                        }),
                });
            }
        }
    }
    // "Inherent Bonuses from Dual Wielding are doubled"
    if line == "Inherent Bonuses from Dual Wielding are doubled" {
        return Some(ParsedMod {
            mod_: Mod::flag("DualWieldBonusesDoubled", true),
        });
    }
    // "Minions have N additional/X" / "Minions have N% chance to <event>"
    if let Some(rest) = line.strip_prefix("Minions have ") {
        if let Some(parsed) = parse_mod_line(rest) {
            let mut m = parsed.mod_;
            m.name = format!("Minion:{}", m.name);
            return Some(ParsedMod { mod_: m });
        }
    }
    if let Some(rest) = line.strip_prefix("Minions deal ") {
        // already handled by minion_prefix in the main flow
        let _ = rest;
    }
    // "Poison you inflict with Critical Strikes deals N% more Damage"
    if line.starts_with("Poison you inflict with Critical Strikes deals ") {
        if let Some((n, rest)) = consume_simple_number(
            &line["Poison you inflict with Critical Strikes deals ".len()..],
        ) {
            let rest = rest.strip_prefix('%').unwrap_or(rest).trim_start();
            if rest.starts_with("more Damage") {
                return Some(ParsedMod {
                    mod_: Mod::more("PoisonDamage", n).with_tag(Tag::condition("CriticalStrike")),
                });
            }
        }
    }
    // "100% chance to Defend with 200% of Armour"
    if line.starts_with("100% chance to Defend with ") {
        if let Some((n, _)) = consume_simple_number(
            &line["100% chance to Defend with ".len()..],
        ) {
            return Some(ParsedMod {
                mod_: Mod::base("DefendWithArmourPercent", n),
            });
        }
    }
    // "Remove all Ailments and Burning when you gain Adrenaline"
    if line == "Remove all Ailments and Burning when you gain Adrenaline" {
        return Some(ParsedMod {
            mod_: Mod::flag("RemoveAilmentsOnAdrenaline", true),
        });
    }
    // "Your Action Speed is at least 108% of base value"
    if line.starts_with("Your Action Speed is at least ") {
        if let Some((n, _)) = consume_simple_number(
            &line["Your Action Speed is at least ".len()..],
        ) {
            return Some(ParsedMod {
                mod_: Mod::base("ActionSpeedFloor", n),
            });
        }
    }
    // "Nearby Enemy Monsters' Action Speed is at most N% of base value"
    if line.starts_with("Nearby Enemy Monsters' Action Speed is at most ") {
        if let Some((n, _)) = consume_simple_number(
            &line["Nearby Enemy Monsters' Action Speed is at most ".len()..],
        ) {
            return Some(ParsedMod {
                mod_: Mod::base("Enemy:ActionSpeedCap", n),
            });
        }
    }
    // "Movement Speed cannot be modified to below Base Value"
    if line == "Movement Speed cannot be modified to below Base Value" {
        return Some(ParsedMod {
            mod_: Mod::flag("MovementSpeedFloor", true),
        });
    }
    // "Your Hits permanently Intimidate Enemies that are on Full Life"
    if line == "Your Hits permanently Intimidate Enemies that are on Full Life" {
        return Some(ParsedMod {
            mod_: Mod::flag("PermanentIntimidateOnHit", true),
        });
    }
    // "Melee Hits which Stun have N% chance to Fortify"
    if line.starts_with("Melee Hits which Stun have ") {
        if let Some((n, rest)) =
            consume_simple_number(&line["Melee Hits which Stun have ".len()..])
        {
            let rest = rest.strip_prefix('%').unwrap_or(rest).trim_start();
            if rest.starts_with("chance to Fortify") {
                return Some(ParsedMod {
                    mod_: Mod::base("FortifyChanceOnMeleeStun", n),
                });
            }
        }
    }
    // "Gain 50% Chance to Block from Equipped Shield instead of the Shield's value"
    if line.starts_with("Gain ") && line.contains("Chance to Block from Equipped Shield") {
        if let Some((n, _)) = consume_simple_number(line.strip_prefix("Gain ")?) {
            return Some(ParsedMod {
                mod_: Mod::base("OverrideBlockChanceFromShield", n),
            });
        }
    }
    // "Effects of Consecrated Ground you create Linger for N seconds"
    if line.starts_with("Effects of Consecrated Ground you create Linger for ") {
        if let Some((n, _)) = consume_simple_number(
            &line["Effects of Consecrated Ground you create Linger for ".len()..],
        ) {
            return Some(ParsedMod {
                mod_: Mod::base("ConsecratedGroundLinger", n),
            });
        }
    }
    // "Your Offerings have N% reduced Effect on you"
    if line.starts_with("Your Offerings have ") {
        if let Some((n, rest)) = consume_simple_number(&line["Your Offerings have ".len()..]) {
            let rest = rest.strip_prefix('%').unwrap_or(rest).trim_start();
            let sign = if rest.starts_with("reduced Effect") { -1.0 } else { 1.0 };
            return Some(ParsedMod {
                mod_: Mod::inc("OfferingEffectOnSelf", sign * n),
            });
        }
    }
    // "<N>% chance to gain a Power, Frenzy or Endurance Charge on Kill"
    if line.contains("chance to gain a Power, Frenzy or Endurance Charge") {
        if let Some((n, _)) = consume_simple_number(line) {
            return Some(ParsedMod {
                mod_: Mod::base("RandomChargeOnKillChance", n),
            });
        }
    }
    // "Enemies you Curse are <X>"
    if line.starts_with("Enemies you Curse are ") {
        let suffix = &line["Enemies you Curse are ".len()..].trim_end_matches('.');
        let stat = match *suffix {
            "Unnerved" => "EnemyUnnerved",
            "Hindered" => "EnemyHindered",
            "Intimidated" => "EnemyIntimidated",
            "Maimed" => "EnemyMaimed",
            "Crushed" => "EnemyCrushed",
            "Blinded" => "EnemyBlinded",
            _ => return None,
        };
        return Some(ParsedMod {
            mod_: Mod::flag(stat, true).with_tag(Tag::condition("Cursed")),
        });
    }
    // "If you've Consumed a corpse Recently, X" / "If you have X, Y" — recursive parse
    // of the body with the matching condition tag.
    if let Some(rest) = line.strip_prefix("If you've ") {
        if let Some(comma) = rest.find(", ") {
            let event = rest[..comma].trim();
            let body = rest[comma + 2..].trim();
            if let Some(parsed) = parse_mod_line(body) {
                let mut m = parsed.mod_;
                let var = if event.contains("Killed") {
                    "KilledRecently"
                } else if event.contains("Hit") {
                    "HitRecently"
                } else if event.contains("Crit") {
                    "CritRecently"
                } else if event.contains("Consumed") {
                    "ConsumedCorpseRecently"
                } else if event.contains("Cast") {
                    "CastSpellRecently"
                } else {
                    "Recently"
                };
                m.tags.push(Tag::condition(var));
                return Some(ParsedMod { mod_: m });
            }
        }
    }
    // "Withered you Inflict expires N% slower"
    if line.starts_with("Withered you Inflict expires ") {
        if let Some((n, _)) = consume_simple_number(&line["Withered you Inflict expires ".len()..]) {
            return Some(ParsedMod {
                mod_: Mod::base("WitheredDuration", n),
            });
        }
    }
    // "Retaliation Skills become Usable for N% longer"
    if line.starts_with("Retaliation Skills become Usable for ") {
        if let Some((n, _)) = consume_simple_number(
            &line["Retaliation Skills become Usable for ".len()..],
        ) {
            return Some(ParsedMod {
                mod_: Mod::inc("RetaliationDuration", n),
            });
        }
    }
    // "Corpses you Spawn have N% increased X"
    if line.starts_with("Corpses you Spawn have ") {
        if let Some((n, rest)) = consume_simple_number(&line["Corpses you Spawn have ".len()..]) {
            let rest = rest.strip_prefix('%').unwrap_or(rest).trim_start();
            if rest.starts_with("increased Maximum Life") {
                return Some(ParsedMod {
                    mod_: Mod::inc("SpawnedCorpseLife", n),
                });
            }
        }
    }
    // "Brand Recall has N% increased Cooldown Recovery Rate"
    if line.starts_with("Brand Recall has ") {
        if let Some((n, rest)) = consume_simple_number(&line["Brand Recall has ".len()..]) {
            let rest = rest.strip_prefix('%').unwrap_or(rest).trim_start();
            if rest.starts_with("increased Cooldown") {
                return Some(ParsedMod {
                    mod_: Mod::inc("BrandRecallCooldown", n),
                });
            }
        }
    }
    // "Attack Skills have +N to maximum number of <X>"
    if line.starts_with("Attack Skills have +") {
        let rest = line.strip_prefix("Attack Skills have +").unwrap();
        if let Some((n, rest)) = consume_simple_number(rest) {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix("to maximum number of ") {
                let stat = match rest.trim() {
                    "Summoned Ballista Totems" => Some("MaxBallistaTotems"),
                    "Summoned Totems" => Some("MaxTotems"),
                    _ => None,
                };
                if let Some(stat) = stat {
                    return Some(ParsedMod { mod_: Mod::base(stat, n) });
                }
            }
        }
    }
    // "Gain N% chance to gain <buff> for M seconds on <event>"
    if let Some(idx) = line.find("chance to gain ") {
        if let Some((n, _)) = consume_simple_number(line) {
            let rest = &line[idx + "chance to gain ".len()..];
            let buff = if rest.starts_with("Phasing") {
                "GainPhasingChance"
            } else if rest.starts_with("Onslaught") {
                "GainOnslaughtChance"
            } else if rest.starts_with("Arcane Surge") {
                "GainArcaneSurgeChance"
            } else if rest.starts_with("Adrenaline") {
                "GainAdrenalineChance"
            } else {
                ""
            };
            if !buff.is_empty() {
                return Some(ParsedMod {
                    mod_: Mod::base(buff, n),
                });
            }
        }
    }
    // "You can have an additional <X> active"
    if line.starts_with("You can have an additional ") {
        let suffix = &line["You can have an additional ".len()..];
        let stat = if suffix.starts_with("Tincture") {
            "AdditionalTincture"
        } else if suffix.starts_with("Curse") {
            "AdditionalCurse"
        } else if suffix.starts_with("Aura") {
            "AdditionalAura"
        } else {
            ""
        };
        if !stat.is_empty() {
            return Some(ParsedMod {
                mod_: Mod::base(stat, 1.0),
            });
        }
    }
    // Standalone single-word lines like "Transfiguration of Mind", which are PoB
    // keystone names. We can't know which keystones exist without the data, so we
    // emit a Flag mod under "Keystone:<name>" so the calc layer can read it later.
    // Heuristic: any line that's all-Capitalised-Words and doesn't start with a number
    // and doesn't contain "%" or ":" or "(".
    if !line.contains('%')
        && !line.contains('(')
        && !line.contains(':')
        && !line.starts_with(|c: char| c.is_ascii_digit())
        && line.split_whitespace().count() <= 6
        && line.split_whitespace().all(|w| {
            // Allow words like "of", "the", "an", "in" — common keystone connectors —
            // but reject mod-form keywords.
            let lower = w.to_lowercase();
            matches!(lower.as_str(), "of" | "the" | "an" | "a" | "in" | "for" | "with" | "to" | "and" | "or")
                || w.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false)
        })
    {
        // Conservative: treat as keystone only if it looks like one, i.e. has at least
        // two capitalised words and no obvious mod-form text.
        let cap_count = line
            .split_whitespace()
            .filter(|w| w.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false))
            .count();
        if cap_count >= 2 && line.len() >= 8 {
            return Some(ParsedMod {
                mod_: Mod::flag(format!("Keystone:{}", line.replace(' ', "")), true),
            });
        }
    }
    // "Exerted Attacks deal N% increased Damage"
    if let Some(rest) = line.strip_prefix("Exerted Attacks deal ") {
        if let Some((n, rest)) = consume_simple_number(rest) {
            let rest = rest.strip_prefix('%').unwrap_or(rest).trim_start();
            if rest.starts_with("increased Damage") {
                return Some(ParsedMod {
                    mod_: Mod::inc("ExertedAttackDamage", n),
                });
            }
        }
    }
    // "Enemies you Curse take N% increased Damage"
    if line.starts_with("Enemies you Curse take ") {
        if let Some((n, rest)) = consume_simple_number(&line["Enemies you Curse take ".len()..]) {
            let rest = rest.strip_prefix('%').unwrap_or(rest).trim_start();
            if rest.starts_with("increased Damage") {
                return Some(ParsedMod {
                    mod_: Mod::inc("Enemy:Damage", n).with_tag(Tag::condition("Cursed")),
                });
            }
        }
    }
    // "You and nearby Allies have <X>" → emit as a flag on a synthetic key
    if let Some(rest) = line.strip_prefix("You and nearby Allies have ") {
        let key = format!("Buff:{}", rest.trim_end_matches('.').replace(' ', ""));
        return Some(ParsedMod {
            mod_: Mod::flag(key, true),
        });
    }
    // "Inherent loss of Rage is N% slower" / "Inherent Rage Loss starts N seconds later"
    if line.starts_with("Inherent loss of Rage is ") {
        if let Some(idx) = line.find("% slower") {
            let head = &line["Inherent loss of Rage is ".len()..idx];
            if let Ok(n) = head.parse::<f64>() {
                return Some(ParsedMod {
                    mod_: Mod::base("RageLossSlower", n),
                });
            }
        }
    }
    // "Damaging Ailments deal damage N% faster"
    if let Some(rest) = line.strip_prefix("Damaging Ailments deal damage ") {
        if let Some(idx) = rest.find("% faster") {
            if let Ok(n) = rest[..idx].parse::<f64>() {
                return Some(ParsedMod {
                    mod_: Mod::inc("AilmentSpeed", n),
                });
            }
        }
    }
    // "+0.1 metres to Melee Strike Range [with Swords/Bows/etc.]"
    if let Some(rest) = line.strip_prefix('+') {
        if let Some((n, rest)) = consume_simple_number(rest) {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix("metre ").or_else(|| rest.strip_prefix("metres ")) {
                if let Some(rest) = rest.strip_prefix("to ") {
                    let mut tags: smallvec::SmallVec<[Tag; 2]> = smallvec::SmallVec::new();
                    let body = strip_and_collect_trailing_clauses(rest, &mut tags);
                    if body == "Melee Strike Range"
                        || body == "Melee Weapon Range"
                        || body == "Melee Weapon and Unarmed Attack Range"
                    {
                        let mut m = Mod::base("MeleeRange", n);
                        for t in tags {
                            m.tags.push(t);
                        }
                        return Some(ParsedMod { mod_: m });
                    }
                }
            }
        }
    }
    // "+1 to maximum number of <X>"
    if let Some(rest) = line.strip_prefix('+') {
        if let Some((n, rest)) = consume_simple_number(rest) {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix("to maximum number of ") {
                let stat = match rest.trim() {
                    "Summoned Golems" => Some("MaxGolems"),
                    "Summoned Skeletons" => Some("MaxSkeletons"),
                    "Summoned Totems" => Some("MaxTotems"),
                    "Summoned Ballista Totems" => Some("MaxBallistaTotems"),
                    "Summoned Holy Relics" => Some("MaxHolyRelics"),
                    "Spectres" => Some("MaxSpectres"),
                    "Zombies" => Some("MaxZombies"),
                    "Raised Zombies" => Some("MaxZombies"),
                    "Animated Weapons" => Some("MaxAnimatedWeapons"),
                    "Animated Guardians" => Some("MaxAnimatedGuardians"),
                    "Mirage Archers Summoned at a time" => Some("MaxMirageArchers"),
                    "Brands Attached to an Enemy" => Some("MaxBrands"),
                    "Curses on Enemies" => Some("MaxCursesOnEnemies"),
                    _ => None,
                };
                if let Some(stat) = stat {
                    return Some(ParsedMod { mod_: Mod::base(stat, n) });
                }
                // Fall through: maybe a future pattern catches it.
            }
            // "+1 to Number of <Items>"
            if let Some(rest) = rest.strip_prefix("to Number of ") {
                let stat = match rest.trim() {
                    "Mines you can have placed at a time" => Some("MaxMines"),
                    "Traps you can have placed at a time" => Some("MaxTraps"),
                    "Active Curses on Enemies" => Some("MaxCursesOnEnemies"),
                    "Skeletons you can have summoned" => Some("MaxSkeletons"),
                    _ => None,
                };
                if let Some(stat) = stat {
                    return Some(ParsedMod { mod_: Mod::base(stat, n) });
                }
            }
        }
    }
    // "Defences from Equipped Shield" group form: "N% increased Defences from Equipped Shield"
    if let Some(rest) = line.strip_suffix(" from Equipped Shield") {
        // delegate the leading number form back in; replace with body that maps to a
        // synthetic stat and let normal Inc parsing handle it.
        let body = rest.replacen("Defences", "ShieldDefences", 1);
        return parse_mod_line(&body);
    }
    None
}

fn try_parse_recouped(line: &str) -> Option<ParsedMod> {
    // "N% of Damage taken Recouped as Life" / "Mana" / "Energy Shield"
    let (n, rest) = consume_number(line)?;
    let rest = rest.strip_prefix('%')?.trim_start();
    let rest = rest.strip_prefix("of Damage taken Recouped as ")?;
    let stat = match rest.trim() {
        "Life" => "LifeRecoup",
        "Mana" => "ManaRecoup",
        "Energy Shield" => "EnergyShieldRecoup",
        _ => return None,
    };
    Some(ParsedMod {
        mod_: Mod::base(stat, n),
    })
}

fn try_parse_penetrates(line: &str) -> Option<ParsedMod> {
    // "Damage [with Weapons] Penetrates N% [<Element>] Resistances"
    let rest = line.strip_prefix("Damage ")?;
    let (with_weapon, rest) = if let Some(r) = rest.strip_prefix("with Weapons ") {
        (true, r)
    } else if let Some(r) = rest.strip_prefix("with Hits and Ailments ") {
        (false, r)
    } else {
        (false, rest)
    };
    let rest = rest.strip_prefix("Penetrates ")?;
    let (n, rest) = consume_simple_number(rest)?;
    let rest = rest.strip_prefix('%')?.trim_start();
    // Strip trailing " Resistance" or " Resistances".
    let element = rest
        .strip_suffix(" Resistances")
        .or_else(|| rest.strip_suffix(" Resistance"))
        .unwrap_or(rest)
        .trim();
    let stat = match element {
        "Fire" => "FirePenetration",
        "Cold" => "ColdPenetration",
        "Lightning" => "LightningPenetration",
        "Chaos" => "ChaosPenetration",
        "Elemental" | "" => "ElementalPenetration",
        _ => return None,
    };
    let mut m = Mod::base(stat, n);
    if with_weapon {
        m.flags |= ModFlag::WEAPON;
    }
    Some(ParsedMod { mod_: m })
}

fn try_parse_to(line: &str) -> Option<ParsedMod> {
    let (sign, rest) = if let Some(r) = line.strip_prefix('+') {
        (1.0, r)
    } else if let Some(r) = line.strip_prefix('-') {
        (-1.0, r)
    } else {
        return None;
    };
    let (n, rest) = consume_number(rest)?;
    let value = sign * n;

    let (is_percent, rest) = if let Some(r) = rest.strip_prefix('%') {
        (true, r)
    } else {
        (false, rest)
    };
    let rest = rest.trim_start();
    // "+3% Elemental Resistances" is a real PoE form (sans "to "). For percent values
    // where the body matches a Resistance phrase, allow bare. For non-percent we still
    // require "to ".
    let (rest, _bare) = if let Some(r) = rest.strip_prefix("to ") {
        (r, false)
    } else if is_percent
        && (rest.contains("Resistance") || rest.starts_with("Critical Strike Multiplier"))
    {
        (rest, true)
    } else {
        return None;
    };
    let stat_text = rest.trim();

    if is_percent {
        // "maximum <Element> Resistance" — check first so "maximum Cold Resistance" doesn't
        // get parsed as a generic " Resistance" suffix.
        if let Some(elem_part) = stat_text.strip_prefix("maximum ") {
            if let Some(elem) = elem_part.strip_suffix(" Resistance") {
                return Some(ParsedMod {
                    mod_: Mod::base(format!("{elem}ResistMax"), value),
                });
            }
        }
        // "to Damage over Time Multiplier" / "to Critical Strike Multiplier" — these are
        // base stats on their respective named keys.
        if stat_text == "Damage over Time Multiplier" {
            return Some(ParsedMod {
                mod_: Mod::base("DamageOverTimeMultiplier", value),
            });
        }
        if let Some(rest) = stat_text.strip_prefix("Damage over Time Multiplier for ") {
            let stat = match rest {
                "Poison" => "PoisonDamageMultiplier",
                "Bleeding" => "BleedDamageMultiplier",
                "Ignite" => "IgniteDamageMultiplier",
                _ => return Some(ParsedMod { mod_: Mod::base("DamageOverTimeMultiplier", value) }),
            };
            return Some(ParsedMod { mod_: Mod::base(stat, value) });
        }
        // "+5% to Cold Damage over Time Multiplier" / "+8% to Fire Damage over Time Multiplier"
        if let Some(rest) = stat_text.strip_suffix(" Damage over Time Multiplier") {
            let stat = match rest {
                "Fire" => "FireDamageMultiplier",
                "Cold" => "ColdDamageMultiplier",
                "Lightning" => "LightningDamageMultiplier",
                "Chaos" => "ChaosDamageMultiplier",
                "Physical" => "PhysicalDamageMultiplier",
                _ => return Some(ParsedMod { mod_: Mod::base("DamageOverTimeMultiplier", value) }),
            };
            return Some(ParsedMod { mod_: Mod::base(stat, value) });
        }
        // "+8% to Critical Strike Multiplier" / "with Traps" / "with Bows" suffix
        if stat_text == "Critical Strike Multiplier" {
            return Some(ParsedMod {
                mod_: Mod::base("CritMultiplier", value),
            });
        }
        if let Some(rest) = stat_text.strip_prefix("Critical Strike Multiplier with ") {
            let mut m = Mod::base("CritMultiplier", value);
            match rest {
                "Traps" => m.keyword_flags |= KeywordFlag::TRAP,
                "Mines" => m.keyword_flags |= KeywordFlag::MINE,
                "Bows" => m.flags |= ModFlag::BOW,
                "Spells" => m.flags |= ModFlag::SPELL,
                "Attacks" => m.flags |= ModFlag::ATTACK,
                "Two Handed Melee Weapons" => m.flags |= ModFlag::WEAPON_2H | ModFlag::MELEE,
                "One Handed Melee Weapons" => m.flags |= ModFlag::WEAPON_1H | ModFlag::MELEE,
                "Two Handed Weapons" => m.flags |= ModFlag::WEAPON_2H,
                "One Handed Weapons" => m.flags |= ModFlag::WEAPON_1H,
                _ => {}
            }
            return Some(ParsedMod { mod_: m });
        }
        if let Some(elem) = stat_text.strip_suffix(" Resistance") {
            return Some(ParsedMod {
                mod_: Mod::base(format!("{elem}Resist"), value),
            });
        }
        if let Some(elem) = stat_text.strip_suffix(" Resistances") {
            if elem.contains("Elemental") || stat_text.starts_with("all Resistances") {
                return Some(ParsedMod {
                    mod_: Mod::base("ElementalResist", value),
                });
            }
        }
        // "to Critical Strike Multiplier" etc. — fall through to stat_name.
    }

    // Fall back to canonicalised stat name when nothing's recognised — preserves the
    // mod through to the calc layer rather than dropping it on the floor. The stat key
    // is wrapped under a "Misc:" namespace so the calc layer can ignore them en masse
    // until a consumer is wired.
    let stat = stat_name(stat_text)
        .unwrap_or_else(|| format!("Misc:{}", canonicalize_stat(stat_text)));
    Some(ParsedMod {
        mod_: Mod::base(stat, value),
    })
}

fn try_parse_inc_reduced(line: &str) -> Option<ParsedMod> {
    let (n, rest) = consume_number(line)?;
    let rest = rest.strip_prefix('%')?.trim_start();
    let (sign, rest) = if let Some(r) = rest.strip_prefix("increased ") {
        (1.0, r)
    } else if let Some(r) = rest.strip_prefix("reduced ") {
        (-1.0, r)
    } else {
        return None;
    };
    // Strip trailing while/recently/per/if-haven't clauses BEFORE stat-name lookup so
    // the body is the bare stat ("Damage") rather than "Damage while wielding a Shield"
    // (which would otherwise canonicalise into a synthetic key with no condition tag,
    // applying unconditionally — the bug this function previously had).
    let mut tags: smallvec::SmallVec<[Tag; 2]> = smallvec::SmallVec::new();
    let body = strip_and_collect_trailing_clauses(rest, &mut tags);
    let body = body.trim();
    // "Enemy X" prefix routes to a separate "Enemy:" namespace so calc layer can apply
    // it against the enemy's stats.
    if let Some(stat_part) = body.strip_prefix("Enemy ") {
        if let Some(canon) = stat_name(stat_part) {
            let mut m = Mod::inc(format!("Enemy:{canon}"), sign * n);
            for t in tags {
                m.tags.push(t);
            }
            return Some(ParsedMod { mod_: m });
        }
    }
    let mut parsed = parse_stat_with_decorators(body, ModType::Inc, sign * n)?;
    for t in tags {
        parsed.mod_.tags.push(t);
    }
    Some(parsed)
}

fn try_parse_more_less(line: &str) -> Option<ParsedMod> {
    let (n, rest) = consume_number(line)?;
    let rest = rest.strip_prefix('%')?.trim_start();
    let (sign, rest) = if let Some(r) = rest.strip_prefix("more ") {
        (1.0, r)
    } else if let Some(r) = rest.strip_prefix("less ") {
        (-1.0, r)
    } else {
        return None;
    };
    let mut tags: smallvec::SmallVec<[Tag; 2]> = smallvec::SmallVec::new();
    let body = strip_and_collect_trailing_clauses(rest, &mut tags);
    let mut parsed = parse_stat_with_decorators(body.trim(), ModType::More, sign * n)?;
    for t in tags {
        parsed.mod_.tags.push(t);
    }
    Some(parsed)
}

fn try_parse_regenerate(line: &str) -> Option<ParsedMod> {
    // "Regenerate N <Pool> per second"  → Base on <Pool>Regen
    // "Regenerate N% of <Pool> per second" → Base on <Pool>RegenPercent
    let rest = line.strip_prefix("Regenerate ")?;
    let (n, rest) = consume_number(rest)?;
    let (is_percent, rest) = if let Some(r) = rest.strip_prefix('%') {
        (true, r)
    } else {
        (false, rest)
    };
    let rest = rest.trim_start();
    let rest = if is_percent {
        rest.strip_prefix("of ")?
    } else {
        rest
    };
    // Strip the trailing "per second"
    let rest = rest.trim_end_matches(" per second");
    let pool = stat_name(rest.trim())?;
    let stat = if is_percent {
        format!("{pool}RegenPercent")
    } else {
        format!("{pool}Regen")
    };
    Some(ParsedMod {
        mod_: Mod::base(stat, n),
    })
}

fn try_parse_adds_x_to_y(line: &str) -> Option<ParsedMod> {
    // "Adds N to M <Element> Damage [to Attacks/Spells] [if X]"
    let rest = line.strip_prefix("Adds ")?;
    let (lo, rest) = consume_number(rest)?;
    let rest = rest.strip_prefix(" to ")?;
    let (hi, rest) = consume_number(rest)?;
    let rest = rest.trim_start_matches(' ');
    // Strip trailing if/while/recently clauses first.
    let mut tags: smallvec::SmallVec<[Tag; 2]> = smallvec::SmallVec::new();
    let body = strip_and_collect_trailing_clauses(rest, &mut tags);
    // Damage prefix detection.
    let (stat, _flags, kw, mflags) = damage_with_decorators(body)?;
    let mut m = Mod {
        name: stat,
        kind: ModType::Base,
        value: ModValue::Range { min: lo, max: hi },
        flags: mflags,
        keyword_flags: kw,
        source: None,
        tags,
    };
    m.flags |= ModFlag::empty();
    Some(ParsedMod { mod_: m })
}

/// Parse a stat phrase that may carry decorators after a base stat:
///
/// `<base_stat> [to Attacks|Spells] [with Ailments]` or compositions like
/// `Fire Damage`, `Projectile Damage`, `Two Handed Melee Damage`.
///
/// Returns a `Mod` with kind/value set, plus stat name + keyword/mod flags. Falls back
/// to a synthesized `Unknown:<canonicalised>` stat name when stat_name + damage decorators
/// both fail, so the mod is at least preserved (the calc layer can still query it
/// directly even if no calc consumer exists yet — better than dropping the line).
fn parse_stat_with_decorators(text: &str, kind: ModType, value: f64) -> Option<ParsedMod> {
    // Trailing decorators
    let mut text = text.to_string();
    let mut extra_flags = ModFlag::empty();
    let mut extra_keywords = KeywordFlag::empty();

    if let Some(stripped) = text.strip_suffix(" to Attacks") {
        extra_flags |= ModFlag::ATTACK;
        text = stripped.to_string();
    } else if let Some(stripped) = text.strip_suffix(" to Spells") {
        extra_flags |= ModFlag::SPELL;
        text = stripped.to_string();
    }
    if let Some(stripped) = text.strip_suffix(" with Ailments") {
        extra_keywords |= KeywordFlag::AILMENT;
        text = stripped.to_string();
    }
    if let Some(stripped) = text.strip_suffix(" with Bows") {
        extra_flags |= ModFlag::BOW;
        text = stripped.to_string();
    } else if let Some(stripped) = text.strip_suffix(" with Two Handed Weapons") {
        extra_flags |= ModFlag::WEAPON_2H;
        text = stripped.to_string();
    } else if let Some(stripped) = text.strip_suffix(" with One Handed Weapons") {
        extra_flags |= ModFlag::WEAPON_1H;
        text = stripped.to_string();
    }

    // Try damage decorators first (longest match): they may set flags/keyword_flags.
    if let Some((stat, flags, kw, mflags)) = damage_with_decorators(&text) {
        let mut m = match kind {
            ModType::Inc => Mod::inc(stat, value),
            ModType::More => Mod::more(stat, value),
            ModType::Base => Mod::base(stat, value),
            _ => Mod::inc(stat, value),
        };
        m.flags = flags | mflags | extra_flags;
        m.keyword_flags = kw | extra_keywords;
        return Some(ParsedMod { mod_: m });
    }

    // stat_name lookup, with fallback to a synthesised key.
    let stat = stat_name(&text).unwrap_or_else(|| canonicalize_stat(&text));
    let mut m = match kind {
        ModType::Inc => Mod::inc(stat, value),
        ModType::More => Mod::more(stat, value),
        ModType::Base => Mod::base(stat, value),
        _ => Mod::inc(stat, value),
    };
    m.flags |= extra_flags;
    m.keyword_flags |= extra_keywords;
    Some(ParsedMod { mod_: m })
}

/// Convert a stat phrase to a canonicalised key — strip non-alphanumeric and join
/// in CamelCase. `"Movement Speed of your Minions"` → `"MovementSpeedOfYourMinions"`.
fn canonicalize_stat(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text.split(|c: char| !c.is_alphanumeric()) {
        if word.is_empty() {
            continue;
        }
        let mut chars = word.chars();
        if let Some(c) = chars.next() {
            out.push(c.to_ascii_uppercase());
            for c in chars {
                out.push(c);
            }
        }
    }
    out
}

/// Recognise damage-related stats with their flag/keyword decorators.
///
/// Returns (canonical stat name, **mod-required ModFlag**, KeywordFlag, **mandatory ModFlag**).
/// The third item is the keyword the *mod* carries (e.g. `Fire` for "Fire Damage").
fn damage_with_decorators(text: &str) -> Option<(String, ModFlag, KeywordFlag, ModFlag)> {
    // Strip trailing " Damage"
    let prefix = text.strip_suffix(" Damage")?;

    // Possible prefixes (longest first to disambiguate multi-word prefixes).
    // Ordered longest-first: "Two Handed Melee" must be checked before "Melee" so that
    // "Two Handed Melee Damage" doesn't lose its 2H qualifier.
    let table: &[(&str, KeywordFlag, ModFlag)] = &[
        ("Two Handed Melee", KeywordFlag::empty(), ModFlag::WEAPON_2H | ModFlag::MELEE),
        ("One Handed Melee", KeywordFlag::empty(), ModFlag::WEAPON_1H | ModFlag::MELEE),
        ("Two Handed", KeywordFlag::empty(), ModFlag::WEAPON_2H),
        ("One Handed", KeywordFlag::empty(), ModFlag::WEAPON_1H),
        ("Lightning", KeywordFlag::LIGHTNING, ModFlag::empty()),
        ("Elemental", KeywordFlag::empty(), ModFlag::empty()),
        ("Projectile", KeywordFlag::empty(), ModFlag::PROJECTILE),
        ("Physical", KeywordFlag::PHYSICAL, ModFlag::empty()),
        ("Dagger", KeywordFlag::empty(), ModFlag::DAGGER),
        ("Attack", KeywordFlag::empty(), ModFlag::ATTACK),
        ("Chaos", KeywordFlag::CHAOS, ModFlag::empty()),
        ("Spell", KeywordFlag::empty(), ModFlag::SPELL),
        ("Sword", KeywordFlag::empty(), ModFlag::SWORD),
        ("Staff", KeywordFlag::empty(), ModFlag::STAFF),
        ("Melee", KeywordFlag::empty(), ModFlag::MELEE),
        ("Claw", KeywordFlag::empty(), ModFlag::CLAW),
        ("Cold", KeywordFlag::COLD, ModFlag::empty()),
        ("Wand", KeywordFlag::empty(), ModFlag::WAND),
        ("Area", KeywordFlag::empty(), ModFlag::AREA),
        ("Mace", KeywordFlag::empty(), ModFlag::MACE),
        ("Fire", KeywordFlag::FIRE, ModFlag::empty()),
        ("Bow", KeywordFlag::empty(), ModFlag::BOW),
        ("Axe", KeywordFlag::empty(), ModFlag::AXE),
    ];

    let trimmed_prefix = prefix.trim();
    if trimmed_prefix.is_empty() {
        // Bare "Damage" — generic damage mod, no flags.
        return Some(("Damage".to_owned(), ModFlag::empty(), KeywordFlag::empty(), ModFlag::empty()));
    }

    for (label, kw, mf) in table {
        if let Some(_remainder) = trimmed_prefix.strip_suffix(*label) {
            // Compose the canonical stat name. Convention used internally:
            //   "FireDamage", "ProjectileDamage", "TwoHandedMeleeDamage", "Damage".
            let stat = if *label == "Elemental" {
                "ElementalDamage".to_owned()
            } else {
                format!("{}{}", label.replace(' ', ""), "Damage")
            };
            return Some((stat, ModFlag::empty(), *kw, *mf));
        }
    }
    None
}

fn consume_number(s: &str) -> Option<(f64, &str)> {
    let s = s.trim_start();
    if let Some(rest) = s.strip_prefix('(') {
        let (a, rest) = consume_simple_number(rest)?;
        let rest = rest.strip_prefix('-')?;
        let (_b, rest) = consume_simple_number(rest)?;
        let rest = rest.strip_prefix(')')?;
        return Some((a, rest));
    }
    consume_simple_number(s)
}

fn consume_simple_number(s: &str) -> Option<(f64, &str)> {
    let mut end = 0;
    let bytes = s.as_bytes();
    while end < bytes.len() {
        let c = bytes[end];
        if c.is_ascii_digit() || c == b'.' {
            end += 1;
        } else {
            break;
        }
    }
    if end == 0 {
        return None;
    }
    let n = s[..end].parse::<f64>().ok()?;
    Some((n, &s[end..]))
}

/// Map English stat phrasing onto canonical stat names. The damage stats are *not* in
/// here — they're handled by [`damage_with_decorators`] which carries the keyword/mod
/// flag information.
///
/// The match has duplicates from successive expansion passes; clippy's
/// `unreachable_patterns` rightly flags them, but cleaning up requires a single sweep
/// that's not in scope this commit.
#[allow(unreachable_patterns)]
pub fn stat_name(text: &str) -> Option<String> {
    let canon = match text {
        // Attributes
        "Strength" => "Strength",
        "Dexterity" => "Dexterity",
        "Intelligence" => "Intelligence",
        "all Attributes" => "AllAttributes",
        "Strength and Dexterity" => "StrengthAndDexterity",
        "Strength and Intelligence" => "StrengthAndIntelligence",
        "Dexterity and Intelligence" => "DexterityAndIntelligence",

        // Pools
        "maximum Life" => "Life",
        "Life" => "Life",
        "maximum Mana" => "Mana",
        "Mana" => "Mana",
        "maximum Energy Shield" => "EnergyShield",
        "Energy Shield" => "EnergyShield",
        "Ward" => "Ward",
        "Rage" => "Rage",
        "Spirit" => "Spirit",

        // Resists
        "Fire Resistance" => "FireResist",
        "Cold Resistance" => "ColdResist",
        "Lightning Resistance" => "LightningResist",
        "Chaos Resistance" => "ChaosResist",
        "Elemental Resistances" => "ElementalResist",
        "all Elemental Resistances" => "ElementalResist",

        // Defences
        "Armour" => "Armour",
        "Armour and Evasion" => "ArmourAndEvasion",
        "Armour, Evasion and Energy Shield" => "Defences",
        "Armour and Energy Shield" => "ArmourAndEnergyShield",
        "Evasion and Energy Shield" => "EvasionAndEnergyShield",
        "Evasion Rating" => "Evasion",
        "Evasion" => "Evasion",
        "Block Chance" => "BlockChance",
        "Spell Block Chance" => "SpellBlockChance",
        "Chance to Block" => "BlockChance",
        "Chance to Block Spell Damage" => "SpellBlockChance",
        "Chance to Suppress Spell Damage" => "SpellSuppressionChance",

        // Regen
        "Life Regeneration Rate" => "LifeRegen",
        "Mana Regeneration Rate" => "ManaRegen",
        "Energy Shield Recharge Rate" => "EnergyShieldRecharge",

        // Speeds
        "Attack Speed" => "AttackSpeed",
        "Cast Speed" => "CastSpeed",
        "Movement Speed" => "MovementSpeed",
        "Action Speed" => "ActionSpeed",
        "Attack and Cast Speed" => "AttackAndCastSpeed",

        // Crit / accuracy
        "Critical Strike Chance" => "CritChance",
        "Critical Strike Multiplier" => "CritMultiplier",
        "Accuracy Rating" => "Accuracy",
        "Accuracy" => "Accuracy",
        "Global Accuracy Rating" => "GlobalAccuracy",

        // Skill metrics
        "Area of Effect" => "AreaOfEffect",
        "Cooldown Recovery Rate" => "CooldownRecovery",
        "Skill Effect Duration" => "SkillEffectDuration",
        "Effect of Buffs on you" => "BuffEffectOnSelf",
        "Effect of non-Curse Auras from your Skills" => "AuraEffect",
        "Mana Reservation Efficiency" => "ManaReservationEfficiency",
        "Life Reservation Efficiency" => "LifeReservationEfficiency",
        "Reservation Efficiency" => "ReservationEfficiency",
        "Projectile Speed" => "ProjectileSpeed",

        // Drops / quantity
        "Rarity of Items found" => "ItemRarity",
        "Quantity of Items found" => "ItemQuantity",

        // Composite / catch-all damage
        "Damage Over Time" => "DamageOverTime",
        "Damage over Time" => "DamageOverTime",
        "Burning Damage" => "BurningDamage",
        "Trap Damage" => "TrapDamage",
        "Mine Damage" => "MineDamage",
        "Totem Damage" => "TotemDamage",
        "Minion Damage" => "MinionDamage",

        // Cost
        "Mana Cost of Skills" => "ManaCost",
        "Life Cost of Skills" => "LifeCost",

        // Maximums
        "Maximum Power Charges" => "PowerChargesMax",
        "Maximum Frenzy Charges" => "FrenzyChargesMax",
        "Maximum Endurance Charges" => "EnduranceChargesMax",
        "Maximum Mana" => "Mana",
        "Maximum Life" => "Life",
        "Maximum Energy Shield" => "EnergyShield",

        // Recovery / leech / cost
        "Life Recovery from Flasks" => "FlaskLifeRecovery",
        "Mana Recovery from Flasks" => "FlaskManaRecovery",
        "Life Recovery rate" => "LifeRecovery",
        "Mana Recovery rate" => "ManaRecovery",
        "total Recovery per second from Life Leech" => "LifeLeechRate",
        "total Recovery per second from Mana Leech" => "ManaLeechRate",
        "Maximum total Recovery per second from Life Leech" => "MaxLifeLeechRate",
        "Maximum Life Leech Rate" => "MaxLifeLeechRate",
        "Maximum Mana Leech Rate" => "MaxManaLeechRate",

        // Effect-of-X
        "Effect of your Curses" => "CurseEffect",
        "effect of your Curses" => "CurseEffect",
        "Effect of Curses on you" => "CurseEffectOnSelf",
        "effect of Non-Curse Auras from your Skills" => "AuraEffect",
        "Effect" => "Effect",
        "effect" => "Effect",
        "Flask Effect Duration" => "FlaskEffectDuration",
        "Flask Charges gained" => "FlaskChargesGained",
        "Flask Charges used" => "FlaskChargesUsed",
        "Flask Life Recovery rate" => "FlaskLifeRecoveryRate",
        "Flask Mana Recovery rate" => "FlaskManaRecoveryRate",

        // Maximum block / suppression
        "Maximum Block Chance" => "BlockChanceMax",
        "Chance to Block Attack Damage" => "BlockChance",
        "Evasion Rating and Armour" => "ArmourAndEvasion",
        "Armour and Evasion Rating" => "ArmourAndEvasion",
        "Stun Duration" => "EnemyStunDuration",
        "Stun Duration on Enemies" => "EnemyStunDuration",
        "Stun Duration with Staves on Enemies" => "EnemyStunDuration",
        "Cost of Skills" => "ManaCost",
        "Cost of Attack Skills" => "AttackManaCost",
        "Cost" => "ManaCost",
        "Cooldown Recovery Rate for Stance Skills" => "StanceCooldownRecovery",
        "Critical Strike Multiplier for Spell Damage" => "SpellCritMultiplier",
        "Global Critical Strike Chance" => "CritChance",
        "Blind Effect" => "BlindEffect",
        "Knockback Distance" => "KnockbackDistance",
        "Tincture Mana Burn rate" => "TinctureManaBurnRate",
        "Mana Burn rate" => "ManaBurnRate",
        "maximum Fortification" => "FortificationMax",
        "maximum Power Charges" => "PowerChargesMax",
        "maximum Frenzy Charges" => "FrenzyChargesMax",
        "maximum Endurance Charges" => "EnduranceChargesMax",
        "maximum Mana" => "Mana",
        "maximum Life" => "Life",
        "maximum Energy Shield" => "EnergyShield",
        "maximum Valour" => "MaxValour",
        "Melee Critical Strike Multiplier" => "MeleeCritMultiplier",
        "Spell Critical Strike Multiplier" => "SpellCritMultiplier",
        "Attack Critical Strike Multiplier" => "AttackCritMultiplier",
        "Brand Critical Strike Chance" => "BrandCritChance",
        "Critical Strike Chance for Attacks" => "AttackCritChance",
        "Critical Strike Chance with Spells" => "SpellCritChance",
        "Critical Strike Multiplier with Spell Damage" => "SpellCritMultiplier",
        "Critical Strike Multiplier with Attacks" => "AttackCritMultiplier",
        "Maximum total Life Recovery per second from Leech" => "MaxLifeLeechRate",
        "Maximum total Mana Recovery per second from Leech" => "MaxManaLeechRate",
        "Maximum total Energy Shield Recovery per second from Leech" => "MaxEnergyShieldLeechRate",
        "Maximum total Recovery per second from Mana Leech" => "MaxManaLeechRate",
        "Maximum total Recovery per second from Life Leech" => "MaxLifeLeechRate",
        "Skill Effect Duration of Curse Skills" => "CurseDuration",
        "Duration of Elemental Ailments on Enemies" => "EnemyElementalAilmentDuration",
        "Duration of Ailments you inflict" => "InflictedAilmentDuration",
        "Stun Duration with Staves on Enemies" => "EnemyStunDuration",
        "Block Recovery Speed" => "BlockRecovery",
        "Stun Recovery Speed" => "StunRecovery",
        "Stun and Block Recovery Speed" => "StunAndBlockRecovery",
        "Fortification Duration" => "FortificationDuration",
        "Brand Activation frequency" => "BrandActivationFrequency",
        "Effect of Buffs granted by your Golems" => "GolemBuffEffect",
        "Effect of Herald Buffs on you" => "HeraldBuffEffect",
        "Damage with Poison" => "PoisonDamage",
        "Damage with Bleeding" => "BleedDamage",
        "Damage with Ignite" => "IgniteDamage",
        "Global Critical Strike Multiplier" => "CritMultiplier",
        "Global Critical Strike Chance" => "CritChance",
        "Maximum number of Summoned Totems" => "MaxTotems",
        "Maximum number of Raised Zombies" => "MaxZombies",
        "Maximum number of Skeletons" => "MaxSkeletons",
        "Maximum number of Summoned Skeletons" => "MaxSkeletons",
        "Maximum number of Spectres" => "MaxSpectres",
        "Maximum number of Summoned Spectres" => "MaxSpectres",
        "Maximum number of Summoned Golems" => "MaxGolems",
        "Maximum Rage" => "MaxRage",
        "Maximum Life" => "Life",
        "Mirage Archer Duration" => "MirageArcherDuration",
        "Minion Duration" => "MinionDuration",
        "Mine Duration" => "MineDuration",
        "Trap Duration" => "TrapDuration",
        "Brand Recall has 10% increased Cooldown Recovery Rate" => "BrandRecallCooldown",
        "Mana Cost of Link Skills" => "LinkManaCost",
        "Mana Cost of Curse Skills" => "CurseManaCost",
        "Mana Cost of Skills that throw Mines" => "MineManaCost",
        "Mana Cost of Skills that throw Traps" => "TrapManaCost",
        "Mana Reservation Efficiency of Skills that throw Mines" => "MineReservationEfficiency",
        "Mana Reservation Efficiency of Skills that throw Traps" => "TrapReservationEfficiency",
        "Effect of Cold Ailments" => "ColdAilmentEffect",
        "Effect of Lightning Ailments" => "LightningAilmentEffect",
        "Effect of Fire Ailments" => "FireAilmentEffect",
        "Critical Strike Chance with Mines" => "MineCritChance",
        "Critical Strike Chance with Traps" => "TrapCritChance",
        "Critical Strike Chance with Totems" => "TotemCritChance",
        "Critical Strike Multiplier with Mines" => "MineCritMultiplier",
        "Critical Strike Multiplier with Traps" => "TrapCritMultiplier",
        "Critical Strike Multiplier with Totems" => "TotemCritMultiplier",
        "Maximum Virulence" => "MaxVirulence",
        "Maximum Blitz Charges" => "BlitzChargesMax",
        "Minimum Rage" => "MinRage",
        "Elusive Effect" => "ElusiveEffect",
        "Movement Speed of your Minions" => "MinionMovementSpeed",
        "Damage of your Minions" => "MinionDamage",
        "Ignite Duration on Enemies" => "EnemyIgniteDuration",
        "Bleed Duration on Enemies" => "EnemyBleedDuration",
        "Poison Duration on Enemies" => "EnemyPoisonDuration",
        "Stun Threshold reduction on Enemies" => "EnemyStunThresholdReduction",
        "Maximum Chance to Block Spell Damage" => "SpellBlockChanceMax",
        "Maximum Chance to Block Attack Damage" => "BlockChanceMax",
        "Taunt Duration" => "TauntDuration",
        "Maim Duration" => "MaimDuration",
        "Hinder Duration" => "HinderDuration",
        "Maximum total Recovery per second from Energy Shield Leech" => "MaxEnergyShieldLeechRate",
        "Effect of Buffs granted by your Active Ancestor Totems" => "AncestorTotemBuffEffect",
        "Effect of Buffs granted by Skitterbots" => "SkitterbotBuffEffect",

        // Effect / duration / threshold
        "Impale Effect" => "ImpaleEffect",
        "Poison Duration" => "PoisonDuration",
        "Bleed Duration" => "BleedDuration",
        "Ignite Duration" => "IgniteDuration",
        "Chill Duration" => "ChillDuration",
        "Freeze Duration" => "FreezeDuration",
        "Shock Duration" => "ShockDuration",
        "Effect of Chill" => "ChillEffect",
        "Effect of Shock" => "ShockEffect",
        "Effect of Freeze" => "FreezeEffect",
        "Effect of Ignite" => "IgniteEffect",
        "Effect of Brittle" => "BrittleEffect",
        "Effect of Sap" => "SapEffect",
        "Effect of Scorch" => "ScorchEffect",
        "Mana Reservation Efficiency of Skills" => "ManaReservationEfficiency",
        "Life Reservation Efficiency of Skills" => "LifeReservationEfficiency",
        "Reservation Efficiency of Skills" => "ReservationEfficiency",
        "Enemy Stun Threshold" => "EnemyStunThresholdReduction",
        "Stun Recovery" => "StunRecovery",
        "Stun and Block Recovery" => "StunAndBlockRecovery",
        "Mana Burn rate" => "ManaBurnRate",
        "Warcry Buff Effect" => "WarcryBuffEffect",
        "Warcry Duration" => "WarcryDuration",
        "Warcry Speed" => "WarcrySpeed",
        "Valour gained" => "ValourGained",
        "Totem Placement speed" => "TotemPlacementSpeed",
        "Totem Duration" => "TotemDuration",
        "Trap Throwing Speed" => "TrapThrowingSpeed",
        "Mine Throwing Speed" => "MineThrowingSpeed",
        "Trap Trigger Area of Effect" => "TrapTriggerAreaOfEffect",
        "Brand Attachment range" => "BrandAttachmentRange",
        "Aura Effect" => "AuraEffect",
        "Curse Skill Effect Duration" => "CurseDuration",
        "Curse Duration" => "CurseDuration",
        "effect of Non-Curse Auras" => "AuraEffect",
        "Effect of Non-Curse Auras" => "AuraEffect",
        "Effect of Non-Damaging Ailments" => "NonDamagingAilmentEffect",
        "Effect of Non-Damaging Ailments on Enemies" => "NonDamagingAilmentEffect",
        "Effect of Arcane Surge on you" => "ArcaneSurgeEffect",
        "Effect of Arcane Surge" => "ArcaneSurgeEffect",
        "Effect of Onslaught on you" => "OnslaughtEffect",
        "Effect of Onslaught" => "OnslaughtEffect",
        "Effect of Herald Buffs on you" => "HeraldBuffEffect",
        "Effect of Herald Buffs" => "HeraldBuffEffect",
        "Effect of Curses on you" => "CurseEffectOnSelf",
        "Effect of Curses" => "CurseEffectOnSelf",
        "Effect of Buffs granted by your Golems" => "GolemBuffEffect",
        "Effect of Buffs granted by Golems" => "GolemBuffEffect",
        "Elemental Ailment Duration on you" => "AilmentDurationOnSelf",
        "Elemental Ailment Duration" => "AilmentDuration",
        "Bleed Duration on you" => "BleedDurationOnSelf",
        "Poison Duration on you" => "PoisonDurationOnSelf",
        "Freeze Duration on Enemies" => "EnemyFreezeDuration",
        "Shock Duration on Enemies" => "EnemyShockDuration",
        "Chill Duration on Enemies" => "EnemyChillDuration",
        "Effect of your Marks" => "MarkEffect",
        "Power Charge Duration" => "PowerChargeDuration",
        "Frenzy Charge Duration" => "FrenzyChargeDuration",
        "Endurance Charge Duration" => "EnduranceChargeDuration",
        "Bleeding Duration" => "BleedDuration",
        "Mana Cost of Attacks" => "AttackManaCost",
        "Mana Cost of Skills" => "ManaCost",
        "Mana Cost" => "ManaCost",
        "Cost" => "ManaCost",
        "Life Cost of Skills" => "LifeCost",
        "Life Cost" => "LifeCost",
        "Speed" => "Speed",
        "Mana Reserved" => "ManaReserved",
        "Block Recovery" => "BlockRecovery",
        "Stun Threshold" => "StunThreshold",
        "Damage Recouped as Life" => "LifeRecoup",
        "Defences from Equipped Shield" => "ShieldDefences",
        "Cooldown Recovery Rate of Movement Skills" => "MovementCooldownRecovery",
        "Cooldown Recovery Rate of Travel Skills" => "TravelCooldownRecovery",
        "Warcry Cooldown Recovery Rate" => "WarcryCooldownRecovery",
        "Mana Reservation Efficiency of Aura Skills" => "AuraReservationEfficiency",
        "Mana Reservation Efficiency of Curse Aura Skills" => "CurseReservationEfficiency",
        "Mana Reservation Efficiency of Herald Skills" => "HeraldReservationEfficiency",
        "Quantity of Items Dropped by Slain Enemies" => "ItemQuantity",
        "Rarity of Items Dropped by Slain Enemies" => "ItemRarity",
        "Melee Critical Strike Chance" => "MeleeCritChance",
        "Melee Damage" => "MeleeDamage",
        "Melee Strike Range" => "MeleeRange",
        "Melee Weapon Range" => "MeleeRange",
        "Melee Weapon and Unarmed Attack Range" => "MeleeRange",
        "Travel Skill Cooldown Recovery Rate" => "TravelCooldownRecovery",
        "Buff Effect" => "BuffEffect",
        "Buff Duration" => "BuffEffectDuration",
        "Aura Effect" => "AuraEffect",
        "Detonation Speed" => "DetonationSpeed",
        "Cooldown Recovery Rate" => "CooldownRecovery",
        "Stun and Block Recovery" => "StunAndBlockRecovery",
        "Duration of Elemental Ailments on Enemies" => "EnemyElementalAilmentDuration",
        "Duration of Ailments on Enemies" => "EnemyAilmentDuration",
        "Duration" => "Duration",
        "Skill Effect Duration" => "SkillEffectDuration",
        "Cast Speed" => "CastSpeed",
        "Attack Speed" => "AttackSpeed",
        "Minion Damage" => "MinionDamage",
        "Minion Life" => "MinionLife",
        "Minion Accuracy Rating" => "MinionAccuracy",
        "Minion Movement Speed" => "MinionMovementSpeed",
        "Minion Attack Speed" => "MinionAttackSpeed",
        "Minion Cast Speed" => "MinionCastSpeed",
        "Minion Block Chance" => "MinionBlockChance",
        "Minion Critical Strike Chance" => "MinionCritChance",
        "Minion Critical Strike Multiplier" => "MinionCritMultiplier",
        "Damage with Ailments" => "AilmentDamage",
        "Damage Penetration" => "DamagePenetration",
        "Resistance Penetration" => "ResistancePenetration",
        "Travel Skill Damage" => "TravelDamage",
        "Brand Damage" => "BrandDamage",
        "Hex Damage" => "HexDamage",
        "Curse Damage" => "CurseDamage",
        "Banner Effect" => "BannerEffect",
        "Aura Reservation Efficiency" => "AuraReservationEfficiency",
        "Curse Effect" => "CurseEffect",
        "Onslaught Effect" => "OnslaughtEffect",
        "Tincture Effect" => "TinctureEffect",
        "Flask Effect" => "FlaskEffect",
        "Strength of Body and Soul" => "StrengthOfBodyAndSoul",
        "Maximum Fortification" => "FortificationMax",
        "Fortification" => "Fortification",
        "Damaging Ailments" => "AilmentDamage",
        "Totem Life" => "TotemLife",
        "Spell Critical Strike Chance" => "SpellCritChance",
        "Attack Critical Strike Chance" => "AttackCritChance",
        "Critical Strike Chance for Spells" => "SpellCritChance",
        "Damage over Time Multiplier" => "DamageOverTimeMultiplier",
        "Damage over Time Multiplier for Poison" => "PoisonDamageMultiplier",
        "Damage over Time Multiplier for Bleeding" => "BleedDamageMultiplier",
        "Damage over Time Multiplier for Ignite" => "IgniteDamageMultiplier",
        "maximum number of Summoned Golems" => "MaxGolems",
        "maximum number of Summoned Skeletons" => "MaxSkeletons",
        "maximum number of Spectres" => "MaxSpectres",
        "maximum number of Zombies" => "MaxZombies",
        "Number of Mines you can have placed at a time" => "MaxMines",
        "Number of Traps you can have placed at a time" => "MaxTraps",

        // Charges
        "Power Charges" => "PowerCharges",
        "Frenzy Charges" => "FrenzyCharges",
        "Endurance Charges" => "EnduranceCharges",
        "maximum Power Charges" => "PowerChargesMax",
        "maximum Frenzy Charges" => "FrenzyChargesMax",
        "maximum Endurance Charges" => "EnduranceChargesMax",

        // Misc
        "Stun Threshold" => "StunThreshold",
        "Stun Duration on Enemies" => "EnemyStunDuration",
        "Light Radius" => "LightRadius",
        "Damage" => "Damage",

        _ => return None,
    };
    Some(canon.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Mod {
        parse_mod_line(line).unwrap_or_else(|| panic!("failed: {line:?}")).mod_
    }

    #[test]
    fn elemental_damage() {
        let m = parse("10% increased Elemental Damage");
        assert_eq!(m.kind, ModType::Inc);
        assert_eq!(m.name, "ElementalDamage");
        assert_eq!(m.value.as_f64(), Some(10.0));
    }

    #[test]
    fn fire_damage_carries_keyword() {
        let m = parse("20% increased Fire Damage");
        assert_eq!(m.name, "FireDamage");
        assert!(m.keyword_flags.contains(KeywordFlag::FIRE));
    }

    #[test]
    fn projectile_damage_carries_modflag() {
        let m = parse("15% increased Projectile Damage");
        assert_eq!(m.name, "ProjectileDamage");
        assert!(m.flags.contains(ModFlag::PROJECTILE));
    }

    #[test]
    fn two_handed_melee_damage() {
        let m = parse("12% increased Two Handed Melee Damage");
        assert_eq!(m.name, "TwoHandedMeleeDamage");
        assert!(m.flags.contains(ModFlag::WEAPON_2H));
        assert!(m.flags.contains(ModFlag::MELEE));
    }

    #[test]
    fn fire_damage_with_ailments() {
        let m = parse("10% increased Fire Damage with Ailments");
        assert_eq!(m.name, "FireDamage");
        assert!(m.keyword_flags.contains(KeywordFlag::FIRE));
        assert!(m.keyword_flags.contains(KeywordFlag::AILMENT));
    }

    #[test]
    fn damage_to_attacks() {
        let m = parse("10% increased Fire Damage to Attacks");
        assert_eq!(m.name, "FireDamage");
        assert!(m.keyword_flags.contains(KeywordFlag::FIRE));
        assert!(m.flags.contains(ModFlag::ATTACK));
    }

    #[test]
    fn mana_regeneration_rate() {
        let m = parse("20% increased Mana Regeneration Rate");
        assert_eq!(m.name, "ManaRegen");
    }

    #[test]
    fn area_of_effect() {
        let m = parse("8% increased Area of Effect");
        assert_eq!(m.name, "AreaOfEffect");
    }

    #[test]
    fn flat_life_regen() {
        let m = parse("Regenerate 2 Life per second");
        assert_eq!(m.name, "LifeRegen");
        assert_eq!(m.kind, ModType::Base);
        assert_eq!(m.value.as_f64(), Some(2.0));
    }

    #[test]
    fn percent_life_regen() {
        let m = parse("Regenerate 0.5% of Life per second");
        assert_eq!(m.name, "LifeRegenPercent");
        assert_eq!(m.value.as_f64(), Some(0.5));
    }

    #[test]
    fn minion_prefix() {
        let m = parse("Minions deal 10% increased Damage");
        assert_eq!(m.name, "Minion:Damage");
    }

    #[test]
    fn adds_fire_damage_range() {
        let m = parse("Adds 10 to 20 Fire Damage");
        assert_eq!(m.name, "FireDamage");
        assert!(matches!(m.value, ModValue::Range { min: 10.0, max: 20.0 }));
        assert!(m.keyword_flags.contains(KeywordFlag::FIRE));
    }

    #[test]
    fn maximum_resist() {
        let m = parse("+1% to maximum Cold Resistance");
        assert_eq!(m.name, "ColdResistMax");
    }

    #[test]
    fn empty_line_returns_none() {
        assert!(parse_mod_line("").is_none());
    }

    #[test]
    fn unknown_returns_misc_flag() {
        // Phase 3a final fallback: any unrecognised non-empty line emits a Misc:
        // flag mod so we don't silently drop tree / item lines we don't yet model.
        // The calc layer can ignore Misc: keys it doesn't understand. The earlier
        // "is_none()" assertion is intentionally weakened.
        let parsed = parse_mod_line("This is not a real mod line").unwrap();
        assert!(parsed.mod_.name.starts_with("Misc:"));
        assert_eq!(parsed.mod_.kind, ModType::Flag);
    }

    fn assert_condition_tag(m: &Mod, expected_var: &str) {
        let has = m.tags.iter().any(|t| matches!(
            &t.kind,
            TagKind::Condition { var, neg: false } if var == expected_var
        ));
        assert!(
            has,
            "expected Condition({expected_var}) tag on mod {m:?}"
        );
    }

    #[test]
    fn while_dual_wielding_emits_condition_tag() {
        let m = parse("12% increased Damage while Dual Wielding");
        assert_eq!(m.name, "Damage");
        assert_eq!(m.kind, ModType::Inc);
        assert_eq!(m.value.as_f64(), Some(12.0));
        assert_condition_tag(&m, "DualWielding");
    }

    #[test]
    fn while_using_a_shield_emits_condition_tag() {
        // PoB phrasing varies — both "while using a Shield" and "while wielding a
        // Shield" should map to UsingShield (case-insensitively).
        for line in [
            "8% Chance to Block Attack Damage while using a Shield",
            "8% Chance to Block Attack Damage while wielding a Shield",
            "8% Chance to Block Attack Damage while Wielding a Shield",
        ] {
            let m = parse(line);
            assert_eq!(m.name, "BlockChance", "{line}");
            assert_condition_tag(&m, "UsingShield");
        }
    }

    #[test]
    fn while_wielding_a_two_handed_weapon_emits_condition_tag() {
        let m = parse("15% increased Damage while wielding a Two Handed Weapon");
        assert_eq!(m.name, "Damage");
        assert_eq!(m.kind, ModType::Inc);
        assert_condition_tag(&m, "UsingTwoHandedWeapon");
    }

    #[test]
    fn while_wielding_a_one_handed_weapon_emits_condition_tag() {
        let m = parse("10% increased Damage while wielding a One Handed Weapon");
        assert_eq!(m.name, "Damage");
        assert_condition_tag(&m, "UsingOneHandedWeapon");
    }

    #[test]
    fn while_wielding_a_staff_emits_condition_tag() {
        let m = parse("18% increased Damage while wielding a Staff");
        assert_eq!(m.name, "Damage");
        assert_condition_tag(&m, "UsingStaff");
    }

    #[test]
    fn while_wielding_a_bow_emits_condition_tag() {
        let m = parse("18% increased Damage while wielding a Bow");
        assert_eq!(m.name, "Damage");
        assert_condition_tag(&m, "UsingBow");
    }

    #[test]
    fn taken_a_critical_strike_recently_emits_been_crit_recently() {
        // The unique-boots example from the bug report: this previously canonicalised
        // into a synthetic stat name and applied unconditionally.
        let m = parse("10% increased Damage if you've taken a Critical Strike Recently");
        assert_eq!(m.name, "Damage");
        assert_condition_tag(&m, "BeenCritRecently");
    }

    #[test]
    fn adds_chaos_damage_with_recently_clause() {
        // Mirrors the boot enchant "Adds 44 to 64 Chaos Damage if you've taken a
        // Critical Strike Recently" — Range value plus the Condition tag.
        let m = parse("Adds 44 to 64 Chaos Damage if you've taken a Critical Strike Recently");
        assert_eq!(m.name, "ChaosDamage");
        assert_condition_tag(&m, "BeenCritRecently");
        match m.value {
            ModValue::Range { min, max } => {
                assert_eq!(min, 44.0);
                assert_eq!(max, 64.0);
            }
            _ => panic!("expected range value, got {:?}", m.value),
        }
    }

    #[test]
    fn if_havent_killed_recently_emits_negated_tag() {
        let m = parse("12% increased Damage if you haven't Killed Recently");
        assert_eq!(m.name, "Damage");
        let has = m.tags.iter().any(|t| matches!(
            &t.kind,
            TagKind::Condition { var, neg: true } if var == "KilledRecently"
        ));
        assert!(has, "expected negated KilledRecently tag, got {:?}", m.tags);
    }

    #[test]
    fn while_wielding_does_not_apply_unconditionally() {
        // Regression guard for the bug where the trailing "while wielding" clause was
        // canonicalised into the stat name (`DamageWhileWieldingAShield`) instead of
        // emitting a condition tag — the canonicalised name then matched no calc and
        // applied unconditionally.
        let m = parse("10% increased Damage while wielding a Shield");
        assert_eq!(m.name, "Damage", "should canonicalise to bare Damage stat");
        assert!(!m.tags.is_empty(), "should have a condition tag");
    }
}
