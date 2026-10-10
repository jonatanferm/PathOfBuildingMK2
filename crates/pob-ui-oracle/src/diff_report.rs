//! `target/ui-parity/<name>/{lua,rust}.jsonl` and `report.txt` (first divergence with ±5 ops,
//! both `src` locations, control path), plus `frame_<n>_{lua,rust,diff}.png` through the CPU
//! raster (ui doc §11.2 CLI). Owner: U8.
//!
//! The comparison is T3 (§11.1): per frame, `pob_sg::trace::normalize` turns each side's calls into
//! per-`(layer, sub)` buckets of draws with their effective viewport and colour, and
//! `pob_sg::trace::diff` compares them (`DiffConfig::default()`: 1e-6 on numbers, colours exact).
//! Side effects are compared per frame in order, and a different frame count is a divergence.
//! `normalize`/`diff` are U1's; while they are unimplemented at this unit's base, a local
//! equivalent of the documented semantics is used (U1-PENDING, [`normalize_frame`]).
//!
//! PNGs: the frames are rebuilt by replaying each trace into a fresh `Sg` ([`replay`]) and drawn
//! with the CPU raster (`pob_sg::raster`, U6); while that is unimplemented, with pob-sg-wgpu's
//! offscreen renderer instead (U6-PENDING). Without either, PNGs are skipped with a note.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::Path;
use std::sync::OnceLock;

use pob_sg::trace::{ApiCall, Buckets, DiffConfig, Divergence, DrawOp, FrameTrace, ImgRef};
use pob_sg::{Frame, ImageHandle, ScreenInfo, Sg};

use crate::runner::RunResult;

fn u1_available() -> bool {
    static AVAIL: OnceLock<bool> = OnceLock::new();
    *AVAIL.get_or_init(|| {
        let t = FrameTrace::default();
        let s = crate::runner::screen(100, 100, 1.0);
        crate::runner::if_implemented(|| pob_sg::trace::normalize(&t, &s)).is_some()
    })
}

// ---- U1-PENDING local normalize/diff (the documented semantics of pob_sg::trace) ----

fn c_str(s: &str) -> &str {
    s.split('\0').next().unwrap_or("")
}

fn full_viewport(x: f64, y: f64, screen: &ScreenInfo) -> [f64; 4] {
    let s = f64::from(screen.dpi_scale);
    [x, y, f64::from(screen.fb_w) / s, f64::from(screen.fb_h) / s]
}

/// The colour after a DrawString: the last escape in the text with alpha 1, else unchanged.
fn after_draw_string(mut col: [f32; 4], text: &str) -> [f32; 4] {
    let b = c_str(text).as_bytes();
    let mut i = 0;
    while i < b.len() {
        let n = pob_sg::color::is_color_escape(&b[i..]);
        if n > 0 {
            pob_sg::color::read_color_escape(&b[i..], n, &mut col);
            col[3] = 1.0;
            i += n;
        } else {
            i += 1;
        }
    }
    col
}

/// Replays SetDrawLayer/SetViewport/SetDrawColor (incl. the DrawString escape leak) into buckets.
#[allow(clippy::many_single_char_names)]
pub fn normalize_frame(t: &FrameTrace, screen: &ScreenInfo) -> Buckets {
    if u1_available() {
        return pob_sg::trace::normalize(t, screen);
    }
    let mut out = Buckets::new();
    let mut layer = (0, 0);
    let mut viewport = full_viewport(0.0, 0.0, screen);
    let mut color = [1.0f32; 4];
    for rec in &t.calls {
        let op = || DrawOp {
            viewport,
            color,
            call: rec.call.clone(),
            ctl: rec.ctl.clone(),
            src: rec.src.clone(),
        };
        match &rec.call {
            ApiCall::SetDrawLayer { layer: l, sub } => match (l, sub) {
                (None, Some(s)) => layer.1 = *s,
                (Some(l), s) => layer = (*l, s.unwrap_or(0)),
                (None, None) => {}
            },
            ApiCall::SetViewport(Some([x, y, w, h])) => {
                let s = f64::from(screen.dpi_scale);
                viewport = if (h * s).ceil() as i32 == 0 {
                    full_viewport(*x, *y, screen)
                } else {
                    [*x, *y, *w, *h]
                };
            }
            ApiCall::SetViewport(None) => viewport = full_viewport(0.0, 0.0, screen),
            ApiCall::SetDrawColor(c) => color = c.map(|v| v as f32),
            ApiCall::SetDrawColorEsc(s) => {
                if let Some(c) = pob_sg::color::parse_escape_color(c_str(s)) {
                    color = c;
                }
            }
            ApiCall::DrawImage { .. } | ApiCall::DrawImageQuad { .. } => {
                out.entry(layer).or_default().push(op());
            }
            ApiCall::DrawString { text, .. } => {
                out.entry(layer).or_default().push(op());
                color = after_draw_string(color, text);
            }
        }
    }
    out
}

fn near(a: f64, b: f64, eps: f64) -> bool {
    a == b || (a - b).abs() <= eps
}

fn near_all(a: &[f64], b: &[f64], eps: f64) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| near(*x, *y, eps))
}

fn call_eq(a: &ApiCall, b: &ApiCall, eps: f64) -> bool {
    match (a, b) {
        (
            ApiCall::DrawImage {
                img: ia,
                rect: ra,
                tc: ta,
            },
            ApiCall::DrawImage {
                img: ib,
                rect: rb,
                tc: tb,
            },
        ) => {
            ia == ib
                && near_all(ra, rb, eps)
                && match (ta, tb) {
                    (Some(x), Some(y)) => near_all(x, y, eps),
                    (None, None) => true,
                    _ => false,
                }
        }
        (
            ApiCall::DrawImageQuad {
                img: ia,
                p: pa,
                tc: ta,
            },
            ApiCall::DrawImageQuad {
                img: ib,
                p: pb,
                tc: tb,
            },
        ) => {
            ia == ib
                && near_all(pa, pb, eps)
                && match (ta, tb) {
                    (Some(x), Some(y)) => near_all(x, y, eps),
                    (None, None) => true,
                    _ => false,
                }
        }
        (
            ApiCall::DrawString {
                x: xa,
                y: ya,
                align: aa,
                height: ha,
                font: fa,
                text: ta,
            },
            ApiCall::DrawString {
                x: xb,
                y: yb,
                align: ab,
                height: hb,
                font: fb,
                text: tb,
            },
        ) => {
            near(*xa, *xb, eps)
                && near(*ya, *yb, eps)
                && aa == ab
                && near(*ha, *hb, eps)
                && fa == fb
                && ta == tb
        }
        _ => a == b,
    }
}

fn op_eq(a: &DrawOp, b: &DrawOp, eps: f64) -> bool {
    near_all(&a.viewport, &b.viewport, eps) && a.color == b.color && call_eq(&a.call, &b.call, eps)
}

/// Compares two frames' buckets (`pob_sg::trace::diff`, or the local equivalent).
pub fn diff_buckets(lua: &Buckets, rust: &Buckets, cfg: &DiffConfig) -> Vec<Divergence> {
    if u1_available() {
        return pob_sg::trace::diff(lua, rust, cfg);
    }
    let mut out = Vec::new();
    let keys: BTreeSet<(i32, i32)> = lua.keys().chain(rust.keys()).copied().collect();
    let empty = Vec::new();
    for k in keys {
        let a = lua.get(&k).unwrap_or(&empty);
        let b = rust.get(&k).unwrap_or(&empty);
        for i in 0..a.len().max(b.len()) {
            match (a.get(i), b.get(i)) {
                (Some(x), Some(y)) if !op_eq(x, y, cfg.abs_eps) => out.push(Divergence::Changed {
                    bucket: k,
                    idx: i,
                    lua: x.clone(),
                    rust: y.clone(),
                }),
                (Some(x), None) => out.push(Divergence::Missing {
                    bucket: k,
                    idx: i,
                    op: x.clone(),
                }),
                (None, Some(y)) => out.push(Divergence::Extra {
                    bucket: k,
                    idx: i,
                    op: y.clone(),
                }),
                _ => {}
            }
        }
    }
    out
}

/// One frame's divergences (draws, then side effects in order).
pub fn diff_frame(lua: &FrameTrace, rust: &FrameTrace, cfg: &DiffConfig) -> Vec<Divergence> {
    let screen = lua
        .screen
        .or(rust.screen)
        .unwrap_or_else(|| crate::runner::screen(1920, 1080, 1.0));
    let mut out = diff_buckets(
        &normalize_frame(lua, &lua.screen.unwrap_or(screen)),
        &normalize_frame(rust, &rust.screen.unwrap_or(screen)),
        cfg,
    );
    for i in 0..lua.effects.len().max(rust.effects.len()) {
        let (a, b) = (lua.effects.get(i), rust.effects.get(i));
        if a != b {
            out.push(Divergence::Effect {
                lua: a.cloned(),
                rust: b.cloned(),
            });
        }
    }
    out
}

/// All divergences of a run, per frame index (a frame missing on one side counts as one).
pub fn diff_run(run: &RunResult, cfg: &DiffConfig) -> Vec<(usize, Vec<Divergence>)> {
    let empty = FrameTrace::default();
    let mut out = Vec::new();
    for i in 0..run.lua.len().max(run.rust.len()) {
        let (a, b) = (run.lua.get(i), run.rust.get(i));
        let d = match (a, b) {
            (Some(a), Some(b)) => diff_frame(a, b, cfg),
            (Some(a), None) => diff_frame(
                a,
                &FrameTrace {
                    screen: a.screen,
                    ..empty.clone()
                },
                cfg,
            ),
            (None, Some(b)) => diff_frame(
                &FrameTrace {
                    screen: b.screen,
                    ..empty.clone()
                },
                b,
                cfg,
            ),
            (None, None) => Vec::new(),
        };
        if !d.is_empty() || a.is_none() || b.is_none() {
            out.push((i, d));
        }
    }
    out
}

fn fmt_op(op: &DrawOp) -> String {
    format!(
        "{:?} vp={:?} col={:?}  src={}  ctl={}",
        op.call,
        op.viewport,
        op.color,
        op.src.as_deref().unwrap_or("-"),
        op.ctl.as_deref().unwrap_or("-")
    )
}

fn context(b: &Buckets, bucket: (i32, i32), idx: usize, mark: &str) -> String {
    let mut s = String::new();
    let ops = b.get(&bucket).map(Vec::as_slice).unwrap_or(&[]);
    let lo = idx.saturating_sub(5);
    let hi = (idx + 6).min(ops.len());
    for (i, op) in ops.iter().enumerate().take(hi).skip(lo) {
        let m = if i == idx { mark } else { "  " };
        let _ = writeln!(s, "    {m}[{i}] {}", fmt_op(op));
    }
    if ops.len() <= idx {
        let _ = writeln!(s, "    {mark}[{idx}] (none; bucket has {} ops)", ops.len());
    }
    s
}

fn describe(d: &Divergence) -> String {
    match d {
        Divergence::Missing { bucket, idx, op } => {
            format!(
                "bucket {bucket:?} op {idx}: missing on the Rust side: {}",
                fmt_op(op)
            )
        }
        Divergence::Extra { bucket, idx, op } => {
            format!(
                "bucket {bucket:?} op {idx}: extra on the Rust side: {}",
                fmt_op(op)
            )
        }
        Divergence::Changed {
            bucket,
            idx,
            lua,
            rust,
        } => format!(
            "bucket {bucket:?} op {idx} differs\n    lua:  {}\n    rust: {}",
            fmt_op(lua),
            fmt_op(rust)
        ),
        Divergence::Effect { lua, rust } => {
            format!("side effect differs: lua {lua:?}, rust {rust:?}")
        }
    }
}

fn write_jsonl(path: &Path, frames: &[FrameTrace]) -> anyhow::Result<()> {
    let mut s = String::new();
    for f in frames {
        s.push_str(&serde_json::to_string(f)?);
        s.push('\n');
    }
    std::fs::write(path, s)?;
    Ok(())
}

/// Read a `{lua,rust}.jsonl` trace file.
pub fn read_jsonl(path: &Path) -> anyhow::Result<Vec<FrameTrace>> {
    let text = std::fs::read_to_string(path)?;
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).map_err(Into::into))
        .collect()
}

/// Write the report for `name` under `out_dir`; returns the number of divergences.
pub fn write_report(
    out_dir: &std::path::Path,
    name: &str,
    run: &RunResult,
    png: bool,
) -> anyhow::Result<usize> {
    let dir = out_dir.join(name);
    std::fs::create_dir_all(&dir)?;
    write_jsonl(&dir.join("lua.jsonl"), &run.lua)?;
    write_jsonl(&dir.join("rust.jsonl"), &run.rust)?;
    for (side, snaps) in [("lua", &run.lua_snapshots), ("rust", &run.rust_snapshots)] {
        for (n, xml) in snaps {
            std::fs::write(dir.join(format!("snapshot_{n}_{side}.xml")), xml)?;
        }
    }
    let cfg = DiffConfig::default();
    let divs = if run.compared {
        diff_run(run, &cfg)
    } else {
        Vec::new()
    };
    let total: usize = divs.iter().map(|(_, d)| d.len().max(1)).sum();
    let mut r = String::new();
    let _ = writeln!(r, "ui-parity report: {name}");
    let _ = writeln!(
        r,
        "frames: lua {}, rust {}; draw ops: lua {}, rust {}",
        run.lua.len(),
        run.rust.len(),
        run.lua.iter().map(|f| f.calls.len()).sum::<usize>(),
        run.rust.iter().map(|f| f.calls.len()).sum::<usize>()
    );
    if !u1_available() {
        let _ = writeln!(
            r,
            "note: pob_sg::trace::normalize/diff (U1) unavailable; local equivalent used"
        );
    }
    if !run.compared {
        let _ = writeln!(r, "rust side: not run (Lua side only)");
    }
    if let Some(e) = &run.rust_error {
        let _ = writeln!(r, "rust side error: {e}");
    }
    let _ = writeln!(r, "divergences: {total} in {} frames", divs.len());
    if let Some((fi, ds)) = divs.first() {
        let _ = writeln!(r, "\nfirst divergent frame: {fi}");
        let empty = FrameTrace::default();
        let lf = run.lua.get(*fi).unwrap_or(&empty);
        let rf = run.rust.get(*fi).unwrap_or(&empty);
        let scr = lf
            .screen
            .or(rf.screen)
            .unwrap_or_else(|| crate::runner::screen(1920, 1080, 1.0));
        let lb = normalize_frame(lf, &scr);
        let rb = normalize_frame(rf, &scr);
        match ds.first() {
            None => {
                let _ = writeln!(r, "  frame missing on one side");
            }
            Some(d) => {
                let _ = writeln!(r, "  {}", describe(d));
                if let Divergence::Missing { bucket, idx, .. }
                | Divergence::Extra { bucket, idx, .. }
                | Divergence::Changed { bucket, idx, .. } = d
                {
                    let _ = writeln!(r, "  lua context:\n{}", context(&lb, *bucket, *idx, "=>"));
                    let _ = writeln!(r, "  rust context:\n{}", context(&rb, *bucket, *idx, "=>"));
                }
                if ds.len() > 1 {
                    let _ = writeln!(r, "  (+{} more in this frame)", ds.len() - 1);
                }
            }
        }
    }
    if !run.audit.is_empty() {
        let _ = writeln!(
            r,
            "\naudit: {} hosts with order-dependent overlap",
            run.audit.len()
        );
        for h in &run.audit {
            let _ = writeln!(
                r,
                "  {}  ({} inversions, e.g. {})",
                h.host, h.count, h.example
            );
        }
    }
    if png {
        let mut frames: Vec<usize> = divs.iter().map(|(i, _)| *i).take(1).collect();
        if let Some(last) = run.lua.len().checked_sub(1) {
            frames.push(last);
        }
        frames.dedup();
        for fi in frames {
            match write_pngs(&dir, fi, run) {
                Ok(()) => {
                    let _ = writeln!(r, "png: frame_{fi}_{{lua,rust,diff}}.png");
                }
                Err(e) => {
                    let _ = writeln!(r, "png: frame {fi} skipped: {e}");
                }
            }
        }
    }
    std::fs::write(dir.join("report.txt"), &r)?;
    Ok(total)
}

/// Write the audit host list (§11.2 audit mode): one line per host,
/// `<host path>\t<inversions>\t<example pair>`, sorted by host; a header line names the format.
pub fn write_audit(path: &Path, hits: &[crate::markers::AuditHit]) -> anyhow::Result<()> {
    let mut s = String::from("# host\tinversions\texample (canonical first < native first)\n");
    for h in hits {
        let _ = writeln!(s, "{}\t{}\t{}", h.host, h.count, h.example);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, s)?;
    Ok(())
}

/// Compares two Lua-side runs of the same script (determinism, §11.2; PoB's string-keyed `pairs`
/// order is random per process, calc doc D8). `Ok(sites)`: every frame has the same calls and
/// side effects as multisets; `sites` lists the `src` locations of calls whose position differs
/// (empty when the traces are identical). `Err`: the runs differ in more than order.
pub fn order_only_difference(a: &[FrameTrace], b: &[FrameTrace]) -> Result<Vec<String>, String> {
    if a.len() != b.len() {
        return Err(format!("frame counts differ: {} vs {}", a.len(), b.len()));
    }
    let key = |c: &pob_sg::trace::CallRec| -> String {
        format!(
            "{}|{:?}|{:?}",
            serde_json::to_string(&c.call).unwrap_or_default(),
            c.ctl,
            c.src
        )
    };
    let mut sites = BTreeSet::new();
    for (i, (fa, fb)) in a.iter().zip(b).enumerate() {
        if fa.calls.len() != fb.calls.len() {
            return Err(format!(
                "frame {i}: call counts differ: {} vs {}",
                fa.calls.len(),
                fb.calls.len()
            ));
        }
        let mut ka: Vec<String> = fa.calls.iter().map(key).collect();
        let mut kb: Vec<String> = fb.calls.iter().map(key).collect();
        for (ca, cb) in fa.calls.iter().zip(&fb.calls) {
            if key(ca) != key(cb) {
                sites.insert(ca.src.clone().unwrap_or_default());
                sites.insert(cb.src.clone().unwrap_or_default());
            }
        }
        ka.sort();
        kb.sort();
        if ka != kb {
            return Err(format!("frame {i}: the calls differ (not only in order)"));
        }
        let mut ea: Vec<String> = fa.effects.iter().map(|e| format!("{e:?}")).collect();
        let mut eb: Vec<String> = fb.effects.iter().map(|e| format!("{e:?}")).collect();
        if ea != eb {
            sites.insert(format!("side effects of frame {i}"));
        }
        ea.sort();
        eb.sort();
        if ea != eb {
            return Err(format!(
                "frame {i}: the side effects differ (not only in order)"
            ));
        }
    }
    Ok(sites.into_iter().collect())
}

/// Read a host list written by [`write_audit`].
pub fn read_audit(path: &Path) -> anyhow::Result<Vec<crate::markers::AuditHit>> {
    let text =
        std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    let mut out = Vec::new();
    for l in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        let mut f = l.splitn(3, '\t');
        let (Some(host), Some(count), Some(example)) = (f.next(), f.next(), f.next()) else {
            anyhow::bail!("{}: bad line {l:?}", path.display());
        };
        out.push(crate::markers::AuditHit {
            host: host.to_owned(),
            count: count.parse()?,
            example: example.to_owned(),
        });
    }
    Ok(out)
}

/// The union of several audit runs: one entry per host, inversion counts summed, the first
/// example kept.
pub fn merge_audit(hits: Vec<crate::markers::AuditHit>) -> Vec<crate::markers::AuditHit> {
    let mut by_host: std::collections::BTreeMap<String, crate::markers::AuditHit> =
        std::collections::BTreeMap::new();
    for h in hits {
        by_host
            .entry(h.host.clone())
            .and_modify(|e| e.count += h.count)
            .or_insert(h);
    }
    by_host.into_values().collect()
}

// ---- replay and rendering ----

/// Rebuilds the render-level `Frame` of a trace frame by replaying its calls into `sg` (images by
/// their `ImgRef`, loaded synchronously first so their pixels are among the frame's uploads).
pub fn replay(sg: &mut Sg, t: &FrameTrace) -> Frame {
    let mut handles: HashMap<(String, u8), ImageHandle> = HashMap::new();
    let mut handle = |sg: &mut Sg, r: &Option<ImgRef>| -> Option<ImageHandle> {
        let r = r.as_ref()?;
        let key = (r.path.clone(), r.flags.bits());
        Some(
            handles
                .entry(key)
                .or_insert_with(|| {
                    let h = sg.new_image_handle();
                    h.load(sg, &r.path, r.flags);
                    h
                })
                .clone(),
        )
    };
    // Load first: pixels must be uploaded with this frame.
    let mut pre = Vec::new();
    for c in &t.calls {
        match &c.call {
            ApiCall::DrawImage { img, .. } | ApiCall::DrawImageQuad { img, .. } => {
                pre.push(handle(sg, img));
            }
            _ => {}
        }
    }
    let screen = t
        .screen
        .unwrap_or_else(|| crate::runner::screen(1920, 1080, 1.0));
    sg.begin_frame(screen);
    let mut imgs = pre.into_iter();
    for c in &t.calls {
        let _ = pob_lua::error::catch(|| match &c.call {
            ApiCall::SetDrawLayer { layer, sub } => sg.set_draw_layer(*layer, *sub),
            ApiCall::SetViewport(Some(v)) => sg.set_viewport(v[0], v[1], v[2], v[3]),
            ApiCall::SetViewport(None) => sg.reset_viewport(),
            ApiCall::SetDrawColor(c) => sg.set_draw_color_a(c[0], c[1], c[2], c[3]),
            ApiCall::SetDrawColorEsc(s) => sg.set_draw_color_esc(s),
            ApiCall::DrawImage { rect, tc, .. } => {
                let h = imgs.next().flatten();
                match tc {
                    Some(tc) => sg.draw_image_tc(
                        h.as_ref(),
                        rect[0],
                        rect[1],
                        rect[2],
                        rect[3],
                        tc[0],
                        tc[1],
                        tc[2],
                        tc[3],
                    ),
                    None => sg.draw_image(h.as_ref(), rect[0], rect[1], rect[2], rect[3]),
                }
            }
            ApiCall::DrawImageQuad { p, tc, .. } => {
                let h = imgs.next().flatten();
                match tc {
                    Some(tc) => sg.draw_image_quad_tc(h.as_ref(), *p, *tc),
                    None => sg.draw_image_quad(h.as_ref(), *p),
                }
            }
            ApiCall::DrawString {
                x,
                y,
                align,
                height,
                font,
                text,
            } => sg.draw_string(*x, *y, *align, *height, *font, text),
        });
    }
    sg.end_frame()
}

/// A fresh `Sg` on PoB's fonts and the headless loader (for replays).
pub fn replay_sg() -> anyhow::Result<Sg> {
    let root = crate::boot::locate_pob_root()
        .ok_or_else(|| anyhow::anyhow!("PoB not found (set POB_ROOT)"))?;
    let handles = crate::boot::OracleHandles::default();
    let (sg, _sys) = crate::boot::oracle_parts(&root, &std::env::temp_dir(), &handles)?;
    Ok(sg)
}

fn raster_available() -> bool {
    static AVAIL: OnceLock<bool> = OnceLock::new();
    *AVAIL.get_or_init(|| {
        let f = Frame {
            screen: crate::runner::screen(1, 1, 1.0),
            layers: Vec::new(),
            uploads: Vec::new(),
            releases: Vec::new(),
            digest: 0,
        };
        crate::runner::if_implemented(|| pob_sg::raster::Raster::new().render(&f)).is_some()
    })
}

/// RGBA8 of `frame` at framebuffer size: the CPU raster, or (U6-PENDING) the wgpu renderer.
pub fn render_rgba(frame: &Frame) -> anyhow::Result<(u32, u32, Vec<u8>)> {
    if raster_available() {
        return Ok(pob_sg::raster::Raster::new().render(frame));
    }
    render_wgpu(frame)
}

/// RGBA8 of `frame` through the CPU reference raster only (an error while U6's raster is a stub).
pub fn render_raster(frame: &Frame) -> anyhow::Result<(u32, u32, Vec<u8>)> {
    anyhow::ensure!(
        raster_available(),
        "pob_sg::raster is not implemented at this base (U6 pending)"
    );
    Ok(pob_sg::raster::Raster::new().render(frame))
}

/// pob-sg-wgpu's offscreen renderer on a headless adapter.
pub fn render_wgpu(frame: &Frame) -> anyhow::Result<(u32, u32, Vec<u8>)> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let (adapter, device, queue) = pollster::block_on(pob_sg_wgpu::request_device(&instance, None))
        .map_err(anyhow::Error::msg)?;
    let mut r = pob_sg_wgpu::SgRenderer::new(&device, &adapter.limits(), pob_sg_wgpu::RT_FORMAT);
    r.prepare(&device, &queue, frame);
    let (w, h) = (
        frame.screen.fb_w.max(1) as u32,
        frame.screen.fb_h.max(1) as u32,
    );
    let out = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("trace-render output"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: pob_sg_wgpu::RT_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = out.create_view(&wgpu::TextureViewDescriptor::default());
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    r.render(&mut enc, &view);
    queue.submit(Some(enc.finish()));
    Ok(r.read_back(&device, &queue))
}

/// Write an RGBA8 image as PNG.
pub fn save_png(path: &Path, w: u32, h: u32, rgba: Vec<u8>) -> anyhow::Result<()> {
    let img = image::RgbaImage::from_raw(w, h, rgba)
        .ok_or_else(|| anyhow::anyhow!("bad image buffer {w}x{h}"))?;
    img.save(path)?;
    Ok(())
}

fn write_pngs(dir: &Path, fi: usize, run: &RunResult) -> anyhow::Result<()> {
    let mut imgs = Vec::new();
    for (side, frames) in [("lua", &run.lua), ("rust", &run.rust)] {
        if let Some(t) = frames.get(fi) {
            let mut sg = replay_sg()?;
            let frame = replay(&mut sg, t);
            let (w, h, px) = render_rgba(&frame)?;
            save_png(
                &dir.join(format!("frame_{fi}_{side}.png")),
                w,
                h,
                px.clone(),
            )?;
            imgs.push((w, h, px));
        }
    }
    if let [(w, h, a), (w2, h2, b)] = imgs.as_slice() {
        if (w, h) == (w2, h2) {
            let d: Vec<u8> = a
                .chunks_exact(4)
                .zip(b.chunks_exact(4))
                .flat_map(|(p, q)| {
                    let c = |i: usize| (p[i].abs_diff(q[i]) as u16 * 4).min(255) as u8;
                    [c(0), c(1), c(2), 255]
                })
                .collect();
            save_png(&dir.join(format!("frame_{fi}_diff.png")), *w, *h, d)?;
        }
    }
    Ok(())
}
