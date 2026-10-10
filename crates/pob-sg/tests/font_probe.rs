//! T6 font metrics against the REAL SimpleGraphic.dll (ui doc §11.1; U2 "JSON comparison runs once
//! Wine output exists").
//!
//! `crates/pob-ui-oracle/probe/run.sh` runs `probe/Launch.lua` under Wine and writes
//! `probe/expected/metrics.json` (schema `probe/metrics.schema.json`). When that file exists, every
//! `DrawStringWidth` and `DrawStringCursorIndex` value in it must equal ours exactly. While it is
//! absent (Wine is not installed, AMENDMENTS A5) the test reports PENDING and passes; the
//! comparator itself is always exercised on synthetic metrics.

#![allow(clippy::many_single_char_names)]

#[path = "font_util.rs"]
mod font_util;

use font_util::{font, fonts};
use pob_sg::FontSet;
use serde_json::{json, Value};

const EXPECTED: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../pob-ui-oracle/probe/expected/metrics.json"
);

/// Compare one metrics document with our engine; returns (values compared, mismatches).
fn compare(fs: &FontSet, m: &Value) -> (usize, Vec<String>) {
    assert_eq!(m["schema"], "pob-ui-oracle/probe/metrics.schema.json#1");
    let scale = m["screenScale"].as_f64().unwrap();
    assert!(
        scale == 1.0,
        "probe ran at screen scale {scale}: metrics are not physical px"
    );
    let strings: Vec<&str> = m["strings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect();
    let points: Vec<[i64; 3]> = m["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let p = p.as_array().unwrap();
            [
                p[0].as_i64().unwrap(),
                p[1].as_i64().unwrap(),
                p[2].as_i64().unwrap(),
            ]
        })
        .collect();
    let mut n = 0;
    let mut bad = Vec::new();
    for (fi, fname) in m["fonts"].as_array().unwrap().iter().enumerate() {
        let face = fs.face(font(fname.as_str().unwrap()));
        for (hi, h) in m["heights"].as_array().unwrap().iter().enumerate() {
            let h = h.as_i64().unwrap() as i32;
            for (si, text) in strings.iter().enumerate() {
                // Lua heights at scale 1 are already even (scaled_text_height is the identity).
                let want_w = m["width"][fi][hi][si].as_f64().unwrap();
                let got_w = f64::from(face.string_width(h, text));
                n += 1;
                if got_w != want_w {
                    bad.push(format!(
                        "{fname} {h} {text:?}: width {got_w} != SG {want_w}"
                    ));
                }
                for (pi, p) in points.iter().enumerate() {
                    let (x, y) = (p[0] as i32, p[1] as i32 * h + p[2] as i32);
                    let want = m["cursor"][fi][hi][si][pi].as_i64().unwrap();
                    let got = face.cursor_index(h, text, x, y) as i64 + 1; // ui_api.cpp `+ 1`
                    n += 1;
                    if got != want {
                        bad.push(format!(
                            "{fname} {h} {text:?} ({x}, {y}): cursor {got} != SG {want}"
                        ));
                    }
                }
            }
        }
    }
    (n, bad)
}

#[test]
fn real_simplegraphic_metrics() {
    let fs = fonts();
    let Ok(text) = std::fs::read_to_string(EXPECTED) else {
        eprintln!("PENDING (T6): {EXPECTED} does not exist; run crates/pob-ui-oracle/probe/run.sh under Wine");
        return;
    };
    let m: Value = serde_json::from_str(&text).expect("metrics.json");
    let (n, bad) = compare(&fs, &m);
    eprintln!("T6 probe: {n} values compared, {} mismatches", bad.len());
    assert!(
        bad.is_empty(),
        "{} of {n} differ from the real DLL:\n{}",
        bad.len(),
        bad[..bad.len().min(40)].join("\n")
    );
}

/// The comparator accepts metrics produced by our own engine and flags a single changed value.
#[test]
fn comparator_self_test() {
    let fs = fonts();
    let strings = ["Hello", "a\tb", "^1é\nx", ""];
    let heights = [12, 16];
    let points = [[0, 0, 0], [9, 0, 0], [30, 1, 1]];
    let fonts_used = ["VAR", "FIXED"];
    let mut width = Vec::new();
    let mut cursor = Vec::new();
    for f in fonts_used {
        let face = fs.face(font(f));
        let (mut wf, mut cf) = (Vec::new(), Vec::new());
        for h in heights {
            let (mut wh, mut ch) = (Vec::new(), Vec::new());
            for s in strings {
                wh.push(json!(face.string_width(h, s)));
                ch.push(json!(points
                    .iter()
                    .map(|p| face.cursor_index(h, s, p[0], p[1] * h + p[2]) + 1)
                    .collect::<Vec<_>>()));
            }
            wf.push(json!(wh));
            cf.push(json!(ch));
        }
        width.push(json!(wf));
        cursor.push(json!(cf));
    }
    let mut m = json!({
        "schema": "pob-ui-oracle/probe/metrics.schema.json#1",
        "screenScale": 1.0,
        "fonts": fonts_used, "heights": heights, "points": points, "strings": strings,
        "width": width, "cursor": cursor,
    });
    let (n, bad) = compare(&fs, &m);
    assert_eq!(n, 2 * 2 * 4 * (1 + 3));
    assert!(bad.is_empty(), "{bad:?}");
    m["width"][1][0][0] = json!(1234);
    m["cursor"][0][1][2][1] = json!(99);
    let (_, bad) = compare(&fs, &m);
    assert_eq!(bad.len(), 2, "{bad:?}");
}
