//! Reads PoB's `src/Data/` and `src/TreeData/` Lua tables, emits Rust-loadable JSON under
//! the workspace `data/` directory.
//!
//! Run from the workspace root:
//!
//! ```text
//! cargo run -p pob-extract -- --pob ../PathOfBuilding --out data
//! ```

fn main() -> anyhow::Result<()> {
    println!("pob-extract: not yet implemented");
    Ok(())
}
