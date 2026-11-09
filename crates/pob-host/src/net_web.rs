//! Web [`Net`] over `fetch` (ui doc §5.2, §13 risk 6: CORS limits import, trade and OAuth;
//! wasm32 only). Owner: U7.
//!
//! `launch:DownloadPage` (Launch.lua:259-324) with the browser's `fetch` in place of the curl
//! subscript: each request is a `spawn_local` task whose result [`Net::poll`] hands to the host,
//! which calls `App::on_net` before `on_frame` (CCR-0026 A), as on desktop. The response rules
//! are Launch.lua:296-306: `errMsg` is the transport error, else `"Response code: <code>"`
//! when the code is not 200, else `"No data returned"` for an empty body; the body and the
//! header text are returned either way.
//!
//! DIVERGENCE(browser fetch): `User-Agent` and other forbidden headers cannot be set (the
//! browser sends its own); redirects are followed by the browser and only the final response's
//! headers are visible; a CORS-blocked or offline request fails with the browser's `TypeError`
//! text; the status line is synthesised as `HTTP/1.1 <code> <statusText>`. `oauth_listen` is
//! unavailable (no listening sockets): `LaunchSubScript` returns nil.

use std::cell::{Cell, RefCell};
use std::fmt::Write as _;
use std::rc::Rc;

use pob_sg::{HttpRequest, HttpResponse, Net};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

/// `fetch`-based DownloadPage.
#[derive(Default)]
pub struct WebNet {
    next_id: u64,
    in_flight: Rc<Cell<usize>>,
    done: Rc<RefCell<Vec<(u64, HttpResponse)>>>,
}

fn js_err(e: &JsValue) -> String {
    if let Some(err) = e.dyn_ref::<js_sys::Error>() {
        String::from(err.message())
    } else {
        e.as_string().unwrap_or_else(|| format!("{e:?}"))
    }
}

/// lua: Launch.lua:263-311 (the DownloadPage subscript body) over `fetch`.
pub async fn download(req: HttpRequest) -> HttpResponse {
    let mut header = String::new();
    let mut body = Vec::new();
    let mut code = 0u16;
    let result: Result<(), JsValue> = async {
        let init = web_sys::RequestInit::new();
        init.set_method(if req.body.is_some() { "POST" } else { "GET" });
        let headers = web_sys::Headers::new()?;
        for line in &req.headers {
            if let Some((k, v)) = line.split_once(':') {
                // Forbidden names are silently dropped by the browser; others may throw.
                let _ = headers.append(k.trim(), v.trim());
            }
        }
        init.set_headers(&headers);
        if let Some(b) = &req.body {
            init.set_body(&js_sys::Uint8Array::from(b.as_slice()));
        }
        let request = web_sys::Request::new_with_str_and_init(&req.url, &init)?;
        let w = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
        let resp: web_sys::Response = JsFuture::from(w.fetch_with_request(&request))
            .await?
            .dyn_into()?;
        code = resp.status();
        header = format!("HTTP/1.1 {} {}\r\n", resp.status(), resp.status_text());
        let entries = js_sys::try_iter(&resp.headers())?
            .ok_or_else(|| JsValue::from_str("headers are not iterable"))?;
        for e in entries {
            let pair = js_sys::Array::from(&e?);
            let _ = write!(
                header,
                "{}: {}\r\n",
                pair.get(0).as_string().unwrap_or_default(),
                pair.get(1).as_string().unwrap_or_default()
            );
        }
        header.push_str("\r\n");
        let buf = JsFuture::from(resp.array_buffer()?).await?;
        body = js_sys::Uint8Array::new(&buf).to_vec();
        Ok(())
    }
    .await;
    // local errMsg; if error then .. elseif code ~= 200 then .. elseif #responseBody == 0 then ..
    let err = match result {
        Err(e) => Some(js_err(&e)),
        Ok(()) if code != 200 => Some(format!("Response code: {code}")),
        Ok(()) if body.is_empty() => Some("No data returned".to_owned()),
        Ok(()) => None,
    };
    web_sys::console::log_1(
        &format!(
            "Download complete. Status: {}",
            err.as_deref().unwrap_or("OK")
        )
        .into(),
    );
    HttpResponse {
        body: Some(body),
        error: err,
        headers: header,
    }
}

impl Net for WebNet {
    fn download_page(&mut self, req: HttpRequest) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.in_flight.set(self.in_flight.get() + 1);
        web_sys::console::log_1(&format!("Downloading page at: {}", req.url).into());
        let done = self.done.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let resp = download(req).await;
            done.borrow_mut().push((id, resp));
        });
        id
    }
    fn poll(&mut self) -> Vec<(u64, HttpResponse)> {
        let v = std::mem::take(&mut *self.done.borrow_mut());
        self.in_flight
            .set(self.in_flight.get() - v.len().min(self.in_flight.get()));
        v
    }
    fn busy(&self) -> bool {
        self.in_flight.get() > 0
    }
}
