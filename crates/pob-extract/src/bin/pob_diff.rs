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

/// Find the upstream PoB checkout. Preferred layout: `.PathOfBuilding/`
/// inside the MK2 repo root (hidden subdir, gitignored). Falls back to the
/// legacy sibling layout (`../PathOfBuilding/`) so older clones still work.
fn default_pob_path() -> PathBuf {
    let local = PathBuf::from(".PathOfBuilding");
    if local.join("src").is_dir() {
        return local;
    }
    PathBuf::from("../PathOfBuilding")
}

fn main() -> Result<()> {
    let mut pob_root = default_pob_path();
    let mut build_paths: Vec<PathBuf> = Vec::new();
    let mut json_out: Option<PathBuf> = None;
    let mut explain: Vec<String> = Vec::new();
    let mut dump_env: Option<PathBuf> = None;
    let mut verbose = false;
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--pob" => pob_root = iter.next().expect("--pob requires arg").into(),
            "--build" => build_paths.push(iter.next().expect("--build requires arg").into()),
            "--json-out" => json_out = Some(iter.next().expect("--json-out requires arg").into()),
            "--explain" => explain.extend(
                iter.next()
                    .expect("--explain requires STAT[,STAT…]")
                    .split(',')
                    .map(str::to_owned),
            ),
            "--dump-modparser-env" => {
                dump_env = Some(
                    iter.next()
                        .expect("--dump-modparser-env requires PATH")
                        .into(),
                );
            }
            "--verbose" | "-v" => verbose = true,
            "--help" | "-h" => {
                println!(
                    "Usage: pob_diff [--pob PATH] [--build XML_FILE]... [--json-out DIR] [--verbose]\n\
                     \n\
                     Boots PoB's Lua codebase under mlua once, loads each build XML, and\n\
                     diffs player.output against pob-engine. With --json-out, writes\n\
                     <DIR>/<build stem>.json holding both output maps per build."
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

    // Read the builds (and resolve output paths) BEFORE chdir, then chdir
    // for PoB's relative loadfile calls.
    let mut builds: Vec<(String, String)> = Vec::new();
    for p in &build_paths {
        let xml = std::fs::read_to_string(p).with_context(|| format!("read {}", p.display()))?;
        let stem = p
            .file_stem()
            .map_or_else(|| "build".into(), |s| s.to_string_lossy().into_owned());
        builds.push((stem, xml));
    }
    if builds.is_empty() {
        builds.push(("default".into(), default_witch_xml()));
    }
    let json_out = json_out
        .map(|d| {
            std::fs::create_dir_all(&d)?;
            std::fs::canonicalize(&d)
        })
        .transpose()?;
    let game_data = pob_data::disk::GameData::load(&PathBuf::from(format!(
        "{}/../../data",
        env!("CARGO_MANIFEST_DIR")
    )));

    let dump_env = dump_env
        .map(|p| -> Result<PathBuf> {
            let abs = if p.is_absolute() {
                p
            } else {
                std::env::current_dir()?.join(p)
            };
            Ok(abs)
        })
        .transpose()?;
    let lua = build_lua_sandbox(&pob_runtime_lua)?;
    std::env::set_current_dir(&pob_src)
        .with_context(|| format!("chdir to {}", pob_src.display()))?;

    boot_pob(&lua)?;
    if let Some(path) = dump_env {
        let text: String = lua
            .load(DUMP_MODPARSER_ENV)
            .set_name("dump_modparser_env")
            .eval()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        std::fs::write(&path, text).with_context(|| format!("write {}", path.display()))?;
        println!("wrote {}", path.display());
        return Ok(());
    }
    let multi = builds.len() > 1;
    for (stem, xml) in &builds {
        if multi {
            println!("\n##### {stem}");
        }
        let pob_entries = match run_build(&lua, xml, verbose) {
            Ok(entries) => entries,
            Err(e) => {
                println!("[{stem}] PoB failed: {e}");
                continue;
            }
        };
        if !explain.is_empty() {
            explain_pob(&lua, &explain);
        }
        if std::env::var("POB_DIFF_TIME").is_ok() {
            run(
                &lua,
                "time_calc",
                r"
local b = launch.main.modes['BUILD']
local t0 = os.clock()
local n = 5
for i = 1, n do b.calcsTab:BuildOutput() end
local t1 = os.clock()
local calcs = b.calcsTab.calcs
local t2 = os.clock()
for i = 1, n do local env = calcs.initEnv(b, 'MAIN'); calcs.perform(env) end
local t3 = os.clock()
error(string.format('BuildOutput avg %.1f ms; initEnv+perform avg %.1f ms', (t1-t0)*1000/n, (t3-t2)*1000/n))
",
            );
        }
        let ours =
            match diff_against_pob_engine(&pob_entries, xml, &game_data, verbose, !multi, &explain)
            {
                Ok(ours) => ours,
                Err(e) => {
                    println!("[{stem}] pob-engine failed: {e}");
                    continue;
                }
            };
        if let Some(dir) = &json_out {
            write_json_report(&dir.join(format!("{stem}.json")), stem, &pob_entries, &ours)?;
        }
    }
    Ok(())
}

/// Lua that serialises the subset of PoB's `data` table `Modules/ModParser.lua`
/// reads while building its pattern tables, as a Lua chunk returning a table.
/// Gems reference their granted effects by skill id so shared tables stay
/// shared when the chunk is re-evaluated.
const DUMP_MODPARSER_ENV: &str = r#"
local out = { "-- Generated by pob_diff --dump-modparser-env from PoB's loaded data. Do not edit.", "local skills = {}" }
local function q(s) return string.format("%q", s) end
local function ser(v, depth)
    depth = depth or 0
    local t = type(v)
    if t == "string" then return q(v)
    elseif t == "number" then
        if v == math.floor(v) and math.abs(v) < 1e15 then return string.format("%d", v) end
        return string.format("%.17g", v)
    elseif t == "boolean" then return tostring(v)
    elseif t == "table" then
        if depth > 6 then return "nil" end
        local parts = {}
        local keys = {}
        for k in pairs(v) do keys[#keys+1] = k end
        table.sort(keys, function(a, b)
            if type(a) == type(b) and (type(a) == "number" or type(a) == "string") then return a < b end
            return type(a) < type(b)
        end)
        for _, k in ipairs(keys) do
            local sv = ser(v[k], depth + 1)
            if sv ~= "nil" then
                local ks = type(k) == "string" and ("[" .. q(k) .. "]") or ("[" .. ser(k) .. "]")
                parts[#parts+1] = ks .. "=" .. sv
            end
        end
        return "{" .. table.concat(parts, ",") .. "}"
    end
    return "nil"
end
local skillFields = { "id", "name", "hidden", "fromItem", "fromTree", "support", "skillTypes", "baseFlags" }
local ids = {}
for id in pairs(data.skills) do ids[#ids+1] = id end
table.sort(ids)
for _, id in ipairs(ids) do
    local s = data.skills[id]
    local t = {}
    for _, f in ipairs(skillFields) do t[f] = s[f] end
    out[#out+1] = "skills[" .. q(id) .. "]=" .. ser(t)
end
out[#out+1] = "local gems = {}"
local gemIds = {}
for id in pairs(data.gems) do gemIds[#gemIds+1] = id end
table.sort(gemIds)
for _, id in ipairs(gemIds) do
    local g = data.gems[id]
    local t = { name = g.name, grantedEffectId = g.grantedEffectId, secondaryGrantedEffectId = g.secondaryGrantedEffectId, tags = g.tags, gameId = g.gameId, variantId = g.variantId }
    local line = "gems[" .. q(id) .. "]=" .. ser(t)
    out[#out+1] = line
    if g.grantedEffectId then out[#out+1] = "gems[" .. q(id) .. "].grantedEffect=skills[" .. q(g.grantedEffectId) .. "]" end
    if g.secondaryGrantedEffectId then out[#out+1] = "gems[" .. q(id) .. "].secondaryGrantedEffect=skills[" .. q(g.secondaryGrantedEffectId) .. "]" end
end
out[#out+1] = "return {"
out[#out+1] = "skills = skills, gems = gems,"
out[#out+1] = "gemForBaseName = " .. ser(data.gemForBaseName) .. ","
out[#out+1] = "keystones = " .. ser(data.keystones) .. ","
local cj = { keystones = data.clusterJewels.keystones, notableSortOrder = data.clusterJewels.notableSortOrder, jewels = {} }
for base, jewel in pairs(data.clusterJewels.jewels) do
    local skillsT = {}
    for skillId, skill in pairs(jewel.skills) do skillsT[skillId] = { enchant = skill.enchant } end
    cj.jewels[base] = { skills = skillsT }
end
out[#out+1] = "clusterJewels = " .. ser(cj) .. ","
out[#out+1] = "nonDamagingAilment = " .. ser(data.nonDamagingAilment) .. ","
out[#out+1] = "nonDamagingAilmentTypeList = " .. ser(data.nonDamagingAilmentTypeList) .. ","
out[#out+1] = "}"
return table.concat(out, "\n")
"#;

/// `{ "build": stem, "pob": {key: value}, "ours": {key: value} }` — the raw
/// material for `parity_report`.
fn write_json_report(
    path: &std::path::Path,
    stem: &str,
    pob_entries: &[(String, mlua::Value)],
    ours: &[(String, f64)],
) -> Result<()> {
    let mut pob = serde_json::Map::new();
    for (k, v) in pob_entries {
        let j = match v {
            mlua::Value::Number(n) if n.is_finite() => serde_json::json!(n),
            mlua::Value::Integer(i) => serde_json::json!(i),
            mlua::Value::Boolean(b) => serde_json::json!(b),
            mlua::Value::String(s) => serde_json::json!(s.to_string_lossy()),
            _ => continue,
        };
        pob.insert(k.clone(), j);
    }
    let ours: serde_json::Map<String, serde_json::Value> = ours
        .iter()
        .filter(|(_, v)| v.is_finite())
        .map(|(k, v)| (k.clone(), serde_json::json!(v)))
        .collect();
    let doc = serde_json::json!({ "build": stem, "pob": pob, "ours": ours });
    std::fs::write(path, serde_json::to_string(&doc)?)
        .with_context(|| format!("write {}", path.display()))
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
        .map(|l| {
            if l.trim_start().starts_with("#@") {
                ""
            } else {
                l
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Run a snippet under pcall, printing OK / err — keeps the harness alive across
/// individual module failures so we can see all the gaps in one run.
fn run(lua: &Lua, name: &str, src: &str) {
    let t0 = std::time::Instant::now();
    run_inner(lua, name, src);
    if std::env::var("POB_DIFF_TIME").is_ok() {
        eprintln!("[time] {name}: {:?}", t0.elapsed());
    }
}

fn run_inner(lua: &Lua, name: &str, src: &str) {
    let cleaned = strip_pob_shebangs(src);
    let wrapped = format!(
        "local ok, err = pcall(function()\n{cleaned}\nend)\nif not ok then return tostring(err) else return 'OK' end"
    );
    match lua.load(&wrapped).set_name(name).eval::<String>() {
        Ok(s) => println!("[{name}] {s}"),
        Err(e) => println!("[{name}] eval error: {e}"),
    }
}

fn boot_pob(lua: &Lua) -> Result<()> {
    // Step 1: load HeadlessWrapper.lua but strip the `dofile("Launch.lua")` tail —
    // we want to drive Launch ourselves to skip the manifest XML / update-check
    // bits that need a real disk layout.
    let hw_path = std::path::Path::new("HeadlessWrapper.lua");
    let hw_src =
        std::fs::read_to_string(hw_path).with_context(|| format!("read {}", hw_path.display()))?;
    // HeadlessWrapper ends with a `dofile("Launch.lua")` and an `io.read("*l")`
    // prompt. Cut everything from `dofile("Launch.lua")` onward.
    let hw_trimmed = match hw_src.find("dofile(\"Launch.lua\")") {
        Some(idx) => &hw_src[..idx],
        None => &hw_src,
    };
    run(lua, "HeadlessWrapper.lua", hw_trimmed);
    install_host_shims(lua)?;

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
    Ok(())
}

/// Load one build into the booted PoB and return `env.player.output`'s scalars.
fn run_build(lua: &Lua, build_xml: &str, verbose: bool) -> Result<Vec<(String, mlua::Value)>> {
    // Clear any error prompt left by a previous build.
    run(lua, "reset_prompt", "launch.promptMsg = nil");

    // Stash the build XML in a Lua global so the loader script can pick it up.
    lua.globals()
        .set("__POB_DIFF_BUILD_XML__", build_xml)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    // Step 5: drive a build through PoB.
    lua.load(
        r"
local m = launch.main
if not m then error('launch.main not loaded') end
m:SetMode('BUILD', false, 'pob_diff', __POB_DIFF_BUILD_XML__, false, nil)
runCallback('OnFrame')
if launch.promptMsg then
    error('build load failed: '..tostring(launch.promptMsg))
end
",
    )
    .set_name("load_build")
    .exec()
    .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    // Probe build state to verify class registration and tree alloc count.
    if verbose {
        run(
            lua,
            "enemy_damage_probe",
            r"
local m = launch.main
local b = m.modes['BUILD']
local env = b.calcsTab and (b.calcsTab.mainEnv or b.calcsTab.calcsEnv)
if env then
    print(string.format('  env.enemyLevel = %s', tostring(env.enemyLevel)))
    if env.configInput then
        for _, k in ipairs({'enemyPhysicalDamage', 'enemyFireDamage', 'enemyChaosDamage', 'enemySpeed', 'enemyIsBoss'}) do
            print(string.format('  configInput[%s] = %s', k, tostring(env.configInput[k])))
        end
    end
    if env.configPlaceholder then
        for _, k in ipairs({'enemyPhysicalDamage', 'enemyFireDamage', 'enemyChaosDamage', 'enemyLevel'}) do
            print(string.format('  configPlaceholder[%s] = %s', k, tostring(env.configPlaceholder[k])))
        end
    end
end
",
        );
        run(
            lua,
            "build_probe",
            r"
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
",
        );
    }

    // Step 6: pull env.player.output back into Rust as a sorted scalar map.
    let scalars: mlua::Table = lua
        .load(
            r"
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
",
        )
        .eval()
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let mut entries: Vec<(String, mlua::Value)> = Vec::new();
    for (k, v) in scalars.pairs::<String, mlua::Value>().flatten() {
        entries.push((k, v));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    if !verbose {
        return Ok(entries);
    }
    println!(
        "\n=== PoB env.player.output ({} scalar keys) ===",
        entries.len()
    );
    let limit = entries.len();
    for (k, v) in entries.iter().take(limit) {
        let s = format_value(v);
        println!("  {k:<32} = {s}");
    }
    if !verbose && entries.len() > limit {
        println!(
            "  … ({} more — pass --verbose to see all)",
            entries.len() - limit
        );
    }

    Ok(entries)
}

fn diff_against_pob_engine(
    pob_entries: &[(String, mlua::Value)],
    build_xml: &str,
    data: &pob_data::disk::GameData,
    verbose: bool,
    detailed: bool,
    explain: &[String],
) -> Result<Vec<(String, f64)>> {
    // Use pob-engine's PoB XML importer so the character we feed into compute_full
    // mirrors what PoB itself constructed from the same XML — class, level,
    // ascendancy, allocated nodes, notes are all picked up.
    let character = pob_engine::import_pob_xml(build_xml)
        .map_err(|e| anyhow::anyhow!("import_pob_xml: {e}"))?;
    let class = character.class.0.clone();
    let level = character.level;
    let allocated = character.allocated.len();

    let tree_version = build_tree_version(build_xml).unwrap_or_else(|| "3_28".into());
    let tree = data
        .load_tree(&tree_version)
        .or_else(|_| data.load_tree("3_28"))
        .map_err(|e| anyhow::anyhow!(e))?;
    let skills = pob_engine::SkillRegistry::from_files(data.skill_sets.clone());
    let mut luts: ahash::HashMap<u32, Vec<u8>> = ahash::HashMap::default();
    for jt in pob_engine::timeless_lut_types_needed(&character) {
        if let Some(lut) = data
            .timeless_lut_compressed(jt)
            .and_then(|c| pob_engine::inflate_timeless_lut(&c).ok())
        {
            luts.insert(jt, lut);
        }
    }
    let calc_data = pob_engine::CalcData {
        skills: Some(&skills),
        bases: data.bases.as_ref(),
        cluster_jewels: data.cluster_jewels.as_ref(),
        cluster_jewel_mods: data.cluster_jewel_mods.as_ref(),
        timeless_jewels: data.timeless_jewels.as_ref(),
        timeless_node_index: data.timeless_node_index.as_ref(),
        legion_pool: data.legion_pool.as_ref(),
        timeless_luts: Some(&luts),
        minions: data.minions.as_ref(),
    };
    let (output, env) = pob_engine::compute_with_data(&character, &tree, &calc_data);
    for name in explain {
        explain_ours(&env, name);
    }
    let ours: Vec<(String, f64)> = output.iter().map(|(k, v)| (k.to_string(), v)).collect();
    if !detailed {
        let lookup: std::collections::HashMap<&str, f64> = pob_entries
            .iter()
            .filter_map(|(k, v)| lua_number(v).map(|n| (k.as_str(), n)))
            .collect();
        let (mut shared, mut div) = (0, 0);
        for (k, v) in &ours {
            if let Some(p) = lookup.get(k.as_str()) {
                shared += 1;
                if !close_enough(*v, *p) {
                    div += 1;
                }
            }
        }
        println!(
            "  class={class} level={level} allocated={allocated}: {shared} shared keys, {div} off by >1%"
        );
        return Ok(ours);
    }

    println!(
        "\n=== diff: pob-engine vs PoB (class={class}, level={level}, allocated={allocated}) ==="
    );
    println!(
        "{:<32}  {:>14}  {:>14}  {:>14}",
        "key", "pob-engine", "pob (lua)", "delta"
    );
    println!("{:-<78}", "");

    let lookup: std::collections::HashMap<&str, &mlua::Value> =
        pob_entries.iter().map(|(k, v)| (k.as_str(), v)).collect();

    // Each row: (display name, pob-engine key, PoB key — same if no rename).
    // Some stats use different naming conventions across the two engines, so
    // we keep an explicit alias table for the well-known cases.
    let probe_keys: &[(&str, &str, &str)] = &[
        ("Life", "Life", "Life"),
        ("LifeUnreserved", "LifeUnreserved", "LifeUnreserved"),
        ("Mana", "Mana", "Mana"),
        ("ManaUnreserved", "ManaUnreserved", "ManaUnreserved"),
        ("EnergyShield", "EnergyShield", "EnergyShield"),
        ("Ward", "Ward", "Ward"),
        ("Strength", "Strength", "Str"),
        ("Dexterity", "Dexterity", "Dex"),
        ("Intelligence", "Intelligence", "Int"),
        ("Armour", "Armour", "Armour"),
        ("Evasion", "Evasion", "Evasion"),
        ("BlockChance", "BlockChance", "BlockChance"),
        ("BlockChanceMax", "BlockChanceMax", "BlockChanceMax"),
        ("SpellBlockChance", "SpellBlockChance", "SpellBlockChance"),
        ("FireResist", "FireResist", "FireResist"),
        ("FireResistTotal", "FireResistTotal", "FireResistTotal"),
        ("ColdResist", "ColdResist", "ColdResist"),
        ("ColdResistTotal", "ColdResistTotal", "ColdResistTotal"),
        ("LightningResist", "LightningResist", "LightningResist"),
        (
            "LightningResistTotal",
            "LightningResistTotal",
            "LightningResistTotal",
        ),
        ("ChaosResist", "ChaosResist", "ChaosResist"),
        ("ChaosResistTotal", "ChaosResistTotal", "ChaosResistTotal"),
        ("LifeRegen", "LifeRegen", "LifeRegen"),
        ("ManaRegen", "ManaRegen", "ManaRegen"),
        (
            "EnergyShieldRegen",
            "EnergyShieldRegen",
            "EnergyShieldRegen",
        ),
        ("MovementSpeedMod", "MovementSpeedMod", "MovementSpeedMod"),
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
                println!(
                    "{:<32}{marker} {:>14.2}  {:>14.2}  {:>+14.2}",
                    key, a, b, delta
                );
                shown += 1;
                if (delta).abs() >= 0.5 {
                    diverge += 1;
                }
            }
            (Some(a), None) => {
                println!(
                    "{:<32}  {:>14.2}  {:>14}  {:>14}",
                    key, a, "—", "(only ours)"
                );
            }
            (None, Some(b)) => {
                println!(
                    "{:<32}  {:>14}  {:>14.2}  {:>14}",
                    key, "—", b, "(only PoB)"
                );
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
    let our_keys: std::collections::HashSet<&str> = output.iter().map(|(k, _)| k).collect();
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
    println!(
        "\n=== auto-divergence ({auto_total} shared keys, {} divergent) ===",
        auto_div.len()
    );
    for (name, ours, theirs) in auto_div.iter().take(40) {
        println!(
            "  {name:<32}  ours={ours:>12.2}  pob={theirs:>12.2}  delta={:>+12.2}",
            ours - theirs
        );
    }
    if auto_div.len() > 40 {
        println!(
            "  … ({} more divergent — pass --verbose to see all)",
            auto_div.len() - 40
        );
    }

    println!(
        "\n=== coverage gaps ({} non-trivial PoB keys not emitted by pob-engine) ===",
        missing_outputs.len()
    );
    let coverage_limit = if verbose { missing_outputs.len() } else { 30 };
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
    println!("level, ascendancy, allocated tree nodes, items, skill groups, and");
    println!("config inputs all flow through. Items use pob-engine's parse_item;");
    println!("the active skill picks the gem at <Build mainSocketGroup=N>'s");
    println!("`mainActiveSkill`. Support gems in the same group buff the active");
    println!("skill via skill_mods + addSkillTypes/excludeSkillTypes filtering.");
    Ok(ours)
}

/// Print PoB's player modDB entries for each stat (`modDB:Tabulate`).
fn explain_pob(lua: &Lua, names: &[String]) {
    for name in names {
        let _ = lua.globals().set("__POB_DIFF_EXPLAIN__", name.as_str());
        let res = lua
            .load(
                r#"
local b = launch.main.modes['BUILD']
local env = b.calcsTab and (b.calcsTab.mainEnv or b.calcsTab.calcsEnv)
local db = env.player.modDB
local rows = db:Tabulate(nil, nil, __POB_DIFF_EXPLAIN__)
table.sort(rows, function(a, b) return tostring(a.mod.source) < tostring(b.mod.source) end)
local out = {}
for _, r in ipairs(rows) do
    local m = r.mod
    local tags = {}
    for _, t in ipairs(m) do
        local parts = {}
        for k, v in pairs(t) do if k ~= 'type' then parts[#parts+1] = k .. '=' .. tostring(v) end end
        table.sort(parts)
        tags[#tags+1] = t.type .. '{' .. table.concat(parts, ',') .. '}'
    end
    out[#out+1] = string.format('    %-5s %10s  %-40s %s flags=%s kw=%s',
        m.type, tostring(r.value), tostring(m.source), table.concat(tags, ' '), tostring(m.flags), tostring(m.keywordFlags))
end
local o = env.player.output
return string.format('%d mods (output %s = %s)\n', #rows, __POB_DIFF_EXPLAIN__, tostring(o[__POB_DIFF_EXPLAIN__])) .. table.concat(out, '\n')
"#,
            )
            .eval::<String>();
        match res {
            Ok(text) => println!("[pob:{name}] {text}"),
            Err(e) => println!("[pob:{name}] error: {e}"),
        }
    }
}

/// Print pob-engine's modDB entries for a stat with each mod's evaluated value.
fn explain_ours(env: &pob_engine::Env, name: &str) {
    use pob_engine::ModStore;
    let mut rows: Vec<String> = env
        .mod_db
        .iter_named(name)
        .map(|m| {
            let v = pob_engine::mod_db::eval_mod(m, &env.state)
                .map_or_else(|| "(inactive)".to_owned(), |v| format!("{v}"));
            format!(
                "    {:<5} {:>10}  {:<40} tags={:?} flags={:?} kw={:?}",
                format!("{:?}", m.kind),
                v,
                m.source
                    .as_ref()
                    .map_or_else(|| "-".to_owned(), |s| format!("{s:?}")),
                m.tags,
                m.flags,
                m.keyword_flags
            )
        })
        .collect();
    rows.sort();
    println!(
        "[ours:{name}] {} mods (output {name} = {:?})\n{}",
        rows.len(),
        env.output.try_get(name),
        rows.join("\n")
    );
}

fn lua_number(v: &mlua::Value) -> Option<f64> {
    match v {
        mlua::Value::Number(n) => Some(*n),
        mlua::Value::Integer(i) => Some(*i as f64),
        mlua::Value::Boolean(b) => Some(f64::from(u8::from(*b))),
        _ => None,
    }
}

/// Parity tolerance: within 1% (relative) or 0.5 absolute.
fn close_enough(a: f64, b: f64) -> bool {
    let d = (a - b).abs();
    d < 0.5 || d <= 0.01 * a.abs().max(b.abs())
}

/// `<Spec treeVersion="3_13">` of the build's active spec (first one found).
fn build_tree_version(build_xml: &str) -> Option<String> {
    let start = build_xml.find("treeVersion=\"")? + "treeVersion=\"".len();
    let len = build_xml[start..].find('"')?;
    Some(build_xml[start..start + len].to_string())
}

/// Replace HeadlessWrapper's inert SimpleGraphic stubs with working ones where
/// the calc depends on them: `Inflate` + `NewFileSearch` let PoB load the
/// timeless-jewel LUTs (otherwise every Timeless jewel is silently inert on
/// the PoB side), and `io.open` accepts LuaJIT's lenient `"wb+"` mode.
fn install_host_shims(lua: &Lua) -> Result<()> {
    let g = lua.globals();
    g.set(
        "Inflate",
        lua.create_function(|lua, data: mlua::String| {
            match pob_engine::inflate_timeless_lut(&data.as_bytes()) {
                Ok(out) => Ok(mlua::Value::String(lua.create_string(&out)?)),
                Err(_) => Ok(mlua::Value::String(lua.create_string("")?)),
            }
        })?,
    )?;
    g.set(
        "NewFileSearch",
        lua.create_function(|lua, (pattern, _dirs): (String, Option<bool>)| {
            let (dir, prefix, exact) = match pattern.strip_suffix('*') {
                Some(p) => {
                    let pp = std::path::Path::new(p);
                    (
                        pp.parent()
                            .map(std::path::Path::to_path_buf)
                            .unwrap_or_default(),
                        pp.file_name().map(|n| n.to_string_lossy().into_owned()),
                        false,
                    )
                }
                None => {
                    let pp = std::path::Path::new(&pattern);
                    (
                        pp.parent()
                            .map(std::path::Path::to_path_buf)
                            .unwrap_or_default(),
                        pp.file_name().map(|n| n.to_string_lossy().into_owned()),
                        true,
                    )
                }
            };
            let Some(prefix) = prefix else {
                return Ok(mlua::Value::Nil);
            };
            let dir = if dir.as_os_str().is_empty() {
                std::path::PathBuf::from(".")
            } else {
                dir
            };
            let mut files: Vec<(String, f64)> = std::fs::read_dir(&dir)
                .map(|rd| {
                    rd.filter_map(Result::ok)
                        .filter_map(|e| {
                            let name = e.file_name().to_string_lossy().into_owned();
                            let hit = if exact {
                                name == prefix
                            } else {
                                name.starts_with(&prefix)
                            };
                            let modified = e
                                .metadata()
                                .and_then(|m| m.modified())
                                .ok()
                                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                .map_or(0.0, |d| d.as_secs_f64());
                            hit.then_some((name, modified))
                        })
                        .collect()
                })
                .unwrap_or_default();
            if files.is_empty() {
                return Ok(mlua::Value::Nil);
            }
            files.sort_by(|a, b| a.0.cmp(&b.0));
            let handle = lua.create_table()?;
            let list = lua.create_table()?;
            for (i, (name, modified)) in files.into_iter().enumerate() {
                let entry = lua.create_table()?;
                entry.set("name", name)?;
                entry.set("modified", modified)?;
                list.set(i + 1, entry)?;
            }
            handle.set("files", list)?;
            handle.set("idx", 1)?;
            Ok(mlua::Value::Table(handle))
        })?,
    )?;
    lua.load(
        r#"
local function current(self) return self.files[self.idx] end
local raw_search = NewFileSearch
function NewFileSearch(...)
    local h = raw_search(...)
    if not h then return nil end
    function h:GetFileName() return current(self).name end
    function h:GetFileModifiedTime() return current(self).modified end
    function h:GetFileSize() return 0 end
    function h:NextFile()
        self.idx = self.idx + 1
        return self.files[self.idx] ~= nil
    end
    return h
end
local raw_open = io.open
io.open = function(path, mode)
    if mode then mode = mode:gsub("([rwa])b%+", "%1+b") end
    return raw_open(path, mode)
end
"#,
    )
    .set_name("host_shims")
    .exec()?;
    Ok(())
}

fn build_lua_sandbox(pob_runtime_lua: &std::path::Path) -> Result<Lua> {
    let lua = Lua::new();
    let globals = lua.globals();

    // -- LuaJIT compatibility shims ------------------------------------------------

    // `jit` global (Launch.lua calls jit.opt.start). Provide a no-op opt.start.
    let jit_tbl = lua.create_table()?;
    let jit_opt = lua.create_table()?;
    jit_opt.set(
        "start",
        lua.create_function(|_, _: mlua::MultiValue| Ok(()))?,
    )?;
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

    // Compound assignment (`count += 1`). PoB's newer LuaJIT runtime accepts
    // it but Lua 5.4 doesn't, so rewrite `lhs op= rhs` statements to
    // `lhs = lhs op (rhs)` in every file loaded through `loadfile` / `dofile`
    // (PLoadModule / LoadModule route through `loadfile`).
    lua.load(
        r#"
local raw_loadfile = loadfile
local compound_ops = { ["+"] = true, ["-"] = true, ["*"] = true, ["/"] = true, ["%"] = true, [".."] = true }
local function rewrite_line(line)
    local ind, lhs, op, rhs = line:match("^([ \t]*)([%a_][%w_%.%[%]\"']*)[ \t]*([%+%-%*/%%%.]%.?)=[ \t]*(.-)[ \t]*\r?$")
    if ind and compound_ops[op] and rhs ~= "" then
        local code, comment = rhs:match("^(.-)[ \t]*(%-%-.*)$")
        if code then rhs = code else comment = "" end
        return ind .. lhs .. " = " .. lhs .. " " .. op .. " (" .. rhs .. ")" .. (comment ~= "" and (" " .. comment) or "")
    end
    return line
end
local function rewrite_compound(src)
    return (src:gsub("[^\n]+", rewrite_line))
end
function loadfile(name, mode, env)
    local f = name and io.open(name, "rb")
    if not f then return raw_loadfile(name, mode, env) end
    local src = f:read("a")
    f:close()
    if src:find("[%+%-%*/%%%.]=") then
        src = rewrite_compound(src)
    end
    return load(src, "@" .. name, mode, env or _ENV)
end
function dofile(name)
    local fn = assert(loadfile(name))
    return fn()
end
"#,
    )
    .set_name("compound_assign_shim")
    .exec()?;

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
    let format_patch = r"
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
";
    let _: bool = lua
        .load(format_patch)
        .eval()
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let gsub_patch = r"
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
";
    let _: bool = lua
        .load(gsub_patch)
        .eval()
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    // -- Output redirection: tee ConPrintf/print to a prefix --------------------

    let prefixed_print = lua.create_function(|_, args: mlua::MultiValue| {
        let parts: Vec<String> = args
            .iter()
            .map(|v| match v {
                mlua::Value::String(s) => s.to_string_lossy().clone(),
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
        mlua::Value::String(s) => s.to_string_lossy().clone(),
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
