//! `generated/global.rs`: `Data/Global.lua`'s `ModFlag`, `KeywordFlag`,
//! `MatchKeywordFlags`, `SkillType` and `colorCodes` in the shape fixed by
//! calc-architecture.md section 4.2.

use std::fmt::Write as _;

use anyhow::{anyhow, bail, Result};
use mlua::Table;

use super::SrcFile;
use crate::{lerr, Env};

const FILE: &str = "Data/Global.lua";

/// `(key, line)` of each line matching `prefix<ident> =`, in source order.
fn assignments(src: &SrcFile, prefix: &str, from: usize, to: usize) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    for n in from..=to {
        let l = &src.lines[n - 1];
        if let Some(rest) = l.strip_prefix(prefix) {
            let ident: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !ident.is_empty() && rest[ident.len()..].trim_start().starts_with('=') {
                out.push((ident, n));
            }
        }
    }
    out
}

fn u32_of(t: &Table, name: &str, what: &str) -> Result<u32> {
    let v: f64 = t.get(name).map_err(|e| anyhow!("{what}.{name}: {e}"))?;
    if v.fract() != 0.0 || !(0.0..=f64::from(u32::MAX)).contains(&v) {
        bail!("{what}.{name} = {v} is not a u32");
    }
    Ok(v as u32)
}

fn count_keys(t: &Table) -> Result<usize> {
    let mut n = 0;
    for p in t.clone().pairs::<mlua::Value, mlua::Value>() {
        p.map_err(lerr)?;
        n += 1;
    }
    Ok(n)
}

fn hex(v: u32) -> String {
    format!("0x{:04X}_{:04X}", v >> 16, v & 0xFFFF)
}

/// One `ModFlag`/`KeywordFlag` module.
fn flag_module(out: &mut String, src: &SrcFile, t: &Table, name: &str, doc: &str) -> Result<()> {
    let start = src
        .find(|l| l.starts_with(&format!("{name} = {{")))
        .ok_or_else(|| anyhow!("{FILE}: `{name} = {{` not found"))?;
    let entries = assignments(src, &format!("{name}."), start, src.lines.len());
    let last = entries.last().map(|e| e.1).unwrap_or(start);
    if entries.len() != count_keys(t)? {
        bail!(
            "{name}: source order lists {} entries, the table has {}",
            entries.len(),
            count_keys(t)?
        );
    }
    let _ = writeln!(out, "{}", src.marker(start, last));
    let _ = writeln!(out, "/// PoB `{name}` ({doc}); entries in source order.");
    let _ = writeln!(out, "pub mod {name} {{");
    let mut vals = Vec::new();
    for (k, _) in &entries {
        let v = u32_of(t, k, name)?;
        vals.push((k.clone(), v));
        let _ = writeln!(out, "    pub const {k}: u32 = {};", hex(v));
    }
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "    /// `(name, value)` of every entry, in source order."
    );
    let _ = writeln!(out, "    pub const ENTRIES: &[(&str, u32)] = &[");
    for (k, _) in &vals {
        let _ = writeln!(out, "        (\"{k}\", {k}),");
    }
    let _ = writeln!(out, "    ];");
    let _ = writeln!(out);
    let _ = writeln!(out, "    /// `{name}[s]` (`nil` -> `None`).");
    let _ = writeln!(out, "    pub fn by_name(s: &str) -> Option<u32> {{");
    let _ = writeln!(out, "        match s {{");
    for (k, _) in &vals {
        let _ = writeln!(out, "            \"{k}\" => Some({k}),");
    }
    let _ = writeln!(out, "            _ => None,");
    let _ = writeln!(out, "        }}");
    let _ = writeln!(out, "    }}");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);
    Ok(())
}

fn match_keyword_flags(out: &mut String, src: &SrcFile) -> Result<()> {
    let a = src
        .find(|l| l == "local band = bit.band")
        .ok_or_else(|| anyhow!("{FILE}: `local band = bit.band` not found"))?;
    let clear = src
        .find(|l| l.starts_with("function ClearMatchKeywordFlagsCache("))
        .ok_or_else(|| anyhow!("{FILE}: ClearMatchKeywordFlagsCache not found"))?;
    let clear_end = (clear..=src.lines.len())
        .find(|&n| src.lines[n - 1] == "end")
        .ok_or_else(|| anyhow!("{FILE}: end of ClearMatchKeywordFlagsCache"))?;
    let f = src
        .find(|l| l.starts_with("function MatchKeywordFlags("))
        .ok_or_else(|| anyhow!("{FILE}: MatchKeywordFlags not found"))?;
    let f_end = (f..=src.lines.len())
        .find(|&n| src.lines[n - 1] == "end")
        .ok_or_else(|| anyhow!("{FILE}: end of MatchKeywordFlags"))?;
    let _ = writeln!(out, "{}", src.marker(clear, clear_end));
    let _ = writeln!(
        out,
        "/// The cache MatchKeywordFlags memoises into is not reproduced (it is a pure memo), so there is nothing to clear."
    );
    let _ = writeln!(out, "pub fn ClearMatchKeywordFlagsCache() {{}}");
    let _ = writeln!(out);
    let _ = writeln!(out, "{}", src.marker(a, f_end));
    let _ = writeln!(
        out,
        "/// Whether a mod's KeywordFlags are satisfied by `keywordFlags` (any flag, or all with `MatchAll`)."
    );
    let _ = writeln!(
        out,
        "/// The cache is a pure memo, so it is not reproduced. Flags are below 2^31, so u32 `&` equals LuaJIT's signed `bit.band`."
    );
    out.push_str(
        "pub fn MatchKeywordFlags(keywordFlags: u32, modKeywordFlags: u32) -> bool {
    let MatchAllMask: u32 = !KeywordFlag::MatchAll;
    let matchAll = (modKeywordFlags & KeywordFlag::MatchAll) != 0;
    let modMasked = modKeywordFlags & MatchAllMask;
    let keywordMasked = keywordFlags & MatchAllMask;

    let matches;
    if matchAll {
        matches = (keywordMasked & modMasked) == modMasked;
    } else {
        matches = (modMasked == 0) || ((keywordMasked & modMasked) != 0);
    }
    matches
}

",
    );
    Ok(())
}

fn skill_type(out: &mut String, src: &SrcFile, t: &Table) -> Result<()> {
    let start = src
        .find(|l| l == "SkillType = {")
        .ok_or_else(|| anyhow!("{FILE}: `SkillType = {{` not found"))?;
    let end = (start..=src.lines.len())
        .find(|&n| src.lines[n - 1] == "}")
        .ok_or_else(|| anyhow!("{FILE}: end of SkillType"))?;
    let entries = assignments(src, "\t", start + 1, end - 1);
    if entries.len() != count_keys(t)? {
        bail!(
            "SkillType: source lists {} entries, the table has {}",
            entries.len(),
            count_keys(t)?
        );
    }
    let mut vals = Vec::new();
    for (k, _) in &entries {
        let v = u32_of(t, k, "SkillType")?;
        if v > 255 {
            bail!("SkillType.{k} = {v} does not fit u8");
        }
        vals.push((k.clone(), v));
    }
    let max = vals.iter().map(|v| v.1).max().unwrap_or(0);
    let _ = writeln!(out, "{}", src.marker(start, end));
    let _ = writeln!(
        out,
        "/// PoB `SkillType` (keys of skillTypes / addSkillTypes / requireSkillTypes ...); entries in source order."
    );
    let _ = writeln!(out, "pub mod SkillType {{");
    let _ = writeln!(out, "    pub type T = u8;");
    for (k, v) in &vals {
        let _ = writeln!(out, "    pub const {k}: T = {v};");
    }
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "    /// Number of slots in a value-indexed table (values are 1..={max})."
    );
    let _ = writeln!(out, "    pub const COUNT: usize = {};", max + 1);
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "    /// `(name, value)` of every entry, in source order."
    );
    let _ = writeln!(out, "    pub const ENTRIES: &[(&str, T)] = &[");
    for (k, _) in &vals {
        let _ = writeln!(out, "        (\"{k}\", {k}),");
    }
    let _ = writeln!(out, "    ];");
    let _ = writeln!(out);
    let _ = writeln!(out, "    /// `SkillType[s]` (`nil` -> `None`).");
    let _ = writeln!(out, "    pub fn by_name(s: &str) -> Option<T> {{");
    let _ = writeln!(out, "        match s {{");
    for (k, _) in &vals {
        let _ = writeln!(out, "            \"{k}\" => Some({k}),");
    }
    let _ = writeln!(out, "            _ => None,");
    let _ = writeln!(out, "        }}");
    let _ = writeln!(out, "    }}");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "    /// The name of a value (the inverse of `by_name`)."
    );
    let _ = writeln!(out, "    pub fn name(t: T) -> Option<&'static str> {{");
    let _ = writeln!(out, "        match t {{");
    let mut by_val: Vec<&(String, u32)> = vals.iter().collect();
    by_val.sort_by_key(|v| v.1);
    by_val.dedup_by_key(|v| v.1);
    for (k, v) in by_val {
        let _ = writeln!(out, "            {v} => Some(\"{k}\"),");
    }
    let _ = writeln!(out, "            _ => None,");
    let _ = writeln!(out, "        }}");
    let _ = writeln!(out, "    }}");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);
    let words = (max as usize + 1).div_ceil(64);
    let _ = writeln!(
        out,
        "/// One `requireSkillTypes` / `excludeSkillTypes` / `minionSkillTypes` RPN token. The operators are the
/// skill types `SkillType.OR` / `AND` / `NOT` (Global.lua), everything else is a type."
    );
    out.push_str(
        "#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SkillTypeExpr {
    Type(SkillType::T),
    AND,
    OR,
    NOT,
}

impl SkillTypeExpr {
    /// The token a list entry (a SkillType value) stands for.
    pub fn from_skill_type(t: SkillType::T) -> Self {
        match t {
            SkillType::AND => Self::AND,
            SkillType::OR => Self::OR,
            SkillType::NOT => Self::NOT,
            t => Self::Type(t),
        }
    }
}

",
    );
    let _ = writeln!(
        out,
        "/// `{{[SkillType.X] = true}}` as a bit set over `0..SkillType::COUNT`."
    );
    let _ = writeln!(
        out,
        "#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]"
    );
    let _ = writeln!(out, "pub struct SkillTypeSet(pub [u64; {words}]);");
    out.push_str(
        "
impl SkillTypeSet {
    pub fn has(&self, t: SkillType::T) -> bool {
        let i = t as usize;
        i < SkillType::COUNT && self.0[i / 64] & (1u64 << (i % 64)) != 0
    }

    pub fn set(&mut self, t: SkillType::T, v: bool) {
        let i = t as usize;
        assert!(i < SkillType::COUNT, \"SkillType {} out of range\", t);
        if v {
            self.0[i / 64] |= 1u64 << (i % 64);
        } else {
            self.0[i / 64] &= !(1u64 << (i % 64));
        }
    }

    /// Members in ascending value order.
    pub fn iter(&self) -> impl Iterator<Item = SkillType::T> + '_ {
        (0..SkillType::COUNT).filter(move |&i| self.0[i / 64] & (1u64 << (i % 64)) != 0).map(|i| i as SkillType::T)
    }
}

",
    );
    Ok(())
}

fn color_codes(out: &mut String, src: &SrcFile, t: &Table) -> Result<()> {
    let start = src
        .find(|l| l == "colorCodes = {")
        .ok_or_else(|| anyhow!("{FILE}: `colorCodes = {{` not found"))?;
    let end = (start..=src.lines.len())
        .find(|&n| src.lines[n - 1] == "}")
        .ok_or_else(|| anyhow!("{FILE}: end of colorCodes"))?;
    let mut keys = assignments(src, "\t", start + 1, end - 1);
    let more = assignments(src, "colorCodes.", end + 1, src.lines.len());
    let last = more.last().map(|e| e.1).unwrap_or(end);
    keys.extend(more);
    let mut seen = std::collections::HashSet::new();
    keys.retain(|k| seen.insert(k.0.clone()));
    if keys.len() != count_keys(t)? {
        bail!(
            "colorCodes: source lists {} keys, the table has {}",
            keys.len(),
            count_keys(t)?
        );
    }
    let _ = writeln!(out, "{}", src.marker(start, last));
    let _ = writeln!(
        out,
        "/// `colorCodes` after Global.lua runs (aliases resolved), in source order of first definition."
    );
    let _ = writeln!(out, "pub static colorCodes: &[(&str, &str)] = &[");
    for (k, _) in &keys {
        let v: String = t
            .get(k.as_str())
            .map_err(|e| anyhow!("colorCodes.{k}: {e}"))?;
        let _ = writeln!(out, "    (\"{k}\", {v:?}),");
    }
    let _ = writeln!(out, "];");
    Ok(())
}

/// Render `generated/global.rs` from a booted PoB state.
pub fn render(src: &SrcFile, env: &Env, header: &str) -> Result<String> {
    let g = env.lua.globals();
    let get = |n: &str| -> Result<Table> { g.get(n).map_err(|e| anyhow!("{n}: {e}")) };
    let mut out = String::new();
    let _ = writeln!(out, "{}", src.file_marker());
    out.push_str(header);
    let _ = writeln!(
        out,
        "#![allow(non_upper_case_globals, non_snake_case, dead_code, clippy::all, clippy::pedantic)]"
    );
    let _ = writeln!(out);
    flag_module(
        &mut out,
        src,
        &get("ModFlag")?,
        "ModFlag",
        "bit flags of Mod.flags",
    )?;
    flag_module(
        &mut out,
        src,
        &get("KeywordFlag")?,
        "KeywordFlag",
        "bit flags of Mod.keywordFlags",
    )?;
    match_keyword_flags(&mut out, src)?;
    skill_type(&mut out, src, &get("SkillType")?)?;
    color_codes(&mut out, src, &get("colorCodes")?)?;
    Ok(out)
}
