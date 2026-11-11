# CCR-0109: U43 ImportTab A seams (main.api, rate limiter, clock, U44/U46 hooks, harness gaps)

Unit: **U43** (ImportTab A: `ImportTab.lua:1-787`, `PoEAPI.lua` 1-238, `BuildSiteTools.lua`
1-129). Status: **pending** (code on `port/U43` follows it). Number reserved with
`scripts/port/next_ccr.sh`.

## 1. `main` fields `MainState` lacks

Evidence: `ImportTab.lua:483-485` (`main.api = new("PoEAPI"):PoEAPI(..)`, read by every account
control and by `PoEAPI:UpdateMain`), `:536`/`:539` (`main.lastExportWebsite`, not the saved
`lastExportedWebsite`), `:244` (`main.lastCharacterHash`), `:353-362` (the global `historyList`,
shared by every ImportTab of the session and aliased by the `siteAccountHistory` dropdown).
`MainState` (U16, `main_mode/mod.rs`) has none of them.

Work-around (U43's files, marked CCR-0109 §1): `tabs/import/core.rs` `MainExtra { api,
last_export_website, last_character_hash, history_list }` in a thread-local tagged with the
app's `main.input_events` allocation (a `Weak`, so a new app on the same thread starts empty);
`main_extra` / `main_extra_mut` read and write it. Proposed (U16):

```diff
 pub struct MainState {
+    /// main.api (ImportTab.lua:483-485; PoEAPI.lua)
+    pub api: Option<crate::tabs::import::poe_api::PoEAPI>,
+    /// main.lastExportWebsite (ImportTab.lua:536; not saved)
+    pub last_export_website: Option<String>,
+    /// main.lastCharacterHash (ImportTab.lua:244, :834, :872; not saved)
+    pub last_character_hash: Option<String>,
+    /// the global `historyList` (ImportTab.lua:353-362, :802-812)
+    pub history_list: Option<crate::tabs::import::core::HistoryList>,
```

## 2. `TradeQueryRateLimiter` (U51, optional and deferred)

Evidence: `PoEAPI.lua:23` (`new("TradeQueryRateLimiter")`), `:198-204`, `:210` (NextRequestTime,
InsertRequest, FinishRequest, UpdateFromHeader). The class is U51's (`trade/**`), which is not
scheduled. Applied: `poe_api::RateLimiter` (the four methods; `now` passed in for the
`os.time()` reads), `PendingRateLimiter` (never delays; ids from 0) and
`set_rate_limiter_factory`. Proposed: U51 implements `RateLimiter` for its port and installs
it with `set_rate_limiter_factory`. Until then the client-side rate limiting of the OAuth
character endpoints is off (a server 429 still reaches the "too fast" status).

## 3. `Platform` has no wall clock and no `SetForeground`

Evidence: `PoEAPI.lua:37`, `:56`, `:94`, `:145`, `:197`; `ImportTab.lua:51`, `:59`, `:179`
(`os.time()`); `PoEAPI.lua:148` (`SetForeground()`). ui doc §8 rule 10 forbids reading a wall
clock directly; the oracle's `os.time()` is `VIRTUAL_EPOCH + floor(GetTime()/1000)`
(determinism.lua). Work-around: `poe_api::os_time(cx)` (the system clock, or the virtual
clock's seconds on wasm32) with `set_os_time` for tests; `SetForeground` does nothing
(DIVERGENCE(set-foreground)). Proposed (U1/U7, `pob-sg/src/sys.rs`):

```diff
 pub trait Platform {
     fn time_ms(&self) -> i64;                                   // GetTime (ms since start)
+    fn os_time(&self) -> i64;                                   // os.time() (headless/oracle: virtual)
+    fn set_foreground(&mut self) {}                             // SetForeground
```

## 4. LuaJIT's `math.random` / `math.randomseed` (CCR-0033 §3)

Applied: `poe_api::lj_random`, a port of LuaJIT 2.1 `lib_math.c` (TW223 generator,
`random_init`, the three call shapes), one state per thread. `poe_api.rs` tests compare it with
`/usr/bin/luajit` (seed 1700000000: `2876347540`, then `14 0 0 7`; seed 0: `0.79420629243124097`,
`17`). Proposed: move it to `pob_lua::math` (U28's TimelessJewelListControl needs it too).

## 5. U44 / U46 methods U43's controls call: `core::ImportHooks`

Evidence: `ImportTab.lua:221-223`, `:420` (`BuildCharacterList`), `:260`, `:704`
(`ImportPassiveTreeAndJewels`), `:301`, `:703` (`ImportItemsAndSkills`), `:366`
(`DownloadSiteCharacterList`), `:435`, `:438` (`DownloadPassiveTree`), `:450`
(`DownloadItems`), `:440`, `:451` (`SetPredefinedBuildName`), `:658`
(`self.build.compareTab:ImportBuild`, U46). Applied: `ImportHooks` with
`default_import_hooks()` (each raises "… is not ported yet (U44|U46)") and
`import_hooks()` / `set_import_hooks(..)`, as `ShellHooks` (CCR-0104 §1). Proposed: U44 and U46
replace their entries in `default_import_hooks` (one line each).

Data shapes U44 builds on: decoded JSON is `poe_api::Json` (`serde_json::Value`; dkjson's
`null` reads as nil through `json_get`); `State::character_list` holds each realm's decoded
`characters` value; `State::last_char_list` the site character list; a `siteCharSelectLeague`
entry's value is `accounts::LeagueEntry { league }`; realm dropdown values are
`&'static accounts::Realm`. The fields U44 writes (`charImportMode`, `charImportStatus`,
`lastRealm`, `lastLeague`, `lastAccountHash`, `lastCharacterHash`, `lastCharList`) are in
`core::State`.

Line ranges: the U43 row names "Draw" and the §12.1 rule puts the `Widget` impl with the
class's `Draw`, so U43 ports `ImportTab.lua:776-787` although the U44 row starts at 776; U43
also ports the globals `HexToChar`/`UrlDecode` (`ImportTab.lua:1913-1924`, §7). U44's range is
therefore `789-1912` and `1925-1932`.

## 6. `common.base64` is not re-exported by `crate::model`

Evidence: `ImportTab.lua:505`, `:632`; `PoEAPI.lua:73`. The port of `runtime/lua/base64.lua`
is calc's `pob_calc::classes::passive_spec::{base64_encode, base64_decode}`, which `model.rs`
(U15) does not re-export. Work-around: direct paths (marked CCR-0109 §6). Proposed:
`pub use pob_calc::classes::passive_spec::{base64_decode, base64_encode};` in
`pob-app/src/model.rs`.

## 7. Helpers U43 now owns (ui doc §12 note 15)

`tabs/import/core.rs`: `url_encode` / `url_decode` (Common.lua:1024-1036, first consumer in
wave order), `UrlDecode` / `HexToChar` (ImportTab.lua:1913-1924), `sha1_hex`
(`common.sha1`, runtime/lua/sha1), `deflate` / `inflate` (SimpleGraphic `l_Deflate` level 9 /
`l_Inflate` zlib stream; miniz_oxide: DIVERGENCE(deflate-bytes), the compressed bytes differ
from zlib's, both decode each other), `encode_build_code` / `decode_build_code`. U52's
`similar_build_list` hook (CCR-0104 §1) can use `encode_build_code` for Build.lua:1598; U16's
`main:Init` protocol import (Main.lua:71-84, still raising in `main_mode/init.rs`) can use
`build_sites::{parse_import_link_from_uri, download_build}` and `decode_build_code`.

## 8. Oracle harness gaps found by `tests/u43_import.rs`

* **pob-oracle `jit.rs` `install_natives`**: `Deflate` uses flate2's default level (SG: 9) and
  `Inflate` falls back to raw deflate and returns `""` on failure (SG: zlib only, `nil, msg`).
  Under the oracle every partial or invalid code reads "Code is valid". The u43 test rebinds the
  two Lua globals to `core::{deflate, inflate}` after boot. Proposed: `install_natives` follows
  `ui_api.cpp:1606-1696`.
* **pob-ui-oracle `runner::Driver::frame`** still drops `Net::poll` on the Rust side (its
  pre-CCR-0026 comment), so no `DownloadPage` completion reaches `App::on_net`. The u43 test
  wraps the Rust side's Net to hold completions back and deliver them at the start of
  `on_frame`. Proposed (U8): `Driver::frame` hands each completion to `app.on_net(id, resp)`
  (with `ScenarioApp: pob_host::App`, which has `on_net`).

## 9. U20: `buildMode:OnFrame` keeps drawing the old build after an Init inside a tab

Evidence: `ImportTab.lua:665-667` ("Import to a new build") calls `self.build:Shutdown()` and
`self.build:Init(..)` from the Import button, i.e. inside `self.importTab:Draw` (Build.lua:1301)
in the middle of `buildMode:OnFrame`. The Lua object persists, so the rest of OnFrame
(Build.lua:1321-1351: overlay panes, top bar, side bar, `DrawControls`) draws the new build's
controls. `build/on_frame.rs` reads `me` (the build node) once at the top; `init::init` creates a
new node, so the rest of that frame draws the old node's controls (u43 `d_new_build`, frame 30:
171 differing draws, e.g. the class dropdown shows the old class; the next frame matches).
Proposed (U20, `on_frame.rs`, after the `draw_tab` call):

```diff
     if let Some(tab) = tab {
         (h.draw_tab)(cx, tab, &tab_view_port, input_events);
     }
+    // the tab may have re-initialised the build (ImportTab.lua:665-667): Lua's `self` persists
+    let Some(me) = cx.app().build.as_ref().map(|b| b.node.id()) else { return };
```

`tests/u43/d_new_build.pobscript` sets that frame aside (`#! u43-set-aside-frame 30`, its own
report, counted in the output) until then.

## 10. U7: POST bodies without a Content-Type

curl's `OPT_POSTFIELDS` (BuildSiteTools.lua:58, Launch.lua:281) sends
`Content-Type: application/x-www-form-urlencoded`; `net_native::download` sends the body with
ureq's `send_bytes`, which sets none. UploadBuild and the OAuth token requests are affected.
Proposed (U7): add the curl default header when the request has a body and no Content-Type
header.

## 11. Applied in other units' files (one arm each, CCR-0106 §2)

`build/init.rs` `default_hooks`: `import_try_fetch_character_list`, `import_tab`, and the
`"Import"` arm of `load_ui_section`; `build/on_frame.rs` `default_shell_hooks`: the
`BuildTab::Import` arm of `draw_tab`, `import_select_code_in`, and the `"Import"` arm of
`save_ui_section`. With them the earlier oracle tests (u19-u22) build and save the real
ImportTab on the Rust side, as their Lua sides always did.
