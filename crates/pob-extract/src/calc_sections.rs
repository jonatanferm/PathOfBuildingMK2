//! Extract `Modules/CalcSections.lua` into the typed Calcs-tab section list used by
//! `pob-ui`'s Calcs tab.
//!
//! Slice 1 of [#34](https://github.com/jonatanferm/PathOfBuildingMK2/issues/34) shipped
//! the section + subsection headers; slice 2 walks the `data` table inside each subsection
//! and pulls out a flat row list (label / format / output_key / haveOutput / flag).
//!
//! Mod-breakdown references and matrix-shape per-element formats are still dropped — the
//! row's `output_key` is the *first* `{N:output:Key}` token found by depth-first walk,
//! which is good enough to render a single-value column for most rows.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use mlua::{Lua, Value};
use pob_data::{CalcRow, CalcSection, CalcSubsection};
use serde_json::Value as J;

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
fn parse_section(v: &J) -> Result<CalcSection> {
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

fn parse_subsection(v: &J) -> Result<CalcSubsection> {
    let obj = v
        .as_object()
        .ok_or_else(|| anyhow!("subsection is not an object"))?;
    let label = obj
        .get("label")
        .and_then(J::as_str)
        .unwrap_or("")
        .to_owned();
    let default_collapsed = obj
        .get("defaultCollapsed")
        .and_then(J::as_bool)
        .unwrap_or(false);

    // The `data` field is a Lua mixed table: it has named entries (extra, flag, colWidth, ...)
    // PLUS positional entries 1..n that are the rows. lua_to_json renders that as a JSON
    // object with stringified integer keys. We collect those keys in numeric order.
    let mut rows = Vec::new();
    if let Some(data) = obj.get("data") {
        if let Some(data_obj) = data.as_object() {
            let mut indexed: Vec<(u64, &J)> = data_obj
                .iter()
                .filter_map(|(k, v)| k.parse::<u64>().ok().map(|n| (n, v)))
                .collect();
            indexed.sort_by_key(|(k, _)| *k);
            for (_, row) in indexed {
                if let Some(parsed) = parse_row(row) {
                    rows.push(parsed);
                }
            }
        } else if let Some(arr) = data.as_array() {
            // Older / simpler layouts where `data` is a pure positional array. Each entry
            // is still treated as a row.
            for row in arr {
                if let Some(parsed) = parse_row(row) {
                    rows.push(parsed);
                }
            }
        }
    }

    Ok(CalcSubsection {
        label,
        default_collapsed,
        rows,
    })
}

/// Each row is a Lua mixed table: it has a `label` (and optionally `flag`/`notFlag`/
/// `haveOutput`/`bgCol`) plus positional sub-entries that hold `format` strings and
/// `breakdown` / `modName` references. We:
/// * pull the named fields straight out,
/// * walk the positional sub-entries to find the first `format = "..."` string,
/// * extract the first `{N:output:Key}` token from that format and store as `output_key`.
///
/// Returns `None` for rows that have neither a label nor an output key — those are
/// almost always the matrix-header rows in HitDamage/Damage Taken, which carry per-column
/// strings instead of row-shaped data.
fn parse_row(v: &J) -> Option<CalcRow> {
    let obj = v.as_object()?;
    let label = obj
        .get("label")
        .and_then(J::as_str)
        .unwrap_or("")
        .to_owned();
    let have_output = obj.get("haveOutput").and_then(J::as_str).map(str::to_owned);
    let flag = collect_flag(obj.get("flag"), obj.get("flagList"));
    let not_flag = collect_flag(obj.get("notFlag"), obj.get("notFlagList"));
    let format = first_format(v);
    let output_key = format.as_deref().and_then(extract_output_key);

    if label.is_empty() && output_key.is_none() && format.is_none() {
        return None;
    }
    Some(CalcRow {
        label,
        output_key,
        have_output,
        format,
        flag,
        not_flag,
    })
}

/// Depth-first search for the first `format = "..."` string anywhere inside the row.
/// Handles both single-format rows (`{ format = "..." }`) and matrix rows whose format
/// strings live one level deeper inside per-column sub-tables.
fn first_format(v: &J) -> Option<String> {
    if let Some(s) = v
        .as_object()
        .and_then(|o| o.get("format"))
        .and_then(J::as_str)
    {
        return Some(s.to_owned());
    }
    if let Some(o) = v.as_object() {
        // Walk numeric children (positional entries) only, sorted, so the result is
        // deterministic across PoB versions even if the JSON-object iter order shifts.
        let mut indexed: Vec<(u64, &J)> = o
            .iter()
            .filter_map(|(k, v)| k.parse::<u64>().ok().map(|n| (n, v)))
            .collect();
        indexed.sort_by_key(|(k, _)| *k);
        for (_, child) in indexed {
            if let Some(found) = first_format(child) {
                return Some(found);
            }
        }
    }
    None
}

/// Pull the first `{N:output:Foo}` token out of a format string. Returns `Some("Foo")` or
/// `None` if the format only references mods (`{0:mod:1}`) or is plain text.
fn extract_output_key(fmt: &str) -> Option<String> {
    // Look for `:output:` and slice until the matching `}`.
    let needle = ":output:";
    let start = fmt.find(needle)? + needle.len();
    let tail = &fmt[start..];
    let end = tail.find('}')?;
    let key = &tail[..end];
    // PoB output keys are `[A-Za-z0-9_.]`; reject anything weirder.
    if key.is_empty()
        || key
            .chars()
            .any(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '.')
    {
        return None;
    }
    Some(key.to_owned())
}

/// Flatten `flag = "spell"` and `flagList = {"a", "b"}` into a single comma-joined string.
/// Returns `None` if neither is set.
fn collect_flag(scalar: Option<&J>, list: Option<&J>) -> Option<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(s) = scalar.and_then(J::as_str) {
        if !s.is_empty() {
            out.push(s.to_owned());
        }
    }
    if let Some(arr) = list.and_then(J::as_array) {
        for v in arr {
            if let Some(s) = v.as_str() {
                if !s.is_empty() {
                    out.push(s.to_owned());
                }
            }
        }
    }
    if list.and_then(J::as_object).is_some() {
        // lua_to_json renders 1..n integer-keyed Lua tables as objects when there's a
        // mixed-key entry. Walk the numeric keys in order.
        let o = list.unwrap().as_object().unwrap();
        let mut indexed: Vec<(u64, &str)> = o
            .iter()
            .filter_map(|(k, v)| {
                k.parse::<u64>()
                    .ok()
                    .and_then(|n| v.as_str().map(|s| (n, s)))
            })
            .collect();
        indexed.sort_by_key(|(k, _)| *k);
        for (_, s) in indexed {
            if !s.is_empty() {
                out.push(s.to_owned());
            }
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out.join(","))
    }
}

/// Stub the `colorCodes` global with the small subset CalcSections.lua actually reads.
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
        ("MAINHAND", "^x50FF50"),
        ("MAINHANDBG", "^x071907"),
        ("OFFHAND", "^xB7B7FF"),
        ("OFFHANDBG", "^x070719"),
    ] {
        codes.set(*k, *v)?;
    }
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
        for ancestor in here.ancestors() {
            let candidate = ancestor.join(".PathOfBuilding");
            if candidate.join("src/Modules/CalcSections.lua").exists() {
                return Some(candidate);
            }
        }
        None
    }

    #[test]
    fn extract_output_key_handles_common_formats() {
        assert_eq!(
            extract_output_key("{0:output:DisplayDamage}").as_deref(),
            Some("DisplayDamage")
        );
        assert_eq!(
            extract_output_key("{2:output:Speed}/s").as_deref(),
            Some("Speed")
        );
        assert_eq!(
            extract_output_key("x {2:output:CritMultiplier}").as_deref(),
            Some("CritMultiplier")
        );
        assert_eq!(
            extract_output_key("{2:output:MainHand.CritChance}%").as_deref(),
            Some("MainHand.CritChance")
        );
        assert_eq!(extract_output_key("{0:mod:1,2}%"), None);
        assert_eq!(extract_output_key("plain text"), None);
    }

    #[test]
    fn extracts_all_top_level_sections() {
        let Some(root) = pob_root() else {
            eprintln!("skipping: .PathOfBuilding checkout not found next to workspace");
            return;
        };
        let sections = extract(&root).expect("extracts cleanly");
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
        for s in &sections {
            assert!(
                !s.subsections.is_empty(),
                "section `{}` has no subsections",
                s.id
            );
            assert!((1..=3).contains(&s.width));
            assert!((1..=3).contains(&s.group));
        }

        // Spot-check that row extraction worked for a known section.
        let speed = sections.iter().find(|s| s.id == "Speed").unwrap();
        let speed_sub = &speed.subsections[0];
        assert!(!speed_sub.rows.is_empty(), "Speed subsection has no rows");
        let attacks_per_sec = speed_sub
            .rows
            .iter()
            .find(|r| r.label == "Attacks per second");
        assert!(
            attacks_per_sec.is_some(),
            "Speed has no `Attacks per second` row"
        );
        assert_eq!(
            attacks_per_sec.unwrap().output_key.as_deref(),
            Some("Speed"),
            "`Attacks per second` row should map to output key `Speed`"
        );

        // And one with nested mod-only formats — Bleed's `Bleed Chance` row is mod-only, no
        // output key. Smoke-test that we still record it.
        let crit = sections.iter().find(|s| s.id == "Crit").unwrap();
        let crit_rows = &crit.subsections[0].rows;
        let crit_chance = crit_rows.iter().find(|r| r.label == "Crit Chance");
        assert!(crit_chance.is_some());
        assert_eq!(
            crit_chance.unwrap().output_key.as_deref(),
            Some("CritChance")
        );
    }
}
