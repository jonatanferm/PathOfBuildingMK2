# CCR-0134: U54 web calc client (WorkerCalc) — the D15 worker protocol and its seams

Unit: **U54** (ui doc §12 row U54, optional; file `crates/pob-app/src/calc/worker_web.rs`).
Status: **pending** (code on `port/U54` follows it). Number reserved with `scripts/port/next_ccr.sh`.

Calc doc D15 says that on wasm32 "the calc runs in a dedicated Web Worker that the UI restarts after
an abort", and ui doc §10.4 says `WorkerCalc` "returns `Pending` until the worker answers". Neither
doc defines the protocol, what the worker sends back, or how the call sites behave on `Pending`.
This CCR records what U54 built and the seams in other units' files.

## 1. `calc/mod.rs`: `worker_web` compiled on every target

Evidence: `crates/pob-app/src/calc/mod.rs` (UI-U0) declared `#[cfg(target_arch = "wasm32")] pub mod
worker_web;`. The client state machine and the worker's request handler do not depend on the
browser. Compiling them only for wasm would leave them untested (`cargo test` cannot run wasm
here: there is no wasm-bindgen test runner in the toolchain).

Change (applied, marked `// CCR-0134 §1`): the module is declared without the `cfg`. Its browser
glue (`web` submodule: `web_sys::Worker`, fetches, the worker's `#[wasm_bindgen]` exports) is
`#[cfg(target_arch = "wasm32")]` inside the file. Its desktop stand-in (`ThreadHub`, worker threads)
is `#[cfg(not(target_arch = "wasm32"))]`.

## 2. The protocol (new)

The messages are JSON strings. The full table is in the module docs of `worker_web.rs`.

* client → worker: `{"op":"build_output","seq":n,"xml":…}` with `build:SaveDB()` of the live model.
  The worker runs `loadBuildModelFromXML`, then `wipeGlobalCache()`, then
  `skillsTab:UpdateSocketGroups()`, then `calcsTab:BuildOutput()` (Build.lua:691-695, 1260-1266).
* worker → client: `ready`. The answer `{"seq","ok":true,"stats"}` carries the numeric top-level
  entries of `mainOutput`. `{"seq","ok":false,"error"}` is the `LuaError` text, posted by the
  worker's panic hook before the abort. After that come `dead` (the worker's script caught the
  trap and closed the worker) and `bootError`.
* One request is in flight at a time. Newer XML waits in a one-slot queue, and the newest XML wins.

## 3. What crosses the boundary: verdict + stats, not the env (deviation from "the calc runs in a worker")

Evidence: the UI reads the calc's object graph directly (ui doc §10.4: `build.calcsTab.mainEnv.player.output`,
`.breakdown`, `.modDB`, `mainSkill.skillFlags`, `node.finalModList`). That graph shares objects
with the live model. For example, `LuaCalcBridge` (`crates/pob-ui-oracle/src/lua_calc.rs:16-40`)
has to re-point socket groups, gem instances and items after injecting. The only encoder/injector
pair for these types is calc:ORC-X's `pob_calc::probe::export_root` (feature `probe`) plus
`pob_oracle::inject` (`crates/pob-oracle/src/inject.rs`). It cannot be built for wasm, because
`pob-oracle` links mlua/LuaJIT (AMENDMENTS rule 3).

What U54 does: the worker acts as a **pre-flight**. For a model whose XML the worker calculated
without an error, the client runs the same `power::BuildOutput` on the live model and returns
`Ready`. It then cross-checks the worker's `stats` against the live `mainOutput` (`WorkerStatus::mismatches`;
relative 1e-9). The test corpus shows 0 differences. A Lua error kills only the worker, and the UI
keeps the build's last good results. That is the D15 property, the reason the worker exists. The
cost is that every recalc runs twice and the game data is loaded in both threads, so the page's
thread is not freed during a calc.

Proposed (future, not blocking): move `inject` (pure Rust over pob-calc) out of `pob-oracle` into a
wasm-buildable crate, and turn on `probe` export for the web worker. `WorkerCalc` can then adopt
the worker's pob-state/1 documents as `LuaCalcBridge::adopt_build_output` does, and stop running
the calc on the main thread.

## 4. Call sites that cannot take `Pending` (the first calc runs on the main thread)

Evidence: `crates/pob-app/src/build/init.rs:950-956` ignores the result of the first
`calc.build_output` (Build.lua:695) and calls `refresh_stat_list` right after it (L696).
`build/stats.rs:137-144` (`main_env`) then raises "attempt to index field 'mainEnv' (a nil value)"
when there is no env. `tree_view/draw.rs:632-655` has the same `lua_error!`. On wasm that error
aborts the page. The facade contract (ui doc §10.4: "Call sites handle Pending with the placeholder
documented at each site") is not met at these sites.

Work-around (in U54's file, `// CCR-0134 §4`): while `calcsTab.mainEnv` is nil, `WorkerCalc::build_output`
calculates on the main thread and returns `Ready`. This covers the build's first calc and nothing
else. Each later recalc goes through the worker, and on `Pending` it keeps the previous results.

Proposed for U19/U21/U23: give PoB-style placeholders when `mainEnv` is nil (Init: skip
`RefreshStatList` until the first `Ready`, and recalculate when it arrives; stats/draw: draw no
values). U54 then removes the work-around.

## 5. Showing the calc error (DIVERGENCE(worker-error))

Evidence: Launch.lua:112-118 (`PCall(main.OnFrame)` → `ShowErrMsg("In 'OnFrame': %s")`). The
worker's error reaches the client in a JS task, outside any frame and without a `Cx`. The facade
methods have no `Cx` either.

Now: the client logs the error to the console and keeps it in `WorkerStatus::errors`. Proposed: add
`fn take_lua_error(&mut self) -> Option<String> { None }` to `CalcFacade`. Build OnFrame (U20) would
call it after `build_output` and pass a `Some` to `launch::show_err_msg(cx, "In 'OnFrame': %s", …)`.

## 6. `app/pob-port-web/src/lib.rs` (U7): the worker entry and the factory

Evidence: the worker loads the page's own wasm-bindgen module, so the page's `#[wasm_bindgen(start)]`
also runs in the worker. `run_web` then fails with "no window" and logs to the console. `PobApp::new`
uses `RustCalc`, and nothing on the web initialised `pob_gamedata`, so Launch failed with
"pob_gamedata::init not called".

Change (applied, marked `// CCR-0134 §6`): `start()` returns at once inside a worker
(`worker_web::in_worker()`). With `?app=pob` it runs `worker_web::init_web_data(<data>)`, which
fetches `<data>/data.bin` and the latest tree from `pob-dump compile`'s output (`?data=<url>`,
default `pob/dump`), and then runs `PobApp::with_calc(parts, || WorkerCalc::new(web_spawner(…)))`.
The worker is started from a Blob URL as a module worker. It imports the glue URL given by
`WebWorkerConfig::glue_url`. When that is `None`, it uses the page's
`<link rel="modulepreload" href="….js">` (which Trunk adds), else the first
`<script type="module" src>`, else `./pob-port-web.js`.

Not covered: the main thread prefetches only the latest tree version, so a build on an older tree
is U53's (ui doc §12 "wasm loads the build list and tree"). The worker fetches the versions each
request's XML names.

## Verification (U54)

* `scripts/port/cargo.sh check -p pob-app --target wasm32-unknown-unknown` and `-p pob-port-web`:
  both clean, and clippy shows no warnings in the U54 files.
* `scripts/port/cargo.sh test -p pob-app --test u54_worker_calc -- --test-threads=1`: 4 passed.
  The tests are: headless `PobApp` with `WorkerCalc` on `ThreadHub` (the sidebar shows the new
  numbers once the worker answers), the facade round trip, the client state machine on a scripted
  port, and a Lua error on a worker thread followed by the restart. `--lib calc::worker_web`: 1
  passed (protocol round trip).
* Browser, run by hand: debug wasm built with wasm-bindgen-cli 0.2.129 (`--target web`), headless
  Chromium 153 with WebGPU on SwiftShader, driven over CDP, page `?app=pob`. The new build first
  showed Total Life 60 (level 1). After three clicks on Level "+", the sidebar still showed 60 while
  the request was pending. Then the page console logged
  `WorkerCalc: result epoch 2 (worker Life Some(96.0))`, and the sidebar showed Total Life 96
  (level 4). That run needed a `pob/web-prefetch.txt` listing `Assets/`, `TreeData/3_19`,
  `TreeData/legion`, `TreeData/3_29` and `TreeData/*.png` (§6, U53).
