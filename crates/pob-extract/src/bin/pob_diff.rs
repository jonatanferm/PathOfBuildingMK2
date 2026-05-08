//! Live PoB validation harness — drives PoB's own Lua codebase under mlua.
//!
//! Strategy: we recreate enough of LuaJIT's host shims (SimpleGraphic, `jit`, `bit`,
//! `unpack`, package.preload for the C libraries PoB pulls in) inside a Lua 5.4 VM,
//! chdir into PoB's `src/` and dofile HeadlessWrapper.lua. HeadlessWrapper itself
//! sets up the SimpleGraphic stubs in their canonical form and dofile's Launch.lua,
//! which runs `runCallback("OnInit")`/`OnFrame`. After that point, PoB's full module
//! graph is loaded and `mainObject.main` exposes the Build/Calcs entry points.
//!
//! Once we can boot, the planned next step is:
//!   - feed a build code via `main:LoadBuildFromXML(...)` (or LoadBuildFromText for
//!     the export string)
//!   - call `calcs.buildOutput(env, "MAIN")`
//!   - walk env.player.output and diff against pob-engine's `compute_full` output
//!
//! Run skeleton:
//!     cargo run -p pob-extract --bin pob_diff -- --pob ../PathOfBuilding

#[allow(unused_imports)]
use anyhow::{Context, Result};
use mlua::Lua;
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut pob_root = PathBuf::from("../PathOfBuilding");
    let mut build_xml_path: Option<PathBuf> = None;
    let mut verbose = false;
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--pob" => pob_root = iter.next().expect("--pob requires arg").into(),
            "--build" => build_xml_path = Some(iter.next().expect("--build requires arg").into()),
            "--verbose" | "-v" => verbose = true,
            "--help" | "-h" => {
                println!(
                    "Usage: pob_diff [--pob PATH] [--build XML_FILE] [--verbose]\n\
                     \n\
                     Boots PoB's Lua codebase under mlua, optionally loads a build XML, and\n\
                     prints player.output. Used to validate pob-engine against the canonical\n\
                     PoB calc pipeline."
                );
                return Ok(());
            }
            other => eprintln!("ignoring unknown arg: {other}"),
        }
    }
    let pob_src = pob_root.join("src");
    if !pob_src.is_dir() {
        anyhow::bail!("PoB src dir not found at {}", pob_src.display());
    }
    let pob_runtime_lua = pob_root.join("runtime/lua");

    // Read the build XML (if provided) BEFORE chdir, then chdir for PoB's
    // relative loadfile calls.
    let build_xml = build_xml_path
        .as_ref()
        .map(|p| std::fs::read_to_string(p).with_context(|| format!("read {}", p.display())))
        .transpose()?
        .unwrap_or_else(default_witch_xml);

    let lua = build_lua_sandbox(&pob_runtime_lua)?;
    std::env::set_current_dir(&pob_src)
        .with_context(|| format!("chdir to {}", pob_src.display()))?;

    boot_pob(&lua, &build_xml, verbose)?;

    Ok(())
}

fn default_witch_xml() -> String {
    String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<PathOfBuilding>
  <Build level="90" targetVersion="3_0" pantheonMajorGod="None" pantheonMinorGod="None" className="Witch" ascendClassName="None" mainSocketGroup="1" viewMode="TREE">
  </Build>
  <Items/>
  <Skills sortGemsByDPSField="CombinedDPS" sortGemsByDPS="true" defaultGemQuality="0" defaultGemLevel="normalMaximum" showSupportGemTypes="ALL" showAltQualityGems="false"/>
  <Tree activeSpec="1">
    <Spec title="Default" classId="3" ascendClassId="0" treeVersion="3_28" masteryEffects="" nodes="">
      <Sockets/>
    </Spec>
  </Tree>
  <Notes/>
  <TreeView searchStr="" zoomY="0" showHeatMap="false" zoomLevel="2" showStatDifferences="true" zoomX="0"/>
  <Calcs>
    <Input name="enemyIsBoss" string="None"/>
  </Calcs>
  <Config/>
</PathOfBuilding>
"#,
    )
}

/// Strip PoB's SimpleGraphic shebang lines (`#@ ...`) which Lua treats as a
/// syntax error. They're only meaningful when SimpleGraphic loads the file.
fn strip_pob_shebangs(src: &str) -> String {
    src.lines()
        .map(|l| if l.trim_start().starts_with("#@") { "" } else { l })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Run a snippet under pcall, printing OK / err — keeps the harness alive across
/// individual module failures so we can see all the gaps in one run.
fn run(lua: &Lua, name: &str, src: &str) {
    let cleaned = strip_pob_shebangs(src);
    let wrapped = format!(
        "local ok, err = pcall(function()\n{cleaned}\nend)\nif not ok then return tostring(err) else return 'OK' end"
    );
    match lua.load(&wrapped).set_name(name).eval::<String>() {
        Ok(s) => println!("[{name}] {s}"),
        Err(e) => println!("[{name}] eval error: {e}"),
    }
}

fn boot_pob(lua: &Lua, build_xml: &str, verbose: bool) -> Result<()> {
    // Step 1: load HeadlessWrapper.lua but strip the `dofile("Launch.lua")` tail —
    // we want to drive Launch ourselves to skip the manifest XML / update-check
    // bits that need a real disk layout.
    let hw_path = std::path::Path::new("HeadlessWrapper.lua");
    let hw_src = std::fs::read_to_string(hw_path)
        .with_context(|| format!("read {}", hw_path.display()))?;
    // HeadlessWrapper ends with a `dofile("Launch.lua")` and an `io.read("*l")`
    // prompt. Cut everything from `dofile("Launch.lua")` onward.
    let hw_trimmed = match hw_src.find("dofile(\"Launch.lua\")") {
        Some(idx) => &hw_src[..idx],
        None => &hw_src,
    };
    run(lua, "HeadlessWrapper.lua", hw_trimmed);

    // Step 2: dofile Launch.lua. Launch sets up `launch` and calls runCallback chain.
    // Then call OnInit by hand because HeadlessWrapper's own runCallback("OnInit")
    // is the bit we cut.
    let launch_src = std::fs::read_to_string("Launch.lua").context("read Launch.lua")?;
    run(lua, "Launch.lua", &launch_src);

    // Step 3: trigger init + first frame the way HeadlessWrapper does. Each is run
    // in its own pcall so we see partial successes.
    run(lua, "OnInit", "runCallback('OnInit')");
    run(lua, "OnFrame", "runCallback('OnFrame')");

    // Boot status:
    let prompt: String = lua
        .load("return tostring(launch and launch.promptMsg)")
        .eval()
        .unwrap_or_else(|_| "<eval failed>".into());
    println!("[boot] launch.promptMsg = {prompt}");

    // Stash the build XML in a Lua global so the loader script can pick it up.
    lua.globals()
        .set("__POB_DIFF_BUILD_XML__", build_xml)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    // Step 5: drive a build through PoB.
    run(
        lua,
        "load_build",
        r#"
local m = launch.main
if not m then error('launch.main not loaded') end
m:SetMode('BUILD', false, 'pob_diff', __POB_DIFF_BUILD_XML__, false, nil)
runCallback('OnFrame')
if launch.promptMsg then
    error('build load failed: '..tostring(launch.promptMsg))
end
"#,
    );

    // Probe build state to verify class registration and tree alloc count.
    if verbose {
        run(
            lua,
            "build_probe",
            r#"
local m = launch.main
local b = m.modes['BUILD']
print(string.format('  build.className=%s ascendClassName=%s level=%s',
    tostring(b.className), tostring(b.ascendClassName), tostring(b.characterLevel)))
print(string.format('  build.spec.classId=%s ascendClassId=%s',
    tostring(b.spec and b.spec.classId), tostring(b.spec and b.spec.ascendClassId)))
print(string.format('  build.spec.treeVersion=%s', tostring(b.spec and b.spec.treeVersion)))
local env = b.calcsTab and b.calcsTab.mainEnv
if env and env.classId then
    print(string.format('  env.classId=%s', tostring(env.classId)))
end
if env and env.player and env.player.modDB then
    print(string.format('  player.modDB:Sum BASE Str = %s', tostring(env.player.modDB:Sum('BASE', nil, 'Str'))))
    print(string.format('  player.modDB:Sum BASE Int = %s', tostring(env.player.modDB:Sum('BASE', nil, 'Int'))))
end
if b.spec and b.spec.allocNodes then
    local n = 0
    for _ in pairs(b.spec.allocNodes) do n = n + 1 end
    print(string.format('  spec.allocNodes count=%d', n))
end
if b.spec and b.spec.nodes then
    local n = 0
    for _ in pairs(b.spec.nodes) do n = n + 1 end
    print(string.format('  spec.nodes table size=%d', n))
end
"#,
        );
    }

    // Step 6: pull env.player.output back into Rust as a sorted scalar map.
    let scalars: mlua::Table = lua
        .load(
            r#"
local m = launch.main
local build = m.modes['BUILD']
local env = build.calcsTab and (build.calcsTab.mainEnv or build.calcsTab.calcsEnv)
if not env or not env.player or not env.player.output then return {} end
local out, dst = env.player.output, {}
for k, v in pairs(out) do
    local t = type(v)
    if t == 'number' or t == 'string' or t == 'boolean' then
        dst[k] = v
    end
end
return dst
"#,
        )
        .eval()
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let mut entries: Vec<(String, mlua::Value)> = Vec::new();
    for pair in scalars.pairs::<String, mlua::Value>() {
        if let Ok((k, v)) = pair {
            entries.push((k, v));
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    println!("\n=== PoB env.player.output ({} scalar keys) ===", entries.len());
    let limit = if verbose { entries.len() } else { 80 };
    for (k, v) in entries.iter().take(limit) {
        let s = format_value(v);
        println!("  {k:<32} = {s}");
    }
    if !verbose && entries.len() > limit {
        println!("  … ({} more — pass --verbose to see all)", entries.len() - limit);
    }

    // Step 7: also run pob-engine on a comparable input and print a side-by-side
    // diff for the keys both engines emit. The XML→CharacterState bridge is a
    // simplified mapping for now (class+level only); items/tree allocs are TODO.
    diff_against_pob_engine(&entries, build_xml, verbose)?;

    Ok(())
}

fn diff_against_pob_engine(
    pob_entries: &[(String, mlua::Value)],
    build_xml: &str,
    verbose: bool,
) -> Result<()> {
    // Use pob-engine's PoB XML importer so the character we feed into compute_full
    // mirrors what PoB itself constructed from the same XML — class, level,
    // ascendancy, allocated nodes, notes are all picked up.
    let character = pob_engine::import_pob_xml(build_xml)
        .map_err(|e| anyhow::anyhow!("import_pob_xml: {e}"))?;
    let class = character.class.0.to_string();
    let level = character.level;
    let allocated = character.allocated.len();

    let tree = load_default_tree().context("load 3_28 tree fixture for diff")?;
    let output = pob_engine::perform::compute_with_skills(&character, &tree, None);

    println!(
        "\n=== diff: pob-engine vs PoB (class={class}, level={level}, allocated={allocated}) ==="
    );
    println!("{:<32}  {:>14}  {:>14}  {:>14}", "key", "pob-engine", "pob (lua)", "delta");
    println!("{:-<78}", "");

    let lookup: std::collections::HashMap<&str, &mlua::Value> = pob_entries
        .iter()
        .map(|(k, v)| (k.as_str(), v))
        .collect();

    // Each row: (display name, pob-engine key, PoB key — same if no rename).
    // Some stats use different naming conventions across the two engines, so
    // we keep an explicit alias table for the well-known cases.
    let probe_keys: &[(&str, &str, &str)] = &[
        ("Life",                 "Life",                 "Life"),
        ("LifeUnreserved",       "LifeUnreserved",       "LifeUnreserved"),
        ("Mana",                 "Mana",                 "Mana"),
        ("ManaUnreserved",       "ManaUnreserved",       "ManaUnreserved"),
        ("EnergyShield",         "EnergyShield",         "EnergyShield"),
        ("Ward",                 "Ward",                 "Ward"),
        ("Strength",             "Strength",             "Str"),
        ("Dexterity",            "Dexterity",            "Dex"),
        ("Intelligence",         "Intelligence",         "Int"),
        ("Armour",               "Armour",               "Armour"),
        ("Evasion",              "Evasion",              "Evasion"),
        ("BlockChance",          "BlockChance",          "BlockChance"),
        ("BlockChanceMax",       "BlockChanceMax",       "BlockChanceMax"),
        ("SpellBlockChance",     "SpellBlockChance",     "SpellBlockChance"),
        ("FireResist",           "FireResist",           "FireResist"),
        ("FireResistTotal",      "FireResistTotal",      "FireResistTotal"),
        ("ColdResist",           "ColdResist",           "ColdResist"),
        ("ColdResistTotal",      "ColdResistTotal",      "ColdResistTotal"),
        ("LightningResist",      "LightningResist",      "LightningResist"),
        ("LightningResistTotal", "LightningResistTotal", "LightningResistTotal"),
        ("ChaosResist",          "ChaosResist",          "ChaosResist"),
        ("ChaosResistTotal",     "ChaosResistTotal",     "ChaosResistTotal"),
        ("LifeRegen",            "LifeRegen",            "LifeRegen"),
        ("ManaRegen",            "ManaRegen",            "ManaRegen"),
        ("EnergyShieldRegen",    "EnergyShieldRegen",    "EnergyShieldRegen"),
        ("MovementSpeedMod",     "MovementSpeedMod",     "MovementSpeedMod"),
    ];
    let mut shown = 0;
    let mut diverge = 0;
    for (display, our_key, pob_key) in probe_keys {
        let our = output.try_get(our_key);
        let pob = lookup.get(*pob_key).and_then(|v| match v {
            mlua::Value::Number(n) => Some(*n),
            mlua::Value::Integer(i) => Some(*i as f64),
            mlua::Value::Boolean(true) => Some(1.0),
            mlua::Value::Boolean(false) => Some(0.0),
            _ => None,
        });
        let key = display;
        match (our, pob) {
            (Some(a), Some(b)) => {
                let delta = a - b;
                let marker = if (delta).abs() < 0.5 { " " } else { "*" };
                println!("{:<32}{marker} {:>14.2}  {:>14.2}  {:>+14.2}", key, a, b, delta);
                shown += 1;
                if (delta).abs() >= 0.5 { diverge += 1; }
            }
            (Some(a), None) => {
                println!("{:<32}  {:>14.2}  {:>14}  {:>14}", key, a, "—", "(only ours)");
            }
            (None, Some(b)) => {
                println!("{:<32}  {:>14}  {:>14.2}  {:>14}", key, "—", b, "(only PoB)");
            }
            (None, None) => {}
        }
    }
    println!("{:-<78}", "");
    println!("  {shown} keys compared, {diverge} divergent (>=0.5 absolute delta)");

    // Auto-divergence pass: walk every numeric scalar that pob-engine emits and
    // also has a same-named entry in PoB's output, and report the deltas. This
    // surfaces engine bugs that the curated probe table doesn't cover.
    let mut auto_div: Vec<(String, f64, f64)> = Vec::new();
    let mut auto_match = 0usize;
    let our_keys: std::collections::HashSet<&str> =
        output.iter().map(|(k, _)| k).collect();
    for (name, ours) in output.iter() {
        // Only reported if pob-engine value is non-trivial OR PoB value is non-trivial,
        // so we don't drown in zeros.
        let theirs = lookup.get(name).and_then(|v| match v {
            mlua::Value::Number(n) => Some(*n),
            mlua::Value::Integer(i) => Some(*i as f64),
            mlua::Value::Boolean(true) => Some(1.0),
            mlua::Value::Boolean(false) => Some(0.0),
            _ => None,
        });
        let Some(theirs) = theirs else { continue };
        let delta = ours - theirs;
        if delta.abs() < 0.5 {
            auto_match += 1;
        } else if ours.abs() > 0.5 || theirs.abs() > 0.5 {
            auto_div.push((name.to_string(), ours, theirs));
        }
    }

    // Coverage gap pass: PoB emits a key with a non-trivial numeric value, and
    // pob-engine doesn't expose that key at all. This is a roadmap of stats we
    // could plumb through.
    let mut missing_outputs: Vec<(String, f64)> = Vec::new();
    for (name, value) in pob_entries.iter() {
        if our_keys.contains(name.as_str()) {
            continue;
        }
        let n = match value {
            mlua::Value::Number(n) => *n,
            mlua::Value::Integer(i) => *i as f64,
            _ => continue,
        };
        if n.abs() >= 0.5 && n.is_finite() {
            missing_outputs.push((name.clone(), n));
        }
    }
    missing_outputs.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()));
    auto_div.sort_by(|a, b| (b.1 - b.2).abs().total_cmp(&(a.1 - a.2).abs()));
    let auto_total = auto_match + auto_div.len();
    println!("\n=== auto-divergence ({auto_total} shared keys, {} divergent) ===", auto_div.len());
    for (name, ours, theirs) in auto_div.iter().take(40) {
        println!(
            "  {name:<32}  ours={ours:>12.2}  pob={theirs:>12.2}  delta={:>+12.2}",
            ours - theirs
        );
    }
    if auto_div.len() > 40 {
        println!("  … ({} more divergent — pass --verbose to see all)", auto_div.len() - 40);
    }

    println!(
        "\n=== coverage gaps ({} non-trivial PoB keys not emitted by pob-engine) ===",
        missing_outputs.len()
    );
    let coverage_limit = if !verbose { 30 } else { missing_outputs.len() };
    for (name, value) in missing_outputs.iter().take(coverage_limit) {
        println!("  {name:<40}  pob={value:>12.2}");
    }
    if missing_outputs.len() > coverage_limit {
        println!(
            "  … ({} more — pass --verbose to see the full roadmap)",
            missing_outputs.len() - coverage_limit
        );
    }

    println!("\nXML→CharacterState bridge uses pob-engine's import_pob_xml: class,");
    println!("level, ascendancy, and allocated tree nodes are wired. Items / skills /");
    println!("config inputs are still empty (the upstream XML encodes them as nested");
    println!("elements that need full document traversal — Phase 5 follow-up).");
    Ok(())
}

fn load_default_tree() -> Result<pob_data::PassiveTree> {
    // Resolve relative to CARGO_MANIFEST_DIR so this works regardless of cwd
    // (we chdir'd into PoB's src/ for the Lua boot path).
    let candidates = [
        // ../.. from this crate gets to repo root, then data/trees/...
        format!("{}/../../data/trees/3_28.json", env!("CARGO_MANIFEST_DIR")),
        format!("{}/../../data/trees/3_25.json", env!("CARGO_MANIFEST_DIR")),
    ];
    for path in &candidates {
        if let Ok(json) = std::fs::read_to_string(path) {
            return pob_data::load_passive_tree(&json)
                .map_err(|e| anyhow::anyhow!("parse tree {path}: {e}"));
        }
    }
    anyhow::bail!("no tree fixture found in data/trees/")
}


fn build_lua_sandbox(pob_runtime_lua: &std::path::Path) -> Result<Lua> {
    let lua = Lua::new();
    let globals = lua.globals();

    // -- LuaJIT compatibility shims ------------------------------------------------

    // `jit` global (Launch.lua calls jit.opt.start). Provide a no-op opt.start.
    let jit_tbl = lua.create_table()?;
    let jit_opt = lua.create_table()?;
    jit_opt.set("start", lua.create_function(|_, _: mlua::MultiValue| Ok(()))?)?;
    jit_tbl.set("opt", jit_opt)?;
    jit_tbl.set("on", lua.create_function(|_, _: mlua::MultiValue| Ok(()))?)?;
    jit_tbl.set("off", lua.create_function(|_, _: mlua::MultiValue| Ok(()))?)?;
    jit_tbl.set("status", lua.create_function(|_, ()| Ok(false))?)?;
    jit_tbl.set("version", "LuaJIT 2.1.0 (stub)")?;
    globals.set("jit", jit_tbl)?;

    // `unpack` (LuaJIT keeps it global; Lua 5.4 only has table.unpack).
    let table_tbl: mlua::Table = globals.get("table")?;
    if let Ok(unpack) = table_tbl.get::<mlua::Function>("unpack") {
        globals.set("unpack", unpack)?;
    }

    // `loadstring` alias for load (LuaJIT compat).
    let load_fn: mlua::Function = globals.get("load")?;
    globals.set("loadstring", load_fn)?;

    // `arg` global — Modules/Main.lua reads `arg[1]` to handle command-line build
    // imports. We supply an empty table so the `if arg[1] then` branch falls through.
    globals.set("arg", lua.create_table()?)?;

    // bit library (LuaJIT-style). Lua 5.4 has builtin operators but PoB calls
    // `bit.bor()` etc. directly.
    install_bit(&lua, &globals)?;

    // -- package.preload stubs for native libs PoB requires ------------------------

    let xml_path = std::fs::canonicalize(pob_runtime_lua.join("xml.lua"))
        .with_context(|| format!("canonicalize {}/xml.lua", pob_runtime_lua.display()))?;
    let xml_path_lua = xml_path.to_string_lossy().replace('\\', "/");
    let preload_src_template = r#"
local function make_stub()
    return setmetatable({}, {__index = function() return function() end end})
end
local stub = make_stub()
for _, name in ipairs({
    'lcurl', 'lcurl.safe', 'sha2', 'sha1', 'lfs', 'socket',
    'cjson', 're', 'utf8data', 'lua-profiler',
}) do
    package.preload[name] = function() return stub end
end
-- lua-utf8 — provide functional fallbacks (Lua 5.4 has builtin `utf8` library
-- which has most of these). PoB calls len/reverse/gsub/find/sub/char.
package.preload['lua-utf8'] = function()
    local builtin = utf8 or {}
    local m = {}
    m.len = builtin.len or function(s) return #s end
    m.char = builtin.char or string.char
    m.offset = builtin.offset
    m.codepoint = builtin.codepoint
    m.charpattern = builtin.charpattern or '.'
    m.reverse = function(s) return string.reverse(s) end
    m.gsub = function(s, pat, repl, n) if n then return string.gsub(s, pat, repl, n) end return string.gsub(s, pat, repl) end
    m.find = function(s, pat, init, plain) return string.find(s, pat, init, plain) end
    m.sub = function(s, i, j) return string.sub(s, i, j or -1) end
    m.match = function(s, pat, init) return string.match(s, pat, init) end
    m.upper = function(s) return string.upper(s) end
    m.lower = function(s) return string.lower(s) end
    return m
end
-- dkjson — provide encode/decode that's safe for `require "dkjson"` to succeed.
-- We don't actually need real JSON for the calc-diff harness, only for trade
-- query features that we won't drive.
package.preload['dkjson'] = function()
    return {
        encode = function(t) return tostring(t) end,
        decode = function(s) return {}, nil, nil end,
        null = setmetatable({}, {__tostring = function() return 'null' end}),
    }
end
-- xml — load PoB's own pure-Lua implementation from runtime/lua/xml.lua so
-- ParseXML returns the proper {tree} structure instead of an empty stub.
package.preload['xml'] = function()
    local f, err = loadfile(__POB_XML_LUA_PATH__)
    if not f then error('failed to load xml.lua: '..tostring(err)) end
    return f()
end
package.preload['base64'] = function()
    return {
        encode = function(s) return s end,
        decode = function(s) return s end,
    }
end
return true
"#;
    let preload_src =
        preload_src_template.replace("__POB_XML_LUA_PATH__", &format!("'{xml_path_lua}'"));
    let _: bool = lua
        .load(&preload_src)
        .eval()
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    // -- Lua 5.4 ↔ LuaJIT compatibility patch: lenient string.gsub replacement.
    // PoB's source contains gsub calls like value:gsub(pat, "%(...%)") where the
    // bare `%` is a literal in LuaJIT/5.1 but a syntax error in Lua 5.4 (only `%%`
    // and `%0`-`%9` are valid escapes there). We wrap string.gsub with a Lua
    // shim that rewrites the replacement string.
    // Lua 5.4 ↔ LuaJIT compat: string.format("%d", floatVal) errors in 5.4 because
    // floats with no integer representation are rejected. LuaJIT silently truncated.
    // Wrap string.format to coerce numeric args for %d / %i.
    let format_patch = r#"
local raw_format = string.format
local function coerce_int(v)
    if type(v) == 'number' then
        if v ~= v then return 0 end                  -- NaN
        if v == math.huge then return 2147483647 end
        if v == -math.huge then return -2147483648 end
        local i = math.tointeger(v)
        if i then return i end
        return math.tointeger(math.floor(v)) or 0
    elseif type(v) == 'string' then
        local n = tonumber(v)
        if n then return coerce_int(n) end
    end
    return v
end
string.format = function(fmt, ...)
    if type(fmt) ~= 'string' then return raw_format(fmt, ...) end
    local n_args = select('#', ...)
    local args = {...}
    local i = 0
    local j, n = 1, #fmt
    while j <= n do
        if fmt:sub(j, j) == '%' then
            j = j + 1
            if j > n then break end
            if fmt:sub(j, j) == '%' then
                j = j + 1
            else
                while j <= n and fmt:sub(j, j):match('[%-+ #0]') do j = j + 1 end
                while j <= n and fmt:sub(j, j):match('%d') do j = j + 1 end
                if j <= n and fmt:sub(j, j) == '.' then
                    j = j + 1
                    while j <= n and fmt:sub(j, j):match('%d') do j = j + 1 end
                end
                if j > n then break end
                local conv = fmt:sub(j, j)
                i = i + 1
                if conv == 'd' or conv == 'i' or conv == 'x' or conv == 'X'
                    or conv == 'o' or conv == 'u' then
                    args[i] = coerce_int(args[i])
                elseif conv == 's' then
                    -- LuaJIT silently coerces numbers/booleans to strings; 5.4 too,
                    -- but tables would error — leave as-is.
                end
                j = j + 1
            end
        else
            j = j + 1
        end
    end
    -- Use unpack with explicit count so trailing nils are preserved.
    local ok, res = pcall(raw_format, fmt, table.unpack(args, 1, math.max(n_args, i)))
    if not ok then
        -- Fallback: aggressively coerce every numeric arg to integer.
        for k = 1, math.max(n_args, i) do
            args[k] = coerce_int(args[k])
        end
        return raw_format(fmt, table.unpack(args, 1, math.max(n_args, i)))
    end
    return res
end
return true
"#;
    let _: bool = lua
        .load(format_patch)
        .eval()
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let gsub_patch = r#"
local raw_gsub = string.gsub
local function escape_replacement(repl)
    if type(repl) ~= 'string' then return repl end
    -- Walk char by char, escaping any '%' not followed by digit or '%'.
    local out, i, n = {}, 1, #repl
    while i <= n do
        local c = repl:sub(i, i)
        if c == '%' then
            if i == n then
                out[#out+1] = '%%'
                i = i + 1
            else
                local nxt = repl:sub(i+1, i+1)
                if nxt:match('[%%0-9]') then
                    out[#out+1] = c
                    out[#out+1] = nxt
                    i = i + 2
                else
                    out[#out+1] = '%%'
                    out[#out+1] = nxt
                    i = i + 2
                end
            end
        else
            out[#out+1] = c
            i = i + 1
        end
    end
    return table.concat(out)
end
string.gsub = function(s, pat, repl, n)
    repl = escape_replacement(repl)
    if n then return raw_gsub(s, pat, repl, n) end
    return raw_gsub(s, pat, repl)
end
-- Method form (s:gsub) goes through string metatable's __index = string, so
-- patching string.gsub is enough.
return true
"#;
    let _: bool = lua
        .load(gsub_patch)
        .eval()
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    // -- Output redirection: tee ConPrintf/print to a prefix --------------------

    let prefixed_print = lua.create_function(|_, args: mlua::MultiValue| {
        let parts: Vec<String> = args
            .iter()
            .map(|v| match v {
                mlua::Value::String(s) => s.to_string_lossy().to_string(),
                v => format!("{v:?}"),
            })
            .collect();
        println!("[lua] {}", parts.join("\t"));
        Ok(())
    })?;
    globals.set("print", prefixed_print)?;

    Ok(lua)
}

fn install_bit(lua: &Lua, globals: &mlua::Table) -> Result<()> {
    let bit_tbl = lua.create_table()?;
    bit_tbl.set(
        "bor",
        lua.create_function(|_, args: mlua::MultiValue| {
            let mut acc: i64 = 0;
            for v in args {
                acc |= to_int(&v);
            }
            Ok(acc)
        })?,
    )?;
    bit_tbl.set(
        "band",
        lua.create_function(|_, args: mlua::MultiValue| {
            let mut acc: i64 = -1;
            for v in args {
                acc &= to_int(&v);
            }
            Ok(acc)
        })?,
    )?;
    bit_tbl.set(
        "bxor",
        lua.create_function(|_, args: mlua::MultiValue| {
            let mut acc: i64 = 0;
            for v in args {
                acc ^= to_int(&v);
            }
            Ok(acc)
        })?,
    )?;
    bit_tbl.set("bnot", lua.create_function(|_, v: i64| Ok(!v))?)?;
    bit_tbl.set(
        "lshift",
        lua.create_function(|_, (v, s): (i64, i64)| Ok(v << s))?,
    )?;
    bit_tbl.set(
        "rshift",
        lua.create_function(|_, (v, s): (i64, i64)| Ok((v as u64 >> s) as i64))?,
    )?;
    bit_tbl.set(
        "arshift",
        lua.create_function(|_, (v, s): (i64, i64)| Ok(v >> s))?,
    )?;
    bit_tbl.set(
        "tobit",
        lua.create_function(|_, v: i64| Ok((v as i32) as i64))?,
    )?;
    globals.set("bit", bit_tbl)?;
    Ok(())
}

fn format_value(v: &mlua::Value) -> String {
    match v {
        mlua::Value::Number(n) => {
            if n.fract() == 0.0 && n.abs() < 1e15 {
                format!("{:.0}", n)
            } else {
                format!("{:.4}", n)
            }
        }
        mlua::Value::Integer(i) => i.to_string(),
        mlua::Value::Boolean(b) => b.to_string(),
        mlua::Value::String(s) => s.to_string_lossy().to_string(),
        other => format!("{other:?}"),
    }
}

fn to_int(v: &mlua::Value) -> i64 {
    match v {
        mlua::Value::Integer(i) => *i,
        mlua::Value::Number(n) => *n as i64,
        mlua::Value::Boolean(true) => 1,
        _ => 0,
    }
}
