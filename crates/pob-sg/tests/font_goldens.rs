//! T2 font goldens (ui doc §11.1, U2): 200 strings × 4 heights × 3 fonts.
//!
//! `font_goldens.json` is NOT a snapshot of this crate: it is written by
//! `crates/pob-ui-oracle/probe/rfont_ref.py`, an independent transcription of SimpleGraphic's
//! `r_font.cpp` @ 3b1a346 with emulated f32 arithmetic. Each case checks, bit-exactly,
//! - `DrawStringWidth` (`FontFace::string_width`),
//! - `DrawStringCursorIndex` at 10 points (`FontFace::cursor_index`, 0-based byte offsets),
//! - the whole BIND/COLOR/QUAD stream of two `DrawString`s (LEFT at (7.4, 11.6), then one of the
//!   5 aligns at (300.5, 40.5), colour carried over), compared as an FNV-1a digest of the
//!   canonical f32-bit text form plus quad and command counts.
//!
//! The same strings are the Wine probe's case table (`probe/cases.lua`, checked below), so the
//! real `SimpleGraphic.dll` metrics can be compared once they exist (`font_probe.rs`).
//!
//! Regenerate (only when the case set changes):
//! `python3 crates/pob-ui-oracle/probe/rfont_ref.py --fonts <PoB>/runtime/SimpleGraphic/Fonts goldens crates/pob-sg/tests/font_goldens.json`
//! and `... cases-lua crates/pob-ui-oracle/probe/cases.lua`.

#![allow(clippy::many_single_char_names)]

#[path = "font_util.rs"]
mod font_util;

use font_util::{canon, fnv1a64, font, fonts, Rec};
use pob_sg::Align;
use serde_json::Value;

fn goldens() -> Value {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/font_goldens.json");
    serde_json::from_str(&std::fs::read_to_string(p).expect("font_goldens.json")).unwrap()
}

fn f32s(v: &Value) -> Vec<f32> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap() as f32)
        .collect()
}

#[test]
fn goldens_match_reference() {
    let fs = fonts();
    let g = goldens();
    let strings: Vec<&str> = g["strings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect();
    assert_eq!(strings.len(), 200);
    assert_eq!(g["fonts"].as_array().unwrap().len(), 3);
    assert_eq!(g["heights"].as_array().unwrap().len(), 4);
    let points: Vec<(i32, i32, i32)> = g["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let p = p.as_array().unwrap();
            let i = |k: usize| p[k].as_i64().unwrap() as i32;
            (i(0), i(1), i(2))
        })
        .collect();
    let d = &g["draw"];
    let sw = d["screen_w"].as_i64().unwrap() as i32;
    let wh = d["win_h"].as_i64().unwrap() as i32;
    let col0 = f32s(&d["col"]);
    let lp = f32s(&d["left_pos"]);
    let ap = f32s(&d["align_pos"]);

    let results = g["results"].as_array().unwrap();
    assert_eq!(results.len(), 200 * 4 * 3);
    let mut fails = Vec::new();
    let (mut n_width, mut n_cursor, mut n_draw, mut n_quads) = (0, 0, 0, 0u64);
    for r in results {
        let face = fs.face(font(r["font"].as_str().unwrap()));
        let h = r["height"].as_i64().unwrap() as i32;
        let si = r["s"].as_u64().unwrap() as usize;
        let text = strings[si];
        let tag = format!("{} h={h} s#{si} {text:?}", r["font"]);

        let w = face.string_width(h, text);
        n_width += 1;
        if i64::from(w) != r["width"].as_i64().unwrap() {
            fails.push(format!("{tag}: width {w} != {}", r["width"]));
        }

        for (k, &(x, ky, ay)) in points.iter().enumerate() {
            let c = face.cursor_index(h, text, x, ky * h + ay);
            n_cursor += 1;
            let want = r["cursor"][k].as_u64().unwrap() as usize;
            if c != want {
                fails.push(format!("{tag}: cursor({x}, {}) {c} != {want}", ky * h + ay));
            }
        }

        let mut col = [col0[0], col0[1], col0[2], col0[3]];
        let mut rec = Rec::default();
        face.draw(
            &mut rec,
            [lp[0], lp[1]],
            Align::Left,
            h,
            &mut col,
            text,
            sw,
            wh,
        );
        let align = Align::from_lua(r["align"].as_str()).unwrap();
        face.draw(&mut rec, [ap[0], ap[1]], align, h, &mut col, text, sw, wh);
        n_draw += 2;
        let quads = rec
            .0
            .iter()
            .filter(|e| matches!(e, font_util::Ev::Quad(_)))
            .count();
        n_quads += quads as u64;
        let digest = fnv1a64(&canon(&fs, &rec.0));
        if quads as u64 != r["quads"].as_u64().unwrap()
            || rec.0.len() as u64 != r["cmds"].as_u64().unwrap()
            || digest != r["digest"].as_str().unwrap()
        {
            fails.push(format!(
                "{tag} {:?}: draw quads {quads}/{} cmds {}/{} digest {digest}/{}",
                r["align"],
                r["quads"],
                rec.0.len(),
                r["cmds"],
                r["digest"]
            ));
        }
    }
    eprintln!(
        "font goldens: {} cases, {n_width} widths, {n_cursor} cursor indices, {n_draw} draws ({n_quads} quads), {} mismatches",
        results.len(),
        fails.len()
    );
    assert!(
        fails.is_empty(),
        "{} mismatches:\n{}",
        fails.len(),
        fails[..fails.len().min(30)].join("\n")
    );
}

/// Decode the `"..."` Lua literals `rfont_ref.py` writes (printable ASCII plus `\ddd`).
fn lua_unquote(lit: &str) -> Vec<u8> {
    let b = lit.as_bytes();
    assert!(b[0] == b'"' && b[b.len() - 1] == b'"', "{lit}");
    let b = &b[1..b.len() - 1];
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' {
            let n: u32 = std::str::from_utf8(&b[i + 1..i + 4])
                .unwrap()
                .parse()
                .unwrap();
            out.push(n as u8);
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

#[test]
fn probe_cases_match_goldens() {
    let g = goldens();
    let p = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../pob-ui-oracle/probe/cases.lua"
    );
    let lua = std::fs::read_to_string(p).expect("probe/cases.lua");
    let strings: Vec<Vec<u8>> = lua
        .lines()
        .filter(|l| l.starts_with("\t\t\""))
        .map(|l| lua_unquote(l.trim().trim_end_matches(',')))
        .collect();
    let want: Vec<Vec<u8>> = g["strings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().as_bytes().to_vec())
        .collect();
    assert_eq!(strings, want);
    for (k, v) in [
        ("fonts", "\"VAR\", \"VAR BOLD\", \"FIXED\""),
        ("heights", "12, 16, 22, 30"),
    ] {
        assert!(lua.contains(&format!("\t{k} = {{ {v} }},")), "{k}");
        let arr: Vec<String> = g[k]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.to_string())
            .collect();
        assert_eq!(arr.join(", "), v);
    }
}
