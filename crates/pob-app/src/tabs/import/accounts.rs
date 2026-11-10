//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ImportTab.lua:1-775 (OAuth/account
//! controls: `realmList`, `addOAuthControls`, `addAccountNameControls`). Owner: U43.
//!
//! @pob-file Classes/ImportTab.lua @16de4b82d
//!
//! The two account sections of the Import tab. The constructor (core.rs) calls
//! [`add_oauth_controls`] and [`add_account_name_controls`]; the closures they install call
//! `main.api` (poe_api.rs) and U44's ImportTab methods through `core::ImportHooks`.
//!
//! Dropdown values: a `realmList` entry is a `DropItem` whose value is `&'static Realm`; a
//! `siteCharSelectLeague` entry (filled by U44's DownloadSiteCharacterList, ImportTab.lua:
//! 975-984) holds a [`LeagueEntry`]; league and history entries of the other dropdowns are
//! strings.
//!
//! Lua semantics:
//! * `self.controls.charSelectLeague.selIndex = nil` (L125) is `sel_index = 0`: the next
//!   `SetSel` compares the new index with nil (always different), as 0 is never a list index.
//! * `historyList` is a global table shared by every ImportTab and aliased by the
//!   `siteAccountHistory` dropdown's `list`; [`super::core::HistoryList`] holds it per app, and
//!   `siteRemoveAccount` removes the entry from both (DIVERGENCE(alias) of DropDownControl).
//! * `self.isAuthorized()` is [`is_authorized`] (`main.api.authToken ~= nil`).
//!
//! Props with side effects (ui doc §7.3): `charImportStatusLabel.label` (L53 `self.oauthTimer =
//! nil`, L61 `self.rateLimitEndTime = nil`): `Cell` fields of `core::State`.
//! ORDER: `pairs(set)` (L116) and `pairs(main.gameAccounts)` (L355) feed lists that are sorted
//! right after by `a:lower() < b:lower()`; only names equal up to case could tie (resolution (c):
//! GGG league names and account names with a discriminator are unique ignoring case).

use std::rc::Rc;

use pob_lua::lfmt;
use pob_ui_core::controls::{
    ButtonControl, CheckBoxControl, DropDownControl, DropItem, EditArgs, EditControl, LabelControl,
};
use pob_ui_core::luastr::lmatch;
use pob_ui_core::popups;
use pob_ui_core::prelude::*;

use super::core::{
    alive, import_hooks, main_extra, main_extra_mut, sha1_hex, st, st_mut, HistoryList,
};
use super::poe_api::{self, json_get, json_truthy, Json};
use crate::app::CxApp;

/// One `realmList` entry (ImportTab.lua:18-22).
#[derive(Debug, PartialEq, Eq)]
pub struct Realm {
    pub label: &'static str,
    pub id: &'static str,
    pub realm_code: &'static str,
    pub host_name: &'static str,
    pub profile_url: &'static str,
}

/// `realmList` (ImportTab.lua:18-22).
pub static REALM_LIST: [Realm; 3] = [
    // L19
    Realm {
        label: "PC",
        id: "PC",
        realm_code: "pc",
        host_name: "https://www.pathofexile.com/",
        profile_url: "account/view-profile/",
    },
    // L20
    Realm {
        label: "Xbox",
        id: "XBOX",
        realm_code: "xbox",
        host_name: "https://www.pathofexile.com/",
        profile_url: "account/view-profile/",
    },
    // L21
    Realm {
        label: "Sony",
        id: "SONY",
        realm_code: "sony",
        host_name: "https://www.pathofexile.com/",
        profile_url: "account/view-profile/",
    },
];

/// A `siteCharSelectLeague` entry's fields besides `label` (`{ label = league, league = league }`
/// or `{ label = "All" }`, ImportTab.lua:976-984).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LeagueEntry {
    pub league: Option<String>,
}

/// The realm dropdown's list (`DropDownControl(.., realmList, ..)`).
fn realm_items() -> Vec<DropItem> {
    REALM_LIST
        .iter()
        .map(|r| DropItem::new(r.label, Rc::new(r)))
        .collect()
}

/// `dropdown:GetSelValue()` of a realm dropdown (nil: an empty list).
pub fn sel_realm(cx: &Cx, dropdown: CtrlId) -> Option<&'static Realm> {
    DropDownControl::get_sel_value(cx, dropdown).and_then(|v| v.get::<&'static Realm>().copied())
}

/// `realm.<field>` of a nil `GetSelValue()` is the Lua index error.
fn realm_or_err(r: Option<&'static Realm>) -> &'static Realm {
    r.unwrap_or_else(|| lua_error!("attempt to index local 'realm' (a nil value)"))
}

/// `self.controls.<key>`
fn ctl(cx: &Cx, me: CtrlId, key: &'static str) -> CtrlId {
    cx.child(me, key)
        .unwrap_or_else(|| lua_error!("attempt to index field '{key}' (a nil value)"))
}

/// `self.isAuthorized()`: `main.api.authToken ~= nil` (L26).
pub fn is_authorized(cx: &Cx) -> bool {
    poe_api::auth_token(cx).is_some()
}

/// `ipairs` over a decoded list (stops at the first nil); a non-table is the Lua error.
pub fn json_list(v: &Json) -> Vec<Json> {
    match v {
        Json::Array(a) => a.iter().take_while(|x| !x.is_null()).cloned().collect(),
        Json::Object(_) => Vec::new(),
        _ => lua_error!("bad argument #1 to 'ipairs' (table expected)"),
    }
}

/// `self.characterList[realmCode]` as a list (nil when absent).
fn characters(cx: &Cx, me: CtrlId, realm_code: &str) -> Option<Vec<Json>> {
    st(cx, me).character_list.get(realm_code).map(json_list)
}

/// lua: ImportTab.lua:43-46 `fetchButtonEnabled()`
fn fetch_button_enabled(cx: &Cx, me: CtrlId) -> bool {
    // L44
    let realm = sel_realm(cx, ctl(cx, me, "accountRealm"));
    // L45
    !realm.is_some_and(|r| json_truthy(st(cx, me).character_list.get(r.realm_code)))
}

/// lua: ImportTab.lua:47-71 `charImportStatus()`
fn char_import_status(cx: &Cx, me: CtrlId) -> String {
    let s = st(cx, me);
    let warning = &cx.main.cc.WARNING;
    // L48
    if !is_authorized(cx) && s.oauth_timer.get().is_none() {
        // L49
        format!("{warning}Not authenticated")
    // L50
    } else if !is_authorized(cx) && s.oauth_timer.get().is_some() {
        // L51
        let time_left = pob_lua::math::m_max(
            0.0,
            (s.oauth_timer.get().unwrap_or(0.0) + 60.0) - poe_api::os_time(cx),
        );
        // L52
        if time_left < 1.0 {
            // L53
            s.oauth_timer.set(None);
            // L54
            return format!("{warning}Not authenticated");
        }
        // L56
        lfmt!("Logging in... (%d) - URL copied to clipboard", time_left)
            + s.oauth_err_code.as_deref().unwrap_or("")
    // L57 user is spam changing realms and is rate limited
    // L58
    } else if is_authorized(cx) && s.rate_limit_end_time.get().is_some() {
        // L59
        let time_left = pob_lua::math::m_max(
            0.0,
            s.rate_limit_end_time.get().unwrap_or(0.0) - poe_api::os_time(cx),
        );
        // L60
        if time_left < 0.5 {
            // L61
            s.rate_limit_end_time.set(None);
            // L62
            return "Authenticated".to_owned();
        }
        // L64
        format!(
            "{warning}{}",
            lfmt!("You're doing that too fast. Please wait (%d)", time_left)
        )
    // L65
    } else if is_authorized(cx) && s.oauth_loading {
        // L66
        if fetch_button_enabled(cx, me) {
            "Fetching...".to_owned()
        } else {
            "Importing...".to_owned()
        }
    // L67
    } else if is_authorized(cx) {
        // L68
        "Authenticated".to_owned()
    } else {
        // L70
        String::new()
    }
}

/// lua: ImportTab.lua:98-136 `setLeaguesFromCharList()`
fn set_leagues_from_char_list(cx: &mut Cx, me: CtrlId) {
    let char_select_league = ctl(cx, me, "charSelectLeague");
    let char_select = ctl(cx, me, "charSelect");
    // L99
    let current_realm = realm_or_err(sel_realm(cx, ctl(cx, me, "accountRealm"))).realm_code;
    // L100
    let current_characters = characters(cx, me, current_realm);
    // L101
    let Some(current_characters) = current_characters.filter(|c| !c.is_empty()) else {
        // L102
        DropDownControl::set_list(cx, char_select_league, Vec::new());
        // L103
        DropDownControl::set_list(cx, char_select, Vec::new());
        // L104
        return;
    };
    // L106 (LUA-ORDER: insertion order; sorted below)
    let mut set: Vec<String> = Vec::new();

    // L108
    for character in &current_characters {
        // L109 the api reference says league is (somehow) not necessarily present
        // L110
        if let Some(league) = json_get(character, "league") {
            let league = match league {
                Json::String(l) => l.clone(),
                Json::Bool(false) => continue,
                _ => lua_error!("attempt to call method 'lower' (a nil value)"),
            };
            // L111
            if !set.contains(&league) {
                set.push(league);
            }
        }
    }
    // L114
    let mut ret = Vec::new();

    // L116
    for key in set {
        // L117
        ret.push(key);
    }
    // L119-121
    pob_lua::sort(&mut ret, |a, b| {
        pob_lua::string::lower(a) < pob_lua::string::lower(b)
    });
    // L122
    ret.push("Any".to_owned());

    // L124
    DropDownControl::set_list(
        cx,
        char_select_league,
        ret.into_iter().map(DropItem::str).collect(),
    );
    // L125
    cx.w_mut::<DropDownControl>(char_select_league).sel_index = 0;
    // L126
    if let Some(last_league) = cx.app().main.last_league.clone() {
        // L127
        let list: Vec<String> = cx
            .w::<DropDownControl>(char_select_league)
            .list
            .iter()
            .map(|v| v.label.clone())
            .collect();
        for (i, v) in list.iter().enumerate() {
            // L128
            if *v == last_league {
                // L129
                DropDownControl::set_sel(cx, char_select_league, i + 1, false);
            }
        }
    }
    // L133
    if cx.w::<DropDownControl>(char_select_league).sel_index == 0 {
        // L134
        DropDownControl::set_sel(cx, char_select_league, 1, false);
    }
}

/// lua: ImportTab.lua:138-162 `fetchCharacters()`
fn fetch_characters(cx: &mut Cx, me: CtrlId) {
    // L139
    if poe_api::auth_token(cx).is_none() {
        return;
    }
    // L140
    let realm = sel_realm(cx, ctl(cx, me, "accountRealm"));
    // L141
    st_mut(cx, me).oauth_loading = true;
    // L142-159
    let on_response: poe_api::DownloadCallback = Rc::new(
        move |cx: &mut Cx, body: Option<Json>, err: Option<String>, time_next: Option<f64>| {
            let tab = alive(cx, me);
            // L143
            match err {
                None => {
                    if !tab {
                        // DIVERGENCE(dead-tab)
                        return;
                    }
                    // L144
                    let body = body.unwrap_or_else(|| {
                        lua_error!("attempt to index local 'body' (a nil value)")
                    });
                    let code = realm_or_err(realm).realm_code.to_owned();
                    match json_get(&body, "characters").cloned() {
                        Some(c) => st_mut(cx, me).character_list.insert(code, c),
                        None => st_mut(cx, me).character_list.shift_remove(&code),
                    };
                    // L145
                    set_leagues_from_char_list(cx, me);
                    let s = st_mut(cx, me);
                    // L146
                    s.oauth_loading = false;
                    // L147
                    s.oauth_err_code = None;
                    // L148
                    return;
                }
                // L149
                Some(err) if err == "Response code: 429" => {
                    // L150
                    if tab {
                        st_mut(cx, me).rate_limit_end_time.set(time_next);
                    }
                }
                // L151 token has been invalidated for some reason
                // L152
                Some(err) if lmatch(&err, "401").is_some() => {
                    // L153
                    if tab {
                        st_mut(cx, me).oauth_err_code =
                            Some("Auth token is invalid. Please login again.".to_owned());
                    }
                    // L154
                    poe_api::reset_details(cx);
                }
                Some(err) => {
                    // L156
                    if tab {
                        st_mut(cx, me).oauth_err_code = Some(err);
                    }
                }
            }
            // L158
            if tab {
                st_mut(cx, me).oauth_loading = false;
            }
        },
    );

    // L161
    let code = realm_or_err(realm).realm_code;
    poe_api::download_character_list(cx, code, on_response);
}

/// lua: ImportTab.lua:239-246 `saveDetails(realmId, league, charName)`
fn save_details(cx: &mut Cx, me: CtrlId, realm_id: &str, league: Option<String>, char_name: &str) {
    // L240
    cx.app_mut().main.last_realm = Some(realm_id.to_owned());
    // L241
    st_mut(cx, me).last_realm = Some(realm_id.to_owned());
    // L242
    cx.app_mut().main.last_league = league.clone();
    // L243
    st_mut(cx, me).last_league = league;
    // L244
    let h = sha1_hex(char_name);
    main_extra_mut(cx, |m| m.last_character_hash = Some(h.clone()));
    // L245
    st_mut(cx, me).last_character_hash = Some(sha1_hex(char_name));
}

/// `self.controls.charSelect:GetSelValue().label` (nil is the Lua index error).
fn selected_char_name(cx: &Cx, me: CtrlId, key: &'static str) -> String {
    DropDownControl::get_sel_value(cx, ctl(cx, me, key))
        .map(|v| v.label)
        .unwrap_or_else(|| lua_error!("attempt to index a nil value"))
}

/// `self.controls.charSelectLeague:GetSelValue()` (a league name, or nil).
fn selected_league(cx: &Cx, me: CtrlId) -> Option<String> {
    DropDownControl::get_sel_value(cx, ctl(cx, me, "charSelectLeague"))
        .map(|v| v.as_str().map_or_else(|| v.label.clone(), str::to_owned))
}

/// `data and data.character` / the error message of the character download callbacks.
fn character_or_error(
    cx: &mut Cx,
    me: CtrlId,
    data: Option<&Json>,
    err_msg: Option<String>,
) -> Option<Json> {
    // L258 / L299
    if let Some(c) = data
        .and_then(|d| json_get(d, "character"))
        .filter(|c| json_truthy(Some(c)))
    {
        // L259 / L300
        st_mut(cx, me).oauth_err_code = None;
        Some(c.clone())
    } else {
        // L262 / L303
        st_mut(cx, me).oauth_err_code = Some(match err_msg {
            // L263 / L304
            Some(e) => format!("Could not import: {e}"),
            // L265 / L306
            None => "Could not import character".to_owned(),
        });
        None
    }
}

/// `self.usingOauth and self.isAuthorized() and self.controls.charSelect:GetSelValue()`
fn import_enabled(cx: &Cx, me: CtrlId) -> bool {
    st(cx, me).using_oauth
        && is_authorized(cx)
        && DropDownControl::get_sel_value(cx, ctl(cx, me, "charSelect")).is_some()
}

/// `self.build.spec:CountAllocNodes() > 0`
fn has_alloc_nodes(cx: &Cx) -> bool {
    cx.build().model.spec.get().CountAllocNodes().0 > 0.0
}

/// lua: ImportTab.lua:24-321 `local function addOAuthControls(self)`
/// @pob Classes/ImportTab.lua:24-321 h=82973cae
pub fn add_oauth_controls(cx: &mut Cx, me: CtrlId) {
    {
        let s = st_mut(cx, me);
        // L25
        s.using_oauth = true;
        // L26 self.isAuthorized: `is_authorized`
        // L27-28 the 60 second timer for oauth
        // L29
        s.oauth_timer.set(None);
        // L30-31 timestamp for when we can request again after being rate limited
        // L32
        s.rate_limit_end_time.set(None);
        // L34
        s.oauth_err_code = None;
        // L36
        s.oauth_loading = false;
        // L37-39 an array of Character for each realm. Note this is for the character
        // list, which will mean that equipment and passives are missing
        // https://www.pathofexile.com/developer/docs/reference#type-Character
        // L41
        s.character_list.clear();
    }

    // L43-46 fetchButtonEnabled; L47-71 charImportStatus

    // L73 space after labels
    // L74
    let label_spacing = 6;
    // L75 space between rows
    // L76
    let row_spacing = 6;

    let section = ctl(cx, me, "sectionOauthCharImport");
    // L79-82
    let char_import_status_label = cx.add(
        me,
        "charImportStatusLabel",
        LabelControl::new(
            Anchor::new(TopLeft, section, TopLeft),
            rect(label_spacing, 14, 200, 16),
            Prop::f(move |cx, _| {
                format!("^7Character import status: {}", char_import_status(cx, me))
            }),
        ),
    );

    // L84-88
    let logout = cx.add(
        me,
        "logoutApiButton",
        ButtonControl::new(
            Anchor::new(TopLeft, char_import_status_label, TopRight),
            rect(label_spacing, 0, 170, 16),
            "^7Logout from Path of Exile API",
            |cx| {
                // L86
                poe_api::reset_details(cx);
                // L87
                crate::main_mode::settings::save_settings(cx);
            },
        ),
    );
    // L89
    cx.base_mut(logout).shown = Prop::f(move |cx, _| st(cx, me).using_oauth && is_authorized(cx));

    // L91-92
    let character_import_anchor = cx.add(
        me,
        "characterImportAnchor",
        PlainControl::new(
            Anchor::new(TopLeft, section, TopLeft),
            rect(label_spacing, 40, 200, 16),
        ),
    );
    // L93-95
    cx.base_mut(section).height = Prop::f(|cx, _| if is_authorized(cx) { 200.0 } else { 60.0 });

    // L97 realm select
    // L98-136 setLeaguesFromCharList; L138-162 fetchCharacters

    // L164-180
    let authenticate = cx.add(
        me,
        "authenticateButton",
        ButtonControl::new(
            Anchor::new(TopLeft, character_import_anchor, TopLeft),
            rect(0, 0, 200, 16),
            "^7Authorize with Path of Exile",
            move |cx| {
                // L166
                poe_api::fetch_auth_token(
                    cx,
                    Rc::new(move |cx: &mut Cx, err_code: Option<String>| {
                        if !alive(cx, me) {
                            // DIVERGENCE(dead-tab)
                            return;
                        }
                        // L167
                        if let Some(err_code) = err_code {
                            let s = st_mut(cx, me);
                            // L168
                            s.oauth_err_code = Some(err_code);
                            // L169
                            s.oauth_timer.set(None);
                        } else {
                            let s = st_mut(cx, me);
                            // L171
                            s.oauth_err_code = None;
                            // L172
                            s.oauth_timer.set(None);
                            // L173-175 successful login -> fetch. this will default to PC, but
                            // if the user has ever imported before, it should default
                            // to that last realm
                            // L176
                            fetch_characters(cx, me);
                        }
                    }),
                );
                // L179
                let t = poe_api::os_time(cx);
                st(cx, me).oauth_timer.set(Some(t));
            },
        ),
    );
    // L181-183
    cx.base_mut(authenticate).shown =
        Prop::f(move |cx, _| st(cx, me).using_oauth && !is_authorized(cx));

    // L185 Stage: select realm, league, character, and import data
    // L186-187
    let char_select_header = cx.add(
        me,
        "charSelectHeader",
        LabelControl::new(
            Anchor::new(TopLeft, section, TopLeft),
            rect(label_spacing, 40, 200, 16),
            "^7Choose character to import data from:",
        ),
    );
    // L188-190
    cx.base_mut(char_select_header).shown =
        Prop::f(move |cx, _| st(cx, me).using_oauth && is_authorized(cx));

    // L192-193
    let oauth_error_label = cx.add(
        me,
        "oauthErrorLabel",
        LabelControl::new(
            Anchor::new(TopRight, section, TopRight),
            rect(-8, 40, 0, 18),
            "",
        ),
    );
    // L194-197
    cx.w_mut::<LabelControl>(oauth_error_label).label = Prop::f(move |cx, _| {
        // L195
        match &st(cx, me).oauth_err_code {
            Some(e) => lfmt!("%sError: %s", cx.main.cc.NEGATIVE.as_str(), e.as_str()),
            None => String::new(),
        }
    });

    // L199-202
    let account_realm = cx.add(
        me,
        "accountRealm",
        DropDownControl::new(
            Anchor::new(TopLeft, char_select_header, BottomLeft),
            rect(0, row_spacing, 60, 20),
            realm_items(),
            Some(Rc::new(
                move |cx: &mut Cx, _i: usize, _v: Option<&DropItem>| {
                    // L201
                    set_leagues_from_char_list(cx, me);
                },
            )),
            None,
            false,
        ),
    );
    // L203
    let last_realm = cx
        .app()
        .main
        .last_realm
        .clone()
        .unwrap_or_else(|| "PC".to_owned());
    DropDownControl::sel_by_value(cx, account_realm.id(), &|it| {
        it.get::<&'static Realm>()
            .is_some_and(|r| r.id == last_realm)
    });

    // L205-211 fetchTextFunc
    // L212-213
    let fetch_button = cx.add(
        me,
        "accountRealmFetchButton",
        ButtonControl::new(
            Anchor::new(Left, account_realm, Right),
            rect(label_spacing, 0, 130, 20),
            Prop::f(move |cx, _| {
                // L206
                let realm = sel_realm(cx, ctl(cx, me, "accountRealm"));
                // L207
                if realm.is_some_and(|r| json_truthy(st(cx, me).character_list.get(r.realm_code))) {
                    // L208
                    return "Fetched".to_owned();
                }
                // L210
                "Fetch Characters".to_owned()
            }),
            move |cx| fetch_characters(cx, me),
        ),
    );
    // L214
    cx.base_mut(fetch_button).enabled = Prop::f(move |cx, _| fetch_button_enabled(cx, me));

    // L216 league select
    // L217-225 onLeagueChange(_, newLeague)
    let on_league_change = move |cx: &mut Cx, _i: usize, new_league: Option<&DropItem>| {
        // L219
        let realm = realm_or_err(sel_realm(cx, ctl(cx, me, "accountRealm"))).realm_code;
        let chars = characters(cx, me, realm);
        let char_select = ctl(cx, me, "charSelect");
        let f = import_hooks().build_character_list;
        let new_league = new_league.map(|v| v.label.clone());
        // L220
        if new_league.as_deref() == Some("Any") {
            // L221
            f(cx, me, realm, None, chars.as_deref(), char_select);
        } else {
            // L223
            f(
                cx,
                me,
                realm,
                new_league.as_deref(),
                chars.as_deref(),
                char_select,
            );
        }
    };

    // L227-228
    let char_select_league_label = cx.add(
        me,
        "charSelectLeagueLabel",
        LabelControl::new(
            Anchor::new(TopLeft, account_realm, BottomLeft),
            rect(0, row_spacing, 0, 14),
            "^7League:",
        ),
    );
    // L229-230
    cx.add(
        me,
        "charSelectLeague",
        DropDownControl::new(
            Anchor::new(Left, char_select_league_label, Right),
            rect(label_spacing, 0, 150, 18),
            Vec::new(),
            Some(Rc::new(on_league_change)),
            None,
            false,
        ),
    );
    // L231 character select
    // L232-233
    let char_select = cx.add(
        me,
        "charSelect",
        DropDownControl::new(
            Anchor::new(TopLeft, char_select_league_label, BottomLeft),
            rect(0, row_spacing, 400, 18),
            Vec::new(),
            None,
            None,
            false,
        ),
    );
    // L234-236
    cx.base_mut(char_select).enabled =
        Prop::f(move |cx, _| st(cx, me).using_oauth && is_authorized(cx));

    // L238 import action controls
    // L239-246 saveDetails
    // L247-248
    let char_import_header = cx.add(
        me,
        "charImportHeader",
        LabelControl::new(
            Anchor::new(TopLeft, char_select, BottomLeft),
            rect(0, row_spacing, 200, 16),
            "^7Import:",
        ),
    );
    // L249-280
    let char_import_tree = cx.add(
        me,
        "charImportTree",
        ButtonControl::new(
            Anchor::new(Left, char_import_header, Right),
            rect(label_spacing, 0, 170, 20),
            "Passive Tree and Jewels",
            move |cx| {
                // L251
                let realm = realm_or_err(sel_realm(cx, ctl(cx, me, "accountRealm")));
                // L252
                let league = selected_league(cx, me);
                // L253
                let selected_name = selected_char_name(cx, me, "charSelect");

                // L255
                save_details(cx, me, realm.id, league, &selected_name);
                // L256
                let delete_jewels = cx
                    .w::<CheckBoxControl>(ctl(cx, me, "charImportTreeClearJewels"))
                    .state;
                // L257-269 importHandler(data, errMsg)
                let import_handler: poe_api::DownloadCallback = Rc::new(
                    move |cx: &mut Cx,
                          data: Option<Json>,
                          err_msg: Option<String>,
                          _t: Option<f64>| {
                        if !alive(cx, me) {
                            // DIVERGENCE(dead-tab)
                            return;
                        }
                        if let Some(c) = character_or_error(cx, me, data.as_ref(), err_msg) {
                            // L260
                            let f = import_hooks().import_passive_tree_and_jewels;
                            f(cx, me, &c, delete_jewels);
                        }
                        // L268
                        st_mut(cx, me).oauth_loading = false;
                    },
                );
                // L270
                if has_alloc_nodes(cx) {
                    let selected_name = selected_name.clone();
                    // L271-275
                    popups::open_confirm_popup(
                        cx,
                        "Character Import",
                        "Importing the passive tree will overwrite your current tree.",
                        "Import",
                        move |cx| {
                            // L273
                            st_mut(cx, me).oauth_loading = true;
                            // L274
                            poe_api::download_character(
                                cx,
                                realm.realm_code,
                                &selected_name,
                                import_handler.clone(),
                            );
                        },
                        None,
                    );
                } else {
                    // L277
                    st_mut(cx, me).oauth_loading = true;
                    // L278
                    poe_api::download_character(
                        cx,
                        realm.realm_code,
                        &selected_name,
                        import_handler,
                    );
                }
            },
        ),
    );
    // L281-283
    cx.base_mut(char_import_tree).enabled = Prop::f(move |cx, _| import_enabled(cx, me));
    // L284-285
    cx.add(
        me,
        "charImportTreeClearJewels",
        CheckBoxControl::new(
            Anchor::new(Left, char_import_tree, Right),
            rect(90, 0, 18, 0),
            "Delete jewels:",
            None,
            Some(Prop::from("Delete all equipped jewels when importing.")),
            true,
        ),
    );
    // L286-311
    let char_import_items = cx.add(
        me,
        "charImportItems",
        ButtonControl::new(
            Anchor::new(TopLeft, char_import_tree, BottomLeft),
            rect(0, row_spacing, 110, 20),
            "Items and Skills",
            move |cx| {
                // L288
                let realm = realm_or_err(sel_realm(cx, ctl(cx, me, "accountRealm")));
                // L289
                let league = selected_league(cx, me);
                // L290
                let selected_name = selected_char_name(cx, me, "charSelect");

                // L292
                save_details(cx, me, realm.id, league, &selected_name);

                // L294
                st_mut(cx, me).oauth_loading = true;
                // L295
                poe_api::download_character(
                    cx,
                    realm.realm_code,
                    &selected_name,
                    Rc::new(
                        move |cx: &mut Cx, data: Option<Json>, err_msg: Option<String>, _t| {
                            if !alive(cx, me) {
                                // DIVERGENCE(dead-tab)
                                return;
                            }
                            let state = |cx: &Cx, k: &'static str| {
                                cx.w::<CheckBoxControl>(ctl(cx, me, k)).state
                            };
                            // L296
                            let clear_items = state(cx, "charImportItemsClearItems");
                            // L297
                            let clear_skills = state(cx, "charImportItemsClearSkills");
                            // L298
                            let ignore_weapon_swap = state(cx, "charImportItemsIgnoreWeaponSwap");
                            // L299
                            if let Some(c) = character_or_error(cx, me, data.as_ref(), err_msg) {
                                // L301
                                let f = import_hooks().import_items_and_skills;
                                f(cx, me, &c, clear_items, clear_skills, ignore_weapon_swap);
                            }
                            // L309
                            st_mut(cx, me).oauth_loading = false;
                        },
                    ),
                );
            },
        ),
    );
    // L312-314
    cx.base_mut(char_import_items).enabled = Prop::f(move |cx, _| import_enabled(cx, me));
    // L315-316
    cx.add(
        me,
        "charImportItemsClearSkills",
        CheckBoxControl::new(
            Anchor::new(Left, char_import_items, Right),
            rect(85, 0, 18, 0),
            "Delete skills:",
            None,
            Some(Prop::from("Delete all existing skills when importing.")),
            true,
        ),
    );
    // L317-318
    cx.add(
        me,
        "charImportItemsClearItems",
        CheckBoxControl::new(
            Anchor::new(Left, char_import_items, Right),
            rect(220, 0, 18, 0),
            "Delete equipment:",
            None,
            Some(Prop::from("Delete all equipped items when importing.")),
            true,
        ),
    );
    // L319-320
    cx.add(
        me,
        "charImportItemsIgnoreWeaponSwap",
        CheckBoxControl::new(
            Anchor::new(Left, char_import_items, Right),
            rect(380, 0, 18, 0),
            "Ignore weapon swap:",
            None,
            Some(Prop::from("Ignore items and skills in weapon swap.")),
            false,
        ),
    );
}

/// `buf:match(p)` is truthy.
fn buf_matches(cx: &Cx, edit: CtrlId, p: &str) -> bool {
    lmatch(&cx.w::<EditControl>(edit).buf, p).is_some()
}

/// lua: ImportTab.lua:322-471 `local function addAccountNameControls(self)`
/// @pob Classes/ImportTab.lua:322-471 h=a36c57c5
pub fn add_account_name_controls(cx: &mut Cx, me: CtrlId) {
    {
        let s = st_mut(cx, me);
        // L323
        s.char_import_mode = "GETACCOUNTNAME".to_owned();
        // L324
        s.char_import_status = "Idle".to_owned();
    }
    let section = ctl(cx, me, "sectionCharSiteImport");
    // L325-328
    cx.add(
        me,
        "siteCharImportStatusLabel",
        LabelControl::new(
            Anchor::new(TopLeft, section, TopLeft),
            rect(6, 14, 200, 16),
            Prop::f(move |cx, _| {
                format!(
                    "^7Character import status: {}",
                    st(cx, me).char_import_status
                )
            }),
        ),
    );

    // L330 Stage: input account name
    // L331-332
    let site_account_name_header = cx.add(
        me,
        "siteAccountNameHeader",
        LabelControl::new(
            Anchor::new(TopLeft, section, TopLeft),
            rect(6, 40, 250, 16),
            "^7To start importing a character, enter the character's account name:",
        ),
    );
    // L333-335
    cx.base_mut(site_account_name_header).shown =
        Prop::f(move |cx, _| st(cx, me).char_import_mode == "GETACCOUNTNAME");

    // L337-338
    let site_account_realm = cx.add(
        me,
        "siteAccountRealm",
        DropDownControl::new(
            Anchor::new(TopLeft, site_account_name_header, BottomLeft),
            rect(0, 4, 60, 20),
            realm_items(),
            None,
            None,
            false,
        ),
    );
    // L339
    let last_realm = cx
        .app()
        .main
        .last_realm
        .clone()
        .unwrap_or_else(|| "PC".to_owned());
    DropDownControl::sel_by_value(cx, site_account_realm.id(), &|it| {
        it.get::<&'static Realm>()
            .is_some_and(|r| r.id == last_realm)
    });
    // L340-341
    let site_account_name = cx.add(
        me,
        "siteAccountName",
        EditControl::new(
            Anchor::new(Left, site_account_realm, Right),
            rect(8, 0, 200, 20),
            EditArgs {
                init: Some(cx.app().main.last_account_name.clone().unwrap_or_default()),
                filter: Some("%c".to_owned()),
                clearable: true,
                ..Default::default()
            },
        ),
    );
    let name_id = site_account_name.id();
    // L342-351
    cx.w_mut::<EditControl>(name_id).paste_filter = Some(Rc::new(|_cx: &mut Cx, text: &str| {
        // L343-350 text:gsub(".", function(c) ... end): bytes >= 128 become "%XX"
        let mut out = String::with_capacity(text.len());
        for &byte in text.as_bytes() {
            // L345
            if byte >= 128 {
                // L346
                out.push_str(&lfmt!("%%%02X", f64::from(byte)));
            } else {
                // L348
                out.push(char::from(byte));
            }
        }
        out
    }));
    // L352 accountHistory Control
    // L353
    if main_extra(cx, |m| m.history_list.is_none()) {
        // L354
        let mut history = HistoryList::default();
        // L355 LUA-ORDER: pairs(main.gameAccounts), sorted below
        let names: Vec<String> = cx.app().main.game_accounts.keys().cloned().collect();
        for account_name in names {
            // L356
            history.list.push(account_name.clone());
            // L357
            history.set.insert(account_name);
        }
        // L359-361
        pob_lua::sort(&mut history.list, |a, b| {
            pob_lua::string::lower(a) < pob_lua::string::lower(b)
        });
        main_extra_mut(cx, |m| m.history_list = Some(history));
    } // L362 don't load the list many times
      // L363-367
    let go = cx.add(
        me,
        "siteAccountNameGo",
        ButtonControl::new(
            Anchor::new(Left, site_account_name, Right),
            rect(8, 0, 60, 20),
            "Start",
            move |cx| {
                // L365
                let realm = realm_or_err(sel_realm(cx, ctl(cx, me, "siteAccountRealm")));
                // L366
                let f = import_hooks().download_site_character_list;
                f(cx, me, realm);
            },
        ),
    );
    // L368-370
    cx.base_mut(go).enabled = Prop::f(move |cx, _| buf_matches(cx, name_id, "%S[#%-]%d%d%d%d$"));
    // L371-376
    cx.w_mut::<ButtonControl>(go).tt.tooltip_func =
        Some(Rc::new(move |cx: &mut Cx, tooltip, ()| {
            // L372
            tooltip.clear(false);
            let b = cx.w::<EditControl>(name_id).buf.clone();
            // L373
            if lmatch(&b, "[#%-]%d%d%d%d$").is_none() && !b.is_empty() {
                // L374
                tooltip.add_line(cx, 16.0, &format!("^7Missing discriminator e.g. {b}#1234"));
            }
        }));

    // L378-381
    let history: Vec<DropItem> = main_extra(cx, |m| {
        m.history_list
            .as_ref()
            .map(|h| h.list.iter().cloned().map(DropItem::str).collect())
            .unwrap_or_default()
    });
    let site_account_history = cx.add(
        me,
        "siteAccountHistory",
        DropDownControl::new(
            Anchor::new(Left, go, Right),
            rect(8, 0, 200, 20),
            history,
            Some(Rc::new(
                move |cx: &mut Cx, _i: usize, _v: Option<&DropItem>| {
                    // L380
                    let hist = ctl(cx, me, "siteAccountHistory");
                    let w = cx.w::<DropDownControl>(hist);
                    let Some(v) = w.sel_index.checked_sub(1).and_then(|i| w.list.get(i)) else {
                        // unreachable: SetSel never selects past the end of the list
                        return;
                    };
                    let v = v.label.clone();
                    cx.w_mut::<EditControl>(name_id).buf = v;
                },
            )),
            None,
            false,
        ),
    );
    let hist_id = site_account_history.id();
    // L382
    let last_account_name = cx.app().main.last_account_name.clone();
    DropDownControl::sel_by_value(cx, hist_id, &|it| {
        last_account_name.as_deref() == it.as_str()
    });
    // L383
    DropDownControl::check_dropped_width(cx, hist_id, true);

    // L385-393
    let site_remove_account = cx.add(
        me,
        "siteRemoveAccount",
        ButtonControl::new(
            Anchor::new(Left, site_account_history, Right),
            rect(8, 0, 20, 20),
            "X",
            move |cx| {
                // L387
                let (sel, account_name) = {
                    let w = cx.w::<DropDownControl>(hist_id);
                    (
                        w.sel_index,
                        w.sel_index
                            .checked_sub(1)
                            .and_then(|i| w.list.get(i))
                            .map(|v| v.label.clone()),
                    )
                };
                // L388
                if let Some(account_name) = account_name {
                    // L389 t_remove(self.controls.siteAccountHistory.list, selIndex) (the list is
                    // the global historyList)
                    cx.w_mut::<DropDownControl>(hist_id).list.remove(sel - 1);
                    main_extra_mut(cx, |m| {
                        if let Some(h) = m.history_list.as_mut() {
                            if sel <= h.list.len() {
                                h.list.remove(sel - 1);
                            }
                            // L390
                            h.set.remove(&account_name);
                        }
                    });
                    // L391
                    cx.app_mut().main.game_accounts.shift_remove(&account_name);
                }
            },
        ),
    );

    // L395-398
    cx.w_mut::<ButtonControl>(site_remove_account)
        .tt
        .tooltip_func = Some(Rc::new(|cx: &mut Cx, tooltip, ()| {
        // L396
        tooltip.clear(false);
        // L397
        tooltip.add_line(cx, 16.0, "^7Removes account from the dropdown list");
    }));

    // L400
    let missing = cx.add(
        me,
        "siteAccountNameMissingDiscriminator",
        LabelControl::new(
            Anchor::new(TopLeft, site_account_name, BottomLeft),
            rect(0, 8, 0, 16),
            "^1Missing discriminator e.g. #1234",
        ),
    );
    // L401-403
    cx.base_mut(missing).shown = Prop::f(move |cx, _| {
        !buf_matches(cx, name_id, "[#%-]%d%d%d%d$") && !cx.w::<EditControl>(name_id).buf.is_empty()
    });

    // L405-407
    cx.add(
        me,
        "siteAccountNameUnicode",
        LabelControl::new(
            Anchor::new(TopLeft, site_account_realm, BottomLeft),
            rect(0, 34, 0, 14),
            "^7Note: if the account name contains non-ASCII characters it must be pasted into the textbox,\nnot typed manually.",
        ),
    );

    // L409 Stage: select character and import data
    // L410-411
    let site_char_select_header = cx.add(
        me,
        "siteCharSelectHeader",
        LabelControl::new(
            Anchor::new(TopLeft, section, TopLeft),
            rect(6, 40, 200, 16),
            "^7Choose character to import data from:",
        ),
    );
    // L412-414
    cx.base_mut(site_char_select_header).shown = Prop::f(move |cx, _| {
        let m = &st(cx, me).char_import_mode;
        m == "SELECTCHAR" || m == "IMPORTING"
    });
    // L415-416
    let site_char_select_league_label = cx.add(
        me,
        "siteCharSelectLeagueLabel",
        LabelControl::new(
            Anchor::new(TopLeft, site_char_select_header, BottomLeft),
            rect(0, 6, 0, 14),
            "^7League:",
        ),
    );
    // L417-421
    cx.add(
        me,
        "siteCharSelectLeague",
        DropDownControl::new(
            Anchor::new(Left, site_char_select_league_label, Right),
            rect(4, 0, 150, 18),
            Vec::new(),
            Some(Rc::new(
                move |cx: &mut Cx, _index: usize, value: Option<&DropItem>| {
                    // L419
                    let realm = realm_or_err(sel_realm(cx, ctl(cx, me, "siteAccountRealm")));
                    // L420
                    let value = value.unwrap_or_else(|| {
                        lua_error!("attempt to index local 'value' (a nil value)")
                    });
                    let league = value.get::<LeagueEntry>().and_then(|e| e.league.clone());
                    let chars = st(cx, me).last_char_list.clone();
                    let site_char_select = ctl(cx, me, "siteCharSelect");
                    let f = import_hooks().build_character_list;
                    f(
                        cx,
                        me,
                        realm.realm_code,
                        league.as_deref(),
                        chars.as_deref(),
                        site_char_select,
                    );
                },
            )),
            None,
            false,
        ),
    );
    // L422-423
    let site_char_select = cx.add(
        me,
        "siteCharSelect",
        DropDownControl::new(
            Anchor::new(TopLeft, site_char_select_header, BottomLeft),
            rect(0, 24, 400, 18),
            Vec::new(),
            None,
            None,
            false,
        ),
    );
    let select_char_mode = move |cx: &Cx, _: CtrlId| st(cx, me).char_import_mode == "SELECTCHAR";
    // L424-426
    cx.base_mut(site_char_select).enabled = Prop::f(select_char_mode);
    // L427-428
    let site_char_import_header = cx.add(
        me,
        "siteCharImportHeader",
        LabelControl::new(
            Anchor::new(TopLeft, site_char_select, BottomLeft),
            rect(0, 16, 200, 16),
            "^7Import:",
        ),
    );
    // L429-441
    let site_char_import_tree = cx.add(
        me,
        "siteCharImportTree",
        ButtonControl::new(
            Anchor::new(Left, site_char_import_header, Right),
            rect(8, 0, 170, 20),
            "Passive Tree and Jewels",
            move |cx| {
                // L431
                let realm = realm_or_err(sel_realm(cx, ctl(cx, me, "siteAccountRealm")));
                // L432
                if has_alloc_nodes(cx) {
                    // L433-436
                    popups::open_confirm_popup(
                        cx,
                        "Character Import",
                        "Importing the passive tree will overwrite your current tree.",
                        "Import",
                        move |cx| {
                            // L435
                            let f = import_hooks().download_passive_tree;
                            f(cx, me, realm);
                        },
                        None,
                    );
                } else {
                    // L438
                    let f = import_hooks().download_passive_tree;
                    f(cx, me, realm);
                }
                // L440
                let f = import_hooks().set_predefined_build_name;
                f(cx, me);
            },
        ),
    );
    // L442-444
    cx.base_mut(site_char_import_tree).enabled = Prop::f(select_char_mode);
    // L445-446
    cx.add(
        me,
        "siteCharImportTreeClearJewels",
        CheckBoxControl::new(
            Anchor::new(Left, site_char_import_tree, Right),
            rect(90, 0, 18, 0),
            "Delete jewels:",
            None,
            Some(Prop::from("Delete all equipped jewels when importing.")),
            true,
        ),
    );
    // L447-452
    let site_char_import_items = cx.add(
        me,
        "siteCharImportItems",
        ButtonControl::new(
            Anchor::new(Left, site_char_import_tree, Left),
            rect(0, 36, 110, 20),
            "Items and Skills",
            move |cx| {
                // L449
                let realm = realm_or_err(sel_realm(cx, ctl(cx, me, "siteAccountRealm")));
                // L450
                let f = import_hooks().download_items;
                f(cx, me, realm);
                // L451
                let f = import_hooks().set_predefined_build_name;
                f(cx, me);
            },
        ),
    );
    // L453-455
    cx.base_mut(site_char_import_items).enabled = Prop::f(select_char_mode);
    // L456-457
    cx.add(
        me,
        "siteCharImportItemsClearSkills",
        CheckBoxControl::new(
            Anchor::new(Left, site_char_import_items, Right),
            rect(85, 0, 18, 0),
            "Delete skills:",
            None,
            Some(Prop::from("Delete all existing skills when importing.")),
            true,
        ),
    );
    // L458-459
    cx.add(
        me,
        "siteCharImportItemsClearItems",
        CheckBoxControl::new(
            Anchor::new(Left, site_char_import_items, Right),
            rect(220, 0, 18, 0),
            "Delete equipment:",
            None,
            Some(Prop::from("Delete all equipped items when importing.")),
            true,
        ),
    );
    // L460-461
    cx.add(
        me,
        "siteCharImportItemsIgnoreWeaponSwap",
        CheckBoxControl::new(
            Anchor::new(Left, site_char_import_items, Right),
            rect(380, 0, 18, 0),
            "Ignore weapon swap:",
            None,
            Some(Prop::from("Ignore items and skills in weapon swap.")),
            false,
        ),
    );
    // L462-464
    cx.add(
        me,
        "siteCharBanditNote",
        LabelControl::new(
            Anchor::new(TopLeft, site_char_import_header, BottomLeft),
            rect(0, 50, 200, 14),
            "^7Tip: After you finish importing a character, make sure you update the bandit choice,\nas it can only be imported by logging in above.",
        ),
    );

    // L466-470
    cx.add(
        me,
        "siteCharClose",
        ButtonControl::new(
            Anchor::new(TopLeft, site_char_import_header, BottomLeft),
            rect(0, 90, 60, 20),
            "Close",
            move |cx| {
                let s = st_mut(cx, me);
                // L468
                s.char_import_mode = "GETACCOUNTNAME".to_owned();
                // L469
                s.char_import_status = "Idle".to_owned();
            },
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_lists() {
        let v: Json = serde_json::from_str(r#"[{"a":1}, null, {"b":2}]"#).unwrap();
        assert_eq!(json_list(&v).len(), 1);
        let o: Json = serde_json::from_str(r#"{"a":1}"#).unwrap();
        assert!(json_list(&o).is_empty());
    }
}
