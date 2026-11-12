//! U38 acceptance (ui doc §12 U38 row: ItemsTab tooltips, ItemsTab.lua:3993-5055; "tooltips for
//! 100 uniques; stat-diff blocks through the bridge"). Owner: U38.
//!
//! Both sides run the full app (Launch, the build shell, the Items tab built by U33's
//! constructor) on a fixture build, PoB under LuaJIT on one side and the Rust app with U15's
//! `LuaCalcBridge` on the other; after the frames, both sides fill tooltips with
//! `itemsTab:AddItemTooltip` (PoB's / U38's) and the tests compare every tooltip field the draw
//! reads: the header fields (`tooltipHeader`, `influenceHeader1/2`, `foilType`, `center`,
//! `maxWidth`, `color`), every line (size, text, font, block, modLine presence, background,
//! separator image presence), the block heights and the child tooltips.
//!
//! * `u38_unique_tooltips`: 100 uniques of `main.uniqueDB` (every `#list/100`-th name in byte
//!   order), each with `dbMode` true (the ItemDB's call) and false, with
//!   `showStatDifferences = false` on both sides (the Ctrl+D tip; no calc). Fixtures:
//!   `U38_TT_FIXTURES` (substring filter; default: the first two).
//! * `u38_stat_differences`: the build's own items (by id; equipped ones with their slot), with
//!   `showStatDifferences = true`: the flask/tincture effective stats and toggle compares and the
//!   per-slot "Equipping this item in ..." blocks, every `calcFunc` call through the bridge.
//!   `U38_SD_MAX` items per build (default 8), `U38_SD_FIXTURES` (substring; default all six).
//!   Jewels are skipped when the build has a jewel that changes the passive tree (the bridge
//!   cannot carry `override.spec` built by `buildSpecForJewelComparison`, CCR-0125 §6).
//! * `u38_undo_round_trip`: `AddUndoState` / an equip change / `Undo` / `Redo` on the Rust side
//!   restores the item map, the order list, the slot ids and the item sets (L5021-5055).
//!
//! Tooltips whose lines are equal as a multiset are reported, not failed: the limited-unique
//! branch adds its compare blocks in `pairs(self.slots)` order (ORDER (b), tooltip.rs).

#![allow(
    clippy::too_many_lines,
    clippy::format_push_string,
    clippy::case_sensitive_file_extension_comparisons
)]

use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_app::app::CxApp;
use pob_app::model::{self, Val};
use pob_app::tabs::items::slot::ItemSlotControl;
use pob_app::tabs::items::slot_helper;
use pob_app::tabs::items::tooltip as tt38;
use pob_app::tabs::items::ItemsTab;
use pob_host::{App, PlatformParts};
use pob_lua::n;
use pob_sg::{Key, Sg, Sys};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::{TtBackground, TtColor, Tooltip};
use pob_ui_oracle::boot::{self, Entry, OracleHandles};
use pob_ui_oracle::lua_calc::LuaCalcBridge;
use pob_ui_oracle::oracle_cache::{self, LuaRun};
use pob_ui_oracle::runner::{self, Driver, LuaApp, ScenarioApp, UserDir};

// TP4: the fixture builds of `u38_unique_tooltips` and `u38_stat_differences` run as independent
// cases on `oracle_cache::par_map` (the process-wide case pool replaces the old one-PoB-at-a-time
// lock), and the Lua side of each (the PoB boot, the build and the test's query on the VM) goes
// through the Lua-side cache (`lua_query`). Results are collected in build order.

fn inputs() -> Option<PathBuf> {
    let root = pob_testpaths::require(pob_testpaths::pob_root(), "PoB checkout (.PathOfBuilding)")?;
    let dump = pob_testpaths::require(pob_testpaths::dump_dir(), "data/dump")?;
    assert!(pob_oracle::testkit::init_data(&dump), "data dump");
    let src = std::fs::canonicalize(root.join("src")).unwrap_or_else(|_| root.join("src"));
    pob_gamedata::timeless_lut::set_jewel_fs(std::sync::Arc::new(
        pob_gamedata::timeless_lut::StdJewelFs {
            script_path: src.to_string_lossy().into_owned(),
            writable: false,
        },
    ));
    Some(root)
}

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixture_dir() -> PathBuf {
    crate_dir().join("fixtures/user1")
}

fn fixture_builds() -> Vec<String> {
    let dir = fixture_dir().join("Path of Building/Builds");
    let mut v: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".xml"))
        .collect();
    v.sort();
    assert_eq!(v.len(), 6, "the 6 fixture builds");
    v
}

// ---- the two sides -----------------------------------------------------------------------------

struct LuaSide {
    app: LuaApp,
}

impl App for LuaSide {
    fn sg(&mut self) -> &mut Sg {
        self.app.sg()
    }
    fn sys(&mut self) -> &mut Sys {
        self.app.sys()
    }
    fn on_init(&mut self) {
        self.app.on_init();
    }
    fn on_frame(&mut self) {
        self.app.on_frame();
    }
    fn on_key_down(&mut self, key: Key, double_click: bool) {
        self.app.on_key_down(key, double_click);
    }
    fn on_key_up(&mut self, key: Key) {
        self.app.on_key_up(key);
    }
    fn on_char(&mut self, ch: Key) {
        self.app.on_char(ch);
    }
    fn on_lua_error(&mut self, msg: String) {
        self.app.on_lua_error(msg);
    }
}

impl ScenarioApp for LuaSide {
    fn load_build(&mut self, name: &str, xml: &str) -> anyhow::Result<()> {
        self.app.load_build(name, xml)
    }
    fn snapshot(&mut self) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
    fn take_error(&mut self) -> Option<String> {
        self.app.take_error()
    }
}

struct RustSide {
    app: pob_app::PobApp,
}

impl App for RustSide {
    fn sg(&mut self) -> &mut Sg {
        self.app.sg()
    }
    fn sys(&mut self) -> &mut Sys {
        self.app.sys()
    }
    fn on_init(&mut self) {
        self.app.on_init();
    }
    fn on_frame(&mut self) {
        self.app.on_frame();
    }
    fn on_key_down(&mut self, key: Key, double_click: bool) {
        self.app.on_key_down(key, double_click);
    }
    fn on_key_up(&mut self, key: Key) {
        self.app.on_key_up(key);
    }
    fn on_char(&mut self, ch: Key) {
        self.app.on_char(ch);
    }
    fn on_lua_error(&mut self, msg: String) {
        self.app.on_lua_error(msg);
    }
}

impl ScenarioApp for RustSide {
    fn load_build(&mut self, name: &str, xml: &str) -> anyhow::Result<()> {
        self.app.load_build(name, xml);
        Ok(())
    }
    fn snapshot(&mut self) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
    fn after_frame(&mut self) -> anyhow::Result<()> {
        if let Some(p) = self
            .app
            .cx
            .app()
            .launch
            .prompt_msg
            .clone()
            .filter(|p| p.starts_with("^1Error"))
        {
            anyhow::bail!("the error prompt (launch:ShowErrMsg): {p}");
        }
        Ok(())
    }
}

fn script_for(name: &str) -> pob_host::script::Script {
    runner::parse_script(&format!(
        "frames 1\nload-build \"Builds/{name}\"\nframes 3\n"
    ))
    .unwrap()
}

fn run_lua(root: &Path, name: &str, user: &UserDir) -> LuaSide {
    let handles = OracleHandles::default();
    let app = runner::lua_side(root, user, &handles, Entry::Launch, false).expect("Lua side");
    let mut side = LuaSide { app };
    let mut d = Driver::new(runner::screen(1280, 800, 1.0), handles, user.path.clone());
    let (_frames, r) = runner::drive(&mut side, &script_for(name), &mut d);
    if let Err(e) = r {
        panic!("{name}: Lua: {e}");
    }
    side
}

/// The Lua side of one build through the Lua-side cache (TP4): [`run_lua`] and `query` (a chunk
/// returning a list of strings) on its VM. The key covers this file, the Lua-side code, PoB, the
/// fixture user dir, the build name and the query. `kind` names the test.
fn lua_query(root: &Path, name: &str, user: &UserDir, kind: &str, query: &str) -> Vec<String> {
    let fixture = fixture_dir();
    let key = oracle_cache::ui_key(
        kind,
        pob_ui_oracle::test_source!(),
        root,
        &[&fixture],
        &[name, query],
    );
    oracle_cache::cached_lua_run(key, || {
        let mut lua_side = run_lua(root, name, user);
        let lua: Vec<String> = lua_side
            .app
            .with_vm(|pob| pob.eval::<Vec<String>>(query))
            .map_err(|e| anyhow::anyhow!("Lua tooltips: {e}"))?;
        drop(lua_side);
        Ok(LuaRun {
            extra: lua,
            ..LuaRun::default()
        })
    })
    .unwrap_or_else(|e| panic!("{e:#}"))
    .extra
}

/// One build's outcome (TP4: collected in build order).
#[derive(Default)]
struct BuildOut {
    lines: Vec<String>,
    failures: Vec<String>,
    tooltips: usize,
    n2: usize,
    unordered: usize,
    errors: usize,
}

/// U25 stand-in (TreeTab.lua:492-498, as u20/u22/u32/u33): the spec list labels.
fn rust_stubs() {
    pob_app::build::init::set_hooks(|h| {
        *h = pob_app::build::init::default_hooks();
        h.tree_get_spec_list = Rc::new(|cx| {
            let m = &cx.build().model;
            let gv = &m.data.gameVersions;
            m.treeTab
                .specList
                .with(Clone::clone)
                .iter()
                .map(|spec| {
                    let tv = spec.treeVersion.get();
                    let prefix = if tv == gv.latestTreeVersion {
                        String::new()
                    } else {
                        format!("[{}] ", gv.treeVersions[&tv].display)
                    };
                    let title = spec
                        .title
                        .get()
                        .map_or_else(|| "Default".to_owned(), |s| s.to_string());
                    format!("{prefix}{title}")
                })
                .collect()
        });
    });
}

fn run_rust(root: &Path, name: &str, user: &UserDir) -> RustSide {
    rust_stubs();
    user.reset().expect("user dir");
    let handles = OracleHandles::default();
    let (sg, sys) = boot::oracle_parts(root, &user.path, &handles).expect("oracle parts");
    let src = root.join("src");
    let mut app = pob_app::PobApp::with_calc(
        PlatformParts { sg, sys },
        Rc::new(move || Box::new(LuaCalcBridge::new(Some(src.clone())))),
    );
    app.cx.app_mut().launch.force_release_install = true;
    let mut side = RustSide { app };
    let mut d = Driver::new(runner::screen(1280, 800, 1.0), handles, user.path.clone());
    let (_frames, r) = runner::drive(&mut side, &script_for(name), &mut d);
    if let Err(e) = r {
        panic!("{name}: Rust: {e}");
    }
    side
}

/// Run `f` on a fresh thread (the Rust side's process-global state lives in thread-locals).
fn in_thread<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> R {
    std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(f)
        .expect("spawn")
        .join()
        .unwrap_or_else(|p| std::panic::resume_unwind(p))
}

// ---- serialisation (the same text on both sides) -----------------------------------------------

const LUA_SER: &str = r#"
local function ser(tt, head)
	local out = { head, "header=" .. tostring(tt.tooltipHeader) .. " infl=" .. tostring(tt.influenceHeader1) .. "/" .. tostring(tt.influenceHeader2) .. " foil=" .. tostring(tt.foilType) .. " center=" .. tostring(tt.center) .. " maxWidth=" .. tostring(tt.maxWidth) .. " color=" .. tostring(tt.color) }
	for _, l in ipairs(tt.lines) do
		out[#out + 1] = tostring(l.size) .. "|" .. tostring(l.text) .. "|" .. tostring(l.font) .. "|" .. tostring(l.block) .. (l.modLine and "|M" or "") .. (l.background and ("|bg=" .. tostring(l.background)) or "") .. (l.separatorImage and "|sep" or "")
	end
	for i, b in ipairs(tt.blocks) do
		out[#out + 1] = "block " .. i .. " " .. tostring(b.height)
	end
	for i, c in ipairs(tt.childTooltips or { }) do
		out[#out + 1] = "child " .. i .. " lines " .. #c.lines
		for _, l in ipairs(c.lines) do
			out[#out + 1] = "  " .. tostring(l.size) .. "|" .. tostring(l.text) .. "|" .. tostring(l.font)
		end
	end
	return table.concat(out, "\n")
end
local function errmsg(err)
	return (tostring(err):gsub("^[^:]*:%d+: ", ""))
end
"#;

fn rust_ser(t: &Tooltip, head: &str) -> String {
    let num = pob_lua::tostring_num;
    let opt = |s: &Option<String>| s.clone().unwrap_or_else(|| "nil".to_owned());
    let color = match &t.color {
        Some(TtColor::Esc(s)) => s.clone(),
        Some(TtColor::Rgb(_)) => "table".to_owned(),
        None => "nil".to_owned(),
    };
    let mut out = vec![
        head.to_owned(),
        format!(
            "header={} infl={}/{} foil={} center={} maxWidth={} color={color}",
            opt(&t.tooltip_header),
            opt(&t.influence_header1),
            opt(&t.influence_header2),
            opt(&t.foil_type),
            t.center,
            t.max_width.map_or("nil".to_owned(), num),
        ),
    ];
    for l in &t.lines {
        let bg = match &l.background {
            Some(TtBackground::Image(s) | TtBackground::Color(s)) => format!("|bg={s}"),
            None => String::new(),
        };
        out.push(format!(
            "{}|{}|{}|{}{}{}{}",
            num(l.size),
            l.text.as_deref().unwrap_or("nil"),
            l.font.map_or("nil", pob_sg::Font::lua_name),
            l.block,
            if l.mod_line.is_some() { "|M" } else { "" },
            bg,
            if l.separator_image.is_some() { "|sep" } else { "" },
        ));
    }
    for (i, b) in t.blocks.iter().enumerate() {
        out.push(format!("block {} {}", i + 1, num(b.height)));
    }
    for (i, c) in t.child_tooltips.iter().flatten().enumerate() {
        out.push(format!("child {} lines {}", i + 1, c.lines.len()));
        for l in &c.lines {
            out.push(format!(
                "  {}|{}|{}",
                num(l.size),
                l.text.as_deref().unwrap_or("nil"),
                l.font.map_or("nil", pob_sg::Font::lua_name)
            ));
        }
    }
    out.join("\n")
}

/// `f` under a Lua-error catch: `Err(message)` when it raises.
fn catch_lua(f: impl FnOnce()) -> Result<(), String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(()) => Ok(()),
        Err(p) => Err(p
            .downcast_ref::<pob_lua::LuaError>()
            .map_or_else(|| pob_host::panic_message(&*p), |e| e.0.clone())),
    }
}

/// Compare the two lists of serialised tooltips; returns (failures, multiset-only differences,
/// errors on both sides).
fn compare(
    tag: &str,
    lua: &[String],
    rust: &[String],
    failures: &mut Vec<String>,
) -> (usize, usize) {
    let (mut unordered, mut errors) = (0, 0);
    let mut bad = 0;
    for i in 0..lua.len().max(rust.len()) {
        let (l, r) = (lua.get(i), rust.get(i));
        let is_err = |t: Option<&String>| {
            t.and_then(|t| t.lines().next())
                .is_some_and(|h| h.contains(" ERROR "))
        };
        if is_err(l) && is_err(r) {
            errors += 1;
            println!(
                "{tag} #{i}: both raise: lua {:?} / rust {:?}",
                l.and_then(|t| t.lines().next()),
                r.and_then(|t| t.lines().next())
            );
            continue;
        }
        if l == r {
            continue;
        }
        if let (Some(l), Some(r)) = (l, r) {
            let mut a: Vec<&str> = l.lines().collect();
            let mut b: Vec<&str> = r.lines().collect();
            a.sort_unstable();
            b.sort_unstable();
            if a == b {
                unordered += 1;
                println!(
                    "{tag} #{i}: the same lines in another order (ORDER (b)): {:?}",
                    l.lines().next()
                );
                continue;
            }
        }
        bad += 1;
        if bad <= 6 {
            let (l, r) = (l.cloned().unwrap_or_default(), r.cloned().unwrap_or_default());
            let first = l
                .lines()
                .zip(r.lines())
                .position(|(a, b)| a != b)
                .unwrap_or_else(|| l.lines().count().min(r.lines().count()));
            let ctx = |s: &str| {
                s.lines()
                    .skip(first.saturating_sub(2))
                    .take(6)
                    .collect::<Vec<_>>()
                    .join("\n      ")
            };
            failures.push(format!(
                "{tag} #{i} ({}), first differing line {first}:\n  lua:  {}\n  rust: {}",
                l.lines().next().unwrap_or(""),
                ctx(&l),
                ctx(&r)
            ));
        }
    }
    if bad > 6 {
        failures.push(format!("{tag}: {bad} differing tooltips in all"));
    }
    (unordered, errors)
}

// ---- u38_unique_tooltips -----------------------------------------------------------------------

const N_UNIQUES: usize = 100;

fn lua_unique_tooltips() -> String {
    format!(
        r#"{LUA_SER}
local b = main.modes.BUILD
local it = b.itemsTab
it.showStatDifferences = false
local names = {{ }}
for name in pairs(main.uniqueDB.list) do names[#names + 1] = name end
table.sort(names)
local step = #names / {N_UNIQUES}
local picked = {{ }}
for i = 0, {N_UNIQUES} - 1 do picked[#picked + 1] = names[math.floor(i * step) + 1] end
local res = {{ tostring(main.uniqueDB.loading) .. " " .. #names, table.concat(picked, "\n") }}
for _, name in ipairs(picked) do
	for _, db in ipairs({{ true, false }}) do
		local tt = new("Tooltip"):Tooltip()
		local head = name .. " db=" .. tostring(db)
		local ok, err = pcall(it.AddItemTooltip, it, tt, main.uniqueDB.list[name], nil, db)
		res[#res + 1] = ok and ser(tt, head) or (head .. " ERROR " .. errmsg(err))
	end
end
return res
"#
    )
}

fn rust_unique_tooltips(side: &mut RustSide, picked: &[String]) -> (String, Vec<String>) {
    let cx = &mut side.app.cx;
    let m = cx.build().model.clone();
    pob_calc::model::build_xml::build_extra(&m)
        .get_or_new_table(n!("itemsTab"))
        .set(n!("showStatDifferences"), false);
    let me = slot_helper::items_tab(cx).expect("the Items tab");
    let db = &cx.app().main.unique_db;
    let head = format!("{} {}", db.loading, db.list.len());
    let items: Vec<(String, Option<model::ItemRef>)> = picked
        .iter()
        .map(|n| (n.clone(), db.list.get(n).cloned()))
        .collect();
    let mut res = Vec::new();
    for (name, item) in items {
        for dbm in [true, false] {
            let head = format!("{name} db={dbm}");
            let Some(item) = item.clone() else {
                res.push(format!("{head} MISSING (not in the Rust unique DB)"));
                continue;
            };
            let mut t = Tooltip::default();
            let r = catch_lua(|| {
                tt38::AddItemTooltip(cx, me, &mut t, &item, &Val::Nil, dbm, None);
            });
            res.push(match r {
                Ok(()) => rust_ser(&t, &head),
                Err(e) => format!("{head} ERROR {e}"),
            });
        }
    }
    (head, res)
}

#[test]
fn u38_unique_tooltips() {
    let Some(root) = inputs() else { return };
    let filter = std::env::var("U38_TT_FIXTURES").ok();
    let builds: Vec<String> = fixture_builds()
        .into_iter()
        .enumerate()
        .filter(|(i, n)| filter.as_ref().map_or(*i < 2, |f| n.contains(f.as_str())))
        .map(|(_, n)| n)
        .collect();
    assert!(!builds.is_empty(), "no fixture selected");
    // ORDER (b): uniques with more than one flavour entry (PoB picks one in `pairs` order)
    let multi_flavour: Vec<&String> = tt38::flavour_lookup()
        .iter()
        // Grand Spectrum picks its entry by its lines (L4545-4580), not in `pairs` order
        .filter(|(k, v)| v.len() > 1 && k.as_str() != "Grand Spectrum")
        .map(|(k, _)| k)
        .collect();
    println!("flavour lookup: {} names, {} with several entries: {multi_flavour:?}", tt38::flavour_lookup().len(), multi_flavour.len());
    let results = oracle_cache::par_map(builds, |name| {
        let mut out = BuildOut::default();
        let user = UserDir::new(Some(&fixture_dir())).unwrap();
        let mut lua = lua_query(&root, &name, &user, "ui-u38-unique", &lua_unique_tooltips());
        let lua_head = lua.remove(0);
        let picked: Vec<String> = lua.remove(0).lines().map(str::to_owned).collect();
        assert_eq!(picked.len(), N_UNIQUES, "{name}: picked uniques");
        // Main.lua sets `loading = nil` once done (the Rust field reads false)
        assert!(
            lua_head.starts_with("false ") || lua_head.starts_with("nil "),
            "{name}: Lua unique DB still loading: {lua_head}"
        );
        let (rust_head, rust) = {
            let (root, name, picked) = (root.clone(), name.clone(), picked.clone());
            in_thread(move || {
                let mut side = run_rust(&root, &name, &user);
                rust_unique_tooltips(&mut side, &picked)
            })
        };
        out.lines
            .push(format!("{name}: unique DB lua {lua_head} / rust {rust_head}"));
        // the flavour-order cases: drop them from the comparison
        let skip: Vec<usize> = picked
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                let title = lua
                    .iter()
                    .find(|t| t.starts_with(p.as_str()))
                    .and_then(|t| t.lines().nth(2))
                    .unwrap_or("");
                multi_flavour.iter().any(|m| title.contains(m.as_str()))
            })
            .flat_map(|(i, _)| [2 * i, 2 * i + 1])
            .collect();
        if !skip.is_empty() {
            out.lines.push(format!(
                "{name}: {} tooltips skipped (several flavour entries)",
                skip.len()
            ));
        }
        let keep = |v: &[String]| -> Vec<String> {
            v.iter()
                .enumerate()
                .filter(|(i, _)| !skip.contains(i))
                .map(|(_, t)| t.clone())
                .collect()
        };
        let (u, e) = compare(&name, &keep(&lua), &keep(&rust), &mut out.failures);
        out.unordered += u;
        out.errors += e;
        out.tooltips += lua.len();
        out.n2 += lua.iter().map(|t| t.lines().count() - 2).sum::<usize>();
        out.lines
            .push(format!("{name}: {} tooltips compared", lua.len() - skip.len()));
        out
    });
    let mut failures = Vec::new();
    let (mut tooltips, mut lines, mut unordered, mut errors) = (0, 0, 0, 0);
    for b in results {
        for l in &b.lines {
            println!("{l}");
        }
        failures.extend(b.failures);
        tooltips += b.tooltips;
        lines += b.n2;
        unordered += b.unordered;
        errors += b.errors;
    }
    println!("{}", oracle_cache::summary());
    println!(
        "u38_unique_tooltips: {tooltips} tooltips, {lines} lines, {unordered} order-only, {errors} raise on both sides, {} failures",
        failures.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---- u38_stat_differences ----------------------------------------------------------------------

fn lua_stat_differences(max: usize) -> String {
    format!(
        r#"{LUA_SER}
local b = main.modes.BUILD
local it = b.itemsTab
it.showStatDifferences = true
local function changesTree(item)
	return item and item.type == "Jewel" and item.jewelData and (item.jewelData.conqueredBy or item.jewelRadiusIndex and (item.jewelData.intuitiveLeapLike or item.jewelData.impossibleEscapeKeystone))
end
local treeJewel = false
for _, slot in pairs(it.sockets) do
	if changesTree(it.items[slot.selItemId]) then treeJewel = true end
end
local slotNames = {{ }}
for name in pairs(it.slots) do slotNames[#slotNames + 1] = name end
table.sort(slotNames)
local slotOf = {{ }}
for _, name in ipairs(slotNames) do
	local slot = it.slots[name]
	if slot.selItemId ~= 0 and not slot.inactive and not slotOf[slot.selItemId] then slotOf[slot.selItemId] = name end
end
local ids = {{ }}
for id in pairs(it.items) do ids[#ids + 1] = id end
table.sort(ids)
local picked, jewels = {{ }}, 0
for _, id in ipairs(ids) do
	local item = it.items[id]
	local ok = true
	if item.type == "Jewel" then
		ok = not treeJewel and not changesTree(item) and jewels < 2
		if ok then jewels = jewels + 1 end
	end
	if ok and #picked < {max} then picked[#picked + 1] = id end
end
local list = {{ }}
for _, id in ipairs(picked) do list[#list + 1] = id .. "=" .. (slotOf[id] or "") end
local res = {{ table.concat(list, "\n") }}
for _, id in ipairs(picked) do
	local tt = new("Tooltip"):Tooltip()
	local slot = slotOf[id] and it.slots[slotOf[id]] or nil
	local head = "item " .. id .. " " .. it.items[id].name .. " slot=" .. tostring(slotOf[id])
	local ok, err = pcall(it.AddItemTooltip, it, tt, it.items[id], slot, false)
	res[#res + 1] = ok and ser(tt, head) or (head .. " ERROR " .. errmsg(err))
end
return res
"#
    )
}

fn rust_stat_differences(side: &mut RustSide, picked: &[(u32, String)]) -> Vec<String> {
    let cx = &mut side.app.cx;
    let m = cx.build().model.clone();
    pob_calc::model::build_xml::build_extra(&m)
        .get_or_new_table(n!("itemsTab"))
        .set(n!("showStatDifferences"), true);
    let me = slot_helper::items_tab(cx).expect("the Items tab");
    let mut res = Vec::new();
    for (id, slot_name) in picked {
        let item = m
            .itemsTab
            .items
            .with(|i| i.get(id).cloned())
            .unwrap_or_else(|| panic!("item {id} not in the Rust build"));
        let slot = if slot_name.is_empty() {
            Val::Nil
        } else {
            let c = cx.w::<ItemsTab>(me).ctor.slots[&pob_lua::Name::from(slot_name.as_str())];
            Val::Table(cx.w::<ItemSlotControl>(c).slot.clone())
        };
        let head = format!(
            "item {id} {} slot={}",
            item.name.get(),
            if slot_name.is_empty() { "nil" } else { slot_name }
        );
        let mut t = Tooltip::default();
        let r = catch_lua(|| {
            tt38::AddItemTooltip(cx, me, &mut t, &item, &slot, false, None);
        });
        res.push(match r {
            Ok(()) => rust_ser(&t, &head),
            Err(e) => format!("{head} ERROR {e}"),
        });
    }
    res
}

#[test]
fn u38_stat_differences() {
    let Some(root) = inputs() else { return };
    let filter = std::env::var("U38_SD_FIXTURES").ok();
    let max: usize = std::env::var("U38_SD_MAX")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8);
    let names: Vec<String> = fixture_builds()
        .into_iter()
        .filter(|name| !filter.as_ref().is_some_and(|f| !name.contains(f.as_str())))
        .collect();
    let ran = names.len();
    let results = oracle_cache::par_map(names, |name| {
        let mut out = BuildOut::default();
        let t0 = std::time::Instant::now();
        let user = UserDir::new(Some(&fixture_dir())).unwrap();
        let mut lua = lua_query(&root, &name, &user, "ui-u38-sd", &lua_stat_differences(max));
        let picked: Vec<(u32, String)> = lua
            .remove(0)
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| {
                let (id, slot) = l.split_once('=').expect("id=slot");
                (id.parse().expect("integral item id"), slot.to_owned())
            })
            .collect();
        let t1 = std::time::Instant::now();
        let rust = {
            let (root, name, picked) = (root.clone(), name.clone(), picked.clone());
            in_thread(move || {
                let mut side = run_rust(&root, &name, &user);
                rust_stat_differences(&mut side, &picked)
            })
        };
        let (u, e) = compare(&name, &lua, &rust, &mut out.failures);
        out.unordered += u;
        out.errors += e;
        out.tooltips += lua.len();
        let n_cmp = lua
            .iter()
            .map(|t| {
                t.lines()
                    .filter(|l| l.contains("will give you:"))
                    .count()
            })
            .sum::<usize>();
        out.n2 += n_cmp;
        out.lines.push(format!(
            "{name}: {} items ({}), {n_cmp} compare headers; Lua {:.1}s, Rust (bridge) {:.1}s",
            lua.len(),
            picked
                .iter()
                .map(|(i, s)| format!("{i}{}", if s.is_empty() { String::new() } else { format!("@{s}") }))
                .collect::<Vec<_>>()
                .join(" "),
            (t1 - t0).as_secs_f64(),
            t1.elapsed().as_secs_f64()
        ));
        out
    });
    let mut failures = Vec::new();
    let (mut tooltips, mut compares, mut unordered, mut errors) = (0, 0, 0, 0);
    for b in results {
        for l in &b.lines {
            println!("{l}");
        }
        failures.extend(b.failures);
        tooltips += b.tooltips;
        compares += b.n2;
        unordered += b.unordered;
        errors += b.errors;
    }
    println!("{}", oracle_cache::summary());
    assert!(ran > 0, "no fixture selected");
    println!(
        "u38_stat_differences: {ran} builds, {tooltips} tooltips, {compares} compare headers, {unordered} order-only, {errors} raise on both sides, {} failures",
        failures.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---- u38_undo_round_trip -----------------------------------------------------------------------

#[test]
fn u38_undo_round_trip() {
    // TP4: the process-wide case pool (shared with the `par_map` cases) instead of the lock
    let _permit = oracle_cache::Permit::acquire();
    let Some(root) = inputs() else { return };
    let name = fixture_builds().remove(0);
    let user = UserDir::new(Some(&fixture_dir())).unwrap();
    in_thread(move || {
        let mut side = run_rust(&root, &name, &user);
        let cx = &mut side.app.cx;
        let m = cx.build().model.clone();
        let me = slot_helper::items_tab(cx).expect("the Items tab");
        let snapshot = |cx: &Cx| {
            let t = &m.itemsTab;
            let items: Vec<(u32, String)> = t
                .items
                .with(|i| i.iter().map(|(k, v)| (*k, v.name.get().to_string())).collect());
            let order = t.orderList.with(Clone::clone);
            let slots: Vec<(String, String)> = cx
                .w::<ItemsTab>(me)
                .ctor
                .slots
                .iter()
                .map(|(k, c)| {
                    let v = cx.w::<ItemSlotControl>(*c).slot.get(n!("selItemId"));
                    (k.to_string(), format!("{v:?}"))
                })
                .collect();
            let sets = t.itemSets.with(|s| s.len());
            (items, order, slots, sets, t.activeItemSetId.get())
        };
        tt38::reset_undo(cx, me);
        let before = snapshot(cx);
        // an edit: unequip the first equipped non-socket slot, then record it
        let (slot_name, slot_table) = cx
            .w::<ItemsTab>(me)
            .ctor
            .slots
            .iter()
            .map(|(k, c)| (k.to_string(), cx.w::<ItemSlotControl>(*c).slot.clone()))
            .find(|(_, t)| {
                !t.get(n!("nodeId")).truthy() && !t.get(n!("selItemId")).raw_eq(&Val::Num(0.0))
            })
            .expect("an equipped slot");
        pob_app::tabs::items::slot::set_sel_item_id(&m, &slot_table, 0.0);
        tt38::add_undo_state_hook(cx);
        assert!(tt38::mod_flag(cx), "AddUndoState sets modFlag");
        let edited = snapshot(cx);
        assert_ne!(before, edited, "the edit changes the snapshot");
        let first_item = m.itemsTab.items.with(|i| i.values().next().cloned()).expect("an item");
        tt38::undo(cx, me);
        let undone = snapshot(cx);
        assert_eq!(before, undone, "Undo restores {slot_name}");
        let restored = m.itemsTab.items.with(|i| i.values().next().cloned()).expect("an item");
        assert!(
            !Rc::ptr_eq(&first_item, &restored),
            "the restored items are the state's copies (copyTableSafe)"
        );
        tt38::redo(cx, me);
        assert_eq!(edited, snapshot(cx), "Redo re-applies the edit");
        println!(
            "u38_undo_round_trip: {} items, {} slots; undo/redo of {slot_name} restored",
            before.0.len(),
            before.2.len()
        );
    });
}
