# SimpleGraphic semantics at `3b1a346` (U1 re-verification of ui doc §2)

Owner: U1. Binding companion to `docs/port/ui-architecture.md` §2: where the two disagree, this
file records what the pinned commit does, and CCR-0021 proposes the matching edits to §2.

## Pin

- Ground truth: **PathOfBuilding-SimpleGraphic `3b1a3468223d0ebd4042d6ce76fc6144718ef79b`**
  (2026-07-22, "Merge pull request #104 … Update LuaJIT version"). The shipped
  `.PathOfBuilding/runtime/SimpleGraphic.dll` contains the string `SimpleGraphic v2.5-3b1a346`
  (offset 1629624); `pob-sg`'s test `color::tests::palette_equals_dll_table` asserts it.
- Fetched with `curl` from `raw.githubusercontent.com/PathOfBuildingCommunity/PathOfBuilding-SimpleGraphic/3b1a346…/<path>`:
  `ui_api.cpp`, `ui_main.cpp`, `ui_main.h`, `ui_local.h`, `ui_console.cpp`,
  `engine/render/{r_main.cpp,r_main.h,r_font.cpp,r_font.h,r_texture.cpp}`,
  `engine/common/{common.cpp,keylist.h}`, `engine/common.h`,
  `engine/system/win/{sys_video.cpp,sys_main.cpp}`.
- The ui doc's line numbers come from the research copy (**master @ 8694a07**). Despite the
  name, master is the *later* code for the renderer: `3b1a346` still has `SetBlendMode`, the
  `stackIdx`/`mask` image arguments (which the DLL's strings and `_SimpleGraphic.def.lua:173,
  221-223, 263-265` also show), inline GLSL shaders, per-vertex viewport clipping and a MurmurHash frame
  digest, where master has scissoring, `assets/gles/*` shader files and XXH3.

Byte-level comparison of the research copy with `3b1a346`:

| File | Lines (master → 3b1a346) | Difference |
|---|---|---|
| `engine/render/r_font.cpp` | 474 → 474 | **identical** |
| `engine/common/common.cpp`, `engine/common.h` | 504, 527 | **identical** |
| `ui_api.cpp` | 2301 → 2351 | `SetBlendMode` (+12 lines at 814), `mask` argument of DrawImage/DrawImageQuad, `Deflate`/`Inflate` gzip flag absent, `u8string` casts |
| `engine/render/r_main.cpp` | 1785 → 2083 | BLEND command, inclusive screen-space cull, per-vertex viewport + fragment `discard` instead of `glScissor`, inline shaders, MurmurHash64A digest, `maskLayer` |
| `engine/render/r_main.h` | 229 → 219 | `r_viewport_s {x, y, width, height}` (master: `lo`/`extent`), `curBlendMode` |
| `ui_main.cpp` | 591 → 591 | `u8string` casts only |
| `engine/render/r_texture.cpp` | 707 → 707 | constructor parameter rename (308-314) only |
| `engine/system/win/sys_video.cpp` | 770 → 769 | Wine host check (110), one line fewer at 320: later lines shift by −1 |
| `engine/system/win/sys_main.cpp` | 764 → 740 | Wine version check removed (436-490); lines before 436 unchanged |

## §2 rows, re-verified at `3b1a346`

"Holds" means the §2 text is exact at the pin. Line numbers are at `3b1a346`.

| §2 row | Status | Source at `3b1a346` | Notes / port |
|---|---|---|---|
| Layer queues | Holds, plus BLEND | `ui_api.cpp:759-781`, `r_main.cpp:1675-1704` | A switch also appends `BLEND(curBlendMode)` after the `VIEWPORT` (`:1698`). `SetDrawLayer(l)` is `SetDrawLayer(l, 0)` (`r_main.h:85`). `SetDrawLayer(nil, s)` → `SetDrawSubLayer` (`:1701-1704`). `SetDrawLayer(nil)` → "SetDrawLayer(): must provide subLayer if layer is nil" (`ui_api.cpp:771`). Layer/sub are `(int)lua_tointeger` (truncation; the oracle bridge converts). Port: `api.rs` `set_draw_layer`; tests `layer_switch_reemits_viewport`, `nil_layer`. |
| `GetDrawLayer` | Holds | `ui_api.cpp:783-788`, `r_main.cpp:1706-1709` | Test `get_draw_layer_is_sub`. |
| Frame start | Holds, plus BLEND | `r_main.cpp:1199-1203` | `curLayer = layerList[0]; SetViewport(); SetBlendMode(RB_ALPHA); DrawColor();`. Queues were emptied by the previous `EndFrame` (`:1464-1466`). `renderEnable` is set after `BeginFrame` (`ui_main.cpp:401`) and cleared after `OnFrame` (`:421`). |
| Ordering | Holds | `r_main.cpp:1208-1224` (`layerCompFunc`), `:1298-1302` (`qsort`), `:702-704` (a queue replays in submission order), `:1436-1442` | Keys are unique, so `qsort`'s instability cannot show. Test `layer_sorting_is_stable`. |
| Viewport | Holds; clipping mechanism differs | `ui_api.cpp:790-812`, `r_main.cpp:1711-1723`; batching `:535-553`, shaders `:805-810`, `:832-838` | `x, y = lround(v*dpi)`, `w, h = ceil(v*dpi)` (double product), `height == 0` → full screen with `x, y` kept. At the pin, quads are **not** translated on the CPU and there is **no `glScissor`**: each vertex carries the viewport, the vertex shader adds `vp.xy`, and the fragment shader discards pixels outside `[x, x+w) × [y, y+h)`. For `w, h > 0` this equals the scissor of master. For a negative `w` or `h` (e.g. `SetViewport(0, 0, 10, -5)`), the discard test removes everything, whereas `glScissor` with a negative size is `GL_INVALID_VALUE`. U6 implements the discard semantics. Tests `viewport_height_zero`, `viewport_snapshot_and_scaling`, `rounding_at_dpi`. |
| Consequence (snapshot model) | Holds | — | `trace::normalize` is that snapshot model; `normalize_agrees_with_sg_queues` cross-checks it against `Sg`'s queues. |
| DrawImage | Holds except argument counts | `ui_api.cpp:889-1007`, `r_main.cpp:1760-1791` | Argument count is **5, 6, 7, 9, 10 or 11** (stack index at 6/10, **mask** at 7/11); otherwise the usage error. A nil image binds `@white` (`r_texture.cpp:21-30`, 8×8 grey `0xFF`). An image handle without `hnd` raises "DrawImage(): image handle has no image loaded" (`:931`). Positions `(float)val * dpiScale` in float (`:947`). Corners TL, TR, BR, BL; UVs `(l,t),(r,t),(r,b),(l,b)` (`r_main.cpp:1762-1771`). Every call appends BIND + COLOR(drawColor) + QUAD (`:1776-1790`). PoB never passes a stack index or mask (grep), so `Sg` has no such arguments (DIVERGENCE(stack-mask) in `api.rs`). Test `bind_color_quad_every_draw`, `unloaded_handle_is_a_lua_error`. |
| DrawImageQuad | Holds except argument counts | `ui_api.cpp:1009-1130` | Argument count **9, 10, 11, 17, 18 or 19**; default UVs `(0,0),(1,0),(1,1),(0,1)` (`:1084-1089`). Error text "DrawImageQuad(): image handle has no image loaded" (`:1050`). |
| SetDrawColor | Holds | `ui_api.cpp:826-874`, `common.cpp:201-274` | Components `(float)`; missing/nil alpha → 1 (`:851-861`); a string argument must start with an escape ("SetDrawColor() argument 1: invalid color escape sequence", `:837`), RGB from `ReadColorEscape`, alpha 1, trailing bytes ignored. `lastColor` is read back from `drawColor` (`:863-871`). Test `get_draw_color_after_draw_string`, `color::tests::palette_and_hex`. |
| GetDrawColor | Holds | `ui_api.cpp:876-887`, `:1168-1174`; `ui_main.h:55` | `lastColor` starts as white. `BeginFrame` resets `drawColor` but not `lastColor`. Test `get_draw_color_after_draw_string`. |
| Escape leak | Holds | `r_main.cpp:1793-1809`, `r_font.cpp:326-336, 410-417, 436-439` | `col = NULL` → the font mutates `drawColor`. `Draw` with a non-empty string first emits `COLOR(col)` (`r_font.cpp:442`). Test `escape_leak_and_alpha_one`. |
| Text height | Holds | `ui_api.cpp:1150-1157, 1190-1197, 1222-1228` | Test `font_height_rounding`. |
| Atlas choice | Holds (file identical) | `r_font.cpp:57-117, 288-319` | U2. |
| Glyph advance | Holds (file identical) | `r_font.cpp:143-203, 371-432` | U2. Note for U2: SG binds the atlas (`curTex`, per `DrawTextLine` call) **before** the visibility test of each glyph (`:375-378`), so a culled glyph can still emit a BIND; `font/layout.rs` binds only for visible glyphs and keeps `bound` across lines. Invisible in pixels; visible in a `Frame` command dump. |
| Alignment | Holds | `r_font.cpp:346-369`, `ui_api.cpp:1145, 1161`, `r_main.cpp:1796-1798` | |
| Line visibility | Holds | `r_font.cpp:321-338` | `vid.size[1]` (window height). |
| Non-ASCII | Holds | `r_font.cpp:130-161, 288-302, 398-408`, `r_main.cpp:1845-1860` | Empty string: `DrawStringWidth` returns 0 (`:1835-1837`), `DrawStringCursorIndex` returns 0 → Lua 1 (`:1847-1849`). |
| Lines | Holds (file identical) | `r_font.cpp:263-286, 434-457` | |
| Screen / DPI | Holds | `ui_api.cpp:722-735, 1179-1204, 1534-1541`, `r_main.cpp:1866-1899`, `Launch.lua:69` | `GetCursorPos` = `lround(VirtualMap(cursor) / dpiScale)`; `VirtualMap` is the identity under `DPI_AWARE`. The cursor is `floor` of GLFW's position (`sys_video.cpp:735-742`), snapshotted after `BeginFrame` (`ui_main.cpp:398`). Tests `rounding_at_dpi`, `sys::tests::cursor_rounding_at_dpi`. |
| Textures | Holds | `ui_api.cpp:394-432`, `r_texture.cpp:636-673` | Unknown flag: `LExpect` → Lua error "imgHandle:Load(): unrecognised flag '…'" (`:426`). Non-string flag arguments are skipped (`:409-411`). U3. |
| Missing file | Holds | `r_texture.cpp:552-608` | U3. |
| ImageSize | Holds | `r_main.cpp:1638-1652`, `ui_api.cpp:452-482` | U3. |
| Unuploaded texture | Holds | `r_texture.cpp:340-347` | U3/U6. |
| Shader / blend | Holds in effect; source differs | `r_main.cpp:784-843` (inline GLSL), `:602-617` (blend switch), `:1166-1197` (RT) | Shaders are compiled from strings in `r_main.cpp`, not `assets/gles/*`. `RB_ALPHA` = `SRC_ALPHA, ONE_MINUS_SRC_ALPHA` (the only mode PoB gets). The fragment shader also discards outside the viewport (see Viewport). The RT is `GL_RGB` at virtual size; its blit filter is `NEAREST` when `fmod(dpiScale, 1) < 0.0005`, else `LINEAR` (`:1170-1183`). U6. |
| Wheel | Holds | `sys_video.cpp:607-620` | U7. |
| Double click | Holds | `sys_video.cpp:569-604`, `ui_main.cpp:526-528` | U7. |
| Extra OnChar | Holds | `sys_video.cpp:528-542`, `sys_main.cpp:343-357` | U7. |
| Key names | **Incomplete in §2** | `ui_main.cpp:16-61, 548-572`, `keylist.h:7-51` | `CallKeyHandler` sends `"%c"` for `key < 128`, else `NameForKey(key)`, which looks the number up in `ui_keyNameMap`. `KE_CHAR` passes the **codepoint** through the same function (`ui_main.cpp:523-525`), and key codes start at `KEY_LMOUSE = 128`, so `OnChar` of U+0080..U+00AA delivers a key **name**: U+0080 → `"LEFTBUTTON"`, U+00A0 (NBSP) → `"F7"`, U+00A3 `£` → `"F10"`, U+00A7 `§` → `"F14"`, U+00AA → `"SCROLLLOCK"`. Only codepoints `>= 171` are `"?"`. Port: `Key::from_sg_code` / `Key::sg_code` (`types.rs`), test `types::tests::sg_codes`; U7's host must map `OnChar` through it. |
| Swallowed keys | **Incomplete in §2** | `ui_main.cpp:516-546`, `ui_console.cpp:99-111` | As §2, plus: `ui_main_c::KeyEvent` first offers every event to the console. `KeyDown` of `` ` `` or `F1` with Ctrl held toggles SG's console and is **not** delivered; while the console is open (or opening) it consumes **all** key and char events. PoB never opens it otherwise. U7. |
| Frame loop | Holds; digest differs | `ui_main.cpp:371-446`, `r_main.cpp:1150-1159, 1385-1462` | After `OnFrame`, SG switches to layer 10000 and renders its console (`ui_main.cpp:426-427`): invisible while closed, not emulated (DIVERGENCE(console) in `api.rs`). The elision digest is MurmurHash64A of the raw command bytes per sorted layer; `inhibitElision` is set when any registered texture is not `DONE` (`:1150-1159`). Elision is invisible. |
| Unused by PoB | Holds | grep of `.PathOfBuilding/src` | `SetBlendMode` exists at the pin (`ui_api.cpp:814-824`, registered at `:2271`) and is listed in `_SimpleGraphic.def.lua:173`; PoB never calls it. |

## Further facts found while porting (not §2 rows)

| Fact | Source | Port |
|---|---|---|
| Each layer stores at most `1 << 23` bytes of commands per frame; `NewCommand` silently drops a command when `cursor + size >= 8388608`. Sizes (x64, `enum` is `int`): VIEWPORT 20, BLEND 8, BIND 16, COLOR 20, QUAD 76. One draw is 112 bytes, so a layer holds about 74,900 draws per frame. | `r_main.cpp:128-192, 172, 218-228` | `api.rs` `new_command`/`push_cmd`/`push_blend`, test `layer_command_storage_cap`. |
| The draw cull is inclusive and in screen space: the quad's AABB plus `vp.xy` must satisfy `lo <= vp.hi && hi >= vp.lo` (master: strict, in viewport space). | `r_main.cpp:282-319, 501-509` | `frame.rs` `Quad::is_culled`, test `cull_is_inclusive_at_3b1a346`. |
| SG receives every string argument as a C string: DrawString / DrawStringWidth / DrawStringCursorIndex / StripEscapes / SetDrawColor(escape) / IsKeyDown ignore everything after the first NUL byte. | `ui_api.cpp:1165, 1201, 1234, 1265`, `r_main.cpp:1850` | `api.rs` `c_str`, `trace.rs` `c_str`, `Sys::is_key_down_name`; test `strings_stop_at_nul`, `is_key_down_name_errors`. |
| `IsKeyDown("")` → "IsKeyDown() argument 1: string is empty"; an unknown name → "IsKeyDown(): unrecognised key name". | `ui_api.cpp:1567-1580` | `Sys::is_key_down_name`. |
| Error texts: "SetDrawColor() argument 1: invalid color escape sequence", "<Func>() called outside of OnFrame" (SetDrawLayer, SetViewport, SetDrawColor, DrawImage, DrawImageQuad, DrawString). `SetBlendMode` reuses "SetViewport() called outside of OnFrame". | `ui_api.cpp:763, 794, 818, 830, 837, 893, 1013, 1136` | `api.rs`; test `render_enable`. |
| `l_imgHandleLoad` deletes the old `hnd` before validating the flags and does not clear it, so a `pcall`-caught flag error leaves a dangling handle. PoB never passes a bad flag. | `ui_api.cpp:406-427` | U3 (do not reproduce a use-after-free). |

## Trace normalisation (`trace.rs`)

`normalize` is the snapshot model above, in Lua units. The viewport and the draw colour are
global, so each op carries the values current at its call. `SetViewport()` and a height with
`ceil(h * dpi) == 0` become `[x, y, fb_w / dpi, fb_h / dpi]` at the frame's OS scale. After a
`DrawString`, the colour is that of the last escape in the text (up to the first NUL), with
alpha 1. Calls that SG rejects (`SetDrawLayer(nil)`, an invalid escape string) change nothing.

`diff` compares buckets in `(layer, sub)` order. Multiset ranges (inclusive, lexicographic) are
matched order-insensitively. All other buckets are aligned by an LCS after trimming the common
prefix and suffix; above 4 M cells it falls back to positional pairing. Unmatched runs are
reported as `Changed` pairs, and the rest as `Missing`/`Extra`. f64 arguments and viewports are
compared within `abs_eps`, colours bit-exactly, and strings, images, fonts and alignments
exactly. A `TextMask` skips the text of a `DrawString` pair when the Lua op's position is in the
mask's rectangle and both texts match the Lua pattern. `diff_effects` compares side effects
position by position, and `diff_frames` does both for one frame.
