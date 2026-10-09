# CCR-0013: UI-U0 contract corrections and additions (ui doc §4-§9)

Unit: UI-U0 (ui doc §12.2 U0). Status: proposed; every item is already applied locally in the
UI-U0 stubs (marked `CCR-0013` where it touches code), so the orchestrator only needs to amend the
doc text. Nothing here changes calc-plan crates.

## A. Contract gaps found while freezing the stubs

### A1. `ImageData` needs the sampler flags (§4.5 vs §5.1)

Evidence: ui doc §4.5 `pub struct ImageData { width, height, levels }`; §4.3
`Frame::uploads: Vec<(TexId, Rc<ImageData>)>`; §5.1 "samplers[8] indexed by CLAMP|NEAREST|MIPMAP".
The backend only receives `Frame`, so it cannot know CLAMP/NEAREST for a texture.

Applied (`crates/pob-sg/src/image.rs`, `ImageData`):

```diff
-pub struct ImageData { pub width: u32, pub height: u32, pub levels: Vec<Vec<u8>> /* RGBA8, mip 0.. */ }
+pub struct ImageData { pub width: u32, pub height: u32, pub levels: Vec<Vec<u8>> /* RGBA8, mip 0.. */,
+                       pub flags: ImageFlags }
```

### A2. `ImageHandle::unload(&self)` cannot release a texture

Evidence: §4.5 `pub fn unload(&self);` has no `Sg`/registry access, while SG refcounts shared
textures (`RegisterShader`, r_main.cpp:1366) and `Frame::releases` exists. UI-U0: `unload` clears the
shared slot only; `releases` stays empty (textures live until exit). Proposed (U3 decides):

```diff
-    pub fn unload(&self);
+    pub fn unload(&self, sg: &mut Sg);   // drops one registry reference; 0 => Frame::releases
```

### A3. The `ImageRegistry` surface (§4.5 leaves the body as a comment)

Frozen by UI-U0 (`crates/pob-sg/src/image.rs`): `new(Box<dyn ImageLoader>)` (registers `@white`,
`@black`, default as TexId 0/1/2 with pending uploads), `insert_ready(name, ImageData) -> TexId`
(font atlases, built-ins), `load(&ImageHandle, path, flags)`, `pump()`, `status(id)`,
`source(id) -> Option<(&str, ImageFlags)>` (trace `ImgRef`), `tex_size(id)`, `image_size(id)` (the
blocking emulation through `ImageLoader::read_size`), `set_priority`, `take_uploads`,
`take_releases`. Free functions `decode_image`, `read_image_size`, `build_mips`, `default_texture`.

### A4. Trace recording of image arguments (§4.2 reference body)

Evidence: §4.2 `self.rec(|| ApiCall::draw_image(img, [x, y, w, h], Some([l, t, r, b])))` passes the
handle, but `ImgRef { path, flags }` needs the registry. Applied: `ApiCall::draw_image(img:
Option<ImgRef>, rect, tc)`; `Sg` resolves the `ImgRef` through `ImageRegistry::source` before
recording. `TraceRecorder` (named in §4.2, undefined) is frozen in `crates/pob-sg/src/trace.rs`
(`begin_frame/end_frame/record/effect/ctl_push/ctl_pop/set_src`, `frames`, `cur`).

### A5. Small additions to frozen types

- `Sys::new(platform, fs, net)` (§4.6 has private fields and no constructor).
- `Sg::viewport()`, `Sg::current_draw_color()` (test/inspection accessors), `FontSet::empty()`
  (tests and tools without PoB assets: measuring returns 0, drawing applies escapes only),
  `FontFace::heights()`, `Align::from_lua`/`lua_name`, `Font::index`, `Quad::is_culled` (moved
  `DrawCmd::is_culled`), `frame::LayerSink` and `frame::digest`.
- `AssetSource` (§3 names it only): `read_src(path)` / `read_runtime(path)`.
- `Sg::set_draw_color*` check `renderEnable` like `ui_api.cpp:821`.

### A6. Host: Net completions have no App entry point (§5.2)

Evidence: §5.2 "Frame order. Poll `Net` and deliver completions, then ..." but `App` has no
callback for completions. UI-U0's desktop host does not poll Net. Proposed (U7/U16 pick one):
`fn on_net(&mut self, id: u64, resp: pob_sg::HttpResponse)` on `App`, or document that
`launch:OnFrame`'s port polls `sys.net` first.

## B. Reference-port text that does not compile against the frozen crates

`crates/pob-ui-core/tests/compile_shape.rs` compiles §9.1 and §9.2 verbatim except for these lines.

### B1. §9.2 imports `pob_lua::{gsub, lmatch}`

Evidence: `crates/pob-lua/src/pattern.rs:711` `gsub(s, p, repl, n) -> (String, usize)`;
`pattern.rs:564` `r#match(s, p, init) -> Option<Captures>` (re-exported as `lmatch`). §8 rule 5
(expression position keeps the first value) is what §9.2 needs. Applied: `pob_ui_core::luastr::{gsub,
lmatch}` (`gsub(s, p, r) -> String`, `lmatch(s, p) -> Option<String>`; U5 owns the file).

```diff
-use pob_lua::{gsub, lmatch};
+use pob_ui_core::luastr::{gsub, lmatch};
```

### B2. §9.2 `xml.children()` / `xml.push_text(..)`

Evidence: `crates/pob-calc/src/model/mod.rs:240-252`: `XmlNode { elem, attrib, children:
Vec<XmlChild> }`, `XmlChild::{Node, Text(LStr)}`; no `children()` method, no `push_text`; and
`pob_calc::model::xml` does not re-export the node types. Applied: `pob-app/src/model.rs` re-exports
`XmlNode`/`XmlChild` inside `model::xml`. Proposed §9.2 text:

```diff
-            for node in xml.children() {
+            for node in &xml.children {
...
-            xml.push_text(buf.clone());
+            xml.children.push(XmlChild::Text(buf.clone().into()));
```

### B3. §9.4 `pob_lua::math::min`

Evidence: `crates/pob-lua/src/math.rs:88` names it `m_min` (`math.min` with Lua NaN semantics).

```diff
-    bm.str_width.set(pob_lua::math::min(bnw, limit));
+    bm.str_width.set(pob_lua::math::m_min(bnw, limit));
```

(§8 rule 1 should say `pob_lua::math::{m_min, m_max}` as well.)

### B4. `impl_parts!` placement (§6.4, §9.3)

A `macro_rules!` invoked at item level cannot add methods to a separate `impl Widget` block. UI-U0's
macro expands to the two `Widget` methods (`part`, `part_mut`), so it is invoked INSIDE the impl:

```diff
-pob_ui_core::impl_parts!(GemSelectControl: edit => EditControl);
-impl Widget for GemSelectControl {
+impl Widget for GemSelectControl {
+    pob_ui_core::impl_parts!(GemSelectControl: edit => EditControl);
```

### B5. `ListControl` must implement `Widget`

§6.8 maps `selControl.ListControl` to `cx.is::<ListControl>(id)`, whose bound is `W: Widget`; the
§6.8 surface lists only inherent functions. UI-U0 adds `impl Widget for ListControl` delegating to
them (U12 owns the file).

## C. Placeholder types the doc names but does not define

Defined minimally so the stubs compile; each owner may amend before its wave gate: `ListColumn`,
`ColumnProp` (U12), `DropTipMode`, `DropTipArgs`, `DropDownHooks` fields (U13), `TooltipLine`,
`TooltipBlock` (U10), `ArrowDir`, `world_to_screen` (U9), `EditUndoState` (U11), `ScrollDir`,
`TextListColumn`, `TextListRow` and the non-§6 constructor signatures of the generic controls
(U9/U12), `SearchHost` fields (U13), `script::{Script, Step}` (U7), `TreeCache`, `TreeAssets`,
`ListMode`, `LaunchState`, `MainState` (U16/U18/U23), the split-struct parts (`State`, §12.1).

## D. Behaviour changes of moved pre-port code (AMENDMENTS A4)

The moved tests were adapted, not dropped:
- `SetDrawLayer(nil)` and `DrawImage` with an unloaded handle now raise Lua errors (SG behaviour;
  the pre-port recorder ignored / skipped them).
- `sync_load_and_dedup` no longer asserts a texture release on `Unload` (see A2).
- Mip generation is still the moved sRGB-correct filter; §4.5 asks for a gamma box filter (U3).
