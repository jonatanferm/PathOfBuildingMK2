# CCR-0028: U8 (ui-oracle) contract notes

**Status: proposed** (unit U8, ui doc §11.2, §12.3). Items 1-2 change code outside U8's files only
through the change the unit table already grants U8 (item 1); everything else is worked around
inside `crates/pob-ui-oracle` and marked `CCR-0028` there. The orchestrator decides items 2-5 and
amends the doc text for items 6-9.

## 1. `pob_oracle::jit::{PRELUDE, install_natives}` are `pub` (applied; planned by §11.2 step 2)

Evidence: ui doc §11.2 step 2 ("it gets a `pub fn install_natives` and a `pub` prelude,
coordinated as a CCR"); AMENDMENTS A1 (pob-luahost is `crates/pob-oracle`).

Applied in `crates/pob-oracle/src/jit.rs`:

```diff
-const PRELUDE: &str = r##"
+pub const PRELUDE: &str = r##"
...
-fn install_natives(lua: &Lua) -> mlua::Result<()> {
+pub fn install_natives(lua: &Lua) -> mlua::Result<()> {
```

No behaviour change. `pob-ui-oracle/src/boot.rs` inserts its source-patch hook into the prelude's
`loadfile` (it asserts the anchor `local src = f:read("*a")\n    f:close()\n` exists exactly once, so a
change to the prelude fails the oracle loudly instead of silently skipping the patches).

## 2. Side effects are not recorded by `pob-sg` (proposed; U1 + U3)

Evidence: ui doc §4.6 `SideEffect::{ImageLoad, WindowTitle, Copy, OpenUrl, FileWrite, Screenshot,
Exit}` and §11.1 T3 ("plus side effects"); `crates/pob-sg/src/trace.rs` `TraceRecorder::effect`
exists, but nothing in pob-sg (rust-port, nor the U1 `api.rs`/`sys.rs` or U3 `image.rs` branches)
calls it. The Rust side of a scenario therefore records no effects, while the oracle's Lua side
must (they are part of T3), so every frame that loads an image would diverge.

Proposed:

```diff
 // pob-sg/src/image.rs (U3)
 pub fn load(&self, sg: &mut Sg, path: &str, flags: ImageFlags) {
     sg.images.load(self, path, flags);
+    if let Some(t) = &mut sg.trace { t.effect(SideEffect::ImageLoad(ImgRef { path: path.into(), flags })); }
 }
 // pob-sg/src/api.rs (U1): one entry point for the app's platform glue
+pub fn record_effect(&mut self, e: SideEffect) { if let Some(t) = &mut self.trace { t.effect(e); } }
```

and the Rust callers of `Platform::{copy, set_window_title, open_url, take_screenshot, exit}`
(pob-app's platform module, U16+) call `sg.record_effect(..)` next to them.

Workaround (`sg_bridge.rs`): the bridge records the effect for the Lua side **only if `Sg` did not
record one during the call** (`effect_unless_recorded`), so nothing is counted twice once this lands.

## 3. `FileWrite` is recorded on neither side (open)

Evidence: §4.6 `SideEffect::FileWrite { path, xxh3 }`. PoB writes through Lua's `io.open` (the
prelude's), the port through `Sys::fs`. Recording it needs an `io.open` write-handle proxy on the Lua
side and a hook in `Fs::write` on the Rust side; T4 snapshots already compare the build XML.
Proposed: drop `FileWrite` from T3 until a unit needs it, or assign it to U16 (Settings.xml) with
U8 adding the Lua proxy then.

## 4. pob-oracle's `NewFileSearch` ignores `findDirectories` (bug, proposed fix; owner of pob-oracle)

Evidence: `crates/pob-oracle/src/jit.rs:408` `|lua, (pattern, _dirs): (String, Option<bool>)|`:
directories and files are both returned for either kind of search, case-sensitively and without
SG's `dirOnly` filter (SG: `ui_api.cpp:1341-1378` `NewFileSearch` + `NextFile` skip entries whose
`isDirectory != dirOnly` and `.`/`..`; `sys_main.cpp:139-245` case-insensitive glob). In PoB this
lists every build file a second time as a folder (`BuildListHelpers.lua:89` `NewFileSearch(..
"*", true)` -> `">> name.xml"` rows, `BuildListControl.lua:204`).

Workaround (`sg_bridge.rs` `NewFileSearch`, `find_files`, `glob_match`): the oracle overrides the
global with SG's semantics (dirOnly filter, `.`/`..` skipped, case-insensitive `*`/`?` glob,
`GetFileModifiedTime` = MSVC `file_clock` ticks, 100 ns since 1601, `GetFileSize` 0 for
directories). DIVERGENCE: SG returns `directory_iterator` order; the oracle sorts by upper-cased
name (NTFS order) for determinism. U7's `NativeFs::search` (todo at this base) should use the
same order. Proposed: pob-oracle's owner switches `__mk2_file_search` to the same rules (the
legacy app's build list has the same bug).

## 5. Run directories and `GetFileModifiedTime` determinism (applied in U8)

A run's user dir is a fresh copy of `fixtures/<dir>` whose files and directories get the fixed
mtime `1700000000` (`runner::FIXTURE_MTIME_UNIX`), so "Sort by Last Edited" and anything else that
reads `GetFileModifiedTime` does not depend on checkout or copy time. The Rust side's `Fs` must
see the same copy (it does: both sides use `runner::UserDir`).

## 6. Gallery contract (doc text; applied in U8)

§11.2 names `lua/gallery/<name>.lua` and `src/gallery/<name>.rs` but not their interfaces. As built
(`src/gallery.rs` module doc):

- `lua/gallery/<name>.lua` returns a table `G` with optional `G:Init()` and required
  `G:OnFrame(inputEvents, viewPort)`; it runs after `lua/gallery_launch.lua` (Common, Global,
  Main's methods and `main:Init`'s defaults; `main.inputEvents` collected from the key callbacks
  and wiped after each frame).
- `src/gallery/<name>.rs` exports `pub fn app(parts: pob_host::PlatformParts) -> Box<dyn
  pob_host::App>`; build.rs generates `GALLERIES` and `rust_app(name, parts)`. A file named
  `_<x>.rs` is a shared support module, not a gallery.
- Optional `lua/gallery/<name>.pobscript` drives it (default `frames 3`).

## 7. `.pobscript` additions (doc text for §5.2; U7 owns the parser)

- `fixture net=FILE.json` (relative to `fixtures/`): scripted `launch:DownloadPage` responses
  `{"<url>": {"body": .., "error": .. | null, "header": ..}}` (patch 8). Lua side: the bridge's
  `LaunchSubScript` (light-userdata slot ids, completion in the next frame's sub-script pass before
  `OnFrame`, ui_main.cpp:403-412). Rust side: `boot::ScriptedNet` (`poll` returns a request one
  frame later); the App still has no completion entry point (§13 item 11).
- Lines `#! expect-text TEXT` / `#! expect-snapshot NAME` are comments for the parser and
  expectations for `tests/parity.rs`.
- While U7's `Script::parse` is a stub, `runner::parse_script` uses a local parser of the
  documented grammar (`runner::parse_script_local`); once U7 lands, U7's parser is used.

## 8. `ui-parity` CLI and the harness (doc text for §11.2/§11.3)

- Extra flags: `--screen WxH`, `--audit`, `--lua-only`, `--out DIR`, `--name NAME`. `--dpi 1.5`
  alone selects 1920×1080.
- Audit mode runs the scenario in two child processes (`--audit-child N`; LuaJIT's string hash is
  seeded per process, calc doc D8) and writes the union to `audit.txt`
  (`host<TAB>inversions<TAB>example`).
- `tests/parity.rs` is a custom harness (`harness = false`), so `cargo test -p pob-ui-oracle --
  <unit>` selects the unit's cases by name. Scenario directories whose name starts with `_` are
  Lua-side-only acceptance runs (`scenarios/_smoke`).

## 9. PoB nondeterminism found by the determinism check (information for the tree units)

Two processes running `scenarios/_smoke/boot_build.pobscript` (an open build on the Tree tab)
produce the same calls per frame, but in a different order at these sites (string-keyed `pairs`,
calc doc D8), plus the order of the `ImageLoad` effects of the frame that opens the build:
`Classes/PassiveTreeView.lua:725, 736, 828, 881, 958, 963, 965, 1018, 1302`. The units porting
PassiveTreeView (and the item/slot image loads) need §8 rule 6 (a) or (b) resolutions for them;
`tests/parity.rs` `oracle/determinism/boot_build` regenerates the list
(`target/ui-parity/det_boot_build_run1/order_sites.txt`). The build list and the galleries are
byte-identical across processes.
