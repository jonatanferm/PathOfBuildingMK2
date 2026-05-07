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

    // Smoke-test that the sandbox can run a simple Lua snippet using the shims we
    // installed.
    let result: i64 = lua
        .load("return SkillType.Spell")
        .eval()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("Sandbox boot OK: SkillType.Spell = {result}");

    // Try loading PoB's Data/Global.lua which sets up ModFlag / KeywordFlag /
    // SkillType — the foundation table everything else depends on.
    let global_path = pob_root.join("src/Data/Global.lua");
    if global_path.is_file() {
        let src = std::fs::read_to_string(&global_path)?;
        // Strip the local `band` definitions etc. that don't have Lua-bit available.
        // Wrap in pcall so failures don't kill the binary.
        let wrapped = format!(
            "local ok, err = pcall(function()\n{src}\nend)\nif not ok then return tostring(err) else return 'OK' end"
        );
        match lua
            .load(&wrapped)
            .set_name("Data/Global.lua")
            .eval::<String>()
        {
            Ok(s) => println!("Data/Global.lua: {s}"),
            Err(e) => println!("Data/Global.lua eval error: {e}"),
        }
    } else {
        println!("Data/Global.lua not found at {}", global_path.display());
    }

    println!("\nPhase 2g harness still a skeleton — see docs/divergences.md.");
    println!("To drive a full Build through PoB's calc engine we'd need to:");
    println!("  1. Vendor or stub HeadlessWrapper.lua + Modules/Common.lua + Modules/Main.lua");
    println!("  2. Load every src/Data/* file via the LoadModule shim");
    println!("  3. Construct a Build XML and feed it through Modules/Build.lua's loader");
    println!("  4. Call calcs.buildOutput and diff env.player.output against pob-engine");

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

    // Common.lua-equivalent helpers PoB scripts assume exist at module-load time.
    let copy_table = lua.create_function(|lua, t: mlua::Table| {
        let copy = lua.create_table()?;
        for pair in t.pairs::<mlua::Value, mlua::Value>() {
            let (k, v) = pair?;
            copy.set(k, v)?;
        }
        Ok(copy)
    })?;
    globals.set("copyTable", copy_table)?;

    let wipe_table = lua.create_function(|_, t: mlua::Table| {
        // Set every entry to nil. Iterate keys first to avoid mutation during walk.
        let keys: Vec<mlua::Value> = t
            .pairs::<mlua::Value, mlua::Value>()
            .filter_map(|p| p.ok().map(|(k, _)| k))
            .collect();
        for k in keys {
            t.set(k, mlua::Value::Nil)?;
        }
        Ok(())
    })?;
    globals.set("wipeTable", wipe_table)?;

    let t_insert = lua.create_function(|_, args: mlua::MultiValue| {
        let mut iter = args.into_iter();
        let t = match iter.next() {
            Some(mlua::Value::Table(t)) => t,
            _ => return Ok(()),
        };
        let len = t.raw_len();
        let v = iter.next().unwrap_or(mlua::Value::Nil);
        t.set(len + 1, v)?;
        Ok(())
    })?;
    let table_tbl: mlua::Table = globals.get("table")?;
    table_tbl.set("insert", t_insert)?;
    globals.set("t_insert", table_tbl.get::<mlua::Function>("insert")?)?;

    // bit library (PoB requires LuaJIT-style bit ops).
    let bit_tbl = lua.create_table()?;
    bit_tbl.set(
        "bor",
        lua.create_function(|_, args: mlua::MultiValue| {
            let mut acc: i64 = 0;
            for v in args {
                if let mlua::Value::Integer(i) = v {
                    acc |= i;
                } else if let mlua::Value::Number(n) = v {
                    acc |= n as i64;
                }
            }
            Ok(acc)
        })?,
    )?;
    bit_tbl.set(
        "band",
        lua.create_function(|_, args: mlua::MultiValue| {
            let mut acc: i64 = -1;
            for v in args {
                if let mlua::Value::Integer(i) = v {
                    acc &= i;
                } else if let mlua::Value::Number(n) = v {
                    acc &= n as i64;
                }
            }
            Ok(acc)
        })?,
    )?;
    bit_tbl.set("bnot", lua.create_function(|_, v: i64| Ok(!v))?)?;
    bit_tbl.set(
        "bxor",
        lua.create_function(|_, args: mlua::MultiValue| {
            let mut acc: i64 = 0;
            for v in args {
                if let mlua::Value::Integer(i) = v {
                    acc ^= i;
                }
            }
            Ok(acc)
        })?,
    )?;
    bit_tbl.set(
        "lshift",
        lua.create_function(|_, (v, shift): (i64, i64)| Ok(v << shift))?,
    )?;
    bit_tbl.set(
        "rshift",
        lua.create_function(|_, (v, shift): (i64, i64)| Ok(v >> shift))?,
    )?;
    globals.set("bit", bit_tbl)?;

    Ok(lua)
}
