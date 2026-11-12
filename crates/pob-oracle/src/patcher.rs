//! The oracle patcher (calc-architecture.md §9.2, "Patched source").
//!
//! Copies PoB's `src/` to `<root>/<sha>/src/` (patched files written, every
//! other file hard-linked, or copied where links are impossible) with
//! symlinks to every other entry of PoB's checkout next to it (`runtime/`,
//! where the oracle's loader looks for `runtime/lua`; `manifest.xml`, which
//! gives Launch.lua its version number), and applies, **without changing
//! any line number**:
//!
//! * **line probes** from a `stages.toml` (`[[stage]]` with `id`, `file`,
//!   `line`, `locals`, optional `expect`): the line gets the prefix
//!   `if __PROBE then __PROBE("<id>", { a = a, ... }) end `;
//! * **function probes**: the built-in list of §9.2 ([`builtin_function_probes`]:
//!   `calcs.initEnv`, `perform`, `defence`, `buildDefenceEstimations`,
//!   `offence`, `triggers`, `mirages`, `buildActiveSkillModList`,
//!   `createActiveSkill`, `runSkillFunc`, `getCachedOutputValue`,
//!   `ConfigTab:BuildModList`, every skill hook in `Data/Skills/*.lua`, every
//!   `ConfigOptions` apply, and the jewel-func call sites in `CalcSetup.lua`)
//!   plus any `[[function]]` entries (`id`, `file`, `line`) of the stages
//!   file. A definition `function a.b(p)` … `end` becomes
//!   `a.b = __PROBE_FN("<id>", {"p"}, function(p)` … `end)` (methods,
//!   `local function` and `field = function(…)` forms likewise); the
//!   matching `end` is found with a Lua tokenizer;
//! * **write provenance** (`lua/provenance.lua`): the creation sites of
//!   `actor.output`, `output.MainHand/OffHand/Minion`, `minion.output` and
//!   the breakdown table are wrapped in `__PROV(kind, …)`, and every
//!   statement of `Classes/ModDB.lua` / `Classes/ModList.lua` that stores a
//!   mod gets `if __PROV_MOD then __PROV_MOD(self, mod) end` appended.
//!
//! Every patched file is compiled with `loadstring` (after the same
//! compound-assignment rewrite the oracle's loader applies) before the copy
//! is published. The `<sha>` covers the patcher version, the probe runtime (`lua/*.lua`), the stages file,
//! the patched texts and a stat fingerprint of the whole source tree, so a
//! changed input never reuses a stale copy.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::oracle::{OracleError, Result};

/// Bumped whenever the patch rules change (part of the `<sha>`).
pub const PATCHER_VERSION: u32 = 2;

/// Format tag of `<root>/<sha>/manifest.json`.
pub const MANIFEST_FORMAT: &str = "pob-oracle-patch/1";

/// The Lua syntax check (`lua/validate.lua`).
pub const VALIDATE_LUA: &str = include_str!("../lua/validate.lua");

fn err(msg: impl Into<String>) -> OracleError {
    OracleError(msg.into())
}

// ---- stages.toml ---------------------------------------------------------------

/// `docs/port/stages.toml` (U0 writes the real one).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StagesFile {
    #[serde(default)]
    pub stage: Vec<StageSpec>,
    /// Extra function probes beyond the built-in list.
    #[serde(default)]
    pub function: Vec<FunctionSpec>,
}

/// A line probe: fires before `file:line` runs, dumping `locals` by name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageSpec {
    pub id: String,
    /// Relative to PoB's `src/`, e.g. `Modules/CalcPerform.lua`.
    pub file: String,
    /// 1-based line the probe fires before.
    pub line: usize,
    #[serde(default)]
    pub locals: Vec<String>,
    /// Text the original line must contain (guards against upstream drift).
    #[serde(default)]
    pub expect: Option<String>,
}

/// A function probe on the function whose definition starts on `line`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FunctionSpec {
    pub id: String,
    pub file: String,
    pub line: usize,
    #[serde(default)]
    pub expect: Option<String>,
}

impl StagesFile {
    pub fn parse(text: &str) -> Result<Self> {
        toml::from_str(text).map_err(|e| err(format!("stages.toml: {e}")))
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).map_err(|e| err(format!("{}: {e}", path.display())))?;
        Self::parse(&text)
    }
}

// ---- probe sites -------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SiteKind {
    /// `[[stage]]` line probe.
    Line,
    /// Function definition wrapped with `__PROBE_FN` (fires `:in` / `:out`).
    Function,
    /// Call site wrapped with `__PROBE_CALL` (fires `:in` / `:out`).
    Call,
    /// `__PROV` creation site of a proxied output / breakdown table.
    Provenance,
    /// `__PROV_MOD` after a statement that stores a mod.
    ModInsert,
}

/// One patched site, as listed in the manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeSite {
    pub id: String,
    pub kind: SiteKind,
    pub file: String,
    pub line: usize,
    /// Last line of a wrapped function.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<usize>,
    /// Parameter names a function probe dumps (`...` becomes `varargs`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<String>,
}

/// A text edit at original coordinates (1-based line, byte column).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit {
    pub line: usize,
    pub col: usize,
    /// Original text replaced (often empty).
    pub removed: String,
    pub inserted: String,
}

/// A file of the patch: original and patched text plus the edits between.
#[derive(Debug, Clone)]
pub struct PatchedFile {
    pub file: String,
    pub original: String,
    pub patched: String,
    pub edits: Vec<Edit>,
}

/// What [`plan`] produced (in memory, nothing written yet).
#[derive(Debug, Clone)]
pub struct Plan {
    pub stages: StagesFile,
    pub sites: Vec<ProbeSite>,
    pub files: Vec<PatchedFile>,
}

// ---- Lua tokenizer (just enough to match blocks) ---------------------------------------------

/// A word token (identifier or keyword) with its position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Word<'a> {
    pub line: usize,
    pub col: usize,
    pub text: &'a str,
}

/// The identifier / keyword tokens of a Lua source, skipping strings,
/// comments and numbers. Lines are 1-based, columns are byte offsets.
pub fn lua_words(src: &str) -> Result<Vec<Word<'_>>> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut line = 1usize;
    let mut line_start = 0usize;
    // `[` `=`* `[` at i: the level, or None
    let long_open = |i: usize| -> Option<usize> {
        if b.get(i) != Some(&b'[') {
            return None;
        }
        let mut j = i + 1;
        while b.get(j) == Some(&b'=') {
            j += 1;
        }
        (b.get(j) == Some(&b'[')).then_some(j - i - 1)
    };
    while i < b.len() {
        let c = b[i];
        match c {
            b'\n' => {
                i += 1;
                line += 1;
                line_start = i;
            }
            b'-' if b.get(i + 1) == Some(&b'-') => {
                if let Some(level) = long_open(i + 2) {
                    i = skip_long(b, i + 2, level, &mut line, &mut line_start)
                        .ok_or_else(|| err(format!("unterminated long comment at line {line}")))?;
                } else {
                    while i < b.len() && b[i] != b'\n' {
                        i += 1;
                    }
                }
            }
            b'[' if long_open(i).is_some() => {
                let level = long_open(i).unwrap_or(0);
                i = skip_long(b, i, level, &mut line, &mut line_start)
                    .ok_or_else(|| err(format!("unterminated long string at line {line}")))?;
            }
            b'"' | b'\'' => {
                let q = c;
                i += 1;
                loop {
                    match b.get(i) {
                        None => return Err(err(format!("unterminated string at line {line}"))),
                        Some(&x) if x == q => {
                            i += 1;
                            break;
                        }
                        Some(b'\\') => {
                            if b.get(i + 1) == Some(&b'\n') {
                                line += 1;
                                line_start = i + 2;
                            }
                            i += 2;
                        }
                        Some(b'\n') => {
                            return Err(err(format!("unfinished string at line {line}")));
                        }
                        Some(_) => i += 1,
                    }
                }
            }
            b'0'..=b'9' => {
                i += 1;
                while i < b.len() {
                    let x = b[i];
                    let exponent_sign =
                        (x == b'+' || x == b'-') && matches!(b[i - 1], b'e' | b'E' | b'p' | b'P');
                    if x.is_ascii_alphanumeric() || x == b'.' || x == b'_' || exponent_sign {
                        i += 1;
                    } else {
                        break;
                    }
                }
            }
            b'.' if b.get(i + 1).is_some_and(u8::is_ascii_digit) => {
                i += 1;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'.') {
                    i += 1;
                }
            }
            c if c.is_ascii_alphabetic() || c == b'_' => {
                let start = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                    i += 1;
                }
                out.push(Word {
                    line,
                    col: start - line_start,
                    text: &src[start..i],
                });
            }
            _ => i += 1,
        }
    }
    Ok(out)
}

/// Skip a long bracket starting at `i` (`[` of `[==[`); returns the index
/// after the closing bracket.
fn skip_long(
    b: &[u8],
    i: usize,
    level: usize,
    line: &mut usize,
    line_start: &mut usize,
) -> Option<usize> {
    let mut j = i + level + 2;
    while j < b.len() {
        if b[j] == b'\n' {
            *line += 1;
            *line_start = j + 1;
        } else if b[j] == b']' {
            let mut k = j + 1;
            while k < b.len() && b[k] == b'=' {
                k += 1;
            }
            if k - j - 1 == level && b.get(k) == Some(&b']') {
                return Some(k + 1);
            }
        }
        j += 1;
    }
    None
}

/// Index (into `words`) of the `end` closing the `function` keyword at
/// `words[fi]`.
pub fn matching_end(words: &[Word<'_>], fi: usize) -> Result<usize> {
    let mut depth = 0i64;
    for (k, w) in words.iter().enumerate().skip(fi) {
        match w.text {
            "function" | "do" | "if" | "repeat" => depth += 1,
            "end" | "until" => {
                depth -= 1;
                if depth == 0 {
                    if w.text == "until" {
                        break;
                    }
                    return Ok(k);
                }
            }
            _ => {}
        }
    }
    Err(err(format!(
        "no matching `end` for the function at line {}",
        words[fi].line
    )))
}

// ---- PoB's compound assignment (`a += 1`) --------------------------------------------------

/// The rewrite the oracle's loader applies to a line (jit.rs PRELUDE,
/// `rewrite_line`): `lhs op= rhs` -> `lhs = lhs op (rhs)`. Returns `None`
/// when the line is not a compound assignment. Mirrors the Lua pattern
/// `^([ \t]*)([%a_][%w_%.%[%]"']*)[ \t]*([%+%-%*/%%%.]%.?)=[ \t]*(.-)[ \t]*\r?$`
/// including its backtracking.
pub fn compound_rewrite(line: &str) -> Option<String> {
    let b = line.as_bytes();
    let ind_end = b
        .iter()
        .position(|&c| c != b' ' && c != b'\t')
        .unwrap_or(b.len());
    let first = *b.get(ind_end)?;
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return None;
    }
    let lhs_class =
        |c: u8| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'[' | b']' | b'"' | b'\'');
    let mut lhs_max = ind_end + 1;
    while lhs_max < b.len() && lhs_class(b[lhs_max]) {
        lhs_max += 1;
    }
    let op_class = |c: u8| matches!(c, b'+' | b'-' | b'*' | b'/' | b'%' | b'.');
    for lhs_end in (ind_end + 1..=lhs_max).rev() {
        let mut j = lhs_end;
        while j < b.len() && (b[j] == b' ' || b[j] == b'\t') {
            j += 1;
        }
        if j >= b.len() || !op_class(b[j]) {
            continue;
        }
        let op_start = j;
        j += 1;
        // `%.?` then `=`: try with the optional dot, then without
        let mut cands = Vec::new();
        if b.get(j) == Some(&b'.') && b.get(j + 1) == Some(&b'=') {
            cands.push(j + 1);
        }
        if b.get(j) == Some(&b'=') {
            cands.push(j);
        }
        let Some(&eq) = cands.first() else { continue };
        let op = &line[op_start..eq];
        let mut r = eq + 1;
        while r < b.len() && (b[r] == b' ' || b[r] == b'\t') {
            r += 1;
        }
        let mut rhs = &line[r..];
        rhs = rhs.strip_suffix('\r').unwrap_or(rhs);
        rhs = rhs.trim_end_matches([' ', '\t']);
        // after `\r?$` the pattern's `(.-)[ \t]*` trims trailing blanks
        if !matches!(op, "+" | "-" | "*" | "/" | "%" | "..") || rhs.is_empty() {
            return None;
        }
        let (code, comment) = match rhs.find("--") {
            Some(p) => (rhs[..p].trim_end_matches([' ', '\t']), &rhs[p..]),
            None => (rhs, ""),
        };
        let lhs = &line[ind_end..lhs_end];
        let ind = &line[..ind_end];
        let mut out = format!("{ind}{lhs} = {lhs} {op} ({code})");
        if !comment.is_empty() {
            out.push(' ');
            out.push_str(comment);
        }
        return Some(out);
    }
    None
}

// ---- built-in sites ----------------------------------------------------------------------------

/// The fixed function probes of §9.2: (file, definition prefix, id).
pub const NAMED_FUNCTION_PROBES: &[(&str, &str, &str)] = &[
    (
        "Modules/CalcSetup.lua",
        "function calcs.initEnv(",
        "initEnv",
    ),
    (
        "Modules/CalcPerform.lua",
        "function calcs.perform(",
        "perform",
    ),
    (
        "Modules/CalcDefence.lua",
        "function calcs.defence(",
        "defence",
    ),
    (
        "Modules/CalcDefence.lua",
        "function calcs.buildDefenceEstimations(",
        "buildDefenceEstimations",
    ),
    (
        "Modules/CalcOffence.lua",
        "function calcs.offence(",
        "offence",
    ),
    (
        "Modules/CalcTriggers.lua",
        "function calcs.triggers(",
        "triggers",
    ),
    (
        "Modules/CalcMirages.lua",
        "function calcs.mirages(",
        "mirages",
    ),
    (
        "Modules/CalcActiveSkill.lua",
        "function calcs.buildActiveSkillModList(",
        "buildActiveSkillModList",
    ),
    (
        "Modules/CalcActiveSkill.lua",
        "function calcs.createActiveSkill(",
        "createActiveSkill",
    ),
    (
        "Modules/CalcOffence.lua",
        "local function runSkillFunc(",
        "runSkillFunc",
    ),
    (
        "Modules/CalcPerform.lua",
        "local function getCachedOutputValue(",
        "getCachedOutputValue",
    ),
    (
        "Classes/ConfigTab.lua",
        "function ConfigTabClass:BuildModList(",
        "ConfigTab:BuildModList",
    ),
    (
        "Modules/ConfigOptions.lua",
        "local function mapAffixDropDownFunction(",
        "config:mapAffixDropDownFunction",
    ),
];

/// Jewel funcs are closures built by ModParser; they are probed where
/// CalcSetup calls them: (file, call prefix, id, params, expected count).
pub const CALL_PROBES: &[(&str, &str, &str, &[&str], usize)] = &[(
    "Modules/CalcSetup.lua",
    "rad.func(",
    "jewelFunc",
    &["node", "out", "data"],
    3,
)];

/// Provenance proxies: (file, exact trimmed line, replaced text, kind).
pub const PROVENANCE_SITES: &[(&str, &str, &str, &str)] = &[
    (
        "Modules/CalcPerform.lua",
        "env.player.output = { }",
        "{ }",
        "output",
    ),
    (
        "Modules/CalcPerform.lua",
        "env.enemy.output = {}",
        "{}",
        "enemy.output",
    ),
    (
        "Modules/CalcPerform.lua",
        "output.Minion = { }",
        "{ }",
        "output.Minion",
    ),
    (
        "Modules/CalcPerform.lua",
        "minion.output = output or { }",
        "output or { }",
        "minion.output",
    ),
    (
        "Modules/CalcOffence.lua",
        "output.MainHand = { }",
        "{ }",
        "output.MainHand",
    ),
    (
        "Modules/CalcOffence.lua",
        "output.OffHand = { }",
        "{ }",
        "output.OffHand",
    ),
    (
        "Modules/CalcBreakdown.lua",
        "local breakdown = {}",
        "{}",
        "breakdown",
    ),
];

/// Statements that store a mod: (file, exact trimmed line, mod expression,
/// expected count).
pub const MOD_INSERT_SITES: &[(&str, &str, &str, usize)] = &[
    ("Classes/ModList.lua", "t_insert(self, mod)", "mod", 1),
    ("Classes/ModList.lua", "self[i] = mod", "mod", 2),
    (
        "Classes/ModList.lua",
        "t_insert(self, modList[i])",
        "modList[i]",
        1,
    ),
    (
        "Classes/ModDB.lua",
        "t_insert(self.mods[name], mod)",
        "mod",
        1,
    ),
    ("Classes/ModDB.lua", "modList[modIndex] = mod", "mod", 1),
    (
        "Classes/ModDB.lua",
        "t_insert(self.mods[newName], mod)",
        "mod",
        1,
    ),
    ("Classes/ModDB.lua", "t_insert(mods[name], mod)", "mod", 1),
    (
        "Classes/ModDB.lua",
        "t_insert(modsName, modList[i])",
        "modList[i]",
        1,
    ),
];

/// A function probe to place: the first `function` keyword on `line` at
/// or after byte column `col`.
#[derive(Debug, Clone)]
struct FnTarget {
    id: String,
    file: String,
    line: usize,
    col: usize,
}

/// Block-structure tokens of a file (owned, cached per file).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Function,
    Open,
    End,
    Until,
}

#[derive(Debug, Clone, Copy)]
struct BlockTok {
    line: usize,
    col: usize,
    kind: BlockKind,
}

fn block_tokens(src: &str) -> Result<Vec<BlockTok>> {
    Ok(lua_words(src)?
        .into_iter()
        .filter_map(|w| {
            let kind = match w.text {
                "function" => BlockKind::Function,
                "do" | "if" | "repeat" => BlockKind::Open,
                "end" => BlockKind::End,
                "until" => BlockKind::Until,
                _ => return None,
            };
            Some(BlockTok {
                line: w.line,
                col: w.col,
                kind,
            })
        })
        .collect())
}

/// Sort order of edits: by position; at the same position pure insertions
/// come before replacements.
fn edit_order(a: &Edit, b: &Edit) -> std::cmp::Ordering {
    (a.line, a.col, !a.removed.is_empty()).cmp(&(b.line, b.col, !b.removed.is_empty()))
}

fn lua_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn is_ident(s: &str) -> bool {
    let mut cs = s.chars();
    cs.next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

struct Source {
    file: String,
    text: String,
    /// Byte offset of each line start.
    starts: Vec<usize>,
}

impl Source {
    fn new(file: &str, text: String) -> Self {
        let mut starts = vec![0];
        for (i, c) in text.bytes().enumerate() {
            if c == b'\n' {
                starts.push(i + 1);
            }
        }
        Self {
            file: file.to_owned(),
            text,
            starts,
        }
    }

    fn line_count(&self) -> usize {
        // a trailing newline does not start a line
        if self.text.ends_with('\n') {
            self.starts.len() - 1
        } else {
            self.starts.len()
        }
    }

    /// Line `n` (1-based) without its newline (and without `\r`).
    fn line(&self, n: usize) -> Option<&str> {
        if n == 0 || n > self.line_count() {
            return None;
        }
        let a = self.starts[n - 1];
        let b = self.starts.get(n).map_or(self.text.len(), |&e| e - 1);
        let l = &self.text[a..b];
        Some(l.strip_suffix('\r').unwrap_or(l))
    }
}

/// Collects edits per file and the probe sites.
struct Planner<'a> {
    pob_src: &'a Path,
    sources: BTreeMap<String, Source>,
    blocks: BTreeMap<String, Vec<BlockTok>>,
    edits: BTreeMap<String, Vec<Edit>>,
    sites: Vec<ProbeSite>,
    ids: BTreeSet<String>,
}

impl<'a> Planner<'a> {
    fn new(pob_src: &'a Path) -> Self {
        Self {
            pob_src,
            sources: BTreeMap::new(),
            blocks: BTreeMap::new(),
            edits: BTreeMap::new(),
            sites: Vec::new(),
            ids: BTreeSet::new(),
        }
    }

    fn source(&mut self, file: &str) -> Result<&Source> {
        if !self.sources.contains_key(file) {
            if file.contains("..") || Path::new(file).is_absolute() {
                return Err(err(format!("{file}: paths are relative to PoB's src/")));
            }
            let path = self.pob_src.join(file);
            let text = std::fs::read_to_string(&path)
                .map_err(|e| err(format!("{}: {e}", path.display())))?;
            self.sources
                .insert(file.to_owned(), Source::new(file, text));
        }
        Ok(&self.sources[file])
    }

    fn line_text(&mut self, file: &str, line: usize) -> Result<String> {
        let src = self.source(file)?;
        src.line(line)
            .map(str::to_owned)
            .ok_or_else(|| err(format!("{file}:{line}: no such line")))
    }

    fn push_edit(&mut self, file: &str, e: Edit) {
        self.edits.entry(file.to_owned()).or_default().push(e);
    }

    /// A unique probe id (`<id>@<line>` on collision).
    fn unique_id(&mut self, id: &str, line: usize) -> String {
        let mut id = id.to_owned();
        if self.ids.contains(&id) {
            id = format!("{id}@{line}");
        }
        self.ids.insert(id.clone());
        id
    }

    fn add_stage(&mut self, st: &StageSpec) -> Result<()> {
        if st.id.is_empty() || st.id.contains(['"', '\\', '\n', '#']) {
            return Err(err(format!(
                "stage id {:?}: must be non-empty without '\"', '\\\\', '#'",
                st.id
            )));
        }
        if !self.ids.insert(st.id.clone()) {
            return Err(err(format!("duplicate probe id {}", st.id)));
        }
        let text = self.line_text(&st.file, st.line)?;
        if let Some(expect) = &st.expect {
            if !text.contains(expect.as_str()) {
                return Err(err(format!(
                    "{}:{}: expected {expect:?}, found {text:?}",
                    st.file, st.line
                )));
            }
        }
        let mut fields = Vec::new();
        for l in &st.locals {
            if !is_ident(l) {
                return Err(err(format!(
                    "stage {}: local {l:?} is not an identifier",
                    st.id
                )));
            }
            fields.push(format!("{l} = {l}"));
        }
        let mut prefix = format!(
            "if __PROBE then __PROBE({}, {{ {} }}) end ",
            lua_quote(&st.id),
            fields.join(", ")
        );
        if fields.is_empty() {
            prefix = format!("if __PROBE then __PROBE({}, {{ }}) end ", lua_quote(&st.id));
        }
        // The loader rewrites `a += b` only at the start of a line; do it
        // here for a line that gets a prefix.
        if let Some(rewritten) = compound_rewrite(&text) {
            self.push_edit(
                &st.file,
                Edit {
                    line: st.line,
                    col: 0,
                    removed: text.clone(),
                    inserted: format!("{prefix}{rewritten}"),
                },
            );
        } else {
            self.push_edit(
                &st.file,
                Edit {
                    line: st.line,
                    col: 0,
                    removed: String::new(),
                    inserted: prefix,
                },
            );
        }
        self.sites.push(ProbeSite {
            id: st.id.clone(),
            kind: SiteKind::Line,
            file: st.file.clone(),
            line: st.line,
            end_line: None,
            params: st.locals.clone(),
        });
        Ok(())
    }

    fn blocks(&mut self, file: &str) -> Result<&[BlockTok]> {
        if !self.blocks.contains_key(file) {
            self.source(file)?;
            let toks = block_tokens(&self.sources[file].text)
                .map_err(|e| err(format!("{file}: {}", e.0)))?;
            self.blocks.insert(file.to_owned(), toks);
        }
        Ok(&self.blocks[file])
    }

    /// Wrap the function whose `function` keyword is the first on
    /// `target.line` at or after `target.col`.
    fn add_function(&mut self, target: &FnTarget) -> Result<()> {
        let file = target.file.clone();
        let line = target.line;
        let toks = self.blocks(&file)?.to_vec();
        let text = self.line_text(&file, line)?;
        let fi = toks
            .iter()
            .position(|w| w.line == line && w.col >= target.col && w.kind == BlockKind::Function)
            .ok_or_else(|| err(format!("{file}:{line}: no `function` keyword on the line")))?;
        let mut depth = 0i64;
        let mut ei = None;
        for (k, w) in toks.iter().enumerate().skip(fi) {
            match w.kind {
                BlockKind::Function | BlockKind::Open => depth += 1,
                BlockKind::End | BlockKind::Until => {
                    depth -= 1;
                    if depth == 0 {
                        if w.kind == BlockKind::End {
                            ei = Some(k);
                        }
                        break;
                    }
                }
            }
        }
        let ei = ei.ok_or_else(|| err(format!("{file}:{line}: no matching `end`")))?;
        let fw = toks[fi];
        let ew = toks[ei];
        let open = text[fw.col..]
            .find('(')
            .map(|p| fw.col + p)
            .ok_or_else(|| err(format!("{file}:{line}: no `(` after `function`")))?;
        let close = text[open..].find(')').map(|p| open + p).ok_or_else(|| {
            err(format!(
                "{file}:{line}: parameter list must close on the definition line"
            ))
        })?;
        let params_text = &text[open + 1..close];
        let mut params: Vec<String> = params_text
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty() && *p != "...")
            .map(str::to_owned)
            .collect();
        let name = text[fw.col + "function".len()..open].trim().to_owned();
        let before = &text[..fw.col];
        let id = self.unique_id(&target.id, line);
        // (start column, replaced text, replacement) for the head
        let (start, head) = if name.is_empty() {
            // `field = function(` / `return function(` / argument
            if !before.trim_end().ends_with('=') {
                return Err(err(format!(
                    "{file}:{line}: anonymous function not assigned to a field"
                )));
            }
            (
                fw.col,
                format!(
                    "__PROBE_FN({}, {{{}}}, function(",
                    lua_quote(&id),
                    quoted_list(&params)
                ),
            )
        } else if before.trim_end().ends_with("local") {
            let local_col = before.trim_end().len() - "local".len();
            if !is_ident(&name) {
                return Err(err(format!(
                    "{file}:{line}: bad local function name {name:?}"
                )));
            }
            (
                local_col,
                format!(
                    "local {name}; {name} = __PROBE_FN({}, {{{}}}, function(",
                    lua_quote(&id),
                    quoted_list(&params)
                ),
            )
        } else if before.trim().is_empty() {
            if let Some((obj, method)) = name.split_once(':') {
                params.insert(0, "self".to_owned());
                let self_sep = if params_text.trim().is_empty() {
                    "self"
                } else {
                    "self, "
                };
                (
                    fw.col,
                    format!(
                        "{obj}.{method} = __PROBE_FN({}, {{{}}}, function({self_sep}",
                        lua_quote(&id),
                        quoted_list(&params)
                    ),
                )
            } else {
                (
                    fw.col,
                    format!(
                        "{name} = __PROBE_FN({}, {{{}}}, function(",
                        lua_quote(&id),
                        quoted_list(&params)
                    ),
                )
            }
        } else {
            return Err(err(format!(
                "{file}:{line}: unsupported function definition form"
            )));
        };
        let removed = text[start..=open].to_owned();
        self.push_edit(
            &file,
            Edit {
                line,
                col: start,
                removed,
                inserted: head,
            },
        );
        self.push_edit(
            &file,
            Edit {
                line: ew.line,
                col: ew.col + 3,
                removed: String::new(),
                inserted: ")".to_owned(),
            },
        );
        self.sites.push(ProbeSite {
            id,
            kind: SiteKind::Function,
            file,
            line,
            end_line: Some(ew.line),
            params,
        });
        Ok(())
    }

    fn add_named_functions(&mut self) -> Result<()> {
        for (file, prefix, id) in NAMED_FUNCTION_PROBES {
            self.source(file)?;
            let src = &self.sources[*file];
            let hits: Vec<usize> = (1..=src.line_count())
                .filter(|&n| {
                    src.line(n)
                        .is_some_and(|l| l.trim_start().starts_with(prefix))
                })
                .collect();
            if hits.len() != 1 {
                return Err(err(format!(
                    "{file}: expected one line starting with {prefix:?}, found {}",
                    hits.len()
                )));
            }
            self.add_function(&FnTarget {
                id: (*id).to_owned(),
                file: (*file).to_owned(),
                line: hits[0],
                col: 0,
            })?;
        }
        Ok(())
    }

    /// `<hook>Func = function(` inside `skills["<id>"] = {` in Data/Skills.
    fn add_skill_hooks(&mut self) -> Result<usize> {
        let dir = self.pob_src.join("Data/Skills");
        let mut files: Vec<String> = std::fs::read_dir(&dir)
            .map_err(|e| err(format!("{}: {e}", dir.display())))?
            .filter_map(std::result::Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".lua"))
            .map(|n| format!("Data/Skills/{n}"))
            .collect();
        files.sort();
        let mut n = 0;
        for file in files {
            self.source(&file)?;
            let src = &self.sources[&file];
            let mut targets = Vec::new();
            let mut skill: Option<String> = None;
            for ln in 1..=src.line_count() {
                let l = src.line(ln).unwrap_or_default();
                if let Some(rest) = l.strip_prefix("skills[\"") {
                    if let Some(end) = rest.find("\"]") {
                        skill = Some(rest[..end].to_owned());
                    }
                }
                let indent = l.len() - l.trim_start().len();
                let t = l.trim_start();
                let Some(eq) = t.find('=') else { continue };
                let field = t[..eq].trim();
                let after = t[eq + 1..].trim_start();
                if field.ends_with("Func") && is_ident(field) && after.starts_with("function") {
                    let rest = after["function".len()..].trim_start();
                    if !rest.starts_with('(') {
                        continue;
                    }
                    let skill = skill.clone().ok_or_else(|| {
                        err(format!(
                            "{file}:{ln}: skill hook outside a skills[...] entry"
                        ))
                    })?;
                    targets.push(FnTarget {
                        id: format!("skill:{skill}:{field}"),
                        file: file.clone(),
                        line: ln,
                        col: indent + eq,
                    });
                }
            }
            for t in targets {
                self.add_function(&t)?;
                n += 1;
            }
        }
        Ok(n)
    }

    /// Every `apply = function(` of ConfigOptions, named by the nearest
    /// preceding `var = "<name>"`.
    fn add_config_applies(&mut self) -> Result<usize> {
        let file = "Modules/ConfigOptions.lua";
        self.source(file)?;
        let src = &self.sources[file];
        let mut targets = Vec::new();
        let mut var: Option<String> = None;
        for ln in 1..=src.line_count() {
            let l = src.line(ln).unwrap_or_default();
            let mut pos = 0;
            loop {
                let next_var = l[pos..].find("var = \"").map(|p| p + pos);
                let next_apply = l[pos..].find("apply = function").map(|p| p + pos);
                match (next_var, next_apply) {
                    (Some(v), a) if a.map_or(true, |a| v < a) => {
                        let s = v + "var = \"".len();
                        let e = l[s..].find('"').map_or(l.len(), |e| s + e);
                        var = Some(l[s..e].to_owned());
                        pos = e;
                    }
                    (_, Some(a)) => {
                        let v = var
                            .clone()
                            .ok_or_else(|| err(format!("{file}:{ln}: apply before any var")))?;
                        targets.push(FnTarget {
                            id: format!("config:{v}:apply"),
                            file: file.to_owned(),
                            line: ln,
                            col: a,
                        });
                        pos = a + "apply = function".len();
                    }
                    _ => break,
                }
            }
        }
        let n = targets.len();
        for t in targets {
            self.add_function(&t)?;
        }
        Ok(n)
    }

    fn add_call_probes(&mut self) -> Result<()> {
        for (file, prefix, id, params, expected) in CALL_PROBES {
            self.source(file)?;
            let src = &self.sources[*file];
            let mut found = Vec::new();
            for ln in 1..=src.line_count() {
                let l = src.line(ln).unwrap_or_default();
                let mut pos = 0;
                while let Some(p) = l[pos..].find(prefix) {
                    found.push((ln, pos + p));
                    pos += p + prefix.len();
                }
            }
            if found.len() != *expected {
                return Err(err(format!(
                    "{file}: expected {expected} call sites {prefix:?}, found {}",
                    found.len()
                )));
            }
            let callee = prefix.trim_end_matches('(');
            let plist: Vec<String> = params.iter().map(|p| (*p).to_owned()).collect();
            for (ln, col) in found {
                self.push_edit(
                    file,
                    Edit {
                        line: ln,
                        col,
                        removed: (*prefix).to_owned(),
                        inserted: format!(
                            "__PROBE_CALL({}, {{{}}}, {callee})(",
                            lua_quote(id),
                            quoted_list(&plist)
                        ),
                    },
                );
                self.sites.push(ProbeSite {
                    id: (*id).to_owned(),
                    kind: SiteKind::Call,
                    file: (*file).to_owned(),
                    line: ln,
                    end_line: None,
                    params: plist.clone(),
                });
            }
            self.ids.insert((*id).to_owned());
        }
        Ok(())
    }

    fn lines_equal(&mut self, file: &str, trimmed: &str) -> Result<Vec<(usize, String)>> {
        self.source(file)?;
        let src = &self.sources[file];
        Ok((1..=src.line_count())
            .filter_map(|n| {
                let l = src.line(n)?;
                (l.trim() == trimmed).then(|| (n, l.to_owned()))
            })
            .collect())
    }

    fn add_provenance(&mut self) -> Result<()> {
        for (file, line_text, from, kind) in PROVENANCE_SITES {
            let hits = self.lines_equal(file, line_text)?;
            if hits.len() != 1 {
                return Err(err(format!(
                    "{file}: expected one line {line_text:?}, found {}",
                    hits.len()
                )));
            }
            let (ln, text) = &hits[0];
            let col = text
                .rfind(from)
                .ok_or_else(|| err(format!("{file}:{ln}: {from:?} not found")))?;
            self.push_edit(
                file,
                Edit {
                    line: *ln,
                    col,
                    removed: (*from).to_owned(),
                    inserted: format!("(__PROV and __PROV({}, {from}) or {from})", lua_quote(kind)),
                },
            );
            self.sites.push(ProbeSite {
                id: format!("prov:{kind}"),
                kind: SiteKind::Provenance,
                file: (*file).to_owned(),
                line: *ln,
                end_line: None,
                params: Vec::new(),
            });
        }
        for (file, line_text, expr, expected) in MOD_INSERT_SITES {
            let hits = self.lines_equal(file, line_text)?;
            if hits.len() != *expected {
                return Err(err(format!(
                    "{file}: expected {expected} lines {line_text:?}, found {}",
                    hits.len()
                )));
            }
            for (ln, text) in hits {
                let col = text.trim_end().len();
                self.push_edit(
                    file,
                    Edit {
                        line: ln,
                        col,
                        removed: String::new(),
                        inserted: format!(" if __PROV_MOD then __PROV_MOD(self, {expr}) end"),
                    },
                );
                self.sites.push(ProbeSite {
                    id: "prov:mod".to_owned(),
                    kind: SiteKind::ModInsert,
                    file: (*file).to_owned(),
                    line: ln,
                    end_line: None,
                    params: Vec::new(),
                });
            }
        }
        Ok(())
    }

    fn finish(self, stages: StagesFile) -> Result<Plan> {
        let mut files = Vec::new();
        for (file, mut edits) in self.edits {
            let src = &self.sources[&file];
            edits.sort_by(edit_order);
            for w in edits.windows(2) {
                if w[0].line == w[1].line && w[0].col + w[0].removed.len() > w[1].col {
                    return Err(err(format!(
                        "{file}:{}: overlapping patches at columns {} and {}",
                        w[0].line, w[0].col, w[1].col
                    )));
                }
            }
            let patched = apply_edits(src, &edits)?;
            files.push(PatchedFile {
                file: file.clone(),
                original: src.text.clone(),
                patched,
                edits,
            });
        }
        let mut sites = self.sites;
        sites.sort_by(|a, b| (&a.file, a.line, &a.id).cmp(&(&b.file, b.line, &b.id)));
        Ok(Plan {
            stages,
            sites,
            files,
        })
    }
}

fn quoted_list(items: &[String]) -> String {
    items
        .iter()
        .map(|s| lua_quote(s))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Apply edits (sorted by line, col; non-overlapping) to a source.
fn apply_edits(src: &Source, edits: &[Edit]) -> Result<String> {
    let mut out = String::with_capacity(src.text.len() + edits.len() * 64);
    let mut by_line: BTreeMap<usize, Vec<&Edit>> = BTreeMap::new();
    for e in edits {
        by_line.entry(e.line).or_default().push(e);
    }
    let mut last = 0usize; // byte offset copied so far
    for (line, es) in by_line {
        let start = *src
            .starts
            .get(line - 1)
            .ok_or_else(|| err(format!("{}:{line}: no such line", src.file)))?;
        for e in es {
            let at = start + e.col;
            if src.text.get(at..at + e.removed.len()) != Some(e.removed.as_str()) {
                return Err(err(format!(
                    "{}:{line}: patch expected {:?} at column {}",
                    src.file, e.removed, e.col
                )));
            }
            out.push_str(&src.text[last..at]);
            out.push_str(&e.inserted);
            last = at + e.removed.len();
        }
    }
    out.push_str(&src.text[last..]);
    Ok(out)
}

/// Undo [`apply_edits`]: rebuild the original text from a patched one and
/// its edits (the patcher's round-trip check).
pub fn revert(patched: &str, edits: &[Edit]) -> Result<String> {
    let mut out = String::with_capacity(patched.len());
    let mut pos = 0usize; // in patched
    let mut line = 1usize;
    let mut line_start = 0usize; // patched offset of current line start
    let mut shift: i64 = 0; // patched col - original col on the current line
    let mut sorted: Vec<&Edit> = edits.iter().collect();
    sorted.sort_by(|a, b| edit_order(a, b));
    for e in sorted {
        while line < e.line {
            let nl = patched[line_start..]
                .find('\n')
                .ok_or_else(|| err(format!("revert: line {} beyond the text", e.line)))?;
            line_start += nl + 1;
            line += 1;
            shift = 0;
        }
        let at = (line_start as i64 + e.col as i64 + shift) as usize;
        if patched.get(at..at + e.inserted.len()) != Some(e.inserted.as_str()) {
            return Err(err(format!(
                "revert: line {} column {}: inserted text not found",
                e.line, e.col
            )));
        }
        out.push_str(&patched[pos..at]);
        out.push_str(&e.removed);
        pos = at + e.inserted.len();
        shift += e.inserted.len() as i64 - e.removed.len() as i64;
    }
    out.push_str(&patched[pos..]);
    Ok(out)
}

/// Compute every patch for `pob_src` (PoB's `src/`) and `stages`.
pub fn plan(pob_src: &Path, stages: &StagesFile) -> Result<Plan> {
    let mut p = Planner::new(pob_src);
    // function probes first, so stage ids that collide are reported against them
    p.add_named_functions()?;
    let hooks = p.add_skill_hooks()?;
    if hooks == 0 {
        return Err(err("no skill hooks found in Data/Skills"));
    }
    let applies = p.add_config_applies()?;
    if applies == 0 {
        return Err(err("no config applies found in Modules/ConfigOptions.lua"));
    }
    p.add_call_probes()?;
    p.add_provenance()?;
    for f in &stages.function {
        if p.ids.contains(&f.id) {
            return Err(err(format!("duplicate probe id {}", f.id)));
        }
        let text = p.line_text(&f.file, f.line)?;
        if let Some(expect) = &f.expect {
            if !text.contains(expect.as_str()) {
                return Err(err(format!(
                    "{}:{}: expected {expect:?}, found {text:?}",
                    f.file, f.line
                )));
            }
        }
        p.add_function(&FnTarget {
            id: f.id.clone(),
            file: f.file.clone(),
            line: f.line,
            col: 0,
        })?;
    }
    for st in &stages.stage {
        if st.id.ends_with(":in") || st.id.ends_with(":out") {
            let base = st.id.rsplit_once(':').map_or("", |x| x.0);
            if p.sites
                .iter()
                .any(|s| s.kind == SiteKind::Function && s.id == base)
            {
                return Err(err(format!(
                    "stage id {} collides with function probe {base}",
                    st.id
                )));
            }
        }
        p.add_stage(st)?;
    }
    p.finish(stages.clone())
}

// ---- validation -------------------------------------------------------------------------------

/// Compile every patched file with `loadstring` (no PoB, no run).
#[cfg(all(not(target_arch = "wasm32"), feature = "luajit"))]
pub fn validate(plan: &Plan) -> Result<()> {
    let lua = mlua::Lua::new();
    let check: mlua::Function = lua
        .load(VALIDATE_LUA)
        .set_name("validate.lua")
        .eval()
        .map_err(|e| err(format!("validate.lua: {e}")))?;
    let mut failures = Vec::new();
    for f in &plan.files {
        let src = lua
            .create_string(&f.patched)
            .map_err(|e| err(e.to_string()))?;
        let (ok, msg): (Option<bool>, Option<String>) =
            check
                .call((src, format!("@{}", f.file)))
                .map_err(|e| err(format!("{}: {e}", f.file)))?;
        if ok != Some(true) {
            failures.push(format!("{}: {}", f.file, msg.unwrap_or_default()));
        }
        if f.patched.matches('\n').count() != f.original.matches('\n').count() {
            failures.push(format!("{}: line count changed", f.file));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(err(format!(
            "patched sources do not compile:\n  {}",
            failures.join("\n  ")
        )))
    }
}

// ---- publishing the copy -----------------------------------------------------------------------

/// `<root>/<sha>/manifest.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format: String,
    pub sha: String,
    pub patcher_version: u32,
    /// The PoB `src/` that was copied.
    pub upstream: String,
    pub stages_file: Option<String>,
    pub stages: StagesFile,
    pub sites: Vec<ProbeSite>,
    /// Patched files with their edits (original coordinates).
    pub files: BTreeMap<String, Vec<Edit>>,
}

/// A published patched copy.
#[derive(Debug, Clone)]
pub struct Patched {
    pub sha: String,
    /// `<root>/<sha>`.
    pub dir: PathBuf,
    /// `<root>/<sha>/src`: pass as the oracle's PoB source.
    pub src: PathBuf,
    pub manifest: Manifest,
}

/// Where patched copies go: `$POB_ORACLE_SRC_ROOT`, else
/// `$CARGO_TARGET_DIR/oracle-src`, else this workspace's `target/oracle-src`.
#[must_use]
pub fn default_root() -> PathBuf {
    if let Some(p) = std::env::var_os("POB_ORACLE_SRC_ROOT") {
        return PathBuf::from(p);
    }
    if let Some(p) = std::env::var_os("CARGO_TARGET_DIR") {
        return PathBuf::from(p).join("oracle-src");
    }
    let target = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::canonicalize(&target)
        .unwrap_or(target)
        .join("oracle-src")
}

// ---- the shared copy and its pruning (FUP1, HYG3 exposed 2) -------------------------------------

/// The one content-keyed home of every patched copy on the machine: `$POB_ORACLE_SRC_ROOT`,
/// else `<.parity>/patched-src` (the main checkout's `.parity`, found as
/// [`crate::oracle::find_parity_dir`] finds it, so every worktree and every caller shares one
/// `<sha>` copy), else `None` (no `.parity`: [`patch`] uses its `root` argument).
#[must_use]
pub fn shared_root() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("POB_ORACLE_SRC_ROOT") {
        return Some(PathBuf::from(p));
    }
    crate::oracle::find_parity_dir().map(|p| p.join("patched-src"))
}

/// The use marker of a published copy (`<root>/<sha>/.last-used`): [`patch`] rewrites it every
/// time it hands the copy out, so its mtime is the copy's last use.
const USE_MARKER: &str = ".last-used";

/// Copies (with a use marker) not handed out for this long are pruned; default 24 h,
/// `$POB_PATCHED_SRC_MAX_AGE_HOURS` overrides, `0` disables pruning.
fn prune_max_age() -> Option<std::time::Duration> {
    let hours = std::env::var("POB_PATCHED_SRC_MAX_AGE_HOURS")
        .ok()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .unwrap_or(24.0);
    (hours > 0.0).then(|| std::time::Duration::from_secs_f64(hours * 3600.0))
}

/// Copies without a use marker come from the code before FUP1, which never marks a reuse, so
/// their age is their publication time; another worktree on an older branch may still boot PoB
/// from one. They are pruned only after this many times [`prune_max_age`] (default 72 h).
const LEGACY_AGE_FACTOR: u32 = 3;

/// Mark the copy in `dir` as used now. False when `dir` is gone (pruned meanwhile).
#[cfg(all(not(target_arch = "wasm32"), feature = "luajit"))]
fn touch_use_marker(dir: &Path) -> bool {
    dir.is_dir() && std::fs::write(dir.join(USE_MARKER), format!("{}\n", std::process::id())).is_ok()
}

/// The in-use lock of a published copy (`<root>/<sha>/.in-use.lock`). Every process that is handed
/// the copy holds a shared `flock` on it until it exits ([`hold_copy`]), and [`prune`] removes a
/// copy only while it holds the lock exclusively, so a copy in use is never pruned however long
/// ago its use marker was written (FUP3, FOLLOWUPS "FUP1" 4). The kernel drops a dead process's
/// lock, so a crash cannot pin a copy.
const USE_LOCK: &str = ".in-use.lock";

#[cfg(unix)]
fn flock(file: &std::fs::File, op: rustix::fs::FlockOperation) -> std::io::Result<bool> {
    loop {
        match rustix::fs::flock(file, op) {
            Ok(()) => return Ok(true),
            Err(e) if e == rustix::io::Errno::INTR => {}
            Err(e) if e == rustix::io::Errno::WOULDBLOCK => return Ok(false),
            Err(e) => return Err(std::io::Error::from(e)),
        }
    }
}

/// Block until this process holds a shared lock on `file`.
#[cfg(unix)]
fn lock_shared(file: &std::fs::File) -> std::io::Result<()> {
    flock(file, rustix::fs::FlockOperation::LockShared).map(|_| ())
}

#[cfg(not(unix))]
#[allow(clippy::incompatible_msrv)]
fn lock_shared(file: &std::fs::File) -> std::io::Result<()> {
    file.lock_shared()
}

/// Try for an exclusive lock on `file` without blocking (`Ok(false)`: another holder).
#[cfg(unix)]
fn try_lock_exclusive(file: &std::fs::File) -> std::io::Result<bool> {
    flock(file, rustix::fs::FlockOperation::NonBlockingLockExclusive)
}

#[cfg(not(unix))]
#[allow(clippy::incompatible_msrv)]
fn try_lock_exclusive(file: &std::fs::File) -> std::io::Result<bool> {
    match file.try_lock() {
        Ok(()) => Ok(true),
        Err(std::fs::TryLockError::WouldBlock) => Ok(false),
        Err(std::fs::TryLockError::Error(e)) => Err(e),
    }
}

/// Is the open `file` still the file at `path` (not renamed away with its copy, nor replaced by a
/// republished copy's lock)?
#[cfg(unix)]
fn same_file(file: &std::fs::File, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (file.metadata(), std::fs::metadata(path)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn same_file(_file: &std::fs::File, path: &Path) -> bool {
    path.is_file()
}

/// Hold the copy in `dir` for the rest of the process: a shared lock on its [`USE_LOCK`], kept
/// open in a process-wide table (once per copy). False when the copy is gone or was pruned while
/// the lock was being taken (the caller publishes it again).
pub fn hold_copy(dir: &Path) -> bool {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static HELD: OnceLock<Mutex<HashMap<PathBuf, std::fs::File>>> = OnceLock::new();
    let lock = dir.join(USE_LOCK);
    let mut held = HELD
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(f) = held.get(&lock) {
        if same_file(f, &lock) {
            return true;
        }
        held.remove(&lock);
    }
    if !dir.is_dir() {
        return false;
    }
    let Ok(file) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock)
    else {
        return false;
    };
    if lock_shared(&file).is_err() {
        return false;
    }
    // a prune holds the lock exclusively across its rename: once ours is granted, the copy is
    // either still at `dir` (and safe) or in the prune's trash
    if !same_file(&file, &lock) {
        return false;
    }
    held.insert(lock, file);
    true
}

fn mtime(p: &Path) -> Option<std::time::SystemTime> {
    std::fs::symlink_metadata(p).and_then(|m| m.modified()).ok()
}

/// A published copy's directory name: the 16 hex digits of a [`plan_sha`].
fn is_sha_name(name: &str) -> bool {
    name.len() == 16 && name.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Remove the patched copies under `root` that are stale: not `keep`, not held by a running
/// process (its [`USE_LOCK`]: [`hold_copy`]), and not handed out for `max_age` (the use marker's
/// mtime; a copy without one, by its `manifest.json`'s mtime, after [`LEGACY_AGE_FACTOR`] ×
/// `max_age`), plus leftover `.<sha>.tmp.*` / `.<sha>.prune.*` directories older than
/// `max_age`. A copy is first renamed out of the way (atomic, so no process can find a
/// half-deleted copy under its sha; one that looks for it republishes it), with its lock held
/// exclusively across the rename, then deleted; its unpatched files are hard links, so the PoB
/// checkout is untouched. Errors are ignored (another process may prune the same copy). Returns
/// the copies removed.
pub fn prune(root: &Path, keep: Option<&str>, max_age: std::time::Duration) -> Vec<PathBuf> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let now = std::time::SystemTime::now();
    let older = |t: Option<std::time::SystemTime>, age: std::time::Duration| {
        t.and_then(|t| now.duration_since(t).ok())
            .is_some_and(|d| d >= age)
    };
    let mut removed = Vec::new();
    let Ok(rd) = std::fs::read_dir(root) else {
        return removed;
    };
    for e in rd.filter_map(std::result::Result::ok) {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = e.path();
        if !e.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        if is_sha_name(&name) {
            if Some(name.as_str()) == keep {
                continue;
            }
            let marker = path.join(USE_MARKER);
            let stale = if marker.is_file() {
                older(mtime(&marker), max_age)
            } else {
                older(
                    mtime(&path.join("manifest.json")).or_else(|| mtime(&path)),
                    max_age * LEGACY_AGE_FACTOR,
                )
            };
            if !stale {
                continue;
            }
            // in use: a process holds the copy (shared lock). No lock file: no process of this
            // code was ever handed the copy (one that takes it now finds it renamed and
            // republishes it).
            let lock = std::fs::File::options().read(true).open(path.join(USE_LOCK));
            let _exclusive = match lock {
                Ok(f) => match try_lock_exclusive(&f) {
                    Ok(true) => Some(f),
                    _ => continue,
                },
                Err(_) => None,
            };
            let trash = root.join(format!(
                ".{name}.prune.{}.{}",
                std::process::id(),
                SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            if std::fs::rename(&path, &trash).is_ok() {
                let _ = std::fs::remove_dir_all(&trash);
                removed.push(path);
            }
        } else if name.starts_with('.')
            && (name.contains(".tmp.") || name.contains(".prune."))
            && older(mtime(&path), max_age)
        {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
    removed
}

/// [`prune`] each of `roots` once per process (with [`prune_max_age`]; nothing when disabled).
#[cfg(all(not(target_arch = "wasm32"), feature = "luajit"))]
fn prune_once(roots: &[&Path], keep: &str) {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    static DONE: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    let Some(age) = prune_max_age() else {
        return;
    };
    let done = DONE.get_or_init(Mutex::default);
    for root in roots {
        let first = done
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(root.to_path_buf());
        if first {
            let _ = prune(root, Some(keep), age);
        }
    }
}

/// The places patched copies were published before FUP1: `root` as the caller passes it, the
/// default [`default_root`], and the checkout's `target/oracle-src` next to PoB's source
/// (`runner::shared_patch_root`).
#[cfg(all(not(target_arch = "wasm32"), feature = "luajit"))]
fn legacy_roots(pob_src: &Path, root: &Path) -> Vec<PathBuf> {
    let mut v = vec![root.to_path_buf(), default_root()];
    let canon = std::fs::canonicalize(pob_src).unwrap_or_else(|_| pob_src.to_path_buf());
    if let Some(checkout) = canon.parent().and_then(Path::parent) {
        if checkout.join("Cargo.toml").is_file() {
            v.push(checkout.join("target").join("oracle-src"));
        }
    }
    v
}

fn walk_files(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(std::result::Result::ok)
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let r = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        let ft = e.file_type()?;
        if ft.is_dir() {
            walk_files(&e.path(), &r, out)?;
        } else if ft.is_file() || ft.is_symlink() {
            out.push((r, e.path()));
        }
    }
    Ok(())
}

/// Is `rel` (a path relative to PoB's `src/`) a cache PoB derives from its own sources and writes
/// back into the tree at run time? Such a file is left out of every source fingerprint
/// ([`plan_sha`], [`crate::runner::source_key`]): rewriting it does not change what PoB computes,
/// and any PoB run may rewrite it.
///
/// `Data/TimelessJewelData/<name>.bin` is the inflated `<name>.zip` / `<name>.zip.part*`
/// (Modules/DataJewelFileLoader.lua:44-90: when the `.bin` is missing or older than the zip, PoB
/// inflates the zip and rewrites the `.bin` with `io.open(..., "wb+")`); the zips stay
/// fingerprinted, so a changed source still changes the key.
#[must_use]
pub fn is_derived_cache(rel: &str) -> bool {
    rel.strip_prefix("Data/TimelessJewelData/")
        .map(Path::new)
        .is_some_and(|name| {
            name.parent() == Some(Path::new(""))
                && name
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("bin"))
        })
}

/// Hash the stat fingerprint (relative path, size, mtime) of every file of `files` except PoB's
/// derived caches ([`is_derived_cache`]) into `h`.
pub(crate) fn hash_stat_fingerprint(
    h: &mut blake3::Hasher,
    files: &[(String, PathBuf)],
) -> std::result::Result<(), (PathBuf, std::io::Error)> {
    for (rel, path) in files {
        if is_derived_cache(rel) {
            continue;
        }
        let meta = std::fs::metadata(path).map_err(|e| (path.clone(), e))?;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        h.update(rel.as_bytes());
        h.update(&meta.len().to_le_bytes());
        h.update(&mtime.to_le_bytes());
    }
    Ok(())
}

/// The `<sha>` of a plan over a source tree.
pub fn plan_sha(pob_src: &Path, plan: &Plan, files: &[(String, PathBuf)]) -> Result<String> {
    let mut h = blake3::Hasher::new();
    h.update(format!("pob-oracle patcher {PATCHER_VERSION}\n").as_bytes());
    // the probe runtime decides what a dump contains (probe-cache key)
    for lua in [
        crate::runner::STATE_DUMP_LUA,
        crate::runner::PROVENANCE_LUA,
        crate::runner::PROBE_LUA,
    ] {
        h.update(lua.as_bytes());
        h.update(&[0]);
    }
    h.update(
        toml::to_string(&plan.stages)
            .map_err(|e| err(e.to_string()))?
            .as_bytes(),
    );
    for f in &plan.files {
        h.update(f.file.as_bytes());
        h.update(&[0]);
        h.update(f.patched.as_bytes());
        h.update(&[0]);
    }
    let canon = std::fs::canonicalize(pob_src).unwrap_or_else(|_| pob_src.to_path_buf());
    // the Lua runtime the probe binary boots the copy with (HYG3)
    h.update(crate::runner::lua_runtime_versions().as_bytes());
    h.update(&[0]);
    h.update(canon.to_string_lossy().as_bytes());
    hash_stat_fingerprint(&mut h, files).map_err(|(p, e)| err(format!("{}: {e}", p.display())))?;
    Ok(h.finalize().to_hex()[..16].to_owned())
}

/// Write the patched tree (src/, links to PoB's siblings, manifest.json)
/// into the fresh directory `tmp`.
#[cfg(all(not(target_arch = "wasm32"), feature = "luajit"))]
fn build_patched_tree(
    pob_src: &Path,
    plan: &Plan,
    tree: &[(String, PathBuf)],
    manifest: &Manifest,
    tmp: &Path,
) -> Result<()> {
    let io = |e: std::io::Error, p: &Path| err(format!("{}: {e}", p.display()));
    let patched: BTreeMap<&str, &str> = plan
        .files
        .iter()
        .map(|f| (f.file.as_str(), f.patched.as_str()))
        .collect();
    let tsrc = tmp.join("src");
    // Every file is created new (`create_new`): the unpatched ones are hard
    // links to PoB's own files, so opening an existing `dst` for writing
    // would write through the link into the PoB checkout itself (a copy
    // onto an existing link truncates the original).
    let create_new = |dst: &Path| {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dst)
            .map_err(|e| io(e, dst))
    };
    for (rel, path) in tree {
        let dst = tsrc.join(rel);
        if let Some(d) = dst.parent() {
            std::fs::create_dir_all(d).map_err(|e| io(e, d))?;
        }
        if let Some(text) = patched.get(rel.as_str()) {
            std::io::Write::write_all(&mut create_new(&dst)?, text.as_bytes())
                .map_err(|e| io(e, &dst))?;
            continue;
        }
        match std::fs::hard_link(path, &dst) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Err(io(e, &dst)),
            Err(_) => {
                // another filesystem: copy
                let mut from = std::fs::File::open(path).map_err(|e| io(e, path))?;
                std::io::copy(&mut from, &mut create_new(&dst)?).map_err(|e| io(e, &dst))?;
            }
        }
    }
    // Everything next to PoB's src/ (runtime/ for the loader, manifest.xml
    // for Launch.lua's version number, ...) is linked, so the copy boots
    // exactly like the original checkout.
    let pob_root = std::fs::canonicalize(pob_src.join("..")).map_err(|e| io(e, pob_src))?;
    let src_name = std::fs::canonicalize(pob_src)
        .ok()
        .and_then(|p| p.file_name().map(std::ffi::OsStr::to_os_string));
    let mut siblings: Vec<_> = std::fs::read_dir(&pob_root)
        .map_err(|e| io(e, &pob_root))?
        .filter_map(std::result::Result::ok)
        .filter(|e| {
            Some(e.file_name()) != src_name && !e.file_name().to_string_lossy().starts_with('.')
        })
        .collect();
    siblings.sort_by_key(std::fs::DirEntry::file_name);
    for e in siblings {
        #[cfg(unix)]
        std::os::unix::fs::symlink(e.path(), tmp.join(e.file_name())).map_err(|x| io(x, tmp))?;
        #[cfg(not(unix))]
        let _ = e;
    }
    let json = serde_json::to_string_pretty(manifest).map_err(|e| err(e.to_string()))?;
    std::fs::write(tmp.join("manifest.json"), json).map_err(|e| io(e, tmp))
}

/// Plan, validate and publish the patched copy of `pob_src` (reused when `<root>/<sha>` already
/// exists).
///
/// FUP1 (HYG3 exposed 2): the copy lives in the [`shared_root`] (`<.parity>/patched-src/<sha>`)
/// whatever `root` the caller passes; `root` is only used when there is no shared root. Every
/// call marks the copy as used (`.last-used`) and, once per process, [`prune`]s the stale copies
/// of the shared root and of the places copies were published before (`root`, [`default_root`],
/// the checkout's `target/oracle-src`). [`patch_into`] publishes into exactly the `root` it is
/// given instead.
#[cfg(all(not(target_arch = "wasm32"), feature = "luajit"))]
pub fn patch(pob_src: &Path, stages_path: Option<&Path>, root: &Path) -> Result<Patched> {
    let shared = shared_root();
    let mut prune_roots = vec![shared.clone().unwrap_or_else(|| root.to_path_buf())];
    for r in legacy_roots(pob_src, root) {
        if !prune_roots.contains(&r) {
            prune_roots.push(r);
        }
    }
    publish(pob_src, stages_path, shared.as_deref().unwrap_or(root), &prune_roots)
}

/// [`patch`] into exactly `root/<sha>`, bypassing the [`shared_root`]: for callers that need a
/// root of their own, e.g. P3's test of concurrent publishes into an empty root. Marks the copy
/// as used and prunes (once per process) the stale copies of `root` alone.
#[cfg(all(not(target_arch = "wasm32"), feature = "luajit"))]
pub fn patch_into(pob_src: &Path, stages_path: Option<&Path>, root: &Path) -> Result<Patched> {
    publish(pob_src, stages_path, root, &[root.to_path_buf()])
}

/// Plan, validate and publish the patched copy of `pob_src` into `root/<sha>` (reused when it
/// exists), marking it used and pruning `prune_roots` once per process.
#[cfg(all(not(target_arch = "wasm32"), feature = "luajit"))]
fn publish(
    pob_src: &Path,
    stages_path: Option<&Path>,
    root: &Path,
    prune_roots: &[PathBuf],
) -> Result<Patched> {
    let prune_now = |sha: &str| {
        let roots: Vec<&Path> = prune_roots.iter().map(PathBuf::as_path).collect();
        prune_once(&roots, sha);
    };
    let stages = match stages_path {
        Some(p) => StagesFile::load(p)?,
        None => StagesFile::default(),
    };
    let plan = plan(pob_src, &stages)?;
    let mut tree = Vec::new();
    walk_files(pob_src, "", &mut tree).map_err(|e| err(format!("{}: {e}", pob_src.display())))?;
    let sha = plan_sha(pob_src, &plan, &tree)?;
    let dir = root.join(&sha);
    let manifest_path = dir.join("manifest.json");
    if let Ok(text) = std::fs::read_to_string(&manifest_path) {
        if let Ok(manifest) = serde_json::from_str::<Manifest>(&text) {
            // (a copy pruned between the read and the mark is published again below)
            if manifest.sha == sha && touch_use_marker(&dir) && hold_copy(&dir) {
                prune_now(&sha);
                return Ok(Patched {
                    src: dir.join("src"),
                    dir,
                    sha,
                    manifest,
                });
            }
        }
    }
    validate(&plan)?;
    let manifest = Manifest {
        format: MANIFEST_FORMAT.to_owned(),
        sha: sha.clone(),
        patcher_version: PATCHER_VERSION,
        upstream: pob_src.to_string_lossy().into_owned(),
        stages_file: stages_path.map(|p| p.to_string_lossy().into_owned()),
        stages: plan.stages.clone(),
        sites: plan.sites.clone(),
        files: plan
            .files
            .iter()
            .map(|f| (f.file.clone(), f.edits.clone()))
            .collect(),
    };
    std::fs::create_dir_all(root).map_err(|e| err(format!("{}: {e}", root.display())))?;
    // a temp dir of this call alone: other threads of this process (cargo
    // test) and other processes may publish the same sha concurrently
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp = root.join(format!(
        ".{sha}.tmp.{}.{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let published = build_patched_tree(pob_src, &plan, &tree, &manifest, &tmp).and_then(|()| {
        std::fs::rename(&tmp, &dir).map_err(|e| err(format!("{}: {e}", dir.display())))
    });
    if let Err(e) = published {
        // another thread or process published the same sha first (the
        // rename finds `dir` in place: EEXIST / ENOTEMPTY), or this one failed
        let _ = std::fs::remove_dir_all(&tmp);
        let theirs = std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|t| serde_json::from_str::<Manifest>(&t).ok())
            .filter(|m| m.sha == sha);
        let Some(manifest) = theirs else {
            return Err(err(format!("could not publish {}: {e}", dir.display())));
        };
        touch_use_marker(&dir);
        hold_copy(&dir);
        prune_now(&sha);
        return Ok(Patched {
            src: dir.join("src"),
            dir,
            sha,
            manifest,
        });
    }
    touch_use_marker(&dir);
    hold_copy(&dir);
    prune_now(&sha);
    Ok(Patched {
        src: dir.join("src"),
        dir,
        sha,
        manifest,
    })
}

/// Read the manifest of a patched copy given its `src/` (or its dir).
pub fn manifest_of(src: &Path) -> Option<Manifest> {
    for d in [src.to_path_buf(), src.join("..")] {
        if let Ok(text) = std::fs::read_to_string(d.join("manifest.json")) {
            if let Ok(m) = serde_json::from_str(&text) {
                return Some(m);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_caches_are_the_jewel_bins() {
        assert!(is_derived_cache("Data/TimelessJewelData/LethalPride.bin"));
        assert!(!is_derived_cache("Data/TimelessJewelData/LethalPride.zip"));
        assert!(!is_derived_cache(
            "Data/TimelessJewelData/AbyssKurgal.zip.part0"
        ));
        assert!(!is_derived_cache(
            "Data/TimelessJewelData/LegionPassives.lua"
        ));
        assert!(!is_derived_cache("Data/TimelessJewelData/sub/x.bin"));
        assert!(!is_derived_cache("Data/Other/x.bin"));
    }

    /// Rewriting a jewel `.bin` (as any PoB run may) leaves the `<sha>` unchanged; changing the
    /// zip it is inflated from does not.
    #[test]
    fn plan_sha_ignores_derived_caches() {
        // not std::env::temp_dir(): /tmp is a RAM-backed tmpfs on the port machines
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tmp")
            .join(format!("hyg3-plan-sha-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let jd = d.join("Data/TimelessJewelData");
        std::fs::create_dir_all(&jd).unwrap();
        std::fs::write(d.join("a.lua"), "return 1").unwrap();
        std::fs::write(jd.join("X.zip"), "zip").unwrap();
        std::fs::write(jd.join("X.bin"), "inflated").unwrap();
        let plan = Plan {
            stages: StagesFile::default(),
            sites: Vec::new(),
            files: Vec::new(),
        };
        let sha = || {
            let mut tree = Vec::new();
            walk_files(&d, "", &mut tree).unwrap();
            plan_sha(&d, &plan, &tree).unwrap()
        };
        let k1 = sha();
        std::fs::write(jd.join("X.bin"), "inflated, written again").unwrap();
        std::fs::File::options()
            .write(true)
            .open(jd.join("X.bin"))
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(3600))
            .unwrap();
        assert_eq!(k1, sha(), "a .bin rewrite changed the sha");
        std::fs::remove_file(jd.join("X.bin")).unwrap();
        assert_eq!(k1, sha(), "a missing .bin changed the sha");
        std::fs::write(jd.join("X.zip"), "zip v2").unwrap();
        assert_ne!(k1, sha(), "the zip is not fingerprinted");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// FUP1: `prune` removes the copies not used for `max_age` (use marker), marker-less
    /// (pre-FUP1) copies only after `LEGACY_AGE_FACTOR` × `max_age`, never `keep`, and old
    /// leftover temp dirs; anything else stays.
    #[test]
    fn prune_removes_stale_copies() {
        use std::time::{Duration, SystemTime};
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tmp")
            .join(format!("fup1-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let hour = Duration::from_secs(3600);
        let set_age = |p: &Path, age: Duration| {
            std::fs::File::options()
                .read(true)
                .open(p)
                .unwrap()
                .set_modified(SystemTime::now() - age)
                .unwrap();
        };
        let copy = |name: &str, marker: Option<Duration>, manifest_age: Duration| {
            let d = root.join(name);
            std::fs::create_dir_all(d.join("src")).unwrap();
            std::fs::write(d.join("src/a.lua"), "return 1").unwrap();
            std::fs::write(d.join("manifest.json"), "{}").unwrap();
            set_age(&d.join("manifest.json"), manifest_age);
            if let Some(age) = marker {
                std::fs::write(d.join(USE_MARKER), "1\n").unwrap();
                set_age(&d.join(USE_MARKER), age);
            }
            d
        };
        let fresh = copy("00000000000000a1", Some(Duration::from_secs(60)), 10 * hour);
        let stale = copy("00000000000000a2", Some(2 * hour), 10 * hour);
        let kept = copy("00000000000000a3", Some(5 * hour), 10 * hour);
        let legacy_young = copy("00000000000000a4", None, 2 * hour);
        let legacy_old = copy("00000000000000a5", None, 4 * hour);
        let other = root.join("not-a-copy");
        std::fs::create_dir_all(&other).unwrap();
        let tmp_old = root.join(".00000000000000a6.tmp.1.0");
        std::fs::create_dir_all(&tmp_old).unwrap();
        set_age(&tmp_old, 2 * hour);
        let tmp_new = root.join(".00000000000000a7.tmp.1.0");
        std::fs::create_dir_all(&tmp_new).unwrap();
        let mut removed = prune(&root, Some("00000000000000a3"), hour);
        removed.sort();
        assert_eq!(removed, vec![stale.clone(), legacy_old.clone()]);
        for (p, there) in [
            (&fresh, true),
            (&stale, false),
            (&kept, true),
            (&legacy_young, true),
            (&legacy_old, false),
            (&other, true),
            (&tmp_old, false),
            (&tmp_new, true),
        ] {
            assert_eq!(p.exists(), there, "{}", p.display());
        }
        // no trash left behind
        let left: Vec<String> = std::fs::read_dir(&root)
            .unwrap()
            .filter_map(std::result::Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".prune."))
            .collect();
        assert!(left.is_empty(), "{left:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// FUP3 (FOLLOWUPS "FUP1" 4): a stale copy a process holds ([`hold_copy`]) is not pruned
    /// while it is held (by this process or another one); one whose lock nobody holds is.
    #[test]
    fn prune_keeps_held_copies() {
        use std::time::{Duration, SystemTime};
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tmp")
            .join(format!("fup3-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let hour = Duration::from_secs(3600);
        let copy = |name: &str| {
            let d = root.join(name);
            std::fs::create_dir_all(d.join("src")).unwrap();
            std::fs::write(d.join("manifest.json"), "{}").unwrap();
            std::fs::write(d.join(USE_MARKER), "1\n").unwrap();
            std::fs::File::options()
                .read(true)
                .open(d.join(USE_MARKER))
                .unwrap()
                .set_modified(SystemTime::now() - 2 * hour)
                .unwrap();
            d
        };
        let held = copy("00000000000000b1");
        let released = copy("00000000000000b2");
        let other_process = copy("00000000000000b3");
        assert!(hold_copy(&held));
        assert!(hold_copy(&held), "a second hold of the same copy is a no-op");
        // a lock file some earlier process held and released
        drop(std::fs::File::create(released.join(USE_LOCK)).unwrap());
        // another holder: a shared lock through a descriptor of its own (as another process has)
        let theirs = std::fs::File::create(other_process.join(USE_LOCK)).unwrap();
        lock_shared(&theirs).unwrap();
        let removed = prune(&root, None, hour);
        assert_eq!(removed, vec![released.clone()]);
        assert!(held.is_dir() && other_process.is_dir() && !released.exists());
        // the other holder exits: its copy goes on the next prune; ours stays held
        drop(theirs);
        assert_eq!(prune(&root, None, hour), vec![other_process.clone()]);
        assert!(held.is_dir());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn compound_rewrite_matches_the_loader() {
        assert_eq!(
            compound_rewrite("\tn += 1").as_deref(),
            Some("\tn = n + (1)")
        );
        assert_eq!(
            compound_rewrite("  a.b[c] ..= \"x\" -- note").as_deref(),
            Some("  a.b[c] = a.b[c] .. (\"x\") -- note")
        );
        assert_eq!(compound_rewrite("x == 1"), None);
        assert_eq!(compound_rewrite("x = 1"), None);
        assert_eq!(compound_rewrite("if a then"), None);
        assert_eq!(
            compound_rewrite("t.x.y*=2").as_deref(),
            Some("t.x.y = t.x.y * (2)")
        );
    }

    #[test]
    fn words_skip_strings_and_comments() {
        let src = "local s = \"function end\" -- if do\n--[[ function\n end ]] x = [==[ do ]==] function f() if a then end end";
        let words = lua_words(src).unwrap();
        let texts: Vec<&str> = words.iter().map(|w| w.text).collect();
        assert_eq!(
            texts,
            ["local", "s", "x", "function", "f", "if", "a", "then", "end", "end"]
        );
        let fi = texts.iter().position(|t| *t == "function").unwrap();
        assert_eq!(matching_end(&words, fi).unwrap(), texts.len() - 1);
        assert_eq!(words[3].line, 3);
    }

    #[test]
    fn edits_round_trip() {
        let src = Source::new(
            "x.lua",
            "a = 1\nfunction f(x)\n  return x\nend\n".to_owned(),
        );
        let edits = vec![
            Edit {
                line: 2,
                col: 0,
                removed: "function f(".into(),
                inserted: "f = W(\"f\", {\"x\"}, function(".into(),
            },
            Edit {
                line: 2,
                col: "function f(".len(),
                removed: String::new(),
                inserted: String::new(),
            },
            Edit {
                line: 4,
                col: 3,
                removed: String::new(),
                inserted: ")".into(),
            },
        ];
        let patched = apply_edits(&src, &edits).unwrap();
        assert_eq!(
            patched,
            "a = 1\nf = W(\"f\", {\"x\"}, function(x)\n  return x\nend)\n"
        );
        assert_eq!(revert(&patched, &edits).unwrap(), src.text);
    }
}
