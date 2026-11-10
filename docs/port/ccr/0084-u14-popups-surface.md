# CCR-0084 (U14): popup surface amendments, and two SimpleGraphic functions `Sys` lacks

Unit: U14 (Popups). Status: proposed. Number: next free at the time of writing (0080 and 0081
are taken; renumber on collision, the code comments say `CCR-0084`). Every item is worked around
inside U14's own files (`crates/pob-ui-core/src/popups.rs`), marked `// CCR-0084` where the code
stands in for a missing contract.

## 1. `open_cloud_error_popup` needs the `launch` fields (ui doc §6.10, frozen)

Evidence: `Modules/Main.lua:1764`
`local version = "^8v"..launch.versionNumber..(launch.versionBranch and " "..launch.versionBranch or "")..(launch.devMode and " (dev)" or "")`
and `:1768` `"... then restart "..APP_NAME.." and try again."` (`APP_NAME`: `Launch.lua:9`). The
`launch` global is pob-app's `LaunchState` (`crates/pob-app/src/launch.rs`, U16), which
pob-ui-core cannot see; `MainCore` (U4) has no such fields.

Owner amendment (no callers exist yet; the Lua callers are `Main.lua:527`, `:690`,
`BuildListHelpers.lua:23`, `Build.lua:2226`, all U16 or later):

```diff
-pub fn open_cloud_error_popup(cx: &mut Cx, file_name: &str);
+/// The `launch` fields and `APP_NAME` that main:OpenCloudErrorPopup reads.
+#[derive(Clone, Debug)]
+pub struct LaunchInfo { pub app_name: String, pub version_number: String, pub version_branch: Option<String>, pub dev_mode: bool }
+pub fn open_cloud_error_popup(cx: &mut Cx, file_name: &str, launch: &LaunchInfo) -> Ctrl<PopupDialog>;
```

It returns the popup, as the Lua does (`return self:OpenPopup(...)`). `file_name` stays `&str`:
every caller passes a concatenated string, so the `fileName or "?"` / `fileName and ... or ""`
nil branches are dead. Alternative: U4 adds the four values to `MainCore` and the signature
returns to the frozen one; U16 then fills them in `main:Init`.

## 2. `draw_popup_layer` (Main.lua:437-443)

U14's row lists `Main.lua:437-443 (popup layer and dim)`, which is a block inside
`main:OnFrame` (U16's). U14 ports it as

```rust
/// lua: Main.lua:437-443
pub fn draw_popup_layer(cx: &mut Cx, vp: &ViewPort);
```

and U16's `main:OnFrame` calls it in place of the block (with `self.viewPort`). The input half,
`Main.lua:402-407` (`self.popups[1]:ProcessInput(..)` then `wipeTable(self.inputEvents)`), stays
U16's; it is three lines around `popups::popup_process_input`.

## 3. `GetCloudProvider` has no `Sys` counterpart (to U1/U7)

Evidence: `Main.lua:1761` `local provider, _, status = GetCloudProvider(fileName)`;
`_SimpleGraphic.def.lua:333-335` documents `return nil, nil, nil`. SimpleGraphic implements it on
Windows only (Cloud Files API). Neither `pob_sg::Platform` nor `pob_sg::Sys` has it, and the
oracle's `sg_bridge.rs` does not bind it, so real PoB under the oracle raises
`attempt to call global 'GetCloudProvider' (a nil value)` the first time a cloud-error popup opens
(the U14 galleries define the def-file stub in Lua, `lua/gallery/_u14.lua`).

Local workaround: `popups::get_cloud_provider` returns `(None, None, None)` (marked `CCR-0084`),
which is the value on every platform the port targets. Proposed: U1/U7 add
`fn cloud_provider(&self, path: &str) -> (Option<String>, Option<String>, Option<i64>)` to
`Platform` with that default, and U8 binds `GetCloudProvider` in `sg_bridge.rs` (returning
nothing); then `get_cloud_provider` calls it.

## 4. Lua `MakeDir` semantics are not one `Fs::make_dir` contract (to U1/U7)

Evidence: `Main.lua:1738-1742` uses both results of `MakeDir(path..newFolderName)` (`res, msg`;
the message goes into the error popup). SimpleGraphic's `l_MakeDir` (ui_api.cpp:1768-1786, as
`crates/pob-ui-oracle/src/sg_bridge.rs:1178-1199` models it) is `std::filesystem::create_directory`:
not recursive; an existing directory gives `false` with no error, i.e. `nil, strerror(0)` =
`nil, "Success"`; an error gives `nil, strerror(ec)`. The `Fs::make_dir` impls disagree on the
existing-directory case: `pob-host/src/fs_native.rs` (`create_dir`, Err "File exists"),
`pob-host/src/headless.rs:392` and `pob-host/src/lib.rs:571` `MemFs` (Err "File exists"),
`pob-ui-oracle/src/boot.rs:215` `OracleFs`
(`create_dir_all`: Ok, and recursive).

Local workaround (`popups::make_dir`, marked `CCR-0084`): an exact-name `Fs::search(path, true)`
first (an existing directory gives `Err("Success")`), then `Fs::make_dir`, with the io error text
minus its " (os error N)" suffix (the bridge's `strerror`). Proposed: U1 states the contract on
`Fs::make_dir` (not recursive; `AlreadyExists` for an existing entry) and adds
`Sys::make_dir_lua(&mut self, path) -> Result<(), String>` with the SG results above; U7/U8 make
their impls follow it; then `popups::make_dir` forwards to it.

## 5. `main:WrapString` is in (closes CCR-0073 D)

`crate::wrap::wrap_string` (`@pob Modules/Main.lua:1795-1820`) is implemented. U10 can replace
its local copy (`tooltip::tooltip::wrap_string`, CCR-0073 D) with a call to it. The port returns a
fresh `Vec` per call where Lua returns the one shared `wrapTable` (DIVERGENCE(alias) in
`wrap.rs`; no PoB caller keeps the result across a second call).

## 6. Gallery notes (U8, informational)

- The gallery mini-launch (`lua/gallery_launch.lua`) does not set `APP_NAME`; `_u14.lua` sets it
  (Launch.lua:9's value) next to the `GetCloudProvider` stub of item 3.
- Popups are centred on `main.screenW/screenH`, and the two acceptance screens have different
  virtual heights (800 and 720), so one `.pobscript` cannot click inside a popup at both.
  `u14_mouse` pins `main.screenH = 720` on both sides after the launch sets it; `u14_keys` runs
  at the real size and only uses the keyboard inside popups. A per-screen `move` (or a `#!`
  coordinate macro) in the script language would remove the pin.
