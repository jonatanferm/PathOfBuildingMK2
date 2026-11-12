//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/PoBArchivesProvider.lua 1-150 (the
//! pobarchives.com build list provider: "Trending" / "Latest" builds, or "Similar Builds" of an
//! import code). Owner: U52.
//!
//! @pob-file Classes/PoBArchivesProvider.lua @16de4b82d
//!
//! `newClass("PoBArchivesProvider", "ExtBuildListProvider")`: [`PoBArchivesProvider`] implements
//! [`ExtBuildListProviderClass`] and overrides `GetPageUrl` and `GetBuilds`.
//!
//! Network:
//! * "builds" mode fetches `GetApiUrl()` with `launch:DownloadPage` (U16's
//!   [`crate::platform::net::download_page`] over `Sys::net`).
//! * "similar" mode POSTs `importCode=<code>` to `/api/recommendations` with its own curl
//!   sub-script (`LaunchSubScript`, L51-75). The port hands the same request to `Sys::net` and
//!   registers the completion with `launch:RegisterSubScript` (U16's
//!   [`crate::launch::register_sub_script`]), as U43's `UploadBuild` does. The sub-script returns
//!   `page, res` (the body and `getinfo_response_code()`), so the callback's `errMsg` is the
//!   numeric HTTP status: [`response_code`] reads it from the completion's headers (the last
//!   status line; 0 when the transfer produced none, which is what curl reports for a failed
//!   `perform`).
//!
//! DIVERGENCE(http-options): the sub-script's `OPT_IPRESOLVE` / `OPT_PROXY` options
//! (`launch.connectionProtocol`, `launch.proxyURL`) have no `HttpRequest` field (CCR-0026 C, as
//! in U16's DownloadPage); curl's `OPT_ACCEPT_ENCODING ""` is what `Sys::net` always does.
//! DIVERGENCE(nil-code): a nil import code makes the Lua sub-script fail to concatenate it
//! (`launch:OnSubError`, which ignores CUSTOM sub-scripts, so `statusMsg` stays "Loading..");
//! the port sends nothing in that case, with the same visible result.
//! DIVERGENCE(json): dkjson is serde_json (U43's `poe_api::dkjson_decode`): numbers beyond f64
//! range fail to parse (dkjson reads them as `inf`).
//! ORDER: `pairs(obj.builds)` (L100) is the document order for a JSON array (dkjson stores an
//! array 1..n in the array part, so `pairs` visits it in index order); for a JSON object (string
//! keys, PoB's order random per process, calc doc §3.4) the port visits the keys sorted
//! (serde_json's map order); the API returns an array.
#![allow(non_snake_case, non_upper_case_globals)] // Lua names verbatim (calc doc §3.1 rule 1)
#![allow(clippy::field_reassign_with_default)] // `build.x = ..` statement by statement

use std::cell::RefCell;
use std::rc::Rc;

use pob_sg::HttpRequest;
use pob_ui_core::prelude::*;

use super::provider::{ExtBuild, ExtBuildListProvider, ExtBuildListProviderClass, LVal};
use crate::app::CxApp;
use crate::platform::net::{download_page, DownloadParams, PageResponse};

/// `local archivesUrl = 'https://pobarchives.com'`
/// @pob Classes/PoBArchivesProvider.lua:7-13 h=b7bf0c13
pub const archivesUrl: &str = "https://pobarchives.com";

/// A `PoBArchivesProvider` object.
pub struct PoBArchivesProvider {
    base: RefCell<ExtBuildListProvider>,
    /// `self.mode` ("builds" or "similar")
    pub mode: String,
}

/// The final HTTP status of a completion (`easy:getinfo_response_code()`): the last status line
/// of the response headers, 0 without one.
pub fn response_code(headers: &str) -> f64 {
    headers
        .split(['\r', '\n'])
        .filter_map(|l| pob_ui_core::luastr::lmatch(l, "^HTTP/[%d%.]+ (%d+)"))
        .next_back()
        .and_then(|c| pob_lua::tonumber(&c))
        .unwrap_or(0.0)
}

/// `t.k` of a decoded JSON value, with Lua's rules for indexing it (`what` names `t` in the
/// error, e.g. `"field 'build_info'"`): a table gives its field (nil for a JSON `null` or an
/// array), a string gives nil (the string library has none of the keys PoB reads), anything else
/// is the Lua index error.
#[track_caller]
fn index(t: Option<&serde_json::Value>, k: &str, what: &str) -> Option<serde_json::Value> {
    use serde_json::Value as J;
    match t {
        Some(J::Object(m)) => m.get(k).filter(|v| !v.is_null()).cloned(),
        Some(J::Array(_) | J::String(_)) => None,
        other => {
            let ty = match other {
                None | Some(J::Null) => "nil",
                Some(J::Bool(_)) => "boolean",
                _ => "number",
            };
            lua_error!("attempt to index {what} (a {ty} value)")
        }
    }
}

/// `t.k` stored into a build field.
#[track_caller]
fn field(t: Option<&serde_json::Value>, k: &str, what: &str) -> Option<LVal> {
    index(t, k, what).as_ref().and_then(LVal::from_json)
}

impl PoBArchivesProvider {
    /// lua: PoBArchivesProvider.lua:15-24 PoBArchivesProvider(mode)
    /// @pob Classes/PoBArchivesProvider.lua:15-24 h=4175bdd1
    pub fn new(mode: &str) -> Rc<PoBArchivesProvider> {
        // L16
        let base = if mode == "builds" {
            // L17
            ExtBuildListProvider::new(Some(vec!["Trending".to_owned(), "Latest".to_owned()]))
        } else {
            // L19
            ExtBuildListProvider::new(Some(vec!["Similar Builds".to_owned()]))
        };
        let p = PoBArchivesProvider {
            base: RefCell::new(base),
            // L22
            mode: mode.to_owned(),
        };
        // L21
        p.base.borrow_mut().buildList = Vec::new();
        // L23 return self
        Rc::new(p)
    }

    /// lua: PoBArchivesProvider.lua:26-32 GetApiUrl()
    /// @pob Classes/PoBArchivesProvider.lua:26-32 h=f85fa3d5
    pub fn get_api_url(&self) -> String {
        let b = self.base.borrow();
        // L27
        if b.importCode.is_some() {
            // L28
            format!("{archivesUrl}/api/recommendations")
        } else {
            // L30
            let active = b.activeList.as_deref().unwrap_or_else(|| {
                lua_error!("bad argument #1 to 'lower' (string expected, got nil)")
            });
            format!(
                "{archivesUrl}/api/builds?q={}",
                pob_lua::string::lower(active)
            )
        }
    }

    /// lua: PoBArchivesProvider.lua:50-90 GetRecommendations(buildCode, postURL)
    /// @pob Classes/PoBArchivesProvider.lua:50-90 h=6e19b640
    pub fn get_recommendations(
        self: Rc<Self>,
        cx: &mut Cx,
        buildCode: Option<String>,
        postURL: &str,
    ) {
        // L51-75 the curl sub-script: setopt_url(postURL) (L56), OPT_POST (L57), the user agent
        // (L58), OPT_POSTFIELDS 'importCode=' .. code (L59), ACCEPT_ENCODING "" (L60); its
        // arguments (L75): buildCode, launch.connectionProtocol, launch.proxyURL
        let version = cx.app().launch.version_number.clone().unwrap_or_else(|| {
            lua_error!("attempt to concatenate field 'versionNumber' (a nil value)")
        });
        // DIVERGENCE(nil-code), module notes
        let id = buildCode.map(|code| {
            let req = HttpRequest {
                url: postURL.to_owned(),
                headers: Vec::new(),
                body: Some(format!("importCode={code}").into_bytes()),
                user_agent: Some(format!("Path of Building/{version}")),
            };
            cx.sys.net.download_page(req)
        });

        // L77
        if let Some(id) = id {
            let this = self.clone();
            // L78
            crate::launch::register_sub_script(
                cx,
                Some(id),
                Some(Rc::new(move |_cx: &mut Cx, resp: pob_sg::HttpResponse| {
                    // the sub-script's `return page, res` (L73)
                    let response = resp
                        .body
                        .as_ref()
                        .map(|b| String::from_utf8_lossy(b).into_owned())
                        .unwrap_or_default();
                    let errMsg = response_code(&resp.headers);
                    // L79
                    if errMsg == 200.0 {
                        // L80
                        this.base.borrow_mut().statusMsg = None;
                        // L81
                        this.parse_builds(Some(&response));
                        // L82 return
                    } else {
                        // L84
                        this.base.borrow_mut().statusMsg = Some(format!(
                            "Error while fetching similar builds: {}",
                            pob_lua::tostring_num(errMsg)
                        ));
                        // L85 return
                    }
                })),
            );
        }
    }

    /// lua: PoBArchivesProvider.lua:92-127 ParseBuilds(message)
    /// @pob Classes/PoBArchivesProvider.lua:92-127 h=a29bb7e2
    pub fn parse_builds(&self, message: Option<&str>) {
        use serde_json::Value as J;
        // L93 local obj = dkjson.decode(message)
        let message = message
            .unwrap_or_else(|| lua_error!("bad argument #1 to 'find' (string expected, got nil)"));
        let obj = crate::tabs::import::poe_api::dkjson_decode(message);
        // L94 if not obj or not obj.builds or next(obj.builds) == nil then
        let builds = match &obj {
            None | Some(J::Bool(false)) => None,
            o => index(o.as_ref(), "builds", "local 'obj'"),
        };
        let values: Option<Vec<J>> = match &builds {
            None | Some(J::Bool(false)) => None,
            // ORDER: module notes (array: document order; object: sorted keys)
            Some(J::Array(a)) => Some(a.iter().filter(|v| !v.is_null()).cloned().collect()),
            Some(J::Object(m)) => Some(m.values().filter(|v| !v.is_null()).cloned().collect()),
            Some(other) => lua_error!(
                "bad argument #1 to 'next' (table expected, got {})",
                LVal::type_name(LVal::from_json(other).as_ref())
            ),
        };
        let Some(values) = values.filter(|v| !v.is_empty()) else {
            // L95
            self.base.borrow_mut().statusMsg = Some("No builds found.".to_owned());
            // L96
            return;
        };

        // L100 for _, value in pairs(obj.builds) do
        for value in &values {
            let v = Some(value);
            let build_info = || index(v, "build_info", "local 'value'");
            let bi = |k: &str| field(build_info().as_ref(), k, "field 'build_info'");
            // L101
            let mut build = ExtBuild::default();
            // L102
            build.buildName = bi("title");
            // L103
            build.author = bi("author");
            // L104
            build.mainSkill = bi("mainSkill");
            // L105
            let asc = bi("ascendancy");
            if asc != Some(LVal::Str("None".to_owned())) {
                // L106
                build.ascendancy = bi("ascendancy");
            }
            // L108
            if bi("class") != Some(LVal::Str("None".to_owned())) {
                // L109
                build.class = bi("class");
            }
            // L111
            build.previewLink = Some(format!(
                "{archivesUrl}/build/{}",
                LVal::concat(bi("short_uuid").as_ref(), "field 'short_uuid'")
            ));
            // L112
            build.buildLink = bi("build_link");
            let stats = || index(v, "stats", "local 'value'");
            // L113
            build.ehp = field(stats().as_ref(), "TotalEHP", "field 'stats'");
            // L114
            build.life = field(stats().as_ref(), "LifeUnreserved", "field 'stats'");
            // L115
            build.es = field(stats().as_ref(), "EnergyShield", "field 'stats'");
            // L116
            build.dps = field(v, "fullDPS", "local 'value'");
            // L117 build.version = value.tree and value.tree.version
            let tree = field(v, "tree", "local 'value'");
            build.version = if LVal::truthy(tree.as_ref()) {
                field(
                    index(v, "tree", "local 'value'").as_ref(),
                    "version",
                    "field 'tree'",
                )
            } else {
                tree
            };

            // L119
            let score = field(v, "similarity_score", "local 'value'");
            if LVal::truthy(score.as_ref()) {
                // L120-122
                build.metadata = Some(vec![("Match Score".to_owned(), score)]);
            }
            // L124 -- build.score = value.similarity_score
            // L125
            self.base.borrow_mut().buildList.push(Rc::new(build));
        }
    }
}

impl ExtBuildListProviderClass for PoBArchivesProvider {
    fn base(&self) -> &RefCell<ExtBuildListProvider> {
        &self.base
    }

    /// lua: PoBArchivesProvider.lua:34-49 GetPageUrl()
    /// @pob Classes/PoBArchivesProvider.lua:34-49 h=76ed64b2
    fn get_page_url(&self) -> Option<String> {
        // L35
        let buildsPath = "/builds";
        let active = self.base.borrow().activeList.clone();
        // L36
        if active.as_deref() == Some("Latest") {
            // L37
            return Some(format!("{archivesUrl}{buildsPath}/yenTGNDb"));
        }
        // L39
        if active.as_deref() == Some("Trending") {
            // L40
            return Some(format!(
                "{archivesUrl}{buildsPath}/7U8QXU8m?sort=popularity"
            ));
        }
        // L42 -- TODO extract id and page
        // L43
        if self.mode == "similar" {
            // L44 -- return archivesUrl .. buildsPath .. '/?similar=' .. self.importCode
            // L45
            return None;
        }

        // L48
        None
    }

    /// lua: PoBArchivesProvider.lua:129-150 GetBuilds()
    /// @pob Classes/PoBArchivesProvider.lua:129-150 h=31c63758
    fn get_builds(self: Rc<Self>, cx: &mut Cx) {
        {
            let mut b = self.base.borrow_mut();
            // L130
            b.statusMsg = Some("Loading..".to_owned());
            // L131
            b.buildList.clear();
            // L132
            b.contentHeight = None;
        }

        // L135
        if self.mode == "similar" {
            // L136
            let code = self.base.borrow().importCode.clone();
            let url = self.get_api_url();
            self.get_recommendations(cx, code, &url);
            // L137 return
        } else {
            // L139
            let url = self.get_api_url();
            let this = self.clone();
            download_page(
                cx,
                &url,
                Rc::new(
                    move |_cx: &mut Cx, response: PageResponse, errMsg: Option<String>| {
                        // L140
                        if let Some(errMsg) = errMsg {
                            // L141
                            this.base.borrow_mut().statusMsg = Some(errMsg);
                            // L142
                            return;
                        }

                        // L145
                        this.parse_builds(response.body.as_deref());

                        // L147
                        this.base.borrow_mut().statusMsg = None;
                    },
                ),
                Some(DownloadParams::default()),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(p: &PoBArchivesProvider) -> Option<Vec<String>> {
        p.get_list_titles()
    }

    #[test]
    fn modes_titles_and_urls() {
        let b = PoBArchivesProvider::new("builds");
        assert_eq!(
            titles(&b),
            Some(vec!["Trending".to_owned(), "Latest".to_owned()])
        );
        b.base.borrow_mut().activeList = Some("Trending".into());
        assert_eq!(
            b.get_api_url(),
            "https://pobarchives.com/api/builds?q=trending"
        );
        assert_eq!(
            b.get_page_url().as_deref(),
            Some("https://pobarchives.com/builds/7U8QXU8m?sort=popularity")
        );
        b.base.borrow_mut().activeList = Some("Latest".into());
        assert_eq!(
            b.get_page_url().as_deref(),
            Some("https://pobarchives.com/builds/yenTGNDb")
        );
        let s = PoBArchivesProvider::new("similar");
        assert_eq!(titles(&s), Some(vec!["Similar Builds".to_owned()]));
        s.set_import_code(Some("abc".into()));
        assert_eq!(
            s.get_api_url(),
            "https://pobarchives.com/api/recommendations"
        );
        assert_eq!(s.get_page_url(), None);
    }

    #[test]
    fn response_codes() {
        assert_eq!(response_code("HTTP/1.1 200 OK\r\n\r\n"), 200.0);
        assert_eq!(
            response_code("HTTP/1.1 301 Moved\r\nLocation: x\r\n\r\nHTTP/2 404 \r\n\r\n"),
            404.0
        );
        assert_eq!(response_code(""), 0.0);
    }

    #[test]
    fn parse_builds_fields() {
        let p = PoBArchivesProvider::new("builds");
        p.parse_builds(Some(
            r#"{"builds":[{"build_info":{"title":"A","author":"me","mainSkill":"Arc",
            "ascendancy":"None","class":"Witch","short_uuid":"u1","build_link":"https://pobb.in/x"},
            "stats":{"TotalEHP":1000.5,"LifeUnreserved":"12","EnergyShield":3},"fullDPS":12345.6,
            "tree":{"version":"3_25"},"similarity_score":0.75}]}"#,
        ));
        let b = p.base.borrow();
        assert_eq!(b.statusMsg, None);
        let e = &b.buildList[0];
        assert_eq!(e.buildName, Some(LVal::Str("A".into())));
        assert_eq!(e.ascendancy, None);
        assert_eq!(e.class, Some(LVal::Str("Witch".into())));
        assert_eq!(
            e.previewLink.as_deref(),
            Some("https://pobarchives.com/build/u1")
        );
        assert_eq!(e.life, Some(LVal::Str("12".into())));
        assert_eq!(e.version, Some(LVal::Str("3_25".into())));
        assert_eq!(
            e.metadata,
            Some(vec![("Match Score".to_owned(), Some(LVal::Num(0.75)))])
        );
    }

    #[test]
    fn parse_builds_empty_and_errors() {
        for msg in [
            "",
            "nope",
            "[]",
            "{}",
            r#"{"builds":[]}"#,
            r#"{"builds":{}}"#,
            "false",
            "null",
            r#""str""#,
        ] {
            let p = PoBArchivesProvider::new("builds");
            p.parse_builds(Some(msg));
            assert_eq!(
                p.base.borrow().statusMsg.as_deref(),
                Some("No builds found."),
                "{msg}"
            );
        }
        let e =
            pob_lua::error::catch(|| PoBArchivesProvider::new("builds").parse_builds(Some("5")))
                .expect_err("index a number");
        assert_eq!(e.0, "attempt to index local 'obj' (a number value)");
        let e = pob_lua::error::catch(|| {
            PoBArchivesProvider::new("builds").parse_builds(Some(r#"{"builds":[{}]}"#));
        })
        .expect_err("no build_info");
        assert_eq!(e.0, "attempt to index field 'build_info' (a nil value)");
    }
}
