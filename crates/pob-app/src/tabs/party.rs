//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/PartyTab.lua:16-515, 670-717 (ctor,
//! Draw; Load/Save and ParseBuffs/exports are calc:BM-B). Owner: U45.
//!
//! @pob-file Classes/PartyTab.lua @16de4b82d
//!
//! The tab is a free node owned by the build node (`self.partyTab`), created by [`ctor_hook`]
//! and found again through `BuildState::tabs.party`, as the Notes tab (CCR-0106 §2). The model
//! half is calc:BM-B's `model::PartyTab` (`build.partyTab`): the actor, `enemyModList`,
//! `enableExportBuffs`, `lastContent`, Load/Save, ParseBuffs and the exports. CCR-0019 put the
//! state of the controls Load, Save and ParseBuffs touch into the model as Lua-like tables
//! (`partyTab.extra.controls`: `{ buf }` for the seven edits, `{ label }` for the six `simple*`
//! labels, `{ list, selIndex }`, `{ state }`). This file keeps the controls and ties them to
//! those tables (CCR-0114 §1):
//! * the `simple*` labels' `label` is a property that reads the model's table, so ParseBuffs
//!   (model) and clearInputText (here) write the one field the Lua has;
//! * the seven edits keep their own `buf` (EditControl state); every `SetText` here also writes
//!   the model's `buf`, and the end of [`PartyTab::draw`] copies the edits' `buf`, the two
//!   check boxes' `state` and the destination's `selIndex` into the model (the only other place
//!   they change is input inside Draw), so the model's Save writes what the controls hold;
//! * Load (model) runs inside `Init_model`, before the UI constructors (CCR-0080 §1), so
//!   [`ctor_hook`] replays Load's UI statements after the constructor: `SetText` of each edit,
//!   `SelByValue`, the two `state`s (PartyTab.lua:522-571).
//!
//! Lua semantics:
//! * `buf:gsub(..)` in an expression keeps the first result (`luastr::gsub`); `s:match(p)` is
//!   `luastr::lmatch`.
//! * `common.base64.decode(x:gsub("_","/"))` passes gsub's count as an ignored second argument.
//! * `Inflate` is SimpleGraphic's (`l_Inflate`: zlib stream, nil unless it reaches the stream
//!   end), [`inflate`] here (CCR-0114 §3).
//! * `launch:ShowErrMsg("^1Error loading '%s': %s", fileName, errMsg)` (L198, L201): `fileName`
//!   is an undeclared global in the ctor (LUA-GLOBAL-NIL), formatted as `nil`.
//! * `node[1]` of an `<ExportedBuffs>` node that holds an element instead of text is a table
//!   in Lua: when the edit's `buf` is non-empty `buf.."\n"..node[1]` raises, and so does the
//!   port; otherwise `SetText(node[1])` stores `tostring(table)`, an address string
//!   (`table: 0x...`; the port formats the node's address, no two runs agree on it).
//! * `for id, event in ipairs(inputEvents)` stops at the first hole (`next_ipairs`).
//!
//! DIVERGENCE:
//! * DIVERGENCE(bytes), as in `tabs::import::core`: `Inflate`'s result is a Lua byte string;
//!   the port keeps the code's XML as UTF-8 (`from_utf8_lossy`), so a code whose XML is not
//!   UTF-8 gets U+FFFD for each invalid sequence both in the dev-mode `Copy` and in what
//!   `ParseXML` reads (PoB's own exports are UTF-8).
//! * "Disable Party Effects" / "Rebuild All" (L353-360, L363-377): `wipeTable(self.actor)` and
//!   `self.actor = { … }` replace the actor table; the model's actor is one `Rc` (calc:BM-B), so
//!   the port resets it in place (`extra` wiped and refilled, a fresh `modDB` whose `actor` is
//!   the actor, a fresh `output`). An env built earlier sees the new contents where PoB's sees
//!   the wiped old table; nothing reads a stale env's party members between the click and the
//!   next calc. `wipeTable(self.enemyModList)` wipes a ModList nobody else holds (CalcSetup
//!   copies its mods), so the port only replaces it.
//! * `buildSites` (Modules/BuildSiteTools.lua, U43) and ImportTab.lua's global `UrlDecode`
//!   (U44) are reached through [`SiteHooks`] (CCR-0114 §2) until their owners provide them: the
//!   defaults know no site (a URL reads "Invalid input") and raise a Lua error naming the owner.
//!
//! ORDER: no `pairs` loops. The controls are added in the Lua's insertion order (D7).
//!
//! Props with side effects (ui doc §7.3): `notesDesc.width` (L69-76) writes
//! `notesDesc.lastWidth` and `notesDesc.label`: [`PartyTab::notes_desc_last_width`] and
//! [`PartyTab::notes_desc_label`] (`Cell`/`RefCell`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pob_lua::{n, FmtArg};
use pob_ui_core::controls::{
    ButtonControl, CheckBoxControl, DropDownControl, DropItem, EditArgs, EditControl, LabelControl,
};
use pob_ui_core::luastr::{gsub, lmatch};
use pob_ui_core::prelude::*;

use crate::app::CxApp;
use crate::main_mode::background::draw_background;
use crate::model::party_tab::BuffList;
use crate::model::xml::{XmlChild, XmlNode};
use crate::model::{self, Table, Val};

// @pob Classes/PartyTab.lua:1-15 h=9a815d38
// (the `local` aliases; `newClass("PartyTab", "ControlHost", "Control")`: the `Widget` impl
// below with `.host()` in the constructor)

/// `partyDestinations` (L39).
pub const PARTY_DESTINATIONS: [&str; 8] = [
    "All",
    "Party Member Stats",
    "Aura",
    "Curse",
    "Warcry Skills",
    "Link Skills",
    "EnemyConditions",
    "EnemyMods",
];

// `theme` (L41-61)
/// `theme.stringHeight`
const STRING_HEIGHT: f64 = 16.0;
/// `theme.buttonHeight`
const BUTTON_HEIGHT: f64 = 20.0;
/// `theme.widthThreshold1`
const WIDTH_THRESHOLD1: f64 = 1350.0;
/// `theme.bufferHeightSmall`
const BUFFER_HEIGHT_SMALL: f64 = 106.0;
/// `theme.bufferHeightRight` (4 elements)
const BUFFER_HEIGHT_RIGHT: f64 = 434.0;

/// `theme.lineCounter(label)` (L44-52): 16 per newline.
fn line_counter(label: &str) -> f64 {
    let mut line_count = 0.0;
    for c in label.bytes() {
        if c == b'\n' {
            line_count += 1.0;
        }
    }
    line_count * 16.0
}

/// `theme.bufferHeightLeft()` (L55-58, 2 elements).
fn buffer_height_left(cx: &Cx, me: CtrlId) -> f64 {
    let (w, h) = tab_size(cx, me);
    let header = c(cx, me, "importCodeHeader");
    let auras_label = c(cx, me, "editAurasLabel");
    h - 378.0
        - if w > 1350.0 { 0.0 } else { 24.0 }
        - cx.base(header).y.get(cx, header)
        - cx.base(auras_label).y.get(cx, auras_label)
}

/// `notesDesc` (L63-66; the long string keeps its tabs and newlines).
const NOTES_DESC: &str = "^7To import a build it must be exported with \"Export support\" enabled in the import/export tab\n\
\tAuras with the highest effect will take priority, your curses will take priority over a support's\n\
\t\n\
\tAll of these effects can be found in the Calcs tab";

/// One site of `buildSites.websiteList` that PartyTab reads (`label`, `matchURL`).
#[derive(Clone, Debug)]
pub struct BuildSite {
    pub label: String,
    pub match_url: String,
}

/// `buildSites.DownloadBuild`'s callback `(isSuccess, data)`.
pub type DownloadCallback = Box<dyn FnOnce(&mut Cx, bool, &str)>;

/// CCR-0114 §2: what PartyTab calls of `buildSites` (Modules/BuildSiteTools.lua, U43) and of
/// ImportTab.lua's global `UrlDecode` (U44). The owners install theirs with [`set_site_hooks`].
#[derive(Clone)]
pub struct SiteHooks {
    /// `buildSites.websiteList`
    pub website_list: Rc<dyn Fn(&Cx) -> Vec<BuildSite>>,
    /// `buildSites.DownloadBuild(link, buildSites.websiteList[site], callback)` (`site` is the
    /// 1-based index into `website_list`)
    pub download_build: Rc<dyn Fn(&mut Cx, &str, usize, DownloadCallback)>,
    /// `UrlDecode(url)` (ImportTab.lua:1917-1924)
    pub url_decode: Rc<dyn Fn(&str) -> String>,
}

fn not_ported(what: &str, owner: &str) -> ! {
    lua_error!("{what} is not ported yet ({owner})")
}

/// The placeholders until U43/U44 land: no site, and the calls raise a Lua error naming the
/// owner.
pub fn default_site_hooks() -> SiteHooks {
    SiteHooks {
        website_list: Rc::new(|_| Vec::new()),
        download_build: Rc::new(|_, _, _, _| not_ported("buildSites.DownloadBuild", "U43")),
        url_decode: Rc::new(|_| not_ported("UrlDecode", "U44")),
    }
}

thread_local! {
    static SITE_HOOKS: RefCell<Option<SiteHooks>> = const { RefCell::new(None) };
}

/// The installed [`SiteHooks`] (a clone; entries are `Rc`s).
pub fn site_hooks() -> SiteHooks {
    SITE_HOOKS.with(|h| {
        h.borrow_mut()
            .get_or_insert_with(default_site_hooks)
            .clone()
    })
}

/// Replace entries of the installed [`SiteHooks`].
pub fn set_site_hooks(f: impl FnOnce(&mut SiteHooks)) {
    SITE_HOOKS.with(|h| f(h.borrow_mut().get_or_insert_with(default_site_hooks)));
}

/// SimpleGraphic's `Inflate(string)` (`l_Inflate`, ui_api.cpp): a zlib stream; the inflated
/// bytes when `inflate` reaches `Z_STREAM_END`, nil otherwise (CCR-0114 §3).
pub fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    use flate2::{Decompress, FlushDecompress, Status};
    let mut z = Decompress::new(true);
    let mut out: Vec<u8> = Vec::with_capacity(data.len().saturating_mul(4).max(64));
    loop {
        if out.len() == out.capacity() {
            // the output buffer filled: embiggen it
            out.reserve(out.capacity());
        }
        let (in0, out0) = (z.total_in(), z.total_out());
        let input = &data[usize::try_from(z.total_in()).ok()?..];
        match z.decompress_vec(input, &mut out, FlushDecompress::None) {
            Ok(Status::StreamEnd) => return Some(out),
            Ok(Status::Ok) if z.total_in() != in0 || z.total_out() != out0 => {}
            // Z_BUF_ERROR (no progress: the input ended before the stream did) or Z_DATA_ERROR
            _ => return None,
        }
    }
}

pub struct PartyTab {
    /// `self.build`
    pub build: CtrlId,
    /// `self.importCodeSite` (1-based index into `buildSites.websiteList`)
    pub import_code_site: Option<usize>,
    /// `self.importCodeDetail` (nil until the first change of the code edit)
    pub import_code_detail: Option<String>,
    /// `self.importCodeXML`
    pub import_code_xml: Option<String>,
    /// `self.importCodeValid` (nil reads as false)
    pub import_code_valid: bool,
    /// `self.importCodeFetching` (nil reads as false)
    pub import_code_fetching: bool,
    /// `self.modFlag` (nil until the first Draw: false)
    pub mod_flag: bool,
    /// `self.controls.notesDesc.lastWidth` (written by the `width` property, L72)
    pub notes_desc_last_width: Cell<Option<f64>>,
    /// `self.controls.notesDesc.label` (written by the `width` property, L73)
    pub notes_desc_label: RefCell<String>,
}

/// drawn by BuildMode
impl Widget for PartyTab {
    const NAME: &'static str = "PartyTab";
    const CAPS: Caps = Caps::empty();
}

// ---- helpers (Rust-only) ----------------------------------------------------------------------

/// `self.controls[key]`
fn c(cx: &Cx, me: CtrlId, key: &'static str) -> CtrlId {
    cx.child(me, key)
        .unwrap_or_else(|| lua_error!("attempt to index field '{key}' (a nil value)"))
}

/// The tab's raw `self.width`, `self.height` (numbers once Draw ran).
fn tab_size(cx: &Cx, me: CtrlId) -> (f64, f64) {
    let b = cx.base(me);
    (b.width.raw(), b.height.raw())
}

/// `self.build` (the model half).
fn model_of(cx: &Cx) -> model::BuildRef {
    cx.build().model.clone()
}

/// `self.controls[key]` of the model half (CCR-0019: `partyTab.extra.controls`).
fn mctl(cx: &Cx, key: &str) -> Table {
    cx.build().model.partyTab.control(key)
}

/// A label's text, as `DrawString` / `#label` read it.
fn val_text(v: &Val, what: &str) -> String {
    match v {
        Val::Str(s) => s.to_string(),
        Val::Num(x) => pob_lua::tostring_num(*x),
        other => lua_error!(
            "attempt to get length of field '{what}' (a {} value)",
            other.lua_type()
        ),
    }
}

/// `self.controls[key].label` of a `simple*` label (the model's table, see the module notes).
fn simple_label(cx: &Cx, key: &str) -> String {
    val_text(&mctl(cx, key).get(n!("label")), "label")
}

/// Lua `v == b` for a boolean `b`.
fn val_is_bool(v: &Val, b: bool) -> bool {
    matches!(v, Val::Bool(x) if *x == b)
}

/// `self.controls[key].label = s`
fn set_simple_label(cx: &Cx, key: &str, s: &str) {
    mctl(cx, key).set(n!("label"), s);
}

/// `self.controls[key].buf`
fn buf(cx: &Cx, me: CtrlId, key: &'static str) -> String {
    cx.w::<EditControl>(c(cx, me, key)).buf.clone()
}

/// `self.controls[key]:SetText(text)` (no notify), and the model's copy of `buf`.
fn set_text(cx: &mut Cx, me: CtrlId, key: &'static str, text: &str) {
    let id = c(cx, me, key);
    EditControl::set_text(cx, id, text, false);
    let b = cx.w::<EditControl>(id).buf.clone();
    mctl(cx, key).set(n!("buf"), b);
}

/// `self.controls[key].hasFocus`
fn has_focus(cx: &Cx, me: CtrlId, key: &'static str) -> bool {
    cx.base(c(cx, me, key)).has_focus == Some(true)
}

/// `self.controls.ShowAdvanceTools.state`
fn show_advanced(cx: &Cx, me: CtrlId) -> bool {
    cx.w::<CheckBoxControl>(c(cx, me, "ShowAdvanceTools")).state
}

/// `self.controls[key].height()` (a function-valued field called directly)
fn height_of(cx: &Cx, me: CtrlId, key: &'static str) -> f64 {
    let id = c(cx, me, key);
    cx.base(id).height.get(cx, id)
}

/// `partyDestinations[self.controls.importCodeDestination.selIndex] == d`
fn dest_is(cx: &Cx, me: CtrlId, d: &str) -> bool {
    let sel = cx
        .w::<DropDownControl>(c(cx, me, "importCodeDestination"))
        .sel_index;
    sel >= 1 && PARTY_DESTINATIONS.get(sel - 1) == Some(&d)
}

/// The edits whose `buf` the model's Load/Save use (CCR-0019), and their model keys.
const MODEL_EDITS: [&str; 7] = [
    "editPartyMemberStats",
    "editAuras",
    "editCurses",
    "editWarcries",
    "editLinks",
    "enemyCond",
    "enemyMods",
];

/// `wipeTable(self.actor); self.actor = { Aura = {}, Curse = {}, Warcry = { }, Link = {}, modDB =
/// new("ModDB"):ModDB(), output = { } }; self.actor.modDB.actor = self.actor` (L354-357,
/// L364-367), in place (see the module notes' DIVERGENCE).
fn reset_actor(m: &model::BuildRef) {
    let actor = &m.partyTab.actor;
    actor.extra.wipe();
    for k in [n!("Aura"), n!("Curse"), n!("Warcry"), n!("Link")] {
        actor.extra.set(k, Table::new());
    }
    actor.modDB.set(
        pob_calc::classes::ModStore::ModDB(None), /* CCR-0114 §4 */
    );
    actor.output.set(Table::new());
    actor.modDB.get().actor.set(Rc::downgrade(actor));
}

/// `wipeTable(self.enemyModList); self.enemyModList = new("ModList"):ModList()`
fn reset_enemy_mod_list(m: &model::BuildRef) {
    m.partyTab.enemyModList.set(
        pob_calc::classes::ModStore::ModList(None), /* CCR-0114 §4 */
    );
}

/// `wipeTable(self.actor[k]); self.actor[k] = { }`
fn reset_actor_list(m: &model::BuildRef, k: model::Name) {
    m.partyTab.actorList(k).wipe();
    m.partyTab.actor.extra.set(k, Table::new());
}

/// `self:ParseBuffs(self.actor[k], buf, buffType, self.controls[label])`
fn parse_actor_list(cx: &Cx, k: model::Name, buf: &str, buff_type: model::Name, label: &str) {
    let m = model_of(cx);
    let list = m.partyTab.actorList(k);
    let label = m.partyTab.control(label);
    m.partyTab
        .ParseBuffs(&m, BuffList::Table(&list), buf, buff_type, Some(&label));
}

/// `self:ParseBuffs(self.actor["modDB"], buf, "PartyMemberStats", self.actor["output"])`
fn parse_party_member_stats(cx: &Cx, buf: &str) {
    let m = model_of(cx);
    let mod_db = m.partyTab.actor.modDB.get();
    let output = m.partyTab.actor.output.get();
    m.partyTab.ParseBuffs(
        &m,
        BuffList::Store(&mod_db),
        buf,
        n!("PartyMemberStats"),
        Some(&output),
    );
}

/// `self:ParseBuffs(self.enemyModList, buf, buffType[, self.controls.simpleEnemyMods])`
fn parse_enemy(cx: &Cx, buf: &str, buff_type: model::Name, label: Option<&str>) {
    let m = model_of(cx);
    let list = m.partyTab.enemyModList.get();
    let label = label.map(|l| m.partyTab.control(l));
    m.partyTab
        .ParseBuffs(&m, BuffList::Store(&list), buf, buff_type, label.as_ref());
}

/// `node[1]` of an `<ExportedBuffs>` node in `finishImport` (L213-270): nil, its text, or the
/// element child (a table in Lua).
enum Node1<'a> {
    Nil,
    Str(String),
    Table(&'a XmlNode),
}

impl Node1<'_> {
    fn of(node: &XmlNode) -> Node1<'_> {
        match node.children.first() {
            None => Node1::Nil,
            Some(XmlChild::Text(s)) => Node1::Str(s.to_string()),
            Some(XmlChild::Node(n)) => Node1::Table(n),
        }
    }

    /// `buf.."\n"..(node[1] or "")` (`..` raises on a table)
    fn concat_after(self, cur: &str) -> Self {
        match self {
            Node1::Nil => Node1::Str(format!("{cur}\n")),
            Node1::Str(s) => Node1::Str(format!("{cur}\n{s}")),
            Node1::Table(_) => lua_error!("attempt to concatenate a table value"),
        }
    }

    /// `node[1]` in `node[1] == "..."` (a table never equals a string)
    fn as_str(&self) -> Option<&str> {
        match self {
            Node1::Str(s) => Some(s),
            Node1::Nil | Node1::Table(_) => None,
        }
    }

    /// `SetText(node[1] or "")`: `tostring(text)`; a table gives its address string (Lua's
    /// differs run to run; the port formats the node's address)
    fn set_text_arg(&self) -> String {
        match self {
            Node1::Nil => String::new(),
            Node1::Str(s) => s.clone(),
            Node1::Table(n) => format!("table: {:p}", *n),
        }
    }
}

impl PartyTab {
    /// lua: PartyTab.lua:16-515 (L22-36: model part, calc:BM-B `PartyTab::ctor_model`, run by
    /// `Init_model`)
    /// @pob Classes/PartyTab.lua:16-21 h=a60bca04
    /// @pob Classes/PartyTab.lua:37-81 h=a8e5853c
    /// @pob Classes/PartyTab.lua:287-515 h=25942d5e
    pub fn new(build: CtrlId) -> New<PartyTab> {
        // L18-19 self:ControlHost(); self:Control()
        New::new(
            Base::new(Anchor::NONE, rect(0, 0, 0, 0)),
            PartyTab {
                // L21
                build,
                import_code_site: None,
                import_code_detail: None,
                import_code_xml: None,
                import_code_valid: false,
                import_code_fetching: false,
                mod_flag: false,
                notes_desc_last_width: Cell::new(None),
                notes_desc_label: RefCell::new(NOTES_DESC.to_owned()),
            },
        )
        .host()
        .init(|cx, me| {
            let me = me.id();
            // L22-36 (model part: calc:BM-B)
            // L39 partyDestinations, L41-61 theme: module constants and functions
            let label = |text: &str| -> Prop<String> { Prop::Val(text.to_owned()) };
            // L68
            let notes_desc = cx
                .add(
                    me,
                    "notesDesc",
                    LabelControl::new(
                        Anchor::new(TopLeft, me, TopLeft),
                        rect(8, 8, 150, STRING_HEIGHT),
                        "",
                    ),
                )
                .id();
            cx.w_mut::<LabelControl>(notes_desc).label = Prop::f(move |cx, _| {
                cx.w::<PartyTab>(me).notes_desc_label.borrow().clone()
            });
            // L69-76
            cx.base_mut(notes_desc).width = Prop::f(move |cx, _| {
                // L70
                let width = cx.base(me).width.raw() / 2.0 - 16.0;
                let t = cx.w::<PartyTab>(me);
                // L71
                if Some(width) != t.notes_desc_last_width.get() {
                    // L72
                    t.notes_desc_last_width.set(Some(width));
                    // L73
                    let wrapped =
                        pob_ui_core::wrap::wrap_string(cx, NOTES_DESC, STRING_HEIGHT, width - 50.0);
                    *t.notes_desc_label.borrow_mut() = wrapped.join("\n");
                }
                // L75
                width
            });
            // L77
            let header = cx
                .add(
                    me,
                    "importCodeHeader",
                    LabelControl::new(
                        Anchor::new(TopLeft, notes_desc, BottomLeft),
                        rect(0, 32, 0, STRING_HEIGHT),
                        "^7Enter a build code/URL below:",
                    ),
                )
                .id();
            // L78-80
            cx.base_mut(header).y = Prop::f(move |cx, _| {
                line_counter(&cx.w::<PartyTab>(me).notes_desc_label.borrow()) + 4.0
            });

            // L82-118 clearInputText, L120-160 importCodeHandle, L162-285 finishImport:
            // PartyTab::{clear_input_text, import_code_handle, finish_import}

            // L287
            let code_in = cx
                .add(
                    me,
                    "importCodeIn",
                    EditControl::new(
                        Anchor::new(TopLeft, header, BottomLeft),
                        rect(0, 4, 328, BUTTON_HEIGHT),
                        EditArgs {
                            init: Some(String::new()),
                            change_func: Some(Rc::new(move |cx: &mut Cx, buf: &str| {
                                PartyTab::import_code_handle(cx, me, buf);
                            })),
                            ..Default::default()
                        },
                    ),
                )
                .id();
            // L288-290
            cx.base_mut(code_in).width = Prop::f(move |cx, _| {
                let w = cx.base(me).width.raw();
                if w > 880.0 {
                    328.0
                } else {
                    w / 2.0 - 100.0
                }
            });
            // L291-295
            cx.w_mut::<EditControl>(code_in).enter_func = Some(Rc::new(move |cx: &mut Cx, _| {
                // L292
                if cx.w::<PartyTab>(me).import_code_valid {
                    // L293
                    PartyTab::click_import_go(cx, me);
                }
            }));
            // L296
            let state = cx
                .add(
                    me,
                    "importCodeState",
                    LabelControl::new(
                        Anchor::new(Left, code_in, Right),
                        rect(8, 0, 0, STRING_HEIGHT),
                        "",
                    ),
                )
                .id();
            // L297-299
            cx.w_mut::<LabelControl>(state).label = Prop::f(move |cx, _| {
                cx.w::<PartyTab>(me)
                    .import_code_detail
                    .clone()
                    .unwrap_or_default()
            });
            // L300
            let dest = cx
                .add(
                    me,
                    "importCodeDestination",
                    DropDownControl::new(
                        Anchor::new(TopLeft, code_in, BottomLeft),
                        rect(0, 4, 160, BUTTON_HEIGHT),
                        PARTY_DESTINATIONS.iter().map(|d| DropItem::str(*d)).collect(),
                        None,
                        None,
                        false,
                    ),
                )
                .id();
            // L301
            cx.w_mut::<DropDownControl>(dest).tt.tooltip_text = Some(Prop::Val(Some(
                "Destination for Import/clear\nCurrently Links Skills do not export".to_owned(),
            )));
            // L302-321
            let go = cx
                .add(
                    me,
                    "importCodeGo",
                    ButtonControl::new(
                        Anchor::new(Left, dest, Right),
                        rect(8, 0, 160, BUTTON_HEIGHT),
                        "Import",
                        move |cx| PartyTab::import_go_on_click(cx, me),
                    ),
                )
                .id();
            // L322-324
            cx.base_mut(go).enabled = Prop::f(move |cx, _| {
                let t = cx.w::<PartyTab>(me);
                t.import_code_valid && !t.import_code_fetching
            });
            // L325-329
            cx.w_mut::<ButtonControl>(go).enter_func = Some(Rc::new(move |cx: &mut Cx| {
                // L326
                if cx.w::<PartyTab>(me).import_code_valid {
                    // L327
                    PartyTab::click_import_go(cx, me);
                }
            }));
            // L330-331
            let append = cx
                .add(
                    me,
                    "appendNotReplace",
                    CheckBoxControl::new(
                        Anchor::new(Left, go, Right),
                        rect(60, 0, BUTTON_HEIGHT, 0),
                        "Append",
                        Some(Rc::new(|_: &mut Cx, _state: bool| {})),
                        Some("This sets the import button to append to the current party lists instead of replacing them (curses will still replace)".into()),
                        false,
                    ),
                )
                .id();
            // L332-334
            cx.base_mut(append).x = Prop::f(move |cx, _| {
                if cx.base(me).width.raw() > WIDTH_THRESHOLD1 {
                    60.0
                } else {
                    -276.0
                }
            });
            // L335-337
            cx.base_mut(append).y = Prop::f(move |cx, _| {
                if cx.base(me).width.raw() > WIDTH_THRESHOLD1 {
                    0.0
                } else {
                    24.0
                }
            });

            // L339-344
            let clear = cx
                .add(
                    me,
                    "clear",
                    ButtonControl::new(
                        Anchor::new(Left, append, Right),
                        rect(8, 0, 160, BUTTON_HEIGHT),
                        "Clear",
                        move |cx| {
                            // L340
                            PartyTab::clear_input_text(cx, me);
                            // L341-342
                            let m = model_of(cx);
                            reset_enemy_mod_list(&m);
                            // L343
                            m.buildFlag.set(true);
                        },
                    ),
                )
                .id();
            // L345
            cx.w_mut::<ButtonControl>(clear).tt.tooltip_text =
                Some(Prop::Val(Some("^7Clears all the party tab imported data".to_owned())));

            // L347-348
            let show_adv = cx
                .add(
                    me,
                    "ShowAdvanceTools",
                    CheckBoxControl::new(
                        Anchor::new(TopLeft, dest, BottomLeft),
                        rect(140, 4, BUTTON_HEIGHT, 0),
                        "^7Show Advanced Info",
                        Some(Rc::new(|_: &mut Cx, _state: bool| {})),
                        Some("This shows the advanced info like what stats each aura/curse etc are adding, as well as enables the ability to edit them without a re-export\nDo not edit any boxes unless you know what you are doing, use copy/paste or import instead".into()),
                        false,
                    ),
                )
                .id();
            // L349-351
            cx.base_mut(show_adv).y = Prop::f(move |cx, _| {
                if cx.base(me).width.raw() > WIDTH_THRESHOLD1 {
                    4.0
                } else {
                    28.0
                }
            });

            // L353-360
            let remove = cx
                .add(
                    me,
                    "removeEffects",
                    ButtonControl::new(
                        Anchor::new(Left, show_adv, Right),
                        rect(8, 0, 160, BUTTON_HEIGHT),
                        "Disable Party Effects",
                        move |cx| {
                            let m = model_of(cx);
                            // L354-358
                            reset_actor(&m);
                            reset_enemy_mod_list(&m);
                            // L359
                            m.buildFlag.set(true);
                        },
                    ),
                )
                .id();
            // L361
            cx.w_mut::<ButtonControl>(remove).tt.tooltip_text = Some(Prop::Val(Some(
                "^7Removes the effects of the supports, without removing the data\nUse \"rebuild all\" to apply the effects again".to_owned(),
            )));

            // L363-377
            let rebuild = cx
                .add(
                    me,
                    "rebuild",
                    ButtonControl::new(
                        Anchor::new(Left, remove, Right),
                        rect(8, 0, 160, BUTTON_HEIGHT),
                        "^7Rebuild All",
                        move |cx| {
                            let m = model_of(cx);
                            // L364-368
                            reset_actor(&m);
                            reset_enemy_mod_list(&m);
                            // L369
                            parse_party_member_stats(cx, &buf(cx, me, "editPartyMemberStats"));
                            // L370
                            parse_actor_list(
                                cx,
                                n!("Aura"),
                                &buf(cx, me, "editAuras"),
                                n!("Aura"),
                                "simpleAuras",
                            );
                            // L371
                            parse_actor_list(
                                cx,
                                n!("Curse"),
                                &buf(cx, me, "editCurses"),
                                n!("Curse"),
                                "simpleCurses",
                            );
                            // L372
                            parse_actor_list(
                                cx,
                                n!("Warcry"),
                                &buf(cx, me, "editWarcries"),
                                n!("Warcry"),
                                "simpleWarcries",
                            );
                            // L373
                            parse_actor_list(
                                cx,
                                n!("Link"),
                                &buf(cx, me, "editLinks"),
                                n!("Link"),
                                "simpleLinks",
                            );
                            // L374
                            parse_enemy(cx, &buf(cx, me, "enemyCond"), n!("EnemyConditions"), None);
                            // L375
                            parse_enemy(
                                cx,
                                &buf(cx, me, "enemyMods"),
                                n!("EnemyMods"),
                                Some("simpleEnemyMods"),
                            );
                            // L376
                            m.buildFlag.set(true);
                        },
                    ),
                )
                .id();
            // L378
            cx.w_mut::<ButtonControl>(rebuild).tt.tooltip_text = Some(Prop::Val(Some(
                "^7Reparse all the inputs in case they have been disabled or they have changed since loading the build or importing".to_owned(),
            )));
            // L379-381
            cx.base_mut(rebuild).x = Prop::f(move |cx, _| {
                if cx.base(me).width.raw() > WIDTH_THRESHOLD1 {
                    8.0
                } else {
                    -328.0
                }
            });
            // L382-384
            cx.base_mut(rebuild).y = Prop::f(move |cx, _| {
                if cx.base(me).width.raw() > WIDTH_THRESHOLD1 {
                    0.0
                } else {
                    24.0
                }
            });

            // the big edits: EditControl(anchor, rect, "", nil, "^%C\t\n", nil, nil, 14, true)
            let big_edit = |anchor: Anchor, y: f64| {
                EditControl::new(
                    anchor,
                    rect(0, y, 0, 0),
                    EditArgs {
                        init: Some(String::new()),
                        filter: Some("^%C\t\n".into()),
                        line_height: Some(14.0),
                        allow_zoom: true,
                        ..Default::default()
                    },
                )
            };
            // `width = function() return self.width / 2 - 16 end`
            let half_width = move || Prop::f(move |cx: &Cx, _| cx.base(me).width.raw() / 2.0 - 16.0);
            let shown_adv = move || Prop::f(move |cx: &Cx, _| show_advanced(cx, me));
            let shown_simple = move || Prop::f(move |cx: &Cx, _| !show_advanced(cx, me));
            // a `simple*` label whose text is the model's table (module notes)
            let simple = |cx: &mut Cx, key: &'static str, anchor: Anchor| {
                let id = cx
                    .add(
                        me,
                        key,
                        LabelControl::new(anchor, rect(0, 18, 0, STRING_HEIGHT), ""),
                    )
                    .id();
                cx.w_mut::<LabelControl>(id).label =
                    Prop::f(move |cx, _| simple_label(cx, key));
                cx.base_mut(id).shown = shown_simple();
                id
            };

            // L386
            let auras_label = cx
                .add(
                    me,
                    "editAurasLabel",
                    LabelControl::new(
                        Anchor::new(TopLeft, show_adv, TopLeft),
                        rect(-140, 40, 0, STRING_HEIGHT),
                        label("^7Auras"),
                    ),
                )
                .id();
            // L387-389
            cx.base_mut(auras_label).y = Prop::f(move |cx, _| {
                36.0 + if cx.base(me).width.raw() <= WIDTH_THRESHOLD1 {
                    24.0
                } else {
                    0.0
                }
            });
            // L390
            let edit_auras = cx
                .add(
                    me,
                    "editAuras",
                    big_edit(Anchor::new(TopLeft, auras_label, TopLeft), 18.0),
                )
                .id();
            // L391-393
            cx.base_mut(edit_auras).width = half_width();
            // L394-396
            cx.base_mut(edit_auras).height = Prop::f(move |cx, _| {
                if has_focus(cx, me, "editWarcries") || has_focus(cx, me, "editLinks") {
                    BUFFER_HEIGHT_SMALL
                } else {
                    buffer_height_left(cx, me)
                }
            });
            // L398-400
            cx.base_mut(edit_auras).shown = shown_adv();
            // L401-404
            simple(
                cx,
                "simpleAuras",
                Anchor::new(TopLeft, auras_label, TopLeft),
            );

            // L406
            let warcries_label = cx
                .add(
                    me,
                    "editWarcriesLabel",
                    LabelControl::new(
                        Anchor::new(TopLeft, auras_label, BottomLeft),
                        rect(0, 8, 0, STRING_HEIGHT),
                        label("^7Warcry Skills"),
                    ),
                )
                .id();
            // L407-409
            cx.base_mut(warcries_label).y = Prop::f(move |cx, _| {
                if show_advanced(cx, me) {
                    height_of(cx, me, "editAuras") + 8.0
                } else {
                    line_counter(&simple_label(cx, "simpleAuras")) + 4.0
                }
            });
            // L410
            let edit_warcries = cx
                .add(
                    me,
                    "editWarcries",
                    big_edit(Anchor::new(TopLeft, warcries_label, TopLeft), 18.0),
                )
                .id();
            // L411-413
            cx.base_mut(edit_warcries).width = half_width();
            // L414-416
            cx.base_mut(edit_warcries).height = Prop::f(move |cx, _| {
                if has_focus(cx, me, "editWarcries") {
                    buffer_height_left(cx, me)
                } else {
                    BUFFER_HEIGHT_SMALL
                }
            });
            // L417-419
            cx.base_mut(edit_warcries).shown = shown_adv();
            // L420-423
            simple(
                cx,
                "simpleWarcries",
                Anchor::new(TopLeft, warcries_label, TopLeft),
            );

            // L425
            let links_label = cx
                .add(
                    me,
                    "editLinksLabel",
                    LabelControl::new(
                        Anchor::new(TopLeft, warcries_label, BottomLeft),
                        rect(0, 8, 0, STRING_HEIGHT),
                        label("^7Link Skills"),
                    ),
                )
                .id();
            // L426-428
            cx.base_mut(links_label).y = Prop::f(move |cx, _| {
                if show_advanced(cx, me) {
                    height_of(cx, me, "editWarcries") + 8.0
                } else {
                    line_counter(&simple_label(cx, "simpleWarcries")) + 4.0
                }
            });
            // L429
            let edit_links = cx
                .add(
                    me,
                    "editLinks",
                    big_edit(Anchor::new(TopLeft, links_label, TopLeft), 18.0),
                )
                .id();
            // L430-432
            cx.base_mut(edit_links).width = half_width();
            // L433-435
            cx.base_mut(edit_links).height = Prop::f(move |cx, _| {
                if has_focus(cx, me, "editLinks") {
                    buffer_height_left(cx, me)
                } else {
                    BUFFER_HEIGHT_SMALL
                }
            });
            // L436-438
            cx.base_mut(edit_links).shown = shown_adv();
            // L439-442
            simple(
                cx,
                "simpleLinks",
                Anchor::new(TopLeft, links_label, TopLeft),
            );

            // L444
            let stats_label = cx
                .add(
                    me,
                    "editPartyMemberStatsLabel",
                    LabelControl::new(
                        Anchor::new(TopLeft, notes_desc, TopRight),
                        rect(8, 0, 0, STRING_HEIGHT),
                        label("^7Party Member Stats"),
                    ),
                )
                .id();
            // L445
            let edit_stats = cx
                .add(
                    me,
                    "editPartyMemberStats",
                    big_edit(Anchor::new(TopLeft, stats_label, BottomLeft), 2.0),
                )
                .id();
            // L446-448
            cx.base_mut(edit_stats).width = half_width();
            // L449-451
            cx.base_mut(edit_stats).height = Prop::f(move |cx, _| {
                if has_focus(cx, me, "editPartyMemberStats") {
                    tab_size(cx, me).1 - BUFFER_HEIGHT_RIGHT
                } else {
                    BUFFER_HEIGHT_SMALL
                }
            });
            // L452-454
            cx.base_mut(edit_stats).shown = shown_adv();

            // L456
            let cond_label = cx
                .add(
                    me,
                    "enemyCondLabel",
                    LabelControl::new(
                        Anchor::new(TopLeft, stats_label, BottomLeft),
                        rect(0, 8, 0, STRING_HEIGHT),
                        label("^7Enemy Conditions"),
                    ),
                )
                .id();
            // L457-459
            cx.base_mut(cond_label).y = Prop::f(move |cx, _| {
                if show_advanced(cx, me) {
                    height_of(cx, me, "editPartyMemberStats") + 8.0
                } else {
                    4.0
                }
            });
            // L460
            let enemy_cond = cx
                .add(
                    me,
                    "enemyCond",
                    big_edit(Anchor::new(TopLeft, cond_label, BottomLeft), 2.0),
                )
                .id();
            // L461-463
            cx.base_mut(enemy_cond).width = half_width();
            // L464-466
            cx.base_mut(enemy_cond).height = Prop::f(move |cx, _| {
                if has_focus(cx, me, "enemyCond") {
                    tab_size(cx, me).1 - BUFFER_HEIGHT_RIGHT
                } else {
                    BUFFER_HEIGHT_SMALL
                }
            });
            // L467-469
            cx.base_mut(enemy_cond).shown = shown_adv();
            // L470-473
            simple(
                cx,
                "simpleEnemyCond",
                Anchor::new(TopLeft, cond_label, TopLeft),
            );

            // L475
            let mods_label = cx
                .add(
                    me,
                    "enemyModsLabel",
                    LabelControl::new(
                        Anchor::new(TopLeft, cond_label, BottomLeft),
                        rect(0, 8, 0, STRING_HEIGHT),
                        label("^7Enemy Modifiers"),
                    ),
                )
                .id();
            // L476-478
            cx.base_mut(mods_label).y = Prop::f(move |cx, _| {
                if show_advanced(cx, me) {
                    height_of(cx, me, "enemyCond") + 8.0
                } else {
                    line_counter(&simple_label(cx, "simpleEnemyCond")) + 4.0
                }
            });
            // L479
            let enemy_mods = cx
                .add(
                    me,
                    "enemyMods",
                    big_edit(Anchor::new(TopLeft, mods_label, BottomLeft), 2.0),
                )
                .id();
            // L480-482
            cx.base_mut(enemy_mods).width = half_width();
            // L483-485
            cx.base_mut(enemy_mods).height = Prop::f(move |cx, _| {
                if has_focus(cx, me, "enemyMods") {
                    tab_size(cx, me).1 - BUFFER_HEIGHT_RIGHT
                } else {
                    BUFFER_HEIGHT_SMALL
                }
            });
            // L486-488
            cx.base_mut(enemy_mods).shown = shown_adv();
            // L489-492
            simple(
                cx,
                "simpleEnemyMods",
                Anchor::new(TopLeft, mods_label, TopLeft),
            );

            // L494
            let curses_label = cx
                .add(
                    me,
                    "editCursesLabel",
                    LabelControl::new(
                        Anchor::new(TopLeft, mods_label, BottomLeft),
                        rect(0, 8, 0, STRING_HEIGHT),
                        label("^7Curses"),
                    ),
                )
                .id();
            // L495-497
            cx.base_mut(curses_label).y = Prop::f(move |cx, _| {
                if show_advanced(cx, me) {
                    height_of(cx, me, "enemyMods") + 8.0
                } else {
                    line_counter(&simple_label(cx, "simpleEnemyMods")) + 4.0
                }
            });
            // L498
            let edit_curses = cx
                .add(
                    me,
                    "editCurses",
                    big_edit(Anchor::new(TopLeft, curses_label, BottomLeft), 2.0),
                )
                .id();
            // L499-501
            cx.base_mut(edit_curses).width = half_width();
            // L502-504
            cx.base_mut(edit_curses).height = Prop::f(move |cx, _| {
                if has_focus(cx, me, "enemyCond")
                    || has_focus(cx, me, "enemyMods")
                    || has_focus(cx, me, "editPartyMemberStats")
                {
                    BUFFER_HEIGHT_SMALL
                } else {
                    tab_size(cx, me).1 - BUFFER_HEIGHT_RIGHT
                }
            });
            // L505-507
            cx.base_mut(edit_curses).shown = shown_adv();
            // L508-511
            simple(
                cx,
                "simpleCurses",
                Anchor::new(TopLeft, curses_label, TopLeft),
            );
            // L512
            cx.select_control(me, Some(edit_auras));
            // L513 return self
        })
    }

    /// `self.controls.importCodeGo.onClick()`
    fn click_import_go(cx: &mut Cx, me: CtrlId) {
        let go = c(cx, me, "importCodeGo");
        let f = cx.w::<ButtonControl>(go).on_click.clone();
        let _ = f(cx);
    }

    /// The Import button's `onClick` (L302-321).
    fn import_go_on_click(cx: &mut Cx, me: CtrlId) {
        // L303 local importCodeFetching = false (unused)
        // L304
        let (site, has_xml) = {
            let t = cx.w::<PartyTab>(me);
            (t.import_code_site, t.import_code_xml.is_some())
        };
        if let (Some(site), false) = (site, has_xml) {
            // L305
            cx.w_mut::<PartyTab>(me).import_code_fetching = true;
            // L306 local selectedWebsite = buildSites.websiteList[self.importCodeSite]
            // L307-316
            let link = buf(cx, me, "importCodeIn");
            let h = site_hooks();
            (h.download_build)(
                cx,
                &link,
                site,
                Box::new(move |cx: &mut Cx, is_success: bool, data: &str| {
                    // L308
                    cx.w_mut::<PartyTab>(me).import_code_fetching = false;
                    // L309
                    if !is_success {
                        // L310
                        let detail = format!("{}{data}", cx.main.cc.NEGATIVE);
                        let t = cx.w_mut::<PartyTab>(me);
                        t.import_code_detail = Some(detail);
                        // L311
                        t.import_code_valid = false;
                    } else {
                        // L313
                        PartyTab::import_code_handle(cx, me, data);
                        // L314
                        PartyTab::finish_import(cx, me);
                    }
                }),
            );
            // L317
            return;
        }

        // L320
        PartyTab::finish_import(cx, me);
    }

    /// The constructor's local `clearInputText` (L82-118).
    /// @pob Classes/PartyTab.lua:82-119 h=dda044ae
    pub fn clear_input_text(cx: &mut Cx, me: CtrlId) {
        let m = model_of(cx);
        // L83
        if dest_is(cx, me, "All") || dest_is(cx, me, "Party Member Stats") {
            // L84
            set_text(cx, me, "editPartyMemberStats", "");
        }
        // L86
        if dest_is(cx, me, "All") || dest_is(cx, me, "Aura") {
            // L87
            set_simple_label(cx, "simpleAuras", "");
            // L88
            set_text(cx, me, "editAuras", "");
            // L89-90
            reset_actor_list(&m, n!("Aura"));
        }
        // L92
        if dest_is(cx, me, "All") || dest_is(cx, me, "Curse") {
            // L93
            set_simple_label(cx, "simpleCurses", "");
            // L94
            set_text(cx, me, "editCurses", "");
            // L95-96
            reset_actor_list(&m, n!("Curse"));
        }
        // L98
        if dest_is(cx, me, "All") || dest_is(cx, me, "Warcry Skills") {
            // L99
            set_simple_label(cx, "simpleWarcries", "");
            // L100
            set_text(cx, me, "editWarcries", "");
            // L101-102
            reset_actor_list(&m, n!("Warcry"));
        }
        // L104
        if dest_is(cx, me, "All") || dest_is(cx, me, "Link Skills") {
            // L105
            set_simple_label(cx, "simpleLinks", "");
            // L106
            set_text(cx, me, "editLinks", "");
            // L107-108
            reset_actor_list(&m, n!("Link"));
        }
        // L110
        if dest_is(cx, me, "All") || dest_is(cx, me, "EnemyConditions") {
            // L111
            set_simple_label(cx, "simpleEnemyCond", "^7---------------------------\n");
            // L112
            set_text(cx, me, "enemyCond", "");
        }
        // L114
        if dest_is(cx, me, "All") || dest_is(cx, me, "EnemyMods") {
            // L115
            set_simple_label(cx, "simpleEnemyMods", "\n");
            // L116
            set_text(cx, me, "enemyMods", "");
        }
    }

    /// The constructor's local `importCodeHandle` (L120-160), the code edit's `changeFunc`.
    /// @pob Classes/PartyTab.lua:120-161 h=8ff73b18
    pub fn import_code_handle(cx: &mut Cx, me: CtrlId, buf: &str) {
        {
            let t = cx.w_mut::<PartyTab>(me);
            // L121
            t.import_code_site = None;
            // L122
            t.import_code_detail = Some(String::new());
            // L123
            t.import_code_xml = None;
            // L124
            t.import_code_valid = false;
        }

        // L126
        if buf.is_empty() {
            // L127
            return;
        }

        // L130
        let negative = cx.main.cc.NEGATIVE.clone();
        cx.w_mut::<PartyTab>(me).import_code_detail = Some(format!("{negative}Invalid input"));
        // L131 Quick Trim
        let mut url_text = gsub(&gsub(buf, "^[%s?]+", ""), "[%s?]+$", "");
        // L132
        if lmatch(&url_text, "youtube%.com/redirect%?").is_some()
            || lmatch(&url_text, "google%.com/url%?").is_some()
        {
            // L133
            let nested_url = gsub(&url_text, ".*[?&]q=([^&]+).*", "%1");
            // L134
            url_text = (site_hooks().url_decode)(&nested_url);
        }

        // L137
        let sites = (site_hooks().website_list)(cx);
        for (j, site) in sites.iter().enumerate() {
            // L138
            if lmatch(&url_text, &site.match_url).is_some() {
                // L139 self.controls.importCodeIn.text = urlText (a field nothing reads)
                let positive = cx.main.cc.POSITIVE.clone();
                let t = cx.w_mut::<PartyTab>(me);
                // L140
                t.import_code_valid = true;
                // L141
                t.import_code_detail = Some(format!("{positive}URL is valid ({})", site.label));
                // L142
                t.import_code_site = Some(j + 1);
                // L143
                if buf != url_text {
                    // L144
                    let code_in = c(cx, me, "importCodeIn");
                    EditControl::set_text(cx, code_in, &url_text, false);
                }
                // L146
                return;
            }
        }

        // L150
        let decoded = // CCR-0114 §4: `common.base64` is not re-exported by `model.rs`
        pob_calc::classes::passive_spec::base64_decode(&gsub(&gsub(buf, "-", "+"), "_", "/"));
        let Some(xml_bytes) = inflate(&decoded) else {
            // L151-152
            return;
        };
        // DIVERGENCE(bytes): see the module notes
        let xml_text = String::from_utf8_lossy(&xml_bytes).into_owned();
        // L154
        if cx.app().launch.dev_mode && cx.sys.is_key_down(Key::Shift) {
            // L155
            cx.sys.platform.copy(&xml_text);
        }
        let positive = cx.main.cc.POSITIVE.clone();
        let t = cx.w_mut::<PartyTab>(me);
        // L157
        t.import_code_valid = true;
        // L158
        t.import_code_detail = Some(format!("{positive}Code is valid"));
        // L159
        t.import_code_xml = Some(xml_text);
    }

    /// The constructor's local `finishImport` (L162-285).
    /// @pob Classes/PartyTab.lua:162-286 h=dddc8ad1
    pub fn finish_import(cx: &mut Cx, me: CtrlId) {
        // L163
        {
            let t = cx.w::<PartyTab>(me);
            if !t.import_code_valid || t.import_code_fetching {
                // L164
                return;
            }
        }
        let m = model_of(cx);

        // L167
        let mut current_curse_buffer: Option<String> = None;
        // L168
        let mut current_link_buffer: Option<String> = None;
        // L169
        let append = cx.w::<CheckBoxControl>(c(cx, me, "appendNotReplace")).state;
        if !append {
            // L170
            PartyTab::clear_input_text(cx, me);
        } else {
            // L172
            if dest_is(cx, me, "All") || dest_is(cx, me, "Aura") {
                // L173-174
                reset_actor_list(&m, n!("Aura"));
            }
            // L176
            if dest_is(cx, me, "All") || dest_is(cx, me, "Curse") {
                // L177
                current_curse_buffer = Some(buf(cx, me, "editCurses"));
                // L178 curses do not play nicely with append atm, need to fix
                set_text(cx, me, "editCurses", "");
                // L179-180
                reset_actor_list(&m, n!("Curse"));
            }
            // L182
            if dest_is(cx, me, "All") || dest_is(cx, me, "Warcry Skills") {
                // L183-184
                reset_actor_list(&m, n!("Warcry"));
            }
            // L186
            if dest_is(cx, me, "All") || dest_is(cx, me, "Link Skills") {
                // L187 only one link can be applied at a time anyway
                // L188
                current_link_buffer = Some(buf(cx, me, "editLinks"));
                // L189
                set_text(cx, me, "editLinks", "");
                // L190-191
                reset_actor_list(&m, n!("Link"));
            }
        }

        // L195 Parse the XML
        // L196
        let xml = cx
            .w::<PartyTab>(me)
            .import_code_xml
            .clone()
            .unwrap_or_else(|| {
                lua_error!("bad argument #1 to 'ParseXML' (string expected, got nil)")
            });
        let db_xml = match crate::model::xml::ParseXML_root(&xml) {
            Ok(root) => root,
            Err(err_msg) => {
                // L198 (`fileName` is an undeclared global: LUA-GLOBAL-NIL)
                crate::launch::show_err_msg(
                    cx,
                    "^1Error loading '%s': %s",
                    &[FmtArg::Nil, FmtArg::Str(&err_msg)],
                );
                // L199
                return;
            }
        };
        // L200 `dbXML[1].elem`
        let root = match db_xml.children.first() {
            None => lua_error!("attempt to index field '?' (a nil value)"),
            Some(XmlChild::Node(n)) if &*n.elem == "PathOfBuilding" => n,
            Some(_) => {
                // L201 (`fileName`: LUA-GLOBAL-NIL)
                crate::launch::show_err_msg(
                    cx,
                    "^1Error parsing '%s': 'PathOfBuilding' root element missing",
                    &[FmtArg::Nil],
                );
                // L202
                return;
            }
        };

        // L205 Load data
        // L206
        for tab_node in &root.children {
            // L207
            let XmlChild::Node(tab_node) = tab_node else {
                continue;
            };
            if &*tab_node.elem != "Party" {
                continue;
            }
            // L208
            for node in &tab_node.children {
                // L209 (a text child: `("…").elem` is nil)
                let XmlChild::Node(node) = node else {
                    continue;
                };
                if &*node.elem != "ExportedBuffs" {
                    continue;
                }
                // L210
                match node.attrib.get("name").map(|s| &**s) {
                    None => {
                        // L211
                        cx.sys.platform.con_print("missing name");
                    }
                    // L212
                    Some("PartyMemberStats") => {
                        // L213
                        if dest_is(cx, me, "All") || dest_is(cx, me, "Party Member Stats") {
                            let mut text = Node1::of(node);
                            let cur = buf(cx, me, "editPartyMemberStats");
                            // L214
                            if !cur.is_empty() {
                                // L215
                                text = text.concat_after(&cur);
                            }
                            // L217
                            set_text(cx, me, "editPartyMemberStats", &text.set_text_arg());
                            // L218
                            parse_party_member_stats(cx, &buf(cx, me, "editPartyMemberStats"));
                        }
                    }
                    // L220
                    Some("Aura") => {
                        // L221
                        if dest_is(cx, me, "All") || dest_is(cx, me, "Aura") {
                            let mut text = Node1::of(node);
                            let cur = buf(cx, me, "editAuras");
                            // L222
                            if !cur.is_empty() {
                                // L223
                                text = text.concat_after(&cur);
                            }
                            // L225
                            set_text(cx, me, "editAuras", &text.set_text_arg());
                            // L226
                            parse_actor_list(
                                cx,
                                n!("Aura"),
                                &buf(cx, me, "editAuras"),
                                n!("Aura"),
                                "simpleAuras",
                            );
                        }
                    }
                    // L228
                    Some("Curse") => {
                        // L229
                        if dest_is(cx, me, "All") || dest_is(cx, me, "Curse") {
                            let mut text = Node1::of(node);
                            let cur = buf(cx, me, "editCurses");
                            // L230
                            if !cur.is_empty() {
                                // L231
                                text = text.concat_after(&cur);
                            }
                            // L233
                            if let Some(ccb) = &current_curse_buffer {
                                if text.as_str() == Some("--- Curse Limit ---\n1") {
                                    // L234
                                    text = Node1::Str(ccb.clone());
                                }
                            }
                            // L236
                            set_text(cx, me, "editCurses", &text.set_text_arg());
                            // L237
                            parse_actor_list(
                                cx,
                                n!("Curse"),
                                &buf(cx, me, "editCurses"),
                                n!("Curse"),
                                "simpleCurses",
                            );
                        }
                    }
                    // L239
                    Some("Warcry Skills") => {
                        // L240
                        if dest_is(cx, me, "All") || dest_is(cx, me, "Warcry Skills") {
                            let mut text = Node1::of(node);
                            let cur = buf(cx, me, "editWarcries");
                            // L241
                            if !cur.is_empty() {
                                // L242
                                text = text.concat_after(&cur);
                            }
                            // L244
                            set_text(cx, me, "editWarcries", &text.set_text_arg());
                            // L245
                            parse_actor_list(
                                cx,
                                n!("Warcry"),
                                &buf(cx, me, "editWarcries"),
                                n!("Warcry"),
                                "simpleWarcries",
                            );
                        }
                    }
                    // L247
                    Some("Link Skills") => {
                        // L248
                        if dest_is(cx, me, "All") || dest_is(cx, me, "Link Skills") {
                            let mut text = Node1::of(node);
                            let cur = buf(cx, me, "editLinks");
                            // L249
                            if !cur.is_empty() {
                                // L250
                                text = text.concat_after(&cur);
                            }
                            // L252
                            if let Some(clb) = &current_link_buffer {
                                if matches!(text, Node1::Nil) || text.as_str() == Some("") {
                                    // L253
                                    text = Node1::Str(clb.clone());
                                }
                            }
                            // L255
                            set_text(cx, me, "editLinks", &text.set_text_arg());
                            // L256
                            parse_actor_list(
                                cx,
                                n!("Link"),
                                &buf(cx, me, "editLinks"),
                                n!("Link"),
                                "simpleLinks",
                            );
                        }
                    }
                    // L258
                    Some("EnemyConditions") => {
                        // L259
                        if dest_is(cx, me, "All") || dest_is(cx, me, "EnemyConditions") {
                            let mut text = Node1::of(node);
                            let cur = buf(cx, me, "enemyCond");
                            // L260
                            if !cur.is_empty() {
                                // L261
                                text = text.concat_after(&cur);
                            }
                            // L263
                            set_text(cx, me, "enemyCond", &text.set_text_arg());
                        }
                    }
                    // L265
                    Some("EnemyMods") => {
                        // L266
                        if dest_is(cx, me, "All") || dest_is(cx, me, "EnemyMods") {
                            let mut text = Node1::of(node);
                            let cur = buf(cx, me, "enemyMods");
                            // L267
                            if !cur.is_empty() {
                                // L268
                                text = text.concat_after(&cur);
                            }
                            // L270
                            set_text(cx, me, "enemyMods", &text.set_text_arg());
                        }
                    }
                    Some(_) => {}
                }
            }
            // L275
            if dest_is(cx, me, "All")
                || dest_is(cx, me, "EnemyConditions")
                || dest_is(cx, me, "EnemyMods")
            {
                // L276-277
                reset_enemy_mod_list(&m);
                // L278
                parse_enemy(cx, &buf(cx, me, "enemyCond"), n!("EnemyConditions"), None);
                // L279
                parse_enemy(
                    cx,
                    &buf(cx, me, "enemyMods"),
                    n!("EnemyMods"),
                    Some("simpleEnemyMods"),
                );
            }
            // L281
            m.buildFlag.set(true);
            // L282
            break;
        }
    }

    /// lua: PartyTab.lua:670-717
    /// @pob Classes/PartyTab.lua:670-717 h=9dbd8333
    pub fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, ev: &mut InputEvents) {
        {
            let b = cx.base_mut(me);
            // L671-674
            b.x = vp.x.into();
            b.y = vp.y.into();
            b.width = vp.width.into();
            b.height = vp.height.into();
        }

        // L676
        let mut i = 0;
        while let Some(e) = ev.next_ipairs(&mut i) {
            // L677
            if let EventKind::KeyDown { .. } = e.kind {
                // L678
                if e.key == Key::Char(b'z') && cx.sys.is_key_down(Key::Ctrl) {
                    // L679-687
                    for key in [
                        "editAuras",
                        "editCurses",
                        "editLinks",
                        "editPartyMemberStats",
                    ] {
                        if has_focus(cx, me, key) {
                            let id = c(cx, me, key);
                            EditControl::undo(cx, id);
                            break;
                        }
                    }
                // L688
                } else if e.key == Key::Char(b'y') && cx.sys.is_key_down(Key::Ctrl) {
                    // L689-697
                    for key in [
                        "editAuras",
                        "editCurses",
                        "editLinks",
                        "editPartyMemberStats",
                    ] {
                        if has_focus(cx, me, key) {
                            let id = c(cx, me, key);
                            EditControl::redo(cx, id);
                            break;
                        }
                    }
                }
            }
        }
        // L701
        cx.process_controls_input(me, ev, vp);

        // L703
        draw_background(cx, vp);

        // L705
        cx.draw_controls(me, vp, None);

        // L707-715
        let last = model_of(cx).partyTab.lastContent();
        let differs = |k: &str, b: &str| -> bool {
            match last.get(model::Name::from(k)) {
                Val::Str(s) => &*s != b,
                _ => true,
            }
        };
        let mut mod_flag = differs("Aura", &buf(cx, me, "editAuras"))
            || differs("Curse", &buf(cx, me, "editCurses"))
            || differs("Warcry", &buf(cx, me, "editWarcries"))
            || differs("Link", &buf(cx, me, "editLinks"))
            || differs("PartyMemberStats", &buf(cx, me, "editPartyMemberStats"))
            || differs("EnemyCond", &buf(cx, me, "enemyCond"))
            || differs("EnemyMods", &buf(cx, me, "enemyMods"));
        if !mod_flag {
            let export = model_of(cx).partyTab.enableExportBuffs.get();
            mod_flag = !val_is_bool(&last.get(n!("EnableExportBuffs")), export);
        }
        if !mod_flag {
            let adv = show_advanced(cx, me);
            mod_flag = !val_is_bool(&last.get(n!("showAdvancedTools")), adv);
        }
        cx.w_mut::<PartyTab>(me).mod_flag = mod_flag;

        // CCR-0114 §1: the model's copy of the controls' state (module notes)
        sync_to_model(cx, me);
    }
}

/// CCR-0114 §1: the edits' `buf`, the check boxes' `state` and the destination's `selIndex`
/// into the model's control tables, which the model's Save reads.
fn sync_to_model(cx: &mut Cx, me: CtrlId) {
    for key in MODEL_EDITS {
        let b = buf(cx, me, key);
        mctl(cx, key).set(n!("buf"), b);
    }
    for key in ["appendNotReplace", "ShowAdvanceTools"] {
        let s = cx.w::<CheckBoxControl>(c(cx, me, key)).state;
        mctl(cx, key).set(n!("state"), s);
    }
    let sel = cx
        .w::<DropDownControl>(c(cx, me, "importCodeDestination"))
        .sel_index;
    mctl(cx, "importCodeDestination").set(n!("selIndex"), sel as f64);
}

/// Load's UI statements (PartyTab.lua:522-571), replayed after the constructor: the model's
/// Load ran inside `Init_model` (module notes). `SetText` of each edit (an edit whose section
/// was absent holds `""`, as after the constructor), `SelByValue(destination)`, the two
/// check boxes' `state`.
fn load_ui_tail(cx: &mut Cx, me: CtrlId) {
    for key in MODEL_EDITS {
        let v = mctl(cx, key).get(n!("buf"));
        let text = match &v {
            Val::Str(s) => s.to_string(),
            other => val_text(other, "buf"),
        };
        let id = c(cx, me, key);
        EditControl::set_text(cx, id, &text, false);
    }
    // L569
    let sel = mctl(cx, "importCodeDestination").get(n!("selIndex")).num();
    let dest = c(cx, me, "importCodeDestination");
    cx.w_mut::<DropDownControl>(dest).sel_index = sel as usize;
    // L570-571
    for key in ["appendNotReplace", "ShowAdvanceTools"] {
        let s = val_is_bool(&mctl(cx, key).get(n!("state")), true);
        let id = c(cx, me, key);
        cx.w_mut::<CheckBoxControl>(id).state = s;
    }
}

// ---- the BuildMode seams (CCR-0098 §3, CCR-0104 §1, CCR-0106 §2) -------------------------------

/// `self.partyTab` of the open build, when the tab exists.
fn tab(cx: &Cx) -> Option<CtrlId> {
    let id = cx.app().build.as_ref()?.tabs.party.id();
    cx.ui.try_w::<PartyTab>(id).map(|_| id)
}

fn tab_or_err(cx: &Cx) -> CtrlId {
    tab(cx).unwrap_or_else(|| lua_error!("attempt to index field 'partyTab' (a nil value)"))
}

/// `BuildHooks::party_tab`: `self.partyTab = new("PartyTab"):PartyTab(self)` (Build.lua:610;
/// the model part ran in `Init_model`), then Load's UI statements for the section `Init_model`
/// loaded (see [`load_ui_tail`]).
pub fn ctor_hook(cx: &mut Cx, build: CtrlId) -> CtrlId {
    let me = cx.add_free(Some(build), PartyTab::new(build)).id();
    load_ui_tail(cx, me);
    me
}

/// `ShellHooks::draw_tab` for `BuildTab::Party`: `self.partyTab:Draw(tabViewPort, inputEvents)`
/// (Build.lua:1305).
pub fn draw_tab(cx: &mut Cx, vp: &ViewPort, ev: &mut InputEvents) {
    let me = tab_or_err(cx);
    PartyTab::draw(cx, me, vp, ev);
}

/// `ShellHooks::tab_flag` for `TabFlag::Party`: `self.partyTab.modFlag` (nil before the first
/// Draw reads as false).
pub fn mod_flag(cx: &Cx) -> bool {
    tab(cx).is_some_and(|me| cx.w::<PartyTab>(me).mod_flag)
}

/// `ShellHooks::set_tab_flag` for `TabFlag::Party`: `self.partyTab.modFlag = v`
/// (ResetModFlags, Build.lua:1103).
pub fn set_mod_flag(cx: &mut Cx, v: bool) {
    if let Some(me) = tab(cx) {
        cx.w_mut::<PartyTab>(me).mod_flag = v;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_counter_counts_newlines() {
        assert_eq!(line_counter(""), 0.0);
        assert_eq!(line_counter("a\nb\n"), 32.0);
        assert_eq!(line_counter("^7---------------------------\n"), 16.0);
    }

    #[test]
    fn inflate_follows_simplegraphic() {
        use std::io::Write;
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(b"<PathOfBuilding/>").unwrap();
        let z = enc.finish().unwrap();
        assert_eq!(inflate(&z).as_deref(), Some(&b"<PathOfBuilding/>"[..]));
        // a truncated stream never reaches Z_STREAM_END: nil
        assert_eq!(inflate(&z[..z.len() - 3]), None);
        // not a zlib stream: nil
        assert_eq!(inflate(b"hello"), None);
        assert_eq!(inflate(b""), None);
        // trailing bytes after the stream end are ignored
        let mut t = z.clone();
        t.extend_from_slice(b"junk");
        assert_eq!(inflate(&t).as_deref(), Some(&b"<PathOfBuilding/>"[..]));
    }

    #[test]
    fn notes_desc_is_the_long_string() {
        // a missing checkout fails unless POB_ALLOW_SKIP=1 (CCR-0031); POB_ROOT / POB_SRC
        // override the search (worktrees do not hold the gitignored checkout)
        let Some(src) = pob_testpaths::require(
            pob_testpaths::pob_src(),
            "PoB checkout (.PathOfBuilding/src)",
        ) else {
            return;
        };
        let lua = std::fs::read_to_string(src.join("Classes/PartyTab.lua")).expect("PartyTab.lua");
        let a = lua.find("local notesDesc = [[").unwrap() + "local notesDesc = [[".len();
        let b = a + lua[a..].find("]]").unwrap();
        assert_eq!(&lua[a..b], NOTES_DESC);
    }
}
