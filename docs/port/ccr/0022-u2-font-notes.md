# CCR-0022: ui doc §2/§4.4 font notes from U2's verification at SimpleGraphic `3b1a346`

- **Unit:** U2 (sg-font)
- **Affects:** `docs/port/ui-architecture.md` §2 (rows Glyph advance, Escape leak), §4.4 (the
  `draw` bullet list and the surface listing). Consumers: U1 (`Sg::draw_string*` callers, no
  change needed), U8 (oracle bridge, may use the byte-string entry points), U6/U53 (the probe).
- **Evidence:** PathOfBuilding-SimpleGraphic @ `3b1a3468223d0ebd4042d6ce76fc6144718ef79b`:
  `engine/render/r_font.cpp`, `r_font.h`, `engine/common/common.cpp`, `common.h` are
  byte-identical to the research copy (master @ 8694a07; CCR-0021 finding 1 agrees).
  Line numbers below are at `3b1a346`.
- **No frozen signature changes.** The frozen `FontSet`/`FontFace` methods keep their
  signatures; everything below is either a doc precision or an additive `pub` item in U2's
  own files.

## Findings

1. **BIND placement (behaviour; the moved pre-port code was wrong).** `drawCodepoint` binds at
   its start, before the glyph lookup and independently of the glyph's width and of the x cull,
   whenever the atlas differs from `curTex` (`r_font.cpp:373-378`). `curTex` is a local of
   `DrawTextLine` (`:371`), so **every drawn line starts with a BIND** of its first glyph's
   atlas, and a line whose glyphs are all culled still emits its BIND(s). Tabs and escapes do
   not bind. The moved code bound only before an emitted quad and kept the atlas across lines:
   134 of the 2400 T2 golden cases differed. Fixed in `font/layout.rs`.
2. **NUL and invalid UTF-8.** `r_renderer_c::DrawString*` take `const char*`
   (`r_main.cpp:1793-1860`), so text ends at the first NUL (CCR-0021 finding 9: U1's `Sg` cuts
   there too; the face entry points cut again, which is idempotent). `IndexUTF8ToUTF32`
   (`common.cpp:440-500`) decodes overlong forms and surrogates by bit pattern and turns every
   byte that does not start a complete sequence into U+FFFD, advancing one byte; each U+FFFD
   is then tofu `[U+FFFD]`. A Rust `&str` cannot carry such bytes (ui doc §8 rule 4 accepts
   that divergence), so U2 adds byte-string twins for the oracle bridge.
3. **Cursor index past the end.** After a trailing `'\n'`, `StringCursorIndex` returns
   `len + lastIndex` of the previous line (`r_font.cpp:271-285`); `DrawStringCursorIndex` maps any
   index `>= offsets.size()` to the byte length (`r_main.cpp:1856-1859`). The port reproduces
   both (hand cases in `crates/pob-sg/tests/font_cursor.rs`).
4. **Non-positive heights** (face level only; `Sg` never passes them): height 0 uses atlas
   index 0 with scale 0; a negative height uses index 0 with a negative scale, so line widths
   go negative and `StringWidth` returns its initial 0 (`r_font.cpp:184-203, 304-319`).
5. **`.tgf` parsing is `sscanf`.** `"HEIGHT %u;"` / `"GLYPH %u %u %u %d %d;"`
   (`r_font.cpp:73, 85`): the keyword must start the line, a space in the format matches zero
   or more whitespace, and text after the last conversion is ignored. The pre-port token
   parser accepted leading whitespace and rejected trailing text. No shipped file is affected.

## Additive `pub` items (U2 files only)

`font::IndexedUtf32`, `font::index_utf8_to_utf32(&[u8])` (`index_utf8(&str)` kept);
`FontFace::{string_width_bytes, cursor_index_bytes, draw_bytes}` (Lua byte strings);
`FontFace::{string_width_u32, string_cursor_index_u32, draw_u32}` (`r_font_c`'s codepoint
level); `font::layout::{is_color_escape_u32, read_color_escape_u32}` (`common.cpp:220-299`);
`font::tgf::parse` now takes `&[u8]` (only caller: `FontFace::load`).

## Proposed diff to `docs/port/ui-architecture.md`

```diff
@@ §2 Glyph advance row
-| Glyph advance | Draw: `x += spL*scale`; if `width > 0`, emit a quad ...
+| Glyph advance | Draw: BIND the glyph's atlas if it differs from the line's current one (before culling; the current atlas resets for every line), `x += spL*scale`; if `width > 0`, emit a quad ...
@@ §4.4 draw bullets
-- `bind` whenever the atlas changes;
+- `bind` at the start of every glyph (tofu glyphs included) whose atlas differs from the line's
+  current atlas, before the glyph lookup and the x cull; the current atlas is per line, so each
+  drawn line begins with a BIND (`r_font.cpp:371-378`);
@@ §4.4 after the code block
+Byte-string twins (`string_width_bytes`, `cursor_index_bytes`, `draw_bytes`) take Lua strings
+as SG does (`const char*`: cut at NUL; `IndexUTF8ToUTF32`: invalid bytes → U+FFFD). The `&str`
+methods delegate to them. (CCR-0022)
```
