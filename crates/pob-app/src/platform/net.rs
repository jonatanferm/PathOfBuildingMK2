//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Launch.lua:259-324 (launch:DownloadPage over Sys::net).
//! Owner: U16.
//!
//! @pob-file Launch.lua @16de4b82d
//!
//! The Lua runs the curl sub-script of L261-314 through `LaunchSubScript`; the port hands the
//! request to `Sys::net` (pob-host's ureq/fetch implementations do the transfer) and gets the
//! completion back through `App::on_net` -> `launch::on_net` (CCR-0026 A).
//!
//! DIVERGENCE(http-options): `HttpRequest` has no field for `connectionProtocol`, `proxyURL` or
//! `noSSL` (CCR-0026 C, pending), so PoB's IP-version, proxy and no-SSL options (L283-292) are not
//! passed on.
//! DIVERGENCE(bytes): Lua response bodies are byte strings; [`PageResponse`] holds them as UTF-8
//! (invalid bytes become U+FFFD). Every PoB caller downloads text (JSON, build codes, XML).

use std::rc::Rc;

use pob_sg::HttpRequest;
use pob_ui_core::prelude::*;

use crate::app::CxApp;
use crate::launch::SubScript;

/// The `response` table a DownloadPage callback receives: `{ header = .., body = .. }` (both nil
/// after a sub-script error).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageResponse {
    pub header: Option<String>,
    pub body: Option<String>,
}

/// `params = { header, body }` (L258).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DownloadParams {
    pub header: Option<String>,
    pub body: Option<String>,
}

/// Download the given page in the background, and call `callback(response, errMsg)` when done.
/// lua: Launch.lua:255-324 launch:DownloadPage(url, callback, params)
/// @pob Launch.lua:255-324 h=55c0448e
pub fn download_page(
    cx: &mut Cx,
    url: &str,
    callback: Rc<dyn Fn(&mut Cx, PageResponse, Option<String>)>,
    params: Option<DownloadParams>,
) {
    // L260
    let params = params.unwrap_or_default();
    // L261-314 the sub-script: request headers split on CR/LF (L268-274), the user agent
    // (L276), a POST body (L279-282)
    let headers: Vec<String> = params
        .header
        .as_deref()
        .map(|h| {
            h.split(['\r', '\n'])
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let version = cx.app().launch.version_number.clone().unwrap_or_else(|| {
        lua_error!("attempt to concatenate field 'versionNumber' (a nil value)")
    });
    let req = HttpRequest {
        url: url.to_owned(),
        headers,
        body: params.body.map(String::into_bytes),
        user_agent: Some(format!("Path of Building/{version}")),
    };
    // L315
    let id = cx.sys.net.download_page(req);
    // L316-323
    cx.app_mut().launch.sub_scripts.insert(
        id,
        SubScript::Download(Rc::new(move |cx, response, err_msg| {
            // L320 callback({header=responseHeader, body=responseBody}, errMsg)
            callback(cx, response, err_msg);
        })),
    );
}
