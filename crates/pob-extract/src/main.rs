//! Reads PoB's `src/Data/` and `src/TreeData/` Lua tables, emits JSON files under the
//! workspace `data/` directory that the runtime crates can deserialise.
//!
//! Run from the workspace root:
//!
//! ```text
//! cargo run -p pob-extract -- --pob ../PathOfBuilding --out data
//! ```
//!
//! Defaults: `--pob ../PathOfBuilding`, `--out data`. The input directory must be a
//! checkout of PathOfBuildingCommunity/PathOfBuilding (or a fork).

#![allow(dead_code)] // many helpers are convenience that submodules grow into

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use mlua::{Lua, Table, Value};

mod calc_sections;
mod cli;
mod cluster_jewel_mods;
mod cluster_jewels;
mod enchants;
mod minions;
mod tattoos;
mod tree;

fn main() -> Result<()> {
    let args = cli::Args::parse();
    println!(
        "pob-extract: pob={} out={}",
        args.pob.display(),
        args.out.display()
    );
    std::fs::create_dir_all(&args.out)
        .with_context(|| format!("creating output dir {}", args.out.display()))?;

    let mut wrote = Vec::new();

    // Bases — many small files merged into one.
    let bases_path = args.out.join("bases.json");
    let bases =
        bases::extract_all(&args.pob).with_context(|| "extracting item bases".to_string())?;
    let json = serde_json::to_string_pretty(&bases)?;
    std::fs::write(&bases_path, json)
        .with_context(|| format!("writing {}", bases_path.display()))?;
    wrote.push(bases_path);

    // Gems — one file.
    let gems_path = args.out.join("gems.json");
    let gems = gems::extract(&args.pob)?;
    let json = serde_json::to_string_pretty(&gems)?;
    std::fs::write(&gems_path, json).with_context(|| format!("writing {}", gems_path.display()))?;
    wrote.push(gems_path);

    // Skills — one JSON per skill type, plus a combined index.
    let skills_dir = args.out.join("skills");
    std::fs::create_dir_all(&skills_dir)?;
    let skill_index = skills::extract_all(&args.pob, &skills_dir)
        .with_context(|| "extracting skills".to_string())?;
    let index_path = skills_dir.join("index.json");
    std::fs::write(&index_path, serde_json::to_string_pretty(&skill_index)?)?;
    wrote.push(index_path);

    // Cluster jewels — one JSON for cluster-jewel sub-graph synthesis.
    let cj_path = args.out.join("cluster_jewels.json");
    let cj = cluster_jewels::extract(&args.pob)
        .with_context(|| "extracting cluster jewels".to_string())?;
    std::fs::write(&cj_path, serde_json::to_string_pretty(&cj)?)
        .with_context(|| format!("writing {}", cj_path.display()))?;
    wrote.push(cj_path);

    // Cluster jewel mods — notable + corrupted mods that roll on cluster jewels.
    let cjm_path = args.out.join("cluster_jewel_mods.json");
    let cjm = cluster_jewel_mods::extract(&args.pob)
        .with_context(|| "extracting cluster jewel mods".to_string())?;
    std::fs::write(&cjm_path, serde_json::to_string_pretty(&cjm)?)
        .with_context(|| format!("writing {}", cjm_path.display()))?;
    wrote.push(cjm_path);

    // Minions — one JSON for the parallel minion calc env.
    let minions_path = args.out.join("minions.json");
    let minions = minions::extract(&args.pob).with_context(|| "extracting minions".to_string())?;
    std::fs::write(&minions_path, serde_json::to_string_pretty(&minions)?)
        .with_context(|| format!("writing {}", minions_path.display()))?;
    wrote.push(minions_path);

    // Tattoos — one JSON for the Tree-tab tattoo picker.
    let tattoos_path = args.out.join("tattoos.json");
    let tattoos = tattoos::extract(&args.pob).with_context(|| "extracting tattoos".to_string())?;
    std::fs::write(&tattoos_path, serde_json::to_string_pretty(&tattoos)?)
        .with_context(|| format!("writing {}", tattoos_path.display()))?;
    wrote.push(tattoos_path);

    // Helmet enchants — one JSON for the Items-tab "Apply
    // Enchantment" picker. Issue #221 follow-up: the UI dialog
    // reads this catalogue.
    let helmet_enchants_path = args.out.join("enchants_helmet.json");
    let helmet_enchants =
        enchants::extract(&args.pob).with_context(|| "extracting helmet enchants".to_string())?;
    std::fs::write(
        &helmet_enchants_path,
        serde_json::to_string_pretty(&helmet_enchants)?,
    )
    .with_context(|| format!("writing {}", helmet_enchants_path.display()))?;
    wrote.push(helmet_enchants_path);

    // Glove enchants — flat-tier catalogue. Companion to helmet
    // enchants for the issue #221 picker; the UI follow-up surfaces
    // glove + boot picks on those equipped slots.
    let glove_enchants_path = args.out.join("enchants_gloves.json");
    let glove_enchants = enchants::extract_gloves(&args.pob)
        .with_context(|| "extracting glove enchants".to_string())?;
    std::fs::write(
        &glove_enchants_path,
        serde_json::to_string_pretty(&glove_enchants)?,
    )
    .with_context(|| format!("writing {}", glove_enchants_path.display()))?;
    wrote.push(glove_enchants_path);

    // Boot enchants — same flat-tier shape as gloves.
    let boot_enchants_path = args.out.join("enchants_boots.json");
    let boot_enchants = enchants::extract_boots(&args.pob)
        .with_context(|| "extracting boot enchants".to_string())?;
    std::fs::write(
        &boot_enchants_path,
        serde_json::to_string_pretty(&boot_enchants)?,
    )
    .with_context(|| format!("writing {}", boot_enchants_path.display()))?;
    wrote.push(boot_enchants_path);

    // Body armour / belt / weapon / flask enchants — same flat
    // shape as gloves + boots; tier names vary per slot
    // (HARVEST / DEDICATION / ENKINDLING etc).
    let body_enchants_path = args.out.join("enchants_body.json");
    let body_enchants = enchants::extract_body(&args.pob)
        .with_context(|| "extracting body enchants".to_string())?;
    std::fs::write(
        &body_enchants_path,
        serde_json::to_string_pretty(&body_enchants)?,
    )
    .with_context(|| format!("writing {}", body_enchants_path.display()))?;
    wrote.push(body_enchants_path);

    let belt_enchants_path = args.out.join("enchants_belt.json");
    let belt_enchants = enchants::extract_belt(&args.pob)
        .with_context(|| "extracting belt enchants".to_string())?;
    std::fs::write(
        &belt_enchants_path,
        serde_json::to_string_pretty(&belt_enchants)?,
    )
    .with_context(|| format!("writing {}", belt_enchants_path.display()))?;
    wrote.push(belt_enchants_path);

    let weapon_enchants_path = args.out.join("enchants_weapon.json");
    let weapon_enchants = enchants::extract_weapon(&args.pob)
        .with_context(|| "extracting weapon enchants".to_string())?;
    std::fs::write(
        &weapon_enchants_path,
        serde_json::to_string_pretty(&weapon_enchants)?,
    )
    .with_context(|| format!("writing {}", weapon_enchants_path.display()))?;
    wrote.push(weapon_enchants_path);

    let flask_enchants_path = args.out.join("enchants_flask.json");
    let flask_enchants = enchants::extract_flask(&args.pob)
        .with_context(|| "extracting flask enchants".to_string())?;
    std::fs::write(
        &flask_enchants_path,
        serde_json::to_string_pretty(&flask_enchants)?,
    )
    .with_context(|| format!("writing {}", flask_enchants_path.display()))?;
    wrote.push(flask_enchants_path);

    // Calc sections — one JSON for the Calcs-tab section layout.
    let calc_sections_path = args.out.join("calc_sections.json");
    let calc_sections = calc_sections::extract(&args.pob)
        .with_context(|| "extracting calc sections".to_string())?;
    std::fs::write(
        &calc_sections_path,
        serde_json::to_string_pretty(&calc_sections)?,
    )
    .with_context(|| format!("writing {}", calc_sections_path.display()))?;
    wrote.push(calc_sections_path);

    // Trees — one JSON per tree version directory under data/trees/.
    let trees_dir = args.out.join("trees");
    std::fs::create_dir_all(&trees_dir)?;
    let mut tree_versions = Vec::new();
    let mut skipped_trees = Vec::new();
    for version in tree::list_tree_versions(&args.pob)? {
        let tree_path = trees_dir.join(format!("{version}.json"));
        match tree::extract(&args.pob, &version) {
            Ok(tree) => {
                let json = serde_json::to_string(&tree)?;
                std::fs::write(&tree_path, json)
                    .with_context(|| format!("writing {}", tree_path.display()))?;
                tree_versions.push(version);
                wrote.push(tree_path);
            }
            Err(tree::ExtractError::IncompatibleSchema(reason)) => {
                eprintln!("  skip {version}: {reason}");
                skipped_trees.push((version, reason));
            }
            Err(tree::ExtractError::Other(e)) => {
                return Err(e).with_context(|| format!("extracting tree {version}"));
            }
        }
    }
    if !skipped_trees.is_empty() {
        let summary: Vec<_> = skipped_trees
            .iter()
            .map(|(v, r)| format!("{v}: {r}"))
            .collect();
        std::fs::write(
            args.out.join("trees/skipped.txt"),
            summary.join("\n") + "\n",
        )?;
    }
    let index_path = trees_dir.join("index.json");
    std::fs::write(&index_path, serde_json::to_string_pretty(&tree_versions)?)?;
    wrote.push(index_path);

    println!("wrote {} files:", wrote.len());
    for p in &wrote {
        let bytes = std::fs::metadata(p).map_or(0, |m| m.len());
        println!("  {} ({} bytes)", p.display(), bytes);
    }
    Ok(())
}

mod bases;
mod gems;
mod lua_value;
mod skills;

/// Helper used by submodules: open a sandboxed Lua state with stubs for the globals PoB's
/// data files expect, plus `SkillType` / `KeywordFlag` enum tables for skill data.
pub(crate) fn make_lua() -> Result<Lua> {
    let lua = Lua::new();
    {
        let globals = lua.globals();
        // PoB's Bases/* and Gems.lua reference these helpers; provide inert stand-ins so
        // execution doesn't fault. These only matter for files we don't fully extract.
        let stub = lua.create_function(|_, _: mlua::MultiValue| Ok(()))?;
        globals.set("LoadModule", stub.clone())?;
        globals.set("ConPrintf", stub.clone())?;

        // Constants the skill files reference. The numeric values must match
        // `pob_data::flags::{SkillType, KeywordFlag, ModFlag}` — same as PoB's
        // `Data/Global.lua`.
        let skill_type = lua.create_table()?;
        for (name, v) in skill_type_pairs() {
            skill_type.set(*name, *v as i64)?;
        }
        globals.set("SkillType", skill_type)?;

        let keyword_flag = lua.create_table()?;
        for (name, bit) in keyword_flag_pairs() {
            keyword_flag.set(*name, *bit as i64)?;
        }
        globals.set("KeywordFlag", keyword_flag)?;

        let mod_flag = lua.create_table()?;
        for (name, bit) in mod_flag_pairs() {
            mod_flag.set(*name, *bit as i64)?;
        }
        globals.set("ModFlag", mod_flag)?;

        // bit.bor / bit.band shims (Lua 5.4 has no `bit` library by default — mlua's
        // `lua54` feature ships LuaJIT-compatible bit ops, but explicit shims are safer.)
        let bit_tbl = lua.create_table()?;
        bit_tbl.set(
            "bor",
            lua.create_function(|_, args: mlua::MultiValue| {
                let mut acc: i64 = 0;
                for v in args {
                    if let Value::Integer(i) = v {
                        acc |= i;
                    } else if let Value::Number(n) = v {
                        acc |= n as i64;
                    }
                }
                Ok(acc)
            })?,
        )?;
        bit_tbl.set(
            "band",
            lua.create_function(|_, args: mlua::MultiValue| {
                let mut acc: i64 = -1;
                for v in args {
                    if let Value::Integer(i) = v {
                        acc &= i;
                    } else if let Value::Number(n) = v {
                        acc &= n as i64;
                    }
                }
                Ok(acc)
            })?,
        )?;
        bit_tbl.set("bnot", lua.create_function(|_, v: i64| Ok(!v))?)?;
        bit_tbl.set(
            "bxor",
            lua.create_function(|_, args: mlua::MultiValue| {
                let mut acc: i64 = 0;
                for v in args {
                    if let Value::Integer(i) = v {
                        acc ^= i;
                    }
                }
                Ok(acc)
            })?,
        )?;
        globals.set("bit", bit_tbl)?;
    }
    Ok(lua)
}

fn skill_type_pairs() -> &'static [(&'static str, u32)] {
    // Mirror the `SkillType` enum in pob-data::flags (1..=141 range, 1-indexed names).
    &[
        ("Attack", 1),
        ("Spell", 2),
        ("Projectile", 3),
        ("DualWieldOnly", 4),
        ("Buff", 5),
        ("Removed6", 6),
        ("MainHandOnly", 7),
        ("Removed8", 8),
        ("Minion", 9),
        ("Damage", 10),
        ("Area", 11),
        ("Duration", 12),
        ("RequiresShield", 13),
        ("ProjectileSpeed", 14),
        ("HasReservation", 15),
        ("ReservationBecomesCost", 16),
        ("Trappable", 17),
        ("Totemable", 18),
        ("Mineable", 19),
        ("ElementalStatus", 20),
        ("MinionsCanExplode", 21),
        ("Removed22", 22),
        ("Chains", 23),
        ("Melee", 24),
        ("MeleeSingleTarget", 25),
        ("Multicastable", 26),
        ("TotemCastsAlone", 27),
        ("Multistrikeable", 28),
        ("CausesBurning", 29),
        ("SummonsTotem", 30),
        ("TotemCastsWhenNotDetached", 31),
        ("Fire", 32),
        ("Cold", 33),
        ("Lightning", 34),
        ("Triggerable", 35),
        ("Trapped", 36),
        ("Movement", 37),
        ("Removed38", 38),
        ("DamageOverTime", 39),
        ("RemoteMined", 40),
        ("Triggered", 41),
        ("Vaal", 42),
        ("Aura", 43),
        ("Removed44", 44),
        ("CanTargetUnusableCorpse", 45),
        ("Removed46", 46),
        ("RangedAttack", 47),
        ("Removed48", 48),
        ("Chaos", 49),
        ("FixedSpeedProjectile", 50),
        ("Removed51", 51),
        ("ThresholdJewelArea", 52),
        ("ThresholdJewelProjectile", 53),
        ("ThresholdJewelDuration", 54),
        ("ThresholdJewelRangedAttack", 55),
        ("Removed56", 56),
        ("Channel", 57),
        ("DegenOnlySpellDamage", 58),
        ("Removed59", 59),
        ("InbuiltTrigger", 60),
        ("Golem", 61),
        ("Herald", 62),
        ("AuraAffectsEnemies", 63),
        ("NoRuthless", 64),
        ("ThresholdJewelSpellDamage", 65),
        ("Cascadable", 66),
        ("ProjectilesFromUser", 67),
        ("MirageArcherCanUse", 68),
        ("ProjectileSpiral", 69),
        ("SingleMainProjectile", 70),
        ("MinionsPersistWhenSkillRemoved", 71),
        ("ProjectileNumber", 72),
        ("Warcry", 73),
        ("Instant", 74),
        ("Brand", 75),
        ("DestroysCorpse", 76),
        ("NonHitChill", 77),
        ("ChillingArea", 78),
        ("AppliesCurse", 79),
        ("CanRapidFire", 80),
        ("AuraDuration", 81),
        ("AreaSpell", 82),
        ("OR", 83),
        ("AND", 84),
        ("NOT", 85),
        ("Physical", 86),
        ("AppliesMaim", 87),
        ("CreatesMinion", 88),
        ("Guard", 89),
        ("Travel", 90),
        ("Blink", 91),
        ("CanHaveBlessing", 92),
        ("ProjectilesNotFromUser", 93),
        ("AttackInPlaceIsDefault", 94),
        ("Nova", 95),
        ("InstantNoRepeatWhenHeld", 96),
        ("InstantShiftAttackForLeftMouse", 97),
        ("AuraNotOnCaster", 98),
        ("Banner", 99),
        ("Rain", 100),
        ("Cooldown", 101),
        ("ThresholdJewelChaining", 102),
        ("Slam", 103),
        ("Stance", 104),
        ("NonRepeatable", 105),
        ("OtherThingUsesSkill", 106),
        ("Steel", 107),
        ("Hex", 108),
        ("Mark", 109),
        ("Aegis", 110),
        ("Orb", 111),
        ("KillNoDamageModifiers", 112),
        ("RandomElement", 113),
        ("LateConsumeCooldown", 114),
        ("Arcane", 115),
        ("FixedCastTime", 116),
        ("RequiresOffHandNotWeapon", 117),
        ("Link", 118),
        ("Blessing", 119),
        ("ZeroReservation", 120),
        ("DynamicCooldown", 121),
        ("Microtransaction", 122),
        ("OwnerCannotUse", 123),
        ("ProjectilesNumberModifiersNotApplied", 124),
        ("TotemsAreBallistae", 125),
        ("SkillGrantedBySupport", 126),
        ("PreventHexTransfer", 127),
        ("MinionsAreUndamagable", 128),
        ("InnateTrauma", 129),
        ("DualWieldRequiresDifferentTypes", 130),
        ("NoVolley", 131),
        ("Retaliation", 132),
        ("NeverExertable", 133),
        ("DisallowTriggerSupports", 134),
        ("ProjectileCannotReturn", 135),
        ("Offering", 136),
        ("SupportedByBane", 137),
        ("WandAttack", 138),
        ("GainsIntensity", 139),
        ("CreatesSentinelMinion", 140),
        ("SupportedByAutoExertion", 141),
    ]
}

fn keyword_flag_pairs() -> &'static [(&'static str, u32)] {
    &[
        ("Aura", 0x0000_0001),
        ("Curse", 0x0000_0002),
        ("Warcry", 0x0000_0004),
        ("Movement", 0x0000_0008),
        ("Physical", 0x0000_0010),
        ("Fire", 0x0000_0020),
        ("Cold", 0x0000_0040),
        ("Lightning", 0x0000_0080),
        ("Chaos", 0x0000_0100),
        ("Vaal", 0x0000_0200),
        ("Bow", 0x0000_0400),
        ("Arrow", 0x0000_0800),
        ("Trap", 0x0000_1000),
        ("Mine", 0x0000_2000),
        ("Totem", 0x0000_4000),
        ("Minion", 0x0000_8000),
        ("Attack", 0x0001_0000),
        ("Spell", 0x0002_0000),
        ("Hit", 0x0004_0000),
        ("Ailment", 0x0008_0000),
        ("Brand", 0x0010_0000),
        ("Poison", 0x0020_0000),
        ("Bleed", 0x0040_0000),
        ("Ignite", 0x0080_0000),
        ("PhysicalDot", 0x0100_0000),
        ("LightningDot", 0x0200_0000),
        ("ColdDot", 0x0400_0000),
        ("FireDot", 0x0800_0000),
        ("ChaosDot", 0x1000_0000),
        ("MatchAll", 0x4000_0000),
    ]
}

fn mod_flag_pairs() -> &'static [(&'static str, u32)] {
    &[
        ("Attack", 0x0000_0001),
        ("Spell", 0x0000_0002),
        ("Hit", 0x0000_0004),
        ("Dot", 0x0000_0008),
        ("Cast", 0x0000_0010),
        ("Melee", 0x0000_0100),
        ("Area", 0x0000_0200),
        ("Projectile", 0x0000_0400),
        ("SourceMask", 0x0000_0600),
        ("Ailment", 0x0000_0800),
        ("MeleeHit", 0x0000_1000),
        ("Weapon", 0x0000_2000),
        ("Axe", 0x0001_0000),
        ("Bow", 0x0002_0000),
        ("Claw", 0x0004_0000),
        ("Dagger", 0x0008_0000),
        ("Mace", 0x0010_0000),
        ("Staff", 0x0020_0000),
        ("Sword", 0x0040_0000),
        ("Wand", 0x0080_0000),
        ("Unarmed", 0x0100_0000),
        ("Fishing", 0x0200_0000),
        ("WeaponMelee", 0x0400_0000),
        ("WeaponRanged", 0x0800_0000),
        ("Weapon1H", 0x1000_0000),
        ("Weapon2H", 0x2000_0000),
        ("WeaponMask", 0x2FFF_0000),
    ]
}

/// Walk a Lua value into a `serde_json::Value`. Keeps integer/string keys distinct so
/// later code can decide how to interpret them. Functions and userdata are dropped
/// (replaced with `null`) — they're meaningful at runtime only.
pub(crate) fn lua_to_json(v: Value) -> Result<serde_json::Value> {
    use serde_json::Value as J;
    match v {
        Value::Nil => Ok(J::Null),
        Value::Boolean(b) => Ok(J::Bool(b)),
        Value::Integer(i) => Ok(J::Number(i.into())),
        Value::Number(n) => Ok(serde_json::Number::from_f64(n).map_or(J::Null, J::Number)),
        Value::String(s) => Ok(J::String(s.to_str()?.to_owned())),
        Value::Table(t) => table_to_json(t),
        // Function / Thread / UserData / LightUserData / Error: not representable. Replace
        // with a sentinel rather than erroring so partial extractions still succeed.
        Value::Function(_) => Ok(J::Object(serde_json::Map::from_iter([(
            "__lua_function".to_owned(),
            J::Bool(true),
        )]))),
        Value::Thread(_) => Ok(J::Null),
        Value::UserData(_) => Ok(J::Null),
        Value::LightUserData(_) => Ok(J::Null),
        Value::Error(e) => Ok(J::String(format!("__lua_error: {e}"))),
        _ => bail!("unsupported lua value"),
    }
}

fn table_to_json(t: Table) -> Result<serde_json::Value> {
    use serde_json::Value as J;
    // Decide whether this is an array (consecutive 1..=n integer keys, no others) or a
    // map. Lua's `t:sequence_values()` would only return the array part, but we want to
    // detect mixed tables too.
    let mut arr_max: i64 = 0;
    let mut has_string_key = false;
    let mut has_non_seq_int_key = false;

    let pairs: Vec<(Value, Value)> = t.pairs::<Value, Value>().collect::<Result<_, _>>()?;
    for (k, _) in &pairs {
        match k {
            Value::Integer(i) => {
                if *i >= 1 {
                    arr_max = arr_max.max(*i);
                } else {
                    has_non_seq_int_key = true;
                }
            }
            Value::String(_) => has_string_key = true,
            _ => has_non_seq_int_key = true,
        }
    }

    let total = pairs.len() as i64;
    if !has_string_key && !has_non_seq_int_key && arr_max == total && arr_max > 0 {
        // Sparse-free 1..=n array.
        let mut out = vec![J::Null; arr_max as usize];
        for (k, v) in pairs {
            if let Value::Integer(i) = k {
                out[(i - 1) as usize] = lua_to_json(v)?;
            }
        }
        return Ok(J::Array(out));
    }

    let mut map = serde_json::Map::with_capacity(pairs.len());
    for (k, v) in pairs {
        let key = match k {
            Value::String(s) => s.to_str()?.to_owned(),
            Value::Integer(i) => i.to_string(),
            Value::Number(n) => n.to_string(),
            other => bail!("unsupported table key: {other:?}"),
        };
        map.insert(key, lua_to_json(v)?);
    }
    Ok(J::Object(map))
}

/// Read a Lua file and return its top-level `return`ed value, evaluated in our sandbox.
pub(crate) fn load_lua_file_returning(lua: &Lua, path: &Path) -> Result<serde_json::Value> {
    let src =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let chunk = lua.load(&src).set_name(path.to_string_lossy().as_ref());
    let v: Value = chunk
        .eval()
        .with_context(|| format!("evaluating {}", path.display()))?;
    lua_to_json(v)
}

/// Read a Lua file like `Bases/sword.lua` that mutates an `itemBases` table passed via
/// varargs. Returns the table after execution.
pub(crate) fn load_lua_file_with_table_arg(lua: &Lua, path: &Path) -> Result<serde_json::Value> {
    let src =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let table = lua.create_table()?;
    let chunk = lua.load(&src).set_name(path.to_string_lossy().as_ref());
    let func = chunk
        .into_function()
        .with_context(|| format!("compiling {}", path.display()))?;
    func.call::<()>(table.clone())
        .with_context(|| format!("executing {}", path.display()))?;
    table_to_json(table)
}

/// Convenience: walk a directory and return paths of files matching `predicate`, sorted.
pub(crate) fn list_files(dir: &Path, predicate: impl Fn(&Path) -> bool) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let p = entry.path();
        if p.is_file() && predicate(&p) {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}

/// Used by tree.rs as a typed accessor.
pub(crate) fn obj(v: &serde_json::Value) -> Result<&serde_json::Map<String, serde_json::Value>> {
    v.as_object()
        .ok_or_else(|| anyhow!("expected JSON object, got {v:?}"))
}

#[allow(dead_code)]
pub(crate) fn arr(v: &serde_json::Value) -> Result<&[serde_json::Value]> {
    v.as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| anyhow!("expected JSON array, got {v:?}"))
}

pub(crate) fn merge_into(
    dest: &mut BTreeMap<String, serde_json::Value>,
    other: serde_json::Value,
) -> Result<()> {
    let map = match other {
        serde_json::Value::Object(m) => m,
        _ => bail!("expected object to merge"),
    };
    for (k, v) in map {
        dest.insert(k, v);
    }
    Ok(())
}
