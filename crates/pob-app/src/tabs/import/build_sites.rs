//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/BuildSiteTools.lua 1-129. Owner: U43.
//!
//! @pob-file Modules/BuildSiteTools.lua @16de4b82d
//!
//! The `buildSites` global: [`WEBSITE_LIST`], [`upload_build`], [`download_build`],
//! [`parse_import_link_from_uri`].
//!
//! Lua semantics:
//! * `link:gsub(regexURL, downloadURL)` and the protocol-handler patterns are Lua patterns with
//!   Lua 5.1 replacement rules (`%.` in a replacement is a literal `.`): `pob_lua::gsub`.
//! * `UploadBuild` runs its own curl sub-script (L50-79), which returns `page` for HTTP 200 and
//!   `nil, page` otherwise. The port sends the POST through `Sys::net` (the request the script
//!   makes: URL, user agent, body `postFields .. code`) and [`upload_result`] turns the completion
//!   back into those two results, reading the final status line of the response headers
//!   (`easy:getinfo_response_code()` after `FOLLOWLOCATION`; 0 when no response arrived).
//!   DIVERGENCE(http-options): as for DownloadPage (CCR-0026 C), `connectionProtocol` and
//!   `proxyURL` (L61-66) do not reach the request.
//! * `launch:DownloadPage` is [`crate::platform::net::download_page`].
//!
//! ORDER: `ipairs` only.

use std::rc::Rc;

use pob_sg::{HttpRequest, HttpResponse};
use pob_ui_core::prelude::*;

use crate::app::CxApp;
use crate::platform::net::{download_page, PageResponse};

// @pob Modules/BuildSiteTools.lua:1-42 h=de04d024
// L1-7 buildSites = { }

/// One `buildSites.websiteList` entry (L9-18):
/// label: What a user sees in the export dropdown and when the import box recognizes the website
/// id: Protocol handler id used to load builds with the pob: URL scheme e.g. pob://Maxroll/siteSpecificBuildId
/// matchURL: A pattern to match URLs belonging to this website to show a valid url message in ImportTab
/// regexURL: Copied link from website to extract the build ID and pass to downloadURL to get the raw build XML
/// downloadURL: The URL to download the raw build code
/// codeOut: Gets prepended to returned code from postUrl.  Needed to enable export in ImportTab
/// postUrl: The URL to upload a build code.  Needed to enable export in ImportTab
/// postFields: The POST fields prepended to base64-encoded XML.  Needed to enable export in ImportTab
/// linkURL: The URL pattern to link to the provided build code (Unused currently)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WebsiteInfo {
    pub label: &'static str,
    pub id: &'static str,
    pub match_url: &'static str,
    pub regex_url: &'static str,
    pub download_url: &'static str,
    pub code_out: Option<&'static str>,
    pub post_url: Option<&'static str>,
    pub post_fields: Option<&'static str>,
    pub link_url: &'static str,
}

/// `buildSites.websiteList` (L19-42).
pub static WEBSITE_LIST: [WebsiteInfo; 8] = [
    // L20-23
    WebsiteInfo {
        label: "Maxroll",
        id: "Maxroll",
        match_url: "^https://maxroll%.gg/poe/pob/.*",
        regex_url: "maxroll%.gg/poe/pob/(.+)%s*$",
        download_url: "maxroll%.gg/poe/api/pob/%1",
        code_out: Some("https://maxroll.gg/poe/pob/"),
        post_url: Some("https://maxroll.gg/poe/api/pob"),
        post_fields: Some("pobCode="),
        link_url: "maxroll%.gg/poe/pob/%1",
    },
    // L24-27
    WebsiteInfo {
        label: "pob.codes",
        id: "PoBCodes",
        match_url: "^https://pob%.codes/b/.+",
        regex_url: "pob%.codes/b/(.+)%s*$",
        download_url: "api.pob.codes/%1/raw",
        code_out: Some("https://pob.codes/b/"),
        post_url: Some("https://api.pob.codes/pob/plain"),
        post_fields: Some(""),
        link_url: "pob.codes/b/%1",
    },
    // L28-31
    WebsiteInfo {
        label: "pobb.in",
        id: "POBBin",
        match_url: "^https://pobb%.in/.+",
        regex_url: "pobb%.in/(.+)%s*$",
        download_url: "pobb.in/pob/%1",
        code_out: Some("https://pobb.in/"),
        post_url: Some("https://pobb.in/pob/"),
        post_fields: Some(""),
        link_url: "pobb.in/%1",
    },
    // L32-35
    WebsiteInfo {
        label: "PoeNinja",
        id: "PoeNinja",
        match_url: "^https://poe%.ninja/?p?o?e?1?/pob/%w+",
        regex_url: "poe%.ninja/?p?o?e?1?/pob/(%w+)%s*$",
        download_url: "poe.ninja/poe1/pob/raw/%1",
        code_out: Some(""),
        post_url: Some("https://poe.ninja/poe1/pob/api/upload"),
        post_fields: Some("code="),
        link_url: "poe.ninja/poe1/pob/%1",
    },
    // L36-38
    WebsiteInfo {
        label: "Pastebin.com",
        id: "pastebin",
        match_url: "^https://pastebin%.com/%w+",
        regex_url: "pastebin%.com/(%w+)%s*$",
        download_url: "pastebin.com/raw/%1",
        code_out: None,
        post_url: None,
        post_fields: None,
        link_url: "pastebin.com/%1",
    },
    // L39
    WebsiteInfo {
        label: "PastebinP.com",
        id: "pastebinProxy",
        match_url: "^https://pastebinp%.com/%w+",
        regex_url: "pastebinp%.com/(%w+)%s*$",
        download_url: "pastebinp.com/raw/%1",
        code_out: None,
        post_url: None,
        post_fields: None,
        link_url: "pastebin.com/%1",
    },
    // L40
    WebsiteInfo {
        label: "Rentry.co",
        id: "rentry",
        match_url: "^https://rentry%.co/%w+",
        regex_url: "rentry%.co/(%w+)%s*$",
        download_url: "rentry.co/paste/%1/raw",
        code_out: None,
        post_url: None,
        post_fields: None,
        link_url: "rentry.co/%1",
    },
    // L41
    WebsiteInfo {
        label: "poedb.tw",
        id: "PoEDB",
        match_url: "^https://poedb%.tw/.+",
        regex_url: "poedb%.tw/pob/(.+)%s*$",
        download_url: "poedb.tw/pob/%1/raw",
        code_out: Some(""),
        post_url: Some("https://poedb.tw/pob/api/gen"),
        post_fields: Some(""),
        link_url: "poedb.tw/pob/%1",
    },
];

/// The final HTTP status of a completion (`easy:getinfo_response_code()`): the last status line
/// of the response headers, 0 without one.
fn response_code(headers: &str) -> f64 {
    headers
        .split(['\r', '\n'])
        .filter_map(|l| pob_ui_core::luastr::lmatch(l, "^HTTP/[%d%.]+ (%d+)"))
        .next_back()
        .and_then(|c| pob_lua::tonumber(&c))
        .unwrap_or(0.0)
}

/// The results of the UploadBuild sub-script (L72-78) for its completion: `page` when the
/// response code is 200, else `nil, page`.
pub fn upload_result(resp: &HttpResponse) -> (Option<String>, Option<String>) {
    let page = resp
        .body
        .as_ref()
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_default();
    // L72
    let res = response_code(&resp.headers);
    // L74
    if res == 200.0 {
        // L75
        (Some(page), None)
    } else {
        // L77
        (None, Some(page))
    }
}

/// Uploads a PoB build code to a website (`websiteInfo`: the postUrl, any postParams, and a
/// prefix to add to the response). Returns the sub-script id; its completion is decoded with
/// [`upload_result`].
/// lua: BuildSiteTools.lua:44-82 `buildSites.UploadBuild(buildCode, websiteInfo)`
/// @pob Modules/BuildSiteTools.lua:44-82 h=bf9460cd
pub fn upload_build(
    cx: &mut Cx,
    build_code: &str,
    website_info: Option<&WebsiteInfo>,
) -> Option<u64> {
    // L48
    let mut response = None;
    // L49
    if let Some(website_info) = website_info {
        // L50-79 the curl sub-script: setopt_url(postUrl) (L55), OPT_POST (L56), the user agent
        // (L57), OPT_POSTFIELDS postFields .. code (L58), ACCEPT_ENCODING "" and FOLLOWLOCATION
        // (L59-60, what Sys::net always does)
        let post_url = website_info
            .post_url
            .unwrap_or_else(|| lua_error!("attempt to concatenate field 'postUrl' (a nil value)"));
        let version = cx.app().launch.version_number.clone().unwrap_or_else(|| {
            lua_error!("attempt to concatenate field 'versionNumber' (a nil value)")
        });
        let post_fields = website_info.post_fields.unwrap_or_else(|| {
            lua_error!("attempt to concatenate field 'postFields' (a nil value)")
        });
        let req = HttpRequest {
            url: post_url.to_owned(),
            headers: Vec::new(),
            body: Some(format!("{post_fields}{build_code}").into_bytes()),
            user_agent: Some(format!("Path of Building/{version}")),
        };
        response = Some(cx.sys.net.download_page(req));
    }
    // L81
    response
}

/// `callback(isSuccess, data, siteCodeURL)`
pub type DownloadBuildCallback = Rc<dyn Fn(&mut Cx, bool, String, Option<String>)>;

/// The `pob:[/\]*<id>[/\]+(.+)` protocol-handler pattern of a site.
fn protocol_pattern(site_info: &WebsiteInfo) -> String {
    format!(
        "^pob:[/\\]*{}[/\\]+(.+)",
        pob_lua::string::lower(site_info.id)
    )
}

/// Downloads a PoB build code from a website (`link`: a link to the site that contains the link
/// to the raw build code; `websiteInfo`: contains the downloadUrl).
/// lua: BuildSiteTools.lua:84-113 `buildSites.DownloadBuild(link, websiteInfo, callback)`
/// @pob Modules/BuildSiteTools.lua:84-113 h=ebdfe86f
pub fn download_build(
    cx: &mut Cx,
    link: &str,
    website_info: Option<&WebsiteInfo>,
    callback: DownloadBuildCallback,
) {
    // L89
    let mut site_code_url: Option<String> = None;
    let mut website_info = website_info.copied();
    // L90 Only called on program start via protocol handler
    // L91
    if website_info.is_none() {
        // L92
        for site_info in &WEBSITE_LIST {
            let pat = protocol_pattern(site_info);
            // L93
            if pob_lua::lmatch(link, &pat, None).is_some() {
                // L94
                site_code_url = Some(
                    pob_lua::gsub(
                        link,
                        &pat,
                        &format!("https://{}", site_info.download_url),
                        None,
                    )
                    .0,
                );
                // L95
                website_info = Some(*site_info);
                // L96
                break;
            }
        }
    } else if let Some(w) = &website_info {
        // L99 called via the ImportTab
        // L100
        site_code_url = Some(pob_lua::gsub(link, w.regex_url, w.download_url, None).0);
    }
    // L102
    if website_info.is_some() {
        let url = site_code_url.clone().unwrap_or_else(|| {
            lua_error!("bad argument #1 to 'DownloadPage' (string expected, got nil)")
        });
        let url2 = url.clone();
        // L103
        download_page(
            cx,
            &url,
            Rc::new(move |cx, response: PageResponse, err_msg: Option<String>| {
                // L104
                if let Some(err_msg) = err_msg {
                    // L105
                    callback(cx, false, err_msg, Some(url2.clone()));
                } else {
                    // L107
                    callback(
                        cx,
                        true,
                        response.body.unwrap_or_default(),
                        Some(url2.clone()),
                    );
                }
            }),
            None,
        );
    } else {
        // L111
        callback(
            cx,
            false,
            "Download information not found".to_owned(),
            site_code_url,
        );
    }
}

/// Parses and converts URI's to import links. Currently only supports protocol handler URI's,
/// extend as needed (`uri`: e.g. `pob://pobbin/<id>` or `pob://poeninja/<id>`).
/// lua: BuildSiteTools.lua:115-129 `buildSites.ParseImportLinkFromURI(uri)`
/// @pob Modules/BuildSiteTools.lua:115-129 h=ac9bdc1a
pub fn parse_import_link_from_uri(uri: &str) -> Option<String> {
    // L118
    let mut import_link = None;

    // L120 Check if it's an URI from protocol handler
    // L121
    for site_info in &WEBSITE_LIST {
        let pat = protocol_pattern(site_info);
        // L122
        if pob_lua::lmatch(uri, &pat, None).is_some() {
            // L123
            import_link =
                Some(pob_lua::gsub(uri, &pat, &format!("https://{}", site_info.link_url), None).0);
            // L124
            break;
        }
    }

    // L128
    import_link
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_urls() {
        let pobbin = &WEBSITE_LIST[2];
        assert!(pob_lua::lmatch("https://pobb.in/abc_D-1", pobbin.match_url, None).is_some());
        assert_eq!(
            pob_lua::gsub(
                "https://pobb.in/abc_D-1",
                pobbin.regex_url,
                pobbin.download_url,
                None
            )
            .0,
            "https://pobb.in/pob/abc_D-1"
        );
        // `%.` in the replacement is a literal dot
        let maxroll = &WEBSITE_LIST[0];
        assert_eq!(
            pob_lua::gsub(
                "https://maxroll.gg/poe/pob/xyz",
                maxroll.regex_url,
                maxroll.download_url,
                None
            )
            .0,
            "https://maxroll.gg/poe/api/pob/xyz"
        );
        let ninja = &WEBSITE_LIST[3];
        for u in [
            "https://poe.ninja/pob/abc",
            "https://poe.ninja/poe1/pob/abc",
        ] {
            assert!(pob_lua::lmatch(u, ninja.match_url, None).is_some(), "{u}");
            assert_eq!(
                pob_lua::gsub(u, ninja.regex_url, ninja.download_url, None).0,
                "https://poe.ninja/poe1/pob/raw/abc"
            );
        }
    }

    #[test]
    fn protocol_links() {
        assert_eq!(
            parse_import_link_from_uri("pob://pobbin/abc").as_deref(),
            Some("https://pobb.in/abc")
        );
        assert_eq!(
            parse_import_link_from_uri("pob:\\\\poeninja\\abc").as_deref(),
            Some("https://poe.ninja/poe1/pob/abc")
        );
        // "pastebin" is tried before "pastebinProxy": pob://pastebinproxy/x does not match
        // "pastebin[/\\]+" (the next character is "p")
        assert_eq!(
            parse_import_link_from_uri("pob://pastebinproxy/x").as_deref(),
            Some("https://pastebin.com/x")
        );
        assert_eq!(parse_import_link_from_uri("https://pobb.in/abc"), None);
    }

    #[test]
    fn upload_results() {
        let ok = HttpResponse {
            body: Some(b"abc".to_vec()),
            error: None,
            headers: "HTTP/1.1 302 Found\r\nLocation: x\r\n\r\nHTTP/1.1 200 OK\r\n\r\n".to_owned(),
        };
        assert_eq!(upload_result(&ok), (Some("abc".to_owned()), None));
        let created = HttpResponse {
            body: Some(b"id".to_vec()),
            error: None,
            headers: "HTTP/2 201 Created\r\n\r\n".to_owned(),
        };
        assert_eq!(upload_result(&created), (None, Some("id".to_owned())));
        let failed = HttpResponse {
            body: None,
            error: Some("connection refused".to_owned()),
            headers: String::new(),
        };
        assert_eq!(upload_result(&failed), (None, Some(String::new())));
    }
}
