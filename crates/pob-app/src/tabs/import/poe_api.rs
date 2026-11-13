//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/PoEAPI.lua 1-238. Owner: U43.
//!
//! @pob-file Classes/PoEAPI.lua @16de4b82d
//!
//! `main.api` (one `PoEAPI` object per app, created by the first ImportTab, ImportTab.lua:483-485)
//! lives in [`super::core::MainExtra`] (CCR-0109 §1). Its methods are the free functions here,
//! which act on that object (Lua `self` is always `main.api`: every caller writes
//! `main.api:Method(..)`). Callbacks are `Rc` closures called where the Lua calls them.
//!
//! Lua semantics:
//! * `dkjson.decode(s)` is [`dkjson_decode`] over serde_json: JSON `null` decodes to nil (dkjson's
//!   default `nullval`), a parse error to nil. Decoded tables are [`Json`] values.
//!   DIVERGENCE(json-scalars): a token field that is neither a string nor a number (Lua would
//!   store the boolean/table and fail later at a concatenation) reads as nil.
//! * `math.randomseed` / `math.random` are LuaJIT 2.1's Tausworthe generator,
//!   `pob_lua::math::{m_randomseed, m_random}` (`lib_math.c`; CCR-0033 §3, CCR-0109 §4; FUP7
//!   replaced the local `lj_random` copy), one state per app thread as LuaJIT keeps one per Lua
//!   state.
//! * `os.time()` is [`os_time`] (CCR-0109 §3: `Platform` has no wall clock).
//! * `launch:DownloadPage` is [`crate::platform::net::download_page`]; the OAuth listener
//!   (`LaunchSubScript(LaunchServer.lua)`) is `Sys::net.oauth_listen`, whose completion is decoded
//!   with `pob_host::OAuthResult::from_response` (CCR-0026 B).
//! * `self.rateLimiter` (`TradeQueryRateLimiter`, U51, optional and deferred) is the
//!   [`RateLimiter`] trait; until U51 installs its class through [`set_rate_limiter_factory`] the
//!   stand-in [`PendingRateLimiter`] never delays a request (CCR-0109 §2).
//!
//! DIVERGENCE(set-foreground): `SetForeground()` (L148) has no `Platform` method; it does
//! nothing (CCR-0109 §3).
//! ORDER: no `pairs` loops.

use std::cell::RefCell;
use std::rc::Rc;

use pob_lua::math::{m_random, m_randomseed};
use pob_lua::{lfmt, FmtArg};
use pob_sg::HttpResponse;
use pob_ui_core::luastr::lmatch;
use pob_ui_core::prelude::*;

use crate::app::CxApp;
use crate::launch::SubScript;
use crate::main_mode::settings::save_settings;
use crate::platform::net::{download_page, DownloadParams, PageResponse};

use super::core::{main_extra, main_extra_mut};

/// A dkjson-decoded value (a Lua table, string, number or boolean).
pub type Json = serde_json::Value;

/// `dkjson.decode(s)`: the value, or nil on a parse error or a top-level `null`.
pub fn dkjson_decode(s: &str) -> Option<Json> {
    match serde_json::from_str::<Json>(s) {
        Ok(Json::Null) | Err(_) => None,
        Ok(v) => Some(v),
    }
}

/// `t.k` of a decoded table (nil for a missing key, a JSON `null`, or a non-table `t`).
pub fn json_get<'a>(t: &'a Json, k: &str) -> Option<&'a Json> {
    t.get(k).filter(|v| !v.is_null())
}

/// Lua truthiness of a decoded value (`nil` and `false` are falsy).
pub fn json_truthy(v: Option<&Json>) -> bool {
    !matches!(v, None | Some(Json::Null | Json::Bool(false)))
}

/// A decoded scalar stored in a string field (`self.authToken = responseLua.access_token`):
/// strings as is, numbers as `tostring` writes them; DIVERGENCE(json-scalars) otherwise nil.
fn json_string(v: Option<&Json>) -> Option<String> {
    match v? {
        Json::String(s) => Some(s.clone()),
        Json::Number(n) => n.as_f64().map(pob_lua::tostring_num),
        _ => None,
    }
}

/// A decoded value in arithmetic (`os.time() + responseLua.expires_in`): a number, or a string
/// Lua coerces; anything else is the Lua error.
fn json_arith(v: Option<&Json>, field: &str) -> f64 {
    let n = match v {
        Some(Json::Number(n)) => n.as_f64(),
        Some(Json::String(s)) => pob_lua::tonumber(s),
        _ => None,
    };
    n.unwrap_or_else(|| {
        let ty = match v {
            None => "nil",
            Some(Json::String(_)) => "string",
            Some(Json::Bool(_)) => "boolean",
            Some(_) => "table",
        };
        lua_error!("attempt to perform arithmetic on field '{field}' (a {ty} value)")
    })
}

/// `ConPrintf(fmt, ...)`.
fn con_printf(cx: &mut Cx, fmt: &str, args: &[FmtArg<'_>]) {
    let s = pob_lua::lua_format(fmt, args);
    cx.sys.platform.con_print(&s);
}

// ---- os.time (CCR-0109 §3) -------------------------------------------------------------------

thread_local! {
    static OS_TIME: RefCell<Option<Rc<dyn Fn(&Cx) -> f64>>> = const { RefCell::new(None) };
}

/// `os.time()`. CCR-0109 §3: `Platform` has no wall clock, so the default reads the system clock
/// (the virtual clock's seconds on wasm32, where `SystemTime` is unavailable:
/// DIVERGENCE(os-time-wasm)); tests and the oracle install the virtual clock with
/// [`set_os_time`].
pub fn os_time(cx: &Cx) -> f64 {
    if let Some(f) = OS_TIME.with(|t| t.borrow().clone()) {
        return f(cx);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = cx;
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs() as f64)
    }
    #[cfg(target_arch = "wasm32")]
    {
        (cx.sys.time_ms() / 1000) as f64
    }
}

/// Replace the `os.time()` source (`None`: the default).
pub fn set_os_time(f: Option<Rc<dyn Fn(&Cx) -> f64>>) {
    OS_TIME.with(|t| *t.borrow_mut() = f);
}

// ---- TradeQueryRateLimiter seam (U51; CCR-0109 §2) -------------------------------------------

/// The `TradeQueryRateLimiter` methods PoEAPI calls (TradeQueryRateLimiter.lua:115, :155, :204,
/// :232). U51 implements it for its port of the class.
pub trait RateLimiter {
    /// `NextRequestTime(policy, time)`
    fn next_request_time(&mut self, policy: &str, time: f64) -> f64;
    /// `InsertRequest(policy)` (the request id; `now` is the `os.time()` it reads)
    fn insert_request(&mut self, policy: &str, now: f64) -> f64;
    /// `FinishRequest(policy, requestId)`
    fn finish_request(&mut self, policy: &str, request_id: f64);
    /// `UpdateFromHeader(headerString, policy)` (`now`: the `os.time()` it reads)
    fn update_from_header(&mut self, header: &str, policy: &str, now: f64);
}

/// CCR-0109 §2 stand-in until U51 ports `TradeQueryRateLimiter`: every request may go now,
/// request ids count from 0 (TradeQueryRateLimiter.lua:50, :226-227).
#[derive(Default)]
pub struct PendingRateLimiter {
    request_id: f64,
}

impl RateLimiter for PendingRateLimiter {
    fn next_request_time(&mut self, _policy: &str, time: f64) -> f64 {
        time
    }
    fn insert_request(&mut self, _policy: &str, _now: f64) -> f64 {
        let id = self.request_id;
        self.request_id += 1.0;
        id
    }
    fn finish_request(&mut self, _policy: &str, _request_id: f64) {}
    fn update_from_header(&mut self, _header: &str, _policy: &str, _now: f64) {}
}

type RateLimiterFactory = Rc<dyn Fn() -> Box<dyn RateLimiter>>;

thread_local! {
    static RATE_LIMITER_FACTORY: RefCell<Option<RateLimiterFactory>> = const { RefCell::new(None) };
}

/// `new("TradeQueryRateLimiter"):TradeQueryRateLimiter()` for new PoEAPI objects (U51).
pub fn set_rate_limiter_factory(f: Option<RateLimiterFactory>) {
    RATE_LIMITER_FACTORY.with(|r| *r.borrow_mut() = f);
}

fn new_rate_limiter() -> Box<dyn RateLimiter> {
    match RATE_LIMITER_FACTORY.with(|r| r.borrow().clone()) {
        Some(f) => f(),
        None => Box::new(PendingRateLimiter::default()),
    }
}

// ---- the class -----------------------------------------------------------------------------

// @pob Classes/PoEAPI.lua:1-28 h=9c09f550
// L1-3 (`require("base64")`, `require("sha2")`, `require "dkjson"`: crate::model / sha2 / serde_json)

/// L5-10 `scopesOAuth`
const SCOPES_OAUTH: [&str; 4] = [
    "account:profile",
    "account:leagues",
    "account:characters",
    "account:trade",
];

/// L12
const FILENAME: &str = "poe_api_response.json";

/// L14-15 `newClass("PoEAPI")`: the fields of a PoEAPI object.
pub struct PoEAPI {
    pub retries: f64,
    pub auth_token: Option<String>,
    pub refresh_token: Option<String>,
    pub token_expiry: Option<f64>,
    pub base_url: String,
    pub rate_limiter: Box<dyn RateLimiter>,
    pub token_has_been_validated: bool,
    pub error_no_auth: &'static str,
}

impl PoEAPI {
    /// lua: PoEAPI.lua:17-28 `PoEAPI(authToken, refreshToken, tokenExpiry)`
    pub fn new(
        auth_token: Option<String>,
        refresh_token: Option<String>,
        token_expiry: Option<f64>,
    ) -> PoEAPI {
        PoEAPI {
            // L18
            retries: 0.0,
            // L19
            auth_token,
            // L20
            refresh_token,
            // L21
            token_expiry: Some(token_expiry.unwrap_or(0.0)),
            // L22
            base_url: "https://api.pathofexile.com".to_owned(),
            // L23
            rate_limiter: new_rate_limiter(),
            // L24
            token_has_been_validated: false,
            // L26
            error_no_auth: "No auth token",
        }
    }
}

/// `main.api` (an index error when it does not exist, as in Lua).
fn with_api<R>(cx: &Cx, f: impl FnOnce(&mut PoEAPI) -> R) -> R {
    main_extra_mut(cx, |m| match m.api.as_mut() {
        Some(a) => f(a),
        None => lua_error!("attempt to index field 'api' (a nil value)"),
    })
}

/// `main.api.authToken`
pub fn auth_token(cx: &Cx) -> Option<String> {
    main_extra(cx, |m| m.api.as_ref().and_then(|a| a.auth_token.clone()))
}

/// `callback(valid, errMsg)`
pub type ValidateCallback = Rc<dyn Fn(&mut Cx, bool, Option<String>)>;

/// lua: PoEAPI.lua:30-69 `PoEAPIClass:ValidateAuth(callback)`
/// @pob Classes/PoEAPI.lua:30-69 h=a7629703
pub fn validate_auth(cx: &mut Cx, callback: ValidateCallback) {
    // L30-32 performs a basic check on the validity of the current login by refreshing the
    // token if necessary. if a refresh is attempted and fails, the login details
    // will be reset.
    let (auth, refresh, expiry) = with_api(cx, |a| {
        (
            a.auth_token.clone(),
            a.refresh_token.clone(),
            a.token_expiry,
        )
    });
    // L35
    if let (Some(_), Some(refresh), Some(expiry)) = (auth, refresh, expiry) {
        // L36
        con_printf(cx, "Validating auth token", &[]);
        // L37
        if expiry < os_time(cx) {
            // L38
            con_printf(cx, "Auth token expired", &[]);
            // L39 here recreate the token with the refresh_token
            // L40
            let form_text =
                format!("client_id=pob&grant_type=refresh_token&refresh_token={refresh}");
            // L41
            download_page(
                cx,
                "https://www.pathofexile.com/oauth/token",
                Rc::new(move |cx, response: PageResponse, err_msg: Option<String>| {
                    // L42
                    con_printf(cx, "Recreating auth token", &[]);
                    // L43
                    if let Some(err_msg) = err_msg {
                        // L44
                        con_printf(
                            cx,
                            "Failed to recreate auth token: %s",
                            &[FmtArg::from(err_msg.as_str())],
                        );
                        // L45
                        reset_details(cx);
                        // L46
                        callback(cx, false, Some(err_msg));
                        // L47
                        return;
                    }
                    // L49
                    let response_lua = dkjson_decode(body_or_err(&response));
                    // L50
                    match response_lua {
                        None => {
                            // L51
                            reset_details(cx);
                            // L52
                            callback(cx, false, Some("Malformed response".to_owned()));
                        }
                        Some(r) => {
                            let now = os_time(cx);
                            with_api(cx, |a| {
                                // L54
                                a.auth_token = json_string(json_get(&r, "access_token"));
                                // L55
                                a.refresh_token = json_string(json_get(&r, "refresh_token"));
                                // L56
                                a.token_expiry = Some(
                                    now + json_arith(json_get(&r, "expires_in"), "expires_in"),
                                );
                            });
                            // L57
                            update_main(cx);
                            // L58
                            with_api(cx, |a| a.retries = 0.0);
                            // L59
                            callback(cx, true, None);
                        }
                    }
                }),
                // L62
                Some(DownloadParams {
                    header: None,
                    body: Some(form_text),
                }),
            );
        } else {
            // L64
            callback(cx, true, None);
        }
    } else {
        // L67
        callback(cx, false, None);
    }
}

/// `dkjson.decode(response.body)` with a nil body is dkjson's argument error.
fn body_or_err(response: &PageResponse) -> &str {
    response
        .body
        .as_deref()
        .unwrap_or_else(|| lua_error!("bad argument #1 to 'find' (string expected, got nil)"))
}

/// lua: PoEAPI.lua:71-74 `local function base64_encode(secret)`
/// @pob Classes/PoEAPI.lua:71-74 h=ea94ab54
pub fn base64_encode(secret: &[u8]) -> String {
    // L73 base64.encode(secret):gsub("+", "-"):gsub("/", "_"):gsub("=$", "") (the last gsub
    // removes one trailing "=")
    // CCR-0109 §6: runtime/lua/base64.lua is calc's port in passive_spec (not re-exported by
    // crate::model)
    let s = pob_calc::classes::passive_spec::base64_encode(secret)
        .replace('+', "-")
        .replace('/', "_");
    match s.strip_suffix('=') {
        Some(t) => t.to_owned(),
        None => s,
    }
}

/// lua: PoEAPI.lua:76-82 `PoEAPIClass:ResetDetails()`
/// @pob Classes/PoEAPI.lua:76-82 h=43a85795
pub fn reset_details(cx: &mut Cx) {
    // L76 resets current authorization details
    with_api(cx, |a| {
        // L78
        a.auth_token = None;
        // L79
        a.refresh_token = None;
        // L80
        a.token_expiry = None;
    });
    // L81
    update_main(cx);
}

/// lua: PoEAPI.lua:84-90 `PoEAPIClass:UpdateMain()`
/// @pob Classes/PoEAPI.lua:84-90 h=d1a47601
pub fn update_main(cx: &mut Cx) {
    // L84 updates main so that API details are saved across restarts
    let (auth, refresh, expiry) = with_api(cx, |a| {
        (
            a.auth_token.clone(),
            a.refresh_token.clone(),
            a.token_expiry,
        )
    });
    let m = &mut cx.app_mut().main;
    // L86
    m.last_token = auth;
    // L87
    m.last_refresh_token = refresh;
    // L88
    m.token_expiry = expiry;
    // L89
    save_settings(cx);
}

/// `sha.hex_to_bin(sha.sha256(s))`: the raw SHA-256 digest.
fn sha256_bin(s: &[u8]) -> Vec<u8> {
    use sha2::Digest as _;
    sha2::Sha256::digest(s).to_vec()
}

/// `callback(errCode)`
pub type AuthCallback = Rc<dyn Fn(&mut Cx, Option<String>)>;

/// lua: PoEAPI.lua:92-154 `PoEAPIClass:FetchAuthToken(callback)`
/// @pob Classes/PoEAPI.lua:92-154 h=c4946280
pub fn fetch_auth_token(cx: &mut Cx, callback: AuthCallback) {
    // L94
    m_randomseed(os_time(cx));
    // L95
    let secret = m_random(Some(pob_lua::math::lpow(2.0, 32.0) - 1.0), None);
    // L96
    let code_verifier = base64_encode(pob_lua::tostring_num(secret).as_bytes());
    // L97
    let code_challenge = base64_encode(&sha256_bin(code_verifier.as_bytes()));

    // L99 16 character hex string
    // L100-102
    let initial_state = pob_lua::gsub_fn(
        "xxxxxxxxxxxxxxxx",
        "x",
        |_| {
            Some(lfmt!(
                "%x",
                m_random(Some(0.0), Some(f64::from(0xf)))
            ))
        },
        None,
    )
    .0;

    // L104-109
    let auth_url = lfmt!(
        "https://www.pathofexile.com/oauth/authorize?client_id=pob&response_type=code&scope=%s&state=%s&code_challenge=%s&code_challenge_method=S256",
        SCOPES_OAUTH.join("%20").as_str(),
        initial_state.as_str(),
        code_challenge.as_str()
    );

    // L111-113 LaunchSubScript(LaunchServer.lua, "", "ConPrintf,OpenURL,Copy", authUrl): the
    // host's OAuth listener (CCR-0026 B)
    let id = cx.sys.net.oauth_listen(&auth_url);
    // L114
    if let Some(id) = id {
        // L115-117 launch.subScripts[id] = { type = "DOWNLOAD", callback = function(code,
        // errMsg, state, port) ... }: a DOWNLOAD entry is called with the sub-script's results,
        // which travel as an HttpResponse (CCR-0026 B)
        let cb: Rc<dyn Fn(&mut Cx, PageResponse, Option<String>)> = Rc::new(
            move |cx, page: PageResponse, err: Option<String>| {
                let r = pob_host::OAuthResult::from_response(&HttpResponse {
                    body: page.body.map(String::into_bytes),
                    error: err,
                    headers: page.header.unwrap_or_default(),
                });
                let (code, err_msg, state, port) = (r.code, r.err, r.state, r.port);
                // L118
                let Some(code) = code else {
                    // L119
                    con_printf(
                        cx,
                        "Failed to get code from server: %s",
                        &[err_msg.as_deref().map_or(FmtArg::Nil, FmtArg::from)],
                    );
                    // L120
                    reset_details(cx);
                    // L121
                    let no_auth = with_api(cx, |a| a.error_no_auth);
                    callback(cx, Some(err_msg.unwrap_or_else(|| no_auth.to_owned())));
                    // L122
                    return;
                };

                // L125
                if Some(initial_state.as_str()) != state.as_deref() {
                    // L126
                    con_printf(cx, "OAuth state mismatch during authentication", &[]);
                    // L127
                    reset_details(cx);
                    // L128
                    callback(cx, Some("OAuth state mismatch".to_owned()));
                    // L129
                    return;
                }
                // L131-134
                let port = port.unwrap_or_else(|| {
                    lua_error!("attempt to concatenate local 'port' (a nil value)")
                });
                let form_text = format!(
                    "client_id=pob&grant_type=authorization_code&code={code}&redirect_uri=http://localhost:{port}&scope={}&code_verifier={code_verifier}",
                    SCOPES_OAUTH.join(" ")
                );
                let callback = callback.clone();
                // L135
                download_page(
                    cx,
                    "https://www.pathofexile.com/oauth/token",
                    Rc::new(move |cx, response: PageResponse, err_msg: Option<String>| {
                        // L136
                        if let Some(err_msg) = err_msg {
                            // L137
                            con_printf(
                                cx,
                                &format!("Failed to get token from server: {err_msg}"),
                                &[],
                            );
                            // L138
                            reset_details(cx);
                            // L139
                            callback(cx, Some(err_msg));
                            // L140
                            return;
                        }
                        // L142
                        let response_lua =
                            dkjson_decode(body_or_err(&response)).unwrap_or_else(|| {
                                lua_error!("attempt to index local 'responseLua' (a nil value)")
                            });
                        let now = os_time(cx);
                        with_api(cx, |a| {
                            // L143
                            a.auth_token = json_string(json_get(&response_lua, "access_token"));
                            // L144
                            a.refresh_token = json_string(json_get(&response_lua, "refresh_token"));
                            // L145
                            a.token_expiry = Some(
                                now + json_arith(
                                    json_get(&response_lua, "expires_in"),
                                    "expires_in",
                                ),
                            );
                        });
                        // L146
                        update_main(cx);
                        // L147
                        with_api(cx, |a| a.retries = 0.0);
                        // L148 SetForeground(): DIVERGENCE(set-foreground)
                        // L149
                        callback(cx, None);
                    }),
                    // L150
                    Some(DownloadParams {
                        header: None,
                        body: Some(form_text),
                    }),
                );
            },
        );
        cx.app_mut()
            .launch
            .sub_scripts
            .insert(id, SubScript::Download(cb));
    }
}

/// `callback(response, errorMsg)` (`response` is nil when validation failed).
pub type RefreshCallback = Rc<dyn Fn(&mut Cx, Option<PageResponse>, Option<String>)>;

/// lua: PoEAPI.lua:156-190 `PoEAPIClass:DownloadWithRefresh(endpoint, callback)`
/// @pob Classes/PoEAPI.lua:156-190 h=d28f2ad8
pub fn download_with_refresh(cx: &mut Cx, endpoint: &str, callback: RefreshCallback) {
    let endpoint = endpoint.to_owned();
    // L159
    validate_auth(
        cx,
        Rc::new(move |cx, valid, validation_err_msg| {
            // L160
            if !valid {
                // L161 Clean info about token and refresh token
                // L162
                reset_details(cx);
                // L163
                let no_auth = with_api(cx, |a| a.error_no_auth);
                callback(
                    cx,
                    None,
                    Some(validation_err_msg.unwrap_or_else(|| no_auth.to_owned())),
                );
                // L164
                return;
            }

            // L167
            let (base_url, auth_token) =
                with_api(cx, |a| (a.base_url.clone(), a.auth_token.clone()));
            let url = format!("{base_url}{endpoint}");
            let endpoint2 = endpoint.clone();
            let callback2 = callback.clone();
            let on_page = Rc::new(
                move |cx: &mut Cx, response: PageResponse, err_msg: Option<String>| {
                    let retries = with_api(cx, |a| a.retries);
                    // L168
                    if err_msg
                        .as_deref()
                        .is_some_and(|e| lmatch(e, "401").is_some())
                        && retries < 1.0
                    {
                        // L169 try once again with refresh token
                        with_api(cx, |a| {
                            // L170
                            a.retries = 1.0;
                            // L171
                            a.token_expiry = Some(0.0);
                        });
                        // L172
                        download_with_refresh(cx, &endpoint2, callback2.clone());
                    } else {
                        // L174
                        with_api(cx, |a| a.retries = 0.0);
                        // L175
                        if let Some(e) = &err_msg {
                            // L176
                            con_printf(
                                cx,
                                "Failed to download %s: %s",
                                &[FmtArg::from(endpoint2.as_str()), FmtArg::from(e.as_str())],
                            );
                        // L177
                        } else if response.body.is_some() && cx.app().launch.dev_mode {
                            // L178 create the file and log the name file
                            // L179-183
                            let body = response.body.clone().unwrap_or_default();
                            let _ = cx.sys.fs.write(FILENAME, body.as_bytes());
                            // L184
                            con_printf(
                                cx,
                                "Download %s:\n%s\n",
                                &[FmtArg::from(endpoint2.as_str()), FmtArg::from(FILENAME)],
                            );
                        }
                        // L186
                        callback2(cx, Some(response), err_msg);
                    }
                },
            );
            // L188 { header = "Authorization: Bearer " .. self.authToken }
            let auth_token = auth_token.unwrap_or_else(|| {
                lua_error!("attempt to concatenate field 'authToken' (a nil value)")
            });
            download_page(
                cx,
                &url,
                on_page,
                Some(DownloadParams {
                    header: Some(format!("Authorization: Bearer {auth_token}")),
                    body: None,
                }),
            );
        }),
    );
}

/// `DownloadCallback fun(body: table?, err: string?, timeout: integer?)`. DIVERGENCE: on a
/// download error the Lua passes the raw `response` table as `body`; its only reader
/// (`data.character`, ImportTab.lua:258, :299) gets nil there, so the port passes `None`.
pub type DownloadCallback = Rc<dyn Fn(&mut Cx, Option<Json>, Option<String>, Option<f64>)>;

/// lua: PoEAPI.lua:192-221 `PoEAPIClass:DownloadWithRateLimit(policy, url, callback)`
/// @pob Classes/PoEAPI.lua:192-221 h=50f4f023
pub fn download_with_rate_limit(
    cx: &mut Cx,
    policy: &'static str,
    url: &str,
    callback: DownloadCallback,
) {
    // L197
    let now = os_time(cx);
    // L198
    let time_next = with_api(cx, |a| a.rate_limiter.next_request_time(policy, now));
    // L199
    if now >= time_next {
        // L200
        let t = os_time(cx);
        let request_id = with_api(cx, |a| a.rate_limiter.insert_request(policy, t));
        // L201
        let on_complete: RefreshCallback = Rc::new(move |cx, response, err_msg| {
            // L202
            with_api(cx, |a| a.rate_limiter.finish_request(policy, request_id));
            // L203
            let header = response
                .as_ref()
                .and_then(|r| r.header.clone())
                .unwrap_or_default();
            // L204
            let t = os_time(cx);
            with_api(cx, |a| {
                a.rate_limiter.update_from_header(&header, policy, t);
            });
            // L205
            if let Some(err_msg) = err_msg {
                // L206
                callback(cx, None, Some(err_msg), None);
                // L207
                return;
            }
            // L209
            if lmatch(&header, "HTTP/[%d%.]+ (%d+)").as_deref() == Some("429") {
                // L210
                let time_next = with_api(cx, |a| a.rate_limiter.next_request_time(policy, now));
                // L211
                callback(
                    cx,
                    None,
                    Some("Response code: 429".to_owned()),
                    Some(time_next),
                );
                // L212
                return;
            }
            // L214
            let response = response.unwrap_or_default();
            let response_lua = dkjson_decode(body_or_err(&response));
            // L215
            callback(cx, response_lua, None, None);
        });
        // L217
        download_with_refresh(cx, url, on_complete);
    } else {
        // L219
        callback(
            cx,
            None,
            Some("Response code: 429".to_owned()),
            Some(time_next),
        );
    }
}

/// lua: PoEAPI.lua:223-229 `PoEAPIClass:DownloadCharacterList(realm, callback)`
/// @pob Classes/PoEAPI.lua:223-229 h=a55a9965
pub fn download_character_list(cx: &mut Cx, realm: &str, callback: DownloadCallback) {
    // L223-225 Fetches character list from PoE's OAuth api
    // L227-228
    let url = format!(
        "/character{}",
        if realm == "pc" {
            String::new()
        } else {
            format!("/{realm}")
        }
    );
    download_with_rate_limit(cx, "character-list-request-limit", &url, callback);
}

/// lua: PoEAPI.lua:231-238 `PoEAPIClass:DownloadCharacter(realm, name, callback)`
/// @pob Classes/PoEAPI.lua:231-238 h=123d0ce7
pub fn download_character(cx: &mut Cx, realm: &str, name: &str, callback: DownloadCallback) {
    // L231-234 Fetches character from PoE's OAuth api
    // L236-237
    let url = format!(
        "/character{}/{name}",
        if realm == "pc" {
            String::new()
        } else {
            format!("/{realm}")
        }
    );
    download_with_rate_limit(cx, "character-request-limit", &url, callback);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_url_strips_one_pad() {
        // "ab" -> "YWI=" -> "YWI"; "a" -> "YQ==" -> "YQ=" (gsub("=$", "") removes one)
        assert_eq!(base64_encode(b"ab"), "YWI");
        assert_eq!(base64_encode(b"a"), "YQ=");
        assert_eq!(base64_encode(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn dkjson_null_and_errors_are_nil() {
        assert!(dkjson_decode("null").is_none());
        assert!(dkjson_decode("{bad").is_none());
        let v = dkjson_decode(r#"{"a": null, "b": 1}"#).unwrap();
        assert!(json_get(&v, "a").is_none());
        assert_eq!(json_arith(json_get(&v, "b"), "b"), 1.0);
    }
}
