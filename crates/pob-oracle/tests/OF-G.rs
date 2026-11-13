//! OF-G acceptance (calc doc §11 row OF-G, §9.4 L4): CalcOffence.lua 4155-4989.
//!
//! * `offence:4155#n` → `offence:4164#n`: the ailment prelude (`seg_ailmentPrelude`).
//! * `offence:4165#n` → `offence:4990#n`: ailment loop body A (`seg_bleedPoison`), per pass. Both
//!   probes fire exactly once per pass, so equal hit numbers are the same pass.
//!
//! **Dumps.** An `in`/`out` pair must come from one PoB process: PoB's `pairs` order (mod lists of
//! buffs, …) differs between processes, and the shared probe cache mixes dumps of different runs
//! (other units refill `offence:4155` for their own `out`). So this test probes every build itself
//! (`pob_oracle probe`, which takes the machine-wide oracle lock) into its own entry of the
//! content-keyed selection cache (`testkit::probe_selection`: one run per entry, keyed by the
//! patched source, the build and the selection; CK1, CCR-0147), reused while the key holds
//! (`POB_QUERY_CACHE=refresh` probes again). The selection is the main env's first hit of each probe plus `#1..#6` of the two
//! loop-body probes (off-hand passes and other skills' passes).
//!
//! The segment calls OF-A's `calcAilmentSourceDamage` (CalcOffence.lua:143-154) and
//! `calcResistForType` (494-508). Until OF-A lands they are `unported!` stubs; a pair whose run
//! hits one of them is reported as BLOCKED (not compared). A pair whose `skillModList` parent is
//! a store of another env (`$foreign`, e.g. an offence run inside `calcs.mirages`) is reported as
//! FOREIGN: the dump lacks that store's mods. Every other pair must match exactly.

use std::path::Path;

use pob_oracle::schema::StateDump;
use pob_oracle::testkit;

/// Curated set: spells and attacks, poison / bleed / neither, main hand and dual wield.
const BUILDS: &[&str] = &[
    "allflame_exp_000",
    "allflame_exp_001",
    "allflame_exp_005",
    "allflame_exp_006",
    "allflame_exp_007",
    "allflame_exp_013",
    "allflame_exp_017",
    "allflame_exp_029",
    "allflame_exp_042",
    "allflame_exp_045",
    "allflame_exp_046",
    "allflame_exp_050",
    "upstream_Dual_Wield_Cospris_CoC",
    "upstream_Generals_Perforate_Zerker",
    "upstream_Mirage_Archer_Toxic_Rain",
];

/// `unported!` sites of other units this segment calls.
const DEP_STUBS: &[&str] = &[
    "Modules/CalcOffence.lua:143-154",
    "Modules/CalcOffence.lua:494-508",
    "Modules/CalcOffence.lua:70-141",
];

fn dep_hits() -> u64 {
    pob_calc::stub::stub_report()
        .iter()
        .filter(|(s, _)| DEP_STUBS.contains(s))
        .map(|(_, n)| *n)
        .sum()
}

/// The build's dumps: the shared probe cache's entry of the selection on the copy patched with
/// `docs/port/stages.toml`, probed in a fresh `pob_oracle probe` process when absent
/// (`testkit::probe_selection`, `runner::SelectionCache`). CK1 (CCR-0146 §4, CCR-0147): the
/// entry is content-keyed (the patched source's sha, which covers the stages file, the patcher
/// and the dump/probe Lua; the build's BLAKE3; the selection and dump cap; the probe's Rust
/// code), so a harness, dump-format or PoB change probes again. The earlier
/// `$CARGO_TARGET_TMPDIR/OF-G/<stem>/` was reused whenever it held a `complete` marker, whatever
/// made it. Hold the result while reading the dumps.
fn ensure_dumps(xml: &Path) -> Result<pob_oracle::runner::ProbeDumps, String> {
    let mut sel: Vec<String> = ["4155", "4164", "4165", "4990"]
        .iter()
        .map(|l| format!("offence:{l}@main"))
        .collect();
    for k in 1..=6 {
        sel.push(format!("offence:4165#{k}"));
        sel.push(format!("offence:4990#{k}"));
    }
    let opts = pob_oracle::runner::ProbeOptions {
        select: sel,
        // the `pob_oracle probe` default the earlier private run used
        max_dumps: 200,
        provenance: false,
    };
    let stages = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/port/stages.toml");
    pob_oracle::testkit::probe_selection(
        Path::new(env!("CARGO_BIN_EXE_pob_oracle")),
        xml,
        Some(&stages),
        &opts,
    )
}

/// Every `<probe>#n` dump present in `dir` (hit numbers).
fn hits_of(dir: &Path, probe: &str) -> Vec<u64> {
    let prefix = format!("{probe}#");
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.filter_map(Result::ok) {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(rest) = name.strip_prefix(&prefix) {
                let n: String = rest.chars().take_while(char::is_ascii_digit).collect();
                if let Ok(n) = n.parse::<u64>() {
                    if !out.contains(&n) {
                        out.push(n);
                    }
                }
            }
        }
    }
    out.sort_unstable();
    out
}

/// `testkit::segment_diff` on explicit dump files (both from this test's own probe run).
fn segment_diff(build: &Path, stem: &str, pin: &Path, pout: &Path) -> Vec<String> {
    let (Ok(din), Ok(dout)) = (StateDump::load(pin), StateDump::load(pout)) else {
        return vec![format!(
            "unreadable dump {} / {}",
            pin.display(),
            pout.display()
        )];
    };
    testkit::segment_diff_dumps(build, stem, &din, &dout)
}

/// Whether `locals.skillModList`'s parent chain reaches a store of another env
/// (`{"$foreign": …}`). The dump does not carry that store's mods (an env built inside
/// `calcs.mirages`, whose active skill belongs to the outer env), so injection cannot rebuild
/// what `skillModList:Sum` sees in PoB and the pair is not comparable.
fn foreign_skill_parent(d: &StateDump) -> bool {
    fn index<'a>(
        v: &'a serde_json::Value,
        ids: &mut std::collections::HashMap<String, &'a serde_json::Value>,
    ) {
        match v {
            serde_json::Value::Object(m) => {
                if let Some(id) = m.get("$id") {
                    ids.insert(id.to_string().trim_matches('"').to_owned(), v);
                }
                m.values().for_each(|x| index(x, ids));
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| index(x, ids)),
            _ => {}
        }
    }
    let mut ids = std::collections::HashMap::new();
    index(&d.root, &mut ids);
    let mut cur = d.root.get("locals").and_then(|l| l.get("skillModList"));
    for _ in 0..16 {
        let Some(mut v) = cur else { return false };
        if let Some(r) = v.get("$ref") {
            match ids.get(r.to_string().trim_matches('"')) {
                Some(t) => v = t,
                None => return false,
            }
        }
        if v.get("$foreign").is_some() {
            return true;
        }
        cur = v.get("parent");
    }
    false
}

#[derive(Default)]
struct Tally {
    pass: usize,
    fail: usize,
    blocked: usize,
    foreign: usize,
    lines: Vec<String>,
}

fn check(
    t: &mut Tally,
    xml: &Path,
    stem: &str,
    state: &Path,
    probe_in: &str,
    probe_out: &str,
    n: u64,
) {
    let din = format!("{probe_in}#{n}");
    let dout = format!("{probe_out}#{n}");
    let dirs = [state.to_path_buf()];
    let (Some(pin), Some(pout)) = (
        testkit::find_dump(&dirs, &din),
        testkit::find_dump(&dirs, &dout),
    ) else {
        return;
    };
    if StateDump::load(&pin).is_ok_and(|d| foreign_skill_parent(&d)) {
        t.foreign += 1;
        t.lines.push(format!(
            "FOREIGN {stem} {din}->{dout}: skillModList's parent is another env's store (not in the dump)"
        ));
        return;
    }
    let before = dep_hits();
    let mm = segment_diff(xml, stem, &pin, &pout);
    let blocked = dep_hits() > before;
    if blocked {
        t.blocked += 1;
        t.lines.push(format!(
            "BLOCKED {stem} {din}->{dout}: calls an unported OF-A helper ({} mismatches)",
            mm.len()
        ));
        // `OFG_VERBOSE=1`: list what differs downstream of the stubbed helpers
        if std::env::var_os("OFG_VERBOSE").is_some() {
            t.lines.extend(mm.iter().map(|m| format!("          {m}")));
        }
    } else if mm.is_empty() {
        t.pass += 1;
        t.lines.push(format!("ok      {stem} {din}->{dout}"));
    } else {
        t.fail += 1;
        t.lines.push(format!(
            "FAIL    {stem} {din}->{dout}: {} mismatches",
            mm.len()
        ));
        for m in mm.iter().take(20) {
            t.lines.push(format!("          {m}"));
        }
    }
}

#[test]
fn ofg_l4_prelude_and_bleed_poison() {
    if pob_testpaths::require(pob_testpaths::dump_dir(), "data/dump").is_none() {
        return;
    }
    if !testkit::init_default_data() {
        panic!("data/dump does not load");
    }
    let mut t = Tally::default();
    let mut missing = Vec::new();
    for stem in BUILDS {
        let Some(xml) = pob_testpaths::parity_build(stem) else {
            // CK1: the quick corpus (`POB_QUICK_CORPUS=1`) is a deliberate subset of all57, so a
            // build of BUILDS outside it is filtered out, not a missing input
            if pob_testpaths::quick_corpus() {
                t.lines
                    .push(format!("note    {stem}: not in the quick corpus"));
            } else {
                missing.push(format!("{stem}: build xml"));
            }
            continue;
        };
        let state = match ensure_dumps(&xml) {
            Ok(s) => s,
            Err(e) => {
                missing.push(format!("{stem}: {e}"));
                continue;
            }
        };
        let pre = hits_of(&state, "offence:4155");
        let body = hits_of(&state, "offence:4165");
        if pre.is_empty() && body.is_empty() {
            t.lines.push(format!(
                "note    {stem}: the main env runs no offence ailment section"
            ));
        }
        for n in pre {
            check(
                &mut t,
                &xml,
                stem,
                &state,
                "offence:4155",
                "offence:4164",
                n,
            );
        }
        for n in body {
            check(
                &mut t,
                &xml,
                stem,
                &state,
                "offence:4165",
                "offence:4990",
                n,
            );
        }
    }
    for l in &t.lines {
        eprintln!("{l}");
    }
    eprintln!(
        "OF-G L4: {} ok, {} failed, {} blocked on OF-A, {} not comparable (foreign store); {} builds without inputs",
        t.pass,
        t.fail,
        t.blocked,
        t.foreign,
        missing.len()
    );
    for m in &missing {
        eprintln!("missing: {m}");
    }
    if !missing.is_empty() && pob_testpaths::require::<()>(None, &missing.join("; ")).is_none() {
        return;
    }
    assert_eq!(t.fail, 0, "OF-G L4 mismatches (see stderr)");
    assert!(t.pass > 0, "no OF-G L4 pair compared");
}
