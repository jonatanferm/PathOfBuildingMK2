//! The parity oracle: Path of Building's own numbers for a build, as JSON.
//!
//! Two dumps per build, both produced by the `pob_oracle` binary in a fresh
//! process (so no state leaks from one build into the next):
//!
//! * **outputs** — the main env's complete `env.player.output` (and
//!   `env.minion.output`), types and nesting preserved ([`OracleOutput`]).
//! * **stages** — the main env right after `calcs.initEnv` (player / enemy /
//!   minion mod DBs with conditions and multipliers, every active skill's
//!   types, flags, data, cfg and mod list), plus the final output of every
//!   socket-group skill computed as calcFullDPS does ([`OracleStages`]).
//!
//! Tests use [`Oracle::outputs`] / [`Oracle::stages`]; the JSON conventions
//! are documented in the crate's `README.md`.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use pob_gamedata::{Mod, Value};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value as J};

/// The Lua that hooks PoB and produces the dumps (`__oracle_outputs`,
/// `__oracle_stages`).
pub const ORACLE_LUA: &str = include_str!("oracle.lua");

/// Format tags written into the dumps.
pub const OUTPUTS_FORMAT: &str = "pob-oracle/outputs/1";
pub const STAGES_FORMAT: &str = "pob-oracle/stages/1";

#[derive(Debug, Clone)]
pub struct OracleError(pub String);

impl fmt::Display for OracleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for OracleError {}

impl From<String> for OracleError {
    fn from(s: String) -> Self {
        Self(s)
    }
}

pub type Result<T> = std::result::Result<T, OracleError>;

// ---- locating PoB / the binary ----------------------------------------------

fn search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(d) = std::env::current_dir() {
        roots.push(d);
    }
    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    if let Ok(e) = std::env::current_exe() {
        roots.push(e);
    }
    // a worktree outside the main checkout (e.g. under /tmp) still finds the
    // main checkout's gitignored inputs (.parity, data/dump, .PathOfBuilding)
    let more: Vec<PathBuf> = roots.iter().filter_map(|r| main_checkout_of(r)).collect();
    roots.extend(more);
    roots
}

/// The main checkout of the linked worktree containing `p`: the nearest
/// `.git` *file* (`gitdir: <common>/worktrees/<name>`), that directory's
/// `commondir`, and the common directory's parent. None in the main checkout
/// itself or outside a repository.
fn main_checkout_of(p: &Path) -> Option<PathBuf> {
    let dotgit = p.ancestors().map(|a| a.join(".git")).find(|g| g.exists())?;
    if !dotgit.is_file() {
        return None;
    }
    let text = std::fs::read_to_string(&dotgit).ok()?;
    let linked = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
    let linked = if linked.is_relative() {
        dotgit.parent()?.join(linked)
    } else {
        linked
    };
    let common = std::fs::read_to_string(linked.join("commondir")).ok()?;
    let common = linked.join(common.trim());
    let common = common.canonicalize().unwrap_or(common);
    if common.file_name()? != ".git" {
        return None;
    }
    common.parent().map(Path::to_path_buf)
}

/// `rel` under the nearest ancestor (of the working directory, this crate or
/// the running executable) where it exists — e.g. `data/lua/x.lua`. Lets
/// tests find the gitignored PoB checkout / data from a nested worktree.
#[must_use]
pub fn find_up(rel: &str) -> Option<PathBuf> {
    search_roots()
        .iter()
        .flat_map(|r| r.ancestors().map(|a| a.join(rel)).collect::<Vec<_>>())
        .find(|p| p.exists())
}

/// PoB's `src/` directory: `$POB_SRC` (either `src/` or the checkout root),
/// else `.PathOfBuilding/src` in the nearest ancestor of the working
/// directory, this crate or the running executable that has one (which also
/// finds the main checkout from a git worktree nested inside it).
#[must_use]
pub fn find_pob_src() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("POB_SRC").map(PathBuf::from) {
        for c in [p.clone(), p.join("src")] {
            if c.join("HeadlessWrapper.lua").is_file() {
                return Some(c);
            }
        }
    }
    for root in search_roots() {
        for a in root.ancestors() {
            let c = a.join(".PathOfBuilding/src");
            if c.join("HeadlessWrapper.lua").is_file() {
                return Some(c);
            }
        }
    }
    None
}

/// The nearest `.parity` directory (build corpus and oracle dumps), found
/// the same way as [`find_pob_src`].
#[must_use]
pub fn find_parity_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("POB_PARITY_DIR").map(PathBuf::from) {
        return p.is_dir().then_some(p);
    }
    for root in search_roots() {
        for a in root.ancestors() {
            let c = a.join(".parity");
            if c.join("builds").is_dir() {
                return Some(c);
            }
        }
    }
    None
}

fn exe_name(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

/// The `pob_oracle` binary: `$POB_ORACLE_BIN`, else next to the running
/// executable (or its parent, for test binaries in `target/*/deps`).
#[must_use]
pub fn find_oracle_bin() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("POB_ORACLE_BIN").map(PathBuf::from) {
        return p.is_file().then_some(p);
    }
    let exe = std::env::current_exe().ok()?;
    let name = exe_name("pob_oracle");
    exe.ancestors()
        .skip(1)
        .take(3)
        .map(|d| d.join(&name))
        .find(|p| p.is_file())
}

// ---- output trees -----------------------------------------------------------

/// A PoB output table as JSON (see README: plain JSON, sorted keys,
/// sequences as arrays, non-finite numbers as `{"$num": "inf"}`).
pub type Tree = Map<String, J>;

/// A scalar leaf of an output tree.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Leaf {
    Num(f64),
    Bool(bool),
    Str(String),
}

impl Leaf {
    #[must_use]
    pub fn num(&self) -> Option<f64> {
        match self {
            Self::Num(n) => Some(*n),
            _ => None,
        }
    }
}

/// A number in the oracle's JSON: a JSON number or `{"$num": "inf"|"-inf"|"nan"}`.
#[must_use]
pub fn json_num(v: &J) -> Option<f64> {
    match v {
        J::Number(n) => n.as_f64(),
        J::Object(m) if m.len() == 1 => match m.get("$num")?.as_str()? {
            "inf" => Some(f64::INFINITY),
            "-inf" => Some(f64::NEG_INFINITY),
            "nan" => Some(f64::NAN),
            _ => None,
        },
        _ => None,
    }
}

/// Flatten a tree to `path → leaf`: object keys joined with `.`, array
/// elements by their 1-based (Lua) index, e.g. `MainHand.Speed`,
/// `SkillDPS.1.dps`. Reference markers (`{"$object": …}` etc.) are leaves
/// with their marker text as a string.
#[must_use]
pub fn flatten(tree: &Tree) -> BTreeMap<String, Leaf> {
    fn walk(prefix: &str, v: &J, out: &mut BTreeMap<String, Leaf>) {
        let key = |k: &str| {
            if prefix.is_empty() {
                k.to_owned()
            } else {
                format!("{prefix}.{k}")
            }
        };
        if let Some(n) = json_num(v) {
            out.insert(prefix.to_owned(), Leaf::Num(n));
            return;
        }
        match v {
            J::Null => {}
            J::Bool(b) => {
                out.insert(prefix.to_owned(), Leaf::Bool(*b));
            }
            J::String(s) => {
                out.insert(prefix.to_owned(), Leaf::Str(s.clone()));
            }
            J::Number(_) => {}
            J::Array(a) => {
                for (i, x) in a.iter().enumerate() {
                    walk(&key(&(i + 1).to_string()), x, out);
                }
            }
            J::Object(m) => {
                if m.len() == 1 && m.keys().next().is_some_and(|k| k.starts_with('$')) {
                    out.insert(
                        prefix.to_owned(),
                        Leaf::Str(J::Object(m.clone()).to_string()),
                    );
                    return;
                }
                for (k, x) in m {
                    walk(&key(k), x, out);
                }
            }
        }
    }
    let mut out = BTreeMap::new();
    for (k, v) in tree {
        walk(k, v, &mut out);
    }
    out
}

/// Look up a `.`-separated path (array elements 1-based) in a tree.
#[must_use]
pub fn tree_get<'a>(tree: &'a Tree, path: &str) -> Option<&'a J> {
    let mut parts = path.split('.');
    let mut cur = tree.get(parts.next()?)?;
    for p in parts {
        cur = match cur {
            J::Object(m) => m.get(p)?,
            J::Array(a) => a.get(p.parse::<usize>().ok()?.checked_sub(1)?)?,
            _ => return None,
        };
    }
    Some(cur)
}

// ---- dump types ---------------------------------------------------------------

/// Which skill an env had as its main skill.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRef {
    /// 1-based index into `env.player.activeSkillList`.
    pub index: Option<usize>,
    pub id: String,
    pub name: String,
    pub skill_part_name: Option<String>,
}

/// The LuaJIT build and mode of an oracle dump (CCR-0001/0002). Oracle parity
/// runs use the interpreter (`jit: false`, [`crate::jit::ORACLE_JIT_OFF`]); on
/// a `numberMode: "dual"` host a ±0 difference from the Rust port (which
/// follows x64 PoB) is a host artefact, never a port bug.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LuajitInfo {
    /// `jit.arch` (`x64`, `arm64`, ...).
    pub arch: String,
    /// `jit.status()`: whether the JIT compiler was on.
    pub jit: bool,
    /// `single` (x86/x64, PoB's platform) or `dual` (ARM64 and others).
    pub number_mode: String,
}

/// `pob_oracle outputs`: the main env's complete output tables.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OracleOutput {
    pub format: String,
    /// The build name PoB was given (the XML file stem).
    pub build: String,
    pub pob_version: Option<String>,
    /// The LuaJIT that produced the dump (absent in dumps made before CCR1).
    #[serde(default)]
    pub luajit: Option<LuajitInfo>,
    pub main_skill: Option<SkillRef>,
    /// `env.player.output`.
    pub player: Tree,
    /// `env.minion.output`, when the main skill has a minion.
    pub minion: Option<Tree>,
}

impl OracleOutput {
    /// A top-level numeric player output (`Life`, `TotalDPS`, …).
    #[must_use]
    pub fn num(&self, key: &str) -> Option<f64> {
        self.player.get(key).and_then(json_num)
    }

    /// A player output by `.`-path (`MainHand.Speed`, `SkillDPS.1.dps`).
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&J> {
        tree_get(&self.player, path)
    }

    /// Player leaves by path, plus minion leaves under `Minion.`.
    #[must_use]
    pub fn flat(&self) -> BTreeMap<String, Leaf> {
        let mut out = flatten(&self.player);
        if let Some(m) = &self.minion {
            for (k, v) in flatten(m) {
                out.insert(format!("Minion.{k}"), v);
            }
        }
        out
    }

    pub fn from_json(s: &str) -> Result<Self> {
        serde_json::from_str(s).map_err(|e| OracleError(format!("outputs JSON: {e}")))
    }
}

// ---- pob_gamedata mods and values in the oracle's JSON shape (ORC-X) -------------------------

fn de_mods<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Vec<Mod>, D::Error> {
    match J::deserialize(d)? {
        J::Array(a) => a
            .iter()
            .map(|j| crate::mod_json::decode_mod(j).map(|(m, _)| m))
            .collect::<std::result::Result<_, _>>()
            .map_err(serde::de::Error::custom),
        J::Object(m) if m.is_empty() => Ok(Vec::new()),
        J::Null => Ok(Vec::new()),
        other => Err(serde::de::Error::custom(format!("expected a mod list, got {other}"))),
    }
}

fn ser_mods<S: Serializer>(mods: &[Mod], s: S) -> std::result::Result<S::Ok, S::Error> {
    let list: Vec<J> = mods
        .iter()
        .map(|m| pob_calc::probe::mod_json(m, None))
        .collect();
    list.serialize(s)
}

fn de_value<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<Value>, D::Error> {
    let j = J::deserialize(d)?;
    Ok((!j.is_null()).then(|| crate::mod_json::j_to_value(&j)))
}

fn ser_value<S: Serializer>(v: &Option<Value>, s: S) -> std::result::Result<S::Ok, S::Error> {
    v.as_ref()
        .map(pob_calc::probe::value_json)
        .unwrap_or(J::Null)
        .serialize(s)
}

fn de_values<'de, D: Deserializer<'de>>(
    d: D,
) -> std::result::Result<BTreeMap<String, Value>, D::Error> {
    match J::deserialize(d)? {
        J::Object(m) => Ok(m
            .iter()
            .map(|(k, v)| (k.clone(), crate::mod_json::j_to_value(v)))
            .collect()),
        _ => Ok(BTreeMap::new()),
    }
}

fn ser_values<S: Serializer>(
    m: &BTreeMap<String, Value>,
    s: S,
) -> std::result::Result<S::Ok, S::Error> {
    let o: Map<String, J> = m
        .iter()
        .map(|(k, v)| (k.clone(), pob_calc::probe::value_json(v)))
        .collect();
    o.serialize(s)
}

fn de_flags<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<u32, D::Error> {
    // LuaJIT's bit ops return signed 32-bit values.
    let v = J::deserialize(d)?;
    Ok(json_num(&v).map_or(0, |n| (n as i64) as u32))
}

/// A ModDB / ModList as PoB held it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModStoreDump {
    /// `ModDB` or `ModList`.
    pub class: String,
    /// ModDB: grouped by name (names sorted), PoB's order within a name.
    /// ModList: PoB's order.
    #[serde(deserialize_with = "de_mods", serialize_with = "ser_mods")]
    pub mods: Vec<Mod>,
    #[serde(deserialize_with = "de_values", serialize_with = "ser_values")]
    pub conditions: BTreeMap<String, Value>,
    #[serde(deserialize_with = "de_values", serialize_with = "ser_values")]
    pub multipliers: BTreeMap<String, Value>,
    pub parent: Option<Box<ModStoreDump>>,
}

impl ModStoreDump {
    /// Mods named `name`.
    pub fn named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Mod> + 'a {
        self.mods.iter().filter(move |m| m.name == name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorStage {
    pub level: Option<f64>,
    #[serde(rename = "modDB")]
    pub mod_db: ModStoreDump,
}

/// One entry of an active skill's `effectList` (active effect first).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectDump {
    pub id: String,
    pub name: Option<String>,
    pub support: bool,
    pub level: Option<f64>,
    pub quality: Option<f64>,
    pub quality_id: Option<String>,
}

/// `activeSkill.skillCfg` with object references reduced to names / ids.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCfgDump {
    #[serde(default, deserialize_with = "de_flags")]
    pub flags: u32,
    #[serde(default, deserialize_with = "de_flags")]
    pub keyword_flags: u32,
    pub skill_name: Option<String>,
    pub summon_skill_name: Option<String>,
    /// `skillGem.name`.
    pub skill_gem: Option<String>,
    /// `skillGrantedEffect.id`.
    pub skill_granted_effect: Option<String>,
    pub skill_part: Option<f64>,
    #[serde(default)]
    pub skill_types: Vec<u32>,
    #[serde(default, deserialize_with = "de_values", serialize_with = "ser_values")]
    pub skill_cond: BTreeMap<String, Value>,
    /// `false` outside effective mode, else the effective range.
    #[serde(default, deserialize_with = "de_value", serialize_with = "ser_value")]
    pub skill_dist: Option<Value>,
    pub slot_name: Option<String>,
    pub socket_color: Option<String>,
    pub socket_num: Option<f64>,
    /// Anything else PoB put in the cfg.
    #[serde(flatten)]
    pub extra: BTreeMap<String, J>,
}

/// An active skill right after `calcs.initEnv`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillStage {
    /// 1-based index into `env.player.activeSkillList`.
    pub index: usize,
    /// Active granted effect id.
    pub id: String,
    pub name: String,
    pub is_main: bool,
    /// 1-based index into `skillsTab.socketGroupList`.
    pub socket_group: Option<usize>,
    pub slot: Option<String>,
    #[serde(rename = "includeInFullDPS")]
    pub include_in_full_dps: bool,
    pub skill_part: Option<f64>,
    pub skill_part_name: Option<String>,
    pub minion: Option<String>,
    pub effects: Vec<EffectDump>,
    pub skill_types: Vec<u32>,
    pub skill_type_names: Vec<String>,
    #[serde(deserialize_with = "de_values", serialize_with = "ser_values")]
    pub skill_flags: BTreeMap<String, Value>,
    #[serde(deserialize_with = "de_values", serialize_with = "ser_values")]
    pub skill_data: BTreeMap<String, Value>,
    pub skill_cfg: Option<SkillCfgDump>,
    pub weapon1_cfg: Option<SkillCfgDump>,
    pub weapon2_cfg: Option<SkillCfgDump>,
    pub skill_mod_list: ModStoreDump,
}

/// The main env as `calcs.initEnv` returned it, before `calcs.perform`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitEnvDump {
    pub mode: String,
    pub main_skill_index: Option<usize>,
    pub player: ActorStage,
    pub enemy: ActorStage,
    pub minion: Option<ActorStage>,
    #[serde(rename = "itemModDB")]
    pub item_mod_db: Option<ModStoreDump>,
    pub active_skills: Vec<SkillStage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MirageDump {
    pub name: Option<String>,
    pub count: Option<f64>,
    pub output: Option<J>,
}

/// One socket-group skill's final output with it as the main skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillOutput {
    pub index: usize,
    pub id: String,
    pub name: String,
    pub socket_group: Option<usize>,
    #[serde(rename = "includeInFullDPS")]
    pub include_in_full_dps: bool,
    /// calcFullDPS's skill count / enabled (getActiveSkillCount).
    pub count: Option<f64>,
    pub enabled: bool,
    pub skill_part_name: Option<String>,
    /// A Lua error raised while calculating this skill.
    pub error: Option<String>,
    pub output: Option<Tree>,
    pub minion_output: Option<Tree>,
    pub mirage: Option<MirageDump>,
}

/// `pob_oracle stages`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OracleStages {
    pub format: String,
    pub build: String,
    pub pob_version: Option<String>,
    #[serde(default)]
    pub luajit: Option<LuajitInfo>,
    pub init_env: InitEnvDump,
    /// Whether the captured env is the one PoB kept as `calcsTab.mainEnv`.
    pub main_env_is_captured: bool,
    pub skill_outputs: Vec<SkillOutput>,
}

impl OracleStages {
    pub fn from_json(s: &str) -> Result<Self> {
        serde_json::from_str(s).map_err(|e| OracleError(format!("stages JSON: {e}")))
    }

    /// The main skill's stage, if any.
    #[must_use]
    pub fn main_skill(&self) -> Option<&SkillStage> {
        self.init_env.active_skills.iter().find(|s| s.is_main)
    }
}

// ---- running the oracle ---------------------------------------------------------

/// Which dump to produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dump {
    Outputs,
    Stages,
}

impl Dump {
    #[must_use]
    pub fn command(self) -> &'static str {
        match self {
            Self::Outputs => "outputs",
            Self::Stages => "stages",
        }
    }
}

/// Runs the `pob_oracle` binary, one fresh process per call.
#[derive(Debug, Clone)]
pub struct Oracle {
    pub bin: PathBuf,
    /// Passed as `--pob`; the binary searches itself when unset.
    pub pob_src: Option<PathBuf>,
    /// Read `<cache>/<stem>/{outputs,stages}.json` instead of running, when
    /// present and newer than the build file (`$POB_ORACLE_CACHE`).
    pub cache: Option<PathBuf>,
    /// TP1: route [`Self::run`] through the shared query cache (`runner::cached_query_result`,
    /// `$POB_QUERY_CACHE`). Default true; false always runs a fresh `pob_oracle` process (the
    /// harness test of the binary itself).
    pub query_cache: bool,
}

impl Oracle {
    /// The binary from [`find_oracle_bin`], PoB from `$POB_SRC` / search,
    /// cache from `$POB_ORACLE_CACHE`.
    pub fn locate() -> Result<Self> {
        let bin = find_oracle_bin().ok_or_else(|| {
            OracleError(
                "pob_oracle binary not found: build it (cargo build -p pob-oracle --bin pob_oracle) or set POB_ORACLE_BIN"
                    .into(),
            )
        })?;
        Ok(Self::with_bin(bin))
    }

    #[must_use]
    pub fn with_bin(bin: impl Into<PathBuf>) -> Self {
        Self {
            bin: bin.into(),
            pob_src: None,
            cache: std::env::var_os("POB_ORACLE_CACHE").map(PathBuf::from),
            query_cache: true,
        }
    }

    /// PoB's output for `build` (a PoB build XML).
    pub fn outputs(build: impl AsRef<Path>) -> Result<OracleOutput> {
        Self::locate()?.run_outputs(build.as_ref())
    }

    /// PoB's calc stages for `build`.
    pub fn stages(build: impl AsRef<Path>) -> Result<OracleStages> {
        Self::locate()?.run_stages(build.as_ref())
    }

    pub fn run_outputs(&self, build: &Path) -> Result<OracleOutput> {
        OracleOutput::from_json(&self.run(Dump::Outputs, build)?)
    }

    pub fn run_stages(&self, build: &Path) -> Result<OracleStages> {
        OracleStages::from_json(&self.run(Dump::Stages, build)?)
    }

    fn cached(&self, dump: Dump, build: &Path) -> Option<String> {
        let dir = self.cache.as_ref()?;
        let stem = build.file_stem()?.to_string_lossy();
        let file = dir.join(&*stem).join(format!("{}.json", dump.command()));
        let fresh = |p: &Path| p.metadata().and_then(|m| m.modified()).ok();
        if fresh(&file)? < fresh(build)? {
            return None;
        }
        std::fs::read_to_string(file).ok()
    }

    /// The raw JSON of a dump, from the cache or a fresh `pob_oracle` process.
    ///
    /// TP1: besides the explicit `$POB_ORACLE_CACHE` directory, a dump goes through the shared
    /// content-addressed query cache (`runner::cached_query`, `$POB_QUERY_CACHE`), keyed by the
    /// PoB source tree ([`crate::runner::source_key`]), the build's bytes, its stem (PoB's build
    /// name), the dump and the binary's code. A failed run is not cached.
    pub fn run(&self, dump: Dump, build: &Path) -> Result<String> {
        if let Some(s) = self.cached(dump, build) {
            return Ok(s);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.query_cache {
            if let (Some(src), Ok(bytes)) = (
                self.pob_src.clone().or_else(find_pob_src),
                std::fs::read(build),
            ) {
                const BIN: &str = include_str!("bin/pob_oracle.rs");
                let stem = build
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                return crate::runner::cached_query_result(
                    &src,
                    &bytes,
                    &["pob_oracle dump 1", dump.command(), &stem, BIN],
                    || self.run_process(dump, build).map_err(|e| e.0),
                )
                .map_err(OracleError);
            }
        }
        self.run_process(dump, build)
    }

    /// [`Self::run`] without any cache: a fresh `pob_oracle` process.
    fn run_process(&self, dump: Dump, build: &Path) -> Result<String> {
        static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let tmp = std::env::temp_dir().join(format!(
            "pob_oracle_{}_{}_{}.json",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            dump.command()
        ));
        let mut cmd = std::process::Command::new(&self.bin);
        cmd.arg(dump.command()).arg(build).arg("--json").arg(&tmp);
        if let Some(p) = &self.pob_src {
            cmd.arg("--pob").arg(p);
        }
        let out = cmd
            .output()
            .map_err(|e| OracleError(format!("running {}: {e}", self.bin.display())))?;
        let result = if out.status.success() {
            std::fs::read_to_string(&tmp)
                .map_err(|e| OracleError(format!("{}: {e}", tmp.display())))
        } else {
            Err(OracleError(format!(
                "pob_oracle {} {} failed ({}): {}",
                dump.command(),
                build.display(),
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        };
        let _ = std::fs::remove_file(&tmp);
        result
    }
}

/// Load a dump written earlier (e.g. by `scripts/parity/oracle_dump.sh`).
pub fn load_outputs(path: &Path) -> Result<OracleOutput> {
    let s = std::fs::read_to_string(path)
        .map_err(|e| OracleError(format!("{}: {e}", path.display())))?;
    OracleOutput::from_json(&s)
}

/// Load a stages dump written earlier.
pub fn load_stages(path: &Path) -> Result<OracleStages> {
    let s = std::fs::read_to_string(path)
        .map_err(|e| OracleError(format!("{}: {e}", path.display())))?;
    OracleStages::from_json(&s)
}

// ---- in-process (what the binary runs) -----------------------------------------

/// Boot PoB under LuaJIT, load one build and produce the requested dumps
/// (JSON strings, in the order asked). Call once per process: PoB keeps
/// global state between builds.
#[cfg(all(not(target_arch = "wasm32"), feature = "luajit"))]
pub fn dump_in_process(pob_src: &Path, build: &Path, dumps: &[Dump]) -> Result<Vec<String>> {
    use crate::headless::Engine;
    let xml = std::fs::read_to_string(build)
        .map_err(|e| OracleError(format!("{}: {e}", build.display())))?;
    let name = build
        .file_stem()
        .map_or_else(|| "build".to_owned(), |s| s.to_string_lossy().into_owned());
    let mut pob = crate::jit::JitPob::boot_oracle(pob_src, Some(QUIET_PRELUDE))
        .map_err(|e| OracleError(format!("PoB boot failed: {e}")))?;
    pob.run(ORACLE_LUA)
        .map_err(|e| OracleError(format!("oracle.lua: {e}")))?;
    pob.load_build_xml(&xml, &name)
        .map_err(|e| OracleError(format!("{}: {e}", build.display())))?;
    let mut out = Vec::new();
    for d in dumps {
        let f = match d {
            Dump::Outputs => "return __oracle_outputs()",
            Dump::Stages => "return __oracle_stages()",
        };
        let r = pob
            .run(f)
            .map_err(|e| OracleError(format!("{}: {e}", d.command())))?;
        out.push(r.first().map(|r| r.text().to_owned()).unwrap_or_default());
    }
    Ok(out)
}

/// Before PoB boots: its console output to stderr, so stdout stays JSON.
pub const QUIET_PRELUDE: &str = r##"
print = function(...)
    local parts = {}
    for i = 1, select("#", ...) do parts[i] = tostring((select(i, ...))) end
    io.stderr:write(table.concat(parts, "\t"), "\n")
end
"##;
