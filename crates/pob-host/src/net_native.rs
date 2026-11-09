//! Native [`Net`] (ui doc §4.6, §5.2): `launch:DownloadPage` (Launch.lua:259-324) over ureq (the
//! logic of `__mk2_http`, `crates/pob-oracle/src/jit.rs:458-510`), plus the OAuth redirect
//! listener `LaunchServer.lua` 1-213 (lifted from `crates/pob-oracle/src/jit.rs:25-90` and made
//! literal). Owner: U7.
//!
//! SG runs both as `LaunchSubScript` threads; their results reach Lua at the start of a later
//! frame (`SubScriptFrame` → `launch:OnSubFinished`). Here each request runs on its own thread
//! and [`Net::poll`] hands the finished ones to the host, which calls `App::on_net` before
//! `on_frame` (CCR-0026 A).
//!
//! The DownloadPage script (Launch.lua:263-311), step by step:
//! - header lines (`requestHeader:gmatch("[^\r\n]+")`) → request headers ([`HttpRequest::headers`]);
//! - `OPT_USERAGENT "Path of Building/<version>"` → [`HttpRequest::user_agent`];
//! - `OPT_ACCEPT_ENCODING ""` (all encodings curl knows, decoded transparently),
//!   `OPT_FOLLOWLOCATION 1`, POST with `OPT_POSTFIELDS` when there is a body;
//! - `errMsg`: the curl error text, else `"Response code: <code>"` when the code is not 200,
//!   else `"No data returned"` for an empty body; the body and header are returned either way.
//!
//! DIVERGENCE(curl): error texts are ureq's, not curl's; the header string is the final response
//! only (curl's header callback also sees each redirect's headers) and header names are lowercase
//! (ureq normalises them; curl passes them through, lowercase under HTTP/2); no compressed
//! transfer encodings are requested (the body is identical); `connectionProtocol`, `proxyURL`
//! and `noSSL` (Launch.lua:315) are not in the frozen [`HttpRequest`] (CCR-0026 C).

use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pob_sg::{HttpRequest, HttpResponse, Net};

use crate::platform_native::SharedClipboard;
use crate::OAuthResult;

type Done = Arc<Mutex<Vec<(u64, HttpResponse)>>>;

/// Threaded HTTP and OAuth listener.
#[derive(Default)]
pub struct NativeNet {
    next_id: u64,
    in_flight: usize,
    done: Done,
    clipboard: Option<SharedClipboard>,
}

impl NativeNet {
    pub fn new() -> NativeNet {
        NativeNet::default()
    }

    /// The clipboard `LaunchServer.lua` copies the authorization URL into.
    pub fn with_clipboard(clipboard: SharedClipboard) -> NativeNet {
        NativeNet {
            clipboard: Some(clipboard),
            ..NativeNet::default()
        }
    }

    fn alloc_id(&mut self) -> u64 {
        self.next_id += 1;
        self.in_flight += 1;
        self.next_id
    }
}

/// lua: Launch.lua:263-311 (the DownloadPage subscript body).
pub fn download(req: &HttpRequest) -> HttpResponse {
    let mut agent = ureq::AgentBuilder::new()
        .redirects(30) // OPT_FOLLOWLOCATION (curl's default CURLOPT_MAXREDIRS)
        .timeout_connect(Duration::from_secs(300)); // curl's default connect timeout
    if let Some(ua) = &req.user_agent {
        agent = agent.user_agent(ua);
    }
    let agent = agent.build();
    let mut r = if req.body.is_some() {
        agent.post(&req.url)
    } else {
        agent.get(&req.url)
    };
    for line in &req.headers {
        if let Some((k, v)) = line.split_once(':') {
            r = r.set(k.trim(), v.trim());
        }
    }
    let result = match &req.body {
        Some(b) => r.send_bytes(b),
        None => r.call(),
    };
    let (resp, mut err) = match result {
        Ok(resp) | Err(ureq::Error::Status(_, resp)) => (Some(resp), None),
        Err(e) => (None, Some(e.to_string())),
    };
    let mut header = String::new();
    let mut body = Vec::new();
    let mut code = 0u16;
    if let Some(resp) = resp {
        code = resp.status();
        header = format!(
            "{} {} {}\r\n",
            resp.http_version(),
            resp.status(),
            resp.status_text()
        );
        let mut names = resp.headers_names();
        names.dedup();
        let mut seen = std::collections::HashSet::new();
        for n in names {
            if seen.insert(n.clone()) {
                for v in resp.all(&n) {
                    let _ = write!(header, "{n}: {v}\r\n");
                }
            }
        }
        header.push_str("\r\n");
        if let Err(e) = resp.into_reader().read_to_end(&mut body) {
            err = Some(e.to_string());
        }
    }
    // local errMsg; if error then .. elseif code ~= 200 then .. elseif #responseBody == 0 then ..
    let err = err.or_else(|| {
        if code != 200 {
            Some(format!("Response code: {code}"))
        } else if body.is_empty() {
            Some("No data returned".to_owned())
        } else {
            None
        }
    });
    println!(
        "Download complete. Status: {}",
        err.as_deref().unwrap_or("OK")
    );
    HttpResponse {
        body: Some(body),
        error: err,
        headers: header,
    }
}

const COMMON_RESPONSE: &str = r#"HTTP/1.1 200 OK
Content-Type: text/html

<!DOCTYPE html>
<html lang="en">
<head>
	<meta charset="UTF-8">
	<meta name="viewport" content="width=device-width, initial-scale=1.0">
	<title>PoB - Authentication Complete</title>
	<style>
		body {
			font-family: Arial, sans-serif;
			background: #121212;
			color: #fff;
			display: flex;
			justify-content: center;
			align-items: center;
			height: 100vh;
			margin: 0;
		}
		.container {
			display: flex;
			flex-direction: column;
			align-items: center;
		}
		.card {
			background: #1E1E1E;
			padding: 20px;
			border-radius: 10px;
			box-shadow: 0px 4px 10px rgba(255, 255, 255, 0.1);
			width: 90%;
			max-width: 400px;
			text-align: center;
		}
		.card h1 {
			font-size: 24px;
			color: #4CAF50;
			margin-bottom: 10px;
		}
		.card p {
			font-size: 18px;
			margin-bottom: 15px;
		}
		.card code {
			background: #f8d7da;
			color: #721c24;
			padding: 3px 6px;
			border-radius: 4px;
			font-family: monospace;
			display: inline-block;
			margin-bottom: 10px;
		}
		.close-button {
			padding: 10px 20px;
			background: #4CAF50;
			color: white;
			border: none;
			border-radius: 5px;
			cursor: pointer;
			font-size: 16px;
			transition: background 0.3s;
		}
		.close-button:hover {
			background: #45a049;
		}
	</style>
</head>
<body>
	<div class="container">
		<div class="card">
"#;

const COMMON_RESPONSE_END: &str = r"		</div>
	</div>
</body>
</html>
";

/// lua: LaunchServer.lua:5-18 `bindSocket`: `localhost` 49082, else 49083, else 49084; `listen(1)`.
pub fn bind_oauth_listener() -> std::io::Result<TcpListener> {
    let mut last = None;
    for port in [49082u16, 49083, 49084] {
        match TcpListener::bind(("127.0.0.1", port)) {
            Ok(l) => return Ok(l),
            Err(e) => last = Some(e),
        }
    }
    Err(last.expect("three attempts"))
}

/// lua: LaunchServer.lua:143-147: `path:gmatch("([^&=?]+)=([^&=?]+)")`, values `%XX`-decoded.
#[allow(clippy::many_single_char_names)] // the Lua names
fn query_params(path: &str) -> std::collections::HashMap<String, Vec<u8>> {
    let b = path.as_bytes();
    let class = |c: u8| !matches!(c, b'&' | b'=' | b'?');
    let run = |mut i: usize| {
        let s = i;
        while i < b.len() && class(b[i]) {
            i += 1;
        }
        i - s
    };
    let mut out = std::collections::HashMap::new();
    let mut i = 0;
    while i < b.len() {
        let k = run(i);
        if k > 0 && i + k < b.len() && b[i + k] == b'=' {
            let v = run(i + k + 1);
            if v > 0 {
                let key = String::from_utf8_lossy(&b[i..i + k]).into_owned();
                let raw = &b[i + k + 1..i + k + 1 + v];
                // v:gsub("%%(%x%x)", function(hex) return string.char(tonumber(hex, 16)) end)
                let mut dec = Vec::with_capacity(raw.len());
                let mut j = 0;
                while j < raw.len() {
                    if raw[j] == b'%'
                        && j + 2 < raw.len()
                        && raw[j + 1].is_ascii_hexdigit()
                        && raw[j + 2].is_ascii_hexdigit()
                    {
                        let h = std::str::from_utf8(&raw[j + 1..j + 3]).unwrap_or("0");
                        dec.push(u8::from_str_radix(h, 16).unwrap_or(0));
                        j += 3;
                    } else {
                        dec.push(raw[j]);
                        j += 1;
                    }
                }
                out.insert(key, dec);
                i += k + 1 + v;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// The outcome of one `handleConnection`.
enum Handled {
    Retry,
    Done {
        code: Option<String>,
        state: Option<String>,
    },
    /// A Lua error ends the subscript (`OnSubError`).
    Error(String),
}

/// lua: LaunchServer.lua:120-176 `handleConnection`.
fn handle_connection(client: &mut TcpStream, attempt: u32) -> Handled {
    let _ = client.set_read_timeout(Some(Duration::from_secs(5))); // client:settimeout(5)
    let mut line = Vec::new();
    // client:receive("*l"): up to LF, CR dropped
    let read = BufReader::new(&*client).read_until(b'\n', &mut line);
    if let Err(e) = read {
        println!("Attempt {attempt} to handle incoming connection failed: {e}");
        return Handled::Retry;
    }
    if line.last() == Some(&b'\n') {
        line.pop();
    } else {
        // receive("*l") returns nil, "closed" (plus the partial line) without a line end.
        println!("Attempt {attempt} to handle incoming connection failed: closed");
        return Handled::Retry;
    }
    line.retain(|&c| c != b'\r');
    let request = String::from_utf8_lossy(&line).into_owned();
    // request:find("^(%S+)%s(%S+)%s(%S+)")
    let parsed = (|| {
        let b = request.as_bytes();
        let tok = |i: usize| {
            let mut j = i;
            while j < b.len() && !b[j].is_ascii_whitespace() {
                j += 1;
            }
            (j > i).then_some(j)
        };
        let m_end = tok(0)?;
        if m_end >= b.len() || !b[m_end].is_ascii_whitespace() {
            return None;
        }
        let p_end = tok(m_end + 1)?;
        if p_end >= b.len() || !b[p_end].is_ascii_whitespace() {
            return None;
        }
        tok(p_end + 1)?;
        Some((&request[..m_end], &request[m_end + 1..p_end]))
    })();
    let (method, path) = match parsed {
        Some((m, p)) => (Some(m), p),
        None => (None, ""),
    };
    if method != Some("GET") {
        println!(
            "Attempt {attempt} to handle incoming connection received an invalid HTTP request: non-GET method {}",
            method.unwrap_or("nil")
        );
        return Handled::Retry;
    }
    let q = query_params(path);
    let s = |k: &str| q.get(k).map(|v| String::from_utf8_lossy(v).into_owned());
    let (response, code, state) = if q.contains_key("code") {
        (
            format!(
                "{COMMON_RESPONSE}\t\t\t<h1>PoB - Authentication Successful</h1>\n\t\t\t<p>✅ Your authentication is complete! You can now return to the app.</p>\n\t\t\t{COMMON_RESPONSE_END}"
            ),
            s("code"),
            s("state"),
        )
    } else {
        // queryParams["error"] .. ": " .. queryParams["error_description"]
        let (Some(e), Some(d)) = (s("error"), s("error_description")) else {
            return Handled::Error(
                "LaunchServer.lua:161: attempt to concatenate a nil value".to_owned(),
            );
        };
        (
            format!(
                "{COMMON_RESPONSE}\t\t\t<h1>PoB - Authentication Failed</h1>\n\t\t\t<p>❌ Authentication failed. Please try again.</p>\n\t\t\t<code>{e}: {d}</code>\n\t\t\t{COMMON_RESPONSE_END}"
            ),
            None,
            None,
        )
    };
    if attempt != 1 {
        println!("Attempt {attempt} to handle incoming connection received a valid HTTP request");
    }
    let _ = client.write_all(response.as_bytes());
    Handled::Done { code, state }
}

/// lua: LaunchServer.lua:190-213: accept connections until a valid redirect arrives or 60 s
/// pass; `return code, errMsg, state, port`.
pub fn oauth_serve(listener: TcpListener, timeout: Duration) -> OAuthResult {
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    let _ = listener.set_nonblocking(true);
    let stop_at = Instant::now() + timeout;
    let mut attempt = 1;
    let (mut code, mut state) = (None, None);
    let mut should_retry = true;
    while Instant::now() < stop_at && should_retry {
        // server:settimeout(remainingTime); server:accept()
        let client = loop {
            match listener.accept() {
                Ok((c, _)) => break Some(c),
                Err(_) if Instant::now() < stop_at => std::thread::sleep(Duration::from_millis(20)),
                Err(_) => break None,
            }
        };
        if let Some(mut client) = client {
            let _ = client.set_nonblocking(false);
            match handle_connection(&mut client, attempt) {
                Handled::Retry => {}
                Handled::Done { code: c, state: s } => {
                    should_retry = false;
                    code = c;
                    state = s;
                }
                Handled::Error(e) => {
                    return OAuthResult {
                        err: Some(e),
                        ..OAuthResult::default()
                    }
                }
            }
        }
        attempt += 1;
    }
    let err = (Instant::now() >= stop_at)
        .then(|| "Timeout reached without a response received by the local server".to_owned());
    OAuthResult {
        code,
        err,
        state,
        port: Some(port.to_string()),
    }
}

impl Net for NativeNet {
    fn download_page(&mut self, req: HttpRequest) -> u64 {
        let id = self.alloc_id();
        println!("Downloading page at: {}", req.url);
        let done = self.done.clone();
        std::thread::spawn(move || {
            let resp = download(&req);
            if let Ok(mut d) = done.lock() {
                d.push((id, resp));
            }
        });
        id
    }

    fn poll(&mut self) -> Vec<(u64, HttpResponse)> {
        let v: Vec<_> = self
            .done
            .lock()
            .map(|mut d| std::mem::take(&mut *d))
            .unwrap_or_default();
        self.in_flight -= v.len().min(self.in_flight);
        v
    }

    /// lua: LaunchServer.lua (via PoEAPI.lua:111-113 `LaunchSubScript(server, "",
    /// "ConPrintf,OpenURL,Copy", authUrl)`). The bind, `Copy(url)` and `OpenURL(url)` run here on
    /// the main thread (SG forwards those two calls to the main thread through `OnSubCall`); the
    /// accept loop runs on a thread. A failed bind is the subscript's `assert` error
    /// (`OnSubError`).
    fn oauth_listen(&mut self, url: &str) -> Option<u64> {
        let id = self.alloc_id();
        let listener = match bind_oauth_listener() {
            Ok(l) => l,
            Err(e) => {
                let r = OAuthResult {
                    err: Some(e.to_string()),
                    ..OAuthResult::default()
                };
                if let Ok(mut d) = self.done.lock() {
                    d.push((id, r.into_response()));
                }
                return Some(id);
            }
        };
        let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
        println!("Server started on 127.0.0.1:{port}");
        let redirect_uri = format!("http://localhost:{port}");
        println!("Redirect URI: {redirect_uri}");
        let url = format!("{url}&redirect_uri={redirect_uri}");
        println!("Authorization URL copied to clipboard: {url}");
        if let Some(cb) = &self.clipboard {
            cb.copy(&url);
        }
        if let Err(e) = crate::platform_native::open_url(&url) {
            eprintln!("OpenURL: {e}");
        }
        let done = self.done.clone();
        std::thread::spawn(move || {
            let r = oauth_serve(listener, Duration::from_secs(60));
            if let Ok(mut d) = done.lock() {
                d.push((id, r.into_response()));
            }
        });
        Some(id)
    }

    fn busy(&self) -> bool {
        self.in_flight > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_parsing() {
        let q = query_params("/?code=a%2Fb&state=0f&x=&=y&error_description=Bad%20thing%");
        assert_eq!(q.get("code").map(Vec::as_slice), Some(&b"a/b"[..]));
        assert_eq!(q.get("state").map(Vec::as_slice), Some(&b"0f"[..]));
        assert!(!q.contains_key("x")); // empty value: no match
        assert_eq!(
            q.get("error_description").map(Vec::as_slice),
            Some(&b"Bad thing%"[..])
        );
        let q = query_params("/cb?a=1=2&b=3");
        assert_eq!(q.get("a").map(Vec::as_slice), Some(&b"1"[..]));
        assert_eq!(q.get("b").map(Vec::as_slice), Some(&b"3"[..]));
    }

    fn serve_with(requests: Vec<&'static str>) -> OAuthResult {
        let l = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        let client = std::thread::spawn(move || {
            let mut bodies = Vec::new();
            for r in requests {
                let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
                s.write_all(r.as_bytes()).unwrap();
                let mut b = String::new();
                let _ = s.read_to_string(&mut b);
                bodies.push(b);
            }
            bodies
        });
        let r = oauth_serve(l, Duration::from_secs(10));
        let bodies = client.join().unwrap();
        // A Lua error ends the subscript (OnSubError): no return values, so no port.
        if !r.err.as_deref().is_some_and(|e| e.contains("concatenate")) {
            assert_eq!(r.port.as_deref(), Some(port.to_string().as_str()));
        }
        if r.code.is_some() {
            assert!(bodies.last().unwrap().contains("Authentication Successful"));
        }
        r
    }

    #[test]
    fn oauth_listener() {
        // a non-GET probe first, then the redirect
        let r = serve_with(vec![
            "POST / HTTP/1.1\r\n\r\n",
            "GET /?code=abc%21&state=0123456789abcdef HTTP/1.1\r\nHost: x\r\n\r\n",
        ]);
        assert_eq!(r.code.as_deref(), Some("abc!"));
        assert_eq!(r.state.as_deref(), Some("0123456789abcdef"));
        assert_eq!(r.err, None);
        // a denied authorization: no code, no error message, no retry
        let r = serve_with(vec![
            "GET /?error=access_denied&error_description=no HTTP/1.1\r\n\r\n",
        ]);
        assert_eq!((r.code, r.err, r.state), (None, None, None));
        // error without description: the concatenation errors (OnSubError)
        let r = serve_with(vec!["GET /?error=x HTTP/1.1\r\n\r\n"]);
        assert!(r.err.unwrap().contains("concatenate"));
    }

    #[test]
    fn oauth_timeout() {
        let l = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let r = oauth_serve(l, Duration::from_millis(100));
        assert_eq!(
            r.err.as_deref(),
            Some("Timeout reached without a response received by the local server")
        );
        assert!(r.code.is_none() && r.port.is_some());
    }

    /// DownloadPage against a local one-shot HTTP server: status line + headers, body, and the
    /// errMsg rules (non-200 code, empty body).
    #[test]
    fn download_page_local() {
        let serve = |status: &'static str, body: &'static str| {
            let l = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let port = l.local_addr().unwrap().port();
            let h = std::thread::spawn(move || {
                let (mut s, _) = l.accept().unwrap();
                let mut req = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    let n = s.read(&mut buf).unwrap();
                    req.extend_from_slice(&buf[..n]);
                    if n == 0 || req.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let resp = format!(
                    "HTTP/1.1 {status}\r\nX-Rate-Limit-Policy: p\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                s.write_all(resp.as_bytes()).unwrap();
                String::from_utf8_lossy(&req).into_owned()
            });
            (format!("http://127.0.0.1:{port}/x"), h)
        };
        let (url, h) = serve("200 OK", "hello");
        let r = download(&HttpRequest {
            url,
            headers: vec!["Content-Type: application/json".into()],
            body: Some(b"{}".to_vec()),
            user_agent: Some("Path of Building/2.67.2".into()),
        });
        let req = h.join().unwrap();
        assert!(req.starts_with("POST /x HTTP/1.1"));
        assert!(req.contains("User-Agent: Path of Building/2.67.2"));
        assert!(req
            .to_ascii_lowercase()
            .contains("content-type: application/json"));
        assert_eq!(r.body.as_deref(), Some(&b"hello"[..]));
        assert_eq!(r.error, None);
        assert!(r.headers.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(r.headers.contains("x-rate-limit-policy: p\r\n"));
        assert!(r.headers.ends_with("\r\n\r\n"));
        let (url, h) = serve("404 Not Found", "nope");
        let r = download(&HttpRequest {
            url,
            headers: vec![],
            body: None,
            user_agent: None,
        });
        h.join().unwrap();
        assert_eq!(r.error.as_deref(), Some("Response code: 404"));
        assert_eq!(r.body.as_deref(), Some(&b"nope"[..]));
        let (url, h) = serve("200 OK", "");
        let r = download(&HttpRequest {
            url,
            headers: vec![],
            body: None,
            user_agent: None,
        });
        h.join().unwrap();
        assert_eq!(r.error.as_deref(), Some("No data returned"));
        // transport error
        let r = download(&HttpRequest {
            url: "http://127.0.0.1:1/".into(),
            headers: vec![],
            body: None,
            user_agent: None,
        });
        assert!(r.error.is_some() && r.body.as_deref() == Some(&b""[..]));
    }

    #[test]
    fn net_ids_and_busy() {
        let mut n = NativeNet::new();
        assert!(!n.busy());
        let id = n.download_page(HttpRequest {
            url: "http://127.0.0.1:1/".into(),
            headers: vec![],
            body: None,
            user_agent: None,
        });
        assert_eq!(id, 1);
        assert!(n.busy());
        let t = Instant::now();
        let mut got = Vec::new();
        while got.is_empty() && t.elapsed() < Duration::from_secs(20) {
            got = n.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, 1);
        assert!(!n.busy());
    }
}
