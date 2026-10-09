//! `target/ui-parity/<name>/{lua,rust}.jsonl` and `report.txt` (first divergence with ±5 ops,
//! both `src` locations, control path), plus `frame_<n>_{lua,rust,diff}.png` through the CPU
//! raster (ui doc §11.2 CLI). Owner: U8. UI-U0 stub.

use crate::runner::RunResult;

/// Write the report for `name` under `out_dir`; returns the number of divergences.
pub fn write_report(
    out_dir: &std::path::Path,
    name: &str,
    run: &RunResult,
    png: bool,
) -> anyhow::Result<usize> {
    let _ = (out_dir, name, run, png);
    todo!("U8: diff report")
}
