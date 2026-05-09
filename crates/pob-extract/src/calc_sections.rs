//! Extract `Modules/CalcSections.lua` into a thin Calcs-tab section list.
//!
//! Slice 1 of [#34](https://github.com/jonatanferm/PathOfBuildingMK2/issues/34) only keeps
//! section/subsection headers (id, label, group, width, color, defaultCollapsed). The deep
//! `data` row tree is intentionally dropped — see [`pob_data::calc_sections`].

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use mlua::{Lua, Value};
use pob_data::{CalcSection, CalcSubsection};

use crate::{lua_to_json, make_lua};

pub fn extract(pob_root: &Path) -> Result<Vec<CalcSection>> {
    let path = pob_root.join("src/Modules/CalcSections.lua");
    let lua = make_lua()?;
    install_color_codes(&lua)?;
    let src =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let value: Value = lua
        .load(&src)
        .set_name(path.to_string_lossy().as_ref())
        .eval()
        .with_context(|| format!("evaluating {}", path.display()))?;
    let json = lua_to_json(value)?;
    let arr = json
        .as_array()
        .ok_or_else(|| anyhow!("CalcSections.lua did not return an array"))?;

    let mut out = Vec::with_capacity(arr.len());
    for (idx, raw) in arr.iter().enumerate() {
        out.push(parse_section(raw).with_context(|| format!("section #{idx}"))?);
    }
    Ok(out)
}

/// Each section is a positional Lua array: `{ width, id, group, color, subsections }`.
/// `lua_to_json` turns that into a 5-element JSON array (1-indexed in Lua → 0-indexed here).
fn parse_section(v: &serde_json::Value) -> Result<CalcSection> {
    let arr = v
        .as_array()
        .ok_or_else(|| anyhow!("expected positional array, got {v}"))?;
    if arr.len() < 5 {
        return Err(anyhow!(
            "section has {} elements, expected at least 5",
            arr.len()
        ));
    }
    let width = arr[0]
        .as_u64()
        .ok_or_else(|| anyhow!("width is not a number"))? as u8;
    let id = arr[1]
        .as_str()
        .ok_or_else(|| anyhow!("id is not a string"))?
        .to_owned();
    let group = arr[2]
        .as_u64()
        .ok_or_else(|| anyhow!("group is not a number"))? as u8;
    let color = arr[3]
        .as_str()
        .ok_or_else(|| anyhow!("color is not a string"))?
        .to_owned();

    let subs_raw = arr[4]
        .as_array()
        .ok_or_else(|| anyhow!("subsections is not an array"))?;
    let mut subsections = Vec::with_capacity(subs_raw.len());
    for sub in subs_raw {
        subsections.push(parse_subsection(sub)?);
    }

    Ok(CalcSection {
        id,
        width,
        group,
        color,
        subsections,
    })
}

fn parse_subsection(v: &serde_json::Value) -> Result<CalcSubsection> {
    let obj = v
        .as_object()
        .ok_or_else(|| anyhow!("subsection is not an object"))?;
    let label = obj
        .get("label")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_owned();
    let default_collapsed = obj
        .get("defaultCollapsed")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    Ok(CalcSubsection {
        label,
        default_collapsed,
    })
}

/// Stub the `colorCodes` global with the small subset CalcSections.lua actually reads.
/// CalcSections only references `colorCodes.OFFENCE/DEFENCE/NORMAL/CRAFTED/RAGE/LIFE/MANA/
/// ES/WARD/ARMOUR/EVASION/PHYS/FIRE/COLD/LIGHTNING/CHAOS` — we provide all of them so any
/// nested `format` strings (which we currently drop) still evaluate cleanly.
fn install_color_codes(lua: &Lua) -> Result<()> {
    let codes = lua.create_table()?;
    for (k, v) in &[
        ("NORMAL", "^xC8C8C8"),
        ("MAGIC", "^x8888FF"),
        ("RARE", "^xFFFF77"),
        ("UNIQUE", "^xAF6025"),
        ("RELIC", "^x60C060"),
        ("GEM", "^x1AA29B"),
        ("CRAFTED", "^xB8DAF1"),
        ("CUSTOM", "^x5CF0BB"),
        ("SOURCE", "^x88FFFF"),
        ("FIRE", "^xB97123"),
        ("COLD", "^x3F6DB3"),
        ("LIGHTNING", "^xADAA47"),
        ("CHAOS", "^xD02090"),
        ("POSITIVE", "^x33FF77"),
        ("NEGATIVE", "^xDD0022"),
        ("HIGHLIGHT", "^xFF0000"),
        ("OFFENCE", "^xE07030"),
        ("DEFENCE", "^x8080E0"),
        ("MARAUDER", "^xE05030"),
        ("RANGER", "^x70FF70"),
        ("WITCH", "^x7070FF"),
        ("WARNING", "^xFF9922"),
    ] {
        codes.set(*k, *v)?;
    }
    // Aliases mirroring `colorCodes.LIFE = colorCodes.MARAUDER`, etc. in Global.lua.
    codes.set("LIFE", "^xE05030")?;
    codes.set("MANA", "^x7070FF")?;
    codes.set("ES", "^x88FFFF")?;
    codes.set("WARD", "^xFFFF77")?;
    codes.set("ARMOUR", "^xC8C8C8")?;
    codes.set("EVASION", "^x33FF77")?;
    codes.set("RAGE", "^xFF9922")?;
    codes.set("PHYS", "^xC8C8C8")?;
    codes.set("STRENGTH", "^xE05030")?;
    codes.set("DEXTERITY", "^x70FF70")?;
    codes.set("INTELLIGENCE", "^x7070FF")?;
    lua.globals().set("colorCodes", codes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pob_root() -> Option<std::path::PathBuf> {
        let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        // Walk up to the workspace root and look for `.PathOfBuilding`.
        for ancestor in here.ancestors() {
            let candidate = ancestor.join(".PathOfBuilding");
            if candidate.join("src/Modules/CalcSections.lua").exists() {
                return Some(candidate);
            }
        }
        None
    }

    #[test]
    fn extracts_all_top_level_sections() {
        let Some(root) = pob_root() else {
            eprintln!("skipping: .PathOfBuilding checkout not found next to workspace");
            return;
        };
        let sections = extract(&root).expect("extracts cleanly");
        // Spot-check a handful of well-known sections; we do not assert an exact count
        // because PoB occasionally adds/removes sections between leagues.
        let ids: Vec<&str> = sections.iter().map(|s| s.id.as_str()).collect();
        for expected in &[
            "HitDamage",
            "Speed",
            "Crit",
            "Life",
            "Mana",
            "EnergyShield",
            "Resist",
            "Armour",
            "Evasion",
        ] {
            assert!(
                ids.contains(expected),
                "missing section `{expected}`; got {ids:?}"
            );
        }
        // Sanity: every section has at least one subsection with a non-empty label.
        for s in &sections {
            assert!(
                !s.subsections.is_empty(),
                "section `{}` has no subsections",
                s.id
            );
            assert!(
                !s.subsections[0].label.is_empty(),
                "section `{}` first subsection has empty label",
                s.id
            );
            assert!(
                (1..=3).contains(&s.width),
                "section `{}` has out-of-range width {}",
                s.id,
                s.width
            );
            assert!(
                (1..=3).contains(&s.group),
                "section `{}` has out-of-range group {}",
                s.id,
                s.group
            );
        }
    }
}
