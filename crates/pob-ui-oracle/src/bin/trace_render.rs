//! `trace-render`: renders a Lua trace through our backend, separating backend bugs from port bugs
//! (ui doc §11.2 CLI). Owner: U8.
//!
//! ```text
//! cargo run -p pob-ui-oracle --bin trace-render -- <trace.jsonl> [--frame N]... [--all]
//!     [--out DIR] [--backend raster|wgpu]
//! ```
//!
//! Each selected frame of the trace (default: the last) is replayed into a fresh `Sg` on PoB's
//! fonts and the synchronous image loader (`diff_report::replay`: images are loaded from their
//! recorded path and flags), and the resulting render-level `Frame` is drawn by the CPU reference
//! raster (`--backend raster`, the default when U6's raster is available) or by pob-sg-wgpu on a
//! headless adapter (`--backend wgpu`). Output: `<out>/frame_<n>.png` (default `out` = the trace's
//! directory).
//!
//! A trace that renders right here but looks wrong in `ui-parity`'s Rust PNG is a port bug; one
//! that renders wrong here although its calls match the Lua side is a backend bug.

use std::path::PathBuf;
use std::process::ExitCode;

use pob_ui_oracle::diff_report;

#[derive(Clone, Copy, PartialEq)]
enum Backend {
    Auto,
    Raster,
    Wgpu,
}

fn usage() -> ExitCode {
    eprintln!(
        "usage: trace-render <trace.jsonl> [--frame N]... [--all] [--out DIR] [--backend raster|wgpu]"
    );
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut it = std::env::args().skip(1);
    let mut trace: Option<PathBuf> = None;
    let mut frames: Vec<usize> = Vec::new();
    let mut all = false;
    let mut out: Option<PathBuf> = None;
    let mut backend = Backend::Auto;
    while let Some(a) = it.next() {
        match a.as_str() {
            "--frame" => match it.next().and_then(|v| v.parse().ok()) {
                Some(n) => frames.push(n),
                None => return usage(),
            },
            "--all" => all = true,
            "--out" => match it.next() {
                Some(d) => out = Some(PathBuf::from(d)),
                None => return usage(),
            },
            "--backend" => {
                backend = match it.next().as_deref() {
                    Some("raster") => Backend::Raster,
                    Some("wgpu") => Backend::Wgpu,
                    _ => return usage(),
                }
            }
            s if s.starts_with('-') => return usage(),
            s => {
                if trace.is_some() {
                    return usage();
                }
                trace = Some(PathBuf::from(s));
            }
        }
    }
    let Some(trace) = trace else {
        return usage();
    };
    let traces = match diff_report::read_jsonl(&trace) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("trace-render: {}: {e:#}", trace.display());
            return ExitCode::from(2);
        }
    };
    if traces.is_empty() {
        eprintln!("trace-render: {}: no frames", trace.display());
        return ExitCode::from(2);
    }
    if all {
        frames = (0..traces.len()).collect();
    } else if frames.is_empty() {
        frames.push(traces.len() - 1);
    }
    let out = out.unwrap_or_else(|| {
        trace
            .parent()
            .map_or_else(|| PathBuf::from("."), std::path::Path::to_path_buf)
    });
    if let Err(e) = std::fs::create_dir_all(&out) {
        eprintln!("trace-render: {}: {e}", out.display());
        return ExitCode::from(2);
    }
    for fi in frames {
        let Some(t) = traces.get(fi) else {
            eprintln!(
                "trace-render: frame {fi} out of range (0..{})",
                traces.len()
            );
            return ExitCode::from(2);
        };
        let r = (|| -> anyhow::Result<PathBuf> {
            let mut sg = diff_report::replay_sg()?;
            let frame = diff_report::replay(&mut sg, t);
            let (w, h, px) = match backend {
                Backend::Auto => diff_report::render_rgba(&frame)?,
                Backend::Raster => diff_report::render_raster(&frame)?,
                Backend::Wgpu => diff_report::render_wgpu(&frame)?,
            };
            let path = out.join(format!("frame_{fi}.png"));
            diff_report::save_png(&path, w, h, px)?;
            Ok(path)
        })();
        match r {
            Ok(p) => println!("{}", p.display()),
            Err(e) => {
                eprintln!("trace-render: frame {fi}: {e:#}");
                return ExitCode::from(1);
            }
        }
    }
    ExitCode::SUCCESS
}
