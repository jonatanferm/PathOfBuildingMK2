# CCR-0026: U7 host contract notes (Net completions decision, OAuth/HttpRequest gaps, `.pobscript` placeholders, pacing)

Unit: **U7** (host). Status: **pending** (code on `port/U7` already follows it; the ui doc does not yet).
Sources checked: SimpleGraphic `3b1a346` (`ui_main.cpp`, `sys_video.cpp`, `sys_main.cpp`, `ui_console.cpp`),
PoB v2.67.2 `Launch.lua`, `LaunchServer.lua`, `Classes/PoEAPI.lua`.

## A. Net completions reach the app through `App::on_net` (decides CCR-0013 A6, ui doc §13 item 11)

SG runs subscript completions inside `Frame`, **after** `renderer->BeginFrame()` and
`GetRelativeCursor`, **before** `OnFrame` (`ui_main.cpp:393-417`: BeginFrame, cursor, the
`SubScriptFrame` loop → `launch:OnSubFinished`/`OnSubError`, then `PushCallback("OnFrame")`).
`launch:OnSubFinished` (Launch.lua) is a callback of its own, protected by its own `PCall`, so an
error in it does not skip `OnFrame`. Choosing an `App` method keeps that structure (one guarded
callback per completion, D17) instead of burying the poll inside the port of `launch:OnFrame`.

Frozen code change (`crates/pob-host/src/lib.rs`, done on `port/U7`):

```rust
pub trait App: 'static {
    // ... unchanged ...
    /// A `Net` request finished (`launch:OnSubFinished(id, ...)` / `OnSubError`): called by the
    /// host inside the frame, after `sg.begin_frame` and before `on_frame`, once per completion
    /// in `Net::poll` order. The default drops it.
    fn on_net(&mut self, id: u64, resp: pob_sg::HttpResponse) { let _ = (id, resp); }
}
```

All hosts share `pob_host::run_frame` (desktop, web, headless), so the order is identical everywhere.
`wants_frames` (pacing) is `App::wants_frames() || sys.net.busy()` (`hasSubscript`).

Doc diff, ui doc §5.2 "Frame order":

```diff
-- **Frame order.** Poll `Net` and deliver completions, then `sys.begin_frame`, `sg.begin_frame`, `on_frame`, `sg.end_frame`, prepare/render, present. `App` has no completion entry point yet and UI-U0's desktop host does not poll `Net`: U7 (with U16) picks either `fn on_net(&mut self, id: u64, resp: pob_sg::HttpResponse)` on `App` or "the port of `launch:OnFrame` polls `sys.net` first" (CCR-0013 A6, §13 item 11).
+- **Frame order** (`pob_host::run_frame`, `ui_main.cpp:393-433`). `sg.begin_frame`, `sys.begin_frame` (cursor snapshot), then each `Net::poll` completion to `App::on_net(id, resp)` (one guarded callback each; the port of `launch:OnSubFinished`), then `on_frame`, `sg.end_frame`, prepare/render, present (CCR-0026 A).
```

and §5.2 `App` gains the `on_net` line above; §13 item 11 becomes "decided: `App::on_net` (CCR-0026 A)".
U16 implements `on_net` as the port of `launch:OnSubFinished`/`OnSubError` (its `subScripts[id]` table).

## B. OAuth completions travel as an `HttpResponse`

`Net::oauth_listen(url) -> Option<u64>` is frozen, but the frozen `Net::poll` returns only
`HttpResponse`s, while `LaunchServer.lua:213` returns `code, errMsg, state, port` (consumed at
`PoEAPI.lua:111-130`). Work-around on `port/U7` (no frozen signature changed):
`pob_host::OAuthResult { code, err, state, port }` with `into_response`/`from_response`:
`body` = code, `error` = errMsg, `headers` = JSON `{"state": .., "port": ..}`. U43 decodes with
`OAuthResult::from_response`. Proposed doc note for §4.6 under `oauth_listen`:

```diff
     fn oauth_listen(&mut self, url: &str) -> Option<u64> { let _ = url; None } // desktop only
+    // Its completion arrives through poll() as pob_host::OAuthResult::into_response() (CCR-0026 B).
```

A Lua error inside `LaunchServer.lua` (e.g. `:161` concatenating a nil `error_description`)
ends the subscript (`OnSubError`): `err` = the message, no `port`.

## C. `HttpRequest` lacks `connectionProtocol`, `proxyURL`, `noSSL`

`Launch.lua:315` passes `self.connectionProtocol, self.proxyURL, self.noSSL` to the DownloadPage
subscript (`OPT_IPRESOLVE`, `OPT_PROXY`, `OPT_SSL_VERIFYPEER/HOST`, Launch.lua:283-292). The frozen
`HttpRequest { url, headers, body, user_agent }` cannot carry them, so the native `Net` ignores
PoB's proxy / IP-version / no-SSL options (marked in `net_native.rs`). Proposed (additive; U1 owns `sys.rs`):

```diff
-pub struct HttpRequest { pub url: String, pub headers: Vec<String>, pub body: Option<Vec<u8>>, pub user_agent: Option<String> }
+pub struct HttpRequest { pub url: String, pub headers: Vec<String>, pub body: Option<Vec<u8>>, pub user_agent: Option<String>,
+                         pub connection_protocol: Option<i32>, pub proxy_url: Option<String>, pub no_ssl: bool }
```

U7 would then map them onto ureq (`proxy`, TLS verification off, resolver family).

## D. Web image loader (resolved: U3 merged)

Interim (before U3 merged): `crates/pob-host/src/web.rs` loaded images through
`loader_headless::HeadlessLoader` over the files it prefetched. Now `run_web` builds
`WebLoader::with_vfs(<pob_src>/, vfs)` over the same VFS that `WebFs` and the font `AssetSource`
read; the `pob/web-prefetch.txt` manifest is fetched with `WebLoader::prefetch`, and ASYNC images are
fetched lazily with one decode per frame. No contract change remains from this section.

## E. `App::on_script_step` and the `.pobscript` placeholders (§13 item 12: `script::{Script, Step}` are U7's)

`load-build`, `snapshot` and app-specific `fixture` keys can only be executed by the app, and both
`run_headless` and the oracle need to hand them over at the scripted point. Added to `App` (default no-op):

```rust
/// A `.pobscript` step only the app can perform (`load-build`, `snapshot`, and `fixture`
/// keys the host does not know), called by `run_headless` between frames.
fn on_script_step(&mut self, step: &pob_host::script::Step) { let _ = step; }
```

Placeholder amendments: `Script { steps, base_dir: Option<PathBuf> }` (fixture paths resolve against the
script's directory; `Script::load(path)`), `Step::Exit` (`exit`: `CanExit`, then `OnExit`), and
`script::Action` + `Script::expand()` (the primitive actions every runner replays; the oracle (U8) should
drive its LuaJIT side from `expand()` so both sides see identical callback sequences). The exact
expansion table is in `crates/pob-host/src/script.rs` (module docs); highlights that refine §5.2:
`click K` = frame, KeyDown, frame, KeyUp, frame; `type` delivers one OnChar per codepoint as SG would
(`\b \t \r \x1b` as KeyDown+Char+KeyUp of BACK/TAB/RETURN/ESCAPE; other control characters dropped;
U+00A0..U+00AA as key names per CCR-0021; other non-ASCII `?`); `move X Y` is in virtual px
(physical = `lround(v * scale)`); mouse KeyDowns get SG's double-click detection on the virtual clock.
Headless fixtures: `userdir=DIR` (copy-on-write overlay, writes recorded as `SideEffect::FileWrite`),
`net=FILE.json` (scripted DownloadPage responses delivered at the start of the next frame).

## F. Pacing: 11 boot frames; swallowed console toggle

- `ui_main.cpp:383-391` tests `framesSinceWindowHidden <= 10` before incrementing, so **11** frames
  always run after boot (ui doc §2 "Frame loop" says 10). Also, an unfocused window sleeps 100 ms after
  every frame even while hovered (`ui_main.cpp:437-440`); only the skip test looks at the hover state.
  `pob_host::pacing` follows the C++.
- `ui_console.cpp:101-104`: a KeyDown of `` ` `` or F1 with Ctrl held toggles SG's console and is not
  delivered to Lua. The port has no console; the KeyDown is swallowed (DIVERGENCE note in `input.rs`)
  so Lua sees what it sees in SG with the console closed. Proposed addition to the §2 "Swallowed keys" row:
  "and KeyDown of `` ` ``/F1 with Ctrl held (console toggle, `ui_console.cpp:101-104`)".

## G. Desktop loop mechanics (no contract change; recorded for reviewers)

§5.2 says "`ControlFlow::Poll` while focused". The desktop host instead requests a redraw as soon as the
next frame is due and waits otherwise (`WaitUntil` the 100 ms idle sleep), presenting with `AutoVsync`:
same frame cadence as SG (which also blocks in `SwapBuffers`), without busy-polling winit.
`Restart()` on desktop runs `OnExit` and re-executes the binary (`run_desktop` takes its factory by `FnOnce`).
