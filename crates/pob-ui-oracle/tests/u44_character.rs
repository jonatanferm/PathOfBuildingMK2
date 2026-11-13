//! U44 acceptance (ui doc §12 U44 row: "Account import flows against recorded HTTP fixtures
//! produce identical builds (T4)"): the full app (Launch, the build list, the build shell, the
//! Import tab) on both sides of the UI oracle. Owner: U44. The harness is u43's
//! (`tests/u43_import.rs`, U43's), copied: test files cannot share modules.
//!
//! `u44_scenarios`: every `tests/u44/*.pobscript` at 1280x800 @1.0 and 1920x1080 @1.5 (both
//! 1280 virtual px wide; a script that sets its own `screen` runs once). Each script opens a
//! build from the real build list, switches to the Import tab (Ctrl+I) and drives the "Import By
//! Account Name" section against `tests/u44/net.json` (recorded site responses: the character
//! list, the profile page, `get-passive-skills` and `get-items` for PoB's own
//! `spec/System/SampleCharacter.json`, and error responses): Start (DownloadSiteCharacterList,
//! BuildCharacterList, SaveAccountHistory), the league dropdown, "Passive Tree and Jewels"
//! (with the confirm popup: DownloadPassiveTree, ImportPassiveTreeAndJewels) and "Items and
//! Skills" (DownloadItems, ImportItemsAndSkills, ImportItem, ImportSocketedItems, the reimport
//! state). Per run: zero T3 divergences (the same draw ops per `(layer, sub)` bucket and the
//! same side effects, frame by frame), the same window titles, and T4: every `snapshot` step's
//! build XML identical between the sides after u20/u22's normalisation.
//!
//! Directives: `#! u44-expect-label TEXT` (some Lua DrawString in the run contains TEXT: guards a
//! scenario whose input silently stopped reaching the tab, which both sides would agree on),
//! `#! u44-expect-import ATTR=VALUE` (a Lua snapshot's `Import` element has it),
//! `#! u44-expect-copy` (the Lua side copied a build code that decodes to a `PathOfBuilding`
//! document), `#! u44-expect-xml TEXT` (the last Lua snapshot contains TEXT: guards that the
//! import reached the build).
//!
//! The Rust side's calc is U15's LuaCalcBridge (ext:calc), as in u20-u22.
//!
//! Stand-ins and harness fixes (both sides run the same ones):
//! * the tabs other than Import and Notes draw nothing (the other Lua tabs' `Draw` are replaced
//!   after every `buildMode:Init`; the Rust side masks the Tree arm of `ShellHooks::draw_tab`,
//!   the tab a build opens on);
//! * `utf8.next` on the Lua side (as u22), `spec.modFlag` and `TreeTab:GetSpecList` (as u22);
//!   the items, skills and config tabs' `modFlag` (UndoHandler.lua:37, set by U44's
//!   `TabSeams` stand-ins through `ShellHooks::set_tab_flag`) on the Rust side, as `spec.modFlag`;
//! * SimpleGraphic's `Deflate`/`Inflate` on the Lua side bound to the port's (as u43);
//! * `os.time()` on the Rust side is the oracle's virtual clock (determinism.lua);
//! * Net completions on the Rust side are handed to `App::on_net` at the start of the next
//!   `on_frame` (as u43, CCR-0109 §8).
//!
//! T4 normalisation besides u20's: a Lua `nil` `mainActiveSkill`/`mainActiveSkillCalcs` of a
//! socket group the import created (ImportTab.lua:1855 sets neither; CalcSetup.lua:1888 sets the
//! main group's) is the model's NaN (CCR-0019), which BM-B's SkillsTab:Save writes as
//! `tostring(NaN)` (CCR-0117 §6); both are compared as `nil` and counted. The LuaCalcBridge's VM
//! loads the Rust build through SkillsTab:Load, whose defaults the adoption hands back: a nil gem
//! `count` (ImportTab.lua:1843 sets none) and a granted skill's empty gem `nameSpec`
//! (SkillsTab.lua:489-493) get their values back after each calc, and a NaN group
//! field crosses as 1 and gets its NaN back unless CalcSetup set it (`NilGroupFieldBridge`,
//! CCR-0117 §6).
//!
//! The Lua side's sub-script pass frees a slot before the download callback runs, so a download
//! started inside a callback (the profile page, ImportTab.lua:948) reuses the id and Launch.lua's
//! trailing `subScripts[id] = nil` drops it; a Lua stand-in keeps the re-registration
//! (CCR-0117 §8).
//!
//! `u44_rust_unit`: the import methods on the Rust side alone: an empty-build import of the
//! sample character checked against values PoB's own `spec/System/TestImport_spec.lua` asserts.
//!
//! Compared side effects exclude `ImageLoad` (the Lua tabs' constructors load the tree's
//! images; U23 ports them).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_app::app::CxApp;
use pob_app::build::init;
use pob_app::build::on_frame::{self as of, TabFlag};
use pob_calc::model::{XmlChild, XmlNode};
use pob_host::{App, PlatformParts};
use pob_sg::trace::{FrameTrace, SideEffect};
use pob_sg::{Key, Sg, Sys};
use pob_ui_oracle::boot::{self, Entry, OracleHandles};
use pob_ui_oracle::diff_report;
use pob_ui_oracle::lua_calc::LuaCalcBridge;
use pob_ui_oracle::oracle_cache::{self, LuaRun};
use pob_ui_oracle::runner::{self, Driver, LuaApp, RunResult, ScenarioApp, UserDir};

const SCREENS: [(i32, i32, f32); 2] = [(1280, 800, 1.0), (1920, 1080, 1.5)];

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

fn test_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/u44")
}

/// The Lua stand-ins (see the module notes): every tab but Import and Notes draws nothing. Also the
/// oracle's `lua-utf8` stand-in has no `utf8.next` (EditControl.lua:558/:572, LEFT/RIGHT;
/// CCR-0033 item 4): luautf8's `Lutf8_next` for EditControl's call shape, as the u11
/// galleries install it (`lua/gallery/u11_single.lua`).
const LUA_STUBS: &str = r#"
local lutf8 = require("lua-utf8")
if not lutf8.next then
	local function iscont(s, p)
		local b = s:byte(p + 1) or 0
		return b >= 0x80 and b < 0xC0
	end
	lutf8.next = function(s, i, off)
		local e = #s
		local p = i - 1
		local idx = off
		if idx >= 0 then
			while p < e and idx > 0 do
				while p < e and iscont(s, p + 1) do
					p = p + 1
				end
				p = (p < e) and p + 1 or e
				idx = idx - 1
			end
		else
			while 0 < p and idx < 0 do
				local q = p
				while 0 < q and iscont(s, q - 1) do
					q = q - 1
				end
				p = (0 < q) and q - 1 or 0
				idx = idx + 1
			end
		end
		if idx ~= 0 or p == e then
			return
		end
		return p + 1, s:byte(p + 1)
	end
end
-- CCR-0117 §8: the oracle's sub-script pass (sg_bridge deliver_subscripts) frees a slot before
-- launch:OnSubFinished/OnSubError run, so a download started from a download callback
-- (ImportTab.lua:948, the profile page of DownloadSiteCharacterList) gets the same id and the
-- trailing `self.subScripts[id] = nil` (Launch.lua:220/:252) drops it. SimpleGraphic frees the
-- slot after the callback, so the new id differs. During the call, a re-registration of `id`
-- survives that trailing nil.
for _, name in ipairs({ "OnSubFinished", "OnSubError" }) do
	local orig = launch[name]
	launch[name] = function(self, id, ...)
		local real = self.subScripts
		local relaunched = false
		self.subScripts = setmetatable({ }, {
			__index = real,
			__newindex = function(_, k, v)
				if k == id then
					if v ~= nil then
						relaunched = true
						real[k] = v
					elseif not relaunched then
						real[k] = nil
					end
				else
					real[k] = v
				end
			end,
		})
		local ok, err = pcall(orig, self, id, ...)
		self.subScripts = real
		if not ok then error(err, 0) end
	end
end
-- calc doc D8 / ui doc §11 patch 6: ImportTab.lua:1384 `pairs(self.build.itemsTab.slots)` visits
-- the slots in PoB's per-process string-hash order, and the order is observable: DeleteItem's
-- PopulateSlots can empty a later slot (a quiver once its bow is gone), whose item then survives
-- "Delete equipment". The Rust side visits them in insertion order (D8), i.e. the slot controls'
-- creation order, which __canonPairs yields.
local function canonSlotOrder(ImportTab)
	local orig = ImportTab.ImportItemsAndSkills
	ImportTab.ImportItemsAndSkills = function(self, ...)
		local slots = self.build.itemsTab.slots
		local nativePairs = pairs
		pairs = function(t)
			if t == slots then
				return __canonPairs(t)
			end
			return nativePairs(t)
		end
		local ret = { n = 0 }
		local function pack(...) ret = { n = select('#', ...), ... } end
		pack(pcall(orig, self, ...))
		pairs = nativePairs
		if not ret[1] then error(ret[2], 0) end
		return unpack(ret, 2, ret.n)
	end
end
local b = main.modes.BUILD
local origInit = b.Init
function b:Init(...)
	origInit(self, ...)
	local ImportTab = common.classes.ImportTab
	if ImportTab and not ImportTab.__u44CanonSlots then
		ImportTab.__u44CanonSlots = true
		canonSlotOrder(ImportTab)
	end
	for _, t in ipairs({ "partyTab", "configTab", "treeTab", "skillsTab", "itemsTab", "calcsTab", "compareTab" }) do
		if self[t] then self[t].Draw = function() end end
	end
end
"#;

thread_local! {
    /// The stand-in `spec.modFlag` (UndoHandler.lua:37 via `spec:AddUndoState`, U25).
    static SPEC_MOD_FLAG: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The stand-in `modFlag`s of the items, skills and config tabs (U34, U30, U42).
    static TAB_MOD_FLAGS: std::cell::Cell<[bool; 3]> = const { std::cell::Cell::new([false; 3]) };
}

fn tab_flag_slot(f: TabFlag) -> Option<usize> {
    match f {
        TabFlag::Items => Some(0),
        TabFlag::Skills => Some(1),
        TabFlag::Config => Some(2),
        _ => None,
    }
}

/// determinism.lua's `os.time()`: `VIRTUAL_EPOCH + floor(GetTime() / 1000)`.
const VIRTUAL_EPOCH: f64 = 1_700_000_000.0;

fn rust_stubs() {
    SPEC_MOD_FLAG.with(|f| f.set(false));
    TAB_MOD_FLAGS.with(|f| f.set([false; 3]));
    pob_app::tabs::import::poe_api::set_os_time(Some(Rc::new(|cx| {
        VIRTUAL_EPOCH + (cx.sys.time_ms() / 1000) as f64
    })));
    init::set_hooks(|h| {
        *h = init::default_hooks();
        h.spec_add_undo_state = Rc::new(|_| SPEC_MOD_FLAG.with(|f| f.set(true)));
        // U25 stand-in: TreeTab.lua:549-557, the model half and the UI tail it leaves out (L556
        // `self.build.spec:SetWindowTitleWithBuildClass()`, L557 `UpdateClassDropdowns`);
        // ImportTab.lua:1210 calls it
        h.tree_set_active_spec = Rc::new(|cx, id| {
            let m = cx.build().model.clone();
            m.treeTab.SetActiveSpec(&m, id);
            init::set_window_title_with_build_class(cx);
            let tv = m.spec.get().treeVersion.get();
            let f = init::hooks(cx).update_class_dropdowns;
            f(cx, Some(tv));
        });
        // U25 stand-in: TreeTab.lua:492-498 (the buildLoadouts selFunc, reached from
        // SyncLoadouts' SetSel), as in u20
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
    of::set_shell_hooks(|h| {
        *h = of::default_shell_hooks();
        // LUA_STUBS replaces `treeTab.Draw`: the Tree tab (U25) draws nothing here as well
        let all_tabs = h.draw_tab.clone();
        h.draw_tab = Rc::new(move |cx, tab, vp, ev| {
            if tab != of::BuildTab::Tree {
                all_tabs(cx, tab, vp, ev);
            }
        });
        // keep the default arms (Notes) and add the spec stand-in
        let (flag, set_flag) = (h.tab_flag.clone(), h.set_tab_flag.clone());
        h.tab_flag = Rc::new(move |cx, f| {
            if f == TabFlag::Spec {
                SPEC_MOD_FLAG.with(std::cell::Cell::get)
            } else if let Some(i) = tab_flag_slot(f) {
                TAB_MOD_FLAGS.with(|c| c.get()[i])
            } else {
                flag(cx, f)
            }
        });
        h.set_tab_flag = Rc::new(move |cx, f, v| {
            if f == TabFlag::Spec {
                SPEC_MOD_FLAG.with(|c| c.set(v));
            } else if let Some(i) = tab_flag_slot(f) {
                TAB_MOD_FLAGS.with(|c| {
                    let mut a = c.get();
                    a[i] = v;
                    c.set(a);
                });
            } else {
                set_flag(cx, f, v);
            }
        });
    });
}

/// `#! NAME ARG` directives.
fn directives(text: &str, name: &str) -> Vec<String> {
    let tag = format!("#! {name}");
    text.lines()
        .filter_map(|l| l.trim().strip_prefix(tag.as_str()))
        .map(|s| s.trim().to_owned())
        .collect()
}

// ---- the Lua side ----

/// SimpleGraphic's `Deflate` / `Inflate` (`ui_api.cpp` `l_Deflate`: zlib, level 9; `l_Inflate`:
/// a zlib stream or `nil, msg`), bound to the port's functions (see the module notes).
fn install_sg_zlib(lua: &mlua::Lua) -> mlua::Result<()> {
    use pob_app::tabs::import::core::{deflate, inflate};
    let g = lua.globals();
    g.set(
        "Deflate",
        lua.create_function(|lua, data: mlua::String| {
            lua.create_string(deflate(&data.as_bytes()))
        })?,
    )?;
    g.set(
        "Inflate",
        lua.create_function(|lua, data: mlua::String| {
            Ok(match inflate(&data.as_bytes()) {
                Some(out) => (Some(lua.create_string(out)?), None),
                None => (None, Some("data error".to_owned())),
            })
        })?,
    )?;
    Ok(())
}

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
        self.app
            .with_vm(|pob| {
                pob.eval::<()>(LUA_STUBS)?;
                install_sg_zlib(&pob.lua)
            })
            .unwrap_or_else(|e| panic!("Lua stubs: {e}"));
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
        self.app.snapshot()
    }
    fn after_frame(&mut self) -> anyhow::Result<()> {
        self.app.after_frame()
    }
    fn take_error(&mut self) -> Option<String> {
        self.app.take_error()
    }
}

// ---- the Rust side ----

/// Records the platform side effects the Lua side's bridge records (the oracle's Rust platform
/// does not, CCR-0028 item 2); as in u16-u21.
struct RecordingPlatform {
    inner: Box<dyn pob_sg::Platform>,
    effects: Rc<RefCell<Vec<SideEffect>>>,
}

impl pob_sg::Platform for RecordingPlatform {
    fn time_ms(&self) -> i64 {
        self.inner.time_ms()
    }
    fn copy(&mut self, text: &str) {
        self.effects
            .borrow_mut()
            .push(SideEffect::Copy(text.to_owned()));
        self.inner.copy(text);
    }
    fn paste(&mut self) -> Option<String> {
        self.inner.paste()
    }
    fn open_url(&mut self, url: &str) {
        self.effects
            .borrow_mut()
            .push(SideEffect::OpenUrl(url.to_owned()));
        self.inner.open_url(url);
    }
    fn set_window_title(&mut self, title: &str) {
        self.effects
            .borrow_mut()
            .push(SideEffect::WindowTitle(title.to_owned()));
        self.inner.set_window_title(title);
    }
    fn take_screenshot(&mut self) {
        self.effects.borrow_mut().push(SideEffect::Screenshot);
        self.inner.take_screenshot();
    }
    fn restart(&mut self) {
        self.inner.restart();
    }
    fn exit(&mut self, msg: Option<&str>) {
        self.effects.borrow_mut().push(SideEffect::Exit);
        self.inner.exit(msg);
    }
    fn script_path(&self) -> String {
        self.inner.script_path()
    }
    fn runtime_path(&self) -> String {
        self.inner.runtime_path()
    }
    fn user_path(&self) -> Result<String, (String, String)> {
        self.inner.user_path()
    }
    fn con_print(&mut self, s: &str) {
        self.inner.con_print(s);
    }
}

struct NullPlatform;
impl pob_sg::Platform for NullPlatform {
    fn time_ms(&self) -> i64 {
        0
    }
    fn copy(&mut self, _: &str) {}
    fn paste(&mut self) -> Option<String> {
        None
    }
    fn open_url(&mut self, _: &str) {}
    fn set_window_title(&mut self, _: &str) {}
    fn take_screenshot(&mut self) {}
    fn restart(&mut self) {}
    fn exit(&mut self, _: Option<&str>) {}
    fn script_path(&self) -> String {
        String::new()
    }
    fn runtime_path(&self) -> String {
        String::new()
    }
    fn user_path(&self) -> Result<String, (String, String)> {
        Ok(String::new())
    }
    fn con_print(&mut self, _: &str) {}
}

/// Holds the inner Net's completions back from `Driver::frame`'s `poll` (which drops them) and
/// hands them to [`RustSide::on_frame`] (see the module notes).
struct DeferredNet {
    inner: Box<dyn pob_sg::Net>,
    held: Rc<RefCell<Vec<(u64, pob_sg::HttpResponse)>>>,
}

impl pob_sg::Net for DeferredNet {
    fn download_page(&mut self, req: pob_sg::HttpRequest) -> u64 {
        self.inner.download_page(req)
    }
    fn poll(&mut self) -> Vec<(u64, pob_sg::HttpResponse)> {
        let done = self.inner.poll();
        self.held.borrow_mut().extend(done);
        Vec::new()
    }
    fn oauth_listen(&mut self, url: &str) -> Option<u64> {
        self.inner.oauth_listen(url)
    }
    fn busy(&self) -> bool {
        self.inner.busy() || !self.held.borrow().is_empty()
    }
}

/// CCR-0117 §6: BM-B's `SkillsTab:Save` writes a nil (NaN) `mainActiveSkill` /
/// `mainActiveSkillCalcs` as `tostring(NaN)` instead of `"nil"`, so the LuaCalcBridge's VM loads
/// NaN (`tonumber("nan")`, SkillsTab.lua:453-454) instead of SkillsTab:Load's `or 1`, and
/// Build.lua:1655 indexes `displaySkillList[NaN]`. Until BM-B writes `"nil"`, the fields the
/// import left nil (ImportTab.lua:1855) cross as the 1 that `"nil"` would load as, and afterwards
/// get their nil back unless CalcSetup.lua:1879-1888 set them in the bridge's VM (the main group
/// of the MAIN / CALCS env with a non-empty skill list). A gem's nil `count` (ImportTab.lua:1843
/// sets none; Load's `or 1`, SkillsTab.lua:501) gets its nil back too, and so does the empty
/// `nameSpec` of a granted skill's gem (Load's SkillsTab.lua:489-493). Nothing else is changed.
struct NilGroupFieldBridge(LuaCalcBridge);

/// The groups with a nil `mainActiveSkill` / `mainActiveSkillCalcs`, the gems with a nil
/// `count`, and the gems without a `gemId` whose `nameSpec` is "".
type NilFields = (
    Vec<(pob_calc::model::SocketGroupRef, bool, bool)>,
    Vec<Rc<pob_calc::model::GemInstance>>,
    Vec<Rc<pob_calc::model::GemInstance>>,
);

fn nil_group_fields_to_one(build: &pob_calc::model::BuildRef) -> NilFields {
    let mut v = Vec::new();
    let mut gems = Vec::new();
    let mut unnamed = Vec::new();
    for g in build.skillsTab.socketGroupList.get().snapshot() {
        for gem in g.gemList.get().snapshot() {
            if gem.gemId.get().is_none() && gem.nameSpec.get().is_empty() {
                unnamed.push(gem.clone());
            }
            if gem.count.get().is_none() {
                gems.push(gem);
            }
        }
        let (a, c) = (
            g.mainActiveSkill.get().is_nan(),
            g.mainActiveSkillCalcs.get().is_nan(),
        );
        if a || c {
            if a {
                g.mainActiveSkill.set(1.0);
            }
            if c {
                g.mainActiveSkillCalcs.set(1.0);
            }
            v.push((g, a, c));
        }
    }
    (v, gems, unnamed)
}

fn restore_nil_group_fields(
    build: &pob_calc::model::BuildRef,
    (v, gems, unnamed): NilFields,
    calc_ran: bool,
) {
    // SkillsTab.lua:501 `tonumber(count) or 1` in the bridge's VM, adopted back; the calc never
    // sets a gem's count
    for gem in gems {
        gem.count.set(None);
    }
    // SkillsTab.lua:489-493: the VM's Load names a gem saved with a skillId and no gemId (a
    // granted skill's, CalcSetup.lua:1487-1492, which ProcessSocketGroup leaves "",
    // SkillsTab.lua:1166) after its granted effect; the calc keeps an existing gem's nameSpec
    for gem in unnamed {
        gem.nameSpec.set(Default::default());
    }
    let groups = build.skillsTab.socketGroupList.get().snapshot();
    let main = build.mainSocketGroup.get();
    let calcs_main = build.calcsTab.input.num("skill_number");
    let non_empty = |n: Option<usize>| n.is_some_and(|n| n > 0);
    for (g, a, c) in v {
        let index = groups
            .iter()
            .position(|x| Rc::ptr_eq(x, &g))
            .map(|i| (i + 1) as f64);
        let set_main =
            calc_ran && index == Some(main) && non_empty(g.displaySkillList.get().map(|l| l.len()));
        let set_calcs = calc_ran
            && index == Some(calcs_main)
            && non_empty(g.displaySkillListCalcs.get().map(|l| l.len()));
        if a && !set_main {
            g.mainActiveSkill.set(f64::NAN);
        }
        if c && !set_calcs {
            g.mainActiveSkillCalcs.set(f64::NAN);
        }
    }
}

impl pob_app::calc::facade::CalcFacade for NilGroupFieldBridge {
    fn build_output(&mut self, build: &pob_calc::model::BuildRef) -> std::task::Poll<()> {
        let v = nil_group_fields_to_one(build);
        let r = self.0.try_build_output(build);
        restore_nil_group_fields(build, v, r.is_ok());
        if let Err(e) = r {
            pob_lua::lua_error!("{e}")
        }
        std::task::Poll::Ready(())
    }
    fn misc_calculator(
        &mut self,
        build: &pob_calc::model::BuildRef,
    ) -> std::task::Poll<(
        Rc<dyn pob_app::calc::facade::MiscCalc>,
        pob_app::calc::facade::Output,
    )> {
        match self.0.misc_calculator(build) {
            std::task::Poll::Ready((calc, base)) => std::task::Poll::Ready((
                Rc::new(NilGroupFieldMisc(calc, build.clone()))
                    as Rc<dyn pob_app::calc::facade::MiscCalc>,
                base,
            )),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

/// The misc calculator's VM loads the build too (Calcs.lua:96-112; it adopts no groups).
struct NilGroupFieldMisc(
    Rc<dyn pob_app::calc::facade::MiscCalc>,
    pob_calc::model::BuildRef,
);

impl pob_app::calc::facade::MiscCalc for NilGroupFieldMisc {
    fn call(
        &self,
        ov: Option<Rc<pob_app::calc::facade::Override>>,
        use_full_dps: Option<bool>,
    ) -> std::task::Poll<pob_app::calc::facade::Output> {
        let v = nil_group_fields_to_one(&self.1);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.0.call(ov, use_full_dps)
        }));
        restore_nil_group_fields(&self.1, v, false);
        match r {
            Ok(o) => o,
            Err(p) => std::panic::resume_unwind(p),
        }
    }
}

struct RustSide {
    app: pob_app::PobApp,
    effects: Rc<RefCell<Vec<SideEffect>>>,
    held: Rc<RefCell<Vec<(u64, pob_sg::HttpResponse)>>>,
}

impl RustSide {
    fn new(
        mut parts: PlatformParts,
        src: PathBuf,
        net: pob_ui_oracle::sg_bridge::NetFixtures,
    ) -> RustSide {
        let held = Rc::new(RefCell::new(Vec::new()));
        parts.sys.net = Box::new(DeferredNet {
            inner: Box::new(boot::ScriptedNet::new(net)),
            held: held.clone(),
        });
        let effects = Rc::new(RefCell::new(Vec::new()));
        let inner = std::mem::replace(&mut parts.sys.platform, Box::new(NullPlatform));
        parts.sys.platform = Box::new(RecordingPlatform {
            inner,
            effects: effects.clone(),
        });
        let mut app = pob_app::PobApp::with_calc(
            parts,
            Rc::new(move || Box::new(NilGroupFieldBridge(LuaCalcBridge::new(Some(src.clone()))))),
        );
        effects.borrow_mut().clear(); // Launch.lua:11 runs at script load, before tracing
                                      // pob-ui-oracle patch 4's equivalent (CCR-0091)
        app.cx.app_mut().launch.force_release_install = true;
        RustSide { app, effects, held }
    }
    fn flush(&mut self) {
        let e: Vec<_> = self.effects.borrow_mut().drain(..).collect();
        if let Some(t) = self.app.cx.sg.trace.as_mut() {
            for e in e {
                t.effect(e);
            }
        }
    }
}

impl App for RustSide {
    fn sg(&mut self) -> &mut Sg {
        self.app.sg()
    }
    fn sys(&mut self) -> &mut Sys {
        self.app.sys()
    }
    fn on_init(&mut self) {
        rust_stubs();
        self.app.on_init();
        self.flush();
    }
    fn on_frame(&mut self) {
        // the completions `poll` held back (pob_host::run_frame: before on_frame)
        let done: Vec<_> = self.held.borrow_mut().drain(..).collect();
        for (id, resp) in done {
            self.app.on_net(id, resp);
        }
        self.app.on_frame();
        self.flush();
    }
    fn on_key_down(&mut self, key: Key, double_click: bool) {
        self.app.on_key_down(key, double_click);
        self.flush();
    }
    fn on_key_up(&mut self, key: Key) {
        self.app.on_key_up(key);
        self.flush();
    }
    fn on_char(&mut self, ch: Key) {
        self.app.on_char(ch);
        self.flush();
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
        Ok(self.app.snapshot())
    }
    fn after_frame(&mut self) -> anyhow::Result<()> {
        // the Lua side fails the run on PoB's error prompt; so does this one
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

// ---- T4 (the rules are u20's; see its module notes) ----

fn unordered_children(elem: &str) -> bool {
    matches!(
        elem,
        "Calcs" | "ConfigSet" | "ItemSet" | "Sockets" | "Overrides"
    )
}

/// The UI-only savers not ported yet: compared separately, a difference is reported.
/// (`Import` is U43's and is compared with the rest.)
const PENDING_SAVERS: [&str; 1] = ["TreeView"];

#[derive(Default)]
struct T4Notes {
    pending_sections: Vec<String>,
    gem_nil_bools: usize,
    ambiguous_variants: usize,
    /// `Party/ExportedBuffs` elements of the Lua side (see `normalise`)
    exported_buffs: usize,
    /// `Spec masteryEffects` compared as sets (see `normalise`)
    mastery_orders: usize,
}

fn ambiguous_variant(node: &XmlNode) -> bool {
    let (Some(game_id), Some(skill_id)) = (node.attrib.get("gemId"), node.attrib.get("skillId"))
    else {
        return false;
    };
    let n = pob_gamedata::data()
        .gems
        .values()
        .filter(|g| *g.gameId == **game_id && *g.grantedEffectId == **skill_id)
        .count();
    n > 1
}

fn normalise(node: &XmlNode, top: bool, lua: bool, notes: &mut T4Notes) -> String {
    let mut attrs: Vec<(String, String)> = Vec::new();
    for (k, v) in &node.attrib {
        let rust_default = match (&*node.elem, &**k) {
            ("Gem", "enableGlobal1" | "enableGlobal2") | ("Skill", "includeInFullDPS") => {
                Some("false")
            }
            _ => None,
        };
        // a nil group field: the Lua side's `nil`, the Rust side's NaN (`tostring(NaN)`, CCR-0117
        // §6) or the model default 0 of a group the bridge's VM created (CalcSetup's tree-granted
        // groups, `source = "Tree:<id>"`; u20's rule)
        let nil_group_field = &*node.elem == "Skill"
            && matches!(&**k, "mainActiveSkill" | "mainActiveSkillCalcs")
            && (matches!(&**v, "nil" | "nan" | "-nan" | "NaN") || (!lua && &**v == "0"));
        if nil_group_field {
            if lua {
                notes.gem_nil_bools += 1;
            }
            attrs.push((k.to_string(), "nil".to_owned()));
        } else if let Some(d) = rust_default.filter(|_| &**v == "nil") {
            notes.gem_nil_bools += 1;
            attrs.push((k.to_string(), d.to_owned()));
        } else if &*node.elem == "Spec" && &**k == "masteryEffects" {
            // PassiveSpec:Save writes `pairs(self.masterySelections)`, whose integer-key layout
            // follows the insertion order, which for an import is `pairs` of the JSON object
            // (ImportTab.lua:1136): PoB's per-process string-hash order (calc doc D8); compared
            // as a set
            let inner = v.trim_start_matches('{').trim_end_matches('}');
            let mut e: Vec<&str> = inner.split("},{").collect();
            e.sort_unstable();
            if lua && e.len() > 1 {
                notes.mastery_orders += 1;
            }
            attrs.push((k.to_string(), format!("{{{}}}", e.join("},{"))));
        } else if &*node.elem == "Gem"
            && matches!(&**k, "variantId" | "nameSpec")
            && ambiguous_variant(node)
        {
            if k.as_ref() == "variantId" {
                notes.ambiguous_variants += 1;
            }
            attrs.push((k.to_string(), "<pairs-chosen variant>".to_owned()));
        } else {
            attrs.push((k.to_string(), v.to_string()));
        }
    }
    attrs.sort();
    let mut kids: Vec<String> = Vec::new();
    for c in &node.children {
        match c {
            XmlChild::Node(n) => {
                if top && notes.pending_sections.iter().any(|p| **p == *n.elem) {
                    continue;
                }
                // PartyTab:Save writes `ExportedBuffs` from `partyTab.buffExports`, which the calc
                // fills when "Export Support" is on (CalcPerform setBuffExports); the Rust
                // side's calc is the LuaCalcBridge, which does not hand them back to the Rust
                // model (ext:calc, U15), so they are set aside and counted
                if &*node.elem == "Party" && &*n.elem == "ExportedBuffs" {
                    if lua {
                        notes.exported_buffs += 1;
                    }
                    continue;
                }
                kids.push(normalise(n, false, lua, notes));
            }
            XmlChild::Text(t) => kids.push(format!("{t:?}")),
        }
    }
    if top || unordered_children(&node.elem) {
        kids.sort();
    }
    format!("<{} {:?}>[{}]", node.elem, attrs, kids.join(","))
}

fn root_element(text: &str) -> Result<XmlNode, String> {
    let root = pob_calc::model::xml::ParseXML_root(text)?;
    root.children
        .into_iter()
        .find_map(|c| match c {
            XmlChild::Node(n) => Some(n),
            XmlChild::Text(_) => None,
        })
        .ok_or_else(|| "no root element".to_owned())
}

fn first_diff(a: &str, b: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (a.split('<').collect(), b.split('<').collect());
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        if x != y {
            return format!("element {i}: lua <{x} | rust <{y}");
        }
    }
    format!("lengths {} vs {}", a.len(), b.len())
}

/// The raw text of the `<Notes>` element (from `<Notes` to `</Notes>`), for the strict check.
fn notes_text(xml: &str) -> Option<&str> {
    let a = xml.find("<Notes")?;
    let b = xml[a..]
        .find("</Notes>")
        .map(|b| a + b + "</Notes>".len())
        .or_else(|| {
            // an empty element written as `<Notes/>`
            xml[a..].find("/>").map(|b| a + b + 2)
        })?;
    Some(&xml[a..b])
}

/// The `Import` element's attributes, sorted (`pairs(xml.attrib)` order is per process).
fn import_attrs(xml: &str) -> Option<Vec<(String, String)>> {
    let root = root_element(xml).ok()?;
    root.children.iter().find_map(|c| match c {
        XmlChild::Node(n) if &*n.elem == "Import" => {
            let mut a: Vec<(String, String)> = n
                .attrib
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            a.sort();
            Some(a)
        }
        _ => None,
    })
}

/// Compare one T4 pair; `Ok(byte_identical)`.
fn compare_t4(what: &str, lua: &str, rust: &str, notes: &mut T4Notes) -> Result<bool, String> {
    let lr = root_element(lua).map_err(|e| format!("{what}: Lua XML: {e}"))?;
    let rr = root_element(rust).map_err(|e| format!("{what}: Rust XML: {e}"))?;
    // Notes (U22's): byte-identical
    let (ln, rn) = (notes_text(lua), notes_text(rust));
    if ln != rn {
        return Err(format!("{what}: Notes differ: lua {ln:?} rust {rn:?}"));
    }
    // Import (U43's): the same attributes
    let (li, ri) = (import_attrs(lua), import_attrs(rust));
    if li.is_none() || li != ri {
        return Err(format!("{what}: Import differs: lua {li:?} rust {ri:?}"));
    }
    let mut local = T4Notes::default();
    let section = |root: &XmlNode, elem: &str, lua: bool| {
        root.children.iter().find_map(|c| match c {
            XmlChild::Node(n) if *n.elem == *elem => {
                Some(normalise(n, false, lua, &mut T4Notes::default()))
            }
            _ => None,
        })
    };
    for elem in PENDING_SAVERS {
        if section(&lr, elem, true) != section(&rr, elem, false) {
            local.pending_sections.push(elem.to_owned());
        }
    }
    let ln = normalise(&lr, true, true, &mut local);
    let rn = normalise(&rr, true, false, &mut local);
    notes.gem_nil_bools += local.gem_nil_bools;
    notes.ambiguous_variants += local.ambiguous_variants;
    notes.exported_buffs += local.exported_buffs;
    notes.mastery_orders += local.mastery_orders;
    notes.pending_sections.extend(local.pending_sections);
    if ln != rn {
        return Err(format!("{what}: T4 differs: {}", first_diff(&ln, &rn)));
    }
    Ok(lua == rust)
}

// ---- runs ----

fn drop_image_loads(frames: &mut [FrameTrace]) {
    for f in frames {
        f.effects.retain(|e| !matches!(e, SideEffect::ImageLoad(_)));
    }
}

struct Side {
    frames: Vec<FrameTrace>,
    error: Option<String>,
    snapshots: Vec<(String, String)>,
    saved: Vec<(String, Option<String>)>,
}

fn read_saved(user: &UserDir, names: &[String]) -> Vec<(String, Option<String>)> {
    names
        .iter()
        .map(|n| {
            let p = user.path.join("Path of Building").join(n);
            (n.clone(), std::fs::read_to_string(p).ok())
        })
        .collect()
}

/// The Lua side of one run through the Lua-side cache (TP4, as u41): PoB boot, `script` and what
/// the test keeps of the VM and the user dir afterwards (`extra`, JSON). The key covers this file
/// (stubs and Lua-side wrapper), the Lua-side code, PoB, this test's scripts and fixtures, the
/// fixture user dir, the script, the screen and the other arguments.
fn lua_run(
    root: &Path,
    script: &pob_host::script::Script,
    user: &UserDir,
    fixture: Option<&Path>,
    screen: pob_sg::ScreenInfo,
    saved: &[String],
    net: &pob_ui_oracle::sg_bridge::NetFixtures,
) -> anyhow::Result<LuaRun> {
    let scripts = test_dir();
    let mut inputs: Vec<&Path> = vec![&scripts];
    inputs.extend(fixture);
    let key = oracle_cache::ui_key(
        "ui-u44",
        pob_ui_oracle::test_source!(),
        root,
        &inputs,
        &[
            &format!("{script:?}"),
            &format!("{screen:?}"),
            &format!("{saved:?}"),
            &format!("{net:?}"),
        ],
    );
    oracle_cache::cached_lua_run(key, || {
        let handles = OracleHandles::default();
        let lua_app = runner::lua_side(root, user, &handles, Entry::Launch, false)?;
        *pob_ui_oracle::sg_bridge::bridge(&lua_app.pob.lua)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .net
            .borrow_mut() = net.clone();
        let mut lua_side = LuaSide { app: lua_app };
        let mut d = Driver::new(screen, handles, user.path.clone());
        let (mut frames, r) = runner::drive(&mut lua_side, script, &mut d);
        drop(lua_side);
        drop_image_loads(&mut frames);
        Ok(LuaRun {
            frames,
            error: r.err().map(|e| e.to_string()),
            snapshots: std::mem::take(&mut d.snapshots),
            extra: vec![serde_json::to_string(&read_saved(user, saved))?],
        })
    })
}

/// Run `script` on both sides with `fixture` as the user dir (fresh copies per side). The Lua
/// VM is dropped before the Rust side starts (memory).
fn run_sides(
    root: &Path,
    script: &pob_host::script::Script,
    fixture: Option<&Path>,
    screen: pob_sg::ScreenInfo,
    saved: &[String],
    net: &pob_ui_oracle::sg_bridge::NetFixtures,
) -> anyhow::Result<(Side, Side)> {
    let user = UserDir::new(fixture)?;

    // Lua (cached, TP4)
    let run = lua_run(root, script, &user, fixture, screen, saved, net)?;
    let lua = Side {
        frames: run.frames,
        error: run.error,
        snapshots: run.snapshots,
        saved: serde_json::from_str(run.extra.first().map_or("null", String::as_str))?,
    };

    // Rust
    user.reset()?;
    let handles = OracleHandles::default();
    let (sg, sys) = boot::oracle_parts(root, &user.path, &handles)?;
    let src = std::fs::canonicalize(root.join("src")).unwrap_or_else(|_| root.join("src"));
    let mut rust_side = RustSide::new(PlatformParts { sg, sys }, src, net.clone());
    let mut d = Driver::new(screen, handles, user.path.clone());
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runner::drive(&mut rust_side, script, &mut d)
    }));
    let (mut frames, error) = match res {
        Ok((frames, r)) => (frames, r.err().map(|e| e.to_string())),
        Err(p) => (
            Vec::new(),
            Some(format!("panicked: {}", pob_host::panic_message(&*p))),
        ),
    };
    drop(rust_side);
    drop_image_loads(&mut frames);
    let rust = Side {
        frames,
        error,
        snapshots: std::mem::take(&mut d.snapshots),
        saved: read_saved(&user, saved),
    };
    Ok((lua, rust))
}

fn out_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(
            || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"),
            PathBuf::from,
        )
        .join("ui-parity")
}

fn draw_ops(frames: &[FrameTrace]) -> usize {
    use pob_sg::trace::ApiCall;
    frames
        .iter()
        .flat_map(|f| &f.calls)
        .filter(|c| {
            matches!(
                c.call,
                ApiCall::DrawImage { .. }
                    | ApiCall::DrawImageQuad { .. }
                    | ApiCall::DrawString { .. }
            )
        })
        .count()
}

fn window_titles(frames: &[FrameTrace]) -> Vec<String> {
    frames
        .iter()
        .flat_map(|f| &f.effects)
        .filter_map(|e| match e {
            SideEffect::WindowTitle(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

fn distinct_titles(t: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in t {
        if out.last() != Some(s) {
            out.push(s.clone());
        }
    }
    out
}

/// Every T4 pair of a run; `Ok(number of pairs, byte-identical pairs)`.
fn t4(lua: &Side, rust: &Side, notes: &mut T4Notes) -> Result<(usize, usize), Vec<String>> {
    let mut errs = Vec::new();
    let (mut n, mut same) = (0, 0);
    if lua.snapshots.len() != rust.snapshots.len() {
        errs.push(format!(
            "snapshots: lua {} rust {}",
            lua.snapshots.len(),
            rust.snapshots.len()
        ));
    }
    for ((ln, lx), (_, rx)) in lua.snapshots.iter().zip(&rust.snapshots) {
        n += 1;
        match compare_t4(&format!("snapshot {ln}"), lx, rx, notes) {
            Ok(b) => same += usize::from(b),
            Err(e) => errs.push(e),
        }
    }
    for ((name, l), (_, r)) in lua.saved.iter().zip(&rust.saved) {
        match (l, r) {
            (Some(l), Some(r)) => {
                n += 1;
                match compare_t4(&format!("saved {name}"), l, r, notes) {
                    Ok(b) => same += usize::from(b),
                    Err(e) => errs.push(e),
                }
            }
            (None, None) => errs.push(format!("saved {name}: written by neither side")),
            (l, r) => errs.push(format!(
                "saved {name}: lua {} rust {}",
                if l.is_some() { "wrote it" } else { "did not" },
                if r.is_some() { "wrote it" } else { "did not" }
            )),
        }
    }
    if errs.is_empty() {
        Ok((n, same))
    } else {
        Err(errs)
    }
}

/// Replace every drawn or copied build code with a placeholder and return the distinct decoded
/// documents in order of appearance: the code is Deflate of SaveDB("code"), whose attribute
/// order is `pairs` order (per process in PoB), so the code text cannot match byte for byte; the
/// decoded documents are compared with the T4 rules instead.
fn mask_codes(frames: &mut [FrameTrace]) -> Vec<(String, String)> {
    use pob_app::tabs::import::core::decode_build_code;
    use pob_sg::trace::ApiCall;
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    let mut take = |t: &str| -> bool {
        match decode_build_code(t).filter(|x| x.contains("<PathOfBuilding")) {
            Some(x) => {
                if !seen.iter().any(|s| s == t) {
                    seen.push(t.to_owned());
                    out.push((t.to_owned(), x));
                }
                true
            }
            None => false,
        }
    };
    for f in frames.iter_mut() {
        let mut code_ctls: Vec<Option<String>> = Vec::new();
        for c in &mut f.calls {
            if let ApiCall::DrawString { text, x, .. } = &mut c.call {
                // a focused EditControl draws "^7" .. buf, scrolled left by the text's width
                let (esc, body) = match text.strip_prefix("^7") {
                    Some(b) => ("^7", b.to_owned()),
                    None => ("", text.clone()),
                };
                if take(&body) {
                    *text = format!("{esc}<build code>");
                    if *x < 0.0 {
                        *x = 0.0;
                    }
                    code_ctls.push(c.ctl.clone());
                }
            }
        }
        // the caret and the text after it (EditControl.lua:425-429) sit at the code's width,
        // which differs with the code: their x inside the edit's text viewport is not compared
        for c in &mut f.calls {
            if c.ctl.is_none() || !code_ctls.contains(&c.ctl) {
                continue;
            }
            match &mut c.call {
                // the (empty) text after the caret at the end of the code
                ApiCall::DrawString { x, text, .. } if text.is_empty() => *x = 0.0,
                // the 1 px caret
                ApiCall::DrawImage { rect, .. } if rect[2] <= 1.0 => rect[0] = 0.0,
                _ => {}
            }
        }
        for e in &mut f.effects {
            if let SideEffect::Copy(text) = e {
                if take(text) {
                    *text = "<build code>".to_owned();
                }
            }
        }
    }
    out
}

/// Every string the Lua side drew.
fn drawn_strings(frames: &[FrameTrace]) -> impl Iterator<Item = &str> {
    use pob_sg::trace::ApiCall;
    frames
        .iter()
        .flat_map(|f| &f.calls)
        .filter_map(|c| match &c.call {
            ApiCall::DrawString { text, .. } => Some(text.as_str()),
            _ => None,
        })
}

/// The `#! u44-expect-*` directives (module notes), checked on the Lua side.
fn check_expectations(text: &str, lua: &Side) -> Vec<String> {
    let mut errs = Vec::new();
    for want in directives(text, "u44-expect-label") {
        if !drawn_strings(&lua.frames).any(|t| t.contains(want.as_str())) {
            errs.push(format!("the Lua side never drew {want:?}"));
        }
    }
    for want in directives(text, "u44-expect-import") {
        let (k, v) = want.split_once('=').unwrap_or((want.as_str(), ""));
        let found = lua.snapshots.iter().any(|(_, x)| {
            import_attrs(x).is_some_and(|a| a.iter().any(|(ak, av)| ak == k && av == v))
        });
        if !found {
            errs.push(format!("no Lua snapshot's Import has {k}={v:?}"));
        }
    }
    for want in directives(text, "u44-expect-xml") {
        if !lua
            .snapshots
            .last()
            .is_some_and(|(_, x)| x.contains(want.as_str()))
        {
            errs.push(format!("the last Lua snapshot lacks {want:?}"));
        }
    }
    if !directives(text, "u44-expect-copy").is_empty() {
        let copied = lua.frames.iter().flat_map(|f| &f.effects).any(|e| match e {
            SideEffect::Copy(code) => pob_app::tabs::import::core::decode_build_code(code)
                .is_some_and(|x| x.contains("<PathOfBuilding")),
            _ => false,
        });
        if !copied {
            errs.push("the Lua side copied no build code".to_owned());
        }
    }
    errs
}

/// One case's outcome (TP4: cases run on `oracle_cache::par_map`; collected in case order).
#[derive(Default)]
struct CaseOut {
    lines: Vec<String>,
    failures: Vec<String>,
    ran: usize,
    t4_pairs: usize,
    t4_bytes: usize,
}

#[test]
fn u44_scenarios() {
    let Some(root) = inputs() else { return };
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(test_dir())
        .expect("tests/u44")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no tests/u44/*.pobscript");
    let filter = std::env::var("U44_FILTER").ok();
    let mut cases = Vec::new();
    for path in scripts {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if filter.as_ref().is_some_and(|f| !stem.contains(f.as_str())) {
            continue;
        }
        let (script, _) = runner::load_script(&path).unwrap();
        let own_screen = matches!(
            script.steps.first(),
            Some(pob_host::script::Step::Screen { .. })
        );
        let screens: Vec<(i32, i32, f32)> = if own_screen {
            vec![(1280, 800, 1.0)]
        } else {
            SCREENS.to_vec()
        };
        for (w, h, s) in screens {
            cases.push((path.clone(), stem.clone(), own_screen, (w, h, s)));
        }
    }
    let results = oracle_cache::par_map(cases, |(path, stem, own_screen, (w, h, s))| {
        let mut out = CaseOut::default();
        let text = std::fs::read_to_string(&path).unwrap();
        let (script, base_dir) = runner::load_script(&path).unwrap();
        let fx = runner::fixtures(&script, &base_dir).unwrap();
        let saved = directives(&text, "u44-saved");
        {
            let tag = if own_screen {
                format!("u44_{stem}")
            } else {
                format!("u44_{stem}@{w}x{h}@{s}")
            };
            let (lua, rust) = match run_sides(
                &root,
                &script,
                fx.user_dir.as_deref(),
                runner::screen(w, h, s),
                &saved,
                &fx.net,
            ) {
                Ok(s) => s,
                Err(e) => {
                    out.failures.push(format!("{tag}: {e:#}"));
                    return out;
                }
            };
            out.ran += 1;
            let titles = (window_titles(&lua.frames), window_titles(&rust.frames));
            let mut notes = T4Notes::default();
            let mut t4r = t4(&lua, &rust, &mut notes);
            let expect = check_expectations(&text, &lua);
            let (mut lua_frames, mut rust_frames) = (lua.frames, rust.frames);
            let (lua_codes, rust_codes) =
                (mask_codes(&mut lua_frames), mask_codes(&mut rust_frames));
            let mut code_errs = Vec::new();
            if lua_codes.len() != rust_codes.len() {
                code_errs.push(format!(
                    "build codes: lua {} rust {}",
                    lua_codes.len(),
                    rust_codes.len()
                ));
            }
            let mut code_same = 0;
            for (i, ((lc, l), (rc, r))) in lua_codes.iter().zip(&rust_codes).enumerate() {
                if lc == rc {
                    // the same code text (a pasted fixture code): byte-identical
                    code_same += 1;
                    continue;
                }
                match compare_t4(&format!("build code {}", i + 1), l, r, &mut notes) {
                    Ok(b) => code_same += usize::from(b),
                    Err(e) => code_errs.push(e),
                }
            }
            match &mut t4r {
                Ok((p, b)) if code_errs.is_empty() => {
                    *p += lua_codes.len();
                    *b += code_same;
                }
                Ok(_) => t4r = Err(code_errs),
                Err(v) => v.extend(code_errs),
            }
            // `#! u44-set-aside-frame N REASON`: frame N's draws are compared on their own (their
            // own report) and reported, not failed; its side effects stay in the main comparison
            let mut aside = Vec::new();
            for d in directives(&text, "u44-set-aside-frame") {
                let (n, reason) = d.split_once(' ').unwrap_or((d.as_str(), ""));
                let Ok(i) = n.parse::<usize>() else { continue };
                if i >= lua_frames.len() || i >= rust_frames.len() {
                    continue;
                }
                let one = RunResult {
                    lua: vec![lua_frames[i].clone()],
                    rust: vec![rust_frames[i].clone()],
                    rust_error: None,
                    lua_snapshots: Vec::new(),
                    rust_snapshots: Vec::new(),
                    audit: Vec::new(),
                    compared: true,
                };
                let k =
                    diff_report::write_report(&out_dir(), &format!("{tag}_frame{i}"), &one, false)
                        .unwrap_or(usize::MAX);
                aside.push(format!("frame {i}: {k} divergences ({reason})"));
                lua_frames[i].calls.clear();
                rust_frames[i].calls.clear();
            }
            let run = RunResult {
                lua: lua_frames,
                rust: rust_frames,
                rust_error: rust.error.clone(),
                lua_snapshots: lua.snapshots.clone(),
                rust_snapshots: rust.snapshots.clone(),
                audit: Vec::new(),
                compared: true,
            };
            let n = diff_report::write_report(&out_dir(), &tag, &run, false).unwrap_or(usize::MAX);
            if !aside.is_empty() {
                out.lines
                    .push(format!("{tag}: set aside {}", aside.join("; ")));
            }
            let ops = draw_ops(&run.lua);
            let t4_line = match &t4r {
                Ok((p, b)) => {
                    out.t4_pairs += p;
                    out.t4_bytes += b;
                    let mut pending = notes.pending_sections.clone();
                    pending.sort();
                    pending.dedup();
                    format!(
                        "T4 {p} identical incl. Import and Notes and {} decoded build codes ({b} byte-identical; set aside: {} Lua nil gem/group fields, {} pairs-chosen gem variants, {} Party/ExportedBuffs, {} masteryEffects compared as sets, pending UI-saver sections differing {:?})",
                        lua_codes.len(), notes.gem_nil_bools, notes.ambiguous_variants, notes.exported_buffs, notes.mastery_orders, pending
                    )
                }
                Err(e) => format!("T4 FAILED: {}", e.join("; ")),
            };
            out.lines.push(format!(
                "{tag}: {} frames, {ops} Lua draw ops, {n} divergences, {t4_line}, titles {:?}{}{}{}",
                run.lua.len(),
                distinct_titles(&titles.0),
                lua.error
                    .as_ref()
                    .map_or_else(String::new, |e| format!(", Lua error: {e}")),
                rust.error
                    .as_ref()
                    .map_or_else(String::new, |e| format!(", Rust error: {e}")),
                if expect.is_empty() {
                    String::new()
                } else {
                    format!(", expectations: {}", expect.join("; "))
                }
            ));
            if n != 0
                || rust.error.is_some()
                || lua.error.is_some()
                || run.lua.len() != run.rust.len()
                || ops == 0
                || titles.0 != titles.1
                || t4r.is_err()
                || !expect.is_empty()
            {
                out.failures.push(format!(
                    "{tag}: {n} divergences, frames lua {} rust {}, {ops} ops, {t4_line}, titles lua {:?} rust {:?}, errors lua {:?} rust {:?}, expectations {expect:?} (see {})",
                    run.lua.len(),
                    run.rust.len(),
                    distinct_titles(&titles.0),
                    distinct_titles(&titles.1),
                    lua.error,
                    rust.error,
                    out_dir().join(&tag).join("report.txt").display()
                ));
            }
        }
        out
    });
    let mut failures = Vec::new();
    let mut ran = 0;
    let (mut t4_pairs, mut t4_bytes) = (0, 0);
    for case in results {
        for l in &case.lines {
            println!("{l}");
        }
        failures.extend(case.failures);
        ran += case.ran;
        t4_pairs += case.t4_pairs;
        t4_bytes += case.t4_bytes;
    }
    println!("u44: {ran} runs, T4 {t4_pairs} pairs ({t4_bytes} byte-identical)");
    println!("{}", oracle_cache::summary());
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---- Rust-only runs ----

/// Run `script_text` (paths relative to tests/u44) on the Rust side only; the app is returned
/// for inspection.
fn rust_only(
    root: &Path,
    script_text: &str,
) -> anyhow::Result<(RustSide, Vec<FrameTrace>, UserDir)> {
    let script =
        pob_host::script::Script::parse(script_text).map_err(|e| anyhow::anyhow!("script: {e}"))?;
    let script = pob_host::script::Script {
        base_dir: Some(test_dir()),
        ..script
    };
    let fx = runner::fixtures(&script, &test_dir())?;
    let user = UserDir::new(fx.user_dir.as_deref())?;
    let handles = OracleHandles::default();
    let (sg, sys) = boot::oracle_parts(root, &user.path, &handles)?;
    let src = std::fs::canonicalize(root.join("src")).unwrap_or_else(|_| root.join("src"));
    let mut rust_side = RustSide::new(PlatformParts { sg, sys }, src, fx.net.clone());
    let mut d = Driver::new(runner::screen(1280, 800, 1.0), handles, user.path.clone());
    let (frames, r) = runner::drive(&mut rust_side, &script, &mut d);
    r?;
    Ok((rust_side, frames, user))
}

/// Open allflame_exp_001 from the build list and switch to the Import tab (Ctrl+I).
const OPEN_IMPORT: &str = "fixture userdir=../../fixtures/user1\nframes 2\nmove 400 91\nclick LEFTBUTTON\nframes 1\npress RETURN\nframes 3\nkey-down CTRL\npress i\nkey-up CTRL\nframes 2\n";

/// Prints where the Import tab's controls are (to write the scenario scripts).
#[test]
#[ignore = "a helper for writing tests/u44 scripts"]
fn u44_probe_positions() {
    // TP4: the process-wide case pool (shared with the `par_map` cases) instead of the lock
    let _permit = oracle_cache::Permit::acquire();
    let Some(root) = inputs() else { return };
    let (side, _frames, _user) = rust_only(&root, OPEN_IMPORT).expect("run");
    let cx = &side.app.cx;
    let tab = pob_app::tabs::import::core::tab(cx).expect("import tab");
    for key in [
        "siteAccountRealm",
        "siteAccountName",
        "siteAccountNameGo",
        "siteAccountHistory",
        "siteCharSelectLeague",
        "siteCharSelect",
        "siteCharImportTree",
        "siteCharImportTreeClearJewels",
        "siteCharImportItems",
        "siteCharImportItemsClearSkills",
        "siteCharImportItemsClearItems",
        "siteCharImportItemsIgnoreWeaponSwap",
    ] {
        if let Some(id) = cx.child(tab, key) {
            let (x, y) = cx.pos(id);
            let (w, h) = cx.size(id);
            println!("{key}: x={x} y={y} w={w} h={h} shown={}", cx.is_shown(id));
        }
    }
}

/// The import methods on the Rust side alone (module notes), against what PoB's own
/// `spec/System/TestImport_spec.lua` asserts for the sample character ("imports with correct
/// tree", "imports with correct jewels", "imports modifier flags from the 3.29 item API").
#[test]
fn u44_rust_unit() {
    use pob_app::tabs::import::character;
    use pob_app::tabs::import::poe_api::{json_get, Json};
    // TP4: the process-wide case pool (shared with the `par_map` cases) instead of the lock
    let _permit = oracle_cache::Permit::acquire();
    let Some(root) = inputs() else { return };
    let sample: Json = serde_json::from_str(
        &std::fs::read_to_string(root.join("spec/System/SampleCharacter.json")).expect("sample"),
    )
    .expect("json");
    let sample = json_get(&sample, "character").expect("character").clone();
    let (mut side, _frames, _user) = rust_only(&root, OPEN_IMPORT).expect("run");
    let cx = &mut side.app.cx;
    let tab = pob_app::tabs::import::core::tab(cx).expect("import tab");

    // "imports with correct tree"
    let msg = character::import_passive_tree_and_jewels(cx, tab, &sample, true);
    assert!(
        msg.ends_with("Passive tree and jewels successfully imported."),
        "{msg}"
    );
    let m = cx.build().model.clone();
    let input = m.configTab.input.clone();
    assert_eq!(input.str("bandit").as_deref(), Some("None"));
    assert_eq!(
        input.str("pantheonMajorGod").as_deref(),
        Some("TheBrineKing")
    );
    assert_eq!(input.str("pantheonMinorGod").as_deref(), Some("Yugul"));
    assert_eq!(m.characterLevel.get(), 99.0);
    let spec = m.spec.get();
    spec.BuildAllDependsAndPaths();
    assert_eq!(spec.curAscendClassName.get().as_deref(), Some("Hierophant"));
    assert_eq!(spec.curClassName.get().as_deref(), Some("Templar"));
    assert_eq!(
        spec.curSecondaryAscendClassName.get().as_deref(),
        Some("Farrul Bloodline")
    );
    assert_eq!(spec.masterySelections.with(|s| s.get(4492)), Some(5356.0));
    assert_eq!(spec.allocatedKeystoneCount.get(), 2.0);
    assert_eq!(spec.allocatedMasteryCount.get(), 7.0);
    assert_eq!(spec.allocatedNotableCount.get(), 29.0);
    assert_eq!(spec.allocatedTattooTypes.num("NgamahuTattoo"), 6.0);

    // "imports with correct jewels"
    let jd = spec.extra.table("jewel_data").expect("jewel_data");
    assert_eq!(jd.pairs().count(), 11);
    let j3 = jd.table(3).expect("jewel_data[3]");
    assert_eq!(j3.num("radius"), 2400.0);
    assert_eq!(j3.str("type").as_deref(), Some("JewelStr"));
    let jewels: Vec<f64> = spec
        .jewels
        .with(|j| j.pairs().into_iter().map(|(_, v)| v).collect());
    let names: Vec<String> = m.itemsTab.items.with(|i| {
        i.values()
            .filter(|it| it.id.get().is_some_and(|id| jewels.contains(&id)))
            .map(|it| it.name.get().to_string())
            .collect()
    });
    for want in [
        "Healthy Mind, Cobalt Jewel",
        "Fulgent Bliss, Small Cluster Jewel",
    ] {
        assert!(
            names.iter().any(|n| n == want),
            "{want} not socketed: {names:?}"
        );
    }

    // "imports modifier flags from the 3.29 item API"
    let item: Json = serde_json::from_str(
        r#"{"id":"test-item-mod-flags","frameType":2,"name":"Test Grip","typeLine":"Rawhide Gloves",
            "inventoryId":"Gloves","ilvl":10,"duplicated":true,"vestigial":true,"properties":[],
            "implicitMods":[{"description":"+20 to maximum Life"}],
            "explicitMods":[{"description":"+10 to maximum Life","flags":{"crafted":true}},
                {"description":"+11 to maximum Mana","flags":{"fractured":true}},
                {"description":"+12 to Strength","flags":{"mutated":true}},
                {"description":"+13 to Dexterity","flags":{"vestigial":true}}]}"#,
    )
    .unwrap();
    character::import_item(cx, &item, None, false, None);
    let m = cx.build().model.clone();
    let slot = m
        .itemsTab
        .slots
        .with(|s| s.get(&pob_lua::Name::from("Gloves")).cloned())
        .unwrap();
    let id = slot.num("selItemId");
    let it = m
        .itemsTab
        .items
        .with(|i| i.get(&(id as u32)).cloned())
        .expect("the gloves");
    assert_eq!(
        it.implicitModLines.get().snapshot()[0]
            .str("line")
            .as_deref(),
        Some("+20 to maximum Life")
    );
    let ex = it.explicitModLines.get().snapshot();
    let line = |l: &str| {
        ex.iter()
            .find(|t| t.str("line").as_deref() == Some(l))
            .cloned()
            .unwrap()
    };
    assert!(line("+10 to maximum Life").truthy("crafted"));
    assert!(line("+11 to maximum Mana").truthy("fractured"));
    assert!(line("+12 to Strength").truthy("mutated"));
    assert!(line("+13 to Dexterity").truthy("vestigial"));
    assert!(it.mirrored.get());
    assert!(it.BuildRaw().contains("{vestigial}"));
}
