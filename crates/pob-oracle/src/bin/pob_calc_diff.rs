//! Compare PoB's numbers (the oracle) with the Rust calc's.
//!
//! ```text
//! pob_calc_diff outputs POB OURS [--json-out DIR] [--nondet DIR] [--unit ID]
//!                                [--provenance DIR] [--doc FILE] [--rel X] [--quiet] [--fail]
//! pob_calc_diff state   POB.json[.zst] OURS.json[.zst] [--nondet FILE] [--json-out FILE] [--rel X]
//! ```
//!
//! `outputs`: POB / OURS are output files or directories of them
//! (`<dir>/<stem>/outputs.json` or `<dir>/<stem>.json`, paired by stem, as
//! scripts/parity/diff.py). A file is a `pob_oracle outputs` dump or any
//! JSON object of outputs. Numbers match within 1e-9·max(1,|pob|)
//! (calc-architecture.md §9.2); values at nondeterministic keys
//! (`<nondet>/<stem>.json`, default `.parity/nondet`) match any value PoB
//! produced. `--json-out DIR` writes `DIR/<stem>.json` =
//! `{build, pob, ours, mismatches, summary}`, which
//! `scripts/parity/report.py DIR` turns into the parity scoreboard.
//! `--unit ID` keeps only mismatches whose last PoB writer (write
//! provenance, `<provenance>/<stem>/provenance.json` or
//! `<provenance>/<stem>.json`, default `.parity/oracle`) lies in the unit's
//! Lua ranges (unit table of docs/port/calc-architecture.md).
//!
//! `state`: two pob-state/1 dumps of the same hit; mod stores compare as
//! multisets with LUA-ORDER reported separately.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pob_oracle::diff::{
    calc_diff_doc, diff_flat, diff_trees, load_flat_outputs, unit_ranges, DiffOptions, LuaRange,
    MismatchKind, REL_TOL,
};
use pob_oracle::nondet::NondetRecord;
use pob_oracle::oracle::{find_parity_dir, find_up};
use pob_oracle::schema::{Provenance, StateDump};

const USAGE: &str = "usage:
  pob_calc_diff outputs POB OURS [--json-out DIR] [--nondet DIR] [--unit ID] [--provenance DIR]
                                 [--doc FILE] [--rel X] [--quiet] [--fail]
  pob_calc_diff state   POB.json[.zst] OURS.json[.zst] [--nondet FILE] [--json-out FILE] [--rel X]";

/// stem -> file, as scripts/parity/diff.py `collect`.
fn collect(path: &Path) -> BTreeMap<String, PathBuf> {
    let mut found = BTreeMap::new();
    if path.is_file() {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let stem = if name == "outputs.json" {
            path.parent()
                .and_then(Path::file_name)
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            name.trim_end_matches(".json").to_owned()
        };
        found.insert(stem, path.to_path_buf());
        return found;
    }
    let Ok(rd) = std::fs::read_dir(path) else {
        return found;
    };
    let mut entries: Vec<_> = rd.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let p = e.path();
        if p.is_dir() && p.join("outputs.json").is_file() {
            found.insert(name, p.join("outputs.json"));
        } else if p.is_file() && name.ends_with(".json") && name != "outputs.json" {
            found.insert(name.trim_end_matches(".json").to_owned(), p);
        }
    }
    found
}

#[derive(Default)]
struct Opts {
    pos: Vec<String>,
    json_out: Option<PathBuf>,
    nondet: Option<PathBuf>,
    unit: Option<String>,
    provenance: Option<PathBuf>,
    doc: Option<PathBuf>,
    rel: f64,
    quiet: bool,
    fail: bool,
}

fn parse(args: impl Iterator<Item = String>) -> Result<Opts, String> {
    let mut o = Opts {
        rel: REL_TOL,
        ..Opts::default()
    };
    let mut it = args.peekable();
    while let Some(a) = it.next() {
        let mut val = || {
            it.next()
                .ok_or_else(|| format!("{a} needs a value\n{USAGE}"))
        };
        match a.as_str() {
            "--json-out" => o.json_out = Some(val()?.into()),
            "--nondet" => o.nondet = Some(val()?.into()),
            "--unit" => o.unit = Some(val()?),
            "--provenance" => o.provenance = Some(val()?.into()),
            "--doc" => o.doc = Some(val()?.into()),
            "--rel" => o.rel = val()?.parse().map_err(|e| format!("--rel: {e}"))?,
            "--quiet" => o.quiet = true,
            "--fail" => o.fail = true,
            "-h" | "--help" => return Err(USAGE.into()),
            _ if !a.starts_with("--") => o.pos.push(a),
            _ => return Err(format!("unexpected argument {a}\n{USAGE}")),
        }
    }
    Ok(o)
}

fn fmt_val(v: Option<&serde_json::Value>) -> String {
    let s = v.map_or_else(|| "-".to_owned(), ToString::to_string);
    if s.len() > 40 {
        format!("{}...", &s[..37])
    } else {
        s
    }
}

fn outputs(o: &Opts) -> Result<bool, String> {
    let [pob_arg, ours_arg] = &o.pos[..] else {
        return Err(USAGE.into());
    };
    let (pob, ours) = (collect(Path::new(pob_arg)), collect(Path::new(ours_arg)));
    let pairs: Vec<(String, PathBuf, PathBuf)> = if pob.len() == 1 && ours.len() == 1 {
        let (s, p) = pob
            .iter()
            .next()
            .map(|(s, p)| (s.clone(), p.clone()))
            .unwrap_or_default();
        vec![(s, p, ours.values().next().cloned().unwrap_or_default())]
    } else {
        pob.iter()
            .filter_map(|(s, p)| ours.get(s).map(|q| (s.clone(), p.clone(), q.clone())))
            .collect()
    };
    if pairs.is_empty() {
        return Err(format!(
            "no builds in common between {pob_arg} ({}) and {ours_arg} ({})",
            pob.len(),
            ours.len()
        ));
    }
    let parity = find_parity_dir();
    let nondet_dir = o
        .nondet
        .clone()
        .or_else(|| parity.as_ref().map(|p| p.join("nondet")));
    let ranges: Option<Vec<LuaRange>> = match &o.unit {
        Some(u) => {
            let doc = o
                .doc
                .clone()
                .or_else(|| find_up("docs/port/calc-architecture.md"))
                .ok_or("--unit: docs/port/calc-architecture.md not found (pass --doc)")?;
            let md =
                std::fs::read_to_string(&doc).map_err(|e| format!("{}: {e}", doc.display()))?;
            Some(unit_ranges(&md, u).map_err(|e| e.0)?)
        }
        None => None,
    };
    let prov_dir = o
        .provenance
        .clone()
        .or_else(|| parity.as_ref().map(|p| p.join("oracle")));
    if let Some(d) = &o.json_out {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let (mut shared, mut matching, mut missing, mut filtered, mut no_writer) = (0, 0, 0, 0, 0);
    let mut worst: BTreeMap<String, usize> = BTreeMap::new();
    for (stem, pf, of) in &pairs {
        let pob = load_flat_outputs(pf).map_err(|e| e.0)?;
        let ours = load_flat_outputs(of).map_err(|e| e.0)?;
        let nondet = nondet_dir
            .as_ref()
            .map(|d| d.join(format!("{stem}.json")))
            .filter(|p| p.is_file())
            .map(|p| NondetRecord::load(&p))
            .transpose()
            .map_err(|e| e.0)?;
        let prov = prov_dir.as_ref().and_then(|d| {
            [
                d.join(stem).join("provenance.json"),
                d.join(format!("{stem}.json")),
            ]
            .into_iter()
            .find(|p| p.is_file())
            .and_then(|p| Provenance::load(&p).ok())
        });
        if ranges.is_some() && prov.is_none() {
            no_writer += 1;
        }
        let opts = DiffOptions {
            rel: o.rel,
            nondet_values: nondet.as_ref().map(|n| &n.outputs),
            nondet_orders: nondet.as_ref().map(|n| &n.output_orders),
        };
        let d = diff_flat(&pob, &ours, &opts, prov.as_ref(), ranges.as_deref());
        shared += d.shared;
        matching += d.matching;
        missing += d.missing;
        filtered += d.filtered;
        for m in &d.mismatches {
            *worst.entry(m.path.clone()).or_default() += 1;
        }
        if !o.quiet {
            println!(
                "{stem:<36} {:>6}/{:<6} match, {:>4} mismatches, {:>4} missing{}",
                d.matching,
                d.shared,
                d.mismatches.len(),
                d.missing,
                if ranges.is_some() {
                    format!(", {} outside the unit", d.filtered)
                } else {
                    String::new()
                }
            );
            for m in d.mismatches.iter().take(5) {
                println!(
                    "    {:<44} pob={:<20} ours={:<20} {}",
                    m.path,
                    fmt_val(m.pob.as_ref()),
                    fmt_val(m.ours.as_ref()),
                    m.writer.as_deref().unwrap_or("")
                );
            }
        }
        if let Some(dir) = &o.json_out {
            let doc = calc_diff_doc(stem, &pob, &ours, &d, o.unit.as_deref());
            let p = dir.join(format!("{stem}.json"));
            std::fs::write(&p, serde_json::to_string(&doc).map_err(|e| e.to_string())?)
                .map_err(|e| format!("{}: {e}", p.display()))?;
        }
    }
    let mism = shared - matching - filtered;
    println!(
        "== {} builds: {matching}/{shared} shared keys match ({:.3}%), {mism} mismatches, {missing} PoB keys missing{}",
        pairs.len(),
        100.0 * matching as f64 / shared.max(1) as f64,
        if ranges.is_some() { format!(", {filtered} mismatches outside unit {}", o.unit.as_deref().unwrap_or("")) } else { String::new() }
    );
    if no_writer > 0 {
        println!(
            "   ({no_writer} builds without provenance: their mismatches cannot be attributed)"
        );
    }
    let mut top: Vec<_> = worst.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    for (k, n) in top.iter().take(20) {
        println!("  {k:<44} {n:>3}");
    }
    Ok(mism == 0 && missing == 0)
}

fn state(o: &Opts) -> Result<bool, String> {
    let [pob_arg, ours_arg] = &o.pos[..] else {
        return Err(USAGE.into());
    };
    let pob = StateDump::load(Path::new(pob_arg)).map_err(|e| e.0)?;
    let ours = StateDump::load(Path::new(ours_arg)).map_err(|e| e.0)?;
    let nondet = o
        .nondet
        .as_ref()
        .map(|p| NondetRecord::load(p))
        .transpose()
        .map_err(|e| e.0)?;
    let values = nondet.as_ref().and_then(|n| n.states.get(&pob.hit));
    let orders = nondet.as_ref().and_then(|n| n.state_orders.get(&pob.hit));
    let opts = DiffOptions {
        rel: o.rel,
        nondet_values: values,
        nondet_orders: orders,
    };
    let d = diff_trees(&pob.resolved(), &ours.resolved(), &opts);
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    for m in &d {
        let kind = serde_json::to_value(m.kind)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        *by_kind.entry(kind).or_default() += 1;
    }
    for m in d.iter().take(40) {
        let tag = if m.kind == MismatchKind::LuaOrder {
            "LUA-ORDER"
        } else {
            ""
        };
        println!(
            "{:<60} {tag} pob={} ours={} {}",
            m.path,
            fmt_val(m.pob.as_ref()),
            fmt_val(m.ours.as_ref()),
            m.writer.as_deref().unwrap_or("")
        );
    }
    println!(
        "== {} {}: {} mismatches {by_kind:?}",
        pob.build,
        pob.hit,
        d.len()
    );
    if let Some(p) = &o.json_out {
        let doc = serde_json::json!({"build": pob.build, "hit": pob.hit, "mismatches": d});
        std::fs::write(p, serde_json::to_string(&doc).map_err(|e| e.to_string())?)
            .map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(d.is_empty())
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(cmd) = args.next() else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    let r = parse(args).and_then(|o| {
        let ok = match cmd.as_str() {
            "outputs" => outputs(&o)?,
            "state" => state(&o)?,
            _ => return Err(format!("unknown command {cmd}\n{USAGE}")),
        };
        Ok(ok || !o.fail)
    });
    match r {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}
