//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ImportTab.lua:1-775 (ctor, Load/Save,
//! Draw) and 776-787 (Draw). Owner: U43.
//!
//! @pob-file Classes/ImportTab.lua @16de4b82d
//! @pob-file Modules/Common.lua @16de4b82d
//!
//! The ImportTab constructor (the "Build Sharing" section: generate/copy/share a build code,
//! import a code or a site URL), `TryFetchCharacterList`, `Load`, `Save`, `Draw`, and the seams
//! BuildMode calls ([`ctor_hook`], [`load_ui_section`], [`save_ui_section`], [`draw_tab`],
//! [`select_code_in`], [`try_fetch_character_list_hook`]; one arm each in
//! `init::default_hooks` / `on_frame::default_shell_hooks`, CCR-0106 §2). The two account
//! sections are `accounts.rs`; the character download/import methods (ImportTab.lua:789-1932)
//! are U44's and are reached through [`ImportHooks`] (CCR-0109 §5).
//!
//! Also here, as the first consumer in wave order (ui doc §12 note 15, CCR-0109 §7):
//! `urlEncode`/`urlDecode` (Common.lua:1024-1036), ImportTab.lua's global `HexToChar`/`UrlDecode`
//! (1913-1924, inside U44's line range but called at L594), `common.sha1`, and SimpleGraphic's
//! `Deflate`/`Inflate` ([`deflate`], [`inflate`]; `ui_api.cpp` `l_Deflate`/`l_Inflate`).
//!
//! `main.api`, `main.lastExportWebsite`, `main.lastCharacterHash` and the global `historyList`
//! are not `MainState` fields; CCR-0109 §1 keeps them in [`MainExtra`] (one per app).
//!
//! Lua semantics:
//! * `common.base64.encode(Deflate(x)):gsub("+","-"):gsub("/","_")` / the reverse: see
//!   [`encode_build_code`] / [`decode_build_code`]. DIVERGENCE(deflate-bytes): SimpleGraphic's
//!   `Deflate` is zlib's `deflateInit(&z, 9)`; the port uses miniz_oxide at level 9, whose
//!   compressed bytes differ from zlib's. The codes decode to the same XML and both decoders
//!   accept both encoders' output.
//! * DIVERGENCE(bytes): build XML and response bodies are Lua byte strings; the port keeps them
//!   as UTF-8 (`from_utf8_lossy` after Inflate), as `platform/net.rs` does.
//! * `SetText(text:gsub(..))` passes gsub's count as `notify` (a number, truthy): `true` here.
//! * DIVERGENCE(dead-tab): a download that completes after its ImportTab was replaced (the build
//!   was re-imported or closed) writes into an object nobody reaches in Lua; the port skips
//!   those writes (the arena node is gone) and keeps the effects on `main`.
//!
//! Props with side effects (ui doc §7.3): `charImportStatusLabel.label` (ImportTab.lua:53, :61)
//! clears `oauthTimer` and `rateLimitEndTime`: both are `Cell`s.
//! ORDER: `pairs(buildSites.websiteList)` (L525) runs over an array (LuaJIT's array part, index
//! order); the others are in accounts.rs.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use pob_ui_core::controls::{
    ButtonControl, CheckBoxControl, DropDownControl, DropItem, EditArgs, EditControl, LabelControl,
    SectionControl,
};
use pob_ui_core::input::InputEvents;
use pob_ui_core::luastr::{gsub, lmatch};
use pob_ui_core::popups;
use pob_ui_core::prelude::*;

use super::accounts::{self, Realm};
use super::build_sites::{self, WebsiteInfo, WEBSITE_LIST};
use super::poe_api::{self, json_get, Json, PoEAPI};
use super::ImportTab;
use crate::app::CxApp;
use crate::build::init as build_init;
use crate::build::io as build_io;
use crate::main_mode::background::draw_background;
use crate::main_mode::ModeArg;
use crate::model::xml::XmlNode;

// ---- SimpleGraphic / Common.lua helpers (CCR-0109 §7) -----------------------------------------

/// `Deflate(data)` (SimpleGraphic `l_Deflate`: zlib stream, level 9). DIVERGENCE(deflate-bytes).
pub fn deflate(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 9)
}

/// `Inflate(data)` (SimpleGraphic `l_Inflate`: a zlib stream; nil unless it ends with
/// `Z_STREAM_END`).
pub fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    miniz_oxide::inflate::decompress_to_vec_zlib(data).ok()
}

/// `common.base64.encode(Deflate(xml)):gsub("+","-"):gsub("/","_")` (ImportTab.lua:505).
/// CCR-0109 §6: `common.base64` is calc's port in `passive_spec` (not re-exported by
/// `crate::model`).
pub fn encode_build_code(xml: &str) -> String {
    pob_calc::classes::passive_spec::base64_encode(&deflate(xml.as_bytes()))
        .replace('+', "-")
        .replace('/', "_")
}

/// `Inflate(common.base64.decode(buf:gsub("-","+"):gsub("_","/")))` (ImportTab.lua:632).
pub fn decode_build_code(buf: &str) -> Option<String> {
    let b64 = buf.replace('-', "+").replace('_', "/");
    let bytes = pob_calc::classes::passive_spec::base64_decode(&b64);
    inflate(&bytes).map(|x| String::from_utf8_lossy(&x).into_owned())
}

/// `common.sha1(s)` (runtime/lua/sha1/init.lua `sha1.sha1`: the lowercase hex digest).
pub fn sha1_hex(s: &str) -> String {
    let data = s.as_bytes();
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().fold(String::new(), |mut out, x| {
        use std::fmt::Write as _;
        let _ = write!(out, "{x:08x}");
        out
    })
}

/// lua: Common.lua:1024-1029 `urlEncode(str)` (in expression position: the string)
/// @pob Modules/Common.lua:1024-1029 h=9886f08f
pub fn url_encode(s: &str) -> String {
    // L1025-1027 charToHex
    // L1028
    pob_lua::gsub_fn(
        s,
        "([^%w_%-.~])",
        |c| {
            let b = c
                .first()
                .map_or(0, |c| c.as_bytes().first().copied().unwrap_or(0));
            Some(pob_lua::lfmt!("%%%02X", f64::from(b)))
        },
        None,
    )
    .0
}

/// `string.char(tonumber(x, 16))` of a `%x%x` capture.
fn hex_char(c: &pob_lua::pattern::Captures<'_>) -> Option<String> {
    let x = c
        .first()
        .map(|c| c.to_str().into_owned())
        .unwrap_or_default();
    let n = pob_lua::tonumber_base(&x, 16).unwrap_or(0.0);
    let b = n as u8;
    // a byte >= 0x80 is not UTF-8 on its own: DIVERGENCE(bytes) (lossy)
    Some(String::from_utf8_lossy(&[b]).into_owned())
}

/// lua: Common.lua:1031-1036 `urlDecode(str)` (in expression position: the string)
/// @pob Modules/Common.lua:1031-1036 h=cc58c3d6
pub fn url_decode(s: &str) -> String {
    // L1032-1034 hexToChar
    // L1035
    pob_lua::gsub_fn(s, "%%(%x%x)", hex_char, None).0
}

/// lua: ImportTab.lua:1913-1924 the globals `HexToChar(x)` and `UrlDecode(url)` (CCR-0109 §7:
/// U43's L594 is their first caller)
/// @pob Classes/ImportTab.lua:1913-1924 h=ddf5f273
#[allow(non_snake_case)] // the Lua global's name
pub fn UrlDecode(url: Option<&str>) -> Option<String> {
    // L1913-1915 HexToChar: `hex_char`
    // L1918
    let url = url?;
    // L1921
    let url = gsub(url, "+", " ");
    // L1922
    let url = pob_lua::gsub_fn(&url, "%%(%x%x)", hex_char, None).0;
    // L1923
    Some(url)
}

// ---- main.* fields that MainState lacks (CCR-0109 §1) ----------------------------------------

/// The global `historyList` (ImportTab.lua:353-362): the sorted account names and the
/// `historyList[accountName] = true` set. The `siteAccountHistory` dropdown's list is the same
/// Lua table; the port keeps the dropdown's `Vec<DropItem>` and this list in step.
#[derive(Clone, Debug, Default)]
pub struct HistoryList {
    pub list: Vec<String>,
    pub set: HashSet<String>,
}

/// `main.api`, `main.lastExportWebsite`, `main.lastCharacterHash` and the global `historyList`.
#[derive(Default)]
pub struct MainExtra {
    pub api: Option<PoEAPI>,
    pub last_export_website: Option<String>,
    pub last_character_hash: Option<String>,
    pub history_list: Option<HistoryList>,
}

thread_local! {
    /// The app it belongs to (its `main.inputEvents` allocation, which lives as long as the app)
    /// and the fields.
    static MAIN_EXTRA: RefCell<Option<(Weak<RefCell<InputEvents>>, MainExtra)>> =
        const { RefCell::new(None) };
}

fn with_main_extra<R>(cx: &Cx, f: impl FnOnce(&mut MainExtra) -> R) -> R {
    let me = Rc::downgrade(&cx.app().main.input_events);
    MAIN_EXTRA.with(|m| {
        let mut m = m.borrow_mut();
        let stale = !m.as_ref().is_some_and(|(w, _)| Weak::ptr_eq(w, &me));
        if stale {
            *m = Some((me, MainExtra::default()));
        }
        f(&mut m.as_mut().expect("set above").1)
    })
}

/// Read [`MainExtra`] (leaf code only: no call that takes `cx`).
pub fn main_extra<R>(cx: &Cx, f: impl FnOnce(&MainExtra) -> R) -> R {
    with_main_extra(cx, |m| f(m))
}

/// Write [`MainExtra`] (leaf code only).
pub fn main_extra_mut<R>(cx: &Cx, f: impl FnOnce(&mut MainExtra) -> R) -> R {
    with_main_extra(cx, f)
}

// ---- seams to U44 and U46 (CCR-0109 §5) -------------------------------------------------------

/// The ImportTab methods of ImportTab.lua:789-1932 (U44) and `CompareTab:ImportBuild` (U46) that
/// U43's controls call. Each default names its owner; owners replace their entry in
/// [`default_import_hooks`] (one line each).
#[derive(Clone)]
pub struct ImportHooks {
    /// `self:BuildCharacterList(realm, league, characters, control)` (ImportTab.lua:1027)
    pub build_character_list:
        Rc<dyn Fn(&mut Cx, CtrlId, &str, Option<&str>, Option<&[Json]>, CtrlId)>,
    /// `self:ImportPassiveTreeAndJewels(charData, deleteJewels)` (:1131)
    pub import_passive_tree_and_jewels: Rc<dyn Fn(&mut Cx, CtrlId, &Json, bool)>,
    /// `self:ImportItemsAndSkills(charData, clearItems, clearSkills, ignoreWeaponSwap)` (:1381)
    pub import_items_and_skills: Rc<dyn Fn(&mut Cx, CtrlId, &Json, bool, bool, bool)>,
    /// `self:DownloadSiteCharacterList(realm)` (:887)
    pub download_site_character_list: Rc<dyn Fn(&mut Cx, CtrlId, &'static Realm)>,
    /// `self:DownloadPassiveTree(realm)` (:814)
    pub download_passive_tree: Rc<dyn Fn(&mut Cx, CtrlId, &'static Realm)>,
    /// `self:DownloadItems(realm)` (:852)
    pub download_items: Rc<dyn Fn(&mut Cx, CtrlId, &'static Realm)>,
    /// `self:SetPredefinedBuildName()` (:1926)
    pub set_predefined_build_name: Rc<dyn Fn(&mut Cx, CtrlId)>,
    /// `self.build.compareTab:ImportBuild(xml, label)` (CompareTab.lua, U46): its result
    pub compare_import_build: Rc<dyn Fn(&mut Cx, Option<&str>, &str) -> bool>,
}

#[track_caller]
fn not_ported(what: &str, owner: &str) -> ! {
    lua_error!("{what} is not ported yet ({owner})")
}

/// The placeholders (each raises a Lua error naming its owner).
pub fn default_import_hooks() -> ImportHooks {
    use super::character as u44;
    ImportHooks {
        // U44 (CCR-0109 §5)
        build_character_list: Rc::new(u44::build_character_list),
        import_passive_tree_and_jewels: Rc::new(u44::import_passive_tree_and_jewels_hook),
        import_items_and_skills: Rc::new(u44::import_items_and_skills_hook),
        download_site_character_list: Rc::new(u44::download_site_character_list),
        download_passive_tree: Rc::new(u44::download_passive_tree),
        download_items: Rc::new(u44::download_items),
        set_predefined_build_name: Rc::new(u44::set_predefined_build_name),
        compare_import_build: Rc::new(|_, _, _| not_ported("CompareTab:ImportBuild", "U46")),
    }
}

thread_local! {
    static IMPORT_HOOKS: RefCell<Option<ImportHooks>> = const { RefCell::new(None) };
}

/// The installed hooks (a clone; entries are `Rc`s).
pub fn import_hooks() -> ImportHooks {
    IMPORT_HOOKS.with(|h| {
        h.borrow_mut()
            .get_or_insert_with(default_import_hooks)
            .clone()
    })
}

/// Replace hook entries (tests, and owners that register at run time).
pub fn set_import_hooks(f: impl FnOnce(&mut ImportHooks)) {
    IMPORT_HOOKS.with(|h| f(h.borrow_mut().get_or_insert_with(default_import_hooks)));
}

// ---- the tab's state -------------------------------------------------------------------------

/// The ImportTab fields of U43's range (U44's methods read and write them too).
#[derive(Default)]
pub struct State {
    /// `self.build`
    pub build: CtrlId,
    // addOAuthControls (L24-321)
    /// `self.usingOauth`
    pub using_oauth: bool,
    /// `self.oauthTimer`: the 60 second timer for oauth (cleared by the status label, L53)
    pub oauth_timer: Cell<Option<f64>>,
    /// `self.rateLimitEndTime`: timestamp for when we can request again after being rate
    /// limited (cleared by the status label, L61)
    pub rate_limit_end_time: Cell<Option<f64>>,
    /// `self.oauthErrCode`
    pub oauth_err_code: Option<String>,
    /// `self.oauthLoading`
    pub oauth_loading: bool,
    /// `self.characterList`: an array of Character for each realm (the character list, so
    /// equipment and passives are missing). LUA-ORDER: string keys, insertion order.
    pub character_list: IndexMap<String, Json>,
    // addAccountNameControls (L322-471)
    /// `self.charImportMode`
    pub char_import_mode: String,
    /// `self.charImportStatus`
    pub char_import_status: String,
    // saveDetails, Load/Save and U44's downloads
    pub last_realm: Option<String>,
    pub last_league: Option<String>,
    pub last_account_hash: Option<String>,
    pub last_character_hash: Option<String>,
    /// `self.importLink` (Load/Save)
    pub import_link: Option<String>,
    /// `self.lastCharList` (written by U44's DownloadSiteCharacterList, read by the
    /// siteCharSelectLeague selFunc, L420)
    pub last_char_list: Option<Vec<Json>>,
    /// `self.autoFetchAttempted`
    pub auto_fetch_attempted: bool,
    // the ctor's build-sharing section (L499-720)
    /// `self.exportWebsiteSelected`
    pub export_website_selected: Option<String>,
    /// `self.importCodeSite` (an index into `buildSites.websiteList`)
    pub import_code_site: Option<usize>,
    /// `self.importCodeDetail`
    pub import_code_detail: Option<String>,
    /// `self.importCodeXML`
    pub import_code_xml: Option<String>,
    /// `self.importCodeValid`
    pub import_code_valid: bool,
    /// `self.importCodeJson`
    pub import_code_json: Option<Json>,
    /// `self.importCodeFetching`
    pub import_code_fetching: bool,
    /// `self.controls.importCodeIn.text` (L599: a field PoB writes on the EditControl and
    /// nothing reads)
    pub import_code_in_text: Option<String>,
}

/// The tab's state (the node must be alive).
pub fn st(cx: &Cx, me: CtrlId) -> &State {
    &cx.w::<ImportTab>(me).core
}

/// The tab's state, mutable.
pub fn st_mut(cx: &mut Cx, me: CtrlId) -> &mut State {
    &mut cx.w_mut::<ImportTab>(me).core
}

/// Whether the tab node still exists (DIVERGENCE(dead-tab), module docs).
pub fn alive(cx: &Cx, me: CtrlId) -> bool {
    cx.ui.try_w::<ImportTab>(me).is_some()
}

/// `self.controls.<key>`
fn ctl(cx: &Cx, me: CtrlId, key: &'static str) -> CtrlId {
    cx.child(me, key)
        .unwrap_or_else(|| lua_error!("attempt to index field '{key}' (a nil value)"))
}

/// `self.controls.<key>.buf`
fn buf(cx: &Cx, me: CtrlId, key: &'static str) -> String {
    cx.w::<EditControl>(ctl(cx, me, key)).buf.clone()
}

/// The export sites (`getExportSitesFromImportList`, L523-532): entries with the fields needed
/// for Export. ORDER: `pairs` over an array (module docs).
fn export_websites_list() -> Vec<&'static WebsiteInfo> {
    // L524
    let mut export_websites = Vec::new();
    // L525
    for v in &WEBSITE_LIST {
        // L526 if entry has fields needed for Export
        // L527
        if v.post_url.is_some() && v.post_fields.is_some() && v.code_out.is_some() {
            // L528
            export_websites.push(v);
        }
    }
    // L531
    export_websites
}

// @pob Classes/ImportTab.lua:1-22 h=e4c84b2c
// L1-14 locals (ipairs, t_insert, t_remove, bit ops, m_max, dkjson)
// L16 `local influenceInfo = itemLib.influenceInfo.all` (U44's)
// L18-22 realmList: `accounts::REALM_LIST`

impl ImportTab {
    /// lua: ImportTab.lua:473-731 `ImportTabClass:ImportTab(build)`
    /// @pob Classes/ImportTab.lua:473-731 h=1bf859c0
    pub fn new(build: CtrlId) -> New<ImportTab> {
        // L473-474 newClass("ImportTab", "ControlHost", "Control"); L478-479 self:ControlHost(),
        // self:Control()
        New::new(
            Base::new(Anchor::NONE, rect(0, 0, 0, 0)),
            ImportTab {
                core: State {
                    // L481
                    build,
                    ..State::default()
                },
                character: super::character::State::default(),
            },
        )
        .host()
        .init(|cx, me| {
            let me = me.id();
            // L483
            if main_extra(cx, |m| m.api.is_none()) {
                // L484
                let (t, r, e) = {
                    let m = &cx.app().main;
                    (m.last_token.clone(), m.last_refresh_token.clone(), m.token_expiry)
                };
                let api = PoEAPI::new(t, r, e);
                main_extra_mut(cx, |m| m.api = Some(api));
            }

            // L488-489
            let section_oauth = cx.add(
                me,
                "sectionOauthCharImport",
                SectionControl::new(
                    Anchor::new(TopLeft, me, TopLeft),
                    rect(10, 18, 650, 200),
                    "Import From Your Account",
                ),
            );

            // L491
            accounts::add_oauth_controls(cx, me);

            // L493-495
            let section_site = cx.add(
                me,
                "sectionCharSiteImport",
                SectionControl::new(
                    Anchor::new(TopLeft, section_oauth, BottomLeft),
                    rect(0, 18, 650, 250),
                    "Import By Account Name",
                ),
            );
            // L496
            accounts::add_account_name_controls(cx, me);

            // L499 Build import/export
            // L500-501
            let section_build = cx.add(
                me,
                "sectionBuild",
                SectionControl::new(
                    Anchor::new(TopLeft, section_site, BottomLeft).collapse(),
                    rect(0, 18, 650, 182),
                    "Build Sharing",
                ),
            );
            // L502-503
            let generate_code_label = cx.add(
                me,
                "generateCodeLabel",
                LabelControl::new(
                    Anchor::new(TopLeft, section_build, TopLeft),
                    rect(6, 14, 0, 16),
                    "^7Generate a code to share this build with other Path of Building users:",
                ),
            );
            // L504-506
            let generate_code = cx.add(
                me,
                "generateCode",
                ButtonControl::new(
                    Anchor::new(Left, generate_code_label, Right),
                    rect(4, 0, 80, 20),
                    "Generate",
                    move |cx| {
                        // L505
                        let xml = build_io::save_db(cx, "code").unwrap_or_else(|| {
                            lua_error!("attempt to index field 'build' (a nil value)")
                        });
                        let code = encode_build_code(&xml);
                        let out = ctl(cx, me, "generateCodeOut");
                        EditControl::set_text(cx, out, &code, true);
                    },
                ),
            );
            // L507-510
            cx.add(
                me,
                "enablePartyExportBuffs",
                CheckBoxControl::new(
                    Anchor::new(Left, generate_code, Right),
                    rect(100, 0, 18, 0),
                    "Export Support",
                    Some(Rc::new(|cx: &mut Cx, state: bool| {
                        let m = &cx.build().model;
                        // L508
                        m.partyTab.enableExportBuffs.set(state);
                        // L509
                        m.buildFlag.set(true);
                    })),
                    Some(Prop::from(
                        "This is for party play, to export support character, it enables the exporting of auras, curses and modifiers to the enemy",
                    )),
                    false,
                ),
            );
            // L511
            let generate_code_out = cx.add(
                me,
                "generateCodeOut",
                EditControl::new(
                    Anchor::new(TopLeft, generate_code_label, BottomLeft),
                    rect(0, 8, 250, 20),
                    EditArgs {
                        init: Some(String::new()),
                        prompt: Some("Code".to_owned()),
                        filter: Some("%Z".to_owned()),
                        ..Default::default()
                    },
                ),
            );
            let out_id = generate_code_out.id();
            // L512-514
            cx.base_mut(out_id).enabled = Prop::f(move |cx, _| !cx.w::<EditControl>(out_id).buf.is_empty());
            // L515-518
            let generate_code_copy = cx.add(
                me,
                "generateCodeCopy",
                ButtonControl::new(
                    Anchor::new(Left, generate_code_out, Right),
                    rect(8, 0, 60, 20),
                    "Copy",
                    move |cx| {
                        // L516
                        let b = cx.w::<EditControl>(out_id).buf.clone();
                        cx.sys.platform.copy(&b);
                        // L517
                        EditControl::set_text(cx, out_id, "", false);
                    },
                ),
            );
            // L519-521
            cx.base_mut(generate_code_copy).enabled =
                Prop::f(move |cx, _| !cx.w::<EditControl>(out_id).buf.is_empty());

            // L523-532 getExportSitesFromImportList
            // L533
            let export_websites_list = Rc::new(export_websites_list());

            // L535-538
            let ewl = export_websites_list.clone();
            let export_from = cx.add(
                me,
                "exportFrom",
                DropDownControl::new(
                    Anchor::new(Left, generate_code_copy, Right),
                    rect(8, 0, 120, 20),
                    ewl.iter()
                        .map(|w| DropItem::new(w.label, Rc::new(*w)))
                        .collect(),
                    Some(Rc::new(move |cx: &mut Cx, _idx: usize, selected: Option<&DropItem>| {
                        let id = selected
                            .and_then(|s| s.get::<&'static WebsiteInfo>())
                            .map(|w| w.id.to_owned())
                            .unwrap_or_else(|| lua_error!("attempt to index local 'selectedWebsite' (a nil value)"));
                        // L536
                        main_extra_mut(cx, |m| m.last_export_website = Some(id.clone()));
                        // L537
                        st_mut(cx, me).export_website_selected = Some(id);
                    })),
                    None,
                    false,
                ),
            );
            // L539
            let want = st(cx, me)
                .export_website_selected
                .clone()
                .or_else(|| main_extra(cx, |m| m.last_export_website.clone()))
                .unwrap_or_else(|| "Pastebin".to_owned());
            DropDownControl::sel_by_value(cx, export_from.id(), &|it| {
                it.get::<&'static WebsiteInfo>().is_some_and(|w| w.id == want)
            });
            // L540-555
            let ewl = export_websites_list.clone();
            let generate_code_by_link = cx.add(
                me,
                "generateCodeByLink",
                ButtonControl::new(
                    Anchor::new(Left, export_from, Right),
                    rect(8, 0, 100, 20),
                    "Share",
                    move |cx| {
                        let export_from = ctl(cx, me, "exportFrom");
                        let link_btn = ctl(cx, me, "generateCodeByLink");
                        // L541
                        let sel = cx.w::<DropDownControl>(export_from).sel_index;
                        let export_website = sel.checked_sub(1).and_then(|i| ewl.get(i)).copied();
                        // L542
                        let code = cx.w::<EditControl>(out_id).buf.clone();
                        let sub_script_id = build_sites::upload_build(cx, &code, export_website);
                        // L543
                        if let Some(sub_script_id) = sub_script_id {
                            let export_website = export_website.expect("upload_build returned an id");
                            // L544
                            EditControl::set_text(cx, out_id, "", false);
                            // L545
                            cx.w_mut::<ButtonControl>(link_btn).label = Prop::from("Creating link...");
                            // L546
                            crate::launch::register_sub_script(
                                cx,
                                Some(sub_script_id),
                                Some(Rc::new(move |cx: &mut Cx, resp: pob_sg::HttpResponse| {
                                    let (paste_link, err_msg) = build_sites::upload_result(&resp);
                                    if !alive(cx, me) {
                                        // DIVERGENCE(dead-tab): the Lua writes into the old
                                        // controls (unreachable) and may open the popup
                                        if let Some(err_msg) = err_msg {
                                            popups::open_message_popup(
                                                cx,
                                                export_website.id,
                                                &format!("Error creating link:\n{err_msg}"),
                                            );
                                        }
                                        return;
                                    }
                                    // L547
                                    cx.w_mut::<ButtonControl>(link_btn).label = Prop::from("Share");
                                    // L548
                                    if let Some(err_msg) = err_msg {
                                        // L549
                                        popups::open_message_popup(
                                            cx,
                                            export_website.id,
                                            &format!("Error creating link:\n{err_msg}"),
                                        );
                                    } else {
                                        // L551
                                        let code_out = export_website.code_out.unwrap_or_else(|| {
                                            lua_error!("attempt to concatenate field 'codeOut' (a nil value)")
                                        });
                                        let text = format!("{code_out}{}", paste_link.unwrap_or_default());
                                        EditControl::set_text(cx, out_id, &text, false);
                                    }
                                })),
                            );
                        }
                    },
                ),
            );
            // L556-563 / L564-571
            let ewl = export_websites_list.clone();
            let share_enabled = move |cx: &Cx| -> bool {
                let b = &cx.w::<EditControl>(out_id).buf;
                // L557 / L565
                for export_site in ewl.iter() {
                    // L558 / L566
                    if !b.is_empty() && lmatch(b, export_site.match_url).is_some() {
                        // L559 / L567
                        return false;
                    }
                }
                // L562 / L570
                !b.is_empty()
            };
            let share_enabled = Rc::new(share_enabled);
            let se = share_enabled.clone();
            cx.base_mut(generate_code_by_link).enabled = Prop::f(move |cx, _| se(cx));
            let se = share_enabled.clone();
            cx.base_mut(export_from).enabled = Prop::f(move |cx, _| se(cx));
            // L572
            let generate_code_note = cx.add(
                me,
                "generateCodeNote",
                LabelControl::new(
                    Anchor::new(TopLeft, generate_code_out, BottomLeft),
                    rect(0, 4, 0, 14),
                    "^7Note: this code can be very long; you can use 'Share' to shrink it.",
                ),
            );
            // L573
            let import_code_header = cx.add(
                me,
                "importCodeHeader",
                LabelControl::new(
                    Anchor::new(TopLeft, generate_code_note, BottomLeft),
                    rect(0, 26, 0, 16),
                    "^7To import a build, enter URL or code here:",
                ),
            );

            // L575-642 importCodeHandle: `ImportTab::import_code_handle`
            // L644-669 importSelectedBuild: `ImportTab::import_selected_build`

            // L671
            let import_code_in = cx.add(
                me,
                "importCodeIn",
                EditControl::new(
                    Anchor::new(TopLeft, import_code_header, BottomLeft),
                    rect(0, 4, 328, 20),
                    EditArgs {
                        init: Some(String::new()),
                        change_func: Some(Rc::new(move |cx: &mut Cx, buf: &str| {
                            ImportTab::import_code_handle(cx, me, buf);
                        })),
                        clearable: true,
                        ..Default::default()
                    },
                ),
            );
            // L672-676
            cx.w_mut::<EditControl>(import_code_in).enter_func = Some(Rc::new(move |cx: &mut Cx, _buf: &str| {
                // L673
                if st(cx, me).import_code_valid {
                    // L674
                    let go = ctl(cx, me, "importCodeGo");
                    let f = cx.w::<ButtonControl>(go).on_click.clone();
                    f(cx);
                }
            }));
            // L677
            let import_code_state = cx.add(
                me,
                "importCodeState",
                LabelControl::new(
                    Anchor::new(Left, import_code_in, Right),
                    rect(8, 0, 0, 16),
                    "",
                ),
            );
            // L678-680
            cx.w_mut::<LabelControl>(import_code_state).label = Prop::f(move |cx, _| {
                st(cx, me).import_code_detail.clone().unwrap_or_default()
            });
            // L681
            let import_code_mode = cx.add(
                me,
                "importCodeMode",
                DropDownControl::new(
                    Anchor::new(TopLeft, import_code_in, BottomLeft),
                    rect(0, 4, 200, 20),
                    vec![
                        DropItem::str("Import to this build"),
                        DropItem::str("Import to a new build"),
                        DropItem::str("Import as comparison"),
                    ],
                    None,
                    None,
                    false,
                ),
            );
            let mode_id = import_code_mode.id();
            // L682-684
            cx.base_mut(mode_id).enabled = Prop::f(move |cx, _| {
                (cx.build().db_file_name.is_some() || cx.w::<DropDownControl>(mode_id).sel_index == 3)
                    && st(cx, me).import_code_valid
            });
            // L685-709
            let import_code_go = cx.add(
                me,
                "importCodeGo",
                ButtonControl::new(
                    Anchor::new(Left, import_code_mode, Right),
                    rect(8, 0, 160, 20),
                    "Import",
                    move |cx| ImportTab::import_code_go(cx, me),
                ),
            );
            let go_id = import_code_go.id();
            // L710-712
            cx.w_mut::<ButtonControl>(go_id).label = Prop::f(move |cx, _| {
                if st(cx, me).import_code_fetching {
                    "Retrieving paste..".to_owned()
                } else {
                    "Import".to_owned()
                }
            });
            // L713-715
            cx.base_mut(go_id).enabled = Prop::f(move |cx, _| {
                let s = st(cx, me);
                s.import_code_valid && !s.import_code_fetching
            });
            // L716-720
            cx.w_mut::<ButtonControl>(go_id).enter_func = Some(Rc::new(move |cx: &mut Cx| {
                // L717
                if st(cx, me).import_code_valid {
                    // L718
                    let f = cx.w::<ButtonControl>(go_id).on_click.clone();
                    f(cx);
                }
            }));

            // L722 validate the status of the api the first time
            // L723
            if poe_api::auth_token(cx).is_some() {
                // L724-728
                poe_api::validate_auth(
                    cx,
                    Rc::new(move |cx: &mut Cx, _valid, err_msg: Option<String>| {
                        // L725
                        if let Some(err_msg) = err_msg {
                            if alive(cx, me) {
                                // L726
                                st_mut(cx, me).oauth_err_code =
                                    Some(format!("Token refresh failed: {err_msg}"));
                            }
                        }
                    }),
                );
            }
            // L730 return self
        })
    }

    /// lua: ImportTab.lua:575-642 the constructor's `importCodeHandle(buf)`
    pub fn import_code_handle(cx: &mut Cx, me: CtrlId, buf: &str) {
        if !alive(cx, me) {
            // DIVERGENCE(dead-tab)
            return;
        }
        {
            let s = st_mut(cx, me);
            // L576
            s.import_code_site = None;
            // L577
            s.import_code_detail = Some(String::new());
            // L578
            s.import_code_xml = None;
            // L579
            s.import_code_valid = false;
            // L580
            s.import_code_json = None;
        }

        // L582
        if buf.is_empty() {
            // L583
            return;
        }

        // L586
        if cx.build().db_file_name.is_none() {
            // L587
            let mode = ctl(cx, me, "importCodeMode");
            cx.w_mut::<DropDownControl>(mode).sel_index = 2;
        }

        // L590
        let negative = cx.main.cc.NEGATIVE.clone();
        st_mut(cx, me).import_code_detail = Some(format!("{negative}Invalid input"));
        // L591 Quick Trim
        let mut url_text = gsub(&gsub(buf, "^[%s?]+", ""), "[%s?]+$", "");
        // L592
        if lmatch(&url_text, "youtube%.com/redirect%?").is_some()
            || lmatch(&url_text, "google%.com/url%?").is_some()
        {
            // L593
            let nested_url = gsub(&url_text, ".*[?&]q=([^&]+).*", "%1");
            // L594
            url_text = UrlDecode(Some(&nested_url)).unwrap_or_default();
        }

        // L597
        for (j, site) in WEBSITE_LIST.iter().enumerate() {
            // L598
            if lmatch(&url_text, site.match_url).is_some() {
                let positive = cx.main.cc.POSITIVE.clone();
                {
                    let s = st_mut(cx, me);
                    // L599
                    s.import_code_in_text = Some(url_text.clone());
                    // L600
                    s.import_code_valid = true;
                    // L601
                    s.import_code_detail = Some(format!("{positive}URL is valid ({})", site.label));
                    // L602
                    s.import_code_site = Some(j + 1);
                }
                // L603
                if buf != url_text {
                    // L604
                    let input = ctl(cx, me, "importCodeIn");
                    EditControl::set_text(cx, input, &url_text, false);
                }
                // L606
                return;
            }
        }

        // L610 If we are in dev mode and the string is a json
        // L611
        if cx.app().launch.dev_mode && lmatch(&url_text, "^%{.*%}$").is_some() {
            // L612
            let decoded = serde_json::from_str::<Json>(&url_text);
            // L613
            let Ok(json_data) = decoded else {
                // L614
                st_mut(cx, me).import_code_detail =
                    Some(format!("{negative}Invalid JSON format (decode error)"));
                // L615
                return;
            };
            if json_data.is_null() {
                lua_error!("attempt to index local 'jsonData' (a nil value)");
            }
            // L617
            let Some(character) = json_get(&json_data, "character").cloned() else {
                // L618
                st_mut(cx, me).import_code_detail =
                    Some(format!("{negative}Invalid JSON format (character missing)"));
                // L619
                return;
            };
            // L621
            let json_data = character;
            // L622
            if json_get(&json_data, "equipment").is_none()
                || json_get(&json_data, "passives").is_none()
            {
                // L623
                st_mut(cx, me).import_code_detail = Some(format!(
                    "{negative}Invalid JSON format (equipment or passives missing)"
                ));
                // L624
                return;
            }
            let positive = cx.main.cc.POSITIVE.clone();
            let s = st_mut(cx, me);
            // L626
            s.import_code_json = Some(json_data);
            // L627
            s.import_code_detail = Some(format!("{positive}JSON is valid"));
            // L628
            s.import_code_valid = true;
            // L629
            return;
        }

        // L632
        let Some(xml_text) = decode_build_code(buf) else {
            // L633-634
            return;
        };
        // L636
        if cx.app().launch.dev_mode && cx.sys.is_key_down(Key::Shift) {
            // L637
            cx.sys.platform.copy(&xml_text);
        }
        let positive = cx.main.cc.POSITIVE.clone();
        let s = st_mut(cx, me);
        // L639
        s.import_code_valid = true;
        // L640
        s.import_code_detail = Some(format!("{positive}Code is valid"));
        // L641
        s.import_code_xml = Some(xml_text);
    }

    /// `self.build.dbFileName` as an argument (`false` and nil kept apart, as `GetArgs` does).
    fn db_file_name_arg(cx: &Cx) -> ModeArg {
        let b = cx.build();
        match &b.db_file_name {
            Some(s) => ModeArg::Str(s.clone()),
            None if b.db_file_name_is_false => ModeArg::Bool(false),
            None => ModeArg::Nil,
        }
    }

    /// `self.importCodeSite and self.controls.importCodeIn.buf or nil`
    fn import_link_arg(cx: &Cx, me: CtrlId) -> ModeArg {
        if st(cx, me).import_code_site.is_some() {
            ModeArg::Str(buf(cx, me, "importCodeIn"))
        } else {
            ModeArg::Nil
        }
    }

    /// `self.build:Shutdown()`, `self.build:Init(...)` (its arguments are evaluated after
    /// Shutdown, as in Lua), `self.build.viewMode = "TREE"`.
    fn reinit_build(cx: &mut Cx, args: impl FnOnce(&Cx) -> Vec<ModeArg>) {
        // self.build:Shutdown()
        let shutdown = build_init::hooks(cx).shutdown;
        shutdown(cx);
        // self.build:Init(...)
        let args = args(cx);
        build_init::init(cx, &args);
        // self.build.viewMode = "TREE"
        if let Some(b) = cx.app().build.as_ref() {
            b.model.viewMode.set(crate::model::Name::from("TREE"));
        }
    }

    /// lua: ImportTab.lua:644-669 the constructor's `importSelectedBuild()`
    pub fn import_selected_build(cx: &mut Cx, me: CtrlId) {
        if !alive(cx, me) {
            // DIVERGENCE(dead-tab)
            return;
        }
        // L645
        if !st(cx, me).import_code_valid || st(cx, me).import_code_fetching {
            // L646
            return;
        }

        let mode = cx
            .w::<DropDownControl>(ctl(cx, me, "importCodeMode"))
            .sel_index;
        // L649
        if mode == 1 {
            // L650
            let warning = cx.main.cc.WARNING.clone();
            popups::open_confirm_popup(
                cx,
                "Build Import",
                &format!("{warning}Warning:^7 Importing to the current build will erase ALL existing data for this build."),
                "Import",
                move |cx| {
                    // L651-653
                    Self::reinit_build(cx, |cx| {
                        vec![
                            Self::db_file_name_arg(cx),
                            cx.build().build_name.clone().map_or(ModeArg::Nil, ModeArg::Str),
                            st(cx, me).import_code_xml.clone().map_or(ModeArg::Nil, ModeArg::Str),
                            ModeArg::Bool(false),
                            Self::import_link_arg(cx, me),
                        ]
                    });
                },
                None,
            );
        // L655
        } else if mode == 3 {
            // L656 Import as comparison build
            // L657 (`self.build.compareTab` always exists in PoB; U46's hook stands for it)
            let xml = st(cx, me).import_code_xml.clone();
            let f = import_hooks().compare_import_build;
            // L658
            if f(cx, xml.as_deref(), "Imported comparison") {
                // L659
                if let Some(b) = cx.app().build.as_ref() {
                    b.model.viewMode.set(crate::model::Name::from("COMPARE"));
                }
            } else {
                // L661
                popups::open_message_popup(
                    cx,
                    "Import Error",
                    "Failed to import build for comparison.",
                );
            }
        } else {
            // L665-667
            Self::reinit_build(cx, |cx| {
                vec![
                    ModeArg::Bool(false),
                    ModeArg::Str("Imported build".to_owned()),
                    st(cx, me)
                        .import_code_xml
                        .clone()
                        .map_or(ModeArg::Nil, ModeArg::Str),
                    ModeArg::Bool(false),
                    Self::import_link_arg(cx, me),
                ]
            });
        }
    }

    /// lua: ImportTab.lua:685-709 the `importCodeGo` button's onClick
    pub fn import_code_go(cx: &mut Cx, me: CtrlId) {
        let (site, has_xml) = {
            let s = st(cx, me);
            (s.import_code_site, s.import_code_xml.is_some())
        };
        // L686
        if let Some(site) = site.filter(|_| !has_xml) {
            // L687
            st_mut(cx, me).import_code_fetching = true;
            // L688
            let selected_website = &WEBSITE_LIST[site - 1];
            // L689
            let link = buf(cx, me, "importCodeIn");
            build_sites::download_build(
                cx,
                &link,
                Some(selected_website),
                Rc::new(
                    move |cx: &mut Cx, is_success: bool, data: String, _url: Option<String>| {
                        if !alive(cx, me) {
                            // DIVERGENCE(dead-tab)
                            return;
                        }
                        // L690
                        st_mut(cx, me).import_code_fetching = false;
                        // L691
                        if !is_success {
                            let negative = cx.main.cc.NEGATIVE.clone();
                            let s = st_mut(cx, me);
                            // L692
                            s.import_code_detail = Some(format!("{negative}{data}"));
                            // L693
                            s.import_code_valid = false;
                        } else {
                            // L695
                            Self::import_code_handle(cx, me, &data);
                            // L696
                            Self::import_selected_build(cx, me);
                        }
                    },
                ),
            );
            // L699
            return;
        }

        // L702
        if let Some(json) = st(cx, me).import_code_json.clone() {
            let h = import_hooks();
            // L703
            (h.import_items_and_skills)(cx, me, &json, true, true, false);
            // L704
            (h.import_passive_tree_and_jewels)(cx, me, &json, true);
            // L705
            return;
        }

        // L708
        Self::import_selected_build(cx, me);
    }

    /// lua: ImportTab.lua:733-740 `ImportTabClass:TryFetchCharacterList()`
    /// @pob Classes/ImportTab.lua:733-740 h=102ebe72
    pub fn try_fetch_character_list(cx: &mut Cx, me: CtrlId) {
        // L733-734 attempt to fetch the last realm's character list once per instance, if there
        // is a last realm saved
        // L736
        if cx.app().main.last_realm.is_some() && !st(cx, me).auto_fetch_attempted {
            // L737
            let b = ctl(cx, me, "accountRealmFetchButton");
            cx.click(b);
            // L738
            st_mut(cx, me).auto_fetch_attempted = true;
        }
    }

    /// lua: ImportTab.lua:742-757 `ImportTabClass:Load(xml, fileName)`
    /// @pob Classes/ImportTab.lua:742-757 h=254a5adc
    pub fn load(cx: &mut Cx, me: CtrlId, xml: &XmlNode, _file_name: Option<&str>) {
        let attr = |k: &str| xml.attrib.get(k).map(|v| v.to_string());
        {
            let s = st_mut(cx, me);
            // L743
            s.last_realm = attr("lastRealm");
            // L744
            s.last_league = attr("lastLeague");
            // L745
            s.last_account_hash = attr("lastAccountHash");
            // L746
            s.import_link = attr("importLink");
        }
        // L747
        let cb = ctl(cx, me, "enablePartyExportBuffs");
        cx.w_mut::<CheckBoxControl>(cb).state = attr("exportParty").as_deref() == Some("true");
        // L748
        let state = cx.w::<CheckBoxControl>(cb).state;
        cx.build().model.partyTab.enableExportBuffs.set(state);
        // L749-755 `if self.lastAccountHash and false then ... end`: never runs (the loop would
        // SetText the account whose common.sha1 matches)
        // L756
        st_mut(cx, me).last_character_hash = attr("lastCharacterHash");
    }

    /// lua: ImportTab.lua:759-774 `ImportTabClass:Save(xml)`
    /// @pob Classes/ImportTab.lua:759-774 h=d1c72059
    pub fn save(cx: &mut Cx, me: CtrlId, xml: &mut XmlNode) {
        let s = st(cx, me);
        let state = cx
            .w::<CheckBoxControl>(ctl(cx, me, "enablePartyExportBuffs"))
            .state;
        // L760-767 (nil fields are absent; the table constructor's order)
        let mut attrib: IndexMap<crate::model::LStr, crate::model::LStr> = IndexMap::new();
        let mut set = |k: &str, v: Option<String>| {
            if let Some(v) = v {
                attrib.insert(k.into(), v.as_str().into());
            }
        };
        set("lastRealm", s.last_realm.clone());
        set("lastLeague", s.last_league.clone());
        set("lastAccountHash", s.last_account_hash.clone());
        set("lastCharacterHash", s.last_character_hash.clone());
        set("exportParty", Some(state.to_string()));
        set("importLink", s.import_link.clone());
        // L769
        if let Some(link) = cx.build().import_link.clone() {
            // L770
            attrib.insert("importLink".into(), link.as_str().into());
        }
        // L772 Gets rid of erroneous, potentially infinitely nested full base64 XML stored as an
        // import link
        // L773
        let keep = attrib.get("importLink").is_some_and(|l| l.len() < 100);
        if !keep {
            attrib.shift_remove("importLink");
        }
        xml.attrib = attrib;
    }

    /// lua: ImportTab.lua:776-787 `ImportTabClass:Draw(viewPort, inputEvents)`
    /// @pob Classes/ImportTab.lua:776-787 h=f960fda2
    pub fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, ev: &mut InputEvents) {
        {
            let b = cx.base_mut(me);
            // L777-780
            b.x = vp.x.into();
            b.y = vp.y.into();
            b.width = vp.width.into();
            b.height = vp.height.into();
        }
        // L782
        cx.process_controls_input(me, ev, vp);
        // L784
        draw_background(cx, vp);
        // L786
        cx.draw_controls(me, vp, None);
    }
}

// ---- the BuildMode seams (CCR-0098 §3, CCR-0104 §1, CCR-0106 §2) -----------------------------

/// `self.importTab` of the open build, when the tab exists.
pub fn tab(cx: &Cx) -> Option<CtrlId> {
    let id = cx.app().build.as_ref()?.tabs.import.id();
    cx.ui.try_w::<ImportTab>(id).map(|_| id)
}

fn tab_or_err(cx: &Cx) -> CtrlId {
    tab(cx).unwrap_or_else(|| lua_error!("attempt to index field 'importTab' (a nil value)"))
}

/// `BuildHooks::import_tab`: `self.importTab = new("ImportTab"):ImportTab(self)` (Build.lua:608).
pub fn ctor_hook(cx: &mut Cx, build: CtrlId) -> CtrlId {
    cx.add_free(Some(build), ImportTab::new(build)).id()
}

/// `BuildHooks::load_ui_section` for `Import`: `self.importTab:Load(node, self.dbFileName)`
/// (Build.lua:665; ImportTab:Load returns nothing, so the build is never closed).
pub fn load_ui_section(cx: &mut Cx, node: &XmlNode, db_file_name: Option<&str>) -> bool {
    let me = tab_or_err(cx);
    ImportTab::load(cx, me, node, db_file_name);
    false
}

/// `ShellHooks::save_ui_section` for `Import` (Build.lua:2289-2290). `None` without the tab.
pub fn save_ui_section(cx: &mut Cx) -> Option<XmlNode> {
    let me = tab(cx)?;
    let mut node = XmlNode {
        elem: "Import".into(),
        ..Default::default()
    };
    ImportTab::save(cx, me, &mut node);
    Some(node)
}

/// `ShellHooks::draw_tab` for `BuildTab::Import`: `self.importTab:Draw(tabViewPort,
/// inputEvents)` (Build.lua:1301).
pub fn draw_tab(cx: &mut Cx, vp: &ViewPort, ev: &mut InputEvents) {
    let me = tab_or_err(cx);
    ImportTab::draw(cx, me, vp, ev);
}

/// `ShellHooks::import_select_code_in`:
/// `self.importTab:SelectControl(self.importTab.controls.importCodeIn)` (Build.lua:1182).
pub fn select_code_in(cx: &mut Cx) {
    let me = tab_or_err(cx);
    let input = ctl(cx, me, "importCodeIn");
    cx.select_control(me, Some(input));
}

/// `BuildHooks::import_try_fetch_character_list`: `self.importTab:TryFetchCharacterList()`.
pub fn try_fetch_character_list_hook(cx: &mut Cx) {
    let me = tab_or_err(cx);
    ImportTab::try_fetch_character_list(cx, me);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_vectors() {
        assert_eq!(sha1_hex(""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(sha1_hex("abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            sha1_hex("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
    }

    #[test]
    fn url_helpers() {
        // values from LuaJIT running Common.lua's / ImportTab.lua's functions
        assert_eq!(url_encode("a b/c~d.e_f-g"), "a%20b%2Fc~d.e_f-g");
        assert_eq!(url_decode("a%20b%2fc+d"), "a b/c+d");
        assert_eq!(UrlDecode(Some("a+b%41%2Gz")).as_deref(), Some("a bA%2Gz"));
        assert_eq!(UrlDecode(None), None);
    }

    #[test]
    fn build_code_round_trip() {
        let xml = "<?xml version=\"1.0\"?><PathOfBuilding><Build level=\"1\"/></PathOfBuilding>";
        let code = encode_build_code(xml);
        assert!(!code.contains('+') && !code.contains('/'));
        assert_eq!(decode_build_code(&code).as_deref(), Some(xml));
        assert_eq!(decode_build_code("not a code"), None);
        // a stream SimpleGraphic's zlib wrote (python zlib.compress(b"hello", 9), base64)
        assert_eq!(
            decode_build_code("eNrLSM3JyQcABiwCFQ==").as_deref(),
            Some("hello")
        );
    }
}
