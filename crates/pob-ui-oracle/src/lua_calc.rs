//! `LuaCalcBridge` (ui doc §10.4, D14; dev only, desktop): serialises the model to XML
//! (calc:BM-A), loads it into a second LuaJIT PoB (pob-oracle), runs the Lua calc, then injects
//! the resulting env/output/breakdown/modDB into the calc doc's Rust types through calc:ORC-X.
//! Owner: U15.
//!
//! lua: Classes/CalcsTab.lua:441-474 (BuildOutput), :760-762 (GetMiscCalculator),
//! Modules/Calcs.lua:89-114 (getMiscCalculator, the `calcFunc` closure 96-112),
//! Modules/Build.lua:1255-1275 and 693-695 (where PoB recalculates), HeadlessWrapper.lua (the
//! headless boot pob-oracle's `JitPob` runs). The calc itself is PoB's (no `@pob` markers: the
//! Rust ports of these lines are calc:PWR's and calc:CA's).
//!
//! One operation = one fresh LuaJIT PoB (`JitPob::boot_oracle`, JIT off, A6), dropped when the
//! operation ends, under the machine-wide oracle lock (`runner::OracleLock`): PoB leaks state
//! between builds in one VM (A3), and a VM costs ≈400 MB. Each operation:
//!
//! 1. `build:SaveDB()` of the live Rust model (BM-A) → XML text;
//! 2. boot PoB with the pob-state/1 encoder (`lua/state_dump.lua`) in the pre chunk, wrap
//!    `calcs.buildOutput` so the socket groups and gems are tagged with their load position
//!    before the first calc pass ([`BRIDGE_INIT_LUA`]), and load the XML (`main:SetMode("BUILD")`
//!    and one `OnFrame`, i.e. `Build:Init` → `wipeGlobalCache()` → `calcsTab:BuildOutput()`,
//!    Build.lua:693-695);
//! 3. **build_output**: dump `calcsTab.mainEnv` (with the misc calculator's base output,
//!    `miscCalculator[2]`) and `calcsTab.calcsEnv` as pob-state/1 documents ([`DUMP_BUILD_OUTPUT_LUA`]),
//!    inject them (`pob_oracle::inject::inject`) and adopt them into the live model
//!    ([`adopt_build_output`]): `calcsTab.{mainEnv,mainOutput,calcsEnv,calcsOutput}`,
//!    `powerBuildFlag`, the socket groups and gem instances the calc wrote (written back onto
//!    the live objects, and the injected envs re-pointed at them), `skillsTab.displayGroup`
//!    cleared when the calc removed it (CalcSetup.lua:1556-1558), `node.finalModList`, the
//!    clamped `build.mainSocketGroup` / `calcsTab.input.skill_number` (CalcSetup.lua:1586,
//!    :1589), the items' `jewelRadiusData` (:293-298) and `jewelData.limitDisabled` (:852,
//!    :866), and every item reference of the injected envs (`env.flasks` / `tinctures` keys,
//!    `actor.itemList`, `radiusJewelList[*].item`, `sourceItem`s, …) re-pointed at
//!    `build.itemsTab.items[item.id]`;
//! 4. **MiscCalc::call**: `calcFunc(override, useFullDPS)` on `calcsTab:GetMiscCalculator()`
//!    with the override rebuilt in Lua ([`override_lua`]), and the returned output dumped and
//!    injected ([`CALL_MISC_LUA`]).
//!
//! DIVERGENCE (all inherent to running PoB in a second VM; the bridge is a parity tool):
//! * `MiscCalc::call` re-serialises the live model and reloads it, so the `calcFunc` it calls is
//!   the one `BuildOutput` creates for the model *as it is at call time*: `usedFullDPS`
//!   (Calcs.lua:95, captured when `getMiscCalculator` ran) and the GlobalCache reflect the
//!   current model. In PoB a caller that mutates the model temporarily
//!   (GemSelectControl:CalcOutputWithThisGem, the ConfigTab tooltips) calls the closure built
//!   by the last real `BuildOutput`, with a GlobalCache that earlier calls filled. For an
//!   unmutated model the sequence is PoB's (load → `BuildOutput` → `calcFunc`).
//! * Overrides cross the VM boundary by value: nodes by id (`build.spec.nodes[id]`), items by
//!   identity when the item is the live `build.itemsTab.items[item.id]`, else as
//!   `new("Item"):Item(item:BuildRaw())`; `conditions` is copied back after the call (the calc
//!   appends to it, CalcSetup.lua:1921-1922); `override.spec` by its index in
//!   `build.treeTab.specList` (TreeTab.lua:70; a spec outside the list raises a Lua error). A
//!   non-empty `override.extraJewelFuncs` and non-scalar `override.extra` entries are not
//!   transferable and raise a Lua error; an empty `extraJewelFuncs` crosses as `new("ModList"):ModList()`; the
//!   `extraJewelFuncs` the calc creates (CalcSetup.lua:1425-1431) is not copied back.
//! * The injected envs keep PoB's own copies of the other env's objects (`$foreign`
//!   placeholders) and of the items the calc makes itself (no `id`: Energy Blade's,
//!   CalcSetup.lua:1124; `override.repItem` copies). Socket groups, gem instances and the
//!   build's items are re-pointed at the live objects (see 3.), because the UI writes through
//!   them (`srcInstance.skillPart`, Build.lua:494-566) or looks them up by identity
//!   (`mainEnv.flasks[item]`, ItemsTab.lua:4868, :4904).
//! * Lost to the per-document registry reset (CCR-0088 §2), in the calcsEnv document: its
//!   object-keyed side entries (`env.flaskSlotMap`, `player.appliedEnemyModifiers`,
//!   `itemList[*].slotModList`, `grantedSkills[*].sourceItem.slotModList`, the
//!   `explodeSources` entries' side keys); the mainEnv document's, at the next `inject` (a
//!   `MiscCalc::call`). No UI code reads them (only `Modules/Calc*.lua` do).
//! * CalcsTab.lua:465-468 (`SetBreakdownData` refresh when `displayData` is set) is UI-side and
//!   belongs to the CalcsTab unit (U39), not to either facade implementation.
//! * A `MiscCalc::call` does not write back what the CALCULATOR pass writes into the model in
//!   PoB (`gemInstance.displayEffect`/`supportEffect`, CalcSetup.lua 1672-1787; its
//!   `build.mainSocketGroup` clamp, :1589, is a no-op after `build_output` since only MAIN
//!   changes the group list): the live groups keep the state of the last `build_output`. The
//!   returned output's model objects (`ReqStrItem.sourceGem`, …) are re-pointed at the live
//!   groups and gems by position.
//! * `calcsTab.miscCalculator` (a `pob_calc::modules::calcs::MiscCalculator`) is not set: the
//!   bridge keeps its own calculator and returns it from `misc_calculator`.
//! * Lua errors of the Lua calc, and failures to boot/load PoB or to inject a dump, are raised
//!   as Rust Lua errors (`lua_error!`), which propagate like the Rust calc's (calc doc D15).
//!
//! ORDER: the calcsEnv document is injected before the mainEnv one (each `inject` resets the
//! probe registry and the GlobalCache; the main env's document is last and carries PoB's
//! GlobalCache and the misc calculator's base output, the calcsEnv one is dumped without the
//! GlobalCache).

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::task::Poll;

use pob_app::calc::facade::{BuildRef, CalcFacade, MiscCalc, Output, Override};
use pob_calc::active_skill::{ActiveSkillRef, SkillEffectRef};
use pob_calc::actor::ActorRef;
use pob_calc::classes::{Item, ItemRef, ModStore, NodeRef};
use pob_calc::env::EnvRef;
use pob_calc::lua_table::{Key, Table, Val};
use pob_calc::model::{Build, GemInstance, GemInstanceRef, SocketGroup, SocketGroupRef};
use pob_calc::probe::{rc_ptr, set_member, PKey, PObj};
use pob_lua::{lua_error, PtrMap, Shared};
use pob_oracle::headless::Engine;
use pob_oracle::inject::{inject, Injected};
use pob_oracle::jit::JitPob;
use pob_oracle::runner::{OracleLock, STATE_DUMP_LUA};
use pob_oracle::schema::StateDump;

// ---- Lua chunks -----------------------------------------------------------------------------

/// Run after boot, before the build loads: helpers for the override chunk, and the load-position
/// tags of socket groups and gems (taken when the first `calcs.buildOutput` starts, i.e. after
/// `SkillsTab:Load` and before the calc inserts, rebuilds or removes granted-skill groups,
/// CalcSetup.lua:1464-1530).
pub const BRIDGE_INIT_LUA: &str = r##"
__bridge = {
	groupLoad = setmetatable({ }, { __mode = "k" }),
	gemLoad = setmetatable({ }, { __mode = "k" }),
	tagged = false,
}
local calcs = require("Modules.Calcs")
local buildOutput = calcs.buildOutput
calcs.buildOutput = function(build, mode, ...)
	if not __bridge.tagged then
		__bridge.tagged = true
		for i, group in ipairs(build.skillsTab.socketGroupList) do
			__bridge.groupLoad[group] = i
			for j, gem in ipairs(group.gemList or { }) do
				__bridge.gemLoad[gem] = i .. ":" .. j
			end
		end
	end
	return buildOutput(build, mode, ...)
end
function __bridge_node(b, id)
	local node = b.spec.nodes[id]
	if not node then
		error("LuaCalcBridge: node " .. tostring(id) .. " is not in build.spec.nodes")
	end
	return node
end
function __bridge_item(b, id, raw)
	if id then
		local item = b.itemsTab.items[id]
		if not item then
			error("LuaCalcBridge: item " .. tostring(id) .. " is not in build.itemsTab.items")
		end
		return item
	end
	return new("Item"):Item(raw)
end
function __bridge_spec(b, i)
	local spec = b.treeTab.specList[i]
	if not spec then
		error("LuaCalcBridge: spec " .. tostring(i) .. " is not in build.treeTab.specList")
	end
	return spec
end
function __bridge_doc(env, hit, locals)
	local S = __state_dump
	return S.encodeDoc({ build = __pob_name, hit = hit .. "#1", probe = hit, n = 1, kind = "out",
		stack = { }, env = env, envId = env and S.envId(env) or nil, locals = locals })
end
-- An encoded document without the GlobalCache (`state_dump.lua` reads it with rawget).
function __bridge_doc_nocache(env, hit, locals)
	local gc = rawget(_G, "GlobalCache")
	rawset(_G, "GlobalCache", nil)
	local ok, res = pcall(__bridge_doc, env, hit, locals)
	rawset(_G, "GlobalCache", gc)
	if not ok then error(res, 0) end
	return res
end
"##;

/// After the load: the documents of `calcsTab.mainEnv` (with `calcsTab.miscCalculator[2]`, the
/// socket groups, their load tags and the spec nodes' `finalModList` as locals) and
/// `calcsTab.calcsEnv` (CalcsTab.lua:459-471). Both envs are registered first, so each one's
/// actors, stores and skills are `$foreign` markers in the other's document.
pub const DUMP_BUILD_OUTPUT_LUA: &str = r#"
local S = __state_dump
local b = __mainObject__.main.modes["BUILD"]
local ct = b.calcsTab
if not ct or not ct.mainEnv or not ct.calcsEnv then
	error("LuaCalcBridge: calcsTab:BuildOutput() did not run")
end
S.envId(ct.mainEnv)
S.envId(ct.calcsEnv)
local groupLoad, gemLoad = { }, { }
for i, group in ipairs(b.skillsTab.socketGroupList) do
	groupLoad[i] = __bridge.groupLoad[group] or 0
	local list = { }
	for j, gem in ipairs(group.gemList or { }) do
		list[j] = __bridge.gemLoad[gem] or ""
	end
	gemLoad[i] = list
end
local nodeFinal = { }
for id, node in pairs(b.spec.nodes) do
	if node.finalModList == nil then
		nodeFinal[id] = false
	elseif rawequal(node.finalModList, node.modList) then
		nodeFinal[id] = true
	else
		nodeFinal[id] = node.finalModList
	end
end
-- the calc's writes onto the build's items: CalcSetup.lua:293-298 (MAIN) item.jewelRadiusData,
-- :852 / :866 item.jewelData.limitDisabled (written for the jewels of allocated sockets only, so
-- only those are reported: elsewhere the live model keeps its own value, as PoB's does)
local itemRadius, itemLimit = { }, { }
for id, item in pairs(b.itemsTab.items) do
	if item.jewelRadiusData then
		itemRadius[id] = item.jewelRadiusData
	end
end
for _, slot in ipairs(b.itemsTab.orderedSlots) do
	if slot.nodeId and ct.mainEnv.allocNodes[slot.nodeId] then
		local item = b.itemsTab.items[slot.selItemId]
		if item and item.id and type(item.jewelData) == "table" then
			itemLimit[item.id] = item.jewelData.limitDisabled and true or false
		end
	end
end
local calcsDoc = __bridge_doc_nocache(ct.calcsEnv, "bridge:calcsEnv",
	{ socketGroupList = b.skillsTab.socketGroupList })
local mainDoc = __bridge_doc(ct.mainEnv, "bridge:mainEnv",
	{ socketGroupList = b.skillsTab.socketGroupList, groupLoad = groupLoad, gemLoad = gemLoad, nodeFinal = nodeFinal,
	  miscCalcBase = ct.miscCalculator and ct.miscCalculator[2],
	  -- CalcSetup.lua:1586 (CALCS) and :1589 (MAIN) clamp these in the model
	  mainSocketGroup = b.mainSocketGroup, skillNumber = ct.input.skill_number,
	  itemRadius = itemRadius, itemLimit = itemLimit })
return mainDoc, calcsDoc
"#;

/// After the load and `__bridge_override` / `__bridge_full` are defined: one
/// `calcFunc(override, useFullDPS)` (Calcs.lua:96-112 through CalcsTab.lua:760-762).
pub const CALL_MISC_LUA: &str = r#"
local S = __state_dump
local b = __mainObject__.main.modes["BUILD"]
local ct = b.calcsTab
S.envId(ct.mainEnv)
S.envId(ct.calcsEnv)
local calcFunc = ct:GetMiscCalculator()
local override = __bridge_override(b)
local output = calcFunc(override, __bridge_full)
return __bridge_doc_nocache(nil, "bridge:calcFunc",
	{ output = output, conditions = override and override.conditions,
	  socketGroupList = b.skillsTab.socketGroupList })
"#;

/// The pre chunk: quiet `print`, the encoder's config and the encoder (as
/// `runner::probe_prelude` loads it, without the probe runtime or provenance).
fn pre_chunk(root: &str) -> String {
    format!(
        "{quiet}\n__probe_cfg = {{ root = {root} }}\ndo\n{dump}\nend\n",
        quiet = pob_oracle::oracle::QUIET_PRELUDE,
        root = lua_quote(root),
        dump = STATE_DUMP_LUA,
    )
}

/// A Lua string literal (bytes kept, quotes, backslashes and control bytes escaped).
pub fn lua_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                let _ = write!(out, "\\{:03}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ---- Lua sessions -----------------------------------------------------------------------------

/// PoB's `src/` for the bridge: the explicit one, else `POB_SRC` / `.PathOfBuilding/src` above
/// the working directory or the executable (`pob_oracle::oracle::find_pob_src`).
pub fn pob_src(explicit: Option<&Path>) -> Result<PathBuf, String> {
    match explicit {
        Some(p) => Ok(p.to_path_buf()),
        None => pob_oracle::oracle::find_pob_src().ok_or_else(|| {
            "LuaCalcBridge: PoB checkout (.PathOfBuilding/src) not found".to_owned()
        }),
    }
}

/// One LuaJIT PoB with a build loaded (held with the oracle lock; dropped after one operation).
pub struct LuaSession {
    pob: JitPob,
    _lock: OracleLock,
}

impl LuaSession {
    /// Boot PoB at `src` and load `xml` as build `name` (the bridge's tags installed first).
    ///
    /// Takes the machine-wide oracle lock (`OracleLock::acquire`, a `flock` on a fresh fd):
    /// a caller in this process that already holds an `OracleLock` must set
    /// `POB_ORACLE_LOCK_HELD`, or this call blocks forever.
    pub fn load(src: &Path, xml: &str, name: &str) -> Result<Self, String> {
        let lock = OracleLock::acquire().map_err(|e| e.0)?;
        let root = std::fs::canonicalize(src).map_err(|e| format!("{}: {e}", src.display()))?;
        let mut root_s = root.to_string_lossy().replace('\\', "/");
        if !root_s.ends_with('/') {
            root_s.push('/');
        }
        let mut pob = JitPob::boot_oracle(&root, Some(&pre_chunk(&root_s)))
            .map_err(|e| format!("LuaCalcBridge: PoB boot failed: {e}"))?;
        pob.run(BRIDGE_INIT_LUA)
            .map_err(|e| format!("LuaCalcBridge: init: {e}"))?;
        pob.load_build_xml(xml, name)
            .map_err(|e| format!("LuaCalcBridge: {e}"))?;
        Ok(Self { pob, _lock: lock })
    }

    /// Run a chunk, returning its results as text.
    pub fn run(&mut self, code: &str) -> Result<Vec<String>, String> {
        self.pob
            .run(code)
            .map(|r| r.iter().map(|x| x.text().to_owned()).collect())
    }

    /// The documents of [`DUMP_BUILD_OUTPUT_LUA`].
    pub fn dump_build_output(&mut self) -> Result<BuildOutputDocs, String> {
        let r = self.run(DUMP_BUILD_OUTPUT_LUA)?;
        let [main, calcs] = <[String; 2]>::try_from(r)
            .map_err(|r| format!("LuaCalcBridge: dump returned {} values", r.len()))?;
        Ok(BuildOutputDocs {
            main: StateDump::from_json(&main).map_err(|e| e.0)?,
            calcs: StateDump::from_json(&calcs).map_err(|e| e.0)?,
        })
    }

    /// `calcFunc(override, useFullDPS)` where `override_body` is the body of a Lua function of
    /// `b` (the build) returning the override table or nil.
    pub fn call_misc(
        &mut self,
        override_body: &str,
        use_full_dps: Option<bool>,
    ) -> Result<StateDump, String> {
        let full = match use_full_dps {
            None => "nil",
            Some(true) => "true",
            Some(false) => "false",
        };
        self.run(&format!(
            "function __bridge_override(b)\n{override_body}\nend\n__bridge_full = {full}\n"
        ))?;
        let r = self.run(CALL_MISC_LUA)?;
        let doc = r
            .into_iter()
            .next()
            .ok_or_else(|| "LuaCalcBridge: calcFunc dump returned nothing".to_owned())?;
        StateDump::from_json(&doc).map_err(|e| e.0)
    }
}

/// The two documents of one `build_output`.
pub struct BuildOutputDocs {
    pub main: StateDump,
    pub calcs: StateDump,
}

/// `build:SaveDB()` of the live model (BM-A), the text the bridge loads into PoB.
pub fn save_xml(build: &Build) -> Result<String, String> {
    build
        .SaveDB("")
        .ok_or_else(|| "LuaCalcBridge: build:SaveDB() produced no XML".to_owned())
}

// ---- Override → Lua ------------------------------------------------------------------------------

/// The body of `__bridge_override(b)` rebuilding `ov` in the Lua VM (see the module doc for
/// what crosses and how).
pub fn override_lua(build: &Build, ov: Option<&Override>) -> Result<String, String> {
    let Some(ov) = ov else {
        return Ok("return nil".to_owned());
    };
    let mut s = String::from("local o = { }\n");
    if let Some(spec) = ov.spec.get() {
        // TreeTab.lua:70 `calcFunc({ spec = spec })`, spec an entry of the build's spec list
        // (TreeTab:Save/Load keep its order)
        let i = build
            .treeTab
            .specList
            .with(|l| l.iter().position(|x| Rc::ptr_eq(x, &spec)))
            .ok_or_else(|| {
                "LuaCalcBridge: override.spec is not in build.treeTab.specList and cannot cross to the Lua calc"
                    .to_owned()
            })?;
        let _ = writeln!(s, "o.spec = __bridge_spec(b, {})", i + 1);
    }
    let nodes = |s: &mut String, field: &str, map: &Shared<pob_lua::LjIntMap<NodeRef>>| {
        let _ = writeln!(s, "o.{field} = {{ }}");
        for (_, node) in map.with(pob_lua::LjIntMap::pairs) {
            let _ = writeln!(
                s,
                "o.{field}[__bridge_node(b, {})] = true",
                lua_num(node.id)
            );
        }
    };
    if let Some(m) = ov.addNodes.get() {
        nodes(&mut s, "addNodes", &m);
    }
    if let Some(m) = ov.removeNodes.get() {
        nodes(&mut s, "removeNodes", &m);
    }
    if let Some(name) = ov.repSlotName.get() {
        let _ = writeln!(s, "o.repSlotName = {}", lua_quote(name.as_str()));
    }
    let item = |it: &pob_calc::classes::ItemRef| -> String {
        let live = it.id.get().and_then(|id| {
            build
                .itemsTab
                .items
                .with(|m| m.get(&(id as u32)).cloned())
                .filter(|l| Rc::ptr_eq(l, it))
                .map(|_| id)
        });
        match live {
            Some(id) => format!("__bridge_item(b, {}, nil)", lua_num(id)),
            None => format!("__bridge_item(b, nil, {})", lua_quote(&it.BuildRaw())),
        }
    };
    if let Some(it) = ov.repItem.get() {
        let _ = writeln!(s, "o.repItem = {}", item(&it));
    }
    if let Some(it) = ov.toggleFlask.get() {
        let _ = writeln!(s, "o.toggleFlask = {}", item(&it));
    }
    if let Some(it) = ov.toggleTincture.get() {
        let _ = writeln!(s, "o.toggleTincture = {}", item(&it));
    }
    if let Some(c) = ov.conditions.get() {
        s.push_str("o.conditions = { }\n");
        for name in c.snapshot() {
            let _ = writeln!(
                s,
                "o.conditions[#o.conditions + 1] = {}",
                lua_quote(name.as_str())
            );
        }
    }
    if let Some(list) = ov.extraJewelFuncs.get() {
        // `#override.extraJewelFuncs > 0` (CalcSetup.lua:1432)
        if !list.is_empty() {
            return Err(
                "LuaCalcBridge: a non-empty override.extraJewelFuncs cannot cross to the Lua calc"
                    .to_owned(),
            );
        }
        s.push_str("o.extraJewelFuncs = new(\"ModList\"):ModList()\n");
    }
    for (k, v) in ov.extra.pairs() {
        let key = match &k {
            Key::S(n) => lua_quote(n.as_str()),
            Key::I(i) => i.to_string(),
            Key::B(b) => b.to_string(),
            Key::F(f) => lua_num(f.get()),
        };
        let val = match &v {
            Val::Bool(b) => b.to_string(),
            Val::Num(n) => lua_num(*n),
            Val::Str(t) => lua_quote(t),
            _ => {
                return Err(format!(
                    "LuaCalcBridge: override[{key}] ({}) cannot cross to the Lua calc",
                    v.lua_type()
                ))
            }
        };
        let _ = writeln!(s, "o[{key}] = {val}");
    }
    s.push_str("return o\n");
    Ok(s)
}

/// A number as a Lua literal that reads back as the same double.
fn lua_num(n: f64) -> String {
    if n.is_finite() && n == n.trunc() && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else if n.is_nan() {
        "(0/0)".to_owned()
    } else if n.is_infinite() {
        if n > 0.0 { "math.huge" } else { "(-math.huge)" }.to_owned()
    } else {
        format!("{n:e}")
    }
}

// ---- adopting an injected build output ---------------------------------------------------------

/// What [`adopt_build_output`] did (diagnostics for tests and the parity CLI).
#[derive(Debug, Default, Clone)]
pub struct AdoptReport {
    /// Injection fallbacks of the injected documents ("<kind>.<key>: reason" → count).
    pub fallbacks: std::collections::BTreeMap<String, u64>,
    /// Injection notes (kind conflicts, unresolvable markers).
    pub notes: Vec<String>,
    /// Socket groups written back onto a live group / created by the Lua calc.
    pub groups_kept: usize,
    pub groups_new: usize,
    pub gems_kept: usize,
    pub gems_new: usize,
    /// Skills / skill effects re-pointed at a live group or gem.
    pub rebound: usize,
    /// Spec nodes whose `finalModList` was set.
    pub nodes_final: usize,
    /// Item references (env maps, item lists, tables) re-pointed at `build.itemsTab.items`.
    pub items_rebound: usize,
    /// `skillsTab.displayGroup` was a group the calc removed (CalcSetup.lua:1556-1558).
    pub display_group_cleared: bool,
}

fn inj(doc: &StateDump, build: &BuildRef, rep: &mut AdoptReport) -> Result<Injected, String> {
    let i = inject(doc, build).map_err(|e| format!("LuaCalcBridge: inject {}: {e}", doc.hit))?;
    for (k, n) in &i.fallbacks {
        *rep.fallbacks.entry(k.clone()).or_default() += n;
    }
    rep.notes.extend(i.notes.iter().cloned());
    Ok(i)
}

/// Inject the documents of one `build_output` and adopt them into `build` (CalcsTab.lua:441-472
/// as PoB ran it). Returns the misc calculator's base output and the report.
pub fn adopt_build_output(
    build: &BuildRef,
    docs: &BuildOutputDocs,
) -> Result<(Output, AdoptReport), String> {
    let mut rep = AdoptReport::default();
    // ORDER: calcs, main (module doc; CCR-0088 §2: each inject resets the probe registry)
    let calcs = inj(&docs.calcs, build, &mut rep)?;
    let main = inj(&docs.main, build, &mut rep)?;
    let base_output = main
        .locals
        .get("miscCalcBase")
        .and_then(Val::table)
        .unwrap_or_default();
    let main_env = main
        .env
        .clone()
        .ok_or_else(|| "LuaCalcBridge: the mainEnv document has no env".to_owned())?;
    let calcs_env = calcs
        .env
        .clone()
        .ok_or_else(|| "LuaCalcBridge: the calcsEnv document has no env".to_owned())?;

    let mut rb = Rebind::new(build);
    adopt_socket_groups(build, &main.locals, &calcs.locals, &mut rb, &mut rep)?;
    adopt_model_writes(build, &main.locals);
    rb.env(&main_env);
    rb.env(&calcs_env);
    for g in build.skillsTab.socketGroupList.get().snapshot() {
        // `group.sourceItem`, `group.explodeSources` (CalcSetup.lua:1486, :1520)
        rb.table(&g.extra);
        if let Some(t) = g.explodeSources.get() {
            rb.table(&t);
        }
        if let Some(l) = g.displaySkillList.get() {
            rb.skills(&l);
        }
        if let Some(l) = g.displaySkillListCalcs.get() {
            rb.skills(&l);
        }
        for gem in g.gemList.get().snapshot() {
            if let Some(e) = gem.displayEffect.get() {
                rb.effect(&e);
            }
            if let Some(e) = gem.supportEffect.get() {
                rb.effect(&e);
            }
        }
    }
    // outputs hold model objects too (`output.ReqStrItem.sourceGem`, CalcPerform.lua)
    for env in [&main_env, &calcs_env] {
        rb.table(&env.player.output.get());
        if let Some(m) = env.minion.get() {
            rb.table(&m.output.get());
        }
    }
    rb.table(&base_output);
    rep.rebound = rb.rebound;
    rep.items_rebound = rb.items_rebound;
    rep.nodes_final = adopt_final_mod_lists(build, main.locals.get("nodeFinal"));

    // CalcsTab.lua:442 self.powerBuildFlag = true
    build.calcsTab.powerBuildFlag.set(true);
    // CalcsTab.lua:459-462
    let ct = &build.calcsTab;
    ct.mainOutput.set(Some(main_env.player.output.get()));
    ct.mainEnv.set(Some(main_env));
    ct.calcsOutput.set(Some(calcs_env.player.output.get()));
    ct.calcsEnv.set(Some(calcs_env));
    Ok((base_output, rep))
}

/// A socket group or gem of a document's `socketGroupList` local: a typed object when a typed
/// context reached it (`activeSkill.socketGroup`, `skillEffect.srcInstance`), else a table.
enum Doc<T> {
    Obj(Rc<T>),
    Table(Table),
}

fn doc_list<T: 'static>(v: Option<&Val>) -> Vec<Doc<T>> {
    let Some(t) = v.and_then(Val::table) else {
        return Vec::new();
    };
    t.ipairs()
        .into_iter()
        .filter_map(|(_, v)| match v.obj::<T>() {
            Some(o) => Some(Doc::Obj(o)),
            None => v.table().map(Doc::Table),
        })
        .collect()
}

fn doc_gems(g: &Doc<SocketGroup>) -> Vec<Doc<GemInstance>> {
    match g {
        Doc::Obj(o) => o
            .gemList
            .get()
            .snapshot()
            .into_iter()
            .map(Doc::Obj)
            .collect(),
        Doc::Table(t) => doc_list::<GemInstance>(Some(&t.get("gemList"))),
    }
}

fn doc_ptr<T>(d: &Doc<T>) -> usize {
    match d {
        Doc::Obj(o) => rc_ptr(o),
        Doc::Table(t) => t.as_ptr() as usize,
    }
}

// CCR-0088 §1: inject copies socket groups and gems; this writes them back onto the live model.
/// Write the Lua calc's socket groups onto the live ones (by load position), keep the new ones
/// (granted skills, explode sources) and drop the ones the calc removed; record the
/// injected → live pointer map for [`Rebind`].
fn adopt_socket_groups(
    build: &BuildRef,
    main: &pob_oracle::inject::Locals,
    calcs: &pob_oracle::inject::Locals,
    rb: &mut Rebind,
    rep: &mut AdoptReport,
) -> Result<(), String> {
    let live_list = build.skillsTab.socketGroupList.get();
    let old: Vec<SocketGroupRef> = live_list.snapshot();
    let old_gems: Vec<Vec<GemInstanceRef>> =
        old.iter().map(|g| g.gemList.get().snapshot()).collect();
    let main_groups = doc_list::<SocketGroup>(main.get("socketGroupList"));
    let calcs_groups = doc_list::<SocketGroup>(calcs.get("socketGroupList"));
    if calcs_groups.len() != main_groups.len() {
        return Err(format!(
            "LuaCalcBridge: socketGroupList has {} groups in the mainEnv document and {} in the calcsEnv one",
            main_groups.len(),
            calcs_groups.len()
        ));
    }
    let group_load = main
        .get("groupLoad")
        .and_then(Val::table)
        .unwrap_or_default();
    let gem_load = main.get("gemLoad").and_then(Val::table).unwrap_or_default();
    let mut new_list = Vec::with_capacity(main_groups.len());
    for (i0, g) in main_groups.iter().enumerate() {
        let i = (i0 + 1) as i64;
        let load = group_load.get(i).num_or(0.0) as usize;
        let live = match old.get(load.wrapping_sub(1)) {
            Some(l) if load >= 1 => {
                rep.groups_kept += 1;
                l.clone()
            }
            _ => {
                rep.groups_new += 1;
                Rc::new(SocketGroup::default())
            }
        };
        write_group(&live, g);
        // gems
        let loads = gem_load.get(i).table().unwrap_or_default();
        let mut gems = Vec::new();
        for (j0, gem) in doc_gems(g).iter().enumerate() {
            let key = loads.get((j0 + 1) as i64).str();
            let pos = key.as_deref().and_then(|k| {
                let (a, b) = k.split_once(':')?;
                Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?))
            });
            let live_gem = if let Some(l) = pos.and_then(|(a, b)| old_gems.get(a - 1)?.get(b - 1)) {
                rep.gems_kept += 1;
                l.clone()
            } else {
                rep.gems_new += 1;
                Rc::new(GemInstance::default())
            };
            write_gem(&live_gem, gem);
            rb.gems.insert(doc_ptr(gem), live_gem.clone());
            gems.push(live_gem);
        }
        live.gemList.get().with_mut(|v| *v = gems.clone());
        rb.groups.insert(doc_ptr(g), live.clone());
        new_list.push(live);
    }
    // the calcsEnv document: the same groups and gems (final positions)
    for (i0, g) in calcs_groups.iter().enumerate() {
        let live = &new_list[i0];
        rb.groups.insert(doc_ptr(g), live.clone());
        // CalcSetup.lua:1913 group.displaySkillListCalcs (the CALCS pass's skills)
        let calcs_list = match g {
            Doc::Obj(o) => o.displaySkillListCalcs.get(),
            Doc::Table(t) => skill_list(&t.get("displaySkillListCalcs")),
        };
        live.displaySkillListCalcs.set(calcs_list);
        let live_gems = live.gemList.get().snapshot();
        for (j0, gem) in doc_gems(g).iter().enumerate() {
            if let Some(l) = live_gems.get(j0) {
                rb.gems.insert(doc_ptr(gem), l.clone());
            }
        }
    }
    // CalcSetup.lua:1556-1558: a removed group stops being the skills tab's display group
    if let Some(d) = build.skillsTab.displayGroup.get() {
        if old.iter().any(|g| Rc::ptr_eq(g, &d)) && !new_list.iter().any(|g| Rc::ptr_eq(g, &d)) {
            build.skillsTab.displayGroup.set(None);
            rep.display_group_cleared = true;
        }
    }
    live_list.with_mut(|v| *v = new_list);
    Ok(())
}

/// The calc's other writes into the model (module doc, 3.): CalcSetup.lua:1589
/// `build.mainSocketGroup`, :1586 `calcsTab.input.skill_number` (the clamps to the final group
/// count), :293-298 `item.jewelRadiusData`, :852 / :866 `item.jewelData.limitDisabled`.
fn adopt_model_writes(build: &BuildRef, main: &pob_oracle::inject::Locals) {
    if let Some(v) = main.get("mainSocketGroup").and_then(Val::opt_num) {
        build.mainSocketGroup.set(v);
    }
    if let Some(v) = main.get("skillNumber").and_then(Val::opt_num) {
        build.calcsTab.input.set("skill_number", v);
    }
    let live = |id: i64| build.itemsTab.items.with(|m| m.get(&(id as u32)).cloned());
    if let Some(t) = main.get("itemRadius").and_then(Val::table) {
        for (k, v) in t.pairs() {
            let (Key::I(id), Some(src)) = (k, v.table()) else {
                continue;
            };
            let Some(item) = live(id) else { continue };
            // L295-297: created once, then one entry per pass (entries accumulate, as in PoB)
            let dst = match item.jewelRadiusData.get() {
                Some(d) => d,
                None => {
                    let d = Table::new();
                    item.jewelRadiusData.set(Some(d.clone()));
                    d
                }
            };
            for (nk, nv) in src.pairs() {
                dst.set(nk, nv);
            }
        }
    }
    if let Some(t) = main.get("itemLimit").and_then(Val::table) {
        for (k, v) in t.pairs() {
            let Key::I(id) = k else { continue };
            let Some(jd) = live(id).and_then(|i| i.jewelData.get()) else {
                continue;
            };
            jd.set(
                "limitDisabled",
                if v.truthy() {
                    Val::Bool(true)
                } else {
                    Val::Nil
                },
            );
        }
    }
}

/// Every typed field of a socket group from a document's group (the gem list excepted).
fn write_group(dst: &SocketGroupRef, src: &Doc<SocketGroup>) {
    match src {
        Doc::Obj(s) => copy_group(dst, s),
        Doc::Table(t) => {
            copy_group(dst, &SocketGroup::default());
            let o = PObj::Group(dst.clone());
            for (k, v) in t.pairs() {
                let Key::S(n) = &k else { continue };
                match n.as_str() {
                    "gemList" => {}
                    // a skill list a typed context did not reach is a dynamic table here
                    "displaySkillList" => dst.displaySkillList.set(skill_list(&v)),
                    "displaySkillListCalcs" => dst.displaySkillListCalcs.set(skill_list(&v)),
                    key => {
                        set_member(&o, &PKey::S(key.to_owned()), v);
                    }
                }
            }
        }
    }
}

/// A skill list from a dynamic-table value: the typed list (`SBox`) or a table of skills.
fn skill_list(v: &Val) -> Option<Shared<Vec<ActiveSkillRef>>> {
    if let Some(b) = v.obj::<pob_calc::probe::SBox<Vec<ActiveSkillRef>>>() {
        return Some(b.0.clone());
    }
    let t = v.table()?;
    Some(Shared::new(
        t.ipairs()
            .into_iter()
            .filter_map(|(_, s)| s.obj::<pob_calc::active_skill::ActiveSkill>())
            .collect(),
    ))
}

fn write_gem(dst: &GemInstanceRef, src: &Doc<GemInstance>) {
    match src {
        Doc::Obj(s) => copy_gem(dst, s),
        Doc::Table(t) => {
            copy_gem(dst, &GemInstance::default());
            let o = PObj::Gem(dst.clone());
            for (k, v) in t.pairs() {
                if let Key::S(n) = &k {
                    set_member(&o, &PKey::S(n.as_str().to_owned()), v);
                }
            }
        }
    }
}

fn copy_table(dst: &Table, src: &Table) {
    if dst.ptr_eq(src) {
        return;
    }
    dst.wipe();
    for (k, v) in src.pairs() {
        dst.set(k, v);
    }
}

fn copy_group(d: &SocketGroup, s: &SocketGroup) {
    d.label.set(s.label.get());
    d.enabled.set(s.enabled.get());
    d.slotEnabled.set(s.slotEnabled.get());
    d.slot.set(s.slot.get());
    d.source.set(s.source.get());
    d.includeInFullDPS.set(s.includeInFullDPS.get());
    d.groupCount.set(s.groupCount.get());
    d.mainActiveSkill.set(s.mainActiveSkill.get());
    d.mainActiveSkillCalcs.set(s.mainActiveSkillCalcs.get());
    d.noSupports.set(s.noSupports.get());
    d.displaySkillList.set(s.displaySkillList.get());
    d.displaySkillListCalcs.set(s.displaySkillListCalcs.get());
    d.displayLabel.set(s.displayLabel.get());
    d.explodeSources.set(s.explodeSources.get());
    copy_table(&d.extra, &s.extra);
}

fn copy_gem(d: &GemInstance, s: &GemInstance) {
    d.nameSpec.set(s.nameSpec.get());
    d.skillId.set(s.skillId.get());
    d.gemId.set(s.gemId.get());
    d.level.set(s.level.get());
    d.quality.set(s.quality.get());
    d.enabled.set(s.enabled.get());
    d.enableGlobal1.set(s.enableGlobal1.get());
    d.enableGlobal2.set(s.enableGlobal2.get());
    d.count.set(s.count.get());
    d.gemData.set(s.gemData.get());
    d.grantedEffect.set(s.grantedEffect.get());
    d.skillPart.set(s.skillPart.get());
    d.skillPartCalcs.set(s.skillPartCalcs.get());
    d.skillStageCount.set(s.skillStageCount.get());
    d.skillStageCountCalcs.set(s.skillStageCountCalcs.get());
    d.skillMineCount.set(s.skillMineCount.get());
    d.skillMineCountCalcs.set(s.skillMineCountCalcs.get());
    d.skillMinion.set(s.skillMinion.get());
    d.skillMinionCalcs.set(s.skillMinionCalcs.get());
    d.skillMinionItemSet.set(s.skillMinionItemSet.get());
    d.skillMinionItemSetCalcs
        .set(s.skillMinionItemSetCalcs.get());
    d.skillMinionSkill.set(s.skillMinionSkill.get());
    d.skillMinionSkillCalcs.set(s.skillMinionSkillCalcs.get());
    d.fromItem.set(s.fromItem.get());
    d.noSupports.set(s.noSupports.get());
    d.triggered.set(s.triggered.get());
    d.displayEffect.set(s.displayEffect.get());
    d.supportEffect.set(s.supportEffect.get());
    d.reqLevel.set(s.reqLevel.get());
    copy_table(&d.extra, &s.extra);
}

/// `node.finalModList` of every spec node as the Lua calc left it (CalcsTab.lua:454-457 and the
/// calc's own writes): `true` = the node's own `modList`, `false` = nil, else the dumped store.
fn adopt_final_mod_lists(build: &BuildRef, v: Option<&Val>) -> usize {
    let Some(t) = v.and_then(Val::table) else {
        return 0;
    };
    let spec = build.spec.get();
    let mut n = 0;
    for (k, v) in t.pairs() {
        let Key::I(id) = k else { continue };
        let Some(node) = spec.nodes.with(|m| m.get(id)) else {
            continue;
        };
        match v {
            Val::Bool(true) => node.finalModList.set(Some(node.modList.get())),
            Val::Bool(false) => node.finalModList.set(None),
            other => match other.obj::<ModStore>() {
                Some(s) => node.finalModList.set(Some(s)),
                None => continue,
            },
        }
        n += 1;
    }
    n
}

/// Re-points the injected graph at the live socket groups and gem instances (CCR-0088 §1).
/// Re-points the injected graph at the live socket groups, gem instances and items (CCR-0088
/// §1): groups and gems by the map [`adopt_socket_groups`] fills, items by `item.id` (an item
/// the calc made itself, e.g. Energy Blade's, CalcSetup.lua:1124, has no id and stays a copy).
struct Rebind {
    groups: HashMap<usize, SocketGroupRef>,
    gems: HashMap<usize, GemInstanceRef>,
    /// `build.itemsTab.items`
    items: HashMap<u32, ItemRef>,
    seen: std::collections::HashSet<usize>,
    rebound: usize,
    items_rebound: usize,
}

impl Rebind {
    fn new(build: &Build) -> Self {
        Self {
            groups: HashMap::new(),
            gems: HashMap::new(),
            items: build
                .itemsTab
                .items
                .with(|m| m.iter().map(|(k, v)| (*k, v.clone())).collect()),
            seen: Default::default(),
            rebound: 0,
            items_rebound: 0,
        }
    }

    fn first(&mut self, p: usize) -> bool {
        self.seen.insert(p)
    }

    /// The live item an injected copy stands for (`None`: keep it).
    fn live_item(&mut self, it: &ItemRef) -> Option<ItemRef> {
        let id = it.id.get()?;
        if id < 0.0 || id != id.trunc() {
            return None;
        }
        let live = self.items.get(&(id as u32))?;
        if Rc::ptr_eq(live, it) || live.name.get() != it.name.get() {
            return None;
        }
        self.items_rebound += 1;
        Some(live.clone())
    }

    /// `env.flasks` / `env.tinctures` (CalcSetup.lua:1031, :1047; read by ItemsTab.lua:4868,
    /// :4904 with the live item).
    fn item_flags(&mut self, m: &Shared<PtrMap<Item, bool>>) {
        let pairs = m.with(PtrMap::pairs);
        let mut out = PtrMap::new();
        for (k, v) in pairs {
            let k = self.live_item(&k).unwrap_or(k);
            out.set(&k, Some(v));
        }
        m.with_mut(|x| *x = out);
    }

    fn env(&mut self, env: &EnvRef) {
        self.actor(&env.player);
        self.actor(&env.enemy);
        if let Some(m) = env.minion.get() {
            self.actor(&m);
        }
        self.skills(&env.auxSkillList.get());
        self.item_flags(&env.flasks.get());
        self.item_flags(&env.tinctures.get());
        if let Some(it) = env.aegisItem.get() {
            if let Some(l) = self.live_item(&it) {
                env.aegisItem.set(Some(l));
            }
        }
        for list in [
            &env.radiusJewelList,
            &env.grantedSkills,
            &env.grantedSkillsNodes,
            &env.grantedSkillsItems,
            &env.explodeSources,
        ] {
            for t in list.get().snapshot() {
                self.table(&t);
            }
        }
        self.table(&env.requirementsTableItems.get());
    }

    fn actor(&mut self, a: &ActorRef) {
        if !self.first(rc_ptr(a)) {
            return;
        }
        // `breakdown.X.slots[i].item` (CalcDefence.lua, the armour breakdown) and other items
        // the breakdown tables hold
        if let Some(b) = a.breakdown.get() {
            self.table(&b.t);
        }
        // `actor.itemList[slotName] = item` (CalcSetup.lua:1068)
        let list: Vec<(pob_lua::Name, ItemRef)> = a
            .itemList
            .with(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
        for (k, it) in list {
            if let Some(l) = self.live_item(&it) {
                a.itemList.with_mut(|m| {
                    m.insert(k, l);
                });
            }
        }
        if let Some(s) = a.mainSkill.get() {
            self.skill(&s);
        }
        self.skills(&a.activeSkillList.get());
        if let Some(m) = a.minion.get() {
            self.actor(&m);
        }
    }

    fn skills(&mut self, l: &Shared<Vec<ActiveSkillRef>>) {
        for s in l.snapshot() {
            self.skill(&s);
        }
    }

    fn skill(&mut self, s: &ActiveSkillRef) {
        if !self.first(rc_ptr(s)) {
            return;
        }
        if let Some(g) = s.socketGroup.get() {
            if let Some(l) = self.groups.get(&rc_ptr(&g)) {
                if !Rc::ptr_eq(l, &g) {
                    s.socketGroup.set(Some(l.clone()));
                    self.rebound += 1;
                }
            }
        }
        self.effect(&s.activeEffect.get());
        for e in s.supportList.get().snapshot() {
            self.effect(&e);
        }
        for e in s.effectList.get().snapshot() {
            self.effect(&e);
        }
        if let Some(x) = s.summonSkill.get() {
            self.skill(&x);
        }
        if let Some(m) = s.minion.get() {
            self.actor(&m);
        }
    }

    /// Model objects stored in a dynamic table (and its sub-tables).
    fn table(&mut self, t: &Table) {
        if !self.first(t.as_ptr() as usize) {
            return;
        }
        for (k, v) in t.pairs() {
            if let Some(it) = v.obj::<Item>() {
                if let Some(l) = self.live_item(&it) {
                    t.set(k, Val::Obj(l));
                }
            } else if let Some(g) = v.obj::<GemInstance>() {
                if let Some(l) = self.gems.get(&rc_ptr(&g)) {
                    if !Rc::ptr_eq(l, &g) {
                        t.set(k, Val::Obj(l.clone()));
                        self.rebound += 1;
                    }
                }
            } else if let Some(g) = v.obj::<SocketGroup>() {
                if let Some(l) = self.groups.get(&rc_ptr(&g)) {
                    if !Rc::ptr_eq(l, &g) {
                        t.set(k, Val::Obj(l.clone()));
                        self.rebound += 1;
                    }
                }
            } else if let Some(sub) = v.table() {
                self.table(&sub);
            }
        }
    }

    fn effect(&mut self, e: &SkillEffectRef) {
        if !self.first(rc_ptr(e)) {
            return;
        }
        if let Some(g) = e.srcInstance.get() {
            if let Some(l) = self.gems.get(&rc_ptr(&g)) {
                if !Rc::ptr_eq(l, &g) {
                    e.srcInstance.set(Some(l.clone()));
                    self.rebound += 1;
                }
            }
        }
    }
}

// ---- the facade ----------------------------------------------------------------------------------

#[derive(Default)]
pub struct LuaCalcBridge {
    /// PoB's `src/`; `None`: [`pob_src`]'s search.
    pub pob_src: Option<PathBuf>,
    /// `calcsTab.miscCalculator` of the last `build_output` (CalcsTab.lua:470-471): the build it
    /// was made for and the base output.
    misc: Option<(Weak<Build>, Output)>,
    /// What the last `build_output` adopted.
    pub last_report: Option<AdoptReport>,
}

impl LuaCalcBridge {
    pub fn new(pob_src: Option<PathBuf>) -> Self {
        Self {
            pob_src,
            ..Self::default()
        }
    }

    /// [`CalcFacade::build_output`] with errors as values.
    pub fn try_build_output(&mut self, build: &BuildRef) -> Result<(), String> {
        let src = pob_src(self.pob_src.as_deref())?;
        let xml = save_xml(build)?;
        let docs = {
            let mut s = LuaSession::load(&src, &xml, &session_name(build))?;
            s.dump_build_output()?
        };
        let (base, rep) = adopt_build_output(build, &docs)?;
        self.misc = Some((Rc::downgrade(build), base));
        self.last_report = Some(rep);
        Ok(())
    }
}

/// The build's name in the Lua VM (`Build:Init`'s `buildName`).
fn session_name(build: &Build) -> String {
    pob_calc::model::build_xml::dbFileName(build)
        .and_then(|n| {
            Path::new(&*n)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "Unnamed build".to_owned())
}

impl CalcFacade for LuaCalcBridge {
    /// lua: Classes/CalcsTab.lua:441-472.
    /// Run by PoB in the second VM; the result is adopted into the model (module doc).
    fn build_output(&mut self, build: &BuildRef) -> Poll<()> {
        if let Err(e) = self.try_build_output(build) {
            lua_error!("{e}")
        }
        Poll::Ready(())
    }

    /// lua: Classes/CalcsTab.lua:760-762.
    fn misc_calculator(&mut self, build: &BuildRef) -> Poll<(Rc<dyn MiscCalc>, Output)> {
        // L761 return self.miscCalculator[1], self.miscCalculator[2]
        let Some((_, base)) = self
            .misc
            .as_ref()
            .filter(|(b, _)| b.upgrade().is_some_and(|b| Rc::ptr_eq(&b, build)))
        else {
            lua_error!("attempt to index field 'miscCalculator' (a nil value)")
        };
        let calc: Rc<dyn MiscCalc> = Rc::new(BridgeMiscCalc {
            build: build.clone(),
            pob_src: self.pob_src.clone(),
            report: RefCell::new(None),
        });
        Poll::Ready((calc, base.clone()))
    }
}

/// The Lua `calcFunc` closure (Calcs.lua:96-112), run in a fresh VM per call.
pub struct BridgeMiscCalc {
    pub build: BuildRef,
    pub pob_src: Option<PathBuf>,
    /// Injection diagnostics of the last call.
    pub report: RefCell<Option<AdoptReport>>,
}

impl BridgeMiscCalc {
    /// [`MiscCalc::call`] with errors as values.
    pub fn try_call(
        &self,
        ov: Option<&Rc<Override>>,
        use_full_dps: Option<bool>,
    ) -> Result<Output, String> {
        let src = pob_src(self.pob_src.as_deref())?;
        let body = override_lua(&self.build, ov.map(|o| &**o))?;
        let xml = save_xml(&self.build)?;
        let doc = {
            let mut s = LuaSession::load(&src, &xml, &session_name(&self.build))?;
            s.call_misc(&body, use_full_dps)?
        };
        let mut rep = AdoptReport::default();
        let i = inj(&doc, &self.build, &mut rep)?;
        // CalcSetup.lua:1921-1922 the calc appends to override.conditions
        if let Some(ov) = ov {
            if let Some(c) = i.locals.get("conditions").and_then(Val::table) {
                let names: Vec<pob_lua::Name> = c
                    .ipairs()
                    .into_iter()
                    .filter_map(|(_, v)| v.str().map(|s| pob_lua::Name::from(&s)))
                    .collect();
                match ov.conditions.get() {
                    Some(list) => list.with_mut(|v| *v = names),
                    None => ov.conditions.set(Some(Shared::new(names))),
                }
            }
        }
        let out = i
            .locals
            .get("output")
            .and_then(Val::table)
            .ok_or_else(|| "LuaCalcBridge: calcFunc returned no output table".to_owned())?;
        // the output's model objects (`ReqStrItem.sourceGem`, …) are the live build's: the call
        // VM loaded the live model, so its groups and gems match by position (CCR-0088 §1)
        let mut rb = Rebind::new(&self.build);
        let live = self.build.skillsTab.socketGroupList.get().snapshot();
        for (g, l) in doc_list::<SocketGroup>(i.locals.get("socketGroupList"))
            .iter()
            .zip(&live)
        {
            rb.groups.insert(doc_ptr(g), l.clone());
            for (gem, lg) in doc_gems(g).iter().zip(l.gemList.get().snapshot()) {
                rb.gems.insert(doc_ptr(gem), lg);
            }
        }
        rb.table(&out);
        rep.rebound = rb.rebound;
        rep.items_rebound = rb.items_rebound;
        *self.report.borrow_mut() = Some(rep);
        Ok(out)
    }
}

impl MiscCalc for BridgeMiscCalc {
    /// lua: Modules/Calcs.lua:96-112.
    fn call(&self, ov: Option<Rc<Override>>, use_full_dps: Option<bool>) -> Poll<Output> {
        match self.try_call(ov.as_ref(), use_full_dps) {
            Ok(o) => Poll::Ready(o),
            Err(e) => lua_error!("{e}"),
        }
    }
}
