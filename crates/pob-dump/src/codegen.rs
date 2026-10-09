//! Code generation from PoB's loaded data.
//!
//! * `enums.rs` (`--codegen-enums`): `SkillType`, `ModFlag`, `KeywordFlag`
//!   constants read from `Data/Global.lua` as PoB itself loads it;
//! * `global.rs`: the same tables plus `MatchKeywordFlags`, `SkillTypeExpr`,
//!   `SkillTypeSet` and `colorCodes`, in the shape of calc-architecture.md
//!   section 4.2 (U0 moves it to `pob-gamedata/src/generated/`);
//! * `misc.rs`: verbatim-key record structs for `data.misc`,
//!   `data.gameConstants`, `data.characterConstants`, `data.monsterConstants`.
//!
//! `--codegen` writes all of them into `crates/pob-dump/generated/`;
//! `--codegen --check` fails when a committed file differs.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

mod global;
pub mod misc;
pub mod sha1;

use anyhow::{anyhow, bail, Result};
use mlua::Table;

use crate::Env;

/// `(name, value)` pairs of one enum table, sorted by value then name.
pub type Entries = Vec<(String, u32)>;

pub struct Enums {
    pub skill_type: Entries,
    pub mod_flag: Entries,
    pub keyword_flag: Entries,
}

fn entries(t: &Table, what: &str) -> Result<Entries> {
    let mut v = Vec::new();
    for pair in t.pairs::<String, f64>() {
        let (k, n) = pair.map_err(|e| anyhow!("{what}: {e}"))?;
        if n.fract() != 0.0 || !(0.0..=f64::from(u32::MAX)).contains(&n) {
            bail!("{what}.{k} = {n} is not a u32");
        }
        v.push((k, n as u32));
    }
    v.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    Ok(v)
}

/// Load `Data/Global.lua` (after `Modules/Common`, which it needs) and read
/// the three enum tables.
pub fn read_enums(pob_src: &Path) -> Result<Enums> {
    let env = Env::new(pob_src)?;
    env.lua
        .load(
            r#"
dofile("_SimpleGraphic.def.lua")
launch = { devMode = false }
LoadModule("Modules/Common")
LoadModule("Data/Global")
"#,
        )
        .set_name("=pob-dump/codegen")
        .exec()
        .map_err(|e| anyhow!("loading Global.lua: {e}"))?;
    let g = env.lua.globals();
    let get = |name: &str| -> Result<Entries> {
        let t: Table = g.get(name).map_err(|e| anyhow!("{name}: {e}"))?;
        entries(&t, name)
    };
    Ok(Enums {
        skill_type: get("SkillType")?,
        mod_flag: get("ModFlag")?,
        keyword_flag: get("KeywordFlag")?,
    })
}

fn module(out: &mut String, name: &str, lua_name: &str, doc: &str, e: &Entries, is_flags: bool) {
    let _ = writeln!(out, "/// PoB `{lua_name}` ({doc}).");
    let _ = writeln!(out, "pub mod {name} {{");
    for (k, v) in e {
        if is_flags {
            let _ = writeln!(out, "    pub const {k}: u32 = 0x{v:08X};");
        } else {
            let _ = writeln!(out, "    pub const {k}: u32 = {v};");
        }
    }
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "    /// `(name, value)` for every entry, ordered by value then name."
    );
    let _ = writeln!(out, "    pub const ENTRIES: &[(&str, u32)] = &[");
    for (k, v) in e {
        if is_flags {
            let _ = writeln!(out, "        (\"{k}\", 0x{v:08X}),");
        } else {
            let _ = writeln!(out, "        (\"{k}\", {v}),");
        }
    }
    let _ = writeln!(out, "    ];");
    let _ = writeln!(out);
    if !is_flags {
        let max = e.iter().map(|x| x.1).max().unwrap_or(0);
        let _ = writeln!(
            out,
            "    /// Largest value (PoB numbers skill types 1..=MAX)."
        );
        let _ = writeln!(out, "    pub const MAX: u32 = {max};");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "    /// Names indexed by value; index 0 and unused values are `\"\"`."
        );
        let _ = writeln!(out, "    pub const NAMES: [&str; {}] = [", max + 1);
        for i in 0..=max {
            let n = e.iter().find(|x| x.1 == i).map_or("", |x| x.0.as_str());
            let _ = writeln!(out, "        \"{n}\",");
        }
        let _ = writeln!(out, "    ];");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "    pub fn name(value: u32) -> Option<&'static str> {{"
        );
        let _ = writeln!(
            out,
            "        NAMES.get(value as usize).copied().filter(|n| !n.is_empty())"
        );
        let _ = writeln!(out, "    }}");
    } else {
        let _ = writeln!(
            out,
            "    /// Names of the single-bit entries set in `value`."
        );
        let _ = writeln!(out, "    pub fn names(value: u32) -> Vec<&'static str> {{");
        let _ = writeln!(out, "        ENTRIES");
        let _ = writeln!(out, "            .iter()");
        let _ = writeln!(
            out,
            "            .filter(|(_, v)| v.is_power_of_two() && value & v != 0)"
        );
        let _ = writeln!(out, "            .map(|(n, _)| *n)");
        let _ = writeln!(out, "            .collect()");
        let _ = writeln!(out, "    }}");
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "    pub fn from_name(name: &str) -> Option<u32> {{");
    let _ = writeln!(
        out,
        "        ENTRIES.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)"
    );
    let _ = writeln!(out, "    }}");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);
}

/// Rust source for the three enums.
pub fn render(e: &Enums, pob_version: Option<&str>) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "// @generated by `pob-dump --codegen-enums` from Path of Building {}'s Data/Global.lua.",
        pob_version.unwrap_or("(unknown version)")
    );
    let _ = writeln!(out, "// Do not edit by hand; regenerate instead.");
    let _ = writeln!(
        out,
        "#![allow(non_upper_case_globals, dead_code, clippy::all, clippy::pedantic)]"
    );
    let _ = writeln!(out);
    module(
        &mut out,
        "skill_type",
        "SkillType",
        "skill type ids; keys of skillTypes/addSkillTypes/excludeSkillTypes",
        &e.skill_type,
        false,
    );
    module(
        &mut out,
        "mod_flag",
        "ModFlag",
        "bit flags of Mod.flags",
        &e.mod_flag,
        true,
    );
    module(
        &mut out,
        "keyword_flag",
        "KeywordFlag",
        "bit flags of Mod.keywordFlags",
        &e.keyword_flag,
        true,
    );
    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}

pub fn codegen_enums(pob_src: &Path) -> Result<String> {
    let e = read_enums(pob_src)?;
    let ver = crate::pob_version_of(pob_src);
    Ok(render(&e, ver.as_deref()))
}

// ---------------------------------------------------------------------------
// `--codegen`: every generated file, and `--codegen --check`.

/// A PoB source file, for provenance markers.
pub struct SrcFile {
    /// Path relative to PoB's `src/`.
    pub rel: String,
    /// Lines as on disk, CR stripped.
    pub lines: Vec<String>,
    /// Short commit of the pin.
    pub pin: String,
}

impl SrcFile {
    pub fn load(pob_src: &Path, rel: &str, pin: &str) -> Result<Self> {
        let text = std::fs::read_to_string(pob_src.join(rel)).map_err(|e| anyhow!("{rel}: {e}"))?;
        Ok(Self {
            rel: rel.to_owned(),
            lines: text
                .split('\n')
                .map(|l| l.trim_end_matches('\r').to_owned())
                .collect(),
            pin: pin.to_owned(),
        })
    }

    /// First line (1-based) satisfying `pred`.
    pub fn find(&self, pred: impl Fn(&str) -> bool) -> Option<usize> {
        self.lines.iter().position(|l| pred(l)).map(|i| i + 1)
    }

    /// `@pob` hash of lines `a..=b` (calc-architecture.md section 3.1 rule 5):
    /// first 8 hex digits of SHA-1 over the lines, trailing whitespace
    /// stripped, joined with `\n`.
    pub fn hash(&self, a: usize, b: usize) -> String {
        let body = self.lines[a - 1..b]
            .iter()
            .map(|l| l.trim_end())
            .collect::<Vec<_>>()
            .join("\n");
        sha1::sha1_hex(body.as_bytes())[..8].to_owned()
    }

    pub fn marker(&self, a: usize, b: usize) -> String {
        format!("/// @pob {}:{a}-{b} h={}", self.rel, self.hash(a, b))
    }

    pub fn file_marker(&self) -> String {
        format!("//! @pob-file {} @{}", self.rel, self.pin)
    }
}

/// The short (9 hex digit) commit of PoB's checkout, as the docs write the pin.
pub fn pin_of(pob_src: &Path) -> String {
    crate::pob_commit_of(pob_src)
        .map(|c| c.chars().take(9).collect())
        .unwrap_or_else(|| "unknown".into())
}

/// Files `--codegen` writes into the generated directory.
pub const GENERATED: &[&str] = &["enums.rs", "global.rs", "misc.rs"];

/// `crates/pob-dump/generated`.
pub fn default_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("generated")
}

/// Contents of every generated file, as `(file name, source)`.
pub fn codegen_all(pob_src: &Path) -> Result<Vec<(&'static str, String)>> {
    let enums = codegen_enums(pob_src)?;
    let pin = pin_of(pob_src);
    let ver = crate::pob_version_of(pob_src).unwrap_or_else(|| "(unknown version)".into());
    let header = |what: &str| {
        format!(
            "//! GENERATED by `pob-dump --codegen` from {what} (Path of Building {ver}, {pin}). Do not edit;\n\
             //! regenerate with `cargo run -p pob-dump -- --codegen` (`--codegen --check` verifies).\n"
        )
    };
    let env = Env::new(pob_src)?;
    env.boot_pob()?;
    let global_src = SrcFile::load(pob_src, "Data/Global.lua", &pin)?;
    let global = global::render(&global_src, &env, &header("Data/Global.lua"))?;
    let misc = misc::render(
        pob_src,
        &env,
        &header("Modules/Data.lua `data.misc` and Data/Misc.lua"),
        &pin,
    )?;
    Ok(vec![
        ("enums.rs", enums),
        ("global.rs", global),
        ("misc.rs", misc),
    ])
}

/// Write every generated file into `dir`.
pub fn write_all(pob_src: &Path, dir: &Path) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let mut written = Vec::new();
    for (name, text) in codegen_all(pob_src)? {
        let p = dir.join(name);
        std::fs::write(&p, text).map_err(|e| anyhow!("write {}: {e}", p.display()))?;
        written.push(p);
    }
    Ok(written)
}

/// Generated files in `dir` that are missing or differ (byte for byte) from
/// what `--codegen` would write now.
pub fn check_all(pob_src: &Path, dir: &Path) -> Result<Vec<String>> {
    let mut stale = Vec::new();
    for (name, text) in codegen_all(pob_src)? {
        match std::fs::read(dir.join(name)) {
            Ok(on_disk) if on_disk == text.as_bytes() => {}
            Ok(_) => stale.push(format!("{name} differs")),
            Err(e) => stale.push(format!("{name}: {e}")),
        }
    }
    Ok(stale)
}
