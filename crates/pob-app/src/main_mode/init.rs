//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Main.lua:1-864 (main:Init).
//! Owner: U16.
//!
//! @pob-file Modules/Main.lua @16de4b82d
//!
//! The module-level code (Main.lua:1-57: the `LoadModule` calls, the `--no-jit`/`--no-ssl`
//! switches and `main = new("ControlHost"):ControlHost()`), `main:Init` (:59-305),
//! `DetectUnicodeSupport` (:307-313), `CanExit` (:365-372) and `Shutdown` (:374-377).
//!
//! DIVERGENCE(mod-cache): `Data/ModCache.lua` is PoB's Lua-side parse cache. The port's mod
//! parser and its cache are the calc plan's (pob-modparser, data/dump), so L135-139 load nothing
//! and the `saveNewModCache` regeneration (L203-222, `main:SaveModCache` :315-348) is skipped with
//! a console note.
//! DIVERGENCE(no-cli-args): the hosts pass no command line (`arg` is empty), so the `arg[1]`
//! protocol-handler import (L71-84, `buildSites`, U43) never runs; `launch.arg` exists for a host
//! that supplies one, and a non-empty `arg[1]` raises until U43 ports `buildSites`.
//! ORDER: `main.onFrameFuncs` (string keys, `pairs` at L490) runs in insertion order; neither
//! function draws or reads the other's effects (FirstFrame prints, LoadItems fills the item DBs),
//! so the order is not observable (§8 rule 6 (c)).

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use pob_lua::n;
use pob_ui_core::controls::{ButtonControl, LabelControl};
use pob_ui_core::coroutine::{co, Coroutine};
use pob_ui_core::prelude::*;

use super::{ItemDb, MainHooks, MainHost, Mode, ModeArg, PendingMode};
use crate::app::CxApp;
use crate::model;

/// CCR-0091: the `main.modes` table of L61-63. U18 replaces the LIST entry with BuildList.lua's
/// mode object and U19 the BUILD entry with Build.lua's.
pub fn default_modes() -> IndexMap<String, Rc<dyn Mode>> {
    let mut modes: IndexMap<String, Rc<dyn Mode>> = IndexMap::new();
    // L62 self.modes["LIST"] = LoadModule("Modules/BuildList")
    modes.insert("LIST".to_owned(), Rc::new(PendingMode::new("LIST")));
    // L63 self.modes["BUILD"] = LoadModule("Modules/Build")
    modes.insert("BUILD".to_owned(), Rc::new(PendingMode::new("BUILD")));
    modes
}

/// CCR-0091: U17's popups and U23's `main:LoadTree`, until their owners install them.
pub fn default_hooks() -> MainHooks {
    MainHooks {
        open_options_popup: Rc::new(|_cx| {
            lua_error!("main:OpenOptionsPopup is not ported yet (U17, Main.lua:865-1316)")
        }),
        open_about_popup: Rc::new(|_cx, _section| {
            lua_error!("main:OpenAboutPopup is not ported yet (U17, Main.lua:1373-1485)")
        }),
        open_update_popup: Rc::new(|_cx| {
            lua_error!("main:OpenUpdatePopup is not ported yet (U17, Main.lua:1317-1372)")
        }),
        // main:LoadTree (U23, tree_view/assets.rs): builds main.tree[treeVersion]
        load_tree: Rc::new(|_cx, _tree_version| {}),
    }
}

/// `LoadModule("Modules/Main")` (Launch.lua:72): the module-level code of Main.lua. Returns the
/// `main` node.
/// lua: Main.lua:1-57
/// @pob Modules/Main.lua:1-57 h=9ceb87ab
pub fn load_module(cx: &mut Cx) -> CtrlId {
    // L17-25: LoadModule("GameVersions"), Common, CalcFormat, Data, ModTools, ItemTools, CalcTools,
    // PantheonTools, BuildSiteTools: the calc plan's crates (pob_gamedata::data() loads lazily).
    let _ = cx.data();
    // L28 ToastNotification = LoadModule("Modules/ToastNotification"): its module-level locals
    // are `cx.app().main.toasts` (ToastState::default()).
    // L41-49: `arg` switches (DIVERGENCE(no-cli-args): `arg` is empty unless a host sets it)
    let args = cx.app().launch.arg.clone();
    // L41 if arg and isValueInTable(arg, "--no-jit") then (no JIT in the port)
    if args.iter().any(|a| a == "--no-jit") {
        // L43
        cx.sys.platform.con_print("JIT Disabled");
    }
    // L46
    if args.iter().any(|a| a == "--no-ssl") {
        // L47
        cx.app_mut().launch.no_ssl = true;
        // L48
        cx.sys.platform.con_print("SSL verification disabled");
    }
    // L51-52 tempTable1/2: unused in U16's range
    // L57 main = new("ControlHost"):ControlHost()
    let node = cx
        .add_free(
            None,
            New::new(Base::new(Anchor::NONE, rect(0, 0, 0, 0)), MainHost).host(),
        )
        .id();
    cx.main.node = node;
    node
}

/// The first `new("Tooltip")` of the process loads Classes/Tooltip.lua, whose module code
/// preloads the recipe images (Tooltip.lua:36-41, U10's `module_load`). In PoB that is the first
/// ButtonControl of main:Init (L228: ButtonControl constructs its TooltipHost).
fn load_tooltip_class(cx: &mut Cx) {
    if !cx.app().main.tooltip_class_loaded {
        cx.app_mut().main.tooltip_class_loaded = true;
        pob_ui_core::tooltip::tooltip::module_load(cx);
    }
}

/// lua: Main.lua:59-305 main:Init()
/// @pob Modules/Main.lua:59-305 h=7e29b409
pub fn main_init(cx: &mut Cx) {
    // L60
    detect_unicode_support(cx);
    // L61-63
    cx.app_mut().main.modes = default_modes();

    // L65
    cx.main.popups.clear();
    // L66-68
    cx.app_mut().main.shared_item_list = Vec::new();
    cx.app_mut().main.shared_item_set_list = Vec::new();
    cx.app_mut().main.game_accounts = IndexMap::new();

    // L70
    #[allow(unused_mut)]
    let mut ignore_build = false;
    // L71 if arg[1] then
    if let Some(a1) = cx.app().launch.arg.first().cloned() {
        // L72-81 buildSites.ParseImportLinkFromURI / DownloadBuild (U43)
        lua_error!(
            "main:Init: importing {a1:?} needs buildSites (Modules/BuildSiteTools.lua, U43), not ported yet"
        );
        // L82 arg[1] = nil
        #[allow(unreachable_code)]
        {
            cx.app_mut().launch.arg.remove(0);
            // L83
            ignore_build = true;
        }
    }

    // L86
    if !ignore_build {
        // L87
        super::on_frame::set_mode(
            cx,
            "BUILD",
            vec![ModeArg::Bool(false), ModeArg::Str("Unnamed build".to_owned())],
        );
    }
    // L89
    let script_path = cx.sys.platform.script_path();
    let dev_mode = cx.app().launch.dev_mode;
    let installed_mode = cx.app().launch.installed_mode;
    if dev_mode || (script_path == cx.sys.platform.runtime_path() && !installed_mode) {
        // L91 If running in dev mode or standalone mode, put user data in the script path
        cx.app_mut().main.user_path = Some(format!("{script_path}/"));
    } else {
        // L93-94
        match cx.sys.platform.user_path() {
            Ok(p) => {
                // L98
                cx.app_mut().main.user_path = Some(format!("{p}/Path of Building/"));
            }
            Err((invalid_path, err_msg)) => {
                // L94 self.userPath = nil; L96
                cx.app_mut().main.user_path = None;
                super::path_popup::open_path_popup(cx, &invalid_path, &err_msg, ignore_build);
            }
        }
    }

    {
        let m = &mut cx.app_mut().main;
        // L102
        m.build_sort_mode = "NAME".to_owned();
        // L103
        m.connection_protocol = 0.0;
        // L104
        m.node_power_theme = "RED/BLUE".to_owned();
    }
    // L105-107 defaultColorCodes.POSITIVE / NEGATIVE / HIGHLIGHT
    let defaults = pob_ui_core::color_codes::ColorCodes::default();
    {
        let m = &mut cx.app_mut().main;
        m.color_positive = defaults.POSITIVE.clone();
        m.color_negative = defaults.NEGATIVE.clone();
        m.color_highlight = defaults.HIGHLIGHT.clone();
    }
    // L108
    cx.main.show_thousands_separators = true;
    // L109-110
    cx.app_mut().main.use_compact_values = false;
    cx.app_mut().main.edge_search_highlight = true;
    // L111-112
    cx.main.thousands_separator = ",".to_owned();
    cx.main.decimal_separator = ".".to_owned();
    // L113-114
    cx.app_mut().main.default_item_affix_quality = Some(0.5);
    cx.app_mut().main.show_titlebar_name = true;
    // L115 GetDPIScaleOverridePercent and GetDPIScaleOverridePercent() or 0
    let dpi = f64::from(cx.sg.get_dpi_scale_override_percent());
    {
        let m = &mut cx.app_mut().main;
        m.dpi_scale_override_percent = dpi;
        // L116-118
        m.show_warnings = true;
        m.slot_only_tooltips = true;
        m.migrate_eldritch_implicits = true;
    }
    // L119
    cx.main.not_supported_mod_tooltips = true;
    {
        let m = &mut cx.app_mut().main;
        // L120
        m.not_supported_tooltip_text = " ^8(Not supported in PoB yet)".to_owned();
        // L121
        m.show_public_builds = true;
    }
    // L122
    cx.main.show_flavour_text = true;
    {
        let m = &mut cx.app_mut().main;
        // L123-124
        m.show_animations = true;
        m.show_all_item_affixes = true;
    }
    // L125
    cx.main.disable_scroll_control_interaction = false;
    // L126
    cx.app_mut().main.error_reading_settings = false;

    // L128 SetDPIScaleOverridePercent exists (Sg)

    // L130
    let ctrl = cx.sys.is_key_down(Key::Ctrl);
    if (cx.app().launch.dev_mode && ctrl) || getenv("REGENERATE_MOD_CACHE").as_deref() == Some("1")
    {
        // L133 If modLib.parseMod doesn't find a cache entry it generates it.
        cx.app_mut().main.save_new_mod_cache = true;
    } else {
        // L136-138 Load mod cache: DIVERGENCE(mod-cache), the calc plan's parser owns its cache.
    }

    // L147
    cx.app().main.input_events.borrow_mut().wipe();
    // L148 self.tooltipLines = { } (unused)

    // L151 self.tree = { }
    cx.app_mut().trees.trees.clear();
    // L152
    let latest = cx.data().gameVersions.latestTreeVersion.to_string();
    let load_tree = cx.app().main.hooks.load_tree.clone();
    load_tree(cx, &latest);

    // L154
    if let Some(user_path) = cx.app().main.user_path.clone() {
        // L155
        super::path_popup::change_user_path(cx, &user_path, ignore_build);
    }

    // L159
    cx.app_mut().main.unique_db = ItemDb {
        loading: true,
        ..ItemDb::default()
    };
    // L161
    cx.app_mut().main.rare_db = ItemDb {
        loading: true,
        ..ItemDb::default()
    };

    // L203
    if cx.app().main.save_new_mod_cache {
        // L204-205
        let saved = cx.app().main.default_item_affix_quality;
        cx.app_mut().main.default_item_affix_quality = Some(0.5);
        // L206
        load_item_dbs_sync(cx);
        // L207-220: DIVERGENCE(mod-cache): regenerating PoB's Lua mod cache is not supported.
        cx.sys
            .platform
            .con_print("REGENERATE_MOD_CACHE: the port does not write Data/ModCache.lua");
        // L221
        cx.app_mut().main.default_item_affix_quality = saved;
    }

    // L224
    load_tooltip_class(cx);
    let anchor_main = cx
        .add_free(Some(cx.main.node), PlainControl::new(Anchor::NONE, rect(4, 0, 0, 0)))
        .id();
    // L225-227
    cx.base_mut(anchor_main).y = Prop::f(|cx, _| cx.main.screen_h - 4.0);
    cx.app_mut().main.anchor_main = Some(anchor_main);
    let node = cx.main.node;
    // L228-230
    cx.add(
        node,
        "options",
        ButtonControl::new(
            Anchor::new(BottomLeft, anchor_main, BottomLeft),
            rect(0, 0, 68, 20),
            "Options",
            |cx| {
                // L229
                let f = cx.app().main.hooks.open_options_popup.clone();
                f(cx);
            },
        ),
    );
    // L231-233
    cx.add(
        node,
        "about",
        ButtonControl::new(
            Anchor::new(BottomLeft, anchor_main, BottomLeft),
            rect(72, 0, 68, 20),
            "About",
            |cx| {
                // L232 self:OpenAboutPopup()
                let f = cx.app().main.hooks.open_about_popup.clone();
                f(cx, ModeArg::Nil);
            },
        ),
    );
    // L234-236
    let apply_update = cx.add(
        node,
        "applyUpdate",
        ButtonControl::new(
            Anchor::new(BottomLeft, anchor_main, BottomLeft),
            rect(0, -24, 140, 20),
            "^x50E050Update Ready",
            |cx| {
                // L235
                let f = cx.app().main.hooks.open_update_popup.clone();
                f(cx);
            },
        ),
    );
    // L237-239
    cx.base_mut(apply_update).shown = Prop::f(|cx, _| {
        let l = &cx.app().launch;
        l.update_available.is_some() && l.update_available.as_deref() != Some("none")
    });
    // L240-242
    let check_update = cx.add(
        node,
        "checkUpdate",
        ButtonControl::new(
            Anchor::new(BottomLeft, anchor_main, BottomLeft),
            rect(0, -24, 140, 20),
            "",
            |cx| {
                // L241
                crate::launch::check_for_update(cx, None);
            },
        ),
    );
    // L243-245
    cx.base_mut(check_update).shown = Prop::f(|cx, _| {
        let l = &cx.app().launch;
        !l.dev_mode && (l.update_available.is_none() || l.update_available.as_deref() == Some("none"))
    });
    // L246-248
    cx.get_mut(check_update).label = Prop::f(|cx, _| {
        let l = &cx.app().launch;
        match (l.update_check_running, &l.update_progress) {
            (true, Some(p)) => p.clone(),
            _ => "Check for Update".to_owned(),
        }
    });
    // L249-251
    cx.base_mut(check_update).enabled = Prop::f(|cx, _| !cx.app().launch.update_check_running);
    // L252
    let fork_label = cx.add(
        node,
        "forkLabel",
        LabelControl::new(
            Anchor::new(BottomLeft, anchor_main, BottomLeft),
            rect(148, -26, 0, 16),
            "",
        ),
    );
    // L253-255
    cx.get_mut(fork_label).label = Prop::f(|_, _| "^8PoB Community Fork".to_owned());
    // L256
    let version_label = cx.add(
        node,
        "versionLabel",
        LabelControl::new(
            Anchor::new(BottomLeft, anchor_main, BottomLeft),
            rect(148, -2, 0, 16),
            "",
        ),
    );
    // L257-259
    cx.get_mut(version_label).label = Prop::f(|cx, _| {
        let l = &cx.app().launch;
        let branch = l.version_branch.as_deref();
        // `.. launch.versionNumber ..`: a nil versionNumber is a concatenation error
        let number = l
            .version_number
            .as_deref()
            .unwrap_or_else(|| lua_error!("attempt to concatenate field 'versionNumber' (a nil value)"));
        format!(
            "^8{}{}{}",
            if branch == Some("beta") { "Beta: " } else { "Version: " },
            number,
            if branch == Some("dev") { " (Dev)" } else { "" }
        )
    });
    // L260
    let neg = cx.main.cc.NEGATIVE.clone();
    let dev = cx.add(
        node,
        "devMode",
        LabelControl::new(
            Anchor::new(BottomLeft, anchor_main, BottomLeft),
            rect(0, -26, 0, 20),
            format!("{neg}Dev Mode"),
        ),
    );
    // L261-263
    cx.base_mut(dev).shown = Prop::f(|cx, _| cx.app().launch.dev_mode);

    // L265
    cx.app_mut().main.main_bar_height = 58.0;

    // L268 Initialize toast notification system
    super::toast::init(cx, anchor_main);

    // L270
    let now = cx.sys.time_ms();
    if cx.app().launch.dev_mode && (0..15000).contains(&now) {
        // L271-279
        super::toast::add(
            cx,
            "^xFF7700Warning: ^7Developer Mode active!\n\
The program is currently running in developer\n\
mode, which is not intended for normal use.\n\
If you are not expecting this, then you may have\n\
set up the program from the source .zip instead\n\
of using one of the installers. If that is the case,\n\
please reinstall using one of the installers from\n\
the \"Releases\" section of the GitHub page.",
        );
    }

    // L282-290
    cx.main.on_frame_funcs = vec![(
        "FirstFrame".to_owned(),
        Rc::new(|cx: &mut Cx| {
            // L284
            remove_on_frame_func(cx, "FirstFrame");
            // L285-287 if launch.devMode then data.printMissingMinionSkills() (a dev report of
            // the data tables; nothing in the port prints it)
            // L288
            let start = cx.app().launch.start_time;
            let line = pob_lua::lfmt!("Startup time: %d ms", (cx.sys.time_ms() - start) as f64);
            cx.sys.platform.con_print(&line);
        }) as Rc<dyn Fn(&mut Cx)>,
    )];

    // L292
    if !cx.app().main.save_new_mod_cache {
        // L293
        let items_coroutine = Rc::new(RefCell::new(Coroutine::create(load_item_dbs())));
        // L295-303
        let f: Rc<dyn Fn(&mut Cx)> = Rc::new(move |cx: &mut Cx| {
            // L296 local res, errMsg = coroutine.resume(itemsCoroutine)
            let (res, err_msg) = {
                let mut c = items_coroutine.borrow_mut();
                resume_catching(&mut c, cx)
            };
            // L297
            if items_coroutine.borrow().is_dead() {
                // L298
                remove_on_frame_func(cx, "LoadItems");
            }
            // L300
            if !res {
                // L301 error(errMsg)
                lua_error!("{}", err_msg.unwrap_or_default());
            }
        });
        cx.main.on_frame_funcs.push(("LoadItems".to_owned(), f));
    }
}

/// `local res, errMsg = coroutine.resume(co)`: any panic of the body is the Lua error.
fn resume_catching(c: &mut Coroutine, cx: &mut Cx) -> (bool, Option<String>) {
    if c.is_dead() {
        return (false, Some("cannot resume dead coroutine".to_owned()));
    }
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.resume(cx))) {
        Ok(res) => (res, None),
        Err(p) => (false, Some(pob_host::panic_message(&*p))),
    }
}

/// `self.onFrameFuncs[name] = nil`.
pub fn remove_on_frame_func(cx: &mut Cx, name: &str) {
    cx.main.on_frame_funcs.retain(|(k, _)| k != name);
}

/// `os.getenv(name)` (no environment on the web).
fn getenv(name: &str) -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var(name).ok()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = name;
        None
    }
}

/// `pairsYield`'s yield test (Common.lua:646-649): inside a coroutine, yield once 20 ms passed.
async fn pairs_yield_tick(start: &mut i64) {
    let now = co::with_cx(|cx| cx.sys.time_ms());
    if co::running() && now - *start > 20 {
        co::yield_now().await;
        *start = co::with_cx(|cx| cx.sys.time_ms());
    }
}

/// One unique of `data.uniques` (L166-172).
fn add_unique(cx: &mut Cx, ty: &str, raw: &str) {
    // L166
    let new_item = super::new_item(Some(raw), Some("UNIQUE"), true);
    // L167
    if new_item.base.get().is_some() {
        // L168-169
        let name = new_item.name.get().to_string();
        let title = new_item
            .title
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'title' (a nil value)"))
            .to_ascii_lowercase();
        let m = &mut cx.app_mut().main;
        m.unique_db.list.insert(name, new_item.clone());
        m.unique_db.by_title.insert(title, new_item);
    } else if cx.app().launch.dev_mode {
        // L171
        let line = pob_lua::lfmt!("Unique DB unrecognised item of type '%s':\n%s", ty, raw);
        cx.sys.platform.con_print(&line);
    }
}

/// One rare of `data.rares` (L180-196).
fn add_rare(cx: &mut Cx, raw: &str) {
    // L180
    let new_item = super::new_item(Some(raw), Some("RARE"), true);
    // L181
    if let Some(base) = new_item.base.get() {
        // L182
        if new_item.extra.truthy(n!("crafted")) {
            let lines = new_item.implicitModLines.get();
            // L183
            if let (Some(implicit), true) = (base.implicit.as_deref(), lines.with(Vec::is_empty)) {
                // L184-185 Automatically add implicit
                let mut implicit_index = 1usize;
                // L186 for line in newItem.base.implicit:gmatch("[^\n]+") do
                for line in implicit.split('\n').filter(|l| !l.is_empty()) {
                    // L187
                    let mod_line = model::Table::new();
                    mod_line.set(n!("line"), line);
                    let tags = model::Table::new();
                    if let Some(t) = base.implicitModTypes.get(implicit_index - 1) {
                        for tag in t {
                            tags.push(*tag);
                        }
                    }
                    mod_line.set(n!("modTags"), tags);
                    lines.push(mod_line);
                    // L188
                    implicit_index += 1;
                }
            }
            // L191
            new_item.Craft();
        }
        // L193
        let name = new_item.name.get().to_string();
        cx.app_mut().main.rare_db.list.insert(name, new_item);
    } else if cx.app().launch.dev_mode {
        // L195
        let line = pob_lua::lfmt!("Rare DB unrecognised item:\n%s", raw);
        cx.sys.platform.con_print(&line);
    }
}

/// `loadItemDBs` as the body of `coroutine.create(loadItemDBs)` (L293).
/// lua: Main.lua:163-201 (the local function loadItemDBs)
async fn load_item_dbs() {
    let data = co::with_cx(|cx| cx.data());
    // L164 for type, typeList in pairsYield(data.uniques) do
    let mut start = co::with_cx(|cx| cx.sys.time_ms());
    for (ty, type_list) in &data.uniques {
        pairs_yield_tick(&mut start).await;
        // L165 for _, raw in pairs(typeList) do
        for raw in type_list {
            co::with_cx(|cx| add_unique(cx, ty.as_str(), raw));
        }
    }
    // the iterator's last call (next returns nil) also runs the yield test
    pairs_yield_tick(&mut start).await;

    co::with_cx(|cx| {
        // L176
        cx.app_mut().main.unique_db.loading = false;
        // L177
        cx.sys.platform.con_print("Uniques loaded");
    });

    // L179 for _, raw in pairsYield(data.rares) do
    let mut start = co::with_cx(|cx| cx.sys.time_ms());
    for raw in &data.rares {
        pairs_yield_tick(&mut start).await;
        co::with_cx(|cx| add_rare(cx, raw));
    }
    pairs_yield_tick(&mut start).await;

    co::with_cx(|cx| {
        // L199
        cx.app_mut().main.rare_db.loading = false;
        // L200
        cx.sys.platform.con_print("Rares loaded");
    });
}

/// `loadItemDBs()` called directly (L206): no coroutine, so `pairsYield` never yields.
fn load_item_dbs_sync(cx: &mut Cx) {
    let data = cx.data();
    for (ty, type_list) in &data.uniques {
        for raw in type_list {
            add_unique(cx, ty.as_str(), raw);
        }
    }
    cx.app_mut().main.unique_db.loading = false;
    cx.sys.platform.con_print("Uniques loaded");
    for raw in &data.rares {
        add_rare(cx, raw);
    }
    cx.app_mut().main.rare_db.loading = false;
    cx.sys.platform.con_print("Rares loaded");
}

/// lua: Main.lua:307-313 main:DetectUnicodeSupport()
/// @pob Modules/Main.lua:307-313 h=a30bd65e
pub fn detect_unicode_support(cx: &mut Cx) {
    // L309 PoeCharm has utf8 global that normal PoB doesn't have
    cx.main.unicode = false;
    // L310
    if cx.main.unicode {
        // L311
        cx.sys.platform.con_print("Unicode support detected");
    }
}

// @pob-skip Modules/Main.lua:315-348 reason="main:SaveModCache writes PoB's Lua mod cache (Data/ModCache.lua); the port's parser cache is the calc plan's (DIVERGENCE(mod-cache))"

/// lua: Main.lua:365-372 main:CanExit()
/// @pob Modules/Main.lua:365-372 h=1541dd12
pub fn can_exit(cx: &mut Cx) -> bool {
    // L366
    let ret = super::on_frame::current_mode(cx).and_then(|m| m.can_exit(cx, "EXIT"));
    // L367-371
    ret.unwrap_or(true)
}

/// lua: Main.lua:374-377 main:Shutdown()
/// @pob Modules/Main.lua:374-377 h=9e0eba4c
pub fn shutdown(cx: &mut Cx) {
    // L375
    if let Some(m) = super::on_frame::current_mode(cx) {
        m.shutdown(cx);
    }
    // L376
    super::settings::save_settings(cx);
}
