# PoB UI Rust port: architecture (final, binding)

> **Read `docs/port/AMENDMENTS.md` first** — it reconciles this document with the crates already built (pob-oracle, pob-dump, pob-sg) and wins on conflict.

Status: **final**. This document is the contract for every UI porting agent. CCR-0013 (UI-U0's contract notes) is applied in place (unit CCR1, 2026-10-09); its two open choices are closed in §13 items 10-11 (CCR-0023 A, CCR-0026 A). The UI CCRs among 0014-0094 are applied in place by unit CCR2 (2026-11-10; decisions and the needs-code list in AMENDMENTS A8). If code and this document disagree, the code is wrong unless the unit that owns the code amends this document, and the orchestrator approves the amendment.

**Scope.** This covers the Rust port of Path of Building v2.67.2's user interface. The Lua source is `.PathOfBuilding/src` at HEAD `16de4b82d`. In scope:

- a SimpleGraphic-equivalent renderer and window host;
- the control framework and every PoB control, tab and module;
- the interface the UI consumes from the calc/model plan;
- the verification strategy;
- the work-unit plan.

The calc port (ModParser, CalcSetup/Perform/Offence/Defence, the model classes) is designed elsewhere. §10 fixes only the surface the UI needs from it.

**Lineage.** The architecture is the "verification" proposal, which the judges ranked first. It takes grafts from "literal" and "state-sync", and every factual error the judges found is fixed here. Each SimpleGraphic fact below was re-checked against the SimpleGraphic C++ source. The shipped DLL identifies itself as `SimpleGraphic v2.5-3b1a346` (strings in `.PathOfBuilding/runtime/SimpleGraphic.dll`), so the ground-truth commit is **PathOfBuilding-SimpleGraphic `3b1a346`**. Every PoB Lua citation was re-checked against the tree. §14 lists what changed relative to the research maps and the proposals.

**How to read this document.**

| Section | Who reads it |
|---|---|
| §1 (Decisions), §7 (closures/borrows), §8 (Lua semantics) | Everyone |
| §6 (framework) | Every control and tab port |
| §2, §4, §5 (renderer and host) | Renderer and host units |
| §11 (verification) | Every unit, because acceptance is defined there |
| §12 | The unit table |

---

## 1. Decisions

| # | Decision | Rationale |
|---|---|---|
| D1 | **winit 0.30.13 + wgpu directly**, with no egui/eframe. U0 pins the newest wgpu stable exactly (`=x.y.z`, ≥ 27) in the new crates only; the workspace's `wgpu 22` stays for the legacy app (*removed with the legacy app, U53/INT-4*). Target WebGL2 first; WebGPU is optional. | eframe 0.29 pins wgpu 22, whose WebGPU path is broken. egui would also own fonts, input and focus, and all three must be PoB's own. |
| D2 | **`pob-sg` reproduces SimpleGraphic's internal model 1:1.** Each `(layer, subLayer)` gets a persistent queue of `VIEWPORT/BIND/COLOR/QUAD` commands. Queues are sorted by `(layer, subLayer)` at end of frame. Every draw is BIND+COLOR+QUAD. DrawString writes escape colours into the persistent draw colour. DPI uses exactly SG's formulas. Fonts are a line-by-line port of `r_font.cpp`. | Every UI-visible renderer behaviour is copied from SG source (§2), not guessed. A pure, GPU-free crate builds fast, runs headless and compiles to wasm. |
| D3 | **One context, `Cx`, is passed `&mut` everywhere.** Controls live in a slotmap arena inside `Cx`. Widget behaviour is a set of *associated functions* `fn draw(cx: &mut Cx, me: CtrlId, ..)`, so `self` is an id (Lua's `self` is a reference, not a borrow). Widget state is read in short `cx.w::<W>(me)` scopes. | The borrow checker enforces at compile time what the "literal" design left to runtime `RefCell` panics. Handlers run synchronously at the same point as in Lua, which "state-sync"'s deferred `Fire` did not do. |
| D4 | **`Prop<T> = Val(T) \| Fn(Rc<dyn Fn(&Cx, CtrlId) -> T>)`**, evaluated lazily on every read, exactly like `GetProperty`. A closure receives the id of the control that owns the property (Lua passes `self`) and can read anything in `Cx`. It may mutate only `Cell`/`RefCell` fields that exist for that purpose (§7.3). | Keeps Lua's "the closure reads live state at call time" semantics: 145 `shown`, 79 `enabled` and about 90 geometry closures port verbatim. `Build.lua:148-157` (the `buildName.width` closure writes `strWidth`/`strLimited`) shows that writes are needed; `Cell` fields cover them. |
| D5 | **Lua subclasses become embedding plus a `part` upcast.** `GemSelectControl { edit: EditControl, .. }`, and `cx.w::<EditControl>(gem_id)` resolves the embedded part through the node's vtable. A super call such as `self.EditControl:OnKeyDown(k)` becomes the static `EditControl::on_key_down(cx, me, k, ..)`. Overridable methods dispatch dynamically through the vtable, through class hook tables, or through per-instance override slots. | About 20 ListControl/DropDown subclasses, plus GemSelect and ResizableEdit, can then reuse base functions with no downcast failure. This was the winner's main gap. |
| D6 | **Per-instance method overrides are supported.** `Node.ov` covers the framework methods, including `DrawControls`/`GetMouseOverControl` and `CanReceiveDrag`/`ReceiveDrag`. Classes carry override slots for their own hooks (List hooks, `BuildListControl:LoadBuild`) and for `Tooltip:Draw`. | PoB overrides methods per instance at `Build.lua:158,198,584`, `CompareTab.lua:1607-1628`, `BuildListControl.lua:33,36`, `TradeQuery.lua:404,733`, `TreeTab.lua:1749,1785,1814,1841` and `Main.lua:1297,1307`. |
| D7 | **Host child order is fixed.** Integer keys come first, ascending. Named keys follow in **control creation order** (a sequence number stamped when the control is created). The LuaJIT oracle is patched to iterate the same way at the four draw/hit-test `pairs(self.controls)` sites (§11.2), and an audit run of the unpatched oracle lists hosts where the order is observable. | Lua `pairs` order for string keys is randomised per LuaJIT process (the hash seed comes from a PRNG; calc doc D8), so real PoB's sibling order is not reproducible. Creation order matches the calc plan's insertion-order rule and author intent, and both sides become deterministic. |
| D8 | **Freeing is deferred.** A replaced, removed or closed control (popup, build) goes to a graveyard and is freed at end of frame. Ids are generation-checked: using a dead id panics with the expected type name, which is the equivalent of a Lua nil-index error. | A click that closes its own popup must not invalidate the running call chain, which mirrors Lua GC. Panics fail loudly in tests. |
| D9 | **Input keeps Lua semantics.** `InputEvents` gives `ipairs`-with-holes iteration, `consume(i)` (`inputEvents[i] = nil`), and `lua_len()` with the LuaJIT border rule (property-tested against LuaJIT). | Consumed events must hide later events from later `ipairs` loops exactly as in PoB. `Build.lua:1216` uses `#inputEvents`. |
| D10 | **Coroutines become `async` blocks polled once per `resume`** with a no-op waker. `coroutine.yield()` becomes `co::yield_now().await`. The body reaches `Cx` only through `co::with_cx(\|cx\| ..)`, a scoped thread-local set during `resume` (crate `scoped-tls-hkt`; no `unsafe` in our code). | PoB's exact yield points are kept (PowerBuilder, GemSelect DPS sort, ItemDB, NotableDB, CompareTab power, item DB load, trade). It is single-threaded and works on wasm. |
| D11 | **All UI arithmetic is `f64`**, as in Lua. `Sg` applies SG's own `float`/`double` conversions internally. Fields that mirror Lua indices keep Lua's 1-based arithmetic. | `floor`, `ceil` and `lround` behave bit-for-bit like Lua and SG. |
| D12 | **Colour is gamma-space throughout.** Textures are `Rgba8Unorm` and there are no sRGB formats anywhere. The blend is `SrcAlpha, OneMinusSrcAlpha`. Rendering goes into an `Rgba8Unorm` offscreen target at framebuffer size, then blits to a non-sRGB surface view. | SG is UNORM end to end (`r_texture.cpp`, `r_main.cpp`). |
| D13 | **Verification is trace-first.** Unmodified PoB Lua runs under LuaJIT and draws through **our** `pob-sg`; both sides replay the same script, and per-frame normalised draw traces are diffed. Each op carries a control path marker, a Lua `file:line` and a Rust `#[track_caller]` location. Supporting checks: Tier-0 differential tests of `Control.lua`/`ControlHost.lua`/`UndoHandler.lua` in mlua; `r_font.cpp`-derived font goldens; a real-SG probe under Wine; and CPU-raster vs wgpu pixel checks. | Trace diffs locate bugs at the op level, need no goldens, and are calc-neutral when paired with the Lua calc bridge (D14). |
| D14 | **The UI reaches the calc only through `CalcFacade`** (§10.4), with three implementations. `RustCalc` (default) delegates to `pob-calc`. `LuaCalcBridge` (dev only) runs PoB's own Lua calc and injects the result into the calc doc's Rust types. `WorkerCalc` is wasm only and optional. Semantics are **blocking on desktop**, as in PoB. The facade returns `Poll` so that the web build (whose calc runs in a Web Worker per calc doc D15) can return `Pending`; on desktop it never does. | UI parity work is not blocked on calc parity. Blocking keeps PoB's frame semantics exactly. Web stays possible without forking call sites. |
| D15 | **The model is the calc doc's `pob_calc::model::Build` (`BuildRef = Rc<Build>`)**, with PoB field names and `Field`/`Shared` interior mutability. The calc mutates it in place (calc doc D2, D13). The UI holds model `Rc` handles where Lua holds tables. It never keeps a `Field`/`Shared` borrow across a `cx` call. The calc plan owns XML I/O and every calc-input function in the tab files (§10.2). The UI owns controls, layout, popups and display state. | One source of truth; aliasing is identical to Lua's on both sides of the seam. |
| D16 | **New crates sit beside the old ones.** The new crates are `pob-sg`, `pob-sg-wgpu`, `pob-host`, `pob-ui-core`, `pob-app`, `pob-ui-oracle` and the apps `app/pob-port-{desktop,web}`. The old `crates/pob-ui`, `app/pob-desktop` and `app/pob-web` keep shipping until cut-over (U53). *Done (CCR-0145 §6): U53 deleted the old ones and renamed `app/pob-port-{desktop,web}` to `app/pob-{desktop,web}`; INT-4 deleted `pob-engine`/`pob-data` (AMENDMENTS A13).* | No shippable-state gap (critic R12). |
| D17 | **A Lua runtime error becomes a Rust panic** (`lua_error!`). The host catches unwinds per callback on native and shows PoB's error prompt, which is what `Launch.lua:109-200` does through `PCall`/`ShowErrMsg`. | Faithful failure behaviour; loud in tests. |
| D18 | **Ports are literal.** Statement order, evaluation order of props, and the Lua method structure are kept. There are no cleanups and no cross-file dedup. Every file and function carries a provenance header. | Upstream drift can be tracked per function, and trace diffs map 1:1 to Lua lines. |

---

## 2. SimpleGraphic semantics (verified; binding on `pob-sg`)

Sources: PoB-SimpleGraphic, mainly `ui_api.cpp`, `engine/render/r_main.cpp`, `r_font.cpp`, `r_texture.cpp`, `ui_main.cpp`, `engine/system/win/sys_video.cpp`, `sys_main.cpp` and `engine/common/common.cpp`. Line numbers refer to the research copy (master @ 8694a07) unless a row says `@3b1a346`. U1 re-verified every row at the pinned commit `3b1a346`; `docs/port/ui/sg-semantics.md` gives the pinned line numbers and the differences (CCR-0021). Where that file and this table disagree, the file wins. An older copy (`old/`, which still has `SetBlendMode` and the stack-mask arguments that the DLL's strings also show) was spot-checked and agrees on SetDrawLayer, SetViewport, DrawString height and colour handling, and GetCursorPos.

| Topic | Exact behaviour | Source |
|---|---|---|
| Layer queues | `SetDrawLayer(l, s)` returns at once if `(l, s)` is already current. Otherwise it finds or creates a persistent `r_layer_c(l, s)`, kept in creation order, and appends `VIEWPORT(curViewport)` to it. `SetDrawLayer(l)` means `s = 0`. `SetDrawLayer(nil, s)` keeps the layer. `SetDrawLayer(nil)` is a Lua error. | `ui_api.cpp:762-784`, `r_main.cpp:1456-1484` |
| `GetDrawLayer` | Returns **only the subLayer** (PoB uses it only in commented code, `Tooltip.lua:474`). | `r_main.cpp:1486` |
| Frame start | `curLayer = layerList[0]` (0,0), then `SetViewport()` (full screen, recorded into layer 0), then `DrawColor()` = white. | `r_main.cpp:1020-1023` |
| Ordering | `qsort` by `(layer, subLayer)`. Keys are unique, so the order is deterministic. A queue replays in submission order. | `r_main.cpp:1028-1042, 1122` |
| Viewport | `x, y = lround(v*dpi)`, `w, h = ceil(v*dpi)`; `v` is a double and `dpi` a float, so the product is a double. If the resulting `height == 0`, width and height become full screen while x and y are kept. The command goes into the current layer only. At batch time each vertex carries the viewport; the shader translates by `vp.xy` and discards fragments outside `[x, x+w) × [y, y+h)` (no `glScissor`; a negative size discards everything). Culling is inclusive, in screen space (CCR-0021). | `ui_api.cpp:790-812`, `r_main.cpp:1711-1723`, `:282-319`, `:501-553`, `:805-838` @3b1a346 |
| Consequence | Every quad's effective viewport is the `curViewport` current when it was submitted, because a layer switch re-emits it. A snapshot model of the trace is therefore exact. | derived |
| DrawImage | Argument count must be 5, 6, 7, 9, 10 or 11 (stack index at 6/10, mask at 7/11; PoB uses neither; CCR-0021). A nil image binds `@white`, an 8×8 grey 0xFF texture. A handle with nothing loaded is a Lua error. Position is `(float)val * dpi`, computed in float. The quad corners are TL, TR, BR, BL with UVs `(l,t),(r,t),(r,b),(l,b)`. **Every** call appends BIND + COLOR(drawColor) + QUAD, with no deduplication. | `ui_api.cpp:880-977`, `r_main.cpp:1532-1563` |
| DrawImageQuad | Argument count 9, 10, 11, 17, 18 or 19 (CCR-0021). Default UVs are `(0,0),(1,0),(1,1),(0,1)`. Otherwise the same as DrawImage. | `ui_api.cpp:979-1077` |
| SetDrawColor | `(r,g,b[,a])`, where a nil or missing alpha means 1. With a string argument `"^N"`/`"^xRRGGBB"` (the `x` may be upper or lower case), alpha is forced to 1. Updates `lastColor`. | `ui_api.cpp:817-865`, `common.cpp:201-245` |
| GetDrawColor | Returns `lastColor`. That is updated by SetDrawColor **and after every DrawString** (from the renderer colour after escapes). | `ui_api.cpp:867-878, 1115-1121` |
| **Escape leak** | DrawString passes `col = NULL`, so the font draws with the renderer's own `drawColor` array. Each `^N`/`^xRRGGBB` overwrites the **persistent** draw colour with alpha forced to 1, and emits COLOR. Later DrawImage calls use the last escape colour. Lines that are off screen still process their escapes. An empty string emits nothing. | `r_main.cpp:1565-1581`, `r_font.cpp:326-336, 409-416, 436-439` |
| Text height | `scaled = lround(h*dpi)`; `≤ 1` becomes 1, otherwise it rounds **up to even**, `(scaled+1) & ~1`. The same rule applies to DrawString, DrawStringWidth and DrawStringCursorIndex. | `ui_api.cpp:1095-1104, 1137-1146, 1166-1176` |
| Atlas choice | `fontHeightMap[h]` picks the atlas whose height is `≥ h`, except that the upper `(gh-below-1)/2` heights of each gap map **up**. The effect is "nearest, ties to the lower", so 11→10, 29→28, 30→28, 31→32. Heights below 10 use 10; heights above 64 use 64. `scale = h / atlasH`. All 7 faces have 16 heights `{10,…,28,32,36,40,48,56,64}` × 128 glyphs. | `r_font.cpp:57-117, 288-319`; `.tgf` files |
| Glyph advance | Draw: BIND the glyph's atlas if it differs from the line's current one (before the glyph lookup and the cull; the current atlas resets for every line, so each drawn line starts with a BIND; tabs and escapes do not bind; CCR-0022), `x += spL*scale`; if `width > 0`, emit a quad `w = width*scale` (culled if `x+w < 0` or `x ≥ VSW`) and `x += w`; `x += spR*scale`; `x = ceil(x)`. Tab: `x += 4*(width+spL+spR of ' ')*scale`, **no ceil**. Width measurement: `w += (width+spL+spR)*scale; w = ceil(w)`, tab ×4 then ceil. The result is the max over lines (`\n`) as an int. | `r_font.cpp:143-203, 371-432` |
| Alignment | `CENTER`: `floor((VSW - w)/2 + x)`. `RIGHT`: `floor(VSW - w - x)`. Here VSW is the **virtual screen width in physical px, not the viewport width**. `CENTER_X`: `floor(x - w/2)`. `RIGHT_X`: `floor(x - w)`. Then `x = round(x)` and `y = floor(y)`. A nil align means LEFT. A font index out of range means FIXED. | `r_font.cpp:339-369`, `ui_api.cpp:1092-1110` |
| Line visibility | A line is skipped, but its escapes are still processed, if `y ≥ vid.size[1]` (the **window** height) or `y ≤ -height`. | `r_font.cpp:321-338` |
| Non-ASCII | Text is decoded from UTF-8. A codepoint `≥ numGlyph` (128) is drawn as `"[U+XXXX]"` with the largest atlas at least 3 px smaller, scale 1, a y-pad of `ceil(diff/2)`. The tofu test comes **before** the escape test while drawing and after it while measuring. `DrawStringCursorIndex` returns a **byte** offset + 1. | `r_font.cpp:130-161, 288-302, 396-405`, `r_main.cpp:1617-1636` |
| Lines | `\n` steps y by `height`. A multi-line cursor index walks lines while `curY > lineY`. | `r_font.cpp:263-286, 434-455` |
| Screen / DPI | PoB always calls `RenderInit("DPI_AWARE")`. `GetScreenSize` returns the **framebuffer** size. `GetScreenScale` returns the override % / 100 if set, else the OS content scale. Lua `GetVirtualScreenSize` is `floor(fb/scale)` when `scale ~= 1` (`Common.lua:1118-1126`). `GetCursorPos` is `lround(cursorPhys / scale)`, int over float. `DrawStringWidth` returns `physW / scale`. | `ui_api.cpp:725-737, 1126-1205, 1481-1488`, `r_main.cpp:1638-1678` |
| Textures | All UNORM. Load flags: `ASYNC`, `CLAMP`, `MIPMAP` (clears the default `TF_NOMIPMAP`) and `NEAREST`; any other flag is a Lua error. Min filter is LINEAR, or LINEAR_MIPMAP_LINEAR when there are mips. Mag filter is LINEAR, or NEAREST with `NEAREST`. Wrap is REPEAT unless `CLAMP`. Anisotropy up to 16. Font atlases use `TF_NOMIPMAP` and REPEAT. A failed load's default texture is uploaded with `TF_NOMIPMAP` only (REPEAT, LINEAR, whatever was asked). Oversize images are not downscaled by SG (`BuildMipSet` only drops existing levels, `r_texture.cpp:398-431`); the wgpu backend's downscale is a harmless port addition (CCR-0023 E). | `ui_api.cpp:394-433`, `r_texture.cpp:620-707` @3b1a346 |
| Missing file | Gets the 8×8 grey "default" texture, `fileWidth` stays 0, so `ImageSize()` is `(0,0)` and `IsLoading()` is `true` (a later `Load` of the same path and flags retries it, `ForceLoad`; CCR-0023 C). | `r_texture.cpp:553-608`, `:376-384` @3b1a346 |
| **ImageSize** | **Blocks** (sleeping 1 ms) until the header is parsed (`SIZE_KNOWN`), **for ASYNC loads too**. Lua therefore never sees `(0,0)` for an existing file. `IsLoading()` is `width == 0`. | `r_main.cpp:1638-1652`, `ui_api.cpp:452-482` @3b1a346 |
| Unuploaded texture | A texture that is not `DONE` (an ASYNC upload still pending) binds `@black`, an 8×8 RGBA all-zero (transparent) texture. Quads are still recorded; nothing is visible. | `r_texture.cpp:340-347` |
| Shader / blend | `color = texture(uv) * tint`, fragments outside the viewport discarded. Blend is `SRC_ALPHA, ONE_MINUS_SRC_ALPHA` (`RB_ALPHA`; `SetDrawLayer` and `BeginFrame` queue a BLEND command, PoB never changes the mode, so `Cmd` has no `Blend` variant and `Sg` only counts its bytes). The scene goes into an RGB render target at framebuffer size, then a full-screen blit (NEAREST for integer DPI scales, else LINEAR). The shaders are inline strings in `r_main.cpp` (CCR-0021). | `r_main.cpp:602-617, 784-843, 1166-1203` @3b1a346 |
| Wheel | One OS scroll event gives `KeyDown` **and** `KeyUp` of `WHEELUP`/`WHEELDOWN`. | `sys_video.cpp:608-620` |
| Double click | Same button within 500 ms and ±5 px. A detected double click resets the click memory, so a triple click yields dbl, single. Delivered as `OnKeyDown(key, true)`. | `sys_video.cpp:568-604`, `ui_main.cpp:526-528` |
| Extra OnChar | Pressing **or repeating** Backspace, Tab, Enter or Escape also sends `OnChar` `"\b"`, `"\t"`, `"\r"`, `"\x1b"`, after the KeyDown. Key repeat re-sends `KeyDown`. | `sys_video.cpp:529-541`, `sys_main.cpp:343-357` |
| Key names | `key < 128` → `"%c"` (letters arrive lowercase). Otherwise the `ui_keyNameMap` name: `LEFTBUTTON MIDDLEBUTTON RIGHTBUTTON MOUSE4 MOUSE5 WHEELUP WHEELDOWN BACK TAB RETURN ESCAPE SHIFT CTRL ALT PAUSE PAGEUP PAGEDOWN END HOME PRINTSCREEN INSERT DELETE UP DOWN LEFT RIGHT F1..F15 NUMLOCK SCROLLLOCK`. `NameForKey` looks the number up by value and OnChar passes the codepoint, so OnChar of U+0080..U+00AA delivers a key name (`£` → `"F10"`, `§` → `"F14"`, NBSP → `"F7"`); codepoints `>= 171` deliver `"?"` (`pob_sg::Key::from_sg_code`; CCR-0021). | `ui_main.cpp:16-61, 523-525, 548-572`, `keylist.h:7-51` |
| Swallowed keys | `KeyUp(F10)` (debug UI toggle) and `KeyUp(PAUSE)` with Shift held are **not** delivered to Lua. `KeyDown` of `` ` `` or `F1` with Ctrl held toggles SG's console and is not delivered; while the console is open it consumes all input. The port has no console and swallows the toggle KeyDown (CCR-0021, CCR-0026 F). | `ui_main.cpp:516-546`, `ui_console.cpp:99-111` |
| Frame loop | 1) If not active, cursor not over the window, and no coroutine or subscript active: sleep 100 ms and skip the frame (frames always run for 11 frames after boot: `framesSinceWindowHidden <= 10` is tested before the increment, CCR-0026 F; an unfocused window sleeps 100 ms after every frame even while hovered). 2) BeginFrame, then snapshot the cursor. 3) Run subscript completions (`OnSubFinished` etc.). 4) `OnFrame`. 5) EndFrame. 6) If inactive and idle, sleep 100 ms. Identical frames are skipped by a command digest (MurmurHash64A of the raw command bytes, `r_main.cpp:1385-1403` @3b1a346); this changes no pixels. Each layer holds at most `1 << 23` bytes of commands per frame (about 74,900 draws); further commands are silently dropped, and `Sg` reproduces this (`r_main.cpp:172, 218-228`). SG reads every string argument up to its first NUL (`const char*`; CCR-0021). | `ui_main.cpp:380-445`, `r_main.cpp:1202-1243` |
| Unused by PoB | `NewArtHandle`, `LoadArt*`, `Texture.*`, stack index and mask, `SetBlendMode`, `SetClearColor`, `ShowCursor`, `SetCursorPos`, `GetAsyncCount`. Not ported. `GetDrawColor` has one use (`Tooltip.lua:613`); `TakeScreenshot` has one (`Launch.lua:164`). | grep of `src` |

Intentional deviations, documented: line visibility uses the framebuffer height in place of `vid.size[1]`. The two are equal on Windows with DPI awareness, and the framebuffer height is the only sane value on other platforms. Frame elision is optional, because it is invisible.

---
## 3. Crate and module layout

```
crates/
  pob-lua/          OWNED BY THE CALC PLAN (calc:U0/P2): Lua stdlib semantics (math, string.find/sub/gsub/gmatch/format,
                    tostring %.14g, floored %, table.sort, ljtab::LjIntMap), Name/LStr, Field/Shared, LuaError and
                    lua_error!. The UI consumes it; UI-only gaps go through CCRs (U5).
  pob-sg/           PURE SimpleGraphic equivalent. No GPU, no winit, no threads, no std::fs; builds for wasm32.
    src/lib.rs        re-exports
    src/types.rs      Num, IRect, ScreenInfo, Align, Font, Key
    src/api.rs        struct Sg: layers/viewport/colour/draw_*/screen/DPI (ui_api.cpp + r_main.cpp)
    src/frame.rs      Frame, LayerQueue, Cmd, Quad, QuadSink
    src/color.rs      is_color_escape, read_color_escape, strip_escapes, PALETTE
    src/font/{mod,tgf,tga,layout}.rs   FontSet/FontFace; .tgf parser; RLE/raw 32-bpp TGA; r_font.cpp port
    src/image.rs      ImageRegistry, ImageHandle (Rc-shared), ImageFlags, TexId, ImageLoader trait, mip gen
    src/assets.rs     AssetSource trait: read_src(path) (PoB src/) and read_runtime(path) (runtime/SimpleGraphic/)
    src/sys.rs        Sys (cursor, key state, time), Platform / Fs / Net traits
    src/trace.rs      ApiCall, DrawOp, FrameTrace, TraceRecorder, normalize(), diff()
    src/raster.rs     [feature "raster"] CPU reference rasteriser: Frame -> RGBA
  pob-sg-wgpu/      SgRenderer: prepare/render/read_back, WGSL, texture cache, samplers, offscreen RT
  pob-host/         App trait; desktop (winit) and web (wasm32) hosts; HeadlessHost; input mapping; pacing;
                    native/web/headless ImageLoader; Platform/Fs/Net impls; the .pobscript scenario DSL
  pob-ui-core/      Cx, Ui arena, Prop, Anchor/GetPos, Widget/VTable/Overrides, ControlHost dispatch,
                    InputEvents, focus/tab groups, Undo, coroutines, MainCore, colour codes, draw helpers,
                    WrapString, Tooltip/TooltipHost, PopupDialog and the main:Open*Popup family, and the
                    generic controls (Label…PathControl)
  pob-app/          PoB itself: Launch, Main, BuildList mode, Build mode, every tab, PassiveTreeView and tree
                    assets, gem/item tooltips, calc facade (RustCalc; LuaCalcBridge injected in dev), platform storage/net glue
  pob-ui-oracle/    DEV ONLY (never a dependency of a shipping binary): LuaJIT PoB on our pob-sg,
                    determinism and order patches, gallery harness, ui-parity CLI, trace-render,
                    pob-lua-gui reference viewer, LuaCalcBridge (implements pob_app::calc::CalcFacade)
app/pob-port-desktop/  fn main(): pob_host::run_desktop(cfg, pob_app::PobApp::new); feature `lua-calc`
                       injects LuaCalcBridge for dev
app/pob-port-web/      wasm entry (Trunk): pob_host::run_web(...)
```

As built since U53 (CCR-0145 §6): `app/pob-port-desktop` is `app/pob-desktop` (package and bin
`pob-desktop`) and `app/pob-port-web` is `app/pob-web` (package `pob-web`, worker glue `./pob-web.js`).
The non-default desktop feature `lua-calc` is the only path from a shipping package to `mlua`
(through pob-ui-oracle and pob-oracle); the default `cargo tree -e normal` of `pob-app`,
`pob-desktop` and `pob-web` has no `mlua` (INT-4, AMENDMENTS A13).

Dependency graph (an arrow points at the dependency):

```
pob-lua (calc plan) <- pob-sg <- pob-ui-core <- pob-app -> pob-calc, pob-gamedata (calc plan)
           pob-sg <- pob-sg-wgpu <- pob-host <- app/pob-port-*
pob-ui-oracle -> pob-sg, pob-host (headless + desktop), pob-app, pob-luahost (LuaJIT natives, made pub), pob-oracle (calc:ORC-X inject)
```

**Pinned versions (new crates only).**

| Crate | Version / features |
|---|---|
| winit | `=0.30.13` |
| wgpu | `=<newest stable at U0>`, feature `webgl` |
| image | `0.25.10` with `png`, `jpeg`, `webp` |
| slotmap | `1.1` |
| bitflags | `2` |
| smallvec | `1` |
| indexmap | `2` |
| arboard | `3.6` (native) |
| scoped-tls-hkt | `0.1` |
| xxhash-rust | `xxh3` |
| bytemuck | `1` |
| serde, ron, serde_json | — |
| mlua | workspace version, oracle only |
| wasm-bindgen-futures, web-sys | wasm |

TGA is decoded by our own decoder (about 60 lines). If `scoped-tls-hkt` does not fit, U4 writes the equivalent 25-line scoped TLS under `#[allow(unsafe_code)]` in `coroutine.rs`, the only allowed `unsafe` in the UI crates.

**Wasm rules.** These apply to every crate except `pob-host` (desktop module) and `pob-ui-oracle`:

- no `std::fs`, `std::thread` or blocking IO;
- all IO goes through `Sys`'s `Platform`/`Fs`/`Net`;
- time comes from `Sys`;
- CI runs `cargo check -p pob-app --target wasm32-unknown-unknown`.

**Conventions (every port file).**

- Header: `//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ButtonControl.lua`.
- Every function carries a doc line `/// lua: ButtonControl.lua:107-117`, the exact line range.
- Names are snake_case of the Lua names (`ProcessControlsInput` → `process_controls_input`, `selControl` → `sel_control`). Host keys keep the exact Lua string (`"notesDesc"`), because the oracle markers use them.
- Statements stay in the Lua order. Never reorder property evaluations (§7). No cleanups, no cross-file dedup.
- Divergences from Lua are marked `// DIVERGENCE(<reason>)` and listed in the unit's acceptance notes.
- Draw-affecting `pairs` loops are marked `// ORDER: <resolution>` (§8, rule 6).

---

## 4. `pob-sg`: the SimpleGraphic API

### 4.1 Types

```rust
// pob-sg/src/types.rs
pub type Num = f64; // every number the Lua UI computes

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct IRect { pub x: i32, pub y: i32, pub w: i32, pub h: i32 } // physical px (r_viewport_s lo/extent)

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScreenInfo {
    pub fb_w: i32,      // framebuffer px (vid.fbSize) == VirtualScreenWidth in DPI_AWARE mode
    pub fb_h: i32,
    pub win_h: i32,     // vid.size[1] (line-visibility test; == fb_h on non-Windows, see §2)
    pub dpi_scale: f32, // OS content scale (vid.dpiScale)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Align { Left, Center, Right, CenterX, RightX } // alignMap order; nil => Left

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum Font { Fixed, Var, VarBold, FontinSc, FontinScItalic, Fontin, FontinItalic } // fontMap order
impl Font {
    pub const ALL: [Font; 7] = [Font::Fixed, Font::Var, Font::VarBold, Font::FontinSc,
                                Font::FontinScItalic, Font::Fontin, Font::FontinItalic];
    pub const fn file_stem(self) -> &'static str {
        match self {
            Font::Fixed => "Bitstream Vera Sans Mono", Font::Var => "Liberation Sans",
            Font::VarBold => "Liberation Sans Bold", Font::FontinSc => "Fontin SmallCaps",
            Font::FontinScItalic => "Fontin SmallCaps Italic", Font::Fontin => "Fontin",
            Font::FontinItalic => "Fontin Italic",
        }
    }
    /// "FIXED" | "VAR" | "VAR BOLD" | "FONTIN SC" | "FONTIN SC ITALIC" | "FONTIN" | "FONTIN ITALIC".
    /// Unknown names are a Lua error in SG (luaL_checkoption); returns None.
    pub fn from_lua(s: &str) -> Option<Font>;
    pub const fn lua_name(self) -> &'static str;
}

/// What Lua receives in OnKeyDown/OnKeyUp/OnChar and passes to IsKeyDown (ui_main.cpp:16-61, 547-590).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum Key {
    LeftButton, MiddleButton, RightButton, Mouse4, Mouse5, WheelUp, WheelDown,
    Back, Tab, Return, Escape, Shift, Ctrl, Alt, Pause, PageUp, PageDown, End, Home,
    PrintScreen, Insert, Delete, Up, Down, Left, Right,
    /// F1..=F15
    F(u8),
    NumLock, ScrollLock,
    /// key < 128 => "%c": lowercase letters, digits and punctuation on KeyDown; any byte < 128
    /// (including b'\x08', b'\t', b'\r', b'\x1b') on Char
    Char(u8),
    /// NameForKey fallback "?" (e.g. OnChar of a codepoint >= 128)
    Unknown,
}
impl Key {
    /// The exact Lua string ("LEFTBUTTON", "z", "?").
    pub fn name(self) -> std::borrow::Cow<'static, str>;
    /// KeyForName (case-insensitive for multi-char names, as IsKeyDown accepts).
    pub fn from_name(s: &str) -> Option<Key>;
    /// `key:match("BUTTON")`
    pub const fn is_button(self) -> bool { matches!(self, Key::LeftButton | Key::MiddleButton | Key::RightButton) }
}
```

### 4.2 `Sg`: exact signatures

All numeric arguments are Lua numbers (virtual px). Every recording method is `#[track_caller]`, so the trace carries the Rust caller location.

```rust
// pob-sg/src/api.rs
pub struct Sg {
    screen: ScreenInfo,
    dpi_override_percent: i32,
    layers: Vec<LayerQueue>,   // SG layerList: creation order, persistent; [0] == (0,0)
    cur: usize,
    viewport: IRect,           // physical (curViewport)
    draw_color: [f32; 4],      // renderer drawColor (escape leak target)
    last_color: [f32; 4],      // ui->lastColor (GetDrawColor)
    render_enable: bool,       // SetDrawLayer/Viewport/Draw* outside OnFrame => lua_error
    pub fonts: std::rc::Rc<crate::font::FontSet>,
    pub images: crate::image::ImageRegistry,
    pub trace: Option<crate::trace::TraceRecorder>,
}

impl Sg {
    pub fn new(fonts: std::rc::Rc<crate::font::FontSet>, images: crate::image::ImageRegistry) -> Sg;

    // ---- host-only lifecycle ----
    /// Clears queues; cur = layer (0,0); SetViewport(); DrawColor(white); pumps the image loader.
    pub fn begin_frame(&mut self, screen: ScreenInfo);
    /// Sorted non-empty queues + texture uploads/releases + XXH3 digest.
    pub fn end_frame(&mut self) -> crate::frame::Frame;

    // ---- screen / DPI ----
    pub fn get_screen_size(&self) -> (i32, i32);            // (fb_w, fb_h)
    pub fn get_screen_scale(&self) -> f32;                  // VirtualScreenScaleFactor
    pub fn set_dpi_scale_override_percent(&mut self, percent: i32);
    pub fn get_dpi_scale_override_percent(&self) -> i32;
    /// Common.lua:1118 GetVirtualScreenSize (Lua helper, here for convenience)
    pub fn get_virtual_screen_size(&self) -> (Num, Num);

    // ---- draw state ----
    #[track_caller] pub fn set_draw_layer(&mut self, layer: Option<i32>, sub_layer: Option<i32>);
    pub fn get_draw_layer(&self) -> i32;                    // returns subLayer (SG quirk)
    #[track_caller] pub fn set_viewport(&mut self, x: Num, y: Num, w: Num, h: Num);
    #[track_caller] pub fn reset_viewport(&mut self);       // SetViewport()
    #[track_caller] pub fn set_draw_color(&mut self, r: Num, g: Num, b: Num);          // a = 1
    #[track_caller] pub fn set_draw_color_a(&mut self, r: Num, g: Num, b: Num, a: Num);
    #[track_caller] pub fn set_draw_color_esc(&mut self, esc: &str);                   // "^7" / "^xRRGGBB"; invalid => lua_error
    pub fn get_draw_color(&self) -> [f32; 4];                                          // lastColor

    // ---- drawing ----
    #[track_caller] pub fn draw_image(&mut self, img: Option<&ImageHandle>, x: Num, y: Num, w: Num, h: Num);
    #[track_caller] pub fn draw_image_tc(&mut self, img: Option<&ImageHandle>, x: Num, y: Num, w: Num, h: Num,
                                         tc_l: Num, tc_t: Num, tc_r: Num, tc_b: Num);
    #[track_caller] pub fn draw_image_quad(&mut self, img: Option<&ImageHandle>, p: [Num; 8]);
    #[track_caller] pub fn draw_image_quad_tc(&mut self, img: Option<&ImageHandle>, p: [Num; 8], tc: [Num; 8]);
    #[track_caller] pub fn draw_string(&mut self, x: Num, y: Num, align: Align, height: Num, font: Font, text: &str);
    pub fn draw_string_width(&self, height: Num, font: Font, text: &str) -> Num;
    /// 1-based byte index into `text`, exactly what Lua receives.
    pub fn draw_string_cursor_index(&self, height: Num, font: Font, text: &str, cx: Num, cy: Num) -> usize;

    // ---- images ----
    pub fn new_image_handle(&mut self) -> ImageHandle;     // NewImageHandle()

    // ---- trace (oracle/tests) ----
    pub fn trace_ctl_push(&mut self, path_elem: &str);     // control-path markers (§11.2, patch 7)
    pub fn trace_ctl_pop(&mut self);
    pub fn set_trace_src(&mut self, src: Option<String>);  // oracle: Lua "Classes/X.lua:54" for the next call
}
pub fn strip_escapes(s: &str) -> String;                    // StripEscapes (IsColorEscape-based, ui_api.cpp:1206)
pub fn scaled_text_height(h: Num, dpi: f32) -> i32 {        // ui_api.cpp:1095-1104
    let v = (h * dpi as f64).round() as i32;                // std::lround: half away from zero == f64::round
    if v <= 1 { 1 } else { (v + 1) & !1 }
}
```

Fidelity-critical bodies. U1 writes the rest in the same style.

```rust
impl Sg {
    #[inline] pub fn scale(&self) -> f32 {
        if self.dpi_override_percent > 0 { self.dpi_override_percent as f32 / 100.0 } else { self.screen.dpi_scale }
    }
    #[inline] fn vsw(&self) -> i32 { self.screen.fb_w }
    #[inline] fn vsh(&self) -> i32 { self.screen.fb_h }

    #[track_caller]
    pub fn set_draw_layer(&mut self, layer: Option<i32>, sub: Option<i32>) {
        self.rec(|| ApiCall::SetDrawLayer { layer, sub });
        let (l, s) = match (layer, sub) {
            (None, Some(s)) => (self.layers[self.cur].layer, s),
            (Some(l), s) => (l, s.unwrap_or(0)),
            (None, None) => lua_error!("SetDrawLayer(): must provide subLayer if layer is nil"),
        };
        let c = &self.layers[self.cur];
        if c.layer == l && c.sub_layer == s { return; }
        let idx = self.layers.iter().position(|q| q.layer == l && q.sub_layer == s)
            .unwrap_or_else(|| { self.layers.push(LayerQueue::new(l, s)); self.layers.len() - 1 });
        self.cur = idx;
        let vp = self.viewport;
        self.layers[idx].cmds.push(Cmd::Viewport(vp));
    }

    #[track_caller]
    pub fn set_viewport(&mut self, x: Num, y: Num, w: Num, h: Num) {
        self.rec(|| ApiCall::SetViewport(Some([x, y, w, h])));
        let s = self.scale() as f64;                               // lua_Number * float -> double
        self.set_viewport_phys(IRect { x: (x * s).round() as i32, y: (y * s).round() as i32,
                                       w: (w * s).ceil() as i32,  h: (h * s).ceil() as i32 });
    }
    fn set_viewport_phys(&mut self, mut vp: IRect) {
        if vp.h == 0 { vp.w = self.vsw(); vp.h = self.vsh(); }    // r_main.cpp:1493 (x, y kept)
        self.viewport = vp;
        self.layers[self.cur].cmds.push(Cmd::Viewport(vp));
    }

    fn quad_phys(&mut self, img: Option<&ImageHandle>, p: [f32; 8], uv: [f32; 8]) {
        let tex = match img {
            None => TexId::WHITE,
            Some(h) => h.tex().unwrap_or_else(|| lua_error!("DrawImage(): image handle has no image loaded")),
        };
        let col = self.draw_color;
        let q = &mut self.layers[self.cur].cmds;
        q.push(Cmd::Bind(tex));
        q.push(Cmd::Color(col));
        q.push(Cmd::Quad(Quad { s: [uv[0], uv[2], uv[4], uv[6]], t: [uv[1], uv[3], uv[5], uv[7]],
                                x: [p[0], p[2], p[4], p[6]],     y: [p[1], p[3], p[5], p[7]] }));
    }

    #[track_caller]
    pub fn draw_image_tc(&mut self, img: Option<&ImageHandle>, x: Num, y: Num, w: Num, h: Num,
                         l: Num, t: Num, r: Num, b: Num) {
        if self.trace.is_some() {                     // the trace records what the image was loaded from (CCR-0013 A4)
            let ir = self.img_ref(img);               // ImageRegistry::source(tex) -> ImgRef { path, flags }
            self.rec(|| ApiCall::draw_image(ir, [x, y, w, h], Some([l, t, r, b])));
        }
        let s = self.scale();
        let (x, y, w, h) = (x as f32 * s, y as f32 * s, w as f32 * s, h as f32 * s); // (float)val * dpiScale
        let (l, t, r, b) = (l as f32, t as f32, r as f32, b as f32);
        self.quad_phys(img, [x, y, x + w, y, x + w, y + h, x, y + h], [l, t, r, t, r, b, l, b]);
    }

    #[track_caller]
    pub fn draw_string(&mut self, x: Num, y: Num, align: Align, height: Num, font: Font, text: &str) {
        self.rec(|| ApiCall::DrawString { x, y, align, height, font, text: text.to_owned() });
        let s = self.scale();
        let pos = [(x * s as f64) as f32, (y * s as f64) as f32];  // double * float -> double -> float
        let h = scaled_text_height(height, s);
        let mut col = self.draw_color;                              // SG passes drawColor *by pointer*
        let fonts = self.fonts.clone();
        let (vsw, win_h) = (self.vsw(), self.screen.win_h);
        fonts.face(font).draw(&mut LayerSink(&mut self.layers[self.cur]), pos, align, h, &mut col, text, vsw, win_h);
        self.draw_color = col;                                      // escapes leak (alpha forced to 1)
        self.last_color = col;
    }

    pub fn draw_string_width(&self, height: Num, font: Font, text: &str) -> Num {
        if text.is_empty() { return 0.0; }
        let s = self.scale();
        self.fonts.face(font).string_width(scaled_text_height(height, s), text) as f64 / s as f64
    }
}
```

### 4.3 Frame (render-level command stream)

```rust
// pob-sg/src/frame.rs
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Quad { pub s: [f32; 4], pub t: [f32; 4], pub x: [f32; 4], pub y: [f32; 4] } // TL,TR,BR,BL, physical px

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Cmd { Viewport(IRect), Bind(TexId), Color([f32; 4]), Quad(Quad) }

pub struct LayerQueue { pub layer: i32, pub sub_layer: i32, pub cmds: Vec<Cmd> }

pub struct Frame {
    pub screen: ScreenInfo,
    pub layers: Vec<LayerQueue>,                                  // sorted by (layer, sub_layer); empty dropped
    pub uploads: Vec<(TexId, std::rc::Rc<crate::image::ImageData>)>, // became ready this frame
    pub releases: Vec<TexId>,
    pub digest: u64,                                              // XXH3 over cmds (optional frame elision)
}

pub trait QuadSink { fn bind(&mut self, t: TexId); fn color(&mut self, c: [f32; 4]); fn quad(&mut self, q: Quad); }
```

### 4.4 Fonts (port of `r_font.cpp`)

```rust
// pob-sg/src/font/mod.rs
#[derive(Clone, Copy, Default)]
pub struct Glyph { pub tc_l: f32, pub tc_r: f32, pub tc_t: f32, pub tc_b: f32, pub width: i32, pub sp_l: i32, pub sp_r: i32 }
pub struct FontHeight { pub tex: TexId, pub height: i32, pub num_glyph: usize, pub glyphs: [Glyph; 128] }
pub struct FontFace { heights: Vec<FontHeight>, height_map: Vec<usize>, max_height: i32 }
pub struct FontSet { faces: [FontFace; 7] }

impl FontSet {
    /// Parses the 7 .tgf files and registers the 112 atlases synchronously (UVs need the atlas size).
    pub fn load(src: &dyn crate::assets::AssetSource, images: &mut crate::image::ImageRegistry) -> std::io::Result<FontSet>;
    pub fn face(&self, f: Font) -> &FontFace;
}
impl FontFace {
    /// FindFontHeight: h > max => last; h < 0 => 0; else height_map[h].
    pub fn find(&self, h: i32) -> usize;
    /// FindSmallerFontHeight(height, idx, 3) => (index, y_pad)
    pub fn find_smaller(&self, h: i32, idx: usize, reduction: i32) -> (usize, i32);
    pub fn string_width(&self, h: i32, text: &str) -> i32;                       // StringWidth
    pub fn cursor_index(&self, h: i32, text: &str, cx: i32, cy: i32) -> usize;   // 0-based BYTE offset
    pub fn draw(&self, out: &mut dyn crate::frame::QuadSink, pos: [f32; 2], align: Align, h: i32,
                col: &mut [f32; 4], text: &str, screen_w: i32, win_h: i32);      // Draw + DrawTextLine
}
```

`draw` mirrors `r_font_c::Draw` and `DrawTextLine` statement by statement:

- an empty `text` returns with nothing emitted;
- one `out.color(*col)` up front;
- per line: the visibility test (`y >= win_h || y <= -h`; when it fails, only the escapes are processed, each with `col[3] = 1` and `out.color`), alignment against `screen_w`, then `x = round(x)` and `y = floor(y)`;
- `bind` at the start of every glyph (tofu glyphs included) whose atlas differs from the line's current atlas, before the glyph lookup and the x cull; the current atlas is per line, so each drawn line begins with a BIND (`r_font.cpp:371-378`; CCR-0022);
- tofu substitution;
- tabs;
- `ceil` after each glyph.

The height-map construction is copied verbatim from the constructor. Text is decoded as UTF-8 to codepoints with a source-byte offset table, so `cursor_index` can map back to bytes (`IndexUTF8ToUTF32`).

As built (CCR-0022): byte-string twins `string_width_bytes`, `cursor_index_bytes`, `draw_bytes` take Lua strings as SG does (cut at the first NUL; `IndexUTF8ToUTF32` turns every byte that does not start a complete sequence into U+FFFD, advancing one byte), and the `&str` methods delegate to them; `string_width_u32`, `string_cursor_index_u32`, `draw_u32` are `r_font_c`'s codepoint level; `font::index_utf8_to_utf32(&[u8])`, `font::layout::{is_color_escape_u32, read_color_escape_u32}`. After a trailing `'\n'`, `StringCursorIndex` returns `len + lastIndex` of the previous line, and `DrawStringCursorIndex` maps any index `>= offsets.size()` to the byte length. Heights `<= 0` use atlas 0 with scale 0 (or a negative scale). `.tgf` lines parse with `sscanf` rules (`"HEIGHT %u;"`, `"GLYPH %u %u %u %d %d;"`).

### 4.5 Images

```rust
// pob-sg/src/image.rs
bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
    pub struct ImageFlags: u8 { const ASYNC = 1; const CLAMP = 2; const MIPMAP = 4; const NEAREST = 8; }
}
impl ImageFlags { pub fn from_lua(flags: &[&str]) -> Result<ImageFlags, String>; } // unknown flag => Err (Lua error)

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct TexId(pub u32);
impl TexId { pub const WHITE: TexId = TexId(0); pub const BLACK: TexId = TexId(1); pub const DEFAULT: TexId = TexId(2); }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TexStatus { Init, InQueue, Processing, SizeKnown, PendingUpload, Done }

/// `flags` carries the sampler state (CLAMP / NEAREST): the backend only receives `Frame` and indexes
/// `samplers[8]` by CLAMP|NEAREST|MIPMAP (§5.1; CCR-0013 A1).
pub struct ImageData { pub width: u32, pub height: u32, pub levels: Vec<Vec<u8>> /* RGBA8, mip 0.. */,
                       pub flags: ImageFlags }

pub enum LoadEvent { Size(TexId, u32, u32), Done(TexId, ImageData), Failed(TexId) }

/// Implemented in pob-host: native thread pool, web (prefetched VFS + lazy fetch), headless.
pub trait ImageLoader {
    /// Non-ASYNC (`sync == true`): must return Done/Failed immediately. ASYNC: may return nothing.
    fn request(&mut self, id: TexId, path: &str, flags: ImageFlags, sync: bool, priority: i32) -> Option<LoadEvent>;
    /// Header-only size read; used by `image_size` to emulate SG's blocking ImageSize (§2).
    fn read_size(&mut self, id: TexId, path: &str) -> Option<(u32, u32)>;
    fn set_priority(&mut self, id: TexId, priority: i32);
    fn poll(&mut self) -> Vec<LoadEvent>;
}

pub struct ImageRegistry { /* infos, by_key: HashMap<(String, ImageFlags), TexId>, loader: Box<dyn ImageLoader>, uploads */ }
impl ImageRegistry {                                     // surface frozen by UI-U0 (CCR-0013 A3)
    /// Registers `@white`, `@black` and the default texture as TexId 0/1/2, with pending uploads.
    pub fn new(loader: Box<dyn ImageLoader>) -> ImageRegistry;
    pub fn insert_ready(&mut self, name: &str, data: ImageData) -> TexId;    // font atlases, built-ins
    pub fn load(&mut self, h: &ImageHandle, path: &str, flags: ImageFlags);
    pub fn pump(&mut self);                                                   // poll the loader
    pub fn status(&self, id: TexId) -> TexStatus;
    pub fn source(&self, id: TexId) -> Option<(&str, ImageFlags)>;           // the trace's ImgRef
    pub fn tex_size(&self, id: TexId) -> Option<(u32, u32)>;
    pub fn image_size(&mut self, id: TexId) -> (i32, i32);                   // blocking emulation via ImageLoader::read_size
    pub fn set_priority(&mut self, id: TexId, priority: i32);
    pub fn take_uploads(&mut self) -> Vec<(TexId, std::rc::Rc<ImageData>)>;
    pub fn take_releases(&mut self) -> Vec<TexId>;
}
pub fn decode_image(bytes: &[u8], name: &str, flags: ImageFlags) -> Option<ImageData>;
pub fn read_image_size(bytes: &[u8], name: &str) -> Option<(u32, u32)>;
pub fn build_mips(w: u32, h: u32, base: Vec<u8>) -> Vec<Vec<u8>>; pub fn default_texture() -> ImageData;

/// imgHandle userdata. Clone shares the slot (Lua aliasing): Load/Unload through any alias is seen by all.
#[derive(Clone, Default)]
pub struct ImageHandle(std::rc::Rc<std::cell::Cell<Option<TexId>>>);
impl ImageHandle {
    pub fn load(&self, sg: &mut Sg, path: &str, flags: ImageFlags); // relative paths resolve against the script dir (src/)
    /// Clears the shared slot (`delete hnd; hnd = NULL`). Textures live until exit, so `Frame::releases` stays empty:
    /// SG frees a texture when the Lua GC collects its last handle, which has no deterministic equivalent (CCR-0023 A).
    pub fn unload(&self);
    pub fn is_valid(&self) -> bool;                     // hnd != NULL
    pub fn is_loading(&self, sg: &mut Sg) -> bool;      // image_size().0 == 0
    /// Blocks (reads the header) until the size is known, like SG; (0,0) for a missing file.
    /// Calling it on an unloaded handle is a Lua error.
    pub fn image_size(&self, sg: &mut Sg) -> (i32, i32);
    pub fn set_loading_priority(&self, sg: &mut Sg, pri: i32);
    pub fn tex(&self) -> Option<TexId>;
}
```

Image rules:

- Registration is deduplicated by `(path, flags)` with `\` read as `/`, compared exactly (SG: case-sensitive 16-bit `StringHash` + `_stricmp` + flags, `r_main.cpp:1585-1614` @3b1a346; hash collisions between case variants are not emulated). Re-registering a texture whose size is still unknown loads it again synchronously (`ForceLoad`), so a failed load is retried on every `Load`. `Load("")` names the script directory: a valid handle on a failed texture (CCR-0023 C).
- `IsLoading`, `ImageSize` and `SetLoadingPriority` on an unloaded handle raise `imgHandle:<Method>(): image handle has no image loaded`; `Unload` and `IsValid` accept one. A failed load keeps the size the format's header callback set. Formats are chosen as `LoaderForFile` does (`.dds`, `.dds.zst`, `.webp` by case-sensitive extension, then magic bytes, TGA by header). `take_uploads` polls the loader first (SG's `PumpShaders` in `EndFrame`). An unknown flag: SG deletes the old handle before it validates the flags; the U8 bridge reproduces that order (CCR-0023 D). `read_image_size_from(reader, name)` reads only the header (native loader).
- PNG, JPEG and WebP decode through the `image` crate (not stb_image v2.26 / libwebp): all 99 PNGs and both WebPs of `src/Assets` + `TreeData/3_27` are byte-identical to SG's decoders, JPEGs differ by at most 4 levels on at most 0.1 % of a file's pixels (inside T6's budget). GIF and DDS are detected but not decoded (PoB ships none). DIVERGENCE, measured (CCR-0023 F).
- A missing file falls back to `TexId::DEFAULT`'s pixels, and its size reads as `(0,0)`.
- MIPMAP levels are `BuildMipSet`'s (`r_texture.cpp:433-461` @3b1a346): each level is `stbir_resize_uint8_srgb_edgemode` (stb_image_resize v0.97: Mitchell, sRGB-linear, alpha-premultiplied when the image has 4 channels, clamped edges) of the previous one, `floor(log2(max(w,h)))+1` levels. Ported bit-exactly in `pob-sg/src/image.rs` (goldens from SG's own header; CCR-0023 B). PoB itself never passes `"MIPMAP"`.
- **Deterministic mode** (oracle, headless, tests): every load completes synchronously, pixels included.
- **Web.** `image_size` cannot block. The host prefetches all synchronously loaded assets (fonts, `Assets/*`, the active tree version's sprite sheets) into a VFS before `on_init` and before a tree version switch. Only ASYNC images are fetched lazily.

### 4.6 System, platform, trace

```rust
// pob-sg/src/sys.rs
pub trait Platform {
    fn time_ms(&self) -> i64;                                   // GetTime (ms since start)
    fn copy(&mut self, text: &str);                             // Copy
    fn paste(&mut self) -> Option<String>;                      // Paste
    fn open_url(&mut self, url: &str);                          // OpenURL
    fn set_window_title(&mut self, title: &str);                // SetWindowTitle
    fn take_screenshot(&mut self);                              // TakeScreenshot
    fn restart(&mut self);                                      // Restart
    fn exit(&mut self, msg: Option<&str>);                      // Exit
    fn script_path(&self) -> String;                            // GetScriptPath
    fn runtime_path(&self) -> String;                           // GetRuntimePath
    fn user_path(&self) -> Result<String, (String, String)>;    // GetUserPath -> path | (invalidPath, errMsg)
    fn con_print(&mut self, s: &str);                           // ConPrintf
}
pub struct FileEntry { pub name: String, pub is_dir: bool, pub size: u64, pub modified: i64 }
pub trait Fs {                                                  // io.open / NewFileSearch / MakeDir / RemoveDir / os.*
    fn read(&self, path: &str) -> std::io::Result<Vec<u8>>;
    fn write(&mut self, path: &str, data: &[u8]) -> std::io::Result<()>;
    fn search(&self, spec: &str, find_dirs: bool) -> Vec<FileEntry>;
    fn make_dir(&mut self, path: &str) -> std::io::Result<()>;
    fn remove_dir(&mut self, path: &str, recursive: bool) -> std::io::Result<()>;
    fn rename(&mut self, from: &str, to: &str) -> std::io::Result<()>;
    fn remove(&mut self, path: &str) -> std::io::Result<()>;
}
pub struct HttpRequest { pub url: String, pub headers: Vec<String>, pub body: Option<Vec<u8>>, pub user_agent: Option<String> }
// PoB's connectionProtocol / proxyURL / noSSL (Launch.lua:283-315) have no field yet: the native Net ignores them (CCR-0026 C, A8 needs-code).
pub struct HttpResponse { pub body: Option<Vec<u8>>, pub error: Option<String>, pub headers: String }
pub trait Net {                                                 // launch:DownloadPage (LaunchSubScript+curl)
    fn download_page(&mut self, req: HttpRequest) -> u64;
    /// Completions are delivered by the host BEFORE on_frame (ui_main.cpp subscript order).
    fn poll(&mut self) -> Vec<(u64, HttpResponse)>;
    fn oauth_listen(&mut self, url: &str) -> Option<u64> { let _ = url; None } // desktop only
    // Its completion arrives through poll() as pob_host::OAuthResult { code, err, state, port }::into_response()
    // (body = code, error = errMsg, headers = JSON {state, port}); U43 decodes with OAuthResult::from_response (CCR-0026 B).
    fn busy(&self) -> bool;                                     // "hasSubscript" for frame pacing
}

pub struct Sys {
    pub platform: Box<dyn Platform>, pub fs: Box<dyn Fs>, pub net: Box<dyn Net>,
    cursor_phys: (i32, i32),  // snapshotted at begin_frame (GetRelativeCursor)
    scale: f32,               // Sg::scale() at begin_frame
    keys: std::collections::HashSet<Key>, // heldKeyState, updated by the host before each key callback
}
impl Sys {
    pub fn new(platform: Box<dyn Platform>, fs: Box<dyn Fs>, net: Box<dyn Net>) -> Sys;   // CCR-0013 A5
    pub fn begin_frame(&mut self, cursor_phys: (i32, i32), scale: f32);
    /// GetCursorPos: lround(cursorPhys / dpiScale) (int / float)
    pub fn cursor_pos(&self) -> (Num, Num) {
        (((self.cursor_phys.0 as f32) / self.scale).round() as f64,
         ((self.cursor_phys.1 as f32) / self.scale).round() as f64)
    }
    pub fn is_key_down(&self, k: Key) -> bool;
    pub fn set_key(&mut self, k: Key, down: bool);
    pub fn time_ms(&self) -> i64 { self.platform.time_ms() }
}
```

```rust
// pob-sg/src/trace.rs
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImgRef { pub path: String, pub flags: ImageFlags }

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ApiCall {
    SetDrawLayer { layer: Option<i32>, sub: Option<i32> },
    SetViewport(Option<[f64; 4]>),
    SetDrawColor([f64; 4]),
    SetDrawColorEsc(String),
    DrawImage { img: Option<ImgRef>, rect: [f64; 4], tc: Option<[f64; 4]> },
    DrawImageQuad { img: Option<ImgRef>, p: [f64; 8], tc: Option<[f64; 8]> },
    DrawString { x: f64, y: f64, align: Align, height: f64, font: Font, text: String },
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SideEffect {
    ImageLoad(ImgRef), WindowTitle(String), Copy(String), OpenUrl(String),
    FileWrite { path: String, xxh3: u64 }, Screenshot, Exit,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CallRec { pub call: ApiCall, pub ctl: Option<String>, pub src: Option<String> }
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct FrameTrace { pub index: u32, pub screen: Option<ScreenInfo>, pub calls: Vec<CallRec>, pub effects: Vec<SideEffect> }

/// One draw with the state it was issued under: the unit of comparison.
/// `ctl` (control path, e.g. "BuildMode/NotesTab/toggleColorCodes") and `src` are excluded from equality.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DrawOp { pub viewport: [f64; 4], pub color: [f32; 4], pub call: ApiCall, pub ctl: Option<String>, pub src: Option<String> }
pub type Buckets = std::collections::BTreeMap<(i32, i32), Vec<DrawOp>>;

/// Recording (frozen by UI-U0, CCR-0013 A4): `Sg.trace: Option<TraceRecorder>`; `ApiCall::draw_image(img: Option<ImgRef>,
/// rect, tc)` and siblings build the calls; `Sg` resolves an `ImageHandle` to its `ImgRef` before recording.
#[derive(Default)]
pub struct TraceRecorder { pub frames: Vec<FrameTrace>, pub cur: FrameTrace, /* ctl path stack, src */ }
impl TraceRecorder {
    pub fn new() -> TraceRecorder; pub fn begin_frame(&mut self, screen: ScreenInfo); pub fn end_frame(&mut self);
    pub fn record(&mut self, call: ApiCall, caller: &std::panic::Location<'_>); pub fn effect(&mut self, e: SideEffect);
    pub fn ctl_push(&mut self, path_elem: &str); pub fn ctl_pop(&mut self); pub fn set_src(&mut self, src: Option<String>);
}

/// Replays SetDrawLayer/SetViewport/SetDrawColor semantics, including the DrawString escape leak
/// (the colour after a DrawString is the last escape in the text with alpha 1, else unchanged).
pub fn normalize(t: &FrameTrace, screen: &ScreenInfo) -> Buckets;
pub struct DiffConfig {
    pub abs_eps: f64,                                   // default 1e-6 on f64 args; colours exact
    pub multiset: Vec<((i32, i32), (i32, i32))>,        // bucket ranges compared order-insensitively
    pub masks: Vec<TextMask>,                           // ignore DrawString text in a rect (open calc issues only)
}
pub struct TextMask { pub rect: [f64; 4], pub pattern: String, pub issue: String }
pub enum Divergence {
    Missing { bucket: (i32, i32), idx: usize, op: DrawOp },
    Extra { bucket: (i32, i32), idx: usize, op: DrawOp },
    Changed { bucket: (i32, i32), idx: usize, lua: DrawOp, rust: DrawOp },
    Effect { lua: Option<SideEffect>, rust: Option<SideEffect> },
}
pub fn diff(lua: &Buckets, rust: &Buckets, cfg: &DiffConfig) -> Vec<Divergence>;
```

---

## 5. GPU backend and host

### 5.1 `pob-sg-wgpu`

```rust
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex { pos: [f32; 2], uv: [f32; 2], tint: [f32; 4] }

pub struct SgRenderer { /* pipeline (Rgba8Unorm target, blend SrcAlpha/OneMinusSrcAlpha for colour AND alpha),
                           blit pipeline, samplers[8] indexed by CLAMP|NEAREST|MIPMAP, textures: HashMap<TexId, ..>,
                           vbuf, static ibuf, rt (fb size, Rgba8Unorm), last_digest, max_tex_dim */ }
impl SgRenderer {
    pub fn new(device: &wgpu::Device, adapter_limits: &wgpu::Limits, output: wgpu::TextureFormat) -> Self;
    /// Uploads (downscaling above max_tex_dim), builds vertices and draw calls.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &pob_sg::Frame);
    /// Clears the RT to opaque black, renders, then blits to `output` (a non-sRGB view).
    pub fn render(&mut self, encoder: &mut wgpu::CommandEncoder, output: &wgpu::TextureView);
    pub fn read_back(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> (u32, u32, Vec<u8>);
}
```

```wgsl
struct Globals { screen: vec2<f32>, _pad: vec2<f32> };
@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var smp: sampler;
struct VIn  { @location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) tint: vec4<f32> };
struct VOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32>, @location(1) tint: vec4<f32> };
@vertex fn vs(v: VIn) -> VOut {
  var o: VOut;
  o.pos = vec4<f32>(mix(vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0), v.pos / g.screen), 0.0, 1.0);
  o.uv = v.uv; o.tint = v.tint; return o;
}
@fragment fn fs(i: VOut) -> @location(0) vec4<f32> { return textureSample(tex, smp, i.uv) * i.tint; }
```

How `prepare` works:

- It walks the sorted queues, replaying `Viewport`/`Bind`/`Color` as state and pushing 4 vertices per `Quad` at `x + vp.x, y + vp.y`.
- A new draw call starts whenever the scissor or texture changes. SG batches several textures per call, which yields the same pixels.
- Scissors are clamped to the RT; empty ones are skipped.
- Grey textures (`@white`, default) expand to RGBA. `@black` is RGBA zero.
- Anisotropy is 16 only when all filters are linear, which is a wgpu constraint.
- Surface: choose a non-sRGB format. If only sRGB formats are offered, configure `view_formats = [fmt.remove_srgb_suffix()]` and blit into that view.
- WebGL2: one bind group per texture, downlevel limits, `max_texture_dimension_2d` taken from the adapter.

### 5.2 `pob-host`

```rust
pub trait App: 'static {
    fn sg(&mut self) -> &mut pob_sg::Sg;
    fn sys(&mut self) -> &mut pob_sg::Sys;
    fn on_init(&mut self);                                  // launch:OnInit
    fn on_frame(&mut self);                                 // launch:OnFrame (between sg.begin_frame/end_frame)
    fn on_key_down(&mut self, key: Key, double_click: bool);
    fn on_key_up(&mut self, key: Key);
    fn on_char(&mut self, ch: Key);
    fn can_exit(&mut self) -> bool { true }
    fn on_exit(&mut self) {}
    /// hasActiveCoroutine || hasSubscript (ui_main.cpp:389): keeps frames running while unfocused
    fn wants_frames(&self) -> bool { false }
    /// A callback panicked with a LuaError (D17): show Launch's error prompt (Launch.lua:381 ShowErrMsg).
    fn on_lua_error(&mut self, msg: String);
    /// A `Net` request finished (`launch:OnSubFinished(id, ...)` / `OnSubError`): called inside the frame, after
    /// `sg.begin_frame`/`sys.begin_frame` and before `on_frame`, once per completion in `Net::poll` order. Default: drop (CCR-0026 A).
    fn on_net(&mut self, id: u64, resp: pob_sg::HttpResponse) { let _ = (id, resp); }
    /// A `.pobscript` step only the app can perform (`load-build`, `snapshot`, app `fixture` keys), called by
    /// `run_headless` between frames. Default: no-op (CCR-0026 E).
    fn on_script_step(&mut self, step: &script::Step) { let _ = step; }
}
pub struct PlatformParts { pub sg: pob_sg::Sg, pub sys: pob_sg::Sys }  // built by the host (fonts loaded, loader installed)
pub struct HostConfig { pub title: String, pub size: (u32, u32), pub min_size: (u32, u32), pub pob_src: String, pub sg_runtime: String }

pub fn run_desktop<A: App>(cfg: HostConfig, make: impl FnOnce(PlatformParts) -> A) -> anyhow::Result<()>;
#[cfg(target_arch = "wasm32")]
pub async fn run_web<A: App>(canvas_id: &str, cfg: HostConfig, make: impl FnOnce(PlatformParts) -> A) -> Result<(), wasm_bindgen::JsValue>;
/// Deterministic and windowless: virtual clock (+16 ms per frame), virtual clipboard, fixture Fs, scripted Net,
/// synchronous image loads. Used by tests and by `ui-parity`.
pub fn run_headless<A: App>(app: &mut A, script: &script::Script) -> Vec<pob_sg::trace::FrameTrace>;
```

Desktop host rules (SG parity, §2):

- **Cursor.** `CursorMoved` stores the physical position, which is snapshotted at `begin_frame`.
- **Mouse buttons.** Map to `*Button`/`Mouse4`/`Mouse5`, with double-click detection (500 ms, ±5 px, memory reset after a double). `sys.set_key` runs before the callback.
- **Wheel.** `LineDelta` gives one Down+Up pair per event. `PixelDelta` accumulates and gives one pair per 40 logical px.
- **Keyboard.**
  - A press or repeat calls `on_key_down`, followed by the extra `on_char` for Back/Tab/Return/Escape.
  - A release calls `on_key_up`, except F10 and Shift+Pause.
  - Printable text gives one `on_char(Char(b))` per ASCII byte, or `Unknown` for non-ASCII. Control characters coming from text events are dropped.
- **Window.**
  - `Resized`/`ScaleFactorChanged` reconfigure: `ScreenInfo { fb = inner_size, win_h = inner_size.h, dpi = scale_factor }`.
  - `CloseRequested` calls `can_exit`, then `on_exit` and exits.
- **Frame order** (`pob_host::run_frame`, shared by the desktop, web and headless hosts; `ui_main.cpp:393-433`). `sg.begin_frame`, `sys.begin_frame` (cursor snapshot), then each `Net::poll` completion to `App::on_net(id, resp)` (one guarded callback each; U16's port of `launch:OnSubFinished`/`OnSubError`), then `on_frame`, `sg.end_frame`, prepare/render, present (CCR-0026 A). `wants_frames` is `App::wants_frames() || sys.net.busy()`.
- **Pacing.** Run frames while focused, hovered, or `wants_frames()` is true; otherwise wait 100 ms between frames. Always run 11 frames after boot (CCR-0026 F). The desktop host requests a redraw when the next frame is due and waits otherwise (`WaitUntil` the 100 ms idle sleep), presenting with `AutoVsync`: SG's cadence without busy-polling winit (CCR-0026 G). `Restart()` runs `OnExit` and re-executes the binary.
- **Scripts** (`pob_host::script`, U7; CCR-0026 E). `Script { steps, base_dir: Option<PathBuf> }` (fixture paths resolve against the script's directory; `Script::load(path)`), `Step::Exit` (`CanExit`, then `OnExit`), `script::Action` + `Script::expand()`: the primitive actions every runner replays (the oracle drives its LuaJIT side from `expand()` too). `click K` = frame, KeyDown, frame, KeyUp, frame; `type` sends one OnChar per codepoint as SG would (`\b \t \r \x1b` as KeyDown+Char+KeyUp; other control characters dropped; U+00A0..U+00AA as key names; other non-ASCII `?`); `move X Y` is in virtual px (physical = `lround(v * scale)`), without a frame; mouse KeyDowns get SG's double-click detection on the virtual clock (off until the first `move`). Headless fixtures: `userdir=DIR` (PoB reads `DIR/Path of Building/`; copy-on-write overlay, writes recorded as `SideEffect::FileWrite`), `net=FILE.json` (a JSON array `[{url, body | null, body_file, error, headers}]`; a request takes the first unused fixture for its URL, else the last; an unknown URL gets an empty body with `error = "No net fixture for <url>"`; `body: null` is a sub-script error), delivered at the next frame's poll.
- **Panics.** Each callback runs inside `catch_unwind(AssertUnwindSafe(..))`. On panic, call `App::on_lua_error(msg)` (Launch's `ShowErrMsg`).
- **Clipboard and network.**
  - Clipboard: `arboard`.
  - `Net`: reuses the ureq logic of `__mk2_http`, plus `oauth_listen`/`open_url` lifted from today's `crates/pob-calc/src/jit.rs:29-132` (`crates/pob-luahost/src/jit.rs` after calc:REN).

Web host:

- Same handler, via winit's web backend. The canvas size is CSS size × devicePixelRatio.
- The VFS prefetch follows §4.5. Lazy ASYNC fetch decodes one image per frame.
- Paste is captured from the DOM `paste` event: cache the text, then synthesise Ctrl+V. Copy uses `navigator.clipboard.writeText`.
- `Fs` reuses the ideas of `crates/pob-ui/src/build_store_wasm.rs` (IndexedDB).
- Unwinding is unavailable on wasm, so a panic is fatal there.

The `.pobscript` DSL is shared by the oracle and `run_headless`, and owned by U7:

```
screen 1280 800 dpi 1.0
fixture userdir=tests/ui-fixtures/user1        # Settings.xml + Builds/
load-build "Builds/witch arc.xml"              # == main:SetMode("BUILD", false, name, xml)
frames 2
move 650 300
click LEFTBUTTON                               # move+frame, down+frame, up+frame
key-down CTRL ; key-down 6 ; key-up 6 ; key-up CTRL
type "Hello ^1world"                           # per byte: Char (+KeyDown/KeyUp for named keys), as SG delivers
press TAB                                      # KeyDown TAB + Char "\t" + frame + KeyUp TAB
wheel down 3                                   # 3x (KeyDown+KeyUp WHEELDOWN)
paste-clipboard "..."                          # sets the virtual clipboard
frames 3
snapshot notes_typed                           # dumps build XML from both sides too
```

---
## 6. Control framework (`pob-ui-core`)

### 6.1 Ids, keys, viewport

```rust
// pob-ui-core/src/ids.rs
slotmap::new_key_type! {
    /// A control or control-like node (tab, mode, popup, anchor helper). Generation-checked.
    pub struct CtrlId;
    pub struct TabGroupId;
}

/// Typed handle: what a Lua field like `self.controls.edit` holds when the class is known. Copy; never owns.
pub struct Ctrl<W>(pub CtrlId, std::marker::PhantomData<fn() -> W>);
impl<W> Ctrl<W> {
    pub const fn from_raw(id: CtrlId) -> Self { Ctrl(id, std::marker::PhantomData) }
    pub const fn id(self) -> CtrlId { self.0 }
    pub fn null() -> Self { Ctrl(CtrlId::default(), std::marker::PhantomData) } // placeholder until wired; use panics
}
impl<W> Clone for Ctrl<W> { fn clone(&self) -> Self { *self } }
impl<W> Copy for Ctrl<W> {}
impl<W> PartialEq for Ctrl<W> { fn eq(&self, o: &Self) -> bool { self.0 == o.0 } }
impl<W> Eq for Ctrl<W> {}
impl<W> std::fmt::Debug for Ctrl<W> { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "Ctrl({:?})", self.0) } }
impl<W> From<Ctrl<W>> for CtrlId { fn from(c: Ctrl<W>) -> CtrlId { c.0 } }

/// A key in a Lua `controls` table.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CtrlKey { Idx(i64), Name(std::borrow::Cow<'static, str>) }
impl From<&'static str> for CtrlKey { fn from(s: &'static str) -> Self { CtrlKey::Name(s.into()) } }
impl From<String> for CtrlKey { fn from(s: String) -> Self { CtrlKey::Name(s.into()) } }
impl From<i64> for CtrlKey { fn from(i: i64) -> Self { CtrlKey::Idx(i) } }

/// Lua viewPort table {x, y, width, height}.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ViewPort { pub x: f64, pub y: f64, pub width: f64, pub height: f64 }
```

### 6.2 `Prop<T>`

```rust
// pob-ui-core/src/prop.rs
use std::rc::Rc;
use crate::{cx::Cx, ids::CtrlId};

pub type PropFn<T> = Rc<dyn Fn(&Cx, CtrlId) -> T>;

/// A Lua field that may hold a value or a function (ControlClass:GetProperty).
pub enum Prop<T: 'static> { Val(T), Fn(PropFn<T>) }

impl<T: Clone + 'static> Prop<T> {
    /// GetProperty: `me` is the control that owns the property (Lua passes `self`).
    #[inline] pub fn get(&self, cx: &Cx, me: CtrlId) -> T { match self { Prop::Val(v) => v.clone(), Prop::Fn(f) => f(cx, me) } }
    pub fn f(f: impl Fn(&Cx, CtrlId) -> T + 'static) -> Self { Prop::Fn(Rc::new(f)) }
    pub fn is_fn(&self) -> bool { matches!(self, Prop::Fn(_)) }
    /// Lua RAW field read (`ctrl.width` without GetProperty) where the field holds a value.
    /// A function there would be a Lua arithmetic/concat error: lua_error!.
    #[track_caller] pub fn raw(&self) -> T { match self { Prop::Val(v) => v.clone(), Prop::Fn(_) => crate::lua_error!("raw read of a function-valued property") } }
}
impl<T: Clone> Clone for Prop<T> {
    fn clone(&self) -> Self { match self { Prop::Val(v) => Prop::Val(v.clone()), Prop::Fn(f) => Prop::Fn(f.clone()) } }
}
impl From<f64> for Prop<f64> { fn from(v: f64) -> Self { Prop::Val(v) } }
impl From<i32> for Prop<f64> { fn from(v: i32) -> Self { Prop::Val(v as f64) } }
impl From<bool> for Prop<bool> { fn from(v: bool) -> Self { Prop::Val(v) } }
impl From<&str> for Prop<String> { fn from(v: &str) -> Self { Prop::Val(v.to_owned()) } }
impl From<String> for Prop<String> { fn from(v: String) -> Self { Prop::Val(v) } }
impl From<&str> for Prop<Option<String>> { fn from(v: &str) -> Self { Prop::Val(Some(v.to_owned())) } }
impl From<Option<String>> for Prop<Option<String>> { fn from(v: Option<String>) -> Self { Prop::Val(v) } }
// Closures are wrapped explicitly with Prop::f(..). There is no blanket From<F>, to keep coherence and readability.
```

### 6.3 Anchors, `Base`, exact `GetPos`/`GetSize`/`IsShown`

```rust
// pob-ui-core/src/layout.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorPoint { TopLeft, Top, TopRight, Right, BottomRight, Bottom, BottomLeft, Left, Center }
impl AnchorPoint {
    /// Control.lua:11-21 anchorPos
    pub const fn factors(self) -> (f64, f64) {
        match self {
            Self::TopLeft => (0.0, 0.0), Self::Top => (0.5, 0.0), Self::TopRight => (1.0, 0.0),
            Self::Right => (1.0, 0.5), Self::BottomRight => (1.0, 1.0), Self::Bottom => (0.5, 1.0),
            Self::BottomLeft => (0.0, 1.0), Self::Left => (0.0, 0.5), Self::Center => (0.5, 0.5),
        }
    }
    pub fn from_lua(s: &str) -> Option<AnchorPoint>;
}

#[derive(Clone, Debug, Default)]
pub struct Anchor {
    pub point: Option<AnchorPoint>,
    pub other: Option<CtrlId>,
    /// PopupDialog only: Lua `anchor.other` given as a sibling key string; resolved when the popup opens.
    pub other_key: Option<CtrlKey>,
    pub other_point: Option<AnchorPoint>,
    pub collapse: bool,
}
impl Anchor {
    pub const NONE: Anchor = Anchor { point: None, other: None, other_key: None, other_point: None, collapse: false };
    /// Lua {"POINT", other, "OTHERPOINT"}
    pub fn new(point: AnchorPoint, other: impl Into<CtrlId>, other_point: AnchorPoint) -> Anchor {
        Anchor { point: Some(point), other: Some(other.into()), other_point: Some(other_point), ..Anchor::NONE }
    }
    /// Lua {"POINT", nil, "OTHERPOINT"} (inside a popup, `nil` means the popup)
    pub fn nil(point: AnchorPoint, other_point: AnchorPoint) -> Anchor {
        Anchor { point: Some(point), other_point: Some(other_point), ..Anchor::NONE }
    }
    /// Lua {"POINT", "siblingKey", "OTHERPOINT"} (popups only)
    pub fn key(point: AnchorPoint, key: &'static str, other_point: AnchorPoint) -> Anchor {
        Anchor { point: Some(point), other_key: Some(key.into()), other_point: Some(other_point), ..Anchor::NONE }
    }
    /// 4th element `true`
    pub fn collapse(mut self) -> Anchor { self.collapse = true; self }
}

pub struct Base {
    pub rect_start: [Prop<f64>; 4],                // self.rectStart
    pub x: Prop<f64>, pub y: Prop<f64>, pub width: Prop<f64>, pub height: Prop<f64>,
    pub anchor: Anchor,
    pub collapse_x: f64, pub collapse_y: f64,      // `self.collapseX or 0`
    pub shown: Prop<bool>, pub enabled: Prop<bool>,
    /// nil until the first SetFocus. Control.lua:141 compares `focus ~= self.hasFocus`, so
    /// SetFocus(false) on a never-focused control DOES call OnFocusLost.
    pub has_focus: Option<bool>,
    pub tab_order: Option<TabGroupId>,
    pub mouse_clip_rect: Option<Prop<Option<[f64; 4]>>>,
}
impl Base {
    /// ControlClass:Control(anchor, rect) (Control.lua:55-66). Pass rect(0,0,0,0) for a nil rect.
    pub fn new(anchor: Anchor, rect: [Prop<f64>; 4]) -> Base {
        let [x, y, width, height] = rect.clone();
        Base { rect_start: rect, x, y, width, height, anchor, collapse_x: 0.0, collapse_y: 0.0,
               shown: Prop::Val(true), enabled: Prop::Val(true), has_focus: None, tab_order: None, mouse_clip_rect: None }
    }
}
pub fn rect(x: impl Into<Prop<f64>>, y: impl Into<Prop<f64>>, w: impl Into<Prop<f64>>, h: impl Into<Prop<f64>>) -> [Prop<f64>; 4] {
    [x.into(), y.into(), w.into(), h.into()]
}

/// Control.lua:92-119. Exact, including floor and WHEN sizes are queried (dynamic dispatch via cx.pos/cx.size).
pub fn base_pos(cx: &Cx, id: CtrlId) -> (f64, f64) {
    let b = cx.base(id);
    if b.anchor.collapse {
        if let Some(o) = b.anchor.other {
            if !cx.base(o).shown.get(cx, o) {                  // other:GetProperty("shown"), NOT IsShown()
                let (x, y) = cx.pos(o);
                return (x + b.collapse_x, y + b.collapse_y);
            }
        }
    }
    let mut x = b.x.get(cx, id);
    let mut y = b.y.get(cx, id);
    if let Some(o) = b.anchor.other {
        let (ox, oy) = cx.pos(o);
        let op = b.anchor.other_point.unwrap_or_else(|| crate::lua_error!("invalid anchor position 'nil'"));
        let (ow, oh) = if op != AnchorPoint::TopLeft { cx.size(o) } else { (0.0, 0.0) };
        let p = b.anchor.point.unwrap_or_else(|| crate::lua_error!("invalid anchor position 'nil'"));
        let (w, h) = if p != AnchorPoint::TopLeft { cx.size(id) } else { (0.0, 0.0) };
        let ((ofx, ofy), (fx, fy)) = (op.factors(), p.factors());
        x = (ox + ow * ofx + x - w * fx).floor();
        y = (oy + oh * ofy + y - h * fy).floor();
    }
    (x, y)
}
/// Control.lua:121-123
pub fn base_size(cx: &Cx, id: CtrlId) -> (f64, f64) { let b = cx.base(id); (b.width.get(cx, id), b.height.get(cx, id)) }
/// Control.lua:125-127
pub fn base_is_shown(cx: &Cx, id: CtrlId) -> bool {
    let b = cx.base(id);
    (b.anchor.other.is_none() || b.anchor.collapse || cx.is_shown(b.anchor.other.unwrap())) && b.shown.get(cx, id)
}
/// Control.lua:129-131
pub fn base_is_enabled(cx: &Cx, id: CtrlId) -> bool { cx.base(id).enabled.get(cx, id) }
/// Control.lua:133-138
pub fn base_is_mouse_in_bounds(cx: &Cx, id: CtrlId) -> bool {
    let (x, y) = cx.pos(id); let (w, h) = cx.size(id); let (cx_, cy) = cx.sys.cursor_pos();
    cx_ >= x && cy >= y && cx_ < x + w && cy < y + h
}
/// Common.lua:413-416 isMouseInRegion
pub fn is_mouse_in_region(cx: &Cx, r: &ViewPort) -> bool {
    let (x, y) = cx.sys.cursor_pos();
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}
```

### 6.4 Widgets: trait, capabilities, vtable, per-instance overrides, construction

```rust
// pob-ui-core/src/widget.rs
use std::{any::{Any, TypeId}, rc::Rc};

bitflags::bitflags! {
    /// Which optional Lua methods exist (`if control.OnKeyDown then`). Effective caps = class CAPS | overrides.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Caps: u32 {
        const DRAW = 1 << 0; const IS_MOUSE_OVER = 1 << 1; const KEY_DOWN = 1 << 2; const KEY_UP = 1 << 3;
        const CHAR = 1 << 4; const HOVER_KEY_UP = 1 << 5; const FOCUS_GAINED = 1 << 6; const FOCUS_LOST = 1 << 7;
        const CLICK = 1 << 8; const CAN_RECEIVE_DRAG = 1 << 9; const RECEIVE_DRAG = 1 << 10;
    }
}
/// The return of OnKeyDown/OnKeyUp/OnChar: `self` => Some(me), nil => None, another control => Some(it).
pub type Focus = Option<CtrlId>;
/// Duck-typed field checks made by the framework (ControlHost.lua:20, :55).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Duck { SelDragActiveWithTargets /* selDragActive and dragTargetList */, ImbuedSelectDropped /* imbuedSelect and dropped */ }
/// A drag payload (ListControl GetDragValue => type, value).
#[derive(Clone)] pub struct DragValue(pub Rc<dyn Any>);

/// A Lua control class. All methods are ASSOCIATED FUNCTIONS: `me` is Lua's `self`.
/// Methods not in CAPS are never called by the framework; their defaults raise the Lua
/// "attempt to call a nil value" error, which is what the Lua would do.
pub trait Widget: Any + Sized {
    const NAME: &'static str;
    const CAPS: Caps;
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) { let _ = (cx, vp, no_tooltip); nil_method::<Self>(me, "Draw") }
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool { let _ = cx; nil_method::<Self>(me, "IsMouseOver") }
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, double_click: bool) -> Focus { let _ = (cx, key, double_click); nil_method::<Self>(me, "OnKeyDown") }
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus { let _ = (cx, key); nil_method::<Self>(me, "OnKeyUp") }
    fn on_char(cx: &mut Cx, me: CtrlId, ch: Key) -> Focus { let _ = (cx, ch); nil_method::<Self>(me, "OnChar") }
    fn on_hover_key_up(cx: &mut Cx, me: CtrlId, key: Key) { let _ = (cx, key); nil_method::<Self>(me, "OnHoverKeyUp") }
    fn on_focus_gained(cx: &mut Cx, me: CtrlId) { let _ = cx; nil_method::<Self>(me, "OnFocusGained") }
    fn on_focus_lost(cx: &mut Cx, me: CtrlId) { let _ = cx; nil_method::<Self>(me, "OnFocusLost") }
    fn click(cx: &mut Cx, me: CtrlId) { let _ = cx; nil_method::<Self>(me, "Click") }
    fn can_receive_drag(cx: &Cx, me: CtrlId, ty: &str, v: &DragValue) -> bool { let _ = (cx, ty, v); nil_method::<Self>(me, "CanReceiveDrag") }
    fn receive_drag(cx: &mut Cx, me: CtrlId, ty: &str, v: &DragValue, source: CtrlId) { let _ = (cx, ty, v, source); nil_method::<Self>(me, "ReceiveDrag") }
    // Always-present Control methods (overridable by subclasses):
    fn get_pos(cx: &Cx, me: CtrlId) -> (f64, f64) { base_pos(cx, me) }
    fn get_size(cx: &Cx, me: CtrlId) -> (f64, f64) { base_size(cx, me) }
    fn is_shown(cx: &Cx, me: CtrlId) -> bool { base_is_shown(cx, me) }
    fn is_enabled(cx: &Cx, me: CtrlId) -> bool { base_is_enabled(cx, me) }
    fn duck(cx: &Cx, me: CtrlId, q: Duck) -> bool { let _ = (cx, me, q); false }
    /// Embedding upcast (D5). Default: only Self. Subclasses chain to their embedded base (see impl_parts!).
    fn part(w: &Self, ty: TypeId) -> Option<&dyn Any> { (ty == TypeId::of::<Self>()).then_some(w as &dyn Any) }
    fn part_mut(w: &mut Self, ty: TypeId) -> Option<&mut dyn Any> { (ty == TypeId::of::<Self>()).then_some(w as &mut dyn Any) }
}
#[track_caller] pub fn nil_method<W: Widget>(me: CtrlId, m: &str) -> ! {
    crate::lua_error!("attempt to call method '{}' (a nil value) on {} {:?}", m, W::NAME, me)
}

/// `impl_parts!(GemSelectControl: edit => EditControl)` expands to the two `Widget` methods `part`/`part_mut`
/// (chaining to the embedded base), so it is invoked INSIDE `impl Widget for …` (CCR-0013 B4).
#[macro_export] macro_rules! impl_parts { /* frozen by UI-U0; U4 owns it */ }

pub struct VTable {
    pub name: &'static str,
    pub caps: Caps,
    pub draw: fn(&mut Cx, CtrlId, &ViewPort, bool),
    pub is_mouse_over: fn(&Cx, CtrlId) -> bool,
    pub on_key_down: fn(&mut Cx, CtrlId, Key, bool) -> Focus,
    pub on_key_up: fn(&mut Cx, CtrlId, Key) -> Focus,
    pub on_char: fn(&mut Cx, CtrlId, Key) -> Focus,
    pub on_hover_key_up: fn(&mut Cx, CtrlId, Key),
    pub on_focus_gained: fn(&mut Cx, CtrlId),
    pub on_focus_lost: fn(&mut Cx, CtrlId),
    pub click: fn(&mut Cx, CtrlId),
    pub can_receive_drag: fn(&Cx, CtrlId, &str, &DragValue) -> bool,
    pub receive_drag: fn(&mut Cx, CtrlId, &str, &DragValue, CtrlId),
    pub get_pos: fn(&Cx, CtrlId) -> (f64, f64),
    pub get_size: fn(&Cx, CtrlId) -> (f64, f64),
    pub is_shown: fn(&Cx, CtrlId) -> bool,
    pub is_enabled: fn(&Cx, CtrlId) -> bool,
    pub duck: fn(&Cx, CtrlId, Duck) -> bool,
    pub part: fn(&dyn Any, TypeId) -> Option<&dyn Any>,
    pub part_mut: fn(&mut dyn Any, TypeId) -> Option<&mut dyn Any>,
}
fn part_shim<W: Widget>(s: &dyn Any, ty: TypeId) -> Option<&dyn Any> { W::part(s.downcast_ref::<W>().expect("vtable/state mismatch"), ty) }
fn part_mut_shim<W: Widget>(s: &mut dyn Any, ty: TypeId) -> Option<&mut dyn Any> { W::part_mut(s.downcast_mut::<W>().expect("vtable/state mismatch"), ty) }
pub fn vtable_of<W: Widget>() -> &'static VTable {
    const {
        &VTable { name: W::NAME, caps: W::CAPS, draw: W::draw, is_mouse_over: W::is_mouse_over,
                  on_key_down: W::on_key_down, on_key_up: W::on_key_up, on_char: W::on_char,
                  on_hover_key_up: W::on_hover_key_up, on_focus_gained: W::on_focus_gained,
                  on_focus_lost: W::on_focus_lost, click: W::click, can_receive_drag: W::can_receive_drag,
                  receive_drag: W::receive_drag, get_pos: W::get_pos, get_size: W::get_size,
                  is_shown: W::is_shown, is_enabled: W::is_enabled, duck: W::duck,
                  part: part_shim::<W>, part_mut: part_mut_shim::<W> }
    }
}

/// Per-instance method assignment (`ctrl.Draw = function(control) … end`, `function ctrl:OnFocusLost() … end`).
/// Dispatch checks these first. A Some(..) also turns the matching Caps bit on.
#[derive(Default, Clone)]
pub struct Overrides {
    pub draw: Option<Rc<dyn Fn(&mut Cx, CtrlId, &ViewPort, bool)>>,
    pub is_mouse_over: Option<Rc<dyn Fn(&Cx, CtrlId) -> bool>>,
    pub on_key_down: Option<Rc<dyn Fn(&mut Cx, CtrlId, Key, bool) -> Focus>>,
    pub on_key_up: Option<Rc<dyn Fn(&mut Cx, CtrlId, Key) -> Focus>>,
    pub on_char: Option<Rc<dyn Fn(&mut Cx, CtrlId, Key) -> Focus>>,
    pub on_hover_key_up: Option<Rc<dyn Fn(&mut Cx, CtrlId, Key)>>,
    pub on_focus_gained: Option<Rc<dyn Fn(&mut Cx, CtrlId)>>,
    pub on_focus_lost: Option<Rc<dyn Fn(&mut Cx, CtrlId)>>,
    pub can_receive_drag: Option<Rc<dyn Fn(&Cx, CtrlId, &str, &DragValue) -> bool>>,
    pub receive_drag: Option<Rc<dyn Fn(&mut Cx, CtrlId, &str, &DragValue, CtrlId)>>,
    pub get_size: Option<Rc<dyn Fn(&Cx, CtrlId) -> (f64, f64)>>,
    /// ControlHost methods overridden on an instance (Main.lua:1297, :1307)
    pub draw_controls: Option<Rc<dyn Fn(&mut Cx, CtrlId, &ViewPort, Option<CtrlId>)>>,
    pub get_mouse_over_control: Option<Rc<dyn Fn(&Cx, CtrlId) -> Option<CtrlId>>>,
}
impl Overrides { pub fn caps(&self) -> Caps; }

/// A not-yet-inserted control: what `new("X"):X(...)` evaluates to.
pub struct New<W: Widget> {
    pub base: Base,
    pub w: W,
    pub(crate) is_host: bool,
    pub(crate) inits: Vec<Box<dyn FnOnce(&mut Cx, Ctrl<W>)>>,
    pub(crate) mixins: Vec<Box<dyn FnOnce(&mut Cx, Ctrl<W>)>>,
}
impl<W: Widget> New<W> {
    pub fn new(base: Base, w: W) -> Self { New { base, w, is_host: false, inits: Vec::new(), mixins: Vec::new() } }
    /// The class is (also) a ControlHost: the node gets a `controls` table.
    pub fn host(mut self) -> Self { self.is_host = true; self }
    /// Constructor tail that needs the id (children, closures over `self`). Runs right after insertion, in order.
    /// A subclass constructor chains the base's init first: `.init(|cx, me| EditControl::init(cx, me.0, ..))`.
    pub fn init(mut self, f: impl FnOnce(&mut Cx, Ctrl<W>) + 'static) -> Self { self.inits.push(Box::new(f)); self }
    /// Lua mixin call `new("X"):X(...) { k = v }`: applied AFTER every init, in call order.
    pub fn mixin(mut self, f: impl FnOnce(&mut Cx, Ctrl<W>) + 'static) -> Self { self.mixins.push(Box::new(f)); self }
    pub fn shown(self, p: Prop<bool>) -> Self { self.mixin(move |cx, c| cx.base_mut(c).shown = p) }
    pub fn enabled(self, p: Prop<bool>) -> Self { self.mixin(move |cx, c| cx.base_mut(c).enabled = p) }
    pub fn x(self, p: Prop<f64>) -> Self { self.mixin(move |cx, c| cx.base_mut(c).x = p) }
    pub fn y(self, p: Prop<f64>) -> Self { self.mixin(move |cx, c| cx.base_mut(c).y = p) }
    pub fn width(self, p: Prop<f64>) -> Self { self.mixin(move |cx, c| cx.base_mut(c).width = p) }
    pub fn height(self, p: Prop<f64>) -> Self { self.mixin(move |cx, c| cx.base_mut(c).height = p) }
}

/// `new("Control"):Control(anchor, rect)`: a pure geometry/anchor node (pointDisplay, anchorTopBarRight, sectionAnchor).
pub struct PlainControl;
impl Widget for PlainControl { const NAME: &'static str = "Control"; const CAPS: Caps = Caps::empty(); }
impl PlainControl { pub fn new(anchor: Anchor, rect: [Prop<f64>; 4]) -> New<PlainControl> { New::new(Base::new(anchor, rect), PlainControl) } }
```

### 6.5 Arena and `Cx`

```rust
// pob-ui-core/src/arena.rs
pub struct Node {
    pub base: Base,
    pub host: Option<HostData>,                      // ControlHost mixin
    pub ov: Overrides,
    pub(crate) vt: &'static VTable,
    pub(crate) state: Box<dyn std::any::Any>,        // the most-derived widget struct
    pub(crate) seq: u64,                             // creation order (D7)
    pub(crate) parent: Option<(CtrlId, CtrlKey)>,    // host membership, for markers and removal
    pub(crate) owner: Option<CtrlId>,                // freed with its owner
    pub(crate) owned: Vec<CtrlId>,
    pub(crate) buried: bool,
}
pub struct HostData {
    pub(crate) map: std::collections::BTreeMap<CtrlKey, CtrlId>,
    /// selControl (raw writes allowed: EditControl.lua:448 assigns it without SetFocus)
    pub sel_control: Option<CtrlId>,
}
pub struct Ui {
    pub(crate) nodes: slotmap::SlotMap<CtrlId, Node>,
    pub(crate) tab_groups: slotmap::SlotMap<TabGroupId, Vec<CtrlId>>,
    pub(crate) graveyard: Vec<CtrlId>,
    pub(crate) next_seq: u64,
}
impl Ui {
    // Field-level accessors, usable while another field of Cx (e.g. cx.sg) is borrowed mutably.
    #[track_caller] pub fn w<W: Widget>(&self, id: impl Into<CtrlId>) -> &W;        // via vtable `part`; panics: dead id / wrong type (names W::NAME)
    #[track_caller] pub fn w_mut<W: Widget>(&mut self, id: impl Into<CtrlId>) -> &mut W;
    pub fn try_w<W: Widget>(&self, id: CtrlId) -> Option<&W>;
    pub fn is<W: Widget>(&self, id: CtrlId) -> bool;                                 // Lua `ctrl.ListControl` is-a check
    #[track_caller] pub fn base(&self, id: impl Into<CtrlId>) -> &Base;
    #[track_caller] pub fn base_mut(&mut self, id: impl Into<CtrlId>) -> &mut Base;
    pub fn is_alive(&self, id: CtrlId) -> bool;
    pub fn class_name(&self, id: CtrlId) -> &'static str;
    #[track_caller] pub fn node(&self, id: CtrlId) -> &Node;
    #[track_caller] pub fn node_mut(&mut self, id: CtrlId) -> &mut Node;
}

// pob-ui-core/src/cx.rs
/// The single context every UI function receives: Lua's globals plus the object arena.
pub struct Cx {
    pub ui: Ui,
    pub sg: pob_sg::Sg,
    pub sys: pob_sg::Sys,
    pub main: MainCore,                    // the part of `main` generic controls use (§6.11)
    pub ext: Box<dyn std::any::Any>,       // pob-app's AppState; typed access through pob_app::CxApp
}
impl Cx {
    // ---- arena (one call per Lua assignment) ----
    /// `host.controls[key] = new(..)`. Assigns the creation seq, runs inits, then mixins. If `key` already held a
    /// control, that control is buried (D8); use `detach_child` first when the Lua keeps referencing it.
    pub fn add<W: Widget>(&mut self, host: impl Into<CtrlId>, key: impl Into<CtrlKey>, n: New<W>) -> Ctrl<W>;
    /// `local c = new(..)` not stored in a controls table (anchor helpers, tabs, popups, mode objects).
    pub fn add_free<W: Widget>(&mut self, owner: Option<CtrlId>, n: New<W>) -> Ctrl<W>;
    /// `host.controls[key] = existing` (an existing object; ownership unchanged).
    pub fn put(&mut self, host: impl Into<CtrlId>, key: impl Into<CtrlKey>, id: CtrlId);
    /// `host.controls[key] = nil` where nothing else keeps the object: detach + bury (deferred).
    pub fn remove_child(&mut self, host: impl Into<CtrlId>, key: &CtrlKey);
    /// `host.controls[key] = nil` where the object is still referenced elsewhere.
    pub fn detach_child(&mut self, host: impl Into<CtrlId>, key: &CtrlKey) -> Option<CtrlId>;
    /// wipeTable(self.controls)
    pub fn clear_children(&mut self, host: impl Into<CtrlId>);
    /// Free `id` and everything it owns at end of frame.
    pub fn bury(&mut self, id: impl Into<CtrlId>);
    /// Host calls this after on_frame. Drops buried nodes (their Rc closures drop here, outside any borrow).
    pub fn end_frame_gc(&mut self);

    #[track_caller] pub fn base(&self, id: impl Into<CtrlId>) -> &Base { self.ui.base(id) }
    #[track_caller] pub fn base_mut(&mut self, id: impl Into<CtrlId>) -> &mut Base { self.ui.base_mut(id) }
    #[track_caller] pub fn w<W: Widget>(&self, id: impl Into<CtrlId>) -> &W { self.ui.w(id) }
    #[track_caller] pub fn w_mut<W: Widget>(&mut self, id: impl Into<CtrlId>) -> &mut W { self.ui.w_mut(id) }
    #[track_caller] pub fn get<W: Widget>(&self, c: Ctrl<W>) -> &W { self.ui.w(c.0) }
    #[track_caller] pub fn get_mut<W: Widget>(&mut self, c: Ctrl<W>) -> &mut W { self.ui.w_mut(c.0) }
    pub fn try_w<W: Widget>(&self, id: CtrlId) -> Option<&W> { self.ui.try_w(id) }
    pub fn is<W: Widget>(&self, id: CtrlId) -> bool { self.ui.is::<W>(id) }
    /// GetProperty on a class-specific Prop field: clones the Prop out, then evaluates it with the whole &Cx.
    pub fn prop<W: Widget, T: Clone + 'static>(&self, me: CtrlId, f: impl FnOnce(&W) -> &Prop<T>) -> T {
        let p = f(self.w::<W>(me)).clone(); p.get(self, me)
    }
    pub fn child(&self, host: impl Into<CtrlId>, key: impl Into<CtrlKey>) -> Option<CtrlId>;
    /// Typed `self.controls.key`; a missing key panics like a Lua nil index.
    #[track_caller] pub fn child_as<W: Widget>(&self, host: impl Into<CtrlId>, key: &'static str) -> Ctrl<W>;
    /// Canonical iteration order (D7): Idx keys ascending, then Name keys by control creation seq. Snapshot.
    pub fn children(&self, host: impl Into<CtrlId>) -> smallvec::SmallVec<[CtrlId; 32]>;
    pub fn host(&self, id: impl Into<CtrlId>) -> &HostData;
    pub fn host_mut(&mut self, id: impl Into<CtrlId>) -> &mut HostData;
    /// Still a member of `host` (a control removed earlier in the same iteration is skipped, as with Lua `pairs`).
    pub fn is_child_of(&self, host: CtrlId, id: CtrlId) -> bool;
    /// Control-path markers for the trace (no-op unless tracing): "<HostClass>.<key>".
    pub fn trace_push(&mut self, host: CtrlId, id: CtrlId);
    pub fn trace_pop(&mut self);
    /// Lend a widget-owned object that needs &mut Cx for its methods (Tooltip, PassiveTreeView, Coroutine):
    /// it is taken out (Default left behind), `f` runs, then it is put back. Only for objects that nothing else
    /// reads during the call (§7.2).
    pub fn lend<W: Widget, T: Default, R>(&mut self, me: CtrlId, field: fn(&mut W) -> &mut T,
                                          f: impl FnOnce(&mut Cx, &mut T) -> R) -> R;

    // ---- Control.lua (dynamic dispatch through vtable + overrides) ----
    pub fn pos(&self, id: impl Into<CtrlId>) -> (f64, f64);       // GetPos
    pub fn size(&self, id: impl Into<CtrlId>) -> (f64, f64);      // GetSize (ov.get_size first)
    pub fn width(&self, id: impl Into<CtrlId>) -> f64;            // GetProperty("width")
    pub fn height(&self, id: impl Into<CtrlId>) -> f64;
    pub fn is_shown(&self, id: impl Into<CtrlId>) -> bool;
    pub fn is_enabled(&self, id: impl Into<CtrlId>) -> bool;
    pub fn is_mouse_in_bounds(&self, id: impl Into<CtrlId>) -> bool;
    /// Control.lua:81-90: x/y are replaced only when BOTH are Some; collapse is always overwritten.
    pub fn set_anchor(&mut self, id: impl Into<CtrlId>, point: AnchorPoint, other: Option<CtrlId>, other_point: AnchorPoint,
                      xy: Option<(Prop<f64>, Prop<f64>)>, collapse: bool);
    pub fn set_focus(&mut self, id: CtrlId, focus: bool);
    pub fn add_to_tab_group(&mut self, id: impl Into<CtrlId>, master: impl Into<CtrlId>);
    pub fn tab_advance(&self, id: CtrlId, step: i32) -> CtrlId;

    // ---- optional-method dispatch (ov first, then vtable) ----
    pub fn has(&self, id: CtrlId, c: Caps) -> bool;               // (vt.caps | ov.caps()).contains(c)
    pub fn draw(&mut self, id: CtrlId, vp: &ViewPort, no_tooltip: bool);
    pub fn is_mouse_over(&self, id: CtrlId) -> bool;
    pub fn on_key_down(&mut self, id: CtrlId, key: Key, double_click: bool) -> Focus;
    pub fn on_key_up(&mut self, id: CtrlId, key: Key) -> Focus;
    pub fn on_char(&mut self, id: CtrlId, ch: Key) -> Focus;
    pub fn on_hover_key_up(&mut self, id: CtrlId, key: Key);
    pub fn on_focus_gained(&mut self, id: CtrlId);
    pub fn on_focus_lost(&mut self, id: CtrlId);
    pub fn click(&mut self, id: CtrlId);
    pub fn can_receive_drag(&self, id: CtrlId, ty: &str, v: &DragValue) -> bool;
    pub fn receive_drag(&mut self, id: CtrlId, ty: &str, v: &DragValue, source: CtrlId);
    pub fn duck(&self, id: CtrlId, q: Duck) -> bool;
    /// Per-instance method assignment: `cx.set_ov(id, |ov| ov.on_focus_lost = Some(Rc::new(..)))`.
    pub fn set_ov(&mut self, id: impl Into<CtrlId>, f: impl FnOnce(&mut Overrides));

    // ---- ControlHost.lua (§6.6) ----
    pub fn select_control(&mut self, host: CtrlId, new: Option<CtrlId>);
    pub fn get_mouse_over_control(&self, host: CtrlId) -> Option<CtrlId>;
    pub fn process_controls_input(&mut self, host: CtrlId, ev: &mut InputEvents, vp: &ViewPort);
    pub fn draw_controls(&mut self, host: CtrlId, vp: &ViewPort, sel_control: Option<CtrlId>);
}
```

Dispatch bodies all follow one shape: clone the override `Rc` out, then call it. Otherwise copy the fn pointer out of the vtable and call it.

```rust
impl Cx {
    pub fn draw(&mut self, id: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        let n = self.ui.node(id);
        if let Some(f) = n.ov.draw.clone() { return f(self, id, vp, no_tooltip); }
        let f = n.vt.draw;
        f(self, id, vp, no_tooltip)
    }
}
```

**Arena policies as built** (U4, CCR-0034 C). `add` creates the node (creation seq), runs `inits` then `mixins`, and only then stores it under `key` (Lua evaluates `new(..):X(..){..}` before the table store). A control replaced under a key (`add`/`put`) is buried only when the host owns it and it is not still stored under another key of the host; otherwise it is only unlinked. `put` of a buried node revives it. `clear_children` (`wipeTable(self.controls)`) buries the children the host owns. `end_frame_gc` frees every buried node and, transitively, what it owns, unlinks them from surviving hosts and owners and drops tab groups with no live member. DIVERGENCE(gc): an id still held by a `selControl`, a tab group or a widget field after its node was freed panics on use ("attempt to index a dead control (expected <Class>)"), where Lua's GC would keep the object alive. Typed access to a node of another class panics with both class names; `child_as` on a missing key panics like a nil index. `trace_push(host, id)` pushes `"<HostClass>.<key>"` only while `sg.trace` is set. The T0 suites (`pob-ui-core/tests/lua/shim.lua`) load the real `Control.lua`, `ControlHost.lua` (oracle patch 6 applied by text substitution), `UndoHandler.lua` and Common.lua's `wipeTable`/`isValueInArray`/`isMouseInRegion` under LuaJIT `jit.off()`.

### 6.6 ControlHost: input dispatch and focus (1:1 with `ControlHost.lua`)

```rust
// pob-ui-core/src/host.rs
impl Cx {
    /// ControlHost.lua:15-29
    pub fn select_control(&mut self, host: CtrlId, new: Option<CtrlId>) {
        let cur = self.host(host).sel_control;
        if cur == new { return; }
        if let Some(c) = cur {
            if self.duck(c, Duck::SelDragActiveWithTargets) { return; }
            self.set_focus(c, false);
        }
        self.host_mut(host).sel_control = new;
        if let Some(n) = new { self.set_focus(n, true); }
    }

    /// ControlHost.lua:31-47 (instance override: Main.lua:1307)
    pub fn get_mouse_over_control(&self, host: CtrlId) -> Option<CtrlId> {
        if let Some(f) = self.ui.node(host).ov.get_mouse_over_control.clone() { return f(self, host); }
        for id in self.children(host) {
            if !self.is_child_of(host, id) { continue; }      // removed earlier in this iteration
            if self.has(id, Caps::IS_MOUSE_OVER) && self.is_mouse_over(id) {
                let clip = self.base(id).mouse_clip_rect.as_ref().and_then(|p| p.get(self, id));
                let Some([x, y, w, h]) = clip else { return Some(id) };
                let (cx_, cy) = self.sys.cursor_pos();
                if cx_ >= x && cy >= y && cx_ < x + w && cy < y + h { return Some(id); }
            }
        }
        None
    }

    /// ControlHost.lua:49-107
    pub fn process_controls_input(&mut self, host: CtrlId, ev: &mut InputEvents, vp: &ViewPort) {
        let mut i = 0;
        while let Some(e) = ev.next_ipairs(&mut i) {
            match e.kind {
                EventKind::KeyDown { double_click } => {
                    let prev_sel = self.host(host).sel_control;
                    if let Some(sel) = prev_sel {
                        let imbued = self.duck(sel, Duck::ImbuedSelectDropped) && e.key.is_button();
                        let next = self.on_key_down(sel, e.key, double_click);   // no caps check, as Lua :56
                        self.select_control(host, next);
                        ev.consume(i);
                        if imbued { return; }                                   // no click-through
                    }
                    if self.host(host).sel_control.is_none() && e.key.is_button() {
                        self.select_control(host, None);
                        if is_mouse_in_region(self, vp) {
                            if let Some(m) = self.get_mouse_over_control(host) {
                                if Some(m) != prev_sel && self.has(m, Caps::KEY_DOWN) {
                                    let next = self.on_key_down(m, e.key, double_click);
                                    self.select_control(host, next);
                                    ev.consume(i);
                                }
                            }
                        }
                    }
                }
                EventKind::KeyUp => {
                    let sel = self.host(host).sel_control;
                    if let Some(s) = sel {
                        if self.has(s, Caps::KEY_UP) { let n = self.on_key_up(s, e.key); self.select_control(host, n); }
                        ev.consume(i);
                    }
                    if let Some(m) = self.get_mouse_over_control(host) {
                        if sel.is_none() || self.has(m, Caps::HOVER_KEY_UP) {
                            if is_mouse_in_region(self, vp) {
                                if sel.is_none() && self.has(m, Caps::KEY_UP) && self.on_key_up(m, e.key).is_some() { ev.consume(i); }
                                if self.has(m, Caps::HOVER_KEY_UP) { self.on_hover_key_up(m, e.key); }
                            }
                        }
                    }
                }
                EventKind::Char => {
                    if let Some(s) = self.host(host).sel_control {
                        if self.has(s, Caps::CHAR) { let n = self.on_char(s, e.key); self.select_control(host, n); }
                        ev.consume(i);
                    }
                }
            }
        }
    }

    /// ControlHost.lua:109-115 (instance override: Main.lua:1297)
    pub fn draw_controls(&mut self, host: CtrlId, vp: &ViewPort, sel_control: Option<CtrlId>) {
        if let Some(f) = self.ui.node(host).ov.draw_controls.clone() { return f(self, host, vp, sel_control); }
        self.draw_controls_default(host, vp, sel_control)
    }
    /// The class method (`originalDrawControls` in Main.lua:1276).
    pub fn draw_controls_default(&mut self, host: CtrlId, vp: &ViewPort, sel_control: Option<CtrlId>) {
        for id in self.children(host) {
            if !self.is_child_of(host, id) { continue; }
            if self.is_shown(id) && self.has(id, Caps::DRAW) {
                let focused = |cx: &Cx, s: Option<CtrlId>| s.is_some_and(|s| cx.base(s).has_focus == Some(true) && s != id);
                let no_tooltip = focused(self, self.host(host).sel_control) || focused(self, sel_control);
                self.trace_push(host, id);                       // control-path marker (no-op unless tracing)
                self.draw(id, vp, no_tooltip);
                self.trace_pop();
            }
        }
    }

    /// Control.lua:140-149
    pub fn set_focus(&mut self, id: CtrlId, focus: bool) {
        if Some(focus) != self.base(id).has_focus {
            if focus { if self.has(id, Caps::FOCUS_GAINED) { self.on_focus_gained(id) } }
            else if self.has(id, Caps::FOCUS_LOST) { self.on_focus_lost(id) }
            self.base_mut(id).has_focus = Some(focus);
        }
    }
}
```

**Focus protocol.** The `on_key_*` functions return the next focused control:

- `Some(me)` keeps focus;
- `None` releases it;
- any other id moves focus there, including a child's child (a list returning its scrollbar).

Ids are global, so this works across hosts.

A super call that compares with the proxy, such as `newSel == self.EditControl and self or newSel` (`GemSelectControl.lua:818`), is the identity in Rust.

**Tab groups.** `Control.lua:151-178` maps to `ui.tab_groups[g]`:

- `add_to_tab_group(master)` creates `[master, self]` the first time;
- `tab_advance` wraps around and accepts a member that is `self`, or that has `KEY_DOWN` and is shown.

### 6.7 Input events (Lua `ipairs` and `#`)

```rust
// pob-ui-core/src/input.rs
#[derive(Clone, Copy, Debug, PartialEq)] pub enum EventKind { KeyDown { double_click: bool }, KeyUp, Char }
#[derive(Clone, Copy, Debug, PartialEq)] pub struct InputEvent { pub kind: EventKind, pub key: Key }

/// main.inputEvents: a Lua array that gets holes when events are consumed.
#[derive(Default)]
pub struct InputEvents { t: pob_lua::ljtab::LjIntMap<InputEvent> }   // the exact LuaJIT 2.1 table model (CCR-0034 A2)
impl InputEvents {
    pub fn push(&mut self, e: InputEvent);                 // t_insert(t, e) (appends at #t + 1)
    pub fn get(&self, i: usize) -> Option<InputEvent>;     // t[i], 1-based
    pub fn consume(&mut self, i: usize);                   // t[i] = nil
    /// `for i, e in ipairs(t)`: i += 1, then t[i]; stops at the first nil. Lazy, so consuming t[i]
    /// does not stop the current loop, but a LATER loop stops at that hole.
    pub fn next_ipairs(&self, i: &mut usize) -> Option<InputEvent> { *i += 1; self.get(*i) }
    /// `#t` with LuaJIT lj_tab_len border semantics (wipe keeps sizes and dead keys, as wipeTable does).
    /// `consume(i)` of a key that is not present inserts a dead key, as `t[k] = nil` does in LuaJIT.
    /// Tested against LuaJIT jit.off: 2000 random sequences + 300 PoB-style frames, 0 divergences (U4).
    pub fn lua_len(&self) -> usize;
    pub fn wipe(&mut self);                                // wipeTable
}
```

Canonical loop for ports:

```rust
let mut i = 0;
while let Some(e) = ev.next_ipairs(&mut i) {
    if let EventKind::KeyDown { .. } = e.kind {
        if e.key == Key::Char(b'z') && cx.sys.is_key_down(Key::Ctrl) { /* … */ ev.consume(i); }
    }
}
```

### 6.8 Subclassing, class hooks, dynamic `self` calls

How PoB's Lua call forms map to Rust:

| Lua | Meaning | Rust |
|---|---|---|
| `newClass("GemSelectControl", "EditControl")` | single or multiple inheritance | `struct GemSelectControl { pub edit: EditControl, .. }` with `impl_parts!(GemSelectControl: edit => EditControl)`. Mixins with state (`TooltipHost`, `UndoHandler`, `SearchHost`) become fields; `ControlHost` becomes `New::host()`. |
| `self:EditControl(anchor, rect, …)` inside the subclass constructor | base constructor on the same object | `EditControl::state(args)` for the fields, plus `.init(\|cx, me\| EditControl::init(cx, me.0, …))`. Children anchor to the outer node, because it is the same node. |
| `self.EditControl:Draw(vp)` / `self.ListControl.Draw(self, vp)` | super call | static call `EditControl::draw(cx, me, vp, nt)` |
| `self:OnKeyUp("DOWN")` and the method is a framework method a subclass or instance may override | virtual call | dynamic call `cx.on_key_up(me, Key::Down)` |
| `self:GetRowValue(…)` and other class hooks (List, DropDown, Edit, BuildList…) | virtual call | through the class hook table and override slot (below) |
| `self:Helper()` and no subclass or instance overrides `Helper` | non-virtual | static `Class::helper(cx, me, ..)` |
| `selControl.ListControl` | is-a | `cx.is::<ListControl>(id)` |
| `function ctrl:OnFocusLost() … end` (per instance) | instance override | `cx.ui.node_mut(id).ov.on_focus_lost = Some(Rc::new(…))` (helper `cx.set_ov(id, \|ov\| ov.on_focus_lost = …)`) |

Each base class with hooks exposes:

- a `&'static` table of `fn` pointers, built from the subclass's associated functions, with `None` meaning "method absent";
- an instance override struct with `Option<Rc<..>>` slots;
- a resolver that applies "instance override, else class hook".

U12 owns `ListControl`. The surface is frozen here:

```rust
// pob-ui-core/src/controls/list.rs (surface; U12 implements)
/// A list value with Lua identity semantics (`selValue == value`). `data` holds the value itself
/// (usually a model Rc such as SocketGroupRef/ItemRef, or a UI-built Rc<..>), which keeps it alive while it is compared.
#[derive(Clone)] pub struct ListValue { pub key: ValKey, pub data: std::rc::Rc<dyn std::any::Any> }
impl PartialEq for ListValue { fn eq(&self, o: &Self) -> bool { self.key == o.key } }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)] pub struct ValKey(pub usize);
impl ValKey { pub fn of_rc<T: ?Sized>(rc: &std::rc::Rc<T>) -> ValKey { ValKey(std::rc::Rc::as_ptr(rc) as *const () as usize) } }
impl ListValue { pub fn of<T: 'static>(rc: std::rc::Rc<T>) -> ListValue { ListValue { key: ValKey::of_rc(&rc), data: rc } } }
/// Adapter for model lists (`Shared<Vec<T>>`, calc doc §3) aliased by a list control: reorder/delete write through.
pub struct SharedVecSource<T: Clone + 'static>(pub pob_lua::Shared<Vec<std::rc::Rc<T>>>);

/// `self.list = build.skillsTab.socketGroupList` aliasing: the list lives in the model; reorder/delete write through.
pub trait ListSource {
    fn len(&self, cx: &Cx) -> usize;
    fn get(&self, cx: &Cx, i: usize /* 1-based */) -> Option<ListValue>;
    fn insert(&self, cx: &mut Cx, i: usize, v: ListValue);
    fn remove(&self, cx: &mut Cx, i: usize) -> ListValue;
}
pub enum ListData { Local(Vec<ListValue>), Source(std::rc::Rc<dyn ListSource>) }

pub struct ListHooks {     // ListControl.lua:6-24 + every `self.X and self:X(..)` call site in ListControl.lua
    pub get_row_value: fn(&Cx, CtrlId, usize /*column*/, usize /*index*/, &ListValue) -> String,
    pub get_row_icon: Option<fn(&Cx, CtrlId, usize, usize, &ListValue) -> Option<pob_sg::ImageHandle>>,
    pub add_value_tooltip: Option<fn(&mut Cx, CtrlId, &mut Tooltip, usize, &ListValue)>,
    pub get_drag_value: Option<fn(&Cx, CtrlId, usize, &ListValue) -> Option<(std::rc::Rc<str>, DragValue)>>,
    pub on_drag_send: Option<fn(&mut Cx, CtrlId, usize, &ListValue, CtrlId)>,
    pub on_order_change: Option<fn(&mut Cx, CtrlId, usize /*selIndex*/, usize /*selDragIndex*/)>, // CCR-0078 A1
    pub on_select: Option<fn(&mut Cx, CtrlId, usize, &ListValue)>,
    pub on_sel_click: Option<fn(&mut Cx, CtrlId, usize, &ListValue, bool) -> Option<bool>>, // Some(false) releases focus
    pub on_sel_copy: Option<fn(&mut Cx, CtrlId, usize, &ListValue)>,
    pub on_sel_cut: Option<fn(&mut Cx, CtrlId, usize, &ListValue)>,
    pub on_sel_delete: Option<fn(&mut Cx, CtrlId, usize, &ListValue)>,
    pub on_sel_key_down: Option<fn(&mut Cx, CtrlId, usize, &ListValue, Key)>,
    pub on_any_key_down: Option<fn(&mut Cx, CtrlId, Key)>,
    pub override_select_index: Option<fn(&mut Cx, CtrlId, usize) -> bool>,
    pub set_highlight_color: Option<fn(&mut Cx, CtrlId, usize, &ListValue) -> bool>,
    pub can_drag_to_value: Option<fn(&Cx, CtrlId, usize, &ListValue, Option<CtrlId>) -> bool>, // CCR-0078 A2
    pub re_sort: Option<fn(&mut Cx, CtrlId, usize)>,
    pub get_column_property: fn(&Cx, CtrlId, usize, &str) -> ColumnProp,  // default = ListControl's own
}
/// Same slots as Rc closures, for per-instance overrides (CompareTab.lua:1614-1627). Instance wins.
#[derive(Default, Clone)] pub struct ListOverrides { /* one Option<Rc<dyn Fn(..)>> per ListHooks field */ }

pub struct ListControl {
    pub hooks: &'static ListHooks, pub ov: ListOverrides,
    pub list: ListData, pub row_height: f64, pub scroll: bool, pub is_mutable: bool, pub force_tooltip: bool,
    pub sel_index: Option<usize>, pub sel_value: Option<ListValue>,
    pub sel_dragging: bool, pub sel_drag_active: bool, pub drag_target_list: Option<Vec<CtrlId>>,
    pub other_drag_source: Option<CtrlId>, pub drag_type: Option<std::rc::Rc<str>>, pub drag_value: Option<DragValue>,
    pub col_list: Vec<ListColumn>, pub col_labels: bool, pub label: Option<Prop<String>>, pub show_row_separators: bool,
    pub tooltip: Tooltip, /* … the remaining ListControl.lua fields, same names in snake_case … */
}
impl ListControl {
    /// ListControl.lua:44 ListControl(anchor, rect, rowHeight, scroll, isMutable, list, forceTooltip)
    pub fn state(hooks: &'static ListHooks, row_height: f64, scroll: bool, is_mutable: bool, list: ListData, force_tooltip: bool) -> ListControl;
    pub fn init(cx: &mut Cx, me: CtrlId);   // scrollbars (anchored to `me`)
    pub fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool);
    pub fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, dbl: bool) -> Focus;
    pub fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus;
    pub fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool;
    pub fn select_index(cx: &mut Cx, me: CtrlId, index: usize);
    pub fn get_hover_index(cx: &Cx, me: CtrlId) -> Option<usize>;
    pub fn get_hover_value(cx: &Cx, me: CtrlId) -> Option<ListValue>;
    pub fn list_len(cx: &Cx, me: CtrlId) -> usize;
    pub fn list_get(cx: &Cx, me: CtrlId, i: usize) -> Option<ListValue>;
}
/// `cx.is::<ListControl>(id)` (the `selControl.ListControl` is-a check) needs `W: Widget`, so ListControl implements
/// `Widget` by delegating to the functions above (CCR-0013 B5; U12 owns the file).
impl Widget for ListControl { /* … */ }
```

Hook set frozen by U12 (CCR-0078, which lists the grep): the 18 `ListHooks` fields are every
method ListControl.lua calls on `self` (`OnSelCut` included, though the class header omits it).
`CanReceiveDrag`/`ReceiveDrag` are `Widget` methods of the drag target (any control; presence =
`Caps::CAN_RECEIVE_DRAG`). `otherDragSource`/`otherDragTargeting` belong on `Base` (CCR-0078 B);
until then read them with `controls::list::other_drag_source(cx, id)`.

`DropDownControl` (U13) follows the same pattern, with an empty class-hook table (CCR-0086):

- `DropItem { label: String, search_filter: Option<String>, detail: Option<String>, value: Rc<dyn Any> }`; a Lua string entry is a `DropItem` whose `value` is the `String` (`DropItem::str`).
- No `DropDownHooks`: no subclass or instance overrides a method DropDownControl.lua calls on `self`; ItemSlot and TimelessJewelSocket override framework methods (their own `Widget` impl, super calls to `DropDownControl::{draw, on_key_down, get_hover_index, ..}`), Build.lua's `mainSkillMinion` uses `Overrides`.
- `sel_func: Option<Rc<dyn Fn(&mut Cx, usize, Option<&DropItem>)>>`.
- The tooltip argument is `(DropTipMode /* BODY|OUT|HOVER */, Option<usize>, Option<DropItem>)`.

`EditControl` (U11) exposes `new(anchor, rect, EditArgs) -> New<EditControl>`, `state(EditArgs)`, `init`, and the methods `set_text`, `insert`, `replace_sel`, `get_sel_text`, `select_all`, `undo`, `redo`, `set_placeholder`, `set_protected`, plus `pub buf: String`, `pub caret: usize`, `pub sel: Option<usize>` (1-based byte positions, as in Lua) and `change_func: Option<Rc<dyn Fn(&mut Cx, &str)>>`.

```rust
/// EditControl(anchor, rect, init, prompt, filter, limit, changeFunc, lineHeight, allowZoom, clearable) (EditControl.lua:43)
#[derive(Default)]
pub struct EditArgs {
    pub init: Option<String>, pub prompt: Option<String>,
    pub filter: Option<String>,                  // nil => main.unicode and "%c" or "^%w%p "
    pub limit: Option<usize>,
    pub change_func: Option<std::rc::Rc<dyn Fn(&mut Cx, &str)>>,
    /// The `placeholder == true` call of a two-argument changeFunc (`SetPlaceholder(text, true)`, EditControl.lua:114;
    /// ConfigTab.lua:305/323/334). When unset, that call goes to `change_func` (CCR-0081 §1).
    pub placeholder_change_func: Option<std::rc::Rc<dyn Fn(&mut Cx, &str)>>,
    pub line_height: Option<f64>, pub allow_zoom: bool, pub clearable: bool,
}
```

Rule: each base-class unit must grep every subclass and every instance override of its class **before** its wave-2 gate, and list the hook set in its file header. After the gate the hook table is frozen; extensions need an amendment to this document.

### 6.9 TooltipHost and Tooltip

```rust
// pob-ui-core/src/tooltip/host.rs: TooltipHost.lua. `A` = extra tooltipFunc args: () Button/Edit/Dragger,
// bool CheckBox (state), f64 Slider (val), DropTipArgs DropDown.
pub type TooltipFn<A> = std::rc::Rc<dyn Fn(&mut Cx, &mut Tooltip, A)>;
pub struct TooltipHost<A: 'static = ()> {
    pub tooltip: Tooltip,
    pub tooltip_text: Option<Prop<Option<String>>>,   // read through self.Object:GetProperty
    pub tooltip_func: Option<TooltipFn<A>>,
}
impl<A: 'static> TooltipHost<A> {
    pub fn new(text: Option<Prop<Option<String>>>) -> Self { TooltipHost { tooltip: Tooltip::default(), tooltip_text: text, tooltip_func: None } }
    /// TooltipHost.lua:15-27. The caller has already done SetDrawLayer(nil, 100).
    pub fn draw_tooltip<W: Widget>(cx: &mut Cx, me: CtrlId, get: fn(&mut W) -> &mut TooltipHost<A>,
                                   x: f64, y: f64, w: f64, h: f64, vp: &ViewPort, args: A) {
        let (func, text) = { let th = get(cx.w_mut::<W>(me)); (th.tooltip_func.clone(), th.tooltip_text.clone()) };
        let mut tt = std::mem::take(&mut get(cx.w_mut::<W>(me)).tooltip);          // lend (6.13)
        if let Some(f) = func {
            f(cx, &mut tt, args);
            tt.draw(cx, x, y, Some(w), Some(h), vp);
        } else if let Some(t) = text.and_then(|p| p.get(cx, me)) {                 // self.Object:GetProperty("tooltipText")
            tt.clear(false);
            tt.add_line(cx, 14.0, &t);
            tt.draw(cx, x, y, Some(w), Some(h), vp);
        }
        get(cx.w_mut::<W>(me)).tooltip = tt;                                         // `me` is still alive: freeing is deferred (D8)
    }
}

// pob-ui-core/src/tooltip/tooltip.rs: Tooltip.lua
#[derive(Clone, Debug, PartialEq)]
pub enum TtKey { Nil, Bool(bool), Num(f64), Str(std::rc::Rc<str>), Obj(TtObj) } // CheckForUpdate args
/// A table argument: compared by Rc identity and kept alive (Lua's updateParams holds the reference too).
#[derive(Clone)] pub struct TtObj(pub std::rc::Rc<dyn std::any::Any>);
impl PartialEq for TtObj { fn eq(&self, o: &Self) -> bool { std::rc::Rc::ptr_eq(&self.0, &o.0) } }
impl std::fmt::Debug for TtObj { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "TtObj({:p})", std::rc::Rc::as_ptr(&self.0)) } }
#[derive(Clone)] pub enum TtColor { Rgb([f64; 3]), Esc(String) }               // self.color = {r,g,b} | "^x.."
#[derive(Clone)] pub enum TtBackground { Image(String), Color(String) }          // AddLine background arg; both are image paths (Tooltip.lua:615-625; CCR-0073 C)
/// modLine: an `Rc<TtModLine>` coerced to `Rc<dyn Any>`; `disabled` reads the model field live, `line` is the
/// model object (U38 downcasts it). Any other value reads as not disabled (CCR-0073 B).
pub type ModLineRef = std::rc::Rc<dyn std::any::Any>;
pub struct TtModLine { pub disabled: std::rc::Rc<dyn Fn() -> bool>, pub line: std::rc::Rc<dyn std::any::Any> }
pub type TooltipDrawFn = std::rc::Rc<dyn Fn(&mut Cx, &mut Tooltip, f64, f64, Option<f64>, Option<f64>, &ViewPort)>;

#[derive(Default)]
pub struct Tooltip {
    pub lines: Vec<TooltipLine>, pub blocks: Vec<TooltipBlock>,
    pub update_params: Option<Vec<TtKey>>, pub update_nsmt: Option<bool>,    // updateParams.notSupportedModTooltips
    pub tooltip_header: Option<String>, pub title_y_offset: f64, pub recipe: Option<Vec<String>>,
    pub center: bool, pub max_width: Option<f64>, pub color: Option<TtColor>,
    pub draw_override: Option<TooltipDrawFn>,   // tooltip.Draw = function(self, ...) (TreeTab.lua:1749 etc.)
    /* … remaining Tooltip.lua fields (childTooltips, _bgHandles …) */
}
impl Tooltip {
    pub fn clear(&mut self, clear_update_params: bool);                                   // :54
    pub fn check_for_update(&mut self, cx: &Cx, params: &[TtKey]) -> bool;                // :71 (also main.notSupportedModTooltips)
    pub fn add_line(&mut self, cx: &Cx, size: f64, text: &str);                           // :91
    pub fn add_line_ex(&mut self, cx: &Cx, size: f64, text: &str, font: Option<Font>,
                       mod_line: Option<ModLineRef>, background: Option<TtBackground>);
    pub fn set_recipe(&mut self, recipe: Option<Vec<String>>);                           // :116
    pub fn add_separator(&mut self, cx: &Cx, size: Option<f64>);                          // :120; the Load is queued in a shared FIFO that
    //   every real_draw flushes (`tooltip::flush_pending_loads`) until it takes `&mut Cx` (CCR-0073 A, A8 needs-code)
    pub fn get_size(&self, cx: &Cx) -> (f64, f64);                                        // :163
    pub fn get_dynamic_size(&self, cx: &Cx, vp: &ViewPort) -> (f64, f64);                 // :194
    /// tooltip:Draw: calls draw_override if set, else real_draw.
    pub fn draw(&mut self, cx: &mut Cx, x: f64, y: f64, w: Option<f64>, h: Option<f64>, vp: &ViewPort);
    pub fn real_draw(&mut self, cx: &mut Cx, x: f64, y: f64, w: Option<f64>, h: Option<f64>, vp: &ViewPort); // :379
}
```

Notes on Tooltip:

- `CheckForUpdate` compares with Lua `~=`. Tables compare by identity: pass `TtKey::Obj(TtObj(rc))` for model objects (it holds the `Rc`, so the address cannot be reused while compared), and never raw pointer numbers.
- `Tooltip:Draw` relies on `GetDrawColor` save/restore (`Tooltip.lua:613`), which depends on the escape-leak semantics of §2.
- A widget-owned tooltip is always drawn through `cx.lend`.

### 6.10 Popups (`PopupDialog.lua`, `Main.lua:1660-1786`)

```rust
// pob-ui-core/src/controls/popup_dialog.rs + popups.rs
pub struct PopupOpts {
    pub enter: Option<CtrlKey>, pub default: Option<CtrlKey>, pub escape: Option<CtrlKey>,
    pub scroll_bar_func: Option<std::rc::Rc<dyn Fn(&mut Cx)>>, pub resize_func: Option<std::rc::Rc<dyn Fn(&mut Cx)>>,
}
pub struct PopupDialog { pub title: Prop<String>, pub enter_control: Option<CtrlKey>, pub escape_control: Option<CtrlKey>, /* … */ }
impl Widget for PopupDialog { const NAME: &'static str = "PopupDialog"; const CAPS: Caps = Caps::DRAW; /* … */ }

/// main:OpenPopup. Creates the dialog node first (centred; x/y props as in PopupDialog.lua). Then `build` adds the
/// children into the popup's host (they are owned by the popup). Then the PopupDialog.lua:25-33 anchor rules apply:
/// no point => TOP/TOP of the popup; `other` nil => the popup; `other_key` => the sibling with that key.
/// Then `default` is selected. Finally the popup is inserted at main.popups[0] (Lua popups[1]).
pub fn open_popup(cx: &mut Cx, width: f64, height: f64, title: impl Into<Prop<String>>,
                  build: impl FnOnce(&mut Cx, Ctrl<PopupDialog>), opts: PopupOpts) -> Ctrl<PopupDialog>;
/// main:ClosePopup: removes popups[0] and buries it (freed at end of frame, D8).
pub fn close_popup(cx: &mut Cx);
pub fn open_message_popup(cx: &mut Cx, title: &str, msg: &str) -> Ctrl<PopupDialog>;
pub fn open_confirm_popup(cx: &mut Cx, title: &str, msg: &str, confirm_label: &str,
                          on_confirm: impl Fn(&mut Cx) + 'static,
                          extra: Option<(&str, std::rc::Rc<dyn Fn(&mut Cx)>)>) -> Ctrl<PopupDialog>;
pub fn open_new_folder_popup(cx: &mut Cx, path: &str, on_close: Option<std::rc::Rc<dyn Fn(&mut Cx, Option<String>)>>);
/// The `launch` fields and `APP_NAME` that main:OpenCloudErrorPopup reads (Main.lua:1764-1768; CCR-0084 §1).
#[derive(Clone, Debug)]
pub struct LaunchInfo { pub app_name: String, pub version_number: String, pub version_branch: Option<String>, pub dev_mode: bool }
pub fn open_cloud_error_popup(cx: &mut Cx, file_name: &str, launch: &LaunchInfo) -> Ctrl<PopupDialog>;
/// Main.lua:437-443 (the popup layer and dim inside main:OnFrame); U16's OnFrame calls it (CCR-0084 §2).
pub fn draw_popup_layer(cx: &mut Cx, vp: &ViewPort);
/// PopupDialog:ProcessInput (ESCAPE => escape:Click() or ClosePopup; RETURN => enter:Click()).
pub fn popup_process_input(cx: &mut Cx, popup: CtrlId, ev: &mut InputEvents, vp: &ViewPort);
```

Lua builds popup `controls` tables **before** the dialog exists, and their closures reach siblings through the local `controls` table when they are called. In Rust, closures capture the popup id `p`. A sibling that already exists is captured as a `Ctrl`; a sibling created later (a forward reference) is looked up with `cx.child_as::<W>(p, "key")` at call time.

### 6.11 `MainCore`, colours, draw helpers

```rust
// pob-ui-core/src/main_core.rs: the part of the global `main` that generic controls read or write
pub struct MainCore {
    pub node: CtrlId,                              // the `main` ControlHost node (bottom bar controls)
    pub screen_w: f64, pub screen_h: f64,          // main.screenW/H (set at the top of main:OnFrame)
    pub view_port: ViewPort,
    pub popups: Vec<Ctrl<PopupDialog>>,            // [0] == Lua popups[1] (topmost)
    // main.inputEvents is owned by pob-app's MainState and passed down as `&mut InputEvents`
    // (only Main.lua touches it globally; verified by grep).
    pub show_drag_text: Option<String>,
    pub cc: ColorCodes,                            // Data/Global.lua colorCodes (mutable; updateColorCode)
    pub show_flavour_text: bool, pub not_supported_mod_tooltips: bool, pub unicode: bool,
    pub disable_scroll_control_interaction: bool,
    // main.invertSliderScrollDirection (SliderControl.lua:168/176) has no field yet: U9's thread-local
    // `controls::slider::INVERT_SLIDER_SCROLL_DIRECTION` stands in (CCR-0069 A, A8 needs-code).
    pub thousands_separator: String, pub decimal_separator: String, pub show_thousands_separators: bool,
    pub on_frame_funcs: Vec<(String, std::rc::Rc<dyn Fn(&mut Cx)>)>, // main.onFrameFuncs (insertion order; ORDER noted)
}
// pob-ui-core/src/color_codes.rs: Data/Global.lua:7-100 (colorCodes, defaultColorCodes, hexToRGB, updateColorCode)
pub struct ColorCodes { /* NORMAL, MAGIC … as pub String fields (exact Lua names in SCREAMING_CASE) */ }
pub const PALETTE: [[f32; 4]; 10]; // ^0..^9 from SimpleGraphic.dll @0x185380 (re-exported from pob_sg::color)
// pob-ui-core/src/draw.rs: Main.lua:1497-1573
pub fn draw_arrow(cx: &mut Cx, x: f64, y: f64, w: f64, h: f64, dir: ArrowDir);       // :1497
pub fn draw_check_mark(cx: &mut Cx, x: f64, y: f64, size: f64);                      // :1515
pub fn render_circle(cx: &mut Cx, x: f64, y: f64, w: f64, h: f64, ox: f64, oy: f64, radius: f64);            // :1539
pub fn render_ring(cx: &mut Cx, x: f64, y: f64, w: f64, h: f64, ox: f64, oy: f64, radius: f64, size: f64); // :1562
// pob-ui-core/src/wrap.rs: Main.lua:1797-1822
pub fn wrap_string(cx: &Cx, s: &str, height: f64, width: f64) -> Vec<String>;
```

`main:DrawBackground` (`Main.lua:1486`) needs the tree `Background` asset, so it lives in `pob-app`.

### 6.12 Undo (`UndoHandler.lua`)

```rust
// pob-ui-core/src/undo.rs
pub struct UndoStack<S> { pub undo: std::collections::VecDeque<S>, pub redo: std::collections::VecDeque<S>, pub mod_flag: bool }
impl<S> Default for UndoStack<S> { fn default() -> Self { UndoStack { undo: Default::default(), redo: Default::default(), mod_flag: false } } }
/// One UndoHandler "class": where the stack lives and the two required methods.
pub struct UndoOps<S: 'static> {
    pub stack: for<'a> fn(&'a mut Cx, CtrlId) -> &'a mut UndoStack<S>,
    pub create: fn(&mut Cx, CtrlId) -> S,          // CreateUndoState (may mutate, e.g. EditControl.lastUndoState)
    pub restore: fn(&mut Cx, CtrlId, S),           // RestoreUndoState
}
impl<S: 'static> UndoOps<S> {
    /// :25-29
    pub fn reset_undo(&self, cx: &mut Cx, me: CtrlId) {            // the wipes come first (CCR-0034 A1)
        { let st = (self.stack)(cx, me); st.undo.clear(); st.redo.clear(); }
        let s = (self.create)(cx, me);
        (self.stack)(cx, me).undo.push_front(s);
    }
    /// :34-41
    pub fn add_undo_state(&self, cx: &mut Cx, me: CtrlId, no_clear_redo: bool) {
        let s = (self.create)(cx, me);
        let st = (self.stack)(cx, me);
        st.undo.push_front(s); st.undo.truncate(101); st.mod_flag = true;
        if !no_clear_redo { st.redo.clear(); }
    }
    /// :44-50
    pub fn undo(&self, cx: &mut Cx, me: CtrlId) {
        let prev = { let st = (self.stack)(cx, me);
            if st.undo.len() < 2 { return; }
            let top = st.undo.pop_front().unwrap(); st.redo.push_front(top); st.undo.pop_front().unwrap() };
        (self.restore)(cx, me, prev);
        self.add_undo_state(cx, me, true);
    }
    /// :53-58
    pub fn redo(&self, cx: &mut Cx, me: CtrlId) {
        let Some(s) = (self.stack)(cx, me).redo.pop_front() else { return };
        (self.restore)(cx, me, s);
        self.add_undo_state(cx, me, true);
    }
}
```

`mod_flag` is the owning object's `modFlag` (tab, spec or edit). The build-level unsaved logic (`Build.lua:1100-1112` ResetModFlags) reads it from there.

### 6.13 Coroutines and lending

```rust
// pob-ui-core/src/coroutine.rs: coroutine.create/resume/status/yield
pub struct Coroutine { fut: Option<std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>> }
impl Default for Coroutine { fn default() -> Self { Coroutine { fut: None } } }
impl Coroutine {
    pub fn create(f: impl std::future::Future<Output = ()> + 'static) -> Self;
    /// coroutine.resume: one poll with Waker::noop(), with `cx` installed in the scoped TLS for the poll.
    /// Lua's `res`: true when the body ran (to its next yield or to its end; the call that finishes the body
    /// returns true and `is_dead()` is then true), false when it was already dead. A panic inside leaves the
    /// coroutine dead and keeps unwinding (CCR-0034 A3).
    pub fn resume(&mut self, cx: &mut Cx) -> bool;
    /// `local res, errMsg = coroutine.resume(co)`: a Lua error inside the body is returned, not raised (not on wasm32).
    pub fn try_resume(&mut self, cx: &mut Cx) -> (bool, Option<String>);
    pub fn is_dead(&self) -> bool;
}
pub mod co {
    /// Access Cx inside a coroutine body, between yields. Nested use panics (re-entrancy guard).
    pub fn with_cx<R>(f: impl FnOnce(&mut super::Cx) -> R) -> R;
    /// `coroutine.running()` as a condition: true while any `resume` is on the stack, also inside `with_cx` (CCR-0034 B).
    pub fn running() -> bool;
    /// `coroutine._list()` (Common.lua:46-57): coroutines created and not dead, for frame pacing (CCR-0034 B).
    pub fn any_alive() -> bool;
    /// coroutine.yield(): Pending exactly once.
    pub async fn yield_now();
}
```

Coroutine rules:

- A coroutine stored in widget state is resumed through `cx.lend(me, |w| &mut w.power_builder, |cx, co| co.resume(cx))`. While it runs, the slot reads as a dead coroutine.
- Lua code that checks `self.powerBuilder` from inside the coroutine must be handled explicitly, with a comment.
- `main:Init`'s item-DB coroutine (`Main.lua:293-300`) follows the same pattern.
- `App::wants_frames()` returns true while any coroutine is alive, which mirrors the SG frame-pacing rule.

### 6.14 Lua errors

```rust
// pob-ui-core/src/lua_error.rs
/// A Lua runtime error (nil index, nil call, assert, error()). Panics with a "LuaError: " prefix.
/// The host catches it per callback and shows PoB's error prompt (Launch.lua ShowErrMsg).
pub use pob_lua::lua_error;   // the calc plan's macro (LuaError payload); one error type across UI and calc
```

---
## 7. Closures, borrows, and the porting cheat sheet

### 7.1 Where each kind of Lua closure goes

| Lua | Rust home | Semantics |
|---|---|---|
| `x`, `y`, `width`, `height`, `shown`, `enabled`, `label`, `locked`, `tooltipText` (and any `GetProperty` field) as a function | `Prop::f(move \|cx, me\| …)` in `Base` or the widget field | Evaluated lazily on every read, in the Lua order. Reads anything. Writes only through `Cell`/`RefCell` fields (§7.3). |
| `onClick`, `changeFunc`, `selFunc`, `onHover`, `enterFunc`, list/dropdown callbacks | `Rc<dyn Fn(&mut Cx, ..)>` stored in the widget | Called synchronously at the Lua call site. **Clone the `Rc` out of the widget before calling.** |
| `tooltipFunc(tooltip, ...)` | `TooltipFn<A> = Rc<dyn Fn(&mut Cx, &mut Tooltip, A)>` | The tooltip is lent (§6.13). |
| `ctrl.Draw = function(control) … end`, `function ctrl:OnFocusLost() … end` | `cx.set_ov(id, …)` (framework) or the class override slot (hooks, `Tooltip.draw_override`) | Instance wins over class. |
| Closure over `self` in a constructor | capture `me: CtrlId` (from `New::init`) | Ids are `Copy`; never capture references. |
| Closure over a local control (`local foo = new(..)`) | capture its `Ctrl<W>` | |
| Closure reading `self.controls.k` (looked up at call time, possibly a forward reference) | `cx.child_as::<W>(host, "k")` inside the closure | Matches Lua's late binding. |
| Shared mutable upvalue (`local count = 0` mutated by several closures) | `Rc<Cell<T>>` / `Rc<RefCell<T>>` captured by each closure | These hold no widget or model state. |
| Coroutine body | `async move { … co::with_cx(\|cx\| …); co::yield_now().await; … }` | §6.13 |

### 7.2 Borrow rules (compiler-enforced where possible)

1. A reference obtained from `cx` (`cx.w`, `cx.get`, `cx.base`, `cx.build()`) cannot outlive a call that takes `cx`. Copy or clone the values out first. The compiler enforces this.
2. **Callbacks and sources are cloned out before the call**, for example `let f = cx.w::<Self>(me).on_click.clone(); f(cx)`.
3. **Disjoint field borrows are allowed and encouraged** in draw code, for example `let w = cx.ui.w::<Self>(me); cx.sg.draw_string(.., &w.label_cache)`. Evaluate `Prop`s (which need the whole `&Cx`) before taking `&mut cx.sg`. To keep the Lua evaluation order, re-take `cx.sg` per statement.
4. **Lend, don't alias.** An object owned by a widget whose methods need `&mut Cx` (Tooltip, PassiveTreeView, Coroutine, sub-objects with draw methods) goes through `cx.lend(..)`. During the call the slot holds `Default`. Never lend an object that the call itself, or something it calls, reads through `cx`. If such a read exists, the object must become its own arena node.
5. `RefCell` is allowed only for (a) values written by property closures (§7.3), (b) shared upvalues (§7.1), and (c) `MiscCalc`/`CalcFacade` internals. Never hold a `RefCell` borrow across a call that takes `cx`.
6. After a callback that can close a popup, close the build, or replace a control, the current function must not touch `me` again unless Lua does. Freeing is deferred (D8), so Lua code that does touch it still works.

### 7.3 Properties with side effects

A Lua property closure that assigns fields must have those fields declared as `Cell<T>`/`RefCell<T>`, with a comment citing the Lua line. Known case: `Build.lua:148-157` (`strWidth`, `strLimited` on `BuildMode`). Each unit greps its range for assignments inside `function(...)` bodies that are stored as props, and lists any it finds in its file header.

### 7.4 Cheat sheet

| Lua | Rust |
|---|---|
| `self.controls.x = new("ButtonControl"):ButtonControl(a, r, l, f)` | `let x = cx.add(me, "x", ButtonControl::new(a, r, l, f));` |
| `new("X"):X(...) { shown = fn, width = fn }` | `X::new(...).shown(Prop::f(..)).width(Prop::f(..))` (mixins run after init) |
| `t_insert(self.controls, c)` | `cx.add(me, CtrlKey::Idx(n + 1), ..)` where `n` is the Lua `#controls` |
| `self.controls.x = nil` | `cx.remove_child(me, &"x".into())` (or `detach_child` if still referenced) |
| `ctrl.shown = function() … end` | `cx.base_mut(ctrl).shown = Prop::f(move \|cx, _\| …)` |
| `ctrl:GetProperty("label")` / raw `ctrl.width` | `cx.prop::<ButtonControl, _>(id, \|w\| &w.label)` / `cx.base(id).width.raw()` |
| `ctrl:SetAnchor("LEFT", other, "RIGHT", 8, 0)` | `cx.set_anchor(ctrl, Left, Some(other.into()), Right, Some((8.into(), 0.into())), false)` |
| `self:SelectControl(c)` / `return self` / `return nil` | `cx.select_control(me, Some(c))` / `Some(me)` / `None` |
| `for id, ev in ipairs(inputEvents)` + `inputEvents[id] = nil` | the `next_ipairs` loop (§6.7) + `ev.consume(i)` |
| `SetDrawColor(1, 1, 1)`, `DrawString(..)` | `cx.sg.set_draw_color(1.0, 1.0, 1.0)`, `cx.sg.draw_string(..)` |
| `GetCursorPos()`, `IsKeyDown("CTRL")`, `GetTime()` | `cx.sys.cursor_pos()`, `cx.sys.is_key_down(Key::Ctrl)`, `cx.sys.time_ms()` |
| `main:OpenPopup(w, h, t, controls, "ok", nil, "cancel")` | `open_popup(cx, w, h, t, \|cx, p\| { … }, PopupOpts { enter: Some("ok".into()), escape: Some("cancel".into()), ..Default::default() })` |
| `main.popups[1]` | `cx.main.popups.first()` |
| `colorCodes.POSITIVE` | `cx.main.cc.POSITIVE` (read at the time Lua reads it) |
| `self.build.buildFlag = true` | `cx.build_mut().build_flag = true` |
| `self.build.spec:AllocNode(n)` | `cx.build().model.spec.get().AllocNode(&n, None)` (calc doc names: PoB identifiers verbatim, `Field`/`Shared` mutation) |
| `self.build.itemsTab:PopulateSlots()` (another tab's UI method) | `let t = cx.build().tabs.items; ItemsTab::populate_slots(cx, t.0)` |
| `calcFunc(override, useFullDPS)` | `let f = cx.build().calc_func.clone().unwrap(); match f.call(Some(Rc::new(ov)), use_full_dps) { Poll::Ready(out) => …, Poll::Pending => /* site placeholder; never on desktop */ }` |
| `tooltip:CheckForUpdate(node, build.outputRevision)` | `tt.check_for_update(cx, &[TtKey::Obj(TtObj(node.clone())), TtKey::Num(model.outputRevision.get())])` |
| `s:gsub(p, r)`, `s:match(p)`, `s:find(p, i, plain)`, `string.format(f, ..)` | In expression position (rule 5, first value only): `pob_ui_core::luastr::{gsub, lmatch}` (`gsub(s, p, r) -> String`, `lmatch(s, p) -> Option<String>`; CCR-0013 B1). When every return value is used: `pob_lua::{gsub, lmatch}` (`(String, usize)`, `Option<Captures>`). `pob_lua::find`, `pob_lua::lformat!(f, ..)` |
| `tostring(n)`, `x .. ""` (number) | `pob_lua::tostring(n)` |
| `table.sort(t, f)` | `pob_lua::sort(&mut v, \|a, b\| lt)` (Lua 5.1 algorithm, not stable) |
| `coroutine.create(f)` / `resume` / `yield` | `Coroutine::create(async move {..})` / `co.resume(cx)` (via lend) / `co::yield_now().await` |
| `error(msg)`, `assert(x, msg)`, nil index | `lua_error!(..)`, `x.unwrap_or_else(\|\| lua_error!(..))` |

---

## 8. Lua-semantics rules (binding)

1. **Numbers** are `f64` everywhere in UI code. `math.floor`/`ceil` map to `f64::floor`/`ceil`. Use `pob_lua::math::{m_min, m_max}` for `math.min`/`math.max`, because `f64::min` differs from Lua on NaN. Lua `%` is floored modulo: use `pob_lua::fmod_lua`, never Rust `%`. PoB's `round(v, dec)` (`Common.lua:709`) is `pob_lua::round`.
2. **Truthiness.** `nil` and `false` are falsy; `0` and `""` are truthy. `a and b or c` must keep its exact result even when `b` is falsy. A Lua `nil` becomes `Option::None`, and a nil/false distinction becomes `Option<bool>`. `x or 0` becomes `x.unwrap_or(0.0)`.
3. **Indices.** Fields that mirror Lua indices (`sel_index`, `caret`, `scroll` rows, list positions) stay **1-based** and keep Lua's arithmetic. Convert only at Rust slice access (`v[i - 1]`).
4. **Strings** are byte strings. All `string.*` operations go through `pob_lua` helpers with Lua byte semantics (1-based, negative indices count from the end). Text buffers are `String`. When Lua byte slicing could split a multibyte character, the helper works on bytes and re-validates lossily; this is a documented divergence, which PoB avoids because its input is ASCII after filtering. Numbers turned into text go through `pob_lua::tostring` (`%.14g`), and `string.format` through `lformat!`.
5. **Multiple returns.** A call inside an expression keeps only its first value: `s:gsub(..)` inside `..` or a method chain is the string. Use the explicit tuple only where Lua uses both values.
6. **Iteration order.** `ipairs` stops at the first hole. Each `pairs` loop in a unit's range whose order is observable (draw order, hit testing, the order of built lists that are not sorted afterwards, the order of side effects) must be annotated `// ORDER:` with one of three resolutions:
   - (a) canonical order plus the matching oracle patch, registered in `patches.rs`;
   - (b) a multiset bucket in `DiffConfig`;
   - (c) a written argument that order does not matter.

   Controls tables always use (a) (D7). `main.onFrameFuncs` uses insertion order, plus a patch if U16 finds an observable effect. Integer-keyed model maps are `LjIntMap` (calc doc D8), which matches LuaJIT's order for a given insertion order. Where the insertion order comes from a `pairs` over a table that ever held a string key, the order is per-process random and the sites use (b), a multiset bucket: `spec.nodes` is filled from `tree.nodes`, which holds the string key `root` until PassiveTree.lua:510, so PassiveTreeView.lua:828, 881, 958, 963, 965 (inside `pairs(spec.nodes)`) need (b), and every other loop over `spec.nodes`, `tree.nodes` or `allocNodes` must be audited (CCR-0028 §9; calc doc §3.4). Native string-key order is randomised per LuaJIT process (calc doc D8), so for string-keyed tables only (a) or (b) are possible.
7. **`table.sort`** uses `pob_lua::sort`, an exact port of Lua 5.1 `auxsort`. It is not stable, so ties must order the same way as in PoB.
8. **Errors** become `lua_error!` panics (D17). Do not "fix" Lua bugs. If PoB would error, the port panics with the same message. Messages are compared without the `<chunk>:<line>: ` prefix that LuaJIT adds for errors raised from Lua code (a C function's error carries its Lua caller's position; CCR-0033 §5).
9. **Globals**: `main` maps to `cx.main` (MainCore) plus `CxApp::app().main` (MainState); `launch` to `app().launch`; `build` to `cx.build()` plus the `BuildMode` node; `data` to `cx.data()`; `colorCodes` to `cx.main.cc`.
10. **Time.** `GetTime()` is an integer number of ms from `cx.sys.time_ms()`. Never read a wall clock directly; the oracle clock is virtual.
11. **Pattern filters** (EditControl `"[" .. filter .. "]"`, SearchHost, CalcFormat, WrapString) use `pob_lua::pattern`, which is the existing `lua_pattern.rs` and already parity-tested.

---

## 9. Reference ports

These show the binding style. U9 and U22 own the real files.

### 9.1 `ButtonControl` (complete)

```rust
//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ButtonControl.lua
use std::rc::Rc;
use pob_sg::{Align, Font, ImageFlags, ImageHandle, Key};
use crate::{prelude::*, tooltip::TooltipHost};

pub type ClickFn = Rc<dyn Fn(&mut Cx) -> Focus>;

pub struct ButtonControl {
    pub tt: TooltipHost<()>,
    pub label: Prop<String>,
    pub on_click: ClickFn,
    pub on_hover: Option<Rc<dyn Fn(&mut Cx)>>,
    pub force_tooltip: bool,
    pub locked: Prop<bool>,                 // GetProperty("locked"); nil => false
    pub enter_func: Option<Rc<dyn Fn(&mut Cx)>>,
    pub image: Option<ImageHandle>,
    pub over_size_text: f64,                // `self.overSizeText or 0`
    pub clicked: bool,
}

impl ButtonControl {
    /// lua: ButtonControl.lua:9-17 ButtonControl(anchor, rect, label, onClick, onHover, forceTooltip)
    pub fn new(anchor: Anchor, rect: [Prop<f64>; 4], label: impl Into<Prop<String>>,
               on_click: impl Fn(&mut Cx) + 'static) -> New<ButtonControl> {
        Self::new_ret(anchor, rect, label, move |cx| { on_click(cx); None })
    }
    /// For onClick functions whose return value Lua feeds into SelectControl (`return self.onClick()` in OnKeyUp).
    pub fn new_ret(anchor: Anchor, rect: [Prop<f64>; 4], label: impl Into<Prop<String>>,
                   on_click: impl Fn(&mut Cx) -> Focus + 'static) -> New<ButtonControl> {
        New::new(Base::new(anchor, rect), ButtonControl {
            tt: TooltipHost::new(None), label: label.into(), on_click: Rc::new(on_click), on_hover: None,
            force_tooltip: false, locked: Prop::Val(false), enter_func: None, image: None,
            over_size_text: 0.0, clicked: false,
        })
    }
    /// lua: ButtonControl.lua:25-32
    pub fn set_image(cx: &mut Cx, me: CtrlId, path: Option<&str>) {
        let img = path.map(|p| { let h = cx.sg.new_image_handle(); h.load(&mut cx.sg, p, ImageFlags::empty()); h });
        cx.w_mut::<Self>(me).image = img;
    }
}
impl New<ButtonControl> {
    /// 5th and 6th constructor arguments.
    pub fn on_hover(mut self, f: impl Fn(&mut Cx) + 'static) -> Self { self.w.on_hover = Some(Rc::new(f)); self }
    pub fn force_tooltip(mut self, v: bool) -> Self { self.w.force_tooltip = v; self }
}

impl Widget for ButtonControl {
    const NAME: &'static str = "ButtonControl";
    const CAPS: Caps = Caps::DRAW.union(Caps::IS_MOUSE_OVER).union(Caps::KEY_DOWN).union(Caps::KEY_UP).union(Caps::CLICK);

    /// lua: ButtonControl.lua:19-23
    fn click(cx: &mut Cx, me: CtrlId) {
        if cx.is_shown(me) && cx.is_enabled(me) { let f = cx.w::<Self>(me).on_click.clone(); f(cx); }
    }

    /// lua: ButtonControl.lua:34-39
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        if !cx.is_shown(me) { return false; }
        cx.is_mouse_in_bounds(me)
    }

    /// lua: ButtonControl.lua:41-105
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        let (x, y) = cx.pos(me);
        let (width, height) = cx.size(me);
        let enabled = cx.is_enabled(me);
        let m_over = cx.is_mouse_over(me);
        let locked = cx.prop::<Self, _>(me, |w| &w.locked);
        if !enabled { cx.sg.set_draw_color(0.33, 0.33, 0.33) }
        else if m_over || locked { cx.sg.set_draw_color(1.0, 1.0, 1.0) }
        else { cx.sg.set_draw_color(0.5, 0.5, 0.5) }
        cx.sg.draw_image(None, x, y, width, height);
        let (clicked, image) = { let w = cx.w::<Self>(me); (w.clicked, w.image.clone()) };
        if !enabled { cx.sg.set_draw_color(0.0, 0.0, 0.0) }
        else if clicked && m_over { cx.sg.set_draw_color(0.5, 0.5, 0.5) }
        else if m_over || locked { cx.sg.set_draw_color(0.33, 0.33, 0.33) }
        else { cx.sg.set_draw_color(0.0, 0.0, 0.0) }
        cx.sg.draw_image(None, x + 1.0, y + 1.0, width - 2.0, height - 2.0);
        if let Some(image) = image {
            if enabled { cx.sg.set_draw_color(1.0, 1.0, 1.0) } else { cx.sg.set_draw_color(0.33, 0.33, 0.33) }
            cx.sg.draw_image(Some(&image), x + 2.0, y + 2.0, width - 4.0, height - 4.0);
            if clicked && m_over {
                cx.sg.set_draw_color_a(1.0, 1.0, 1.0, 0.5);
                cx.sg.draw_image(None, x + 1.0, y + 1.0, width - 2.0, height - 2.0);
            }
        }
        if enabled { cx.sg.set_draw_color(1.0, 1.0, 1.0) } else { cx.sg.set_draw_color(0.33, 0.33, 0.33) }
        let label = cx.prop::<Self, _>(me, |w| &w.label);
        if label == "+" {
            cx.sg.draw_image(None, x + width * 0.2, y + height * 0.45, width * 0.6, height * 0.1);
            cx.sg.draw_image(None, x + width * 0.45, y + height * 0.2, width * 0.1, height * 0.6);
        } else if label == "-" {
            cx.sg.draw_image(None, x + width * 0.2, y + height * 0.45, width * 0.6, height * 0.1);
        } else if label == "x" {
            cx.sg.draw_image_quad(None, [x + width * 0.2, y + height * 0.3, x + width * 0.3, y + height * 0.2,
                                         x + width * 0.8, y + height * 0.7, x + width * 0.7, y + height * 0.8]);
            cx.sg.draw_image_quad(None, [x + width * 0.7, y + height * 0.2, x + width * 0.8, y + height * 0.3,
                                         x + width * 0.3, y + height * 0.8, x + width * 0.2, y + height * 0.7]);
        } else {
            let over_size = cx.w::<Self>(me).over_size_text;
            cx.sg.draw_string(x + width / 2.0, y + 2.0 - over_size, Align::CenterX, height - 4.0 + over_size * 2.0, Font::Var, &label);
        }
        if m_over {
            if !no_tooltip || cx.w::<Self>(me).force_tooltip {
                cx.sg.set_draw_layer(None, Some(100));
                TooltipHost::draw_tooltip::<Self>(cx, me, |w| &mut w.tt, x, y, width, height, vp, ());
                cx.sg.set_draw_layer(None, Some(0));
            }
            if let Some(h) = cx.w::<Self>(me).on_hover.clone() { h(cx); }   // return value ignored by DrawControls
        }
    }

    /// lua: ButtonControl.lua:107-117
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, _double_click: bool) -> Focus {
        if !cx.is_shown(me) || !cx.is_enabled(me) { return None; }
        if key == Key::LeftButton { cx.w_mut::<Self>(me).clicked = true; }
        else if let Some(f) = cx.w::<Self>(me).enter_func.clone() { f(cx); }
        Some(me)
    }

    /// lua: ButtonControl.lua:119-130
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        if !cx.is_shown(me) || !cx.is_enabled(me) { return None; }
        if key == Key::LeftButton && cx.w::<Self>(me).clicked {
            cx.w_mut::<Self>(me).clicked = false;
            if cx.is_mouse_over(me) { let f = cx.w::<Self>(me).on_click.clone(); return f(cx); }
        }
        cx.w_mut::<Self>(me).clicked = false;
        None
    }
}
```

### 9.2 `NotesTab` (complete)

```rust
//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/NotesTab.lua
use pob_ui_core::luastr::{gsub, lmatch};       // first-value wrappers (§8 rule 5): gsub(s, p, r) -> String,
                                               // lmatch(s, p) -> Option<String> (CCR-0013 B1; U5 owns the file)
use crate::model::xml::{XmlChild, XmlNode};   // calc:BM-A port of runtime/lua/xml.lua (pob-app re-exports the types)
use pob_ui_core::{prelude::*, color_codes::ColorCodes, controls::{ButtonControl, EditArgs, EditControl, LabelControl}};
use crate::main_mode::background::draw_background;

const NOTES_DESC: &str = "^7You can use Ctrl +/- (or Ctrl+Scroll) to zoom in and out and Ctrl+0 to reset.\n\
This field also supports different colors.  Using the caret symbol (^) followed by a Hex code or a number (0-9) will set the color.\n\
Below are some common color codes PoB uses:\t";

pub struct NotesTab { pub build: CtrlId, pub last_content: String, pub show_color_codes: bool, pub mod_flag: bool }
impl Widget for NotesTab { const NAME: &'static str = "NotesTab"; const CAPS: Caps = Caps::empty(); } // drawn by BuildMode

type CcField = fn(&ColorCodes) -> &String;

impl NotesTab {
    /// lua: NotesTab.lua:12-51
    pub fn new(build: CtrlId) -> New<NotesTab> {
        New::new(Base::new(Anchor::NONE, rect(0, 0, 0, 0)),
                 NotesTab { build, last_content: String::new(), show_color_codes: false, mod_flag: false })
            .host()
            .init(|cx, me| {
                let me = me.id();
                let notes_desc = cx.add(me, "notesDesc", LabelControl::new(Anchor::new(TopLeft, me, TopLeft), rect(8, 8, 150, 16), NOTES_DESC));
                // The label is read at construction; the click handler reads colorCodes at click time, as the Lua does.
                let btn = |cx: &mut Cx, key: &'static str, to: CtrlId, x: i32, y: i32, cc: CcField, name: &str| {
                    let label = format!("{}{}", cc(&cx.main.cc), name);
                    cx.add(me, key, ButtonControl::new(Anchor::new(TopLeft, to, TopLeft), rect(x, y, 100, 18), label,
                        move |cx| { let c = cc(&cx.main.cc).clone(); NotesTab::set_color(cx, me, &c) })).id()
                };
                let normal = btn(cx, "normal", notes_desc.id(), 0, 48, |c| &c.NORMAL, "NORMAL");
                let magic = btn(cx, "magic", normal, 120, 0, |c| &c.MAGIC, "MAGIC");
                let rare = btn(cx, "rare", magic, 120, 0, |c| &c.RARE, "RARE");
                btn(cx, "unique", rare, 120, 0, |c| &c.UNIQUE, "UNIQUE");
                let fire = btn(cx, "fire", normal, 0, 18, |c| &c.FIRE, "FIRE");
                let cold = btn(cx, "cold", fire, 120, 0, |c| &c.COLD, "COLD");
                let lightning = btn(cx, "lightning", cold, 120, 0, |c| &c.LIGHTNING, "LIGHTNING");
                btn(cx, "chaos", lightning, 120, 0, |c| &c.CHAOS, "CHAOS");
                let strength = btn(cx, "strength", fire, 0, 18, |c| &c.STRENGTH, "STRENGTH");
                let dexterity = btn(cx, "dexterity", strength, 120, 0, |c| &c.DEXTERITY, "DEXTERITY");
                let intelligence = btn(cx, "intelligence", dexterity, 120, 0, |c| &c.INTELLIGENCE, "INTELLIGENCE");
                cx.add(me, "default", ButtonControl::new(Anchor::new(TopLeft, intelligence, TopLeft), rect(120, 0, 100, 18),
                    "^7DEFAULT", move |cx| NotesTab::set_color(cx, me, "^7")));
                // EditControl(anchor, rect, init, prompt, filter, limit, changeFunc, lineHeight, allowZoom)
                let edit = cx.add(me, "edit", EditControl::new(Anchor::new(TopLeft, fire, TopLeft), rect(0, 48, 0, 0),
                    EditArgs { init: Some(String::new()), filter: Some("^%C\t\n".into()), line_height: Some(16.0),
                               allow_zoom: true, ..Default::default() }));
                cx.base_mut(edit).width = Prop::f(move |cx, _| cx.base(me).width.raw() - 16.0);   // `self.width - 16` (raw)
                cx.base_mut(edit).height = Prop::f(move |cx, _| cx.base(me).height.raw() - 128.0);
                cx.add(me, "toggleColorCodes", ButtonControl::new(Anchor::new(TopRight, me, TopRight), rect(-10, 70, 160, 20),
                    "Show Color Codes", move |cx| {
                        let s = !cx.w::<NotesTab>(me).show_color_codes;
                        cx.w_mut::<NotesTab>(me).show_color_codes = s;
                        NotesTab::set_show_color_codes(cx, me, s);
                    }));
                cx.select_control(me, Some(edit.id()));
            })
    }

    /// lua: NotesTab.lua:53-62
    pub fn set_show_color_codes(cx: &mut Cx, me: CtrlId, setting: bool) {
        cx.w_mut::<NotesTab>(me).show_color_codes = setting;
        let toggle = cx.child_as::<ButtonControl>(me, "toggleColorCodes");
        let edit = cx.child_as::<EditControl>(me, "edit");
        if setting {
            cx.get_mut(toggle).label = "Hide Color Codes".into();
            let buf = gsub(&gsub(&cx.get(edit).buf, "%^x(%x%x%x%x%x%x)", "^_x%1"), "%^(%d)", "^_%1");
            cx.get_mut(edit).buf = buf;
        } else {
            cx.get_mut(toggle).label = "Show Color Codes".into();
            let buf = gsub(&gsub(&cx.get(edit).buf, "%^_x(%x%x%x%x%x%x)", "^x%1"), "%^_(%d)", "^%1");
            cx.get_mut(edit).buf = buf;
        }
    }

    /// lua: NotesTab.lua:64-73
    pub fn set_color(cx: &mut Cx, me: CtrlId, color: &str) {
        let show = cx.w::<NotesTab>(me).show_color_codes;
        let mut text = color.to_owned();
        if show { text = gsub(&gsub(color, "%^x(%x%x%x%x%x%x)", "^_x%1"), "%^(%d)", "^_%1"); }
        let edit = cx.child_as::<EditControl>(me, "edit");
        let (sel, caret) = { let e = cx.get(edit); (e.sel, e.caret) };
        if sel.is_none() || sel == Some(caret) {
            EditControl::insert(cx, edit.id(), &text);
        } else {
            let last_color = lmatch(&EditControl::get_sel_text(cx, edit.id()),
                                    if show { "^.*(%^_x%x%x%x%x%x%x)" } else { "^.*(%^x%x%x%x%x%x%x)" })
                .unwrap_or_else(|| "^7".to_owned());
            let stripped = gsub(&EditControl::get_sel_text(cx, edit.id()),
                                if show { "%^_x%x%x%x%x%x%x" } else { "%^x%x%x%x%x%x%x" }, "");
            EditControl::replace_sel(cx, edit.id(), &format!("{text}{stripped}{last_color}"));
        }
    }

    /// lua: NotesTab.lua:75-82
    pub fn load(cx: &mut Cx, me: CtrlId, xml: &XmlNode, _file_name: &str) {
        let edit = cx.child_as::<EditControl>(me, "edit");
        for node in &xml.children {
            if let XmlChild::Text(t) = node { EditControl::set_text(cx, edit.id(), t, false); }
        }
        let buf = cx.get(edit).buf.clone();
        cx.w_mut::<NotesTab>(me).last_content = buf;
    }

    /// lua: NotesTab.lua:84-88
    pub fn save(cx: &mut Cx, me: CtrlId, xml: &mut XmlNode) {
        Self::set_show_color_codes(cx, me, false);
        let buf = cx.get(cx.child_as::<EditControl>(me, "edit")).buf.clone();
        xml.children.push(XmlChild::Text(buf.clone().into()));
        cx.w_mut::<NotesTab>(me).last_content = buf;
    }

    /// lua: NotesTab.lua:90-112
    pub fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, ev: &mut InputEvents) {
        {
            let b = cx.base_mut(me);
            b.x = vp.x.into(); b.y = vp.y.into(); b.width = vp.width.into(); b.height = vp.height.into();
        }
        let mut i = 0;
        while let Some(e) = ev.next_ipairs(&mut i) {
            if let EventKind::KeyDown { .. } = e.kind {
                if e.key == Key::Char(b'z') && cx.sys.is_key_down(Key::Ctrl) {
                    let edit = cx.child_as::<EditControl>(me, "edit"); EditControl::undo(cx, edit.id());
                } else if e.key == Key::Char(b'y') && cx.sys.is_key_down(Key::Ctrl) {
                    let edit = cx.child_as::<EditControl>(me, "edit"); EditControl::redo(cx, edit.id());
                }
            }
        }
        cx.process_controls_input(me, ev, vp);
        draw_background(cx, vp);
        cx.draw_controls(me, vp, None);
        let edit = cx.child_as::<EditControl>(me, "edit");
        let mod_flag = cx.w::<NotesTab>(me).last_content != cx.get(edit).buf;
        cx.w_mut::<NotesTab>(me).mod_flag = mod_flag;
    }
}
```

### 9.3 A subclass (`GemSelectControl`, skeleton)

```rust
//! Port of … src/Classes/GemSelectControl.lua (newClass("GemSelectControl", "EditControl"))
pub struct GemSelectControl { pub edit: EditControl, pub skills_tab: CtrlId, pub index: usize, pub dps_builder: Coroutine, /* … */ }
impl Widget for GemSelectControl {
    pob_ui_core::impl_parts!(GemSelectControl: edit => EditControl);
    const NAME: &'static str = "GemSelectControl";
    const CAPS: Caps = EditControl::CAPS;                       // plus whatever GemSelect adds
    /// lua: GemSelectControl.lua:… (`self.EditControl:Draw(viewPort, noTooltip and not self.forceTooltip)`)
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        /* … */
        let force = cx.w::<Self>(me).force_tooltip;
        EditControl::draw(cx, me, vp, no_tooltip && !force);   // super call: static, same node
        /* … */
    }
    /// lua: GemSelectControl.lua:815-819
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, dbl: bool) -> Focus {
        /* … */
        EditControl::on_key_down(cx, me, key, dbl)              // `newSel == self.EditControl and self or newSel` is identity
    }
}
impl GemSelectControl {
    /// lua: GemSelectControl.lua:28 GemSelectControl(anchor, rect, skillsTab, index, changeFunc, forceTooltip, imbued)
    pub fn new(anchor: Anchor, rect: [Prop<f64>; 4], skills_tab: CtrlId, index: usize, /* … */) -> New<GemSelectControl> {
        let edit = EditControl::state(EditArgs { filter: Some("^ %a':-".into()), ..Default::default() });
        New::new(Base::new(anchor, rect), GemSelectControl { edit, skills_tab, index, dps_builder: Coroutine::default() })
            .host()
            .init(|cx, me| EditControl::init(cx, me.id()))        // EditControl children anchor to the GemSelect node
            .init(|cx, me| { /* GemSelectControl.lua:30.. own setup */ })
    }
}
```

### 9.4 Build top bar: a side-effecting property, an instance `Draw`, a model mutation

```rust
// lua: Build.lua:143-157
let anchor_top_bar_right = cx.child_as::<PlainControl>(me, "anchorTopBarRight").id(); // (illustrative lookup)
let build_name_conditional = move |cx: &Cx| cx.pos(anchor_top_bar_right).0 < 800.0;
let build_name = cx.add(me, "buildName", PlainControl::new(Anchor::new(Left, save_as, Right), rect(4, 36, 0, 20)));
cx.base_mut(build_name).width = Prop::f(move |cx, _control| {
    let c = |k: &'static str| cx.child(me, k).unwrap();
    let limit = if build_name_conditional(cx) { 203.0 } else {
        cx.pos(anchor_top_bar_right).0 - 98.0 - 62.0
            - cx.size(c("pointDisplay")).0 - cx.size(c("levelScalingButton")).0 - cx.size(c("characterLevel")).0
            - cx.size(c("back")).0 - cx.size(c("save")).0 - cx.size(c("saveAs")).0
    };
    let bnw = cx.sg.draw_string_width(16.0, Font::Var, &cx.build().build_name);   // self.buildName
    let bm = cx.w::<BuildMode>(me);
    bm.str_width.set(pob_lua::math::m_min(bnw, limit));        // Cell: written by a property (Build.lua:154)
    bm.str_limited.set(bnw > limit);
    bm.str_width.get() + 98.0
});
// lua: Build.lua:158 `self.controls.buildName.Draw = function(control) … end`
cx.set_ov(build_name, |ov| ov.draw = Some(Rc::new(move |cx, control, _vp, _nt| { /* Build.lua:159-… */ })));

// lua: a selFunc that changes the model, sets buildFlag, and may open a cross-owner confirm popup
move |cx: &mut Cx, _index: usize, value: &DropItem| {
    let class_id = *value.value.downcast_ref::<f64>().unwrap();
    let spec = cx.build().model.spec.get();                    // PassiveSpecRef (calc doc §5.9)
    if class_id == spec.curClassId.get() { return; }
    /* … */
    spec.SelectClass(class_id);                                // model mutation through the calc doc's API
    BUILD_SPEC_UNDO.add_undo_state(cx, me, false);           // spec:AddUndoState()
    cx.build_mut().build_flag = true;
}
```

---
## 10. App state, model boundary, calc integration

The model and calc are specified by **`docs/port/calc-architecture.md`** (binding; "the calc doc"). In this document, calc-plan unit ids are written `calc:<ID>` (for example `calc:BM-A`), and UI unit ids are written `U<n>`. The facts below come from the calc doc:

- The model is `pob_calc::model::Build` (`BuildRef = Rc<Build>`), with PoB's field names verbatim (`build.itemsTab.items`, `build.skillsTab.socketGroupList`, `build.configTab.input`, `build.calcsTab.mainOutput`; calc doc §5.8).
- Aliased Lua tables are `Rc` handles with `Field<T>`/`Shared<T>`/`Cell` interior mutability (calc doc D2, §3).
- **The calc mutates the model in place** (calc doc D13).
- `pob-calc` is `Rc`, `!Send`, and single-threaded.
- On wasm32 the calc runs in a Web Worker (calc doc D15).

### 10.1 App state (`pob-app`)

```rust
// pob-app/src/app.rs
pub struct PobApp { pub cx: Cx }                       // implements pob_host::App
pub trait CxApp {
    fn app(&self) -> &AppState;
    fn app_mut(&mut self) -> &mut AppState;
    fn build(&self) -> &BuildState { self.app().build.as_ref().unwrap_or_else(|| lua_error!("build is nil")) }
    fn build_mut(&mut self) -> &mut BuildState { self.app_mut().build.as_mut().unwrap_or_else(|| lua_error!("build is nil")) }
    fn data(&self) -> &'static model::Data { model::data() }            // pob_gamedata::Data (process global, calc doc §4)
}
impl CxApp for Cx { /* downcast cx.ext */ }
/// Disjoint-borrow helpers: `let a = app(&*cx.ext); cx.sg.draw_string(.., &a.main.x)`.
pub fn app(ext: &dyn std::any::Any) -> &AppState;
pub fn app_mut(ext: &mut dyn std::any::Any) -> &mut AppState;

pub struct AppState {
    pub launch: LaunchState,                           // Launch.lua fields (devMode, versionNumber, promptMsg, subscripts…)
    pub main: MainState,                               // Main.lua fields not in MainCore: mode/newMode/modes, settings,
                                                       // paths, sharedItems, option values, inputEvents, toasts…
    pub trees: TreeCache,                              // main.tree[version]: model::PassiveTreeRef + Rc<TreeAssets> (UI images)
    pub list_mode: Option<Ctrl<ListMode>>,
    pub build: Option<BuildState>,                     // Some while in BUILD mode
    pub calc_factory: std::rc::Rc<dyn Fn() -> Box<dyn CalcFacade>>, // Rust calc (default) | LuaCalcBridge (dev)
}
pub struct BuildState {
    pub node: Ctrl<BuildMode>,                         // the buildMode ControlHost (UI-only fields live in its widget state)
    pub tabs: TabIds,                                  // tree, skills, items, calcs, config, import, notes, party, compare (nodes)
    pub model: model::BuildRef,                        // the calc doc's Build: everything the calc reads and XML stores
    pub calc: Box<dyn CalcFacade>,
    pub calc_func: Option<std::rc::Rc<dyn MiscCalc>>,  // configTab.calcFunc (UI-side home; the calc doc's ConfigTab has none)
    pub calc_base: Option<model::Output>,              // configTab.calcBase
    pub build_flag: bool, pub mod_flag: bool,
    pub db_file_name: Option<String>, pub db_file_sub_path: Option<String>, pub build_name: String,
    /* … remaining UI-side buildMode fields (characterLevelAutoMode, …); fields the calc reads live in `model` */
}
```

The UI may hold model `Rc` handles (`ItemRef`, `SocketGroupRef`, `PassiveSpecRef`, `NodeRef`) wherever the Lua holds the table, because aliasing is the calc doc's model too. Two rules apply:

- Never hold a `Field`/`Shared` borrow across a call that takes `cx`. `Field::get` clones; `Shared::with` runs a leaf closure.
- **Identity** for `ListValue` keys and tooltip memo keys is `Rc` pointer identity, **with the `Rc` kept alive** by the key (§6.8, §6.9). So, as in Lua, an address cannot be reused while it is still compared.

The recalc happens exactly where `Build.lua:1255-1275` does it. Inside `BuildMode::on_frame` (U20):

1. If `build_flag` is set, clear it.
2. `calc.build_output(&model)`. This is the calc doc's port of `CalcsTab:BuildOutput`, which fills `build.calcsTab.{mainEnv, calcsEnv, mainOutput, calcsOutput}` and bumps `outputRevision`.
3. `calc_func`/`calc_base` (Lua `configTab.calcFunc`/`calcBase`) are set from `calc.misc_calculator(&model)`.
4. The Lua follow-ups run in order: `RefreshStatList`, power invalidation, …

All of this is synchronous on desktop (D14).

### 10.2 Model boundary: who ports which Lua lines

`pob-app` names model and calc types only through `pob-app/src/model.rs` (re-exports of `pob_calc::model::*`, the classes, `pob_gamedata`, and `pob_lua::{LStr, Name, Table}`) and `pob-app/src/calc/`. The calc doc's `unported!` stubs return defaults (calc doc D9), so the UI compiles and runs against the real crate from the calc plan's Wave 1 onward. There is no UI-side placeholder model.

| Lua lines | Owner | UI responsibility |
|---|---|---|
| `Build.lua:976-1099` (Load/Save), `:2222-2335` (LoadDB/SaveDB) | calc:BM-A (`pob-calc/src/model/build_xml.rs`) | U20 calls them and ports the UI tails (title, mode flags, tab Load/Save hooks for UI-only sections such as Notes and TreeView). |
| `ItemsTab.lua:1216-1727`, `:2998-3010` | calc:BM-A (`model/items_tab.rs`) for the model halves | U34 ports `Draw` (1419-1572), `PopulateSlots`, list refreshes, and the UI tails of AddItem/sets. Each split function is commented `// lua: … (model part: calc:BM-A <fn>)`. |
| `TreeTab.lua:500-560` (Load/PostLoad/Save/SetActiveSpec data) | calc:BM-A (`model/tree_tab.rs`) | U25 ports the UI tails (spec list control refresh, viewer state). |
| `CalcsTab.lua:163-225` (Load/Save), `:441-773` (BuildOutput, BuildPower, PowerBuilder, power stats, GetMiscCalculator, undo state) | calc:BM-A, **calc:PWR** (`modules/power.rs`, the PowerBuilder as a resumable iterator) | U26 drives the PWR iterator once per frame where `BuildPower` is called. U39/U40 only display. |
| `SkillsTab.lua:441-630`, `:1129-1274`, `:1478-1502` | calc:BM-B (`model/skills_tab.rs`) | U30 ports the UI tails (gem slot rebuild, group list refresh). |
| `ConfigTab.lua:839-1129`, `:1130-1204` (BuildModList); `ConfigOptions` apply; `Data/ModMap.lua` | calc:BM-B, calc:CO-A/B/C | U41/U42 port the controls, visibility and tooltips. The ConfigOptions metadata comes from `pob_gamedata`. |
| `PartyTab.lua:516-669`, `:718-1042` (Load/Save, ParseBuffs, exports) | calc:BM-B | U45 ports the controls and display. |
| `Item.lua`, `ItemTools.lua`, `PassiveTree.lua:1-908, 1041-1062`, `PassiveSpec.lua`, `runtime/lua/xml.lua` | calc:IT-A/B/C, PT, PS-A/B/C, BM-A | U23 ports only the image/sprite/connector parts of `PassiveTree.lua` (`:909-1040`, and the `NewImageHandle` blocks inside 75-794) into `tree_view/assets.rs`. |

**Model halves ported by the calc units** (CCR-0019 §1, CCR-0080 §1). BM-B and BM-A also ported the model halves of functions the table above leaves to the UI, each with its own `@pob` marker; the UI units keep the UI statements of those functions and call the model halves: SkillsTab.lua 323-349 (`imbuedSupport_gemChangeFunc`), 1103-1127 (`FindSkillGem`), 1275-1278 (`SetDisplayGroup`), 1452-1475 (`NewSkillSet`, `RebuildImbuedSupportBySlot`), 89/95-96/423-426 (constructor); ConfigTab.lua 672-692 (`defaultState`, `ConfigTab::ctor_controls_model`: one control-state table per option with a `var`), 1205-1240 (`ImportCalcSettings`), 1280-1300 (`NewConfigSet`), 1337-1362 (`SetActiveConfigSet`); PartyTab.lua 22-36 (`ctor_model`) [U30/U41/U42/U45]; Build.lua 64-110, 603-698 (`build_xml::Init_model`) [U20]; ItemsTab.lua 37, 70-74, 101-108, 140-235, 1196-1215, 2560-2605, 2978-2993 and ItemSlotControl.lua 21-123 (`ItemsTab::ctor_model`, the slot tables with the `ItemSlotControl` fields the model reads; U33/U34 wrap them in their controls) [U33/U34]; TreeTab.lua 26-38 [U25]; CalcsTab.lua 24-45, 151-154, 370-375 and CalcSectionControl.lua 20-49 (`CalcsTab::ctor_model`, `sectionList`) [U26]. `Build:Save`'s display stats come from a hook the UI registers (`build_xml::set_save_stats_hook`, U21); the UI-only savers (Notes, TreeView, Import) are kept and written back unchanged until U20 provides them (CCR-0080 §8).

Name clash: the calc doc's model structs (`model::ItemsTab`, `model::SkillsTab`, `model::ConfigTab`, …) share names with the UI tab widgets (`tabs::items::ItemsTab`, …), so model types are always path-qualified as `model::X` in UI code.

Any additional model or calc API a UI unit needs is requested through the calc doc's change process (calc doc §10.3, "CCR"). No UI unit invents model types.

### 10.3 Undo of model domains

| Lua `CreateUndoState` owner | Ported by | Stack location |
|---|---|---|
| `PassiveSpec` (`build.spec:AddUndoState`; PassiveSpec.lua is an UndoHandler) | calc:PS-A (state fns); U25 wires `UndoOps` | `BuildMode` state `undo_spec` |
| `ItemsTab` (`ItemsTab.lua:5021-5055`) | U38 (the restore tail, PopulateSlots etc., is U34's) | `ItemsTab` node state |
| `SkillsTab` (`SkillsTab.lua:1397-1441`) | U30 | `SkillsTab` node state |
| `ConfigTab` (`ConfigTab.lua:1242-1269`) | U42 | `ConfigTab` node state |
| `CalcsTab` (`CalcsTab.lua:764-773`) | calc:PWR (state fns); U39 wires `UndoOps` | `CalcsTab` node state |
| `EditControl` (`EditControl.lua:743-761`), `PathControl` | U11, U12 | widget state |

Undo states are deep copies as in Lua (`copyTable`), made with the calc doc's copy helpers.

### 10.4 Calc facade (`pob-app/src/calc/facade.rs`, U15)

```rust
pub use crate::model::{BuildRef, EnvRef, Output, Override, MiscCalculator};

/// The single seam between UI and calc. Desktop: the Rust calc, synchronous (D14).
/// Every method may return Poll::Pending ONLY on wasm, where the calc runs in a Web Worker (calc doc D15).
/// Call sites handle Pending with the placeholder documented at each site; on desktop it never happens,
/// so parity is unaffected.
pub trait CalcFacade {
    /// calcsTab:BuildOutput() (CalcsTab.lua:441-474 via calc:PWR): fills build.calcsTab.{mainEnv,calcsEnv,mainOutput,
    /// calcsOutput}. Lua errors are caught as PoB does (calc doc D15 try_buildOutput) and reported via App::on_lua_error.
    fn build_output(&mut self, build: &BuildRef) -> std::task::Poll<()>;
    /// calcsTab:GetMiscCalculator() == calcs.getMiscCalculator(build) (Calcs.lua:89-114): (calcFunc, calcBase).
    fn misc_calculator(&mut self, build: &BuildRef) -> std::task::Poll<(std::rc::Rc<dyn MiscCalc>, Output)>;
}
/// `calcFunc(override, useFullDPS)`. Like the Lua closure it reads the LIVE build it was created for, so callers may
/// mutate the model temporarily and restore it (GemSelectControl:CalcOutputWithThisGem, ConfigTab tooltips).
pub trait MiscCalc {
    fn call(&self, ov: Option<std::rc::Rc<Override>>, use_full_dps: Option<bool>) -> std::task::Poll<Output>;
}
```

`Override` is the calc doc's type (calc doc §5.6): `spec`, `addNodes`/`removeNodes` (`LjIntMap<NodeRef>`), `repSlotName`, `repItem`, a single `toggleFlask`/`toggleTincture` item, `conditions` (`Vec<Name>`), `extraJewelFuncs` (`ModList`), and `extra`. It is shared by identity, and the calc writes into it (CalcSetup.lua:1424-1430, :1921-1922), so the UI builds a fresh `Rc<Override>` per call exactly where Lua builds a fresh table.

The UI reads results directly from the calc doc's types, using PoB names: `build.calcsTab.mainEnv.player.output`, `.breakdown`, `.modDB` (Tabulate for CalcBreakdownControl), `.mainSkill.skillFlags`, `env.grantedPassives`, `node.finalModList`, `env.conditionsUsed`. There is no UI-side view layer.

Implementations:

| Implementation | Location | Role |
|---|---|---|
| `RustCalc` | `calc/rust.rs` (U15) | Delegates to `pob_calc` (`calcs::getMiscCalculator`, the PWR port of `BuildOutput`). Always `Ready`. |
| `LuaCalcBridge` | `pob-ui-oracle/src/lua_calc.rs` (U15, dev only, desktop) | Serialises the model to XML (calc:BM-A), loads it into a second LuaJIT PoB (`pob-luahost`), runs the Lua calc, then **injects** the resulting env/output/breakdown/modDB into the calc doc's Rust types through calc:ORC-X (`pob-oracle` inject). The UI therefore reads exactly what PoB computed, and UI parity is calc-neutral before the Rust calc is at parity. `MiscCalc::call` re-serialises the live model each call: slow, used only in parity scenarios. |
| `WorkerCalc` | `calc/worker_web.rs` (U54, optional) | The wasm Web Worker client (calc doc D15). Returns `Pending` until the worker answers. Call sites show PoB-style placeholders, and tooltip memo keys include a result epoch. Not a parity target. |

PowerBuilder (calc:PWR), the GemSelect DPS sort and the ItemDB/NotableDB sorts keep PoB's per-frame stepping. A UI coroutine (§6.13) or the PWR iterator is stepped where the Lua resumes it.

---
## 11. Verification: "looks the same"

### 11.1 Levels

| Level | What is compared | Reference | Gate |
|---|---|---|---|
| T0 Framework differential | `GetPos`/`GetSize`/`IsShown` over 500 random anchor graphs (all 9×9 point pairs, chains, collapse with hidden/shown other, Prop closures, negative floors). Plus `UndoHandler` sequences, `ProcessControlsInput` focus/consume sequences, `TabAdvance`, and `#t` borders. | The **real** `Control.lua`, `ControlHost.lua`, `UndoHandler.lua` loaded into mlua/LuaJIT with a 20-line class shim. | Bit-exact (U4). |
| T1 Lua helpers | `tostring(f64)`, `string.format`, patterns, `sort` with ties, `LjIntMap` order: covered by calc:P2's L0 suite. U5 adds the UI-only byte-string helpers (`luastr.rs`). | LuaJIT | Exact (calc:P2, U5). |
| T2 Fonts | Width, cursor index and glyph quads: ≥40 cases per function hand-derived from `.tgf` metrics and `r_font.cpp`, plus 200 strings × 4 heights × 3 fonts as goldens. Optionally, real-SG metrics from the Wine probe. | `r_font.cpp` port review; real `SimpleGraphic.dll` under Wine | Exact (U2). |
| **T3 API trace (primary)** | Per frame: normalised draw ops per `(layer, sub)` bucket, with effective viewport, colour and args, plus side effects. | Unmodified PoB Lua under LuaJIT drawing through **our** `pob-sg`, driven by the same script. | 0 divergences (eps 1e-6). Multiset buckets only where §8 rule 6 (b) applies. |
| T4 State | Build XML at each `snapshot` (Rust `save` vs Lua `build:SaveDB()`), and `Settings.xml`. | Same oracle | Byte-identical, else identical after attribute-order normalisation with a written reason. |
| T5 Backend | wgpu read-back vs CPU `raster` for the same `Frame`. | `pob-sg` raster | ±2/255 on ≥ 99.9 % of pixels (U6). |
| T6 Real SG | `DrawStringWidth`/`DrawStringCursorIndex` tables and probe-scene screenshots. At cut-over, 15 app scenes. | Real PoB under Wine, using the `runtime/` already in `.PathOfBuilding` | Metrics exact; pixels ±2/255 on ≥ 99.5 %; scenes SSIM ≥ 0.995 (U2, U53). |

T3 covers all UI logic and layout. Both sides go through the same `pob-sg`, so T3 cannot see font-engine or backend bugs; T2, T5 and T6 cover those. Wine is not installed on this machine. The user installs it (`pacman -S wine`); agents cannot. Until then, the T6 gates are marked pending, and T2 alone gates the font unit.

### 11.2 The oracle (`pob-ui-oracle`, U8)

Boot sequence:

1. Create the mlua LuaJIT VM.
2. Install `pob-oracle`'s natives: `pob_oracle::jit::{PRELUDE, install_natives}` are `pub` (CCR-0028 §1). `pob-ui-oracle/src/boot.rs` inserts its source-patch hook into the prelude's `loadfile` and asserts its anchor exists exactly once.
3. Load the HeadlessWrapper prefix, as `JitPob::boot` does.
4. **Override every SG global** with Rust functions bound to an `Rc<RefCell<Sg>>` and `Rc<RefCell<Sys>>` with tracing on. Covered: `SetDrawLayer`, `GetDrawLayer`, `SetViewport`, `SetDrawColor`, `GetDrawColor`, `DrawImage`/`DrawImageQuad` (with `ui_api.cpp`'s exact argument-count dispatch), `DrawString`, `DrawStringWidth`, `DrawStringCursorIndex`, `StripEscapes`, `NewImageHandle` (userdata with `Load`/`Unload`/`IsValid`/`IsLoading`/`ImageSize`/`SetLoadingPriority`), `GetScreenSize`, `GetScreenScale`, `Get/SetDPIScaleOverridePercent`, `GetCursorPos`, `IsKeyDown`, `GetTime`, `Copy`, `Paste`, `SetWindowTitle`, `OpenURL`, `TakeScreenshot`, `ConPrintf`.
5. Before each draw call, `debug.getinfo(2, "Sl")` sets the trace `src`, for example `"Classes/ButtonControl.lua:54"`.
6. `dofile("Launch.lua")`, then drive `launch:OnInit`, the key callbacks and `launch:OnFrame` from the `.pobscript`.

**Determinism and order patches** (`patches.rs`). Each patch is a text substitution applied when the module loads, asserts that the original text is present (so upstream drift fails loudly), and states its reason.

1. `GetTime`, `os.time`, `os.clock` and `os.date` use a virtual clock that advances 16 ms per frame; `os.time(t)` interprets `t` as UTC (LuaJIT's field rules), and `io.open` for writing notes the file so `GetFileModifiedTime` follows the headless `Fs` (0 for files not written during the run, else MSVC `file_clock` ticks of Unix 1 600 000 000 plus the virtual clock; CCR-0028 §5, §12).
2. `math.randomseed(0)`, and `math.random` is wrapped.
3. `launch.CheckForUpdate` and the update UI are no-ops.
4. `GetUserPath` points at the fixture directory.
5. Image loads are synchronous, on both sides.
6. **Creation order.** `Common.lua:158 new()` records the object's creation sequence number in a weak-keyed oracle side table (no field on the object, so `pairs(object)` and copies are unchanged; CCR-0028 §11). `pairs(self.controls)` becomes `__canonPairs(self.controls)` at `ControlHost.lua:32` and `:110` and `Main.lua:1298` and `:1308`. `__canonPairs` yields integer keys ascending, then the other keys by `__seq`. `PopupDialog.lua:25` and `TradeQuery.lua:630` are order-independent (anchor resolution, deletion) and are not patched.
7. **Control markers.** Inside the patched `DrawControls` loop, `__tracePush(<HostClass>.<key>)` runs before `control:Draw` and `__tracePop()` after. Rust emits the same path from `Cx::draw_controls`.
8. `Net`/`LaunchSubScript`: scripted fixtures, delivered at the start of the next frame on both sides.

The **audit mode** runs **twice** with patch 6 disabled (native hash order is randomised per process; calc doc D8) and lists every host whose traces differ from canonical order while controls overlap. The orchestrator reviews the list once; any real difference gets a per-host order override plus a patch.

The **gallery harness** (for widget units before the app shell exists) works like this:

- A mini-launch loads `Modules/Common.lua`, `Data/Global.lua` and `Modules/Main.lua` (methods only; no `main:Init`).
- It then runs `lua/gallery/<name>.lua`, which builds a `ControlHost` from PoB classes and runs `ProcessControlsInput` + `DrawControls` per frame.
- The Rust counterpart `src/gallery/<name>.rs` builds the same controls through `pob-ui-core`.
- `tests/parity.rs` discovers galleries and scenario directories automatically, so units never edit shared files.
- As built (CCR-0028 §6, §8): `lua/gallery/<name>.lua` returns a table `G` with optional `G:Init()` and required `G:OnFrame(inputEvents, viewPort)`, run after `lua/gallery_launch.lua`; `src/gallery/<name>.rs` exports `pub fn app(parts: pob_host::PlatformParts) -> Box<dyn pob_host::App>` (build.rs generates `GALLERIES`; `_<x>.rs` is a shared support module); an optional `lua/gallery/<name>.pobscript` drives it (default `frames 3`). `#! expect-text TEXT` / `#! expect-snapshot NAME` lines are expectations for `tests/parity.rs`, a custom harness (`harness = false`): `cargo test -p pob-ui-oracle -- <unit>` selects cases by name; scenario directories named `_…` are Lua-only runs. A run fails when PoB shows its error prompt (`launch.promptMsg` starting with `^1Error`). `ui-parity` adds `--screen WxH`, `--audit` (two child processes; union written to `audit.txt`), `--lua-only`, `--out DIR`, `--name NAME`. `NewFileSearch` follows SG (dirOnly filter, `.`/`..` skipped, case-insensitive glob, upper-cased-name order). `FileWrite` effects are recorded on neither side yet and are left out of T3 until U16 needs them (CCR-0028 §3).
- Until U8's switch-over (CCR-0028 §10, A8 needs-code) the oracle drives both sides with its own `runner::drive`, pinned to U7's expansion by `runner::tests::drive_follows_u7_expansion` and `drive_matches_run_headless`.

**CLI.**

| Command | Purpose |
|---|---|
| `cargo run -p pob-ui-oracle --bin ui-parity -- <scenario.pobscript \| --gallery name> [--dpi 1.5] [--png]` | Writes `target/ui-parity/<name>/{lua,rust}.jsonl`, `report.txt` (first divergence with ±5 ops, both `src` locations, control path), and `frame_<n>_{lua,rust,diff}.png` through the CPU raster. |
| `--bin trace-render` | Renders a Lua trace through our backend. This separates backend bugs from port bugs. |
| `--bin pob-lua-gui` | Real PoB Lua on `pob-sg-wgpu` in a winit window (PoB natively on Linux), used as the visual reference. `--record out.pobscript` records a human session for replay. |

### 11.3 Scenarios and CI

- Every UI unit ships `crates/pob-ui-oracle/scenarios/<unit>/*.pobscript`, or `lua/gallery/<unit>_*.lua` plus `src/gallery/<unit>_*.rs` for widget units.
- Each covers at least: the idle frame, hover with tooltip, every primary interaction, and every popup in the unit's range.
- Each runs at 1280×800 @1.0 and 1920×1080 @1.5.
- App scenarios use `crates/pob-ui-oracle/fixtures/user1` (`Settings.xml` plus 6 corpus builds with TotalDPS > 0, taken from `scripts/parity`).
- Rust-side numbers come from `LuaCalcBridge` until the Rust calc reaches parity.
- Text masks are allowed **only** after switching to the Rust calc, and each one names an open calc issue.

```
export CARGO_BUILD_JOBS=3; command -v sccache >/dev/null && export RUSTC_WRAPPER=sccache
cargo test -p pob-sg -p pob-ui-core                       # fast; T0/T2 + luastr (mlua/LuaJIT dev-deps)
cargo test -p pob-sg-wgpu                                  # T5 (lavapipe/llvmpipe acceptable)
cargo test -p pob-ui-oracle -- <unit>                      # T3/T4 for one unit
cargo check -p pob-app --target wasm32-unknown-unknown     # web stays possible
```

**Machine limits.** Calc doc D20 is binding for UI agents too.

- At most **3 agents run at once across both plans**.
- Each agent has its own git worktree and target dir.
- **Every** cargo and LuaJIT oracle invocation goes through `scripts/port/cargo.sh` / `scripts/port/oracle.sh`, which take the global `flock`, so only one compile and one oracle run at a time.
- Prefer `cargo check -p <crate>`. The commands above are shown bare; agents run them through `scripts/port/cargo.sh`.

---
## 12. Work units

### 12.1 Rules

- **Ownership.** One agent per unit. "Owns" lists files that only that unit edits. **U0 creates every listed file as a compiling stub**, including every `lib.rs`, `mod.rs`, `prelude.rs`, `Cargo.toml` and the split-struct declarations, with the signatures from §4-§10 and `todo!()` bodies. Later units never edit `mod`/`pub use` declarations (U0's), even inside a `lib.rs`/`mod.rs` whose remaining content they own. They never edit another unit's files. Files that U0 delivers *complete* pass to the wave-1 unit of that crate: `pob-sg/src/{types.rs,trace.rs}` to U1; `pob-ui-core/src/{ids.rs,prop.rs,widget.rs}` to U4; `pob-app/src/{model.rs,calc/*}` to U15. `pob-ui-oracle/build.rs` (U8) generates the gallery module list from `src/gallery/*.rs`, so gallery files need no `mod` edits.
- **Split structs.** Where one Lua class spans several units (ItemsTab, SkillsTab, ConfigTab, ImportTab, TreeTab, CalcsTab, CompareTab, BuildMode), U0 declares the struct in `mod.rs` with one `pub <part>: <part>::State` field per owning unit (`#[derive(Default)]`). Each owner defines its `State` and its `impl` blocks in its own file. The `Widget` impl lives with the unit that owns the class's `Draw`.
- **Amendments.** A unit that needs a shared type or signature change submits a patch to this document and to the stub. Nobody else changes shared surfaces.
- **"Accept"** always also means: `cargo check`, `cargo clippy` and `cargo test -p <crate>` pass with the commands of §11.3; provenance headers are present; the file header lists DIVERGENCE/ORDER notes. For UI units, every scenario or gallery in the unit's directory reaches **zero T3 divergences** at both resolutions, and T4 where the unit saves state.
- **`ext:` dependencies** name calc-plan units (calc doc §11). A UI unit may start once its `ext:` units have merged their **stubs** (calc doc D9: `unported!` returns defaults). It is *accepted* on LuaCalcBridge numbers once the listed units are merged:
  - `ext:model-core` = calc:BM-A (Build, XML) + calc:P1 (`pob_gamedata`);
  - `ext:model-tree` = calc:PT, calc:PS-A/B/C, calc:LUT;
  - `ext:model-items` = calc:IT-A/B/C + calc:BM-A;
  - `ext:model-skills` = calc:BM-B;
  - `ext:model-config` = calc:BM-B, calc:CO-A/B/C;
  - `ext:model-party` = calc:BM-B;
  - `ext:calc` = calc:CA + calc:PWR (Rust) or the LuaCalcBridge (U15, which needs calc:BM-A and calc:ORC-X).
- **Ids.** UI units are `U<n>`; calc units are written `calc:<ID>`. The two plans share the machine budget and lock scripts (§11.3).

### 12.2 Wave 0 (serial)

| Unit | Lua sources | Owns | Deps | Acceptance |
|---|---|---|---|---|
| **U0** Skeleton and frozen contracts | None (this document); a read-only survey of `_SimpleGraphic.def.lua` 1-516, `Control.lua`, `ControlHost.lua`, `Common.lua:83-230` | Workspace `Cargo.toml` (members plus pinned deps, §3; shared with calc:REN/calc:U0: append-only, merged serially); every new crate's `Cargo.toml`/`lib.rs`/`mod.rs`/`prelude.rs`; every module file named in §3 and §12 as a stub with the final public signatures; `pob-sg/src/{types.rs,trace.rs}` complete; `pob-ui-core/src/{ids.rs,prop.rs,widget.rs}` complete (types, trait, VTable, Overrides, New); `pob-app/src/{model.rs,calc/facade.rs}` (re-exports of the calc-plan crates, which exist as stubs after calc:U0); `app/pob-port-{desktop,web}` stubs; `pob-ui-core/tests/compile_shape.rs` | calc:REN, calc:U0 | `cargo check -p pob-sg -p pob-sg-wgpu -p pob-host -p pob-ui-core -p pob-app -p pob-ui-oracle` is green. `compile_shape.rs` compiles the §9.1 and §9.2 bodies against the stubs. The stub file list equals §12 exactly. The legacy apps still build. |

### 12.3 Wave 1 (8 units that may run in parallel within the 3-agent cap; deps: U0; the wave gate is all eight accepted after integration)

| Unit | Lua / C++ sources | Owns | Deps | Acceptance |
|---|---|---|---|---|
| **U1** sg-core | `_SimpleGraphic.def.lua`; SG `ui_api.cpp:725-1230`, `r_main.cpp:100-560, 975-1130, 1440-1680`, `common.cpp:201-260` (pinned to `3b1a346`); `Common.lua:1118-1126`; `Data/Global.lua:7-100` (palette, colour helpers) | `pob-sg/src/{api.rs,frame.rs,color.rs,sys.rs}`, trace bodies (`normalize`, `diff`) in `trace.rs`, `docs/port/ui/sg-semantics.md` | U0 | One unit test per §2 row except fonts and images (layer re-emit, nil layer, `height == 0` viewport, BIND+COLOR+QUAD, escape leak and alpha=1, `GetDrawColor` after `DrawString`, rounding at dpi 1.0/1.25/1.5, `GetDrawLayer` = sub). `normalize`/`diff` property test (a shuffle across buckets gives no diff; a shuffle within a bucket gives a diff). Palette equals the DLL table. Every row re-verified at `3b1a346` and cited in `sg-semantics.md`. |
| **U2** sg-font | SG `r_font.cpp`/`.h`; `runtime/SimpleGraphic/Fonts/*` | `pob-sg/src/font/{mod.rs,tgf.rs,tga.rs,layout.rs}`, `pob-sg/tests/font_*.rs`, `crates/pob-ui-oracle/probe/**` (Wine probe `Launch.lua`, README, metrics schema) | U0 (U1/U3 for integration) | All 7×16 atlases parse. The TGA is flipped bottom-up with alpha = coverage. The height map matches a hand table (11→10, 29/30→28, 31→32, >64→64). T2 goldens: ≥40 hand-derived cases each for width, cursor index and draw quads (tabs, `\n`, escapes, tofu, all 5 aligns, off-screen lines). Cursor offsets correct for UTF-8. Probe assets ready; JSON comparison runs once Wine output exists. |
| **U3** sg-image and loaders | SG `r_texture.cpp`, `r_main.cpp:1366-1433`; `ui_api.cpp:377-485`; `PassiveTree.lua:909-935`; `PassiveTreeView.lua:549, 1291` (usages) | `pob-sg/src/{image.rs,assets.rs}`, `pob-host/src/{loader_native.rs,loader_headless.rs,loader_web.rs}` | U0 | png/jpg/webp/tga decode, with every image under `src/Assets` and `TreeData/3_27` decoding. Sync loads report size immediately; `image_size` blocks semantics for ASYNC; a missing file gives `(0,0)` and `IsLoading == true`; unknown flags error; `(path, flags)` dedup; mips; deterministic mode; the wasm loader compiles. |
| **U4** ui-core framework | `Control.lua` 1-178, `ControlHost.lua` 1-115, `UndoHandler.lua` 1-58, `Common.lua:40-56` (coroutine wrapper), `:413-416`, `:549-580` (wipeTable, isValueInArray); `CalcsTab.lua:475-495` (coroutine use) | `pob-ui-core/src/{cx.rs,arena.rs,layout.rs,host.rs,input.rs,focus.rs,undo.rs,coroutine.rs,lua_error.rs,main_core.rs,color_codes.rs}`, `pob-ui-core/tests/{tier0_layout.rs,tier0_host.rs,tier0_undo.rs,tier0_len.rs,lua/shim.lua}` | U0 | T0 suites bit-exact against the real Lua files: 500 random anchor graphs; dispatch sequences (focus return, prevSel skip, imbued early return, drag lock, `OnKeyUp` truthiness, `mouseClipRect`, holes); undo cap 101; `TabAdvance`; `lua_len` property test. Deferred free while a click closes its own popup. A dead id panics with the type name. Canonical child order. Coroutine resume/yield/dead with `with_cx`. `impl_parts!` upcast test. |
| **U5** Lua-helper audit | §8 rules; `crates/pob-calc/src/lua_pattern.rs`; the calc plan's `pob-lua` (calc:P2: `math`, `format`, `string`, `pattern`, `sort`, `ljtab`, `LStr`, `lua_error!`) | `pob-ui-core/src/luastr.rs` (UI-only byte-string helpers: caret-safe sub/insert for EditControl, `tostring` glue), `pob-ui-core/tests/luastr.rs` | U0, calc:P2 | Every §8 helper resolves to a `pob_lua` function or to `luastr.rs`. Gaps are filed as CCRs to calc:P2 (no edits to `pob-lua`). `luastr` tests against LuaJIT (`string.sub/find/gsub` byte semantics on UTF-8 input). |
| **U6** wgpu backend and CPU raster | SG `r_main.cpp:300-560` (batching, scissor, blend), `:975-1030` (RT), `assets/gles/*` | `crates/pob-sg-wgpu/**`, `pob-sg/src/raster.rs` | U0 | T5 for 6 synthetic frames (tinted quads, rotated quads, repeat/clamp/flipped UVs, nested viewports and scissor, layered alpha, text). Runs on Vulkan and with `WGPU_BACKEND=gl`. No sRGB anywhere. `cargo check --target wasm32-unknown-unknown -p pob-sg-wgpu`. |
| **U7** host | SG `sys_video.cpp:470-640`, `sys_main.cpp:270-370`, `ui_main.cpp:16-61, 380-591`; `Launch.lua:21-200` (callback contract), `:259-324` (DownloadPage); `crates/pob-calc/src/jit.rs:29-132` (oauth/open_url/http); `crates/pob-ui/src/build_store_wasm.rs` (web Fs ideas) | `crates/pob-host/src/{lib.rs,desktop.rs,web.rs,input.rs,pacing.rs,headless.rs,script.rs,platform_native.rs,platform_web.rs,fs_native.rs,fs_web.rs,net_native.rs,net_web.rs}`, `app/pob-port-desktop/**`, `app/pob-port-web/**` (demo App drawing a test card) | U0 | Key-map table tests (named keys, lowercase chars, extra OnChar on press and repeat, wheel Down+Up, double click incl. reset, swallowed F10/Shift+Pause KeyUp, non-ASCII → `?`). DPI and cursor rounding. Pacing rules. `.pobscript` parser round-trip. Headless determinism. Desktop demo runs; web demo builds. |
| **U8** ui-oracle | `HeadlessWrapper.lua`; `Launch.lua`; `Common.lua:158-230` (`new`); `ControlHost.lua:31-47, 109-115`; `Main.lua:1290-1315`; SG `ui_api.cpp` (argument dispatch); `crates/pob-calc/src/{jit.rs,headless.rs}` | `crates/pob-ui-oracle/src/{lib.rs,boot.rs,sg_bridge.rs,patches.rs,markers.rs,gallery.rs,runner.rs,diff_report.rs}`, `src/bin/{ui_parity.rs,trace_render.rs,pob_lua_gui.rs}`, `tests/parity.rs`, `lua/*.lua` (non-gallery), `fixtures/**`, `scenarios/_smoke/**`, plus the `pub` change in `pob-luahost/src/jit.rs` (CCR) | U0, calc:REN (U1-U3, U7 for acceptance) | PoB boots to the build list and to a fixture build at 1280×800@1.0 and 1920×1080@1.5. Two runs give byte-identical traces. Every patch asserts its anchor text. Audit mode runs and writes its host list. `ui-parity --gallery smoke` runs both sides end to end on a trivial gallery. `pob-lua-gui` displays PoB. |

### 12.4 Wave 2: generic controls (parallel, 6 units; deps: wave 1)

| Unit | Lua sources | Owns | Deps | Acceptance (galleries) |
|---|---|---|---|---|
| **U9** Basic controls and draw helpers | `LabelControl` 1-24, `ButtonControl` 1-130, `CheckBoxControl` 1-117, `RectangleOutlineControl` 1-23, `SectionControl` 1-32, `ScrollBarControl` 1-327, `SliderControl` 1-185, `DraggerControl` 1-146; `Main.lua:1497-1573` (DrawArrow, DrawCheckMark, WorldToScreen, RenderCircle, RenderRing) | `pob-ui-core/src/controls/{label,button,check_box,rect_outline,section,scroll_bar,slider,dragger}.rs`, `src/draw.rs`, `pob-ui-oracle/{lua/gallery,src/gallery}/u9_*` | U4, U1, U2, U8 | Each control in idle, hover, press, release, drag, disabled and locked states; button image and `+ - x` glyphs; slider steps; scrollbar drag and wheel. Zero divergences. |
| **U10** Tooltip | `Tooltip.lua` 1-698, `TooltipHost.lua` 1-27; `Assets/*header*.png`, influence icons, `TreeData/*Oil.png` | `pob-ui-core/src/tooltip/{tooltip.rs,host.rs}`, galleries `u10_*` | U4, U9 (button as host) | Plain lines; separators; rarity headers with flavour text on and off; recipes; multi-column overflow; `maxWidth` wrap; placement flips at all four edges; `CheckForUpdate` memo; `GetDrawColor` restore; `draw_override` (as TreeTab uses it). Zero divergences. |
| **U11** Edit family | `EditControl` 1-761, `ResizableEditControl` 1-50 | `pob-ui-core/src/controls/{edit,resizable_edit}.rs`, galleries `u11_*` | U9, U10 | Typing through filters (`%D`, `^%w%p `, `%c`, `^%C\t\n`); selection by drag, shift+arrows and double click; Ctrl+A/C/V/X/Z/Y; the Tab char via extra OnChar; numeric +/- buttons; protected (FIXED) mode; multi-line with scrollbars; zoom; placeholder; `inactiveText`. Caret byte indices equal Lua's. |
| **U12** Lists | `ListControl` 1-493, `TextListControl` 1-122, `PathControl` 1-110 | `pob-ui-core/src/controls/{list,text_list,path}.rs`, galleries `u12_*` | U9, U10 | `ListHooks`/`ListOverrides`/`ListSource` (hook set frozen at the wave gate after grepping every subclass); column labels and sort; ellipsis via cursor index; drag-reorder and cross-list drag with drag text at layer 20; `CanReceiveDrag` presence semantics; path breadcrumbs. Zero divergences. |
| **U13** Dropdown and search | `DropDownControl` 1-573, `SearchHost` 1-153 | `pob-ui-core/src/controls/{drop_down,search_host}.rs`, galleries `u13_*` | U9, U10, U11 | Dropped list at sublayer 5; `dropUp`; type-to-search with highlights; `CheckDroppedWidth`; BODY/OUT/HOVER tooltips; `selFunc` timing; `DropDownHooks` (frozen at gate). Zero divergences. |
| **U14** Popups | `PopupDialog` 1-96; `Main.lua:1660-1786` (OpenPopup/ClosePopup/Message/Confirm/NewFolder/CloudError), `:1797-1822` (WrapString); `Main.lua:437-443` (popup layer and dim) | `pob-ui-core/src/{popups.rs,wrap.rs}`, `src/controls/popup_dialog.rs`, galleries `u14_*` | U9, U11 | Message popup, confirm with 2 and 3 buttons, new-folder popup; Enter/Escape routing; default focus; string-key and nil anchors; stacked popups; close-from-own-button (deferred free). Zero divergences. |

### 12.5 Wave 3: app shell (deps: wave 2; U22 is the gate for wave 4)

| Unit | Lua sources | Owns | Deps | Acceptance |
|---|---|---|---|---|
| **U15** Calc facade and Lua bridge | `CalcsTab.lua:441-474, 760-763`; `Calcs.lua:89-114`; `Build.lua:1255-1275`; `HeadlessWrapper.lua` | `pob-app/src/calc/{mod.rs,facade.rs,rust.rs}`, `pob-app/src/model.rs`, `pob-ui-oracle/src/lua_calc.rs` | U8, calc:BM-A, calc:ORC-X, calc:CA (stubs suffice for `RustCalc`) | For the 6 fixture builds, after `build_output` through the bridge, `build.calcsTab.mainOutput`/`calcsOutput`/breakdowns equal PoB's. `MiscCalc::call` with addNodes, removeNodes and repSlotName+repItem equals Lua `calcFunc`. `RustCalc` runs against the calc stubs. |
| **U16** Launch and Main core | `Launch.lua` 1-416 (except `ApplyUpdate`/`CheckForUpdate` 325-357, stubbed); `Main.lua:1-864` (Init, OnFrame, input queue, SetMode, settings, shared items, path popup), `:1574-1659` (StatColor, folder ops), `:1787-1796`; `ToastNotification.lua` 1-254 | `pob-app/src/{lib.rs,app.rs,launch.rs}`, `src/main_mode/{mod.rs,init.rs,on_frame.rs,settings.rs,path_popup.rs,folders.rs,toast.rs}`, `src/platform/net.rs` (`launch:DownloadPage`, `Launch.lua:259-324`, over `Sys::net`) | wave 2, U15, ext:model-core | `Settings.xml` round-trips byte-identically for a corpus. Bottom bar, mode switch, toasts, error prompt on panic (`ShowErrMsg`), F1 opens About (stubbed to U17). Zero divergences on `scenarios/u16_*`. |
| **U17** Main popups | `Main.lua:865-1316` (Options), `:1317-1372` (manifest/update popup, display only), `:1373-1485` (About), `:1486-1496` (DrawBackground) | `pob-app/src/main_mode/{options_popup.rs,update_popup.rs,about_popup.rs,background.rs}` | U16 | Every Options widget and its scrolled clip (the instance `DrawControls`/`GetMouseOverControl` overrides at `Main.lua:1297,1307`); About tabs; background tiling at sublayer -100. Zero divergences. |
| **U18** BuildList mode | `Modules/BuildList.lua` 1-220, `BuildListHelpers.lua` 1-201, `BuildListControl` 1-287, `FolderListControl` 1-99 | `pob-app/src/build_list/{mod.rs,helpers.rs,build_list_control.rs,folder_list_control.rs}`, `src/platform/storage.rs` | U16 | Browse, sort modes, search, new folder, drag build to folder (the PathControl instance overrides), rename/delete popups, open build. Zero divergences over the fixture user directory. |
| **U19** Build shell A (Init) | `Build.lua:1-770` (helpers, `buildMode:Init`: top bar, sidebar mode buttons, skill selectors with collapse anchors, stat box, tab creation, savers) | `pob-app/src/build/{mod.rs (BuildMode, BuildState, TabIds; U0 declares the split), init.rs}` | U16, U15 | New build and fixture build: top bar and sidebar layout at 3 widths (the `buildName` Cell property, instance `Draw`s at `Build.lua:158,198,584`). Tabs are stubs. Zero divergences, using LuaCalcBridge numbers (or `RustCalc` on calc stubs with the stat-box text masked until U15 lands). |
| **U20** Build shell B | `Build.lua:771-1617` (SyncLoadouts, EstimatePlayerProgress, CanExit, Shutdown, CloseBuild, Load, Save, ResetModFlags, UpdateSecondaryAscendancyDropdown, **OnFrame 1160-1352**, display stat, conversion/save/save-as popups, spectre library, class dropdowns, similar popup), `:2222-2335` (LoadDB/SaveDB) | `pob-app/src/build/{loadouts.rs,io.rs,on_frame.rs,popups.rs,class_drop.rs}` (Load/Save/LoadDB/SaveDB model halves are calc:BM-A's; §10.2) | U19, ext:model-core | Save produces XML identical to Lua for the 6 fixtures (T4). Recalc on level/class change. Overlay-pane input order (`#inputEvents` loop, `Build.lua:1216`). Save and save-as popups. Zero divergences. |
| **U21** Build stats | `Build.lua:1618-2221` (RefreshSkillSelectControls, FormatStat, breakdown index, sidebar breakdown, AddDisplayStatList, item warnings, RefreshStatList, CompareStatList, AddStatComparesToTooltip); `Modules/BuildDisplayStats.lua` 1-293 | `pob-app/src/build/{skill_select.rs,stats.rs,display_stats.rs}` | U19, U15 | With LuaCalcBridge: sidebar text and compare tooltips have zero divergences for the 6 fixtures. The stat formatter is unit-tested against Lua for 500 values. |
| **U22** NotesTab: end-to-end gate | `NotesTab.lua` 1-112 | `pob-app/src/tabs/notes.rs` | U19-U21 | The full app (Launch, BuildList, Build, Notes) has zero divergences over typing, colour buttons, show/hide codes, undo/redo, Tab key and save (T4). The desktop binary runs. **Its review is the template gate for every wave-4 unit.** |

### 12.6 Wave 4: tabs and the tree (may run in parallel within the shared 3-agent cap, calc doc D20; schedule by `ext:` readiness)

| Unit | Lua sources | Owns (`pob-app/src/…`) | Deps | Acceptance (zero divergences on `scenarios/<unit>/`) |
|---|---|---|---|---|
| **U23** Tree assets and view | `PassiveTreeView.lua:1-1411` (ctor, Load/Save, compare helpers, Draw, DrawImageRotated, DrawAsset, Zoom, Focus, search match); image/sprite-sheet parts of `PassiveTree.lua:75-794` (each `NewImageHandle`/sprite block, listed in the port notes), `:909-1040` (LoadImage, BuildConnector, BuildArc); `Main.lua:352-364` | `tree_view/{mod.rs,assets.rs,connectors.rs,draw.rs,input.rs,compare.rs}` | U22, ext:model-tree | Pan; zoom levels 0/6/12; allocate a path; search highlight; jewel radius; heat map; compare overlay; tree versions 2_6, 3_19, 3_27 (node maps iterate in `LjIntMap` order, so compare in order). Frame time < 8 ms on 3_27. |
| **U24** Tree view names and tooltips | `PassiveTreeView.lua:1412-1856` (AddNodeName, AddNodeTooltip, AddCompareNodeTooltip, AddCompareNodeName) | `tree_view/{names.rs,tooltip.rs}` | U23, U15 | Node, mastery, jewel and compare tooltips, including "Allocating this node will give you" diffs through the bridge. |
| **U25** TreeTab core | `TreeTab.lua:1-1046` (the data halves of 500-560 are calc:BM-A); `PassiveSpecListControl` 1-132; `PassiveMasteryControl` 1-59 | `tabs/tree/{core.rs,popups.rs,spec_list.rs,mastery.rs}` | U23, ext:model-tree | Two-line responsive reflow at 1280/1600/1920; spec manage, version convert, import/export popups; ModifyNodePopup; mastery popup; spec undo. |
| **U26** Power report | `TreeTab.lua:1047-1169` (SetPowerCalc, BuildPowerReportList); `PowerReportListControl` 1-153; the per-frame `calcsTab:BuildPower()` call sites (the PowerBuilder itself, `CalcsTab.lua:441-773`, is calc:PWR) | `tabs/tree/{power.rs,power_report_list.rs}` | U25, U15, calc:PWR | The PWR iterator is stepped exactly where PoB resumes the coroutine; heat map and report list identical through the bridge for 3 fixtures. |
| **U27** Timeless search | `TreeTab.lua:1170-2888` (FindTimelessJewel) | `tabs/tree/timeless.rs` | U25, U28, ext:model-tree (legion/abyss LUTs) | Popup layout, filters, sliders (with the `tooltip.Draw` overrides at `:1749/1785/1814/1841`), and results identical to Lua for 5 searches. |
| **U28** Timeless and notable lists | `TimelessJewelListControl` 1-352, `TimelessJewelSocketControl` 1-57, `NotableDBControl` 1-293 | `tabs/tree/{timeless_list.rs,timeless_socket.rs,notable_db.rs}` | U12, U13 | Galleries or scenarios per control; the NotableDB sort coroutine. |
| **U29** Gem select | `GemSelectControl` 1-854; `GemTooltip.lua` 1-261 | `controls/{gem_select.rs,gem_tooltip.rs}` | U11, U10, U15, ext:model-skills | Filter and sort modes; DPS sort order identical through the bridge (the model is mutated temporarily); gem tooltips. |
| **U30** SkillsTab A | `SkillsTab.lua:1-742` (ctor, Draw, copy/paste; the model halves of 441-630 are calc:BM-B), `:1081-1502` (UpdateGemSlots, FindSkillGem, SetDisplayGroup, AddSocketGroupTooltip, undo state, skill-set popups; 1129-1274 and 1478-1502 model halves are calc:BM-B) | `tabs/skills/core.rs` | U29, ext:model-skills | Socket group CRUD; skill sets; copy/paste; undo/redo. |
| **U31** SkillsTab B | `SkillsTab.lua:743-1080` (CreateGemSlot); `SkillListControl` 1-267, `SkillSetListControl` 1-127, `MinionListControl` 1-107, `MinionSearchListControl` 1-71 | `tabs/skills/{gem_slots.rs,skill_list.rs,skill_set_list.rs,minion_list.rs,minion_search_list.rs}` | U29, U12 | Gem slot rows (rebuilt controls are removed correctly); list drag/reorder; minion lists. |
| **U32** Item lists and slots | `ItemSlotControl` 1-185, `ItemListControl` 1-387, `ItemDBControl` 1-400, `ItemSetListControl` 1-145, `SharedItemListControl` 1-80, `SharedItemSetListControl` 1-112; `Modules/ItemSlotHelper.lua` 1-40 | `tabs/items/{slot.rs,item_list.rs,item_db.rs,set_list.rs,shared_list.rs,shared_set_list.rs,slot_helper.rs}` | U12, U13, U10, ext:model-items | Drag DB → list → slot; collapse slot stacking; flask toggles; DB filters and the sort coroutine. |
| **U33** ItemsTab A (constructor) | `ItemsTab.lua:1-1215` | `tabs/items/ctor.rs` | U32 | Layout at 3 widths; every control's visibility rules. |
| **U34** ItemsTab B | `ItemsTab.lua:1216-1947` (Draw, PopulateSlots, list refreshes, the UI tails of Load/Save, sets, AddItem, DeleteItem, sort; the model halves of 1216-1727 are calc:BM-A) | `tabs/items/core.rs` | U33 | Sets; equip/unequip; delete; undo/redo; T4 save. |
| **U35** ItemsTab display item | `ItemsTab.lua:1948-2608` | `tabs/items/display_item.rs` | U33 | Create/edit display item; variants; affix sliders; cluster controls; custom controls. |
| **U36** ItemsTab crafting A | `ItemsTab.lua:2609-3120` (set manage, CraftItem, EditDisplayItemText, Enchant, anoint helpers, AnointDisplayItem); `Modules/AddImplicitPopup.lua` 1-414 | `tabs/items/{craft_a.rs,add_implicit.rs}` | U35 | Each popup. |
| **U37** ItemsTab crafting B | `ItemsTab.lua:3121-3992` (Corrupt, AddCustomModifier, AddCrucibleModifier, AddItemSetTooltip) | `tabs/items/craft_b.rs` | U35 | Each popup. |
| **U38** ItemsTab tooltips | `ItemsTab.lua:3993-5055` (header influence, FormatItemSource, jewel compare, AddItemTooltip, AddItemStatDifferences, undo state) | `tabs/items/tooltip.rs` | U33, U10, U15 | Tooltips for 100 uniques; stat-diff blocks through the bridge. |
| **U39** CalcsTab | `CalcsTab.lua:1-162, 226-440` (ctor, Draw, NewSection, display stat, CheckFlag, SearchMatch; Load/Save 163-225 and 441-773 are calc:BM-A/calc:PWR); `CalcSectionControl` 1-550; `Modules/CalcSections.lua` as data (from calc:P1's dump schema `calc_sections`) | `tabs/calcs/{core.rs,section.rs,sections_data.rs}` (CalcFormat.lua is calc:CT's `modules/calc_format.rs`; U39 calls it) | U15, U21 | Column packing at 3 widths; skill switch; section collapse; search. Text identical through the bridge. |
| **U40** CalcBreakdownControl | `CalcBreakdownControl` 1-789 | `tabs/calcs/breakdown.rs` | U39 | Breakdown panes (pinned and overlay), mod tables, radius visual. |
| **U41** ConfigTab A | `ConfigTab.lua:1-834` (CustomModBlockControl, ctor building every option control) | `tabs/config/{custom_mod_block.rs,ctor.rs}` | U13, U11, ext:model-config | Every option type; ifFlag/ifSkill visibility; custom mod blocks; tooltips with enable compares (bridge). |
| **U42** ConfigTab B | `ConfigTab.lua:835-1362` (UpdateControls, Draw, sets, undo; the model halves of 839-1204 are calc:BM-B/CO-C); `ConfigSetListControl` 1-114; `Modules/ConfigModBrowser.lua` 1-311; `Modules/ConfigVisibility.lua` 1-160 | `tabs/config/{core.rs,set_list.rs,mod_browser.rs,visibility.rs}` | U41 | Config sets; mod browser; visibility identical through the bridge (`used()` tables). |
| **U43** ImportTab A | `ImportTab.lua:1-775` (OAuth/account controls, ctor, Load/Save, Draw); `PoEAPI.lua` 1-238; `Modules/BuildSiteTools.lua` 1-129 | `tabs/import/{accounts.rs,core.rs,poe_api.rs,build_sites.rs}` | U16, U7 (Net), ext:model-core | Build-code import/export round-trips with Lua (T4); site URL import with recorded fixtures. |
| **U44** ImportTab B | `ImportTab.lua:776-1932` (character list, downloads, import tree/items/skills, reimport state) | `tabs/import/character.rs` | U43, ext:model-items/skills/tree | Account import flows against recorded HTTP fixtures produce identical builds (T4). |
| **U45** PartyTab | `PartyTab.lua:16-515, 670-717` (ctor, Draw; Load/Save and ParseBuffs/exports are calc:BM-B) | `tabs/party.rs` | U11, ext:model-party | Paste and parse buffs; enable toggles; layout. |

### 12.7 Wave 5

| Unit | Lua sources | Owns (`pob-app/src/…`) | Deps | Acceptance |
|---|---|---|---|---|
| **U46** CompareTab A | `CompareTab.lua:1-1649` (helpers, ctor, InitControls, config controls, import/remove/reimport, copy-to-primary, import popups incl. the `buildList` instance overrides `:1607-1628`) | `tabs/compare/{init.rs,import.rs}` | all of wave 4 | Scenarios per popup and control cluster. |
| **U47** CompareTab B | `CompareTab.lua:1650-2515` (Draw, DrawControlList, LayoutTreeView, LayoutConfigView, set selectors, calcs skill controls, scroll input, helpers); `CompareEntry` 1-552 | `tabs/compare/{layout.rs,entry.rs}` | U46 | Layout at 3 widths for all compare views. |
| **U48** CompareTab C | `CompareTab.lua:2516-3161` (ComparePowerBuilder, RunComparePowerReport); `ComparePowerReportListControl` 1-165; `CompareCalcsHelpers` 1-476 | `tabs/compare/{power.rs,power_list.rs,calcs_helpers.rs}` | U46, U26 | Compare power report identical through the bridge. |
| **U49** CompareTab D | `CompareTab.lua:3162-4048` (DrawSummary, DrawStatList, DrawTree, DrawItemExpanded, abyss sockets, DrawItems) | `tabs/compare/draw_a.rs` | U47 | Summary, tree and items views. |
| **U50** CompareTab E | `CompareTab.lua:4049-5036` (DrawSkills, calcs tooltip/header, DrawCalcs, DrawConfig) | `tabs/compare/draw_b.rs` | U47 | Skills, calcs and config views. |
| **U51** Trade (optional, deferred) | `TradeQuery` 1415, `TradeQueryGenerator` 1609, `TradeQueryRequests` 515, `TradeQueryRateLimiter` 285, `TradeHelpers` 623, `TradeStatWeightMultiplierListControl` 42, `CompareBuySimilar` 538 | `trade/**` (split into ≥ 3 units when scheduled) | U38, U7 | Recorded-fixture scenarios. |
| **U52** Network extras (optional) | `ExtBuildListControl` 440, `ExtBuildListProvider` 63, `PoBArchivesProvider` 150 | `ext_build_list/**` | U18, U7 | Recorded-fixture scenarios. |
| **U54** Web calc client (optional) | — (calc doc D15 worker protocol) | `pob-app/src/calc/worker_web.rs` | U15, calc plan's worker | `cargo check --target wasm32-unknown-unknown`; the web app shows the build with numbers once the worker answers. |
| **U53** Integration and cut-over | — | `crates/pob-ui-oracle/scenarios/e2e/**`, `tests/golden/**`, app renames | everything, plus the Rust calc at parity | Every scenario green with the **Rust calc** (masks only for open calc issues); T6 SSIM ≥ 0.995 on 15 Wine scenes; wasm loads the build list and tree; perf pass (tree < 4 ms at 1080p). Then: rename `app/pob-port-*` → `app/pob-*` and delete `crates/pob-ui` (`pob-engine`/`pob-data` are deleted by calc:INT-4). *Renames and deletion done (CCR-0145 §6; INT-4, AMENDMENTS A13).* |

### 12.8 Parallelism summary

| Wave | Units | Gate |
|---|---|---|
| 0 | U0 | Stubs compile; contracts frozen |
| 1 | U1-U8 (8) | sg, framework and host tests green; oracle deterministic |
| 2 | U9-U14 (6) | Galleries zero-diff; List/DropDown hook tables frozen |
| 3 | U15-U18 in parallel, then U19 → U20 ∥ U21 → U22 | **NotesTab end-to-end zero-diff; desktop app runs** |
| 4 | U23-U45 (23), within the shared 3-agent cap (calc doc D20). Order: U23, U25, U29, U32, U39, U41 first (unblock their chains), then by `ext:` readiness | Per-unit scenario dirs zero-diff (bridge) |
| 5 | U46-U54 | Full parity with the Rust calc; cut-over |

Critical path: U0 → U4 → U9 → U11/U12/U13 → U16 → U19 → U20 → U22 → U23 → U25 → U27 → U53.

---

## 13. Risks and open items

1. **SG master vs the shipped DLL.** Done by U1 (CCR-0021, `docs/port/ui/sg-semantics.md`; `r_font.cpp`, `common.cpp`, `common.h` are identical, `r_main.cpp` differs in about 760 diff lines). T6 (Wine) still confirms metrics and pixels once Wine is installed.
2. **Canonical vs real hash order.** The order differs only where controls overlap within one host and layer. The audit mode (U8) finds those hosts. The fix is a per-host order override plus a patch, recorded in `patches.rs`.
3. **Calc-plan timing.** Wave 4 needs model ranges from calc:BM-A/BM-B/PT/PS/IT. Against stubs (`unported!` defaults) UI units compile and run early, but they are *accepted* only after their `ext:` units merge (§12.1). Split functions (§10.2) need both plans to keep the comment cross-references in sync. A drift is a CCR.
4. **Lending hazards** (§7.2 rule 4). Lending an object that is re-read during the call silently sees `Default`. Mitigation: lending is restricted to Tooltip, PassiveTreeView, Coroutine and objects listed in the unit header. Reviewers grep `lend(` against that list.
5. **`#inputEvents` border model.** This needs LuaJIT's array-part sizing. U4 property-tests it; the only use is `Build.lua:1216`.
6. **Web.** The calc runs in a Web Worker (calc doc D15), so the web build uses `WorkerCalc` (Poll-returning facade, U54) and is not a parity target. TreeData is 552 MB, so the host prefetches per tree version. `ImageSize` cannot block on web, so sync loads must be prefetched. CORS limits import, trade and OAuth (out of scope for parity). Unwinding is unavailable on wasm, so a panic is fatal there.
7. **Performance.** Prop closures and recursive `GetPos` cost about what they cost in Lua. The tree view is the hot spot; U23 and U53 profile it.
8. **Borrow ergonomics.** Associated-function widgets are verbose (`cx.w::<Self>(me)` per access). This is accepted in exchange for compile-time aliasing safety. Helpers (`cx.prop`, `cx.child_as`, `cx.lend`, the disjoint `cx.ui`/`cx.sg` borrows) keep ports close to the Lua.
9. **Licensing of redistributed fonts and assets** (critic): must be checked before any web deploy. This is not a UI architecture blocker.
10. **`ImageHandle::unload`**: **closed** (CCR-0023 A). `unload(&self)` is kept; textures are never released (`Frame::releases` stays empty; U6 samples `@black` for a `TexId` bound before its upload).
11. **Net completions**: decided, `App::on_net` (CCR-0026 A; §5.2 Frame order).
12. **Placeholder types** (CCR-0013 C). UI-U0 defined these minimally so the stubs compile; each owner may amend them before its wave gate: `ListColumn`, `ColumnProp`, `ScrollDir`, `TextListColumn`, `TextListRow` and the non-§6 constructor signatures of the generic controls (U9/U12); `DropTipMode`, `DropTipArgs`, `DropDownHooks` fields, `SearchHost` fields (U13); `TooltipLine`, `TooltipBlock` (U10); `ArrowDir`, `world_to_screen` (U9); `EditUndoState` (U11); `script::{Script, Step}` (U7). **Amended by their owners** (the tables are in the CCRs, the code is the reference): U9 CheckBox/ScrollBar/Slider/Dragger/RectangleOutline stubs, `ArrowDir::from_lua`, `lua_for_num` (CCR-0069 B); U10 `TooltipLine.separator_image: Option<ImageHandle>`, `handled`, `bounds`, the remaining Tooltip fields, `calculate_columns`, `module_load`/`RECIPE_NAMES` (CCR-0073 E); U11 `EditUndoState.caret: Rc<Cell<usize>>`, `edit::UNDO`, `filter`/`filter_pattern`, the remaining EditControl fields and methods, `ResizableEditControl::new(.., Option<f64> ×4)` (CCR-0081 §2); U12 `PathControl.sub_path: Option<String>`, `TextListRow` fields, `Rc<TextListRow>`, `TextListControl::new(.., Option<columns>, ..)` (CCR-0078 C); U13 `DropItem.detail`, `SelFunc` with `Option<&DropItem>`, `set_sel_num`, the `DropDownControl` fields and super-call methods, `SearchHost.search_infos` (CCR-0086 B, C); U4 `Coroutine::try_resume`, `co::running`, `co::any_alive`, `MainCore::new`, `Cx::headless` (CCR-0034 B); `TreeCache`, `TreeAssets`, `ListMode`, `LaunchState`, `MainState` (U16/U18/U23); the split-struct parts (`State`, §12.1).
13. **Small frozen additions** (CCR-0013 A5): `Sg::viewport()`, `Sg::current_draw_color()` (inspection), `FontSet::empty()` (no PoB assets: measuring returns 0, drawing applies escapes only), `FontFace::heights()`, `Align::from_lua`/`lua_name`, `Font::index`, `Quad::is_culled` (moved from `DrawCmd::is_culled`), `frame::LayerSink`, `frame::digest`. `Sg::set_draw_color*` check `renderEnable` like `ui_api.cpp:821`.
14. **Calc tag text with `divVar`** (U40 CalcBreakdownControl, CalcBreakdownControl.lua 481/485). For a Multiplier/PerStat tag with `divVar`, Lua prints the `div` EvalMod last wrote into the shared tag (calc doc §5.3 forbids that write). Port: print `tag.div` or, when `tag.divVar` is set, `actor.modDB:GetMultiplier(tag.divVar, cfg)` of the breakdown's actor/cfg, and record the difference in `known_diffs.toml` (CCR-0015 §1).
15. **Unowned helpers** (CCR-0033 §2, §6). `lua-utf8` (U5's `luastr::utf8`, a port of luautf8 bdd3d7f) and `formatNumSep` exist twice (U5's `luastr::format_num_sep`, CT's `calc_format`; the calc reads `main.*` separators through CT's `set_num_sep_settings`); A8 lists the merge into pob-lua. Common.lua's `naturalSortCompare` 673-704, `getFormatNumSep` 830-834, `formatRound`/`formatPercent`/`formatSec` 837-871, `urlEncode`/`urlDecode` 1024-1036 and `string:matchOrPattern` 1038-1079 go to the first consuming unit in wave order (U12 for `naturalSortCompare`/`matchOrPattern`, the CalcBreakdownControl unit for the `format*` family, U43 for `url*`) unless U5 takes them first; `math.random`/`randomseed` (LuaJIT `lib_math.c`) are A8 needs-code for U28/U43.
16. **SG functions `Sys` lacks** (CCR-0084 §3-§4): `GetCloudProvider` (U14 returns `(nil, nil, nil)`, the value on every target) and Lua `MakeDir`'s results (not recursive; an existing directory gives `nil, "Success"`); A8 needs-code for U1/U7/U8.

---

## 14. Errata: corrections relative to the research maps and the proposals

All verified against the sources cited in §2 and the Lua tree.

| # | Claim (source) | Correct behaviour |
|---|---|---|
| 1 | Wheel arrives as `KeyUp` only (map_pob-ui-lua, literal, state-sync) | **KeyDown + KeyUp** pair (`sys_video.cpp:608-620`). |
| 2 | `CENTER`/`RIGHT` align against the viewport width (map, literal, state-sync) | Against the **virtual screen width** (`r_font.cpp:353-357`). So `CompareTab.lua:1782-1785` (`DrawString(0, 40, "CENTER", …)` inside `SetViewport(contentVP…)`) renders shifted right by `contentVP.x` in real PoB, and the port reproduces that. |
| 3 | `GetDrawLayer` returns `(layer, sub)` (literal, state-sync) | Returns **only the subLayer** (`r_main.cpp:1486`). |
| 4 | Escape colours keep alpha / only affect the text (map, literal, state-sync) | Alpha is forced to 1 and the colour **leaks** into the persistent draw colour and `GetDrawColor`. |
| 5 | No extra `OnChar` for control keys (literal, state-sync) | Back/Tab/Return/Escape emit an extra `OnChar` on press **and repeat** (`sys_video.cpp:536-540`). Without it, Tab in Notes breaks. |
| 6 | `on_char(char)` takes a Unicode char (literal, state-sync) | Non-ASCII arrives as `"?"`; `Key::Unknown`. |
| 7 | `ImageSize` is `(0,0)` until loaded (map, all three proposals) | It **blocks until the header is parsed** (`r_main.cpp:1419-1433`). `(0,0)` only for a missing file, whose `IsLoading()` then stays true forever. An unuploaded ASYNC texture draws transparent (`@black`). |
| 8 | Atlas = smallest height ≥ requested (map) | Nearest, ties to the lower; heights rounded up to even first. |
| 9 | Clippy `await_holding_refcell_ref` enforces "no borrow across callbacks" (literal) | That lint applies only across `.await`. This design removes the need: `&mut Cx` borrows are checked at compile time. |
| 10 | Line refs `ControlHost.lua:193/:287`, `PopupDialog.lua:141-172/:202`, `TooltipHost.lua:308`, `NotesTab.lua:290-299` (state-sync) | These do not exist. Correct ranges: SelectControl `ControlHost.lua:15-29`, DrawControls `:109-115`, PopupDialog anchor rules `PopupDialog.lua:25-33`, ProcessInput `:72-96`, DrawTooltip `TooltipHost.lua:15-27`, SetColor `NotesTab.lua:64-73`. The `#inputEvents` loop is `Build.lua:1216`; the raw `selControl` write is `EditControl.lua:448`. |
| 11 | `override.conditions` is a map / `toggleFlask` is a list (state-sync, verification) | `conditions` is an **array of names** (`CalcSetup.lua:696-697`). `toggleFlask`/`toggleTincture` are **one item** (`:1368-1380`). `extraJewelFuncs` is a **ModList** that literal omitted (`:875-905`, `:1425-1432`). The calc writes into the caller's override (`:1425-1432`, `:1921`). |
| 12 | `ImageHandle: Copy` (verification) | Lua handles are shared userdata, so it is an `Rc`-shared slot. |
| 13 | Tofu for codepoints `≥ 128` (verification) | `≥ numGlyph` per atlas, which is 128 for every shipped `.tgf`; the tofu test precedes the escape test only while drawing. |
| 14 | Only `ControlHost.lua:32,110` iterate controls (verification) | `Main.lua:1298,1308` too (instance overrides of the Options popup); `PopupDialog.lua:25` and `TradeQuery.lua:630` are order-independent. |
| 15 | Per-instance overrides limited to a `Custom` widget (verification) | Overrides occur on real controls, on ControlHost methods and on Tooltip instances: §6.4, D6. |
| 16 | Prop closures cannot mutate (state-sync, verification) | `Build.lua:148-157` writes `strWidth`/`strLimited` inside a width property, so `Cell` fields are used (§7.3). |
| 17 | `ButtonControl(anchor, rect, label, onClick)` (all three) | The constructor also takes `onHover, forceTooltip` (`ButtonControl.lua:9`). |
| 18 | `SetFocus` compares with a boolean `hasFocus` (all three) | `hasFocus` starts nil, so `SetFocus(false)` on a never-focused control calls `OnFocusLost` (`Control.lua:141`); hence `Option<bool>`. |
| 19 | "SG v2.5.4, tag to be found" (map, verification) | The DLL self-identifies as `SimpleGraphic v2.5-3b1a346`; pin commit `3b1a346`. |
| 20 | Every key reaches Lua (all three) | `KeyUp(F10)` and Shift+`KeyUp(PAUSE)` are swallowed by SG (`ui_main.cpp:530-541`). |
| 21 | Windows CI runner for real-SG goldens (verification) | Not available here; use Wine locally with the `runtime/` already in `.PathOfBuilding` (the user installs Wine). |
