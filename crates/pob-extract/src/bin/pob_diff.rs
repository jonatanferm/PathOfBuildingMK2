//! Skeleton for the live-PoB validation harness. Phase-future.
//!
//! The intent: take a build code, run it through pob-engine (already pure Rust) and
//! through PoB's Lua calc engine in the same process, then diff the resulting `output`
//! tables. This is the Phase 2g goal that's been deferred since the original scope.
//!
//! What's here: an mlua sandbox initialised with the shims pob-extract already uses,
//! plus stubs for the SimpleGraphic API surface that PoB modules call when they bind
//! their UI (DrawString, NewImageHandle, etc.). The actual `runs PoB through this and
//! diffs` logic is not implemented — driving Calcs.buildOutput requires fully loading
//! Modules/Common, Modules/Main, Modules/Data, plus dozens of Classes/* — at least
//! another day of plumbing per `docs/divergences.md`.
//!
//! Once that's wired, the harness can:
//!   - read a build code (MK2 or PoB) from `--code`
//!   - import it on both engines
//!   - print a side-by-side diff of every key in env.player.output
//!
//! Run skeleton:
//!     cargo run -p pob-extract --bin pob_diff -- --pob ../PathOfBuilding

use anyhow::Result;
use mlua::Lua;
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut pob_root = PathBuf::from("../PathOfBuilding");
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--pob" => pob_root = iter.next().expect("--pob requires arg").into(),
            other => eprintln!("ignoring unknown arg: {other}"),
        }
    }

    let lua = build_lua_sandbox(&pob_root)?;

    // Currently this just verifies the sandbox boots and has the constants we expect.
    let result: i64 = lua
        .load("return SkillType.Spell")
        .eval()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("sandbox OK: SkillType.Spell = {result}");
    println!("Phase 2g harness is a skeleton — see docs/divergences.md.");

    Ok(())
}

fn build_lua_sandbox(_pob_root: &std::path::Path) -> Result<Lua> {
    let lua = Lua::new();
    let globals = lua.globals();

    // Constants — same numeric values pob-extract::main.rs already wires in. Doing it
    // again here keeps this binary self-contained; later we'll factor a shared crate.
    let stub = lua.create_function(|_, _: mlua::MultiValue| Ok(()))?;
    globals.set("LoadModule", stub.clone())?;
    globals.set("ConPrintf", stub.clone())?;
    // SimpleGraphic surface that PoB modules call at top level when they bind UI.
    for name in [
        "NewImageHandle",
        "DrawString",
        "DrawImage",
        "SetDrawColor",
        "SetDrawLayer",
        "SetViewport",
        "GetTime",
        "GetCursorPos",
        "GetScreenSize",
        "GetMainObject",
        "RenderInit",
        "AddToBackgroundLuaQueue",
    ] {
        globals.set(name, stub.clone())?;
    }

    let skill_type = lua.create_table()?;
    let _ = skill_type.set("Spell", 2)?;
    let _ = skill_type.set("Attack", 1)?;
    globals.set("SkillType", skill_type)?;

    let kw = lua.create_table()?;
    for (k, v) in [
        ("Fire", 0x20i64),
        ("Cold", 0x40),
        ("Lightning", 0x80),
        ("Hit", 0x40000),
        ("Spell", 0x20000),
        ("Attack", 0x10000),
    ] {
        let _ = kw.set(k, v)?;
    }
    globals.set("KeywordFlag", kw)?;

    Ok(lua)
}
