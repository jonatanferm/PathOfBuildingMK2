//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ExtBuildListControl.lua 1-440 (the
//! list control of the external build providers) and Modules/Common.lua:1081-1105
//! (`ImportBuild`, whose only caller is this control). Owner: U52.
//!
//! @pob-file Classes/ExtBuildListControl.lua @16de4b82d
//! @pob-file Modules/Common.lua @16de4b82d
//!
//! `newClass("ExtBuildListControl", "ControlHost", "Control")`: a host node with `Draw`,
//! `IsMouseOver`, `OnKeyDown` and `OnKeyUp`. `providers` is the Lua list `{ { name = ..,
//! impl = provider } }` ([`ProviderEntry`]).
//!
//! Lua semantics kept:
//! * **The draw body is dead in PoB 2.67.2.** `Draw` returns at `if true then return end` (L249,
//!   the commented-out `if not main.showPublicBuilds then`), so PoB never draws the list, its
//!   tabs, its dropdown or its buttons (and never creates the Import/Preview buttons). The port
//!   keeps the body after the guard, statement for statement; [`draw_body`] is the guard
//!   (`false`, as in PoB). LUA-QUIRK. The acceptance test turns it on on both sides (the Lua side
//!   by a text patch of the guard) to compare the body against PoB's.
//! * The controls are still there for input when the body is dead: the tab buttons, the
//!   provider dropdown, the scroll bar and "See All" are hit-tested through `IsMouseOver` /
//!   `GetMouseOverControl`, and clicking a tab fetches its list.
//! * **`self.controls` mixes string and integer keys** (`sort`, `scrollBarV`, `all`, and the tab
//!   and Import/Preview buttons appended with `t_insert`). The integer keys `t_insert` picks are
//!   `#self.controls + 1` after Draw has set some of them to nil (L285-289), so the port keeps a
//!   model of the Lua table's layout ([`pob_lua::ljtab::LjTab`] over [`CKey`], every store into
//!   `self.controls` mirrored) for LuaJIT's `#` border. Rehashes happen exactly when a new key
//!   meets a full hash part, so the layout does not depend on where LuaJIT's per-process string
//!   hashes place the three string keys (the stand-in hash places them elsewhere).
//! * `self.width()` / `self.height()` call the field (the callers always store functions there);
//!   a number there is the Lua call error ([`call_field`]).
//! * `ascendancy:gsub("^%l", string.lower)` lower-cases a first letter that is already lower
//!   case (a no-op): `Assets/ascendants/<Name>.jpeg` exists only for lower-case names, so on a
//!   case-sensitive file system PoB finds no portrait for "Necromancer". Kept.
//! * `importBuild` reads `build.authorName`, which `ParseBuilds` never sets (it sets `author`):
//!   the mode's build name is always `build.buildName`. Kept.
//! * `OnKeyDown` passes no `doubleClick` on (L156).
//!
//! DIVERGENCE(gc): the Import/Preview buttons Draw re-creates every frame, and the controls
//! `Init` wipes, may still be a parent host's `selControl` (a button pressed in the frame that
//! replaced it gets its `OnKeyUp` from the parent). Lua's GC keeps such an object alive; the
//! arena frees a buried node at the end of the frame. A control removed from `self.controls`
//! while it has the focus (`hasFocus`, which `SelectControl` sets and clears) is therefore kept
//! (owned by this control) and freed at the next `Init`/`Draw` after it lost the focus.
//! DIVERGENCE(print): Lua `print` goes to the console (`ConPrintf`), joined by tabs.
//! ORDER: `pairs(self.activeListProvider.buildList)` (L301), `pairs(lines)` (L317, L330) and
//! `pairs(build.metadata)` (L399) iterate arrays (index order). `wipeTable(self.controls)` uses
//! the modelled table's `pairs` order (unobservable: every key becomes nil). `self.controls`
//! draw/hit-test order is the canonical one (oracle patch 6).
//! Props with side effects (ui doc §7.3): none.
#![allow(non_snake_case, clippy::used_underscore_binding)] // Lua locals verbatim (calc doc §3.1 rule 1)
#![allow(clippy::assign_op_pattern)] // `x = x + 10` as the Lua writes it (statement order)

use std::cell::Cell;
use std::rc::Rc;

use pob_lua::ljtab::{hashnum, str_sid_standin, LjKey, LjTab};
use pob_lua::math::{m_max, m_min};
use pob_sg::{Align, Font, ImageFlags, ImageHandle};
use pob_ui_core::controls::scroll_bar::ScrollDir;
use pob_ui_core::controls::{ButtonControl, DropDownControl, DropItem, ScrollBarControl};
use pob_ui_core::luastr::{format_num_sep_main, gsub, lmatch};
use pob_ui_core::prelude::*;

use super::provider::{ExtBuild, LVal, ProviderRef};
use crate::main_mode::on_frame::set_mode;
use crate::main_mode::ModeArg;
use crate::tabs::import::build_sites::{download_build, WEBSITE_LIST};
use crate::tabs::import::core::{decode_build_code, UrlDecode};

thread_local! {
    static DRAW_BODY: Cell<bool> = const { Cell::new(false) };
}

/// The `if true then return end` guard of `Draw` (L249): `false` (PoB's dead body) unless a
/// test turned the body on with [`set_draw_body`]. The product never sets it.
pub fn draw_body() -> bool {
    DRAW_BODY.with(Cell::get)
}

/// Test seam: run `Draw` past its dead-code guard (module notes). Changes nothing when unset.
pub fn set_draw_body(on: bool) {
    DRAW_BODY.with(|c| c.set(on));
}

/// One entry of the `providers` list: `{ name = .., impl = provider }`.
#[derive(Clone)]
pub struct ProviderEntry {
    pub name: String,
    pub r#impl: ProviderRef,
}

/// A key of the modelled `self.controls` table (module notes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CKey {
    Int(i64),
    Str(&'static str),
}

impl LjKey for CKey {
    fn as_int(&self) -> Option<i64> {
        match self {
            CKey::Int(i) => Some(*i),
            CKey::Str(_) => None,
        }
    }
    fn from_int(i: i64) -> Self {
        CKey::Int(i)
    }
    fn hash(&self) -> u32 {
        match self {
            CKey::Int(i) => hashnum(*i),
            CKey::Str(s) => str_sid_standin(s),
        }
    }
    fn is_str(&self) -> bool {
        matches!(self, CKey::Str(_))
    }
}

impl CKey {
    fn ctrl_key(self) -> CtrlKey {
        match self {
            CKey::Int(i) => CtrlKey::Idx(i),
            CKey::Str(s) => CtrlKey::from(s),
        }
    }
}

/// `self.width()` / `self.height()` (and `self.controls.sort.width()`): call the field.
#[derive(Clone, Copy)]
enum Field {
    Width,
    Height,
}

/// Call a function-valued field without arguments (module notes).
#[track_caller]
fn call_field(cx: &Cx, id: CtrlId, f: Field) -> f64 {
    let (p, name) = match f {
        Field::Width => (cx.base(id).width.clone(), "width"),
        Field::Height => (cx.base(id).height.clone(), "height"),
    };
    match p {
        Prop::Fn(func) => func(cx, id),
        Prop::Val(_) => lua_error!("attempt to call field '{name}' (a number value)"),
    }
}

/// Lua `print(...)` (DIVERGENCE(print)).
fn print(cx: &mut Cx, args: &[&str]) {
    let line = args.join("\t");
    cx.sys.platform.con_print(&line);
}

/// An `ExtBuildListControl` object.
pub struct ExtBuildListControl {
    /// `self.rowHeight`
    pub row_height: f64,
    /// `self.scroll` ("VERTICAL")
    pub scroll: Option<&'static str>,
    /// `self.forceTooltip`
    pub force_tooltip: bool,
    /// `self.font` ("VAR")
    pub font: Font,
    /// `self.importButtons` (wiped every Draw, never filled)
    pub import_buttons: Vec<CtrlId>,
    /// `self.previewButtons` (wiped every Draw, never filled)
    pub preview_buttons: Vec<CtrlId>,
    /// `self.inTransition`
    pub in_transition: bool,
    /// `self.contentHeight`
    pub content_height: f64,
    /// `self.tabs`
    pub tabs: Vec<CtrlId>,
    /// `self.activeListProvider`
    pub active_list_provider: Option<ProviderRef>,
    /// `self.buildProviders`
    pub build_providers: Rc<Vec<ProviderEntry>>,
    /// `self.buildProvidersList` (the provider names)
    pub build_providers_list: Vec<String>,
    /// `self.providerMaxLength`
    pub provider_max_length: f64,
    /// `self.importCode`
    pub import_code: Option<String>,
    /// `self.scrollOffsetV`
    pub scroll_offset_v: Option<f64>,
    /// The layout of the Lua `self.controls` table (module notes).
    keys: LjTab<CKey, ()>,
    /// Controls removed from `self.controls` while focused (DIVERGENCE(gc)).
    stale: Vec<CtrlId>,
}

impl ExtBuildListControl {
    /// lua: ExtBuildListControl.lua:16-39 ExtBuildListControl(anchor, rect, providers)
    /// @pob Classes/ExtBuildListControl.lua:16-39 h=227c7711
    pub fn new(anchor: Anchor, rect: [Prop<f64>; 4], providers: Vec<ProviderEntry>) -> New<Self> {
        // L17 self:Control(anchor, rect); L18 self:ControlHost()
        New::new(
            Base::new(anchor, rect),
            ExtBuildListControl {
                // L21
                row_height: 200.0,
                // L22
                scroll: Some("VERTICAL"),
                // L23
                force_tooltip: false,
                // L24
                font: Font::Var,
                // L25
                import_buttons: Vec::new(),
                // L26
                preview_buttons: Vec::new(),
                // L27
                in_transition: false,
                // L28
                content_height: 0.0,
                // L29
                tabs: Vec::new(),
                // L30
                active_list_provider: None,
                // L31
                build_providers: Rc::new(providers),
                // L32
                build_providers_list: Vec::new(),
                // L33
                provider_max_length: 150.0,
                import_code: None,
                scroll_offset_v: None,
                keys: LjTab::new(),
                stale: Vec::new(),
            },
        )
        .host()
        .init(|cx, me| {
            let me = me.id();
            // L19
            cx.select_control(me, None);
            // L34
            let providers = cx.w::<Self>(me).build_providers.clone();
            for provider in providers.iter() {
                let font = cx.w::<Self>(me).font;
                // L35
                let w = cx.sg.draw_string_width(16.0, font, &provider.name) + 30.0;
                let st = cx.w_mut::<Self>(me);
                st.provider_max_length = m_max(st.provider_max_length, w);
                // L36
                st.build_providers_list.push(provider.name.clone());
            }
            // L38 return self
        })
    }

    // ---- the modelled `self.controls` (module notes) ----

    /// `self.controls[k]` (the arena child under the same key).
    fn controls_get(cx: &Cx, me: CtrlId, k: CKey) -> Option<CtrlId> {
        if cx.w::<Self>(me).keys.contains_key(k) {
            cx.child(me, k.ctrl_key())
        } else {
            None
        }
    }

    /// `self.controls[k] = new(..)`.
    fn controls_add<W: Widget>(cx: &mut Cx, me: CtrlId, k: CKey, n: New<W>) -> Ctrl<W> {
        let c = cx.add(me, k.ctrl_key(), n);
        cx.w_mut::<Self>(me).keys.set(k, Some(()));
        c
    }

    /// `t_insert(self.controls, new(..))`: at `#self.controls + 1`.
    fn controls_insert<W: Widget>(cx: &mut Cx, me: CtrlId, n: New<W>) -> Ctrl<W> {
        let i = cx.w::<Self>(me).keys.len() as i64 + 1;
        Self::controls_add(cx, me, CKey::Int(i), n)
    }

    /// `self.controls[k] = nil`.
    fn controls_remove(cx: &mut Cx, me: CtrlId, k: CKey) {
        cx.w_mut::<Self>(me).keys.set(k, None);
        if let Some(id) = cx.detach_child(me, &k.ctrl_key()) {
            Self::release(cx, me, id);
        }
    }

    /// `wipeTable(self.controls)`.
    fn controls_wipe(cx: &mut Cx, me: CtrlId) {
        // ORDER: module notes (the modelled table's `pairs` order)
        let keys = cx.w::<Self>(me).keys.keys();
        for k in keys {
            Self::controls_remove(cx, me, k);
        }
    }

    /// Free a control that left `self.controls`, unless a host still has it selected
    /// (DIVERGENCE(gc)).
    fn release(cx: &mut Cx, me: CtrlId, id: CtrlId) {
        if cx.base(id).has_focus == Some(true) {
            cx.w_mut::<Self>(me).stale.push(id);
        } else {
            cx.bury(id);
        }
    }

    /// Free the kept controls that lost the focus (DIVERGENCE(gc)).
    fn free_stale(cx: &mut Cx, me: CtrlId) {
        let stale = std::mem::take(&mut cx.w_mut::<Self>(me).stale);
        let mut keep = Vec::new();
        for id in stale {
            if cx.ui.is_alive(id) && cx.base(id).has_focus == Some(true) {
                keep.push(id);
            } else if cx.ui.is_alive(id) {
                cx.bury(id);
            }
        }
        cx.w_mut::<Self>(me).stale = keep;
    }

    /// `self.controls.scrollBarV` (a nil index error when Init returned early).
    #[track_caller]
    fn scroll_bar_v(cx: &Cx, me: CtrlId) -> CtrlId {
        Self::controls_get(cx, me, CKey::Str("scrollBarV"))
            .unwrap_or_else(|| lua_error!("attempt to index field 'scrollBarV' (a nil value)"))
    }

    /// `self.activeListProvider` (a nil index error when there is none).
    #[track_caller]
    fn provider(cx: &Cx, me: CtrlId) -> ProviderRef {
        cx.w::<Self>(me)
            .active_list_provider
            .clone()
            .unwrap_or_else(|| {
                lua_error!("attempt to index field 'activeListProvider' (a nil value)")
            })
    }

    /// lua: ExtBuildListControl.lua:41-137 Init(providerName)
    /// @pob Classes/ExtBuildListControl.lua:41-137 h=d871d4f6
    pub fn init(cx: &mut Cx, me: CtrlId, providerName: Option<&str>) {
        Self::free_stale(cx, me);
        // L42
        Self::controls_wipe(cx, me);
        // L43
        cx.w_mut::<Self>(me).tabs.clear();

        // L45-48
        let (list, max_len) = {
            let w = cx.w::<Self>(me);
            (w.build_providers_list.clone(), w.provider_max_length)
        };
        let sort = Self::controls_add(
            cx,
            me,
            CKey::Str("sort"),
            DropDownControl::new(
                Anchor::new(Top, me, Top),
                rect(0, -20, max_len, 20),
                list.into_iter().map(DropItem::str).collect(),
                Some(Rc::new(
                    move |cx: &mut Cx, _index: usize, value: Option<&DropItem>| {
                        // L47
                        let value = value.and_then(DropItem::as_str).map(str::to_owned);
                        Self::init(cx, me, value.as_deref());
                    },
                )),
                None,
                false,
            ),
        );

        // L50
        DropDownControl::sel_by_value(cx, sort.id(), &|it| it.as_str() == providerName);

        // L52
        cx.w_mut::<Self>(me).active_list_provider = None;

        // L54
        let mut tabWidth = 0.0;

        // L56
        let providers = cx.w::<Self>(me).build_providers.clone();
        for provider in providers.iter() {
            // L57
            if Some(provider.name.as_str()) == providerName {
                // L58
                cx.w_mut::<Self>(me).active_list_provider = Some(provider.r#impl.clone());
            }
        }

        // L62
        let Some(active) = cx.w::<Self>(me).active_list_provider.clone() else {
            // L63
            print(
                cx,
                &[
                    "Build provider not found: %s",
                    providerName.unwrap_or("nil"),
                ],
            );
            // L64
            return;
        };

        // L67
        let import_code = cx.w::<Self>(me).import_code.clone();
        active.set_import_code(import_code);
        // L68
        active.clone().activate(cx);
        // L69
        let titles = active.get_list_titles();
        active.base().borrow_mut().buildListTitles = titles;

        // L71
        let mut lastControl: Option<CtrlId> = None;
        // L72
        let titles = Self::provider(cx, me)
            .get_list_titles()
            .unwrap_or_else(|| lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)"));
        for (i, title) in titles.into_iter().enumerate() {
            let index = i + 1;
            let font = cx.w::<Self>(me).font;
            // L73
            let stringWidth = cx.sg.draw_string_width(16.0, font, &title);
            // L74
            let mut anchor = Anchor::new(Top, me, Top);
            // L75
            if let Some(last) = lastControl {
                // L76
                anchor = Anchor::new(Left, last, Right);
            }
            // L78-86
            let t = title.clone();
            let n = ButtonControl::new(
                anchor,
                rect(
                    0,
                    if lastControl.is_some() { 0.0 } else { -20.0 },
                    stringWidth + 10.0,
                    20,
                ),
                title.as_str(),
                move |cx| {
                    let p = Self::provider(cx, me);
                    // L79
                    if p.get_active_list().as_deref() == Some(t.as_str()) {
                        // L80
                        return;
                    }
                    // L82
                    p.set_active_list(cx, Some(&t));
                    // L83
                    let tabs = cx.w::<Self>(me).tabs.clone();
                    for _button in tabs {
                        // L84
                        let label = cx.w::<ButtonControl>(_button).label.raw();
                        cx.w_mut::<ButtonControl>(_button).locked = Prop::Val(label == t);
                    }
                },
            );
            // L98 t_insert(self.controls, button) (stored here: L88-96 only write the button's
            // own fields, which nothing reads in between)
            let button = Self::controls_insert(cx, me, n).id();

            // L88
            cx.w_mut::<ButtonControl>(button).locked = Prop::Val(index == 1);

            // L90
            tabWidth = tabWidth + stringWidth + 10.0;

            // L92 -- button.enabled = self.mode ~= "latest"
            // L93
            if lastControl.is_none() {
                // L94-96
                cx.base_mut(button).x = Prop::f(move |cx, _| {
                    (stringWidth + 10.0 - call_field(cx, me, Field::Width)) / 2.0
                });
            }
            // L99
            cx.w_mut::<Self>(me).tabs.push(button);
            // L100
            lastControl = Some(button);
        }

        // L103 -- responsiveness
        // L104-106
        let sort_id = Self::controls_get(cx, me, CKey::Str("sort"))
            .unwrap_or_else(|| lua_error!("attempt to index field 'sort' (a nil value)"));
        cx.base_mut(sort_id).width = Prop::f(move |cx, _| {
            // L105
            m_min(150.0, call_field(cx, me, Field::Width) - tabWidth)
        });
        // L107-109
        let sort_id = Self::controls_get(cx, me, CKey::Str("sort"))
            .unwrap_or_else(|| lua_error!("attempt to index field 'sort' (a nil value)"));
        cx.base_mut(sort_id).x = Prop::f(move |cx, _| {
            // L108
            let sort = Self::controls_get(cx, me, CKey::Str("sort"))
                .unwrap_or_else(|| lua_error!("attempt to index field 'sort' (a nil value)"));
            (call_field(cx, me, Field::Width) - call_field(cx, sort, Field::Width)) / 2.0
        });

        // L111-120
        let scroll = cx.w::<Self>(me).scroll;
        Self::controls_add(
            cx,
            me,
            CKey::Str("scrollBarV"),
            ScrollBarControl::new(
                Anchor::new(Right, me, Right),
                rect(-1, 0, if scroll.is_some() { 16.0 } else { 0.0 }, 0),
                Some(80.0),
                Some(ScrollDir::Vertical),
                false,
            )
            // L113-115 -- y = function() return (self.scrollH and -8 or 0) end,
            .height(Prop::f(move |cx, _| {
                // L117
                let (_, height) = cx.size(me);
                // L118 (`self.scrollH` is never set)
                height - 2.0 - 0.0
            })),
        );
        // L121
        if cx.w::<Self>(me).scroll.is_none() {
            // L122
            let sb = Self::scroll_bar_v(cx, me);
            cx.base_mut(sb).shown = Prop::Val(false);
        }

        // L125
        if Self::provider(cx, me).get_page_url().is_some() {
            // L126-132
            let width = cx.base(me).width.clone();
            let all = Self::controls_add(
                cx,
                me,
                CKey::Str("all"),
                ButtonControl::new(
                    Anchor::new(Bottom, me, Bottom),
                    [Prop::Val(0.0), Prop::Val(1.0), width, Prop::Val(20.0)],
                    "See All",
                    move |cx| {
                        // L128
                        let url = Self::provider(cx, me).get_page_url();
                        // L129
                        if let Some(url) = url {
                            // L130
                            cx.sys.platform.open_url(&url);
                        }
                    },
                ),
            );
            // L133-135
            cx.base_mut(all).width = Prop::f(move |cx, _| call_field(cx, me, Field::Width));
        }
    }

    /// lua: ExtBuildListControl.lua:139-141 SetImportCode(importCode)
    /// @pob Classes/ExtBuildListControl.lua:139-141 h=e1799742
    pub fn set_import_code(cx: &mut Cx, me: CtrlId, importCode: Option<String>) {
        // L140
        cx.w_mut::<Self>(me).import_code = importCode;
    }

    /// lua: ExtBuildListControl.lua:175-187 importBuild(build)
    /// @pob Classes/ExtBuildListControl.lua:175-187 h=3fa6c184
    pub fn import_build(cx: &mut Cx, me: CtrlId, build: &ExtBuild) {
        let _ = me;
        // L176
        if !LVal::truthy(build.buildLink.as_ref()) {
            // L177
            print(cx, &["Build link is not provided."]);
            // L178
            return;
        }
        // L180
        let link = LVal::method_string(build.buildLink.as_ref(), "local 'importLink'").to_owned();
        let name = build.buildName.clone();
        ImportBuild(
            cx,
            &link,
            Rc::new(
                move |cx: &mut Cx, xmlText: Option<String>, urlText: Option<String>| {
                    // L181
                    if let Some(xmlText) = xmlText {
                        // L182-184 `build.authorName` is never set (module notes): the `and` is nil
                        let name = LVal::concat(name.as_ref(), "field 'buildName'");
                        set_mode(
                            cx,
                            "BUILD",
                            vec![
                                ModeArg::Bool(false),
                                ModeArg::Str(name),
                                ModeArg::Str(xmlText),
                                ModeArg::Bool(false),
                                ModeArg::from(urlText),
                            ],
                        );
                    }
                },
            ),
        );
    }

    /// lua: ExtBuildListControl.lua:189-203 GetAscendancyImageHandle(ascendancy)
    /// @pob Classes/ExtBuildListControl.lua:189-203 h=76bafb2f
    pub fn get_ascendancy_image_handle(
        cx: &mut Cx,
        ascendancy: Option<&LVal>,
    ) -> Option<ImageHandle> {
        // L190
        if LVal::truthy(ascendancy) {
            // L191
            let a = LVal::method_string(ascendancy, "local 'ascendancy'");
            let lowered = pob_lua::gsub_fn(
                a,
                "^%l",
                |c| Some(pob_lua::string::lower(&c[0].to_str())),
                None,
            )
            .0;
            let fileName = format!("Assets/ascendants/{lowered}.jpeg");
            // L192 local file = io.open(fileName, "r")
            // L193
            if crate::platform::storage::file_exists(cx, &fileName) {
                // L194 file:close()
                // L195
                let image = cx.sg.new_image_handle();
                // L196
                image.load(&mut cx.sg, &fileName, ImageFlags::empty());
                // L197
                return Some(image);
            }
        }

        // L202
        None
    }

    /// splits strings by word and maxWidth
    /// lua: ExtBuildListControl.lua:205-226 splitStringByWidth(str, maxWidth, font)
    /// @pob Classes/ExtBuildListControl.lua:205-226 h=e43fc81d
    pub fn split_string_by_width(
        cx: &Cx,
        str: Option<&LVal>,
        maxWidth: f64,
        font: Font,
    ) -> Vec<String> {
        // L207
        let mut words: Vec<String> = Vec::new();
        // L208
        let s = LVal::method_string(str, "local 'str'");
        for c in pob_lua::gmatch(s, "%S+") {
            // L209
            words.push(c[0].to_str().into_owned());
        }

        // L212
        let mut lines: Vec<String> = Vec::new();
        // L213
        let mut currentLine = String::new();
        // L214
        for word in &words {
            // L215
            let wordWidth = cx
                .sg
                .draw_string_width(16.0, font, &format!("{currentLine} {word}"));
            // L216
            if wordWidth <= maxWidth {
                // L217
                currentLine = format!(
                    "{currentLine}{}{word}",
                    if currentLine.is_empty() { "" } else { " " }
                );
            } else {
                // L219
                lines.push(currentLine);
                // L220
                currentLine = word.clone();
            }
        }
        // L223
        lines.push(currentLine);

        // L225
        lines
    }

    /// wrappers for Drawing tools to apply scrolling
    /// lua: ExtBuildListControl.lua:228-234 DrawImage(imgHandle, left, top, width, height)
    /// @pob Classes/ExtBuildListControl.lua:228-234 h=c99b1cfe
    pub fn draw_image(
        cx: &mut Cx,
        me: CtrlId,
        imgHandle: Option<&ImageHandle>,
        left: f64,
        top: f64,
        width: f64,
        height: f64,
    ) {
        // L230
        let (_, y) = cx.pos(me);
        // L231
        let offset = cx.w::<ScrollBarControl>(Self::scroll_bar_v(cx, me)).offset;
        if top - offset >= y && top + height - offset < call_field(cx, me, Field::Height) + y {
            // L232
            let offset = cx.w::<ScrollBarControl>(Self::scroll_bar_v(cx, me)).offset;
            cx.sg
                .draw_image(imgHandle, left, top - offset, width, height);
        }
    }

    /// lua: ExtBuildListControl.lua:236-241 DrawString(left, top, align, height, font, text)
    /// @pob Classes/ExtBuildListControl.lua:236-241 h=fe6d2db3
    pub fn draw_string(
        cx: &mut Cx,
        me: CtrlId,
        left: f64,
        top: f64,
        align: Align,
        height: f64,
        font: Font,
        text: &str,
    ) {
        // L237
        let (_, y) = cx.pos(me);
        // L238
        let offset = cx.w::<ScrollBarControl>(Self::scroll_bar_v(cx, me)).offset;
        if top - offset >= y && top + height - offset < call_field(cx, me, Field::Height) + y {
            // L239
            let offset = cx.w::<ScrollBarControl>(Self::scroll_bar_v(cx, me)).offset;
            cx.sg
                .draw_string(left, top - offset, align, height, font, text);
        }
    }
}

impl Widget for ExtBuildListControl {
    const NAME: &'static str = "ExtBuildListControl";
    const CAPS: Caps = Caps::DRAW
        .union(Caps::IS_MOUSE_OVER)
        .union(Caps::KEY_DOWN)
        .union(Caps::KEY_UP);

    /// lua: ExtBuildListControl.lua:143-148 IsMouseOver()
    /// @pob Classes/ExtBuildListControl.lua:143-148 h=30f0cf64
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        // L144
        if !cx.is_shown(me) {
            // L145
            return false;
        }
        // L147
        cx.is_mouse_in_bounds(me) || cx.get_mouse_over_control(me).is_some()
    }

    /// lua: ExtBuildListControl.lua:150-161 OnKeyDown(key, doubleClick)
    /// @pob Classes/ExtBuildListControl.lua:150-161 h=831fd0a8
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, _double_click: bool) -> Focus {
        // L151
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L152
            return None;
        }
        // L154
        let mOverControl = cx.get_mouse_over_control(me);
        // L155
        if let Some(m) = mOverControl.filter(|&m| cx.has(m, Caps::KEY_DOWN)) {
            // L156 (no doubleClick passed on)
            return cx.on_key_down(m, key, false);
        }
        // L158
        if !cx.is_mouse_over(me) && key.is_button() {
            // L159
            return None;
        }
        None
    }

    /// lua: ExtBuildListControl.lua:163-173 OnKeyUp(key)
    /// @pob Classes/ExtBuildListControl.lua:163-173 h=e2f7b802
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        // L164
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L165
            return None;
        }

        // L168
        let sb = Self::scroll_bar_v(cx, me);
        if ScrollBarControl::is_scroll_down_key(cx, sb, key) {
            // L169
            ScrollBarControl::scroll(cx, sb, 1.0);
        } else if ScrollBarControl::is_scroll_up_key(cx, sb, key) {
            // L170-171
            ScrollBarControl::scroll(cx, sb, -1.0);
        }
        None
    }

    /// lua: ExtBuildListControl.lua:243-440 Draw(viewPort, noTooltip)
    /// @pob Classes/ExtBuildListControl.lua:243-440 h=f0f57024
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        Self::free_stale(cx, me);
        // L244
        if cx.w::<Self>(me).active_list_provider.is_none() {
            // L245
            return;
        }

        // L248 --if not main.showPublicBuilds then
        // L249 (LUA-QUIRK: `if true then`, module notes)
        if !draw_body() {
            // L250
            return;
        }

        // L253 -- clear button states
        {
            let w = cx.w_mut::<Self>(me);
            // L254
            w.preview_buttons.clear();
            // L255
            w.import_buttons.clear();
        }

        // L257 -- get variables
        // L258
        let (mut x, mut y) = cx.pos(me);
        // L259
        let (width, height) = cx.size(me);

        // L261 -- drawing area
        // L262 -- SetDrawColor(0.5, 0.5, 0.5)
        // L263
        cx.sg.set_draw_color(1.0, 1.0, 1.0);
        // L264
        cx.sg.draw_image(None, x, y, width, height);
        // L265 -- borders
        // L266
        cx.sg.set_draw_color(0.0, 0.0, 0.0);
        // L267
        cx.sg
            .draw_image(None, x + 1.0, y + 1.0, width - 2.0, height - 2.0);
        // L268
        cx.sg.set_draw_color(1.0, 1.0, 1.0);

        // L270 -- apply padding
        // L271
        x = x + 10.0;
        // L272
        y = y + 10.0;

        // L274
        let mut currentHeight = y;
        let font = cx.w::<Self>(me).font;

        // L276 -- write status message
        // L277
        let status = Self::provider(cx, me).base().borrow().statusMsg.clone();
        if let Some(status) = status {
            // L278
            Self::draw_string(cx, me, x, currentHeight, Align::Left, 16.0, font, &status);
        }

        // L281
        let scrollBarV = Self::scroll_bar_v(cx, me);
        // L282
        let off = cx.w::<ScrollBarControl>(scrollBarV).offset;
        cx.w_mut::<Self>(me).scroll_offset_v = Some(off);

        // L284 -- remove import/export buttons.
        // L285 for i, control in ipairs(self.controls) do (read live each step)
        let mut i = 0;
        loop {
            i += 1;
            let Some(control) = Self::controls_get(cx, me, CKey::Int(i)) else {
                break;
            };
            // L286 (`control.label`: the raw field; nil on a non-button)
            let label = cx.try_w::<ButtonControl>(control).map(|b| b.label.raw());
            if matches!(label.as_deref(), Some("Import" | "Preview")) {
                // L287
                Self::controls_remove(cx, me, CKey::Int(i));
            }
        }

        // L291-298
        let addSeparator = move |cx: &mut Cx, y: f64, fillH: f64| -> f64 {
            // L292
            let y = y + 4.0;
            // L293
            cx.sg.set_draw_color(0.5, 0.5, 0.5);
            // L294
            let w = call_field(cx, me, Field::Width) - if y > fillH { 2.0 } else { 115.0 };
            Self::draw_image(cx, me, None, x - 9.0, y, w, 1.0);
            // L295
            let y = y + 8.0;
            // L296
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
            // L297
            y
        };

        // L300 -- loop through builds
        // L301 (ORDER: an array, module notes)
        let builds = Self::provider(cx, me).base().borrow().buildList.clone();
        for build in builds {
            // L302
            if LVal::truthy(build.buildName.as_ref()) {
                // L303
                let portraitHeight = currentHeight + 82.0;
                // L304
                let mut image: Option<ImageHandle> = None;
                // L305
                if LVal::truthy(build.ascendancy.as_ref()) || LVal::truthy(build.class.as_ref()) {
                    // L306
                    let a = LVal::or(build.ascendancy.as_ref(), build.class.as_ref());
                    image = Self::get_ascendancy_image_handle(cx, a.as_ref());
                    // L307
                    if let Some(image) = &image {
                        // L308
                        cx.sg.set_draw_color(0.5, 0.5, 0.5);
                        // L309
                        let left = x + call_field(cx, me, Field::Width) - 115.0;
                        Self::draw_image(cx, me, None, left, currentHeight - 1.0, 82.0, 82.0);
                        // L310
                        cx.sg.set_draw_color(1.0, 1.0, 1.0);
                        // L311
                        let left = x + call_field(cx, me, Field::Width) - 114.0;
                        Self::draw_image(cx, me, Some(image), left, currentHeight, 80.0, 80.0);
                    }
                }
                let _ = image;

                // L315 -- local lineCount = 0
                // L316
                if LVal::truthy(build.buildName.as_ref()) {
                    // L317
                    let max_w = call_field(cx, me, Field::Width) - 180.0;
                    let lines = Self::split_string_by_width(
                        cx,
                        build.buildName.as_ref(),
                        max_w,
                        Font::VarBold,
                    );
                    for line in lines {
                        // L318 -- lineCount = lineCount + 1
                        // L319
                        Self::draw_string(
                            cx,
                            me,
                            x,
                            currentHeight,
                            Align::Left,
                            18.0,
                            Font::VarBold,
                            &line,
                        );
                        // L320
                        currentHeight = currentHeight + 18.0;
                    }
                }

                // L324 -- decorator line
                // L325
                currentHeight = addSeparator(cx, currentHeight, portraitHeight);

                // L327 -- main skill
                // L328
                cx.sg.set_draw_color(1.0, 1.0, 1.0);
                // L329
                if LVal::truthy(build.mainSkill.as_ref())
                    && build.mainSkill != Some(LVal::Str(String::new()))
                {
                    // L330
                    let max_w = call_field(cx, me, Field::Width) - 125.0;
                    let lines =
                        Self::split_string_by_width(cx, build.mainSkill.as_ref(), max_w, font);
                    for line in lines {
                        // L331
                        Self::draw_string(cx, me, x, currentHeight, Align::Left, 16.0, font, &line);
                        // L332
                        currentHeight = currentHeight + 20.0;
                    }
                    // L334 -- decorator line
                    // L335
                    currentHeight = addSeparator(cx, currentHeight, portraitHeight);
                }

                // L338 -- author
                // L339
                if LVal::truthy(build.author.as_ref()) {
                    // L340
                    let text = LVal::fmt_s(build.author.as_ref());
                    Self::draw_string(cx, me, x, currentHeight, Align::Left, 14.0, font, &text);
                }

                // L343 -- version
                // L344
                if LVal::truthy(build.version.as_ref()) {
                    // L345
                    let authorWidth = if LVal::truthy(build.author.as_ref()) {
                        cx.sg
                            .draw_string_width(14.0, font, &LVal::fmt_s(build.author.as_ref()))
                    } else {
                        0.0
                    };
                    // L346
                    let text = LVal::fmt_s(build.version.as_ref());
                    Self::draw_string(
                        cx,
                        me,
                        x + authorWidth + 20.0,
                        currentHeight,
                        Align::Left,
                        14.0,
                        font,
                        &text,
                    );
                    // L347
                    cx.sg.set_draw_color(0.5, 0.5, 0.5);
                    // L348 if authorWidth then (a number: always true)
                    // L349
                    Self::draw_image(
                        cx,
                        me,
                        None,
                        x + authorWidth + 10.0,
                        currentHeight - 7.0,
                        1.0,
                        28.0,
                    );
                    // L350
                    let version = build.version.as_ref();
                    let version = match version {
                        Some(LVal::Str(s)) => s.clone(),
                        Some(LVal::Num(n)) => pob_lua::tostring_num(*n),
                        other => lua_error!(
                            "DrawStringWidth() argument 3: expected string, got {}",
                            LVal::type_name(other)
                        ),
                    };
                    let vw = cx.sg.draw_string_width(14.0, font, &version);
                    Self::draw_image(
                        cx,
                        me,
                        None,
                        x + authorWidth + vw + 30.0,
                        currentHeight - 7.0,
                        1.0,
                        28.0,
                    );
                }

                // L354
                currentHeight = currentHeight + 16.0;

                // L356 -- decorator line
                // L357
                currentHeight = addSeparator(cx, currentHeight, portraitHeight);

                // L359 -- stats
                // L360
                let mut dpsText = "DPS: 0".to_owned();
                // L361
                let mut lifeText = "Life: 0".to_owned();
                // L362
                let mut ehpText = "EHP: 0".to_owned();
                // L363
                if LVal::truthy(build.dps.as_ref()) {
                    // L364 -- SetDrawColor(1, 0, 0)
                    // L365
                    let dps = LVal::check_number(build.dps.as_ref(), 3, "format");
                    dpsText = format_num_sep_main(
                        &cx.main,
                        &pob_lua::lfmt!("^x%sDPS^7: %0.f", "FFC470", dps),
                    );
                }
                // L367
                if LVal::truthy(build.life.as_ref()) || LVal::truthy(build.es.as_ref()) {
                    // L368 -- SetDrawColor(0, 1, 0)
                    // L369
                    let zero = LVal::Num(0.0);
                    let life = LVal::or(build.life.as_ref(), Some(&zero)).unwrap_or(LVal::Num(0.0));
                    let es = LVal::or(build.es.as_ref(), Some(&zero)).unwrap_or(LVal::Num(0.0));
                    let label = if LVal::lt(&es, &life) {
                        "^xFFAAAALife"
                    } else {
                        "^x667BC6ES"
                    };
                    let max = m_max(
                        LVal::check_number(Some(&life), 1, "max"),
                        LVal::check_number(Some(&es), 2, "max"),
                    );
                    lifeText =
                        format_num_sep_main(&cx.main, &pob_lua::lfmt!("%s^7: %0.f", label, max));
                }
                // L371
                if LVal::truthy(build.ehp.as_ref()) {
                    // L372 -- SetDrawColor(0, 0, 1)
                    // L373
                    let ehp = LVal::check_number(build.ehp.as_ref(), 3, "format");
                    ehpText = format_num_sep_main(
                        &cx.main,
                        &pob_lua::lfmt!("^x%sEHP^7: %0.f", "3AA6B9", ehp),
                    );
                }

                // L376 -- prevent overlapping on smaller screens.
                // L377
                let dpsWidth = cx.sg.draw_string_width(14.0, font, &dpsText);
                // L378
                let lifeWidth = cx.sg.draw_string_width(14.0, font, &lifeText);
                // L379
                let ehpWidth = cx.sg.draw_string_width(14.0, font, &ehpText);
                // L380
                if dpsWidth + lifeWidth + ehpWidth < call_field(cx, me, Field::Width) - 30.0 {
                    // L381
                    Self::draw_string(cx, me, x, currentHeight, Align::Left, 14.0, font, &dpsText);
                    // L382
                    cx.sg.set_draw_color(0.5, 0.5, 0.5);
                    // L383
                    Self::draw_image(
                        cx,
                        me,
                        None,
                        x + dpsWidth + 10.0,
                        currentHeight - 7.0,
                        1.0,
                        28.0,
                    );
                    // L384
                    cx.sg.set_draw_color(1.0, 1.0, 1.0);
                    // L385
                    Self::draw_string(
                        cx,
                        me,
                        x + dpsWidth + 20.0,
                        currentHeight,
                        Align::Left,
                        14.0,
                        font,
                        &lifeText,
                    );
                    // L386
                    cx.sg.set_draw_color(0.5, 0.5, 0.5);
                    // L387
                    Self::draw_image(
                        cx,
                        me,
                        None,
                        x + dpsWidth + lifeWidth + 30.0,
                        currentHeight - 7.0,
                        1.0,
                        28.0,
                    );
                    // L388
                    Self::draw_image(
                        cx,
                        me,
                        None,
                        x + dpsWidth + lifeWidth + ehpWidth + 50.0,
                        currentHeight - 7.0,
                        1.0,
                        28.0,
                    );
                    // L389
                    cx.sg.set_draw_color(1.0, 1.0, 1.0);
                    // L390
                    Self::draw_string(
                        cx,
                        me,
                        x + dpsWidth + lifeWidth + 40.0,
                        currentHeight,
                        Align::Left,
                        14.0,
                        font,
                        &ehpText,
                    );
                    // L391
                    currentHeight = currentHeight + 16.0;
                    // L392 -- decorator line
                    // L393
                    currentHeight = addSeparator(cx, currentHeight, portraitHeight);
                    // L394
                    currentHeight = currentHeight - 5.0;
                }

                // L397
                if let Some(metadata) = &build.metadata {
                    // L398
                    currentHeight = currentHeight + 4.0;
                    // L399 (ORDER: an array)
                    for (key, value) in metadata {
                        // L400
                        cx.sg.set_draw_color(1.0, 1.0, 1.0);
                        // L401
                        let text =
                            format!("{key}: {}", LVal::concat(value.as_ref(), "field 'value'"));
                        Self::draw_string(cx, me, x, currentHeight, Align::Left, 14.0, font, &text);
                        // L402
                        currentHeight = currentHeight + 16.0;
                        // L403
                        currentHeight = addSeparator(cx, currentHeight, portraitHeight);
                    }
                    // L405
                    currentHeight = currentHeight - 4.0;
                }

                // L408 -- draw buttons
                // L409
                currentHeight = currentHeight + 4.0;
                // L410
                let offset = cx.w::<ScrollBarControl>(Self::scroll_bar_v(cx, me)).offset;
                let relativeHeight = currentHeight + 10.0 - offset;
                // L411
                if relativeHeight > y
                    && relativeHeight < call_field(cx, me, Field::Height) + y - 10.0
                {
                    // L412
                    if LVal::truthy(build.buildLink.as_ref()) {
                        // L413-415
                        let offset = cx.w::<ScrollBarControl>(Self::scroll_bar_v(cx, me)).offset;
                        let b = build.clone();
                        let importButton = ButtonControl::new(
                            Anchor::NONE,
                            rect(x, currentHeight - offset, 45, 20),
                            "Import",
                            move |cx| {
                                // L414
                                Self::import_build(cx, me, &b);
                            },
                        );
                        // L416
                        Self::controls_insert(cx, me, importButton);
                    }

                    // L419
                    if let Some(preview_link) = build.previewLink.clone() {
                        // L420-422
                        let offset = cx.w::<ScrollBarControl>(Self::scroll_bar_v(cx, me)).offset;
                        let previewButton = ButtonControl::new(
                            Anchor::NONE,
                            rect(x + 50.0, currentHeight - offset, 60, 20),
                            "Preview",
                            move |cx| {
                                // L421
                                cx.sys.platform.open_url(&preview_link);
                            },
                        );
                        // L423
                        Self::controls_insert(cx, me, previewButton);
                    }
                    // L425
                    currentHeight = currentHeight + 4.0;
                }

                // L428 -- bottom border
                // L429
                cx.sg.set_draw_color(1.0, 1.0, 1.0);
                // L430
                currentHeight = currentHeight + 24.0;
                // L431
                let w = call_field(cx, me, Field::Width) - 1.0;
                Self::draw_image(cx, me, None, x - 9.0, currentHeight, w, 3.0);
                // L432
                currentHeight = currentHeight + 16.0;
            }
        }

        // L436
        let h = call_field(cx, me, Field::Height);
        let sb = Self::scroll_bar_v(cx, me);
        ScrollBarControl::set_content_dimension(cx, sb, currentHeight - y + 17.0, h);
        // L437
        cx.w_mut::<Self>(me).content_height = currentHeight;
        // L438 -- end
        // L439
        let sel = (no_tooltip && !cx.w::<Self>(me).force_tooltip).then_some(me);
        cx.draw_controls(me, vp, sel);
    }
}

/// `callback(xmlText, urlText)` of [`ImportBuild`].
pub type ImportBuildCallback = Rc<dyn Fn(&mut Cx, Option<String>, Option<String>)>;

/// The global `ImportBuild(importLink, callback)`: a build-site link is downloaded
/// (`buildSites.DownloadBuild`, U43), anything else is decoded as a build code.
/// lua: Common.lua:1081-1105 ImportBuild(importLink, callback)
/// @pob Modules/Common.lua:1081-1105 h=93885587
#[allow(non_snake_case)] // the Lua global's name
pub fn ImportBuild(cx: &mut Cx, importLink: &str, callback: ImportBuildCallback) {
    // L1082 -- Quick Trim
    let mut urlText = gsub(&gsub(importLink, "^[%s?]+", ""), "[%s?]+$", "");
    // L1083
    if lmatch(&urlText, "youtube%.com/redirect%?").is_some()
        || lmatch(&urlText, "google%.com/url%?").is_some()
    {
        // L1084
        let nested_url = gsub(&urlText, ".*[?&]q=([^&]+).*", "%1");
        // L1085
        urlText = UrlDecode(Some(&nested_url)).unwrap_or_default();
    }
    // L1087
    let mut websiteInfo = None;
    // L1088
    for site in &WEBSITE_LIST {
        // L1089
        if lmatch(&urlText, site.match_url).is_some() {
            // L1090
            websiteInfo = Some(site);
        }
    }

    // L1094 -- its an import link
    // L1095
    if let Some(websiteInfo) = websiteInfo {
        // L1096
        let url = urlText.clone();
        download_build(
            cx,
            &urlText,
            Some(websiteInfo),
            Rc::new(
                move |cx: &mut Cx, isSuccess: bool, data: String, _site_code_url| {
                    // L1097
                    if isSuccess {
                        // L1098
                        callback(cx, decode_build_code(&data), Some(url.clone()));
                    }
                },
            ),
        );
    } else {
        // L1102 -- try to decode input buffer
        // L1103
        callback(cx, decode_build_code(importLink), None);
    }
}

#[cfg(test)]
mod tests {
    use super::super::provider::{ExtBuildListProvider, ExtBuildListProviderClass};
    use super::*;
    use std::cell::RefCell;

    /// A provider without a fetch.
    struct Dummy(RefCell<ExtBuildListProvider>);
    impl ExtBuildListProviderClass for Dummy {
        fn base(&self) -> &RefCell<ExtBuildListProvider> {
            &self.0
        }
    }
    fn dummy(titles: &[&str]) -> ProviderRef {
        Rc::new(Dummy(RefCell::new(ExtBuildListProvider::new(Some(
            titles.iter().map(|t| (*t).to_owned()).collect(),
        )))))
    }

    fn int_keys(cx: &Cx, list: CtrlId) -> Vec<(i64, String)> {
        (1..=20)
            .filter_map(|i| {
                let c = cx.child(list, CtrlKey::Idx(i))?;
                Some((i, cx.w::<ButtonControl>(c).label.raw()))
            })
            .collect()
    }

    /// Init with another provider (the dropdown's selFunc) while the dropdown is the parent's
    /// selection: the old controls leave `self.controls`, the focused dropdown stays alive until
    /// the parent deselects it (DIVERGENCE(gc)), then the next Init/Draw frees it.
    #[test]
    fn init_again_keeps_a_focused_control_until_it_loses_focus() {
        let mut cx = Cx::headless();
        let cx = &mut cx;
        let host = cx
            .add_free(
                None,
                PlainControl::new(Anchor::NONE, rect(0, 0, 800, 600)).host(),
            )
            .id();
        let providers = vec![
            ProviderEntry {
                name: "A".into(),
                r#impl: dummy(&["x", "y"]),
            },
            ProviderEntry {
                name: "B".into(),
                r#impl: dummy(&["z"]),
            },
        ];
        let list = cx
            .add(
                host,
                "list",
                ExtBuildListControl::new(Anchor::NONE, rect(0, 30, 400, 300), providers),
            )
            .id();
        cx.base_mut(list).width = Prop::f(|_, _| 400.0);
        ExtBuildListControl::init(cx, list, Some("A"));
        assert_eq!(int_keys(cx, list), vec![(1, "x".into()), (2, "y".into())]);
        assert!(cx.child(list, "scrollBarV").is_some());
        assert!(cx.child(list, "all").is_none(), "GetPageUrl is nil");
        let tab_x = cx.w::<ExtBuildListControl>(list).tabs[0];
        assert!(cx.prop::<ButtonControl, _>(tab_x, |w| &w.locked));

        let sort = cx.child(list, "sort").unwrap();
        cx.select_control(host, Some(sort));
        DropDownControl::set_sel(cx, sort, 2, false);
        // Init("B") ran: new controls, the provider switched, the old dropdown left the table
        assert_eq!(int_keys(cx, list), vec![(1, "z".into())]);
        let p = cx
            .w::<ExtBuildListControl>(list)
            .active_list_provider
            .clone()
            .unwrap();
        assert_eq!(p.get_active_list().as_deref(), Some("z"));
        assert_ne!(cx.child(list, "sort"), Some(sort));
        cx.end_frame_gc();
        assert!(cx.ui.is_alive(sort), "still the host's selControl");
        assert!(!cx.ui.is_alive(tab_x), "an unfocused control is freed");

        cx.select_control(host, None);
        ExtBuildListControl::init(cx, list, Some("B"));
        cx.end_frame_gc();
        assert!(!cx.ui.is_alive(sort));

        // an unknown provider: only the dropdown is left
        ExtBuildListControl::init(cx, list, Some("C"));
        assert!(int_keys(cx, list).is_empty());
        assert!(cx.child(list, "sort").is_some());
        assert!(cx.child(list, "scrollBarV").is_none());
        assert!(cx
            .w::<ExtBuildListControl>(list)
            .active_list_provider
            .is_none());
    }

    #[test]
    fn controls_border_after_holes() {
        // the shape Draw produces: tabs at 1-2, then buttons appended, then removed again
        let mut t: LjTab<CKey, ()> = LjTab::new();
        t.set(CKey::Str("sort"), Some(()));
        for i in 1..=2 {
            let n = t.len() as i64 + 1;
            assert_eq!(n, i);
            t.set(CKey::Int(n), Some(()));
        }
        t.set(CKey::Str("scrollBarV"), Some(()));
        t.set(CKey::Str("all"), Some(()));
        for _ in 0..6 {
            let n = t.len() as i64 + 1;
            t.set(CKey::Int(n), Some(()));
        }
        assert_eq!(t.len(), 8);
        for i in 3..=8 {
            t.set(CKey::Int(i), None);
        }
        assert_eq!(t.len(), 2);
    }
}
