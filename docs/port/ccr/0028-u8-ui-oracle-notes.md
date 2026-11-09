# CCR-0028: U8 (ui-oracle) contract notes

**Status: proposed** (unit U8, ui doc §11.2, §12.3). Items 1-2 change code outside U8's files only
through the change the unit table already grants U8 (item 1); everything else is worked around
inside `crates/pob-ui-oracle` and marked `CCR-0028` there. The orchestrator decides items 2-5 and
10 (with U7: the open points of 10 are U7's to accept or reject) and amends the doc text for items
6-9 and 11-13.

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

## 5. Run directories and `GetFileModifiedTime` determinism (applied in U8; revised by item 10)

A run's user dir (`GetUserPath()`) is a fresh copy of the `userdir` fixture DIR (U7's rule: PoB
reads `DIR/Path of Building/`, so `fixtures/user1/Path of Building/{Settings.xml,Builds/}`), or an
empty directory without one. Both sides see the same copy (`runner::UserDir`, reset between the
sides). `GetFileModifiedTime` follows U7's headless `Fs`: 0 for every file not written during the
run, and for a written file MSVC `file_clock` ticks of 2020-09-13T12:26:40Z (Unix 1 600 000 000)
plus the virtual clock at the write (`sg_bridge::virtual_write_ticks`). The Lua side learns of its
writes from `lua/determinism.lua`'s `io.open` wrapper ("w"/"a" modes → `__oracleNoteWrite`), the
Rust side from `OracleFs::write`. So "Sort by Last Edited" depends neither on checkout or copy time
nor on the machine. (The first revision stamped every copied file with mtime 1 700 000 000 and
read the disk; the Rust side's search reported 0, so the two sides disagreed, and files written
during a run carried the wall clock.)

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

## 7. `.pobscript` notes (doc text for §5.2; U7 owns the parser; revised by item 10)

- `fixture net=FILE.json` and `fixture userdir=DIR` follow U7 (CCR-0026 E, `port/U7`
  `pob_host::headless`), see item 10 (a), (b). Lua side: the bridge's `LaunchSubScript`
  (light-userdata slot ids, completion in the next frame's sub-script pass before `OnFrame`,
  ui_main.cpp:403-412; `body: null` → `launch:OnSubError`). Rust side: `boot::ScriptedNet`.
- Lines `#! expect-text TEXT` / `#! expect-snapshot NAME` are comments for the parser and
  expectations for `tests/parity.rs`.
- While U7's `Script::parse` is a stub, `runner::parse_script` uses a local parser of U7's grammar
  (`runner::parse_script_local`: optional `dpi`, optional `wheel` count, U7's escapes; `exit`
  is rejected until `Step::Exit` exists); once U7 lands, U7's parser is used automatically.

## 8. `ui-parity` CLI and the harness (doc text for §11.2/§11.3)

- Extra flags: `--screen WxH`, `--audit`, `--lua-only`, `--out DIR`, `--name NAME`. `--dpi 1.5`
  alone selects 1920×1080.
- Audit mode runs the scenario in two child processes (`--audit-child N`; LuaJIT's string hash is
  seeded per process, calc doc D8) and writes the union to `audit.txt`
  (`host<TAB>inversions<TAB>example`).
- `tests/parity.rs` is a custom harness (`harness = false`), so `cargo test -p pob-ui-oracle --
  <unit>` selects the unit's cases by name. A filter that matches no case runs nothing and
  passes (libtest's rule, so a filter meant for the lib's unit tests does not fail it); it prints
  "running 0 parity cases" and the case count. Scenario directories whose name starts with `_`
  are Lua-side-only acceptance runs (`scenarios/_smoke`).
- A run fails when PoB shows its error prompt (`launch.promptMsg` starting with `^1Error`, i.e. a
  Lua error that PoB's own `PCall` caught and passed to `launch:ShowErrMsg`, Launch.lua:381-388),
  not only on errors that escape `PCall` (`oracle/error_prompt_fails_run`).

## 9. PoB nondeterminism found by the determinism check (information for the tree units)

Two processes running `scenarios/_smoke/boot_build.pobscript` (an open build on the Tree tab)
produce the same calls per frame, but in a different order at these sites (string-keyed `pairs`,
calc doc D8), plus the order of the `ImageLoad` effects of the frame that opens the build:
`Classes/PassiveTreeView.lua:725, 736, 828, 881, 958, 963, 965, 1018, 1302`. The units porting
PassiveTreeView (and the item/slot image loads) need §8 rule 6 (a) or (b) resolutions for them;
`tests/parity.rs` `oracle/determinism/boot_build` regenerates the list
(`target/ui-parity/det_boot_build_run1/order_sites.txt`). The galleries are byte-identical across
processes; the build list has identical draw calls, and only the order of the `ImageLoad` effects
of `OnInit` (the passive tree loads its sprites while iterating string-keyed tables), which belong
to frame 0 since item 10, differs.

**§8 rule 6 is contradicted for `pairs(spec.nodes)` (doc text; affects U23 and the other tree
units).** Rule 6 says integer-keyed model maps are `LjIntMap`, which "iterates exactly like
LuaJIT, so `pairs(spec.nodes)` / `allocNodes` draw order matches with no multiset". Sites 828, 881,
958, 963 and 965 are draw calls directly inside `for nodeId, node in pairs(spec.nodes)`
(PassiveTreeView.lua:816; no inner loop), and their order differs between two processes, so the
iteration order of `spec.nodes` itself differs. Cause: `LjIntMap` reproduces LuaJIT's order only for
a given insertion order, and the insertion order is not fixed. `spec.nodes` is filled by iterating
`pairs(self.tree.nodes)` (PassiveSpec.lua:51-61); `tree.nodes` comes from the tree data's table
constructor, which holds the string key `"root"` next to the integer node ids
(TreeData/3_27/tree.lua:12063-12064; removed only later, PassiveTree.lua:510). That string's hash
is seeded per process (calc doc D8), so it moves the integer keys' hash-node slots, and with them
the order of `pairs(tree.nodes)`, the insertion order into `spec.nodes`, and so `pairs(spec.nodes)`.
Proposed text for rule 6: "`LjIntMap` matches LuaJIT's order for a given insertion order; where the
insertion order comes from a `pairs` over a table that ever held a string key (as `spec.nodes`,
filled from `tree.nodes` with its `root` key), the order is per-process random, and the sites use
(b), a multiset bucket." The tree units then need (b) at PassiveTreeView.lua:828, 881, 958, 963,
965 (and audit any other loop over `spec.nodes`, `tree.nodes` or `allocNodes`), not plain
`LjIntMap` order.

## 10. The oracle's driver is a transitional stand-in for `pob_host::run_headless` (DEVIATION from §5.2; reconciliation with CCR-0026)

**Deviation.** ui doc §5.2 makes `run_headless` the headless driver "used by tests and by
`ui-parity`", and CCR-0026 E asks the oracle to drive its LuaJIT side from U7's
`Script::expand()`. At U8's base `run_headless` is `todo!()`, and the frozen `App` has neither
`on_script_step` (load-build, snapshot; CCR-0026 E) nor `on_net` (CCR-0026 A). U8 therefore drives
both sides with its own `runner::drive` (+ `Driver`, `ScenarioApp`, `UserDir`, `runner::fixtures`,
`sg_bridge::NetFixtures`, `boot::ScriptedNet`). This is transitional: it is deleted when U7
merges (recipe below). Until then it follows U7's rules, and a test keeps it honest.

**Agreed rules: U7's, as on `port/U7` (ae9d002, `crates/pob-host/src/{headless,script}.rs`).** The
first U8 revision disagreed with U7 on these; U8 now follows U7 on all of them:

| | U8 first revision | agreed (U7), U8 now |
|---|---|---|
| (a) `fixture net=` | JSON map `{url: {body, error, header}}` | JSON array `[{url, body \| null, body_file, error, headers}]`; a request takes the first unused fixture for its URL, else the last one for it; unknown URL: empty body, `error = "No net fixture for <url>"`; `body: null` is a sub-script error (Lua side: `launch:OnSubError`) |
| (b) fixture paths, `userdir` | relative to `fixtures/`; DIR was copied to `<run>/Path of Building/` | relative to the script's directory (`Script::base_dir`; U8: `Target::Scenario { base_dir }`, `runner::load_script`); `GetUserPath()` is DIR, PoB reads `DIR/Path of Building/` (fixture moved to `fixtures/user1/Path of Building/`); no `userdir`: an empty user dir |
| (c) file times | every fixture file stamped 1 700 000 000 | 0 for files not written during the run; written files: ticks of Unix 1 600 000 000 + clock (item 5) |
| (d) cursor | starts at (-1, -1); double clicks from the start, `< 500 ms` | starts at (0, 0); double-click detection off until the first `move`; `<= 500 ms`, ±5 px |
| (e) frame order | no `Net` poll on the Rust side | `sg.begin_frame`, `sys.begin_frame` (cursor), `Net::poll` → `App::on_net` per completion, `on_frame`, `sg.end_frame`, clock += 16. U8 polls at that point and drops the completions until `on_net` exists (its default also drops); the Lua side runs its sub-script pass at the same point; a request completes at the next poll |
| (f) `load-build`, `snapshot` | private `ScenarioApp` | `App::on_script_step` (after the U7 merge; see the recipe) |
| `move X Y` | physical `floor(v * dpi)`, then a frame | `round(v * scale)` with scale = the DPI override or the screen's; no frame |
| `click K` | KeyDown, frame, KeyUp, frame | frame, KeyDown, frame, KeyUp, frame |
| `type` | `\n` as RETURN; non-ASCII → `?` | `\b \t \r \x1b` as KeyDown+Char+KeyUp; other control characters dropped; U+00A0..U+00AA as SG key names; other non-ASCII `?` |
| `wheel` | KeyDown/KeyUp with key state | the callbacks only; count optional (1) |
| KeyDown `` ` ``/F1 with Ctrl | delivered | swallowed (SG console toggle, CCR-0026 F) |
| leading `screen` | applied after `OnInit` | applied before `OnInit` |
| effects between frames | dropped by `begin_frame` | belong to the next frame; after the last frame to the last frame |
| parser | `dpi` and wheel count required; lax escapes | U7's grammar |

**Verification.** `runner::tests::drive_follows_u7_expansion` pins `drive` to U7's expansion table
(every rule above that is observable through `App`). `runner::tests::drive_matches_run_headless`
replays the same script through `pob_host::run_headless` as soon as that is implemented (it is
skipped while it is `todo!()`) and fails on any difference in the callback sequence, clock, cursor
or frame count, so the two drivers cannot drift apart unnoticed. Checked against `port/U7`'s
`pob-host` (checked out into the U8 worktree for the test, then reverted): U8 compiles against it
(`Script::base_dir`, `Step::Exit` are handled), U7's `Script::parse` is picked up, and the drift test
passes.

**Open points (proposed to U7; until decided the oracle keeps its behaviour):**

1. *Screen before `OnInit`.* SG creates the window before Lua's `OnInit`, so `GetScreenSize()` in
   `OnInit` is the window size. `drive` runs one untraced empty frame before `on_init` for that;
   `run_headless` calls `on_init` on a fresh `Sg`. Proposed: U7 does the same (no trace changes).
2. *Listing order.* U8 sorts `NewFileSearch` results by the upper-cased name (NTFS order, SG's
   reference platform; item 4); U7's `sort_file_entries` by the lower-cased name. They differ
   for names containing `[ \ ] ^ _ `` ` `` next to letters (`a_b` vs `aZ`). Proposed: upper case.
3. *Fixtures after the first frame.* U7 applies a `fixture` step where it stands; the oracle
   copies the user dir before boot and rejects a `fixture` step after the leading block.
4. *Side-effect order within a frame.* U7 appends the platform's effects (Copy, OpenUrl,
   WindowTitle, Screenshot, Exit, FileWrite) after the frame's `Sg` effects; the bridge records
   them inline, in call order. Identical unless one frame mixes `ImageLoad` with a platform call.
5. *Errors.* `run_headless` logs a callback error and returns the frames so far; the oracle fails
   the run (also on PoB's own error prompt, item 8), via `LuaApp::take_error` after the run.
6. `exit` needs `Step::Exit` (rejected by the local parser until U7 lands).

**Switch-over recipe (at the U7 merge; owner: U8 follow-up, checked by the drift test).**

1. `impl App for LuaApp`: `on_script_step` (`LoadBuild(path)`: read `GetUserPath()/Path of
   Building/<path>`, `main:SetMode("BUILD", false, stem, xml)`; `Snapshot(name)`: push
   `SaveDB` into the LuaApp's snapshot list) and `on_net(id, resp)` (the bridge's
   `LaunchSubScript` calls `sys.net.download_page` and maps the returned id to its slot;
   `on_net` calls `launch:OnSubFinished`/`OnSubError` for that slot). The audit bookkeeping and
   the error-prompt check of `after_frame` move to the end of `LuaApp::on_frame`.
2. `run_with` builds each side's app and calls `pob_host::run_headless(&mut app, &script)` with the
   `userdir` fixture rewritten to the absolute path of the side's `UserDir` copy (the Lua side
   writes through LuaJIT's `io`, so it must not see the fixture itself). Snapshots and errors are
   read from the apps afterwards.
3. The bridge stops recording the platform effects that U7's `HeadlessPlatform` records (keep
   `ImageLoad` until item 2 lands) and keeps `__oracleNoteWrite` for the Lua side's write times
   (`HeadlessFs` only sees writes made through `Fs::write`).
4. Delete `Driver`, `drive`, `ScenarioApp`, `runner::fixtures`, `runner::char_key`,
   `sg_bridge::NetFixtures` and `boot::ScriptedNet`; the drift test becomes redundant.

## 11. Patch 6 stamps creation order in a side table, not in `object.__seq` (doc text)

§11.2 patch 6 says "`Common.lua:158 new()` stamps `object.__seq`". `lua/markers.lua` instead records
the sequence number in a weak-keyed side table (`__stampSeq(object)`, read by `__canonPairs` and
`__seqOf`). Reason: a field on the object would appear in `pairs(object)` and in anything that
copies or serialises PoB objects; the side table changes nothing PoB can see. Proposed doc text:
"`Common.lua:158 new()` records the object's creation sequence number (oracle side table, no field
on the object)."

## 12. Patch 1 also fixes `os.time(table)` and records write times (doc text)

`os.time{...}` is `mktime`, i.e. local time, so a trace could depend on the machine's time zone;
`lua/determinism.lua` converts the table as UTC (LuaJIT's field rules: `sec`/`min` default 0,
`hour` 12, `day`/`month`/`year` required, read in that order; out-of-range fields normalised). It
also wraps `io.open` to note files opened for writing (item 5). Proposed addition to patch 1: "and
`os.time(t)` interprets `t` as UTC".

## 13. T3 runs on a local copy of U1's `normalize`/`diff` until U1 lands (scheduling)

`diff_report.rs` carries a local implementation of `pob_sg::trace::{normalize, diff}`'s documented
semantics (for example the viewport rule `ceil(h * scale) == 0`) and switches to U1's functions
automatically once they are implemented (`diff_report::uses_u1_diff`). Until then a gallery can pass
against a slightly different reading of the semantics. Every gallery and scenario result of
`tests/parity.rs` names the comparison used ("T3 by pob_sg::trace (U1)" or "... local copy
(U1-PENDING)"). Proposed: U1 lands before any widget unit's gallery is gated, and the local copy is
deleted then.
