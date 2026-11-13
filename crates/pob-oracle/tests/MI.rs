//! Unit MI acceptance (calc doc §11.1 row MI, §9.4 L4): inject a `mirages:in` dump, run the Rust
//! `calcs.mirages` and diff the state (env, GlobalCache, the return value) against PoB's
//! `mirages:out`, for every `calcs.mirages` call of the corpus builds that reaches a mirage family
//! (CalcMirages.lua line probes fired inside it), in the build's main env **and** in the other envs
//! of the run (calcFullDPS's CALCULATOR env, `buildActiveSkill` envs: the stale-CALCULATOR-cache
//! cases, where the GlobalCache entries the call reads are left over from earlier calcs).
//!
//! The callees MI calls that cannot run on an injected state are replayed from line probes
//! (`corpus/MI/stages.toml`) through `calc_mirages::replay_hook`:
//!
//! * `copyActiveSkill` (its `initEnv` is CS2/CS3's, still stubs): the delta `mi:39 → mi:41`, with
//!   `(newSkill, newEnv)` decoded into the live graph as the call's results;
//! * `calcs.perform(newEnv)`: the delta `mi:52 → mi:53` (newSkill is attached to the env for the
//!   replay so its changes land on the live skill); newEnv is another env, which a dump writes as a
//!   `$env` marker (CCR-0064 §1), so its `player.output` is taken from PoB's `mirages:out`, where
//!   MI stores it: `mirage.output` (Mirage Archer / Sacred Wisps; written in full since ORC-X2,
//!   CCR-0146) or `env.player.output` (Reflection / Tawhoa's Chosen take it over), minus the keys
//!   MI writes itself;
//! * `buildActiveSkill` (CA): General's Cry `mi:361 → mi:364`; compareFunc's builds (Reflection /
//!   Tawhoa's Chosen) the delta `mirages:in → mi:39` at the first call. The cached env's output is
//!   another env's table, so the attack time General's Cry reads from it is rebuilt from PoB's
//!   `cooldown` before (`mi:376`) and after (`mi:396`) the `m_max`: 0 where the cooldown
//!   dominates (the `m_max` then takes Rust's own calcSkillCooldown result), PoB's value where
//!   the attack time does; Rust's pre-`m_max` cooldown is also compared with `mi:376` directly
//!   (seam `generalsCryCooldown`);
//! * `isTriggered` (TR-A, a stub): the Lua expression (CalcTriggers.lua:41-43), inline.
//!
//! Reflection / Tawhoa's Chosen: the final `env.player.output` is grafted from `mirages:out`
//! (above), so for those families its comparison is by construction (a replay limit, CCR-0090
//! §4.3); what MI writes into it (Tawhoa's Chosen 236-239) is still checked. Mirage Archer /
//! Sacred Wisps: `mirage.output` likewise (CCR-0146: the dumps write it in full since ORC-X2). Any
//! `pob_calc::stub` reached during a call, and a Rust `true` where PoB returned nothing
//! (`ret.n = 0`), make the call divergent.
//!
//! At every replayed call the live state is also diffed against the call's `in` probe (before) and
//! its `out` probe (after): a mismatch before a call is MI's code (divergent), one that a replay adds
//! is the replay's (replay-limited). Gate: every comparable call clean, no divergent or failed one.
//!
//! Dumps come from the shared probe cache (`testkit::probe_selection`: content-keyed, probed in a
//! fresh `pob_oracle probe` process when absent, under the machine-wide oracle lock). Knobs:
//! `MI_BUILDS=stem1,stem2`, `MI_LIMIT=n`, `MI_VERBOSE=1`. A missing input skips with a message;
//! with `POB_ORACLE_REQUIRE=1` (or `CI`) it fails instead.
#![cfg(feature = "luajit")]

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_calc::active_skill::{ActiveSkill, ActiveSkillRef};
use pob_calc::env::{Env, EnvRef, Mode};
use pob_calc::modules::calc_mirages::replay_hook::{self, Call, Ret};
use pob_calc::{Table, Val};
use pob_gamedata::SkillType;
use pob_oracle::diff::{Mismatch, MismatchKind};
use pob_oracle::oracle::find_parity_dir;
use pob_oracle::replay::{self, Replay};
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;
use serde_json::{json, Value as J};

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

fn stem(p: &Path) -> &str {
    p.file_stem().and_then(|s| s.to_str()).unwrap_or_default()
}

fn corpus(parity: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(parity.join("builds"))
        .map(|r| {
            r.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "xml"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    if let Ok(list) = std::env::var("MI_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| want.contains(&stem(p)));
    }
    if let Some(n) = std::env::var("MI_LIMIT")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
    {
        v.truncate(n);
    }
    v
}

fn stages_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus/MI/stages.toml")
}

/// The build's dumps: every `mirages:in/out` and `mi:*` hit of the run (not only the main env's),
/// from the shared probe cache (`testkit::probe_selection`, probed in a fresh `pob_oracle probe`
/// process when absent). MI1: the entry is content-keyed (the patched source's sha, which covers
/// `corpus/MI/stages.toml`, the patcher and the dump Lua; the build's bytes; the selection; the
/// probe's Rust code), so a harness or dump-format change probes again. The earlier
/// `target/tmp/mi-probe/<stem>/` directory was reused whenever it held a `hits.json`, whatever
/// made it. Hold the result while reading the dumps.
fn probe_dumps(xml: &Path) -> Result<pob_oracle::runner::ProbeDumps, String> {
    let opts = pob_oracle::runner::ProbeOptions {
        select: vec!["mirages:*".to_owned(), "mi:*".to_owned()],
        max_dumps: 400,
        provenance: false,
    };
    testkit::probe_selection(
        Path::new(env!("CARGO_BIN_EXE_pob_oracle")),
        xml,
        Some(&stages_file()),
        &opts,
    )
}

/// The hits of one `calcs.mirages` call: its `in`/`out` and the line probes fired inside it.
struct MirageCall {
    hit_in: String,
    din: PathBuf,
    dout: PathBuf,
    /// line probe id (`mi:41`) → dump file
    lines: BTreeMap<String, PathBuf>,
    main: bool,
}

/// Group the dumps of `hits.json` by `mirages` call (a line probe's innermost enclosing function
/// probe hit is the call's `mirages:in#n`).
fn mirage_calls(state: &Path) -> Result<Vec<MirageCall>, String> {
    let dir = state.parent().ok_or("bad dir")?;
    // a cache entry's files are `<name>.zst`, an uncached run's (`POB_QUERY_CACHE=off`) plain
    let file = |rel: &str| -> PathBuf {
        let z = dir.join(format!("{rel}.zst"));
        if z.is_file() {
            z
        } else {
            dir.join(rel)
        }
    };
    let hits: J =
        serde_json::from_str(&pob_oracle::schema::read_text(&file("hits.json")).map_err(|e| e.0)?)
            .map_err(|e| e.to_string())?;
    let main_env = hits["mainEnv"].as_str().unwrap_or_default().to_owned();
    if hits["skipped"].as_u64().unwrap_or(0) > 0 {
        return Err(format!(
            "{} selected hits skipped (max dumps)",
            hits["skipped"]
        ));
    }
    let mut ins: BTreeMap<String, (PathBuf, bool)> = BTreeMap::new();
    let mut outs: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut lines: BTreeMap<String, BTreeMap<String, PathBuf>> = BTreeMap::new();
    for d in hits["dumps"].as_array().into_iter().flatten() {
        let hit = d["hit"].as_str().unwrap_or_default();
        let file = file(d["file"].as_str().unwrap_or_default());
        let (id, n) = hit.split_once('#').unwrap_or((hit, ""));
        match id {
            "mirages:in" => {
                ins.insert(
                    n.to_owned(),
                    (file, d["env"].as_str() == Some(main_env.as_str())),
                );
            }
            "mirages:out" => {
                outs.insert(n.to_owned(), file);
            }
            _ if id.starts_with("mi:") => {
                let Some(call) = d["stack"]
                    .as_array()
                    .and_then(|s| s.last())
                    .and_then(J::as_str)
                    .and_then(|s| s.strip_prefix("mirages:in#"))
                else {
                    return Err(format!("{hit}: not inside a mirages call"));
                };
                lines
                    .entry(call.to_owned())
                    .or_default()
                    .insert(id.to_owned(), file);
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for (n, (din, main)) in ins {
        let l = lines.remove(&n).unwrap_or_default();
        // calls that reach no mirage family are trivial; the main env's is kept as a control
        if l.is_empty() && !main {
            continue;
        }
        let Some(dout) = outs.get(&n).cloned() else {
            continue; // the call raised (no :out)
        };
        out.push(MirageCall {
            hit_in: format!("mirages:in#{n}"),
            din,
            dout,
            lines: l,
            main,
        });
    }
    Ok(out)
}

fn load(p: &Path) -> Result<StateDump, String> {
    StateDump::load(p).map_err(|e| format!("{}: {e}", p.display()))
}

// ---- build-model state a dump does not carry ----------------------------------------------------

/// `"$id" -> "$uuid"` of every active skill in a dump root.
fn skill_uuids(j: &J, out: &mut BTreeMap<u64, String>) {
    match j {
        J::Object(m) => {
            if let (Some(id), Some(u)) = (
                m.get("$id").and_then(J::as_u64),
                m.get("$uuid").and_then(J::as_str),
            ) {
                out.insert(id, u.to_owned());
            }
            for v in m.values() {
                skill_uuids(v, out);
            }
        }
        J::Array(a) => a.iter().for_each(|v| skill_uuids(v, out)),
        _ => {}
    }
}

/// `cacheSkillUUID` (Common.lua:913-918) finds a skill's group in
/// `build.skillsTab.socketGroupList` by identity; the dump writes the build as a marker, so the
/// list is rebuilt from the uuids PoB computed for the env's active skills (their last component
/// is the group index; other slots get empty groups).
fn fill_socket_groups(din: &StateDump, env: &EnvRef) -> Result<(), String> {
    let mut by_id = BTreeMap::new();
    skill_uuids(&din.root, &mut by_id);
    let list = &din.root["env"]["player"]["activeSkillList"];
    let skills: Vec<ActiveSkillRef> = env.player.activeSkillList.get().with(|v| v.clone());
    let mut slots: BTreeMap<usize, pob_calc::model::SocketGroupRef> = BTreeMap::new();
    for (i, skill) in skills.iter().enumerate() {
        let j = &list[i];
        let uuid = j
            .get("$uuid")
            .and_then(J::as_str)
            .map(str::to_owned)
            .or_else(|| {
                j.get("$ref")
                    .and_then(J::as_u64)
                    .and_then(|r| by_id.get(&r).cloned())
            });
        let (Some(uuid), Some(group)) = (uuid, skill.socketGroup.get()) else {
            continue;
        };
        let idx: usize = uuid
            .rsplit('_')
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| format!("bad uuid {uuid}"))?;
        if let Some(g) = slots.get(&idx) {
            if !Rc::ptr_eq(g, &group) {
                return Err(format!("two socket groups claim index {idx}"));
            }
        }
        slots.insert(idx, group);
    }
    let n = slots.keys().max().copied().unwrap_or(0);
    let list: Vec<pob_calc::model::SocketGroupRef> = (1..=n)
        .map(|i| {
            slots
                .get(&i)
                .cloned()
                .unwrap_or_else(|| Rc::new(pob_calc::model::SocketGroup::default()))
        })
        .collect();
    env.build
        .skillsTab
        .socketGroupList
        .set(pob_lua::Shared::new(list));
    Ok(())
}

// ---- dump surgery for the replays ----------------------------------------------------------------

/// `mi:41` with `(newSkill, newEnv)` as the packed results `ret` of the copyActiveSkill replay.
fn with_copy_ret(d: &StateDump) -> Result<StateDump, String> {
    let mut d = d.clone();
    let l = d
        .root
        .get_mut("locals")
        .and_then(J::as_object_mut)
        .ok_or("mi:41: no locals")?;
    let skill = l.remove("newSkill").ok_or("mi:41: no newSkill")?;
    let env = l.remove("newEnv").ok_or("mi:41: no newEnv")?;
    l.insert("ret".to_owned(), json!({"n": 2, "$seq": [skill, env]}));
    Ok(d)
}

/// Key under which newSkill is attached to the env (`env.extra`) for the perform replay.
const ATTACH: &str = "mi_newSkill";

/// `mi:52` / `mi:53` with the `newSkill` local moved to `env.mi_newSkill` (the replay's pairing
/// does not follow locals; the live skill is attached at the same path).
fn with_attached_skill(d: &StateDump) -> Result<StateDump, String> {
    let mut d = d.clone();
    let skill = d
        .root
        .get_mut("locals")
        .and_then(J::as_object_mut)
        .and_then(|l| l.remove("newSkill"))
        .ok_or("no newSkill local")?;
    let env = d
        .root
        .get_mut("env")
        .and_then(J::as_object_mut)
        .ok_or("no env")?;
    env.insert(ATTACH.to_owned(), skill);
    Ok(d)
}

/// Key under which PoB's taken-over output is grafted into `mi:53` (see [`graft_output`]).
const GRAFT: &str = "mi_newEnvOutput";

/// `$id` → canonical path (the path of the full occurrence) of a dump root.
fn id_paths(root: &J) -> BTreeMap<u64, String> {
    fn go(path: &str, v: &J, out: &mut BTreeMap<u64, String>) {
        if let J::Object(m) = v {
            if let Some(id) = m.get("$id").and_then(J::as_u64) {
                out.entry(id).or_insert_with(|| path.to_owned());
            }
            if pob_oracle::schema::is_marker(m) {
                return;
            }
        }
        for (p, c) in pob_oracle::schema::children(path, v) {
            go(&p, c, out);
        }
    }
    let mut out = BTreeMap::new();
    go("", root, &mut out);
    out
}

/// The full occurrence of the table at `path` in `d` (a `$ref` there is followed): its canonical
/// path and the table.
fn full_table(d: &StateDump, path: &str) -> Option<(String, J)> {
    let v = pob_oracle::schema::get_path(&d.root, path)?;
    match v
        .as_object()
        .filter(|m| m.len() == 1)
        .and_then(|m| m.get("$ref"))
        .and_then(J::as_u64)
    {
        Some(n) => {
            let canon = id_paths(&d.root).remove(&n)?;
            let t = pob_oracle::schema::get_path(&d.root, &canon)?.clone();
            Some((canon, t))
        }
        None => Some((path.to_owned(), v.clone())),
    }
}

/// `output` (the subtree of `mirages:out` at `base`: `env.player.output`, or
/// `env.player.mainSkill.mirage.output`) rewritten into `mi53`'s identity space: a reference to a
/// table outside the subtree becomes a reference to the table at the same path in `mi53`
/// (`env.player.mainSkill` of `mirages:out` is `locals.newSkill` there); the subtree's own ids are
/// moved past `mi53`'s.
fn graft_output(mi53: &StateDump, out: &StateDump, base: &str, output: &J) -> Result<J, String> {
    let ids53: BTreeMap<String, u64> = id_paths(&mi53.root)
        .into_iter()
        .map(|(id, p)| (p, id))
        .collect();
    let ids_out = id_paths(&out.root);
    let offset =
        1 + ids53.values().copied().max().unwrap_or(0) + ids_out.keys().copied().max().unwrap_or(0);
    fn rw(
        v: &J,
        ids53: &BTreeMap<String, u64>,
        ids_out: &BTreeMap<u64, String>,
        offset: u64,
        base: &str,
    ) -> Result<J, String> {
        match v {
            J::Array(a) => a
                .iter()
                .map(|x| rw(x, ids53, ids_out, offset, base))
                .collect::<Result<Vec<_>, _>>()
                .map(J::Array),
            J::Object(m) => {
                if m.len() == 1 {
                    if let Some(n) = m.get("$ref").and_then(J::as_u64) {
                        let p = ids_out
                            .get(&n)
                            .ok_or_else(|| format!("dangling $ref {n}"))?;
                        if p == base
                            || p.starts_with(&format!("{base}."))
                            || p.starts_with(&format!("{base}["))
                        {
                            return Ok(json!({ "$ref": n + offset }));
                        }
                        let alt = p
                            .strip_prefix("env.player.mainSkill")
                            .map(|r| format!("locals.newSkill{r}"));
                        let id = ids53
                            .get(p)
                            .or_else(|| alt.as_ref().and_then(|a| ids53.get(a)))
                            .ok_or_else(|| format!("no table at {p} in mi:53"))?;
                        return Ok(json!({ "$ref": id }));
                    }
                }
                if pob_oracle::schema::is_marker(m) {
                    return Ok(v.clone());
                }
                let mut o = serde_json::Map::new();
                for (k, x) in m {
                    if k == "$id" {
                        let n = x.as_u64().ok_or("bad $id")?;
                        o.insert(k.clone(), json!(n + offset));
                    } else {
                        o.insert(k.clone(), rw(x, ids53, ids_out, offset, base)?);
                    }
                }
                Ok(J::Object(o))
            }
            other => Ok(other.clone()),
        }
    }
    rw(output, &ids53, &ids_out, offset, base)
}

/// The `$env` marker id of the `newEnv` local of a line probe dump.
fn new_env_id(d: &StateDump) -> Option<String> {
    d.root["locals"]["newEnv"]["$env"]
        .as_str()
        .map(str::to_owned)
}

// ---- one call ------------------------------------------------------------------------------------

#[derive(Default)]
struct Log {
    replayed: Vec<String>,
    problems: Vec<String>,
    /// (call, mismatches vs the call's `in` probe before, vs its `out` probe after)
    steps: Vec<(String, usize, usize)>,
    first_mismatches: Vec<String>,
    /// The largest number of identity-only mismatches seen at one comparison (see
    /// [`split_identity`]).
    identity_only: usize,
    /// General's Cry: Rust's `cooldown` before the `m_max` (the `generalsCryCooldown` seam)
    gc_cooldown: Vec<f64>,
}

fn verbose() -> bool {
    std::env::var_os("MI_VERBOSE").is_some()
}

fn real(ms: &[Mismatch]) -> Vec<&Mismatch> {
    ms.iter()
        .filter(|m| !matches!(m.kind, MismatchKind::LuaOrder))
        .collect()
}

fn fmt_mismatch(m: &Mismatch) -> String {
    let clip = |x: &Option<J>| {
        let s = x.as_ref().map_or("-".to_owned(), J::to_string);
        if s.len() > 160 {
            let end = s.char_indices().nth(160).map_or(s.len(), |(i, _)| i);
            format!("{}…", &s[..end])
        } else {
            s
        }
    };
    format!(
        "{} {:?} pob={} ours={}",
        m.path,
        m.kind,
        clip(&m.pob),
        clip(&m.ours)
    )
}

// ---- identity-only mismatches ---------------------------------------------------------------------

/// Every table of a resolved (`resolve_ids`) tree by its dump path.
fn index_paths<'a>(path: &str, v: &'a J, out: &mut HashMap<String, &'a J>) {
    out.insert(path.to_owned(), v);
    if let J::Object(m) = v {
        if pob_oracle::schema::is_marker(m) || (m.len() == 1 && m.contains_key("$ref")) {
            return;
        }
    }
    for (p, c) in pob_oracle::schema::children(path, v) {
        index_paths(&p, c, out);
    }
}

/// Whether two values of the resolved trees `ta` / `tb` have the same content once every path
/// reference is followed (`depth` bounds cycles; at the bound two tables count as equal).
fn content_eq(
    a: &J,
    b: &J,
    ta: &HashMap<String, &J>,
    tb: &HashMap<String, &J>,
    depth: usize,
) -> bool {
    let deref = |v: &J, t: &HashMap<String, &J>| -> Option<J> {
        match v
            .as_object()
            .and_then(|m| m.get("$ref"))
            .and_then(J::as_str)
        {
            Some(p) if v.as_object().is_some_and(|m| m.len() == 1) => {
                t.get(p).map(|x| (*x).clone())
            }
            _ => Some(v.clone()),
        }
    };
    let (Some(a), Some(b)) = (deref(a, ta), deref(b, tb)) else {
        return false;
    };
    match (&a, &b) {
        (J::Object(x), J::Object(y)) => {
            if depth == 0 {
                return true;
            }
            x.len() == y.len()
                && x.iter().all(|(k, v)| {
                    y.get(k)
                        .is_some_and(|w| content_eq(v, w, ta, tb, depth - 1))
                })
        }
        (J::Array(x), J::Array(y)) => {
            if depth == 0 {
                return true;
            }
            x.len() == y.len()
                && x.iter()
                    .zip(y)
                    .all(|(v, w)| content_eq(v, w, ta, tb, depth - 1))
        }
        _ => a == b,
    }
}

/// The mismatches whose two sides differ in content, not only in identity (which table a path
/// refers to, or where a shared table is written in full): a replay rebuilds the tables a callee
/// replaced by fresh ones in place (`pob_oracle::replay` "Limits", CCR-0077 §6), so after a
/// replayed call the same content can sit at another table. Returns (content, identity-only).
fn split_identity<'m>(
    ms: &'m [&'m Mismatch],
    pob_root: &J,
    ours_root: &J,
) -> (Vec<&'m Mismatch>, usize) {
    let pob = pob_oracle::schema::resolve_ids(pob_root);
    let ours = pob_oracle::schema::resolve_ids(ours_root);
    let (mut ta, mut tb) = (HashMap::new(), HashMap::new());
    index_paths("", &pob, &mut ta);
    index_paths("", &ours, &mut tb);
    let mut content = Vec::new();
    let mut ident = 0;
    for m in ms {
        let same = matches!(m.kind, MismatchKind::Value)
            && match (&m.pob, &m.ours) {
                (Some(a), Some(b)) => content_eq(a, b, &ta, &tb, 10),
                _ => false,
            };
        if same {
            ident += 1;
        } else {
            content.push(*m);
        }
    }
    (content, ident)
}

/// Live state (env + the given locals) vs a dump: the non-`LUA-ORDER` mismatches that differ in
/// content (identity-only ones are counted in the log).
fn live_vs(
    d: &StateDump,
    env: &EnvRef,
    locals: &[(String, Val)],
    log: &mut Log,
    what: &str,
) -> usize {
    let mut all = vec![(
        "env".to_owned(),
        Val::Obj(Rc::clone(env) as Rc<dyn std::any::Any>),
    )];
    all.extend(locals.iter().cloned());
    let ms = testkit::diff_against(d, Some(env), &all, "");
    let r = real(&ms);
    let pob_locals: Vec<String> = d
        .locals()
        .and_then(J::as_object)
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    let kept: Vec<(String, Val)> = all
        .iter()
        .filter(|(k, _)| pob_locals.contains(k))
        .cloned()
        .collect();
    let ours = testkit::export_after(Some(env), &kept);
    let (content, ident) = split_identity(&r, &d.root, &ours);
    log.identity_only = log.identity_only.max(ident);
    if !content.is_empty() && (verbose() || log.first_mismatches.len() < 12) {
        for m in content.iter().take(if verbose() { usize::MAX } else { 12 }) {
            log.first_mismatches
                .push(format!("{what}: {}", fmt_mismatch(m)));
        }
    }
    content.len()
}

/// Apply the recorded delta `din -> dout` onto the live state (`pob_oracle::replay`).
fn apply_delta(din: &StateDump, dout: &StateDump, env: &EnvRef) -> Result<replay::Applied, String> {
    let mut r = Replay::default();
    r.calls.insert(
        din.hit.clone(),
        replay::Call {
            din: din.clone(),
            dout: dout.clone(),
        },
    );
    let a = r.apply_report(&din.hit, env)?;
    if a.notes.is_empty() {
        Ok(a)
    } else {
        Err(format!("replay notes: {:?}", a.notes))
    }
}

/// [`live_vs`] on the env and GlobalCache only (the dump's locals other than `env` dropped).
fn live_vs_env(d: &StateDump, env: &EnvRef, log: &mut Log, what: &str) -> usize {
    let mut d = d.clone();
    if let Some(l) = d.root.get_mut("locals").and_then(J::as_object_mut) {
        l.retain(|k, _| k == "env");
    }
    live_vs(&d, env, &[], log, what)
}

/// The `isTriggered` expression (CalcTriggers.lua:41-43; TR-A's function is still a stub).
fn is_triggered(skill: &ActiveSkill) -> bool {
    let sd = skill.skillData.get();
    let ae = skill.activeEffect.get();
    sd.truthy("triggeredByUnique")
        || sd.truthy("triggered")
        || skill
            .skillTypes
            .get()
            .with(|s| s.has(SkillType::InbuiltTrigger) || s.has(SkillType::Triggered))
        // `skill.activeEffect.grantedEffect.triggered`: no granted effect has the key at the pin
        // (the strict skills schema has no such field), so it is nil
        || ae.srcInstance.get().is_some_and(|g| g.triggered.get())
}

struct CallResult {
    /// Final mismatches that differ in content (identity-only ones: `final_identity`).
    mismatches: Vec<Mismatch>,
    final_identity: usize,
    /// State differences between PoB's `mirages:in` and `mirages:out` (non-vacuity).
    pob_changes: usize,
    log: Log,
    stubs: Vec<String>,
    family: String,
}

/// A table for the env the dumps write as `{"$env": id}`: a placeholder that exports as that
/// marker, with its `player.output` exported as PoB's `$foreign` marker of it.
fn placeholder_env(env: &EnvRef, id: &str) -> EnvRef {
    let e = Env::new(
        &env.build,
        Mode::CALCULATOR,
        env.r#override.clone(),
        env.spec.get(),
    );
    pob_calc::probe::with_registry(|r| {
        r.set_foreign(
            Rc::clone(&e) as Rc<dyn std::any::Any>,
            json!({ "$env": id }),
        );
    });
    e
}

fn mark_output_foreign(e: &EnvRef, id: &str) {
    let out = e.player.output.get();
    pob_calc::probe::with_registry(|r| {
        r.set_foreign_table(
            &out,
            json!({"$foreign": {"env": id, "path": "player.output"}}),
        );
    });
}

fn run_call(c: &MirageCall) -> Result<CallResult, String> {
    let din = load(&c.din)?;
    let dout = load(&c.dout)?;
    // non-vacuity: what PoB's call changed (env + GlobalCache), which the Rust run had to match
    let strip = |d: &StateDump| {
        let mut r = d.root.clone();
        if let Some(m) = r.as_object_mut() {
            m.remove("locals");
        }
        pob_oracle::schema::resolve_ids(&r)
    };
    let pob_changes = pob_oracle::diff::diff_trees(
        &strip(&din),
        &strip(&dout),
        &pob_oracle::diff::DiffOptions::strict(),
    )
    .len();
    let line = |id: &str| -> Result<Option<StateDump>, String> {
        c.lines.get(id).map(|p| load(p)).transpose()
    };
    let (d39, d41, d52, d53) = (
        line("mi:39")?,
        line("mi:41")?,
        line("mi:52")?,
        line("mi:53")?,
    );
    let (d361, d364, d376, d396) = (
        line("mi:361")?,
        line("mi:364")?,
        line("mi:376")?,
        line("mi:396")?,
    );

    let build = testkit::empty_build();
    let inj = pob_oracle::inject::inject(&din, &build)?;
    let env = inj.env.clone().ok_or("no env in mirages:in")?;
    fill_socket_groups(&din, &env)?;
    let family = env
        .player
        .mainSkill
        .get()
        .map(|s| s.activeEffect.get().grantedEffect.get().name.to_string())
        .unwrap_or_default();
    // newEnv's final `player.output` (`env.player.output = { }`, CalcPerform.lua:1305, then
    // filled by the nested perform) is another env's table, which no line probe reaches (`newEnv`
    // is a `$env` marker there), so the perform replay grafts PoB's copy of it from `mirages:out`
    // (`(base, table)`):
    // * Reflection / Tawhoa's Chosen: `env.player.output = newEnv.player.output`
    //   (CalcMirages.lua 157, 235), minus the keys MI writes itself (Tawhoa's Chosen, 236-239);
    // * Mirage Archer / Sacred Wisps: `env.player.mainSkill.mirage.output = newEnv.player.output`
    //   (100, 326). MI1: since ORC-X2 the dumps write another env's `*.output` in full where they
    //   reach it (state_dump.lua `inlineForeign`), so `mirages:out` holds it; the earlier
    //   `$foreign` placeholder no longer matches. A `$foreign` marker there (a dump of an older
    //   encoder) keeps the placeholder.
    // Either way the output's content is PoB's by construction; what MI checks is that the table
    // lands where MI puts it (and every other path).
    let graft: Option<(String, J)> = if d53.is_none() {
        None
    } else if family == "Reflection" || family == "Tawhoa's Chosen" {
        let mut o = dout.root["env"]["player"]["output"].clone();
        if family == "Tawhoa's Chosen" {
            if let Some(m) = o.as_object_mut() {
                for k in [
                    "Speed",
                    "TriggerRateCap",
                    "EffectiveSourceRate",
                    "SkillTriggerRate",
                ] {
                    m.remove(k);
                }
            }
        }
        Some(("env.player.output".to_owned(), o))
    } else {
        let path = "env.player.mainSkill.mirage.output";
        let (base, o) = full_table(&dout, path)
            .ok_or_else(|| format!("perform ran, but mirages:out has no {path}"))?;
        let marker = o.as_object().is_some_and(pob_oracle::schema::is_marker);
        (!marker).then_some((base, o))
    };

    // General's Cry: the attack time the cached env's output gives (`HitTime or Time`; the
    // cached env's output is another env's table, a `$foreign` marker in every dump), rebuilt
    // from PoB's `cooldown` before (mi:376) and after (mi:396) the `m_max` at 395:
    // * cd376 == cd396 (the cooldown dominates): the attack time is any x <= cd396; x = 0 makes
    //   the `m_max` pick Rust's own cooldown, so the DPS MORE mod at 396 checks Rust's
    //   calcSkillCooldown independently of PoB's result;
    // * cd376 < cd396 (the attack time dominates): x = cd396 exactly.
    // Rust's pre-max cooldown is also compared with cd376 directly (the `generalsCryCooldown`
    // seam), which covers the second case.
    let gc_uuid: Option<String> = d361
        .as_ref()
        .and_then(|d| d.root["locals"]["uuid"].as_str().map(str::to_owned));
    let num_local = |d: &Option<StateDump>| {
        d.as_ref()
            .and_then(|d| pob_oracle::schema::decode_num(&d.root["locals"]["cooldown"]))
    };
    let gc_cd_pre: Option<f64> = num_local(&d376);
    let gc_cd: Option<f64> = num_local(&d396);
    let gc_attack_time: Option<f64> = match (gc_cd_pre, gc_cd) {
        (Some(pre), Some(post)) if pre == post => Some(0.0),
        (Some(pre), Some(post)) if pre < post => Some(post),
        (Some(pre), Some(post)) => {
            return Err(format!(
                "General's Cry: PoB's cooldown went from {pre} (mi:376) down to {post} (mi:396)"
            ))
        }
        (None, None) => None,
        (pre, post) => {
            return Err(format!(
                "General's Cry: cooldown probes incomplete (mi:376 {pre:?}, mi:396 {post:?})"
            ))
        }
    };
    let fix_gc_output = {
        let env = env.clone();
        let gc_uuid = gc_uuid.clone();
        move || {
            if let (Some(u), Some(t)) = (&gc_uuid, gc_attack_time) {
                if let Some(e) =
                    pob_calc::global_cache::cachedData(env.mode, pob_lua::Name::from(u.as_str()))
                {
                    let out = e.Env.player.output.get();
                    if out.get("HitTime").is_nil() && out.get("Time").is_nil() {
                        out.set("Time", t);
                    }
                }
            }
        }
    };
    fix_gc_output();

    let log = Rc::new(RefCell::new(Log::default()));
    let l = Rc::clone(&log);
    let new_env_id41 = d41.as_ref().and_then(new_env_id);
    let mut built_compare = false;
    let main_env = env.clone();
    let dout_h = dout.clone();
    let hook: replay_hook::Hook = Box::new(move |call| {
        let mut lg = l.borrow_mut();
        match call {
            Call::isTriggered { skill } => Some(Ret::Bool(is_triggered(skill))),
            Call::copyActiveSkill { env, skill, .. } => {
                let (Some(a), Some(b)) = (&d39, &d41) else {
                    lg.problems
                        .push("copyActiveSkill called, but PoB did not call it".into());
                    return None;
                };
                let b2 = match with_copy_ret(b) {
                    Ok(x) => x,
                    Err(e) => {
                        lg.problems.push(e);
                        return None;
                    }
                };
                let mut out: Option<Ret> = None;
                let ms_local = vec![(
                    "mirageSkill".to_owned(),
                    Val::Obj(Rc::clone(skill) as Rc<dyn std::any::Any>),
                )];
                let before = live_vs(a, env, &ms_local, &mut lg, "before copyActiveSkill");
                let mut r = Replay::default();
                r.calls.insert(
                    a.hit.clone(),
                    replay::Call {
                        din: a.clone(),
                        dout: b2,
                    },
                );
                match r.apply_report(&a.hit, env) {
                    Ok(applied) => {
                        if !applied.notes.is_empty() {
                            lg.problems
                                .push(format!("copyActiveSkill replay notes: {:?}", applied.notes));
                        }
                        let ret = applied.ret.and_then(|v| v.table());
                        let skill = ret.as_ref().and_then(|t| t.get(1).obj::<ActiveSkill>());
                        let id = new_env_id41.clone().unwrap_or_default();
                        let new_env = ret
                            .as_ref()
                            .and_then(|t| t.get(2).obj::<Env>())
                            .unwrap_or_else(|| placeholder_env(env, &id));
                        match skill {
                            Some(s) => out = Some(Ret::Copy(s, new_env)),
                            None => lg
                                .problems
                                .push("copyActiveSkill replay: newSkill not decoded".into()),
                        }
                    }
                    Err(e) => lg
                        .problems
                        .push(format!("copyActiveSkill replay failed: {e}")),
                }
                if let Some(Ret::Copy(s, e)) = &out {
                    let locals = vec![
                        (
                            "newSkill".to_owned(),
                            Val::Obj(Rc::clone(s) as Rc<dyn std::any::Any>),
                        ),
                        (
                            "newEnv".to_owned(),
                            Val::Obj(Rc::clone(e) as Rc<dyn std::any::Any>),
                        ),
                    ];
                    let after = live_vs(b, env, &locals, &mut lg, "after copyActiveSkill");
                    lg.steps.push(("copyActiveSkill".into(), before, after));
                    lg.replayed.push("copyActiveSkill".into());
                }
                out
            }
            Call::perform { env: new_env } => {
                let (Some(a), Some(b)) = (&d52, &d53) else {
                    lg.problems
                        .push("perform called, but PoB did not call it".into());
                    return None;
                };
                let main = Rc::clone(&main_env);
                let skill = new_env.player.mainSkill.get();
                let Some(skill) = skill else {
                    lg.problems.push("perform: newEnv has no mainSkill".into());
                    return Some(Ret::Unit);
                };
                let locals = vec![
                    (
                        "newSkill".to_owned(),
                        Val::Obj(Rc::clone(&skill) as Rc<dyn std::any::Any>),
                    ),
                    (
                        "newEnv".to_owned(),
                        Val::Obj(Rc::clone(new_env) as Rc<dyn std::any::Any>),
                    ),
                ];
                let before = live_vs(a, &main, &locals, &mut lg, "before perform");
                let (a2, mut b2) = match (with_attached_skill(a), with_attached_skill(b)) {
                    (Ok(x), Ok(y)) => (x, y),
                    (Err(e), _) | (_, Err(e)) => {
                        lg.problems.push(e);
                        return Some(Ret::Unit);
                    }
                };
                if let Some((base, g)) = &graft {
                    match graft_output(b, &dout_h, base, g) {
                        Ok(j) => {
                            if let Some(e) = b2.root.get_mut("env").and_then(J::as_object_mut) {
                                e.insert(GRAFT.to_owned(), j);
                            }
                        }
                        Err(e) => {
                            lg.problems.push(format!("output graft: {e}"));
                            return Some(Ret::Unit);
                        }
                    }
                }
                main.extra
                    .set(ATTACH, Val::Obj(Rc::clone(&skill) as Rc<dyn std::any::Any>));
                let mut r = Replay::default();
                r.calls.insert(
                    a2.hit.clone(),
                    replay::Call {
                        din: a2.clone(),
                        dout: b2,
                    },
                );
                match r.apply_report(&a2.hit, &main) {
                    Ok(applied) => {
                        if !applied.notes.is_empty() {
                            lg.problems
                                .push(format!("perform replay notes: {:?}", applied.notes));
                        }
                    }
                    Err(e) => lg.problems.push(format!("perform replay failed: {e}")),
                }
                // the replay writes the attached skill back at env.mi_newSkill; detach it
                let attached = main.extra.get(ATTACH).obj::<ActiveSkill>();
                main.extra.set(ATTACH, Val::Nil);
                if !attached.is_some_and(|s| Rc::ptr_eq(&s, &skill)) {
                    lg.problems
                        .push("perform replay replaced newSkill instead of updating it".into());
                }
                // `env.player.output = { }` (CalcPerform.lua:1305): newEnv's output after perform
                let id = new_env_id(b).unwrap_or_default();
                match main.extra.get(GRAFT).table() {
                    Some(t) => {
                        main.extra.set(GRAFT, Val::Nil);
                        new_env.player.output.set(t);
                    }
                    None => {
                        if graft.is_some() {
                            lg.problems.push("output graft did not land".into());
                        }
                        new_env.player.output.set(Table::new());
                        mark_output_foreign(new_env, &id);
                    }
                }
                let after = live_vs(b, &main, &locals, &mut lg, "after perform");
                lg.steps.push(("perform".into(), before, after));
                lg.replayed.push("perform".into());
                Some(Ret::Unit)
            }
            Call::buildActiveSkill { env: e, .. } => {
                if let (Some(a), Some(b)) = (&d361, &d364) {
                    let main = Rc::clone(e);
                    let what = "buildActiveSkill (General's Cry)";
                    let before = live_vs_env(a, &main, &mut lg, &format!("before {what}"));
                    if let Err(err) = apply_delta(a, b, &main) {
                        lg.problems.push(format!("{what} replay failed: {err}"));
                    }
                    fix_gc_output();
                    let after = live_vs_env(b, &main, &mut lg, &format!("after {what}"));
                    lg.steps.push((what.to_owned(), before, after));
                    lg.replayed.push(what.to_owned());
                    return Some(Ret::Unit);
                }
                if let Some(b) = &d39 {
                    if built_compare {
                        return Some(Ret::Unit);
                    }
                    built_compare = true;
                    let main = Rc::clone(e);
                    // the compareFunc loop's builds, all at once: mirages:in -> mi:39
                    let what = "buildActiveSkill (compareFunc)";
                    let before = live_vs(&din, &main, &[], &mut lg, &format!("before {what}"));
                    if let Err(err) = apply_delta(&din, b, &main) {
                        lg.problems.push(format!("{what} replay failed: {err}"));
                    }
                    // mi:39 also dumps the loop's result `mirageSkill` (a GlobalCache entry's
                    // skill, of another env: a marker); it is compared at copyActiveSkill
                    let after = live_vs_env(b, &main, &mut lg, &format!("after {what}"));
                    lg.steps.push((what.to_owned(), before, after));
                    lg.replayed.push(what.to_owned());
                    return Some(Ret::Unit);
                }
                lg.problems
                    .push("buildActiveSkill called, but PoB did not call it".into());
                None
            }
            Call::generalsCryCooldown { cooldown } => {
                lg.gc_cooldown.push(*cooldown);
                None
            }
            Call::calcMultiSpellRotationImpact { .. } => {
                lg.problems
                    .push("calcMultiSpellRotationImpact (TR-A stub) reached".into());
                None
            }
        }
    });
    pob_calc::stub::stub_reset();
    replay_hook::set(Some(hook));
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pob_calc::calcs::mirages(&env)
    }));
    replay_hook::set(None);
    let stubs: Vec<String> = pob_calc::stub::stub_report()
        .iter()
        .map(|(site, n)| format!("{site} x{n}"))
        .collect();
    let ours = match run {
        Ok(b) => b,
        Err(p) => {
            let msg = p
                .downcast_ref::<pob_lua::LuaError>()
                .map(|e| e.0.clone())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "panic".to_owned());
            return Err(format!("mirages panicked: {msg}"));
        }
    };
    // `ret`: PoB's packed results, with our value in PoB's shape (`nil` and `false` are both a
    // falsy result; Rust returns the truthiness)
    let pob_ret = &dout.root["locals"]["ret"];
    let ret = Table::new();
    let n = pob_ret["n"].as_f64().unwrap_or(0.0);
    ret.set("n", n);
    let pob1 = pob_ret["$seq"].get(0).cloned().unwrap_or(J::Null);
    let ours1 = match (&pob1, ours) {
        (J::Bool(_), b) => Val::Bool(b),
        (J::Null, false) => Val::Nil,
        (_, b) => Val::Bool(b),
    };
    if n >= 1.0 {
        ret.set(1, ours1);
    }
    let locals = vec![
        (
            "env".to_owned(),
            Val::Obj(Rc::clone(&env) as Rc<dyn std::any::Any>),
        ),
        ("ret".to_owned(), Val::Table(ret)),
    ];
    let all = testkit::diff_against(&dout, Some(&env), &locals, "");
    let ours_root = testkit::export_after(Some(&env), &locals);
    let r = real(&all);
    let (content, final_identity) = split_identity(&r, &dout.root, &ours_root);
    let mismatches: Vec<Mismatch> = content.into_iter().cloned().collect();
    let mut log = Rc::try_unwrap(log)
        .map(RefCell::into_inner)
        .unwrap_or_else(|l| std::mem::take(&mut *l.borrow_mut()));
    // a bare `return` (n = 0: CalcMirages.lua 64, 354, `return calculateMirage(env, nil)`) is a
    // falsy result; the packed `ret` above has no slot to show a Rust `true` there
    if n < 1.0 && ours {
        log.problems
            .push("mirages returned true where PoB returned nothing (ret.n = 0)".into());
    }
    // General's Cry: Rust's cooldown before the `m_max` against PoB's (mi:376)
    match (gc_cd_pre, log.gc_cooldown.as_slice()) {
        (None, []) => {}
        (Some(pob), [ours])
            if pob_oracle::diff::num_close(pob, *ours, pob_oracle::diff::REL_TOL) => {}
        (pob, ours) => log.problems.push(format!(
            "General's Cry cooldown before m_max: PoB {pob:?} (mi:376), Rust {ours:?}"
        )),
    }
    Ok(CallResult {
        mismatches,
        final_identity,
        pob_changes,
        log,
        stubs,
        family,
    })
}

#[derive(PartialEq, Eq, Debug)]
enum Verdict {
    Clean,
    ReplayLimited(String),
    Divergent(String),
}

fn verdict(r: &CallResult, real: usize) -> Verdict {
    if !r.log.problems.is_empty() {
        return Verdict::Divergent(r.log.problems.join("; "));
    }
    // a stub reached on the MI path means the Rust run did not compute what PoB did
    if !r.stubs.is_empty() {
        return Verdict::Divergent(format!("stubs reached: {}", r.stubs.join(", ")));
    }
    if real == 0 && r.log.steps.iter().all(|(_, b, a)| *b == 0 && *a == 0) {
        return Verdict::Clean;
    }
    let mut prev = 0usize;
    let mut limited = Vec::new();
    for (call, before, after) in &r.log.steps {
        if *before > prev {
            return Verdict::Divergent(format!("{} new mismatches before {call}", before - prev));
        }
        if after > before {
            limited.push(format!("{call} +{}", after - before));
        }
        prev = *after;
    }
    if real > prev {
        let why = format!("{} new mismatches after the last call", real - prev);
        if limited.is_empty() {
            return Verdict::Divergent(why);
        }
        limited.push(format!("tail not comparable: {why}"));
    }
    if limited.is_empty() {
        Verdict::Divergent(format!("{real} mismatches"))
    } else {
        Verdict::ReplayLimited(limited.join(", "))
    }
}

/// L4 over the corpus (all of `.parity/builds` by default).
#[test]
fn mi_l4_mirages_in_to_out() {
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    let Some(parity) = find_parity_dir() else {
        return skip("no .parity directory");
    };
    if pob_oracle::oracle::find_pob_src().is_none() {
        return skip("no PoB checkout");
    }
    let builds = corpus(&parity);
    if builds.is_empty() {
        return skip("no .parity/builds/*.xml");
    }
    let mut clean = Vec::new();
    let mut limited = Vec::new();
    let mut dirty = Vec::new();
    let mut failed = Vec::new();
    let mut families: BTreeMap<String, usize> = BTreeMap::new();
    let mut controls = 0usize;
    let mut steps_total = 0usize;
    let mut changes_total = 0usize;
    let mut vacuous: Vec<String> = Vec::new();
    for xml in &builds {
        let stem = stem(xml);
        // held while the build's calls read their dumps
        let dumps = probe_dumps(xml);
        let calls = match dumps
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|s| mirage_calls(s))
        {
            Ok(c) => c,
            Err(e) => {
                failed.push(format!("{stem}: {e}"));
                continue;
            }
        };
        for c in &calls {
            let tag = format!("{stem} {}{}", c.hit_in, if c.main { "@main" } else { "" });
            match run_call(c) {
                Ok(r) => {
                    let real_ms = real(&r.mismatches);
                    for s in &r.stubs {
                        eprintln!("  {tag}: stub {s}");
                    }
                    for m in &r.log.first_mismatches {
                        eprintln!("  {tag}: {m}");
                    }
                    for m in real_ms.iter().take(if verbose() { usize::MAX } else { 12 }) {
                        eprintln!("  {tag}: final {}", fmt_mismatch(m));
                    }
                    if c.lines.is_empty() {
                        controls += 1;
                    } else {
                        *families.entry(r.family.clone()).or_default() += 1;
                    }
                    steps_total += r.log.steps.len();
                    changes_total += r.pob_changes;
                    if !c.lines.is_empty() && r.pob_changes == 0 {
                        vacuous.push(tag.clone());
                    }
                    let v = verdict(&r, real_ms.len());
                    eprintln!(
                        "MI {tag} [{}; PoB changed {} paths; replayed {:?}; identity-only mismatches: final {}, max per step {}]: {v:?}",
                        if c.lines.is_empty() {
                            "no family".to_owned()
                        } else {
                            r.family.clone()
                        },
                        r.pob_changes,
                        r.log.replayed,
                        r.final_identity,
                        r.log.identity_only
                    );
                    match v {
                        Verdict::Clean => clean.push(tag),
                        Verdict::ReplayLimited(w) => limited.push(format!("{tag}: {w}")),
                        Verdict::Divergent(w) => dirty.push(format!("{tag}: {w}")),
                    }
                }
                Err(e) => {
                    eprintln!("MI {tag}: FAILED ({e})");
                    failed.push(format!("{tag}: {e}"));
                }
            }
        }
    }
    eprintln!(
        "MI L4 mirages:in -> mirages:out: {} builds, {} calls ({} family calls by main skill {:?}, {} no-family main-env controls), {} replayed steps, {} state paths PoB changed in all; {} clean, {} replay-limited, {} divergent, {} failed",
        builds.len(),
        clean.len() + limited.len() + dirty.len() + failed.len(),
        families.values().sum::<usize>(),
        families,
        controls,
        steps_total,
        changes_total,
        clean.len(),
        limited.len(),
        dirty.len(),
        failed.len()
    );
    for v in &vacuous {
        eprintln!("  vacuous (PoB changed nothing): {v}");
    }
    for l in &limited {
        eprintln!("  replay-limited: {l}");
    }
    assert!(
        dirty.is_empty() && failed.is_empty(),
        "divergent: {dirty:#?}\nfailed: {failed:#?}"
    );
    assert!(
        limited.is_empty(),
        "replay-limited calls (not comparable): {limited:#?}"
    );
    assert!(
        vacuous.is_empty(),
        "family calls where PoB changed nothing: {vacuous:#?}"
    );
}
