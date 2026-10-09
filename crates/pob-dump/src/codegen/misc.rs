//! `generated/misc.rs`: verbatim-key record structs for `data.misc`
//! (Modules/Data.lua) and `data.gameConstants`, `data.characterConstants`,
//! `data.monsterConstants` (Data/Misc.lua).
//!
//! Each struct has one `f64` field per Lua key, in source order. A key that
//! is a Rust identifier is the field name verbatim; any other key (they
//! contain `%`, `+`, ...) gets a spelled-out field name and
//! `#[serde(rename = "<key>")]`, so the struct deserialises from the dump
//! (`misc.json`) with `deny_unknown_fields`. `get("<key>")` reads a field by
//! its verbatim Lua key, as the Lua code indexes these tables.

use std::collections::HashSet;
use std::fmt::Write as _;

use anyhow::{anyhow, bail, Result};
use mlua::{Table, Value};

use super::SrcFile;
use crate::{lerr, Env};

/// (struct / `data` key, source file, constructor line prefix)
pub const TABLES: &[(&str, &str, &str)] = &[
    ("misc", "Modules/Data.lua", "data.misc = {"),
    ("gameConstants", "Data/Misc.lua", "data.gameConstants = {"),
    (
        "characterConstants",
        "Data/Misc.lua",
        "data.characterConstants = {",
    ),
    (
        "monsterConstants",
        "Data/Misc.lua",
        "data.monsterConstants = {",
    ),
];

const RUST_KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while", "async", "await", "dyn", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "typeof", "unsized", "virtual", "yield", "try",
];

fn is_ident(k: &str) -> bool {
    let mut c = k.chars();
    matches!(c.next(), Some(ch) if ch.is_ascii_alphabetic() || ch == '_')
        && c.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// Rust field name for a Lua key.
pub fn field_name(k: &str) -> String {
    if is_ident(k) {
        return if RUST_KEYWORDS.contains(&k) {
            format!("r#{k}")
        } else {
            k.to_owned()
        };
    }
    let mut s = String::new();
    for ch in k.chars() {
        match ch {
            '%' => s.push_str("pct"),
            '+' => s.push_str("plus"),
            '-' => s.push_str("minus"),
            c if c.is_ascii_alphanumeric() || c == '_' => s.push(c),
            _ => s.push('_'),
        }
    }
    if !s.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        s.insert(0, '_');
    }
    s
}

/// Keys of the table constructor starting at `start`, with their lines.
fn keys_in_source(src: &SrcFile, start: usize) -> Result<(Vec<String>, usize)> {
    let end = (start + 1..=src.lines.len())
        .find(|&n| src.lines[n - 1] == "}")
        .ok_or_else(|| anyhow!("{}: end of table at line {start}", src.rel))?;
    let mut keys = Vec::new();
    for n in start + 1..end {
        let l = &src.lines[n - 1];
        let Some(rest) = l.strip_prefix('\t') else {
            continue;
        };
        if rest.starts_with(['\t', ' ', '-']) {
            continue;
        }
        let key = if let Some(r) = rest.strip_prefix("[\"") {
            let close = r
                .find("\"]")
                .ok_or_else(|| anyhow!("{}:{n}: unterminated key", src.rel))?;
            r[..close].to_owned()
        } else {
            let ident: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if ident.is_empty() || !rest[ident.len()..].trim_start().starts_with('=') {
                continue;
            }
            ident
        };
        keys.push(key);
    }
    Ok((keys, end))
}

fn float_lit(v: f64) -> Result<String> {
    if !v.is_finite() {
        bail!("non-finite constant {v}");
    }
    Ok(format!("{v:?}"))
}

fn record(out: &mut String, src: &SrcFile, data: &Table, name: &str, prefix: &str) -> Result<()> {
    let start = src
        .find(|l| l.starts_with(prefix))
        .ok_or_else(|| anyhow!("{}: `{prefix}` not found", src.rel))?;
    let (keys, end) = keys_in_source(src, start)?;
    let t: Table = data.get(name).map_err(|e| anyhow!("data.{name}: {e}"))?;
    let mut n_lua = 0;
    for p in t.clone().pairs::<Value, Value>() {
        p.map_err(lerr)?;
        n_lua += 1;
    }
    if n_lua != keys.len() {
        bail!(
            "data.{name}: {} keys in {}:{start}-{end}, {n_lua} in the loaded table",
            keys.len(),
            src.rel
        );
    }
    let mut fields = Vec::new();
    let mut seen = HashSet::new();
    for k in &keys {
        let v: Value = t.raw_get(k.as_str()).map_err(lerr)?;
        let v = match v {
            Value::Number(n) => n,
            Value::Integer(i) => i as f64,
            other => bail!(
                "data.{name}[{k:?}] is a {}, not a number",
                other.type_name()
            ),
        };
        let f = field_name(k);
        if !seen.insert(f.clone()) {
            bail!("data.{name}: two keys map to field {f}");
        }
        fields.push((k.clone(), f, v));
    }
    let _ = writeln!(out, "{}", src.marker(start, end));
    let _ = writeln!(
        out,
        "/// `data.{name}` ({}:{start}-{end}): one field per Lua key, in source order.",
        src.rel
    );
    let _ = writeln!(
        out,
        "#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]"
    );
    let _ = writeln!(out, "#[serde(deny_unknown_fields)]");
    let _ = writeln!(out, "pub struct {name} {{");
    for (k, f, _) in &fields {
        if f != k {
            let _ = writeln!(out, "    #[serde(rename = {k:?})]");
        }
        let _ = writeln!(out, "    pub {f}: f64,");
    }
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);
    let _ = writeln!(out, "impl {name} {{");
    let _ = writeln!(out, "    /// The verbatim Lua keys, in source order.");
    let _ = writeln!(out, "    pub const KEYS: &'static [&'static str] = &[");
    for (k, _, _) in &fields {
        let _ = writeln!(out, "        {k:?},");
    }
    let _ = writeln!(out, "    ];");
    let _ = writeln!(out);
    let _ = writeln!(out, "    /// The values PoB loads at the pin.");
    let _ = writeln!(out, "    pub const PINNED: Self = Self {{");
    for (_, f, v) in &fields {
        let _ = writeln!(out, "        {f}: {},", float_lit(*v)?);
    }
    let _ = writeln!(out, "    }};");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "    /// `data.{name}[key]` by verbatim Lua key (`nil` -> `None`)."
    );
    let _ = writeln!(out, "    pub fn get(&self, key: &str) -> Option<f64> {{");
    let _ = writeln!(out, "        match key {{");
    for (k, f, _) in &fields {
        let _ = writeln!(out, "            {k:?} => Some(self.{f}),");
    }
    let _ = writeln!(out, "            _ => None,");
    let _ = writeln!(out, "        }}");
    let _ = writeln!(out, "    }}");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);
    Ok(())
}

/// Render `generated/misc.rs` from a booted PoB state.
pub fn render(pob_src: &std::path::Path, env: &Env, header: &str, pin: &str) -> Result<String> {
    let data: Table = env
        .lua
        .globals()
        .get("data")
        .map_err(|e| anyhow!("data: {e}"))?;
    let mut out = String::new();
    let mut files: Vec<&str> = TABLES.iter().map(|t| t.1).collect();
    files.dedup();
    for f in &files {
        let _ = writeln!(out, "//! @pob-file {f} @{pin}");
    }
    out.push_str(header);
    let _ = writeln!(
        out,
        "#![allow(non_upper_case_globals, non_snake_case, non_camel_case_types, dead_code, clippy::all, clippy::pedantic)]"
    );
    let _ = writeln!(out);
    for (name, file, prefix) in TABLES {
        let src = SrcFile::load(pob_src, file, pin)?;
        record(&mut out, &src, &data, name, prefix)?;
    }
    out.truncate(out.trim_end().len());
    out.push('\n');
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::field_name;

    #[test]
    fn field_names() {
        assert_eq!(field_name("level"), "level");
        assert_eq!(field_name("ServerTickTime"), "ServerTickTime");
        assert_eq!(
            field_name("base_maximum_all_resistances_%"),
            "base_maximum_all_resistances_pct"
        );
        assert_eq!(
            field_name("damage_taken_when_hit_+%_final_per_fortification"),
            "damage_taken_when_hit_pluspct_final_per_fortification"
        );
        assert_eq!(field_name("type"), "r#type");
        assert_eq!(field_name("1x"), "_1x");
    }
}
