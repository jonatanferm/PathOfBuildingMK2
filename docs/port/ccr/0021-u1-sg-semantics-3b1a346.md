# CCR-0021: ui doc §2 corrections from U1's re-verification at SimpleGraphic `3b1a346`

**Status: applied 2026-11-10 by CCR2 (ui doc §2, §13 item 1). See AMENDMENTS A8.**

- **Unit:** U1 (sg-core)
- **Affects:** `docs/port/ui-architecture.md` §2 (rows Viewport, DrawImage, DrawImageQuad, Key
  names, Swallowed keys, Shader / blend, Frame loop, and the source paragraph), §13 item 1; the
  units that implement those rows: U6 (backend), U7 (host input), U8 (oracle argument dispatch).
- **Evidence:** `docs/port/ui/sg-semantics.md` (row-by-row table with `3b1a346` line numbers).
  SimpleGraphic sources fetched from `PathOfBuilding-SimpleGraphic@3b1a3468223d0ebd4042d6ce76fc6144718ef79b`.
- **No frozen signature changes.** U1's code changes are inside its own files: `frame.rs`
  (`Quad::is_culled` semantics), `api.rs` (storage cap, NUL cut, error texts), and additive
  helpers `Key::from_sg_code`/`Key::sg_code` (`types.rs`), `Sys::is_key_down_name` (`sys.rs`),
  `trace::{diff_effects, diff_frames}` and `Debug`/`Clone` derives on `DiffConfig`, `TextMask`
  and `Divergence` (`trace.rs`).

## Findings

1. **The research copy is not older than the pin.** §2 says its line numbers refer to "the
   research copy, which is master", implying the pin is close. In fact `r_main.cpp` differs in
   about 760 diff lines. `r_font.cpp`, `common.cpp` and `common.h` are identical. The
   `3b1a346` line numbers for every row are in `sg-semantics.md`.

2. **Viewport clipping (U6).** At `3b1a346` there is no `glScissor`. Each vertex carries the
   viewport (`r_main.cpp:535-553`), the vertex shader adds `vp.xy` (`:805-810`) and the fragment
   shader discards outside `[x, x+w) × [y, y+h)` (`:832-838`). This is the same as a scissor
   for positive sizes. For a negative width or height, everything is discarded, whereas
   `glScissor` would reject the call.

3. **Cull (U6, frame.rs).** `AabbAabbIntersects` is inclusive and works in screen space
   (`r_main.cpp:282-319, 501-509`). Master's test is strict and in viewport space.
   `Quad::is_culled` now follows the pin. This changes no pixels.

4. **DrawImage / DrawImageQuad argument counts (U8 bridge).** `3b1a346` accepts DrawImage
   with **5, 6, 7, 9, 10, 11** arguments and DrawImageQuad with **9, 10, 11, 17, 18, 19**
   (`ui_api.cpp:907-925`, `:1026-1044`). 7/11 and 11/19 add a `mask` stack index (nil
   allowed). §2 lists only 5/6/9/10 and 9/10/17/18. PoB passes none of the extra arguments.

5. **BLEND command.** `SetDrawLayer` and `BeginFrame` also queue `BLEND(curBlendMode)`
   (`r_main.cpp:1698`, `:1202`). PoB never calls `SetBlendMode`, so the mode is always
   `RB_ALPHA`. `Cmd` stays without a `Blend` variant, and `Sg` counts the bytes only.

6. **OnChar of U+0080..U+00AA delivers key names (U7).** `KE_CHAR` passes the codepoint to
   `CallKeyHandler`, and `NameForKey` matches it against `keyList_e`
   (`ui_main.cpp:523-525, 555-572`, `keylist.h:7-51`, `KEY_LMOUSE = 128`). So `£` (U+00A3)
   arrives as `"F10"`, `§` (U+00A7) as `"F14"` and NBSP (U+00A0) as `"F7"`. Only
   codepoints `>= 171` are `"?"`. EditControl's filters then see e.g. `"F10"` as a typed
   string. U7 should map `OnChar` through `pob_sg::Key::from_sg_code`.

7. **Console swallows Ctrl+` and Ctrl+F1 (U7).** `ui_main_c::KeyEvent` gives the console the
   first look at every event (`ui_main.cpp:518-520`). `KeyDown` of `` ` `` or `F1` with Ctrl
   held toggles it and is not delivered to Lua (`ui_console.cpp:99-104`). While the console
   is open, it takes every event (`:106-109`).

8. **Per-layer command storage cap.** Each layer holds `1 << 23` bytes of commands per frame.
   A command that does not fit is silently dropped (`r_main.cpp:172, 218-228`), which is about
   74,900 draws in one layer. `Sg` reproduces this.

9. **C strings.** SG reads every string argument up to its first NUL byte (`lua_tostring` →
   `const char*`). `Sg` reproduces this for DrawString, DrawStringWidth,
   DrawStringCursorIndex, StripEscapes and SetDrawColor(escape), and `Sys::is_key_down_name`
   does the same for IsKeyDown.

10. **Shaders and frame elision.** The shaders are inline strings in `r_main.cpp:784-843`, not
    `assets/gles/*`. The elision digest is MurmurHash64A of the raw command bytes
    (`:1385-1403`). The RT blit filter is `NEAREST` for integer DPI scales and `LINEAR`
    otherwise (`:1170-1183`).

## Proposed diff to `docs/port/ui-architecture.md`

```diff
@@ §2 source paragraph
-Line numbers refer to the research copy, which is master. U1 pins commit `3b1a346`, re-verifies every row there, and records any difference in `docs/port/ui/sg-semantics.md`.
+Line numbers refer to the research copy (master @ 8694a07). U1 re-verified every row at the pinned commit `3b1a346`; `docs/port/ui/sg-semantics.md` gives the pinned line numbers and the differences (CCR-0021). Where that file and this table disagree, the file wins.
@@ Viewport row
-... At batch time quads are translated by `vp.lo` and scissored to the viewport. | `ui_api.cpp:793-815`, `r_main.cpp:1491-1501`, `:438-447`, `:500-505` |
+... At batch time each vertex carries the viewport; the shader translates by `vp.xy` and discards fragments outside `[x, x+w) × [y, y+h)` (no `glScissor`; a negative size discards everything). Culling is inclusive in screen space. | `ui_api.cpp:790-812`, `r_main.cpp:1711-1723`, `:282-319`, `:501-553`, `:805-838` @3b1a346 |
@@ DrawImage row
-| DrawImage | Argument count must be 5, 6, 9 or 10. ...
+| DrawImage | Argument count must be 5, 6, 7, 9, 10 or 11 (stack index at 6/10, mask at 7/11; PoB uses neither). ...
@@ DrawImageQuad row
-| DrawImageQuad | Argument count 9, 10, 17 or 18. ...
+| DrawImageQuad | Argument count 9, 10, 11, 17, 18 or 19. ...
@@ Key names row
-... Anything else is `"?"`, so a non-ASCII OnChar delivers `"?"`. | `ui_main.cpp:16-61, 547-572` |
+... `NameForKey` looks the number up by value, and OnChar passes the codepoint, so OnChar of U+0080..U+00AA delivers a key name (`£` → `"F10"`, `§` → `"F14"`, NBSP → `"F7"`); codepoints `>= 171` deliver `"?"` (`pob_sg::Key::from_sg_code`). | `ui_main.cpp:16-61, 523-525, 548-572`, `keylist.h:7-51` |
@@ Swallowed keys row
-| Swallowed keys | `KeyUp(F10)` (debug UI toggle) and `KeyUp(PAUSE)` with Shift held are **not** delivered to Lua. | `ui_main.cpp:530-541` |
+| Swallowed keys | `KeyUp(F10)` (debug UI toggle) and `KeyUp(PAUSE)` with Shift held are **not** delivered to Lua. `KeyDown` of `` ` `` or `F1` with Ctrl held toggles SG's console and is not delivered; while the console is open it consumes all input. | `ui_main.cpp:516-546`, `ui_console.cpp:99-111` |
@@ Shader / blend row
-| Shader / blend | `color = texture(uv) * tint`. Blend is `SRC_ALPHA, ONE_MINUS_SRC_ALPHA`. The scene goes into an RGB render target at framebuffer size, then a full-screen blit. | `assets/gles/tinted_texture.*`, `r_main.cpp:483, 729, 982-1020` |
+| Shader / blend | `color = texture(uv) * tint`, fragments outside the viewport discarded. Blend is `SRC_ALPHA, ONE_MINUS_SRC_ALPHA` (`RB_ALPHA`; a BLEND command is queued per layer switch, PoB never changes it). The scene goes into an RGB render target at framebuffer size, then a full-screen blit (NEAREST for integer DPI scales, else LINEAR). | `r_main.cpp:602-617, 784-843, 1166-1203` @3b1a346 |
@@ Frame loop row (append)
+Each layer holds at most `1 << 23` bytes of commands per frame (a draw is 112 bytes); further commands are dropped (`r_main.cpp:172, 218-228`). The elision digest is MurmurHash64A (`:1385-1403`).
@@ §13 item 1
-1. **SG master vs the shipped DLL.** Research used master. The DLL is `v2.5-3b1a346`. U1 re-verifies every §2 row at `3b1a346`; T6 (Wine) confirms metrics and pixels once Wine is installed.
+1. **SG master vs the shipped DLL.** Done by U1 (CCR-0021, `docs/port/ui/sg-semantics.md`). T6 (Wine) still confirms metrics and pixels once Wine is installed.
```

## Work-arounds in U1's code

None needed: every change above is either inside U1's files, or a note for U6/U7/U8 that does not
alter a frozen signature.
