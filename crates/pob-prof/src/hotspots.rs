//! Sampling-profile helpers: fold `perf script` output into flamegraph stacks, and summarise
//! folded stacks into hotspot lists. Owner: PROF1.
//!
//! `collapse` reads `perf script` text (one header line per sample, then one indented line per
//! frame, leaf first, a blank line after each sample) and writes the folded format of Brendan
//! Gregg's `stackcollapse-perf.pl` / inferno (`root;…;leaf count`), so `inferno-flamegraph` (or
//! `flamegraph.pl`) draws it. Rust symbol hashes (`::h0123456789abcdef`) and `+0x…` offsets are
//! removed; an unresolved frame is `[<dso basename>]`.
//!
//! `report` prints, for one folded profile: the top functions by self time (the leaf frame of
//! each sample), self time by crate, and the inclusive share of the calc stages that cannot be
//! timed from outside the calc (`offence`, `defence`, …; see crate::stages).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::BufRead;

/// Strip the Rust legacy-mangling hash and a `+0x…` offset.
pub fn clean_symbol(s: &str) -> String {
    let s = s.split("+0x").next().unwrap_or(s);
    let s = match s.rfind("::h") {
        Some(i) if s.len() - i == 19 && s[i + 3..].chars().all(|c| c.is_ascii_hexdigit()) => {
            &s[..i]
        }
        _ => s,
    };
    s.trim().to_owned()
}

fn frame_name(line: &str) -> Option<String> {
    // "    55d0c0ffee symbol+0x12 (/path/to/dso)"
    let t = line.trim();
    if t.is_empty() {
        return None;
    }
    let (rest, dso) = match t.rfind(" (") {
        Some(i) if t.ends_with(')') => (&t[..i], &t[i + 2..t.len() - 1]),
        _ => (t, ""),
    };
    let sym = rest
        .split_once(char::is_whitespace)
        .map_or("", |(_, s)| s)
        .trim();
    if sym.is_empty() || sym == "[unknown]" {
        let base = dso.rsplit('/').next().unwrap_or(dso);
        return Some(format!(
            "[{}]",
            if base.is_empty() { "unknown" } else { base }
        ));
    }
    Some(clean_symbol(sym))
}

/// Fold `perf script` output (module docs). Returns `stack -> samples`.
pub fn collapse(input: impl BufRead) -> BTreeMap<String, u64> {
    let mut out: BTreeMap<String, u64> = BTreeMap::new();
    let mut frames: Vec<String> = Vec::new();
    let mut comm: Option<String> = None;
    let flush =
        |frames: &mut Vec<String>, comm: &mut Option<String>, out: &mut BTreeMap<String, u64>| {
            if let Some(c) = comm.take() {
                let mut stack = vec![c];
                stack.extend(frames.drain(..).rev());
                *out.entry(stack.join(";")).or_default() += 1;
            }
            frames.clear();
        };
    for line in input.lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            flush(&mut frames, &mut comm, &mut out);
            continue;
        }
        if line.starts_with(|c: char| c.is_whitespace()) {
            if comm.is_some() {
                if let Some(f) = frame_name(&line) {
                    frames.push(f);
                }
            }
        } else {
            flush(&mut frames, &mut comm, &mut out);
            // the command name (may contain spaces; the pid follows it)
            let c = line.split_whitespace().next().unwrap_or("?").to_owned();
            comm = Some(c);
        }
    }
    flush(&mut frames, &mut comm, &mut out);
    out
}

/// Parse folded text (`a;b;c 12` per line).
pub fn parse_folded(text: &str) -> BTreeMap<String, u64> {
    let mut m = BTreeMap::new();
    for l in text.lines() {
        if let Some((s, n)) = l.rsplit_once(' ') {
            if let Ok(n) = n.trim().parse::<u64>() {
                *m.entry(s.to_owned()).or_default() += n;
            }
        }
    }
    m
}

/// The calc stages and app areas whose inclusive share `report` prints: (label, patterns). A
/// sample counts for an area when one of its frames contains one of the patterns. The small
/// entry functions are often inlined (`calcs.defence` is two `seg!` calls), so the patterns name
/// the module paths of their bodies.
pub const STAGE_FRAMES: &[(&str, &[&str])] = &[
    (
        "recalc (calcsTab:BuildOutput)",
        &["modules::power::BuildOutput"],
    ),
    ("calcs.initEnv (calc_setup)", &["modules::calc_setup::"]),
    ("calcs.perform (calc_perform)", &["modules::calc_perform::"]),
    ("calcs.offence (calc_offence)", &["modules::calc_offence::"]),
    (
        "calcs.defence (primary, recovery)",
        &[
            "calc_defence::defence",
            "calc_defence::primary::",
            "calc_defence::recovery::",
        ],
    ),
    (
        "calcs.buildDefenceEstimations (est_*)",
        &[
            "calc_defence::buildDefenceEstimations",
            "calc_defence::est_",
        ],
    ),
    ("calcs.calcFullDPS", &["modules::calcs::calcFullDPS"]),
    (
        "calc triggers/mirages",
        &["modules::calc_triggers::", "modules::calc_mirages::"],
    ),
    (
        "misc calculator call",
        &["modules::calcs::misc_calculator_call"],
    ),
    (
        "power report (PowerBuilder)",
        &["modules::power::PowerBuilder", "modules::power::PowerIter"],
    ),
    (
        "build load (Build:Init, LoadDB)",
        &["build_xml::loadBuildFromXML", "pob_app::build::init::init"],
    ),
    ("tree view draw", &["pob_app::tree_view::"]),
    ("tooltips", &["tooltip", "Tooltip"]),
    (
        "frame: end_frame (sort, digest)",
        &["pob_sg::api::Sg>::end_frame", "pob_sg::frame::digest"],
    ),
    (
        "image decode",
        &[
            "pob_sg::image::decode_image",
            "png::",
            "image_webp::",
            "zune_jpeg::",
        ],
    ),
    (
        "allocator (malloc/free)",
        &["malloc", "cfree", "realloc", "_int_free", "_int_malloc"],
    ),
    ("render: wgpu", &["pob_sg_wgpu::"]),
    ("render: CPU raster", &["pob_sg::raster::"]),
];

fn crate_of(frame: &str) -> String {
    let f = frame.trim_start_matches('<');
    if f.starts_with('[') {
        return f.to_owned();
    }
    match f.split_once("::") {
        Some((c, _)) => c.split(' ').next().unwrap_or(c).to_owned(),
        None => {
            if [
                "malloc", "free", "realloc", "calloc", "_int_", "__libc", "cfree", "memcpy",
                "memmove", "memset",
            ]
            .iter()
            .any(|p| f.contains(p))
            {
                "libc (alloc/mem*)".to_owned()
            } else {
                format!("(c) {f}")
            }
        }
    }
}

/// The hotspot summary of a folded profile.
pub struct Report {
    pub total: u64,
    pub top_self: Vec<(String, u64)>,
    pub by_crate: Vec<(String, u64)>,
    pub stages: Vec<(String, u64)>,
}

/// Summarise `folded` (only samples whose stack contains `filter`, when given).
pub fn analyse(folded: &BTreeMap<String, u64>, filter: Option<&str>) -> Report {
    let mut selfs: BTreeMap<String, u64> = BTreeMap::new();
    let mut crates: BTreeMap<String, u64> = BTreeMap::new();
    let mut stages = vec![0u64; STAGE_FRAMES.len()];
    let mut total = 0;
    for (stack, &n) in folded {
        if let Some(f) = filter {
            if !stack.contains(f) {
                continue;
            }
        }
        total += n;
        let frames: Vec<&str> = stack.split(';').collect();
        let leaf = frames.last().copied().unwrap_or("?");
        *selfs.entry(leaf.to_owned()).or_default() += n;
        *crates.entry(crate_of(leaf)).or_default() += n;
        let set: BTreeSet<&str> = frames.iter().copied().collect();
        for (i, (_, pats)) in STAGE_FRAMES.iter().enumerate() {
            if set.iter().any(|f| pats.iter().any(|p| f.contains(p))) {
                stages[i] += n;
            }
        }
    }
    let mut top_self: Vec<(String, u64)> = selfs.into_iter().collect();
    top_self.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut by_crate: Vec<(String, u64)> = crates.into_iter().collect();
    by_crate.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Report {
        total,
        top_self,
        by_crate,
        stages: STAGE_FRAMES
            .iter()
            .zip(stages)
            .map(|((l, _), n)| ((*l).to_owned(), n))
            .collect(),
    }
}

fn pct(n: u64, t: u64) -> f64 {
    if t == 0 {
        0.0
    } else {
        n as f64 * 100.0 / t as f64
    }
}

/// The text report (`top` entries per list).
pub fn render(r: &Report, top: usize) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "{} samples", r.total);
    let _ = writeln!(o, "\ntop {top} functions by self time:");
    for (f, n) in r.top_self.iter().take(top) {
        let _ = writeln!(o, "  {:5.1}%  {f}", pct(*n, r.total));
    }
    let _ = writeln!(o, "\nself time by crate:");
    for (c, n) in r.by_crate.iter().take(top.max(8)) {
        let _ = writeln!(o, "  {:5.1}%  {c}", pct(*n, r.total));
    }
    let _ = writeln!(o, "\ninclusive share of calc stages and app areas:");
    for (l, n) in &r.stages {
        if *n > 0 {
            let _ = writeln!(o, "  {:5.1}%  {l}", pct(*n, r.total));
        }
    }
    o
}

/// Folded text, one stack per line, sorted.
pub fn to_folded(m: &BTreeMap<String, u64>) -> String {
    let mut s = String::new();
    for (k, v) in m {
        let _ = writeln!(s, "{k} {v}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERF: &str = "pob-prof 1234 [003] 100.000001:     250000 cycles:u: \n\
\t    aaaa1111 pob_calc_offence::modules::calc_offence::offence::h0123456789abcdef+0x1c (/x/pob-prof)\n\
\t    aaaa2222 pob_calc_perform::modules::calc_perform::perform::hfedcba9876543210+0x20 (/x/pob-prof)\n\
\t    aaaa3333 main+0x10 (/x/pob-prof)\n\
\n\
pob-prof 1234 [003] 100.000002:     250000 cycles:u: \n\
\t    bbbb1111 malloc+0x4 (/usr/lib/libc.so.6)\n\
\t    aaaa2222 pob_calc_perform::modules::calc_perform::perform::hfedcba9876543210+0x20 (/x/pob-prof)\n\
\t    aaaa3333 main+0x10 (/x/pob-prof)\n\
\n\
pob-prof 1234 [003] 100.000003:     250000 cycles:u: \n\
\t    cccc1111 [unknown] (/usr/lib/libvulkan.so.1)\n\
\t    aaaa3333 main+0x10 (/x/pob-prof)\n";

    #[test]
    fn collapse_and_report() {
        let f = collapse(PERF.as_bytes());
        assert_eq!(
            to_folded(&f),
            "pob-prof;main;[libvulkan.so.1] 1\n\
pob-prof;main;pob_calc_perform::modules::calc_perform::perform;malloc 1\n\
pob-prof;main;pob_calc_perform::modules::calc_perform::perform;pob_calc_offence::modules::calc_offence::offence 1\n"
        );
        assert_eq!(parse_folded(&to_folded(&f)), f);
        let r = analyse(&f, None);
        assert_eq!(r.total, 3);
        let st: BTreeMap<_, _> = r.stages.iter().cloned().collect();
        assert_eq!(st["calcs.perform (calc_perform)"], 2);
        assert_eq!(st["calcs.offence (calc_offence)"], 1);
        assert_eq!(st["calcs.defence (primary, recovery)"], 0);
        assert_eq!(st["allocator (malloc/free)"], 1);
        let cr: BTreeMap<_, _> = r.by_crate.iter().cloned().collect();
        assert_eq!(cr["libc (alloc/mem*)"], 1);
        assert_eq!(cr["pob_calc_offence"], 1);
        assert_eq!(analyse(&f, Some("calc_perform::perform")).total, 2);
        assert!(render(&r, 5).contains("calcs.offence (calc_offence)"));
    }

    #[test]
    fn symbols() {
        assert_eq!(clean_symbol("a::b::hdeadbeefdeadbeef+0x10"), "a::b");
        assert_eq!(clean_symbol("a::hash_map+0x10"), "a::hash_map");
        assert_eq!(
            clean_symbol("<T as X>::f::h00000000000000ff"),
            "<T as X>::f"
        );
    }
}
