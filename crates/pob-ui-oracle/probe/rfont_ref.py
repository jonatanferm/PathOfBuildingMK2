#!/usr/bin/env python3
"""Independent reference transcription of SimpleGraphic `engine/render/r_font.cpp` (U2, ui doc T2).

PathOfBuilding-SimpleGraphic @ 3b1a346 (the commit the shipped `SimpleGraphic.dll` names,
`SimpleGraphic v2.5-3b1a346`). Written from the C++ only, without looking at the Rust port, so
that `crates/pob-sg/tests/font_goldens.rs` is a differential test between two transcriptions
rather than a self-generated snapshot. Float arithmetic is emulated in IEEE single precision
(every `float` operation of the C++ is rounded to f32 with `f32()`; f64 intermediates are exact
for one +,-,*,/ of two f32 values, so the rounding is the correctly rounded f32 result).

Sources (line numbers at 3b1a346):
  r_font.cpp:50-115   constructor: .tgf parse (sscanf), glyph texcoords, fontHeightMap
  r_font.cpp:131-141  BuildTofuString, tofuSizeReduction
  r_font.cpp:143-203  StringWidthInternal / StringWidth
  r_font.cpp:205-286  StringCursorInternal / StringCursorIndex
  r_font.cpp:288-319  FindSmallerFontHeight / FindFontHeight
  r_font.cpp:321-457  DrawTextLine / Draw
  r_main.cpp:1793-1860 DrawString / DrawStringWidth / DrawStringCursorIndex (font index, offsets)
  common.cpp:220-299  IsColorEscape(u32string_view) / ReadColorEscape(u32string_view)
  common.cpp:440-500  IndexUTF8ToUTF32
  common.h:108-119    colour constants (colorEscape table)

Usage:
  rfont_ref.py --fonts DIR goldens OUT.json      write the T2 goldens (and nothing else)
  rfont_ref.py --fonts DIR cases-lua OUT.lua     write the Wine probe's case table
  rfont_ref.py --fonts DIR width H FONT TEXT      one StringWidth (TEXT is a Python literal)
  rfont_ref.py --fonts DIR cursor H FONT TEXT X Y one StringCursorIndex (byte offset, 0-based)
  rfont_ref.py --fonts DIR draw H FONT TEXT X Y ALIGN SCREEN_W WIN_H
                                                 the BIND/COLOR/QUAD stream of one Draw
"""

import ast
import json
import math
import os
import re
import struct
import sys

# ---------------------------------------------------------------- f32 helpers


def f32(x):
    return struct.unpack("<f", struct.pack("<f", x))[0]


def f32_bits(x):
    return struct.unpack("<I", struct.pack("<f", x))[0]


def ceilf(x):
    return f32(float(math.ceil(x)))


def floorf(x):
    return f32(float(math.floor(x)))


def roundf(x):
    # std::round: half away from zero (exact in f64 for |x| < 2^28).
    return f32(math.copysign(math.floor(abs(x) + 0.5), x))


# ---------------------------------------------------------------- font loading

FONT_NAMES = [  # ui_api.cpp fontMap order == r_main.cpp fonts[] order
    ("FIXED", "Bitstream Vera Sans Mono"),
    ("VAR", "Liberation Sans"),
    ("VAR BOLD", "Liberation Sans Bold"),
    ("FONTIN SC", "Fontin SmallCaps"),
    ("FONTIN SC ITALIC", "Fontin SmallCaps Italic"),
    ("FONTIN", "Fontin"),
    ("FONTIN ITALIC", "Fontin Italic"),
]

RE_HEIGHT = re.compile(r"HEIGHT\s*(\d+)")
RE_GLYPH = re.compile(r"GLYPH\s*(\d+)\s*(\d+)\s*(\d+)\s*(-?\d+)\s*(-?\d+)")


class FontHeight:
    def __init__(self, height, tex_w, tex_h):
        self.height = height
        self.tex_w = tex_w
        self.tex_h = tex_h
        self.glyphs = []  # (tcL, tcR, tcT, tcB, width, spL, spR)

    def glyph(self, cp):
        # f_fontHeight_s::Glyph((char)(unsigned char)cp)
        b = cp & 0xFF
        if b >= len(self.glyphs):
            return (0.0, 0.0, 0.0, 0.0, 0, 0, 0)
        return self.glyphs[b]


class Font:
    def __init__(self, fonts_dir, index):
        self.index = index
        stem = FONT_NAMES[index][1]
        self.heights = []
        self.max_height = 0
        with open(os.path.join(fonts_dir, stem + ".tgf"), "rb") as f:
            lines = f.read().decode("latin-1").split("\n")
        fh = None
        for line in lines:
            m = RE_HEIGHT.match(line)
            if m:
                h = int(m.group(1))
                with open(os.path.join(fonts_dir, "%s.%d.tga" % (stem, h)), "rb") as t:
                    hdr = t.read(18)
                fh = FontHeight(h, int.from_bytes(hdr[12:14], "little"), int.from_bytes(hdr[14:16], "little"))
                self.heights.append(fh)
                self.max_height = max(self.max_height, h)
                continue
            m = RE_GLYPH.match(line)
            if fh is not None and m:
                if len(fh.glyphs) >= 128:
                    continue
                x, y, w, sl, sr = (int(g) for g in m.groups())
                fh.glyphs.append((
                    f32(f32(x) / fh.tex_w),
                    f32(f32(x + w) / fh.tex_w),
                    f32(f32(y) / fh.tex_h),
                    f32(f32(y + fh.height) / fh.tex_h),
                    w, sl, sr,
                ))
        self.height_map = [0] * (self.max_height + 1)
        for i, fhi in enumerate(self.heights):
            gh = fhi.height
            for h in range(gh, self.max_height + 1):
                self.height_map[h] = i
            if i > 0:
                below = self.heights[i - 1].height
                lim = (gh - below - 1) // 2  # both operands >= 0 in the shipped data
                for b in range(lim):
                    self.height_map[gh - b - 1] = i

    # r_font.cpp:304-319
    def find_font_height(self, height):
        if height > self.max_height:
            idx = len(self.heights) - 1
        elif height < 0:
            idx = 0
        else:
            idx = self.height_map[height]
        return idx

    # r_font.cpp:288-302
    def find_smaller(self, height, idx, red):
        ret = (idx, 0)
        for t in range(idx - 1, -1, -1):
            diff = height - self.heights[t].height
            if diff >= red:
                ret = (t, int(math.ceil(f32(diff / 2.0))))
                break
        return ret

    @staticmethod
    def measure(fh, cp):
        g = fh.glyph(cp)
        return g[4] + g[5] + g[6]

    # r_font.cpp:143-182
    def width_internal(self, idx, s, height, scale):
        fh = self.heights[idx]
        tofu_idx, _ = self.find_smaller(height, idx, 3)
        width = 0.0
        i = 0
        while i < len(s):
            ch = s[i]
            esc = is_color_escape(s[i:])
            if esc:
                i += esc
            elif ch >= len(fh.glyphs):
                for cp in tofu(ch):
                    width = f32(width + self.measure(self.heights[tofu_idx], cp))
                    width = ceilf(width)
                i += 1
            elif ch == 9:
                g = fh.glyph(32)
                sp = g[4] + g[5] + g[6]
                width = f32(width + f32((sp * 4) * scale))
                width = ceilf(width)
                i += 1
            else:
                width = f32(width + f32(self.measure(fh, ch) * scale))
                width = ceilf(width)
                i += 1
        return int(width)

    # r_font.cpp:184-203
    def string_width(self, height, s):
        idx = self.find_font_height(height)
        scale = f32(float(height) / self.heights[idx].height)
        mx = 0
        for line in split_lines(s):
            if line:
                mx = max(mx, self.width_internal(idx, line, height, scale))
        return mx

    # r_font.cpp:205-261
    def cursor_internal(self, idx, s, height, scale, cur_x):
        fh = self.heights[idx]
        tofu_idx, _ = self.find_smaller(height, idx, 3)
        x = 0.0
        i = 0
        cx = f32(float(cur_x))
        while i < len(s):
            esc = is_color_escape(s[i:])
            if esc:
                i += esc
            elif s[i] >= len(fh.glyphs):
                for cp in tofu(s[i]):
                    x = f32(x + self.measure(self.heights[tofu_idx], cp))
                    x = ceilf(x)
                    if cx <= x:
                        return i
                i += 1
            elif s[i] == 9:
                g = fh.glyph(32)
                full = f32(f32(f32(g[4] + g[5] + g[6]) * 4.0) * scale)
                half = ceilf(f32(full / 2.0))
                x = f32(x + half)
                x = ceilf(x)
                if cx <= x:
                    break
                x = f32(x + f32(full - half))
                x = ceilf(x)
                if cx <= x:
                    break
                i += 1
            else:
                x = f32(x + f32(self.measure(fh, s[i]) * scale))
                x = ceilf(x)
                if cx <= x:
                    break
                i += 1
        return i

    # r_font.cpp:263-286: codepoint index
    def cursor_index(self, height, s, cur_x, cur_y):
        idx = self.find_font_height(height)
        last = 0
        line_y = height
        scale = f32(float(height) / self.heights[idx].height)
        i = 0
        while i != len(s):
            try:
                end = s.index(10, i)
            except ValueError:
                end = len(s)
            last = self.cursor_internal(idx, s[i:end], height, scale, cur_x)
            if cur_y <= line_y:
                break
            if end == len(s):
                break
            i = end + 1
            line_y += height
        return i + last

    # r_font.cpp:321-432
    def draw_text_line(self, out, pos, align, height, col, s, screen_w, win_h):
        if pos[1] >= win_h or pos[1] <= -height:
            while s:
                esc = is_color_escape(s)
                if esc:
                    s = read_color_escape(s, col)
                    col[3] = 1.0
                    out.append(("C", tuple(col)))
                    continue
                s = s[1:]
            return
        idx = self.find_font_height(height)
        fh = self.heights[idx]
        scale = f32(float(height) / fh.height)
        tofu_idx, tofu_pad = self.find_smaller(height, idx, 3)
        x = pos[0]
        y = floorf(pos[1])
        vsw = f32(float(screen_w))
        if align != "LEFT":
            width = f32(float(self.width_internal(idx, s, height, scale)))
            if align == "CENTER":
                x = floorf(f32(f32(f32(vsw - width) / 2.0) + pos[0]))
            elif align == "RIGHT":
                x = floorf(f32(f32(vsw - width) - pos[0]))
            elif align == "CENTER_X":
                x = floorf(f32(pos[0] - f32(width / 2.0)))
            elif align == "RIGHT_X":
                x = floorf(f32(pos[0] - width))
        x = roundf(x)
        state = {"tex": None, "x": x}

        def draw_cp(fhi, h, sc, yshift, cp):
            fhc = self.heights[fhi]
            cp_y = f32(y + yshift)
            if state["tex"] != fhi:
                state["tex"] = fhi
                out.append(("B", (self.index, fhc.height)))
            g = fhc.glyph(cp)
            xx = f32(state["x"] + f32(g[5] * sc))
            if g[4]:
                w = f32(g[4] * sc)
                if f32(xx + w) >= 0 and xx < vsw:
                    out.append(("Q", (
                        g[0], g[1], g[1], g[0],
                        g[2], g[2], g[3], g[3],
                        xx, f32(xx + w), f32(xx + w), xx,
                        cp_y, cp_y, f32(cp_y + h), f32(cp_y + h),
                    ), cp))
                xx = f32(xx + w)
            xx = f32(xx + f32(g[6] * sc))
            state["x"] = ceilf(xx)

        while s:
            ch = s[0]
            if ch >= len(fh.glyphs):
                for c in tofu(ch):
                    draw_cp(tofu_idx, self.heights[tofu_idx].height, 1.0, tofu_pad, c)
                s = s[1:]
                continue
            esc = is_color_escape(s)
            if esc:
                s = read_color_escape(s, col)
                col[3] = 1.0
                out.append(("C", tuple(col)))
                continue
            if ch == 9:
                g = fh.glyph(32)
                sp = g[4] + g[5] + g[6]
                state["x"] = f32(state["x"] + f32((sp << 2) * scale))
                s = s[1:]
                continue
            draw_cp(idx, height, scale, 0, ch)
            s = s[1:]

    # r_font.cpp:434-457
    def draw(self, pos, align, height, col, s, screen_w, win_h):
        out = []
        if not s:
            return out
        out.append(("C", tuple(col)))
        y = pos[1]
        for line in split_lines(s):
            if line:
                self.draw_text_line(out, [pos[0], y], align, height, col, line, screen_w, win_h)
            y = f32(y + height)
        return out


def split_lines(s):
    # Draw / StringWidth: the pieces between '\n' (a trailing '\n' adds nothing drawable).
    lines = [[]]
    for c in s:
        if c == 10:
            lines.append([])
        else:
            lines[-1].append(c)
    return lines


# ---------------------------------------------------------------- common.cpp

PALETTE = [  # common.h colour constants, as float
    (0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0), (1.0, 1.0, 0.0),
    (1.0, 0.0, 1.0), (0.0, 1.0, 1.0), (1.0, 1.0, 1.0), (f32(0.7), f32(0.7), f32(0.7)),
    (f32(0.4), f32(0.4), f32(0.4)),
]


def is_hex(c):
    return 48 <= c <= 57 or 65 <= c <= 70 or 97 <= c <= 102


def is_color_escape(s):
    if len(s) < 2 or s[0] != 94:
        return 0
    d = s[1]
    if 48 <= d <= 57:
        return 2
    if len(s) >= 8 and d in (120, 88):
        for c in range(6):
            if not is_hex(s[c + 2]):
                return 0
        return 8
    return 0


def read_color_escape(s, out):
    n = is_color_escape(s)
    if n == 2:
        p = PALETTE[s[1] - 48]
        out[0], out[1], out[2] = p
    elif n == 8:
        txt = "".join(chr(c) for c in s[2:8])
        for k in range(3):
            out[k] = f32(int(txt[2 * k:2 * k + 2], 16) / 255.0)
    return s[n:]


def tofu(cp):
    return [ord(c) for c in "[U+%04X]" % cp]


def index_utf8(b):
    """IndexUTF8ToUTF32 (common.cpp:440-500): codepoints and source byte offsets."""
    cps, offs = [], []
    i = 0
    n = len(b)
    while i < n:
        left = n - i
        offs.append(i)
        c0 = b[i]
        if c0 >> 7 == 0:
            cp, k = c0, 1
        elif left >= 2 and c0 >> 5 == 0b110 and b[i + 1] >> 6 == 0b10:
            cp, k = (c0 & 0x1F) << 6 | (b[i + 1] & 0x3F), 2
        elif left >= 3 and c0 >> 4 == 0b1110 and b[i + 1] >> 6 == 0b10 and b[i + 2] >> 6 == 0b10:
            cp, k = (c0 & 0xF) << 12 | (b[i + 1] & 0x3F) << 6 | (b[i + 2] & 0x3F), 3
        elif (left >= 4 and c0 >> 3 == 0b11110 and b[i + 1] >> 6 == 0b10 and b[i + 2] >> 6 == 0b10
              and b[i + 3] >> 6 == 0b10):
            cp = (c0 & 0x7) << 18 | (b[i + 1] & 0x3F) << 12 | (b[i + 2] & 0x3F) << 6 | (b[i + 3] & 0x3F)
            k = 4
        else:
            cp, k = 0xFFFD, 1
        cps.append(cp)
        i += k
    return cps, offs


def c_str(b):
    """r_main.cpp receives `const char*` (lua_tostring): the text ends at the first NUL."""
    z = b.find(b"\0")
    return b if z < 0 else b[:z]


# r_main.cpp:1833-1860 (renderer wrappers)
def drawstring_width(font, height, b):
    b = c_str(b)
    if not b:
        return 0
    cps, _ = index_utf8(b)
    return font.string_width(height, cps)


def drawstring_cursor(font, height, b, x, y):
    """0-based byte offset (r_main.cpp:1845-1860, before ui_api's `+ 1`)."""
    b = c_str(b)
    if not b:
        return 0
    cps, offs = index_utf8(b)
    i = font.cursor_index(height, cps, x, y)
    return offs[i] if i < len(offs) else len(b)


def drawstring(font, pos, align, height, col, b, screen_w, win_h):
    cps, _ = index_utf8(c_str(b))
    col = list(col)
    out = font.draw([f32(pos[0]), f32(pos[1])], align, height, col, cps, screen_w, win_h)
    return out, col


# ---------------------------------------------------------------- canonical stream digest


def canon(out):
    parts = []
    for kind, v, *_ in out:
        if kind == "C":
            parts.append("C" + "".join("%08x" % f32_bits(c) for c in v) + ";")
        elif kind == "B":
            parts.append("B%d.%d;" % v)
        else:
            parts.append("Q" + "".join("%08x" % f32_bits(c) for c in v) + ";")
    return "".join(parts)


def fnv1a64(s):
    h = 0xCBF29CE484222325
    for c in s.encode("ascii"):
        h ^= c
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return "%016x" % h


# ---------------------------------------------------------------- the T2 golden case set

GOLDEN_FONTS = [1, 2, 0]  # VAR, VAR BOLD, FIXED (fontMap indices)
GOLDEN_HEIGHTS = [12, 16, 22, 30]  # 30 has no atlas: scale 30/28
# cursor points: (x, ky, ay) means curX = x, curY = ky*height + ay
GOLDEN_POINTS = [(-3, 0, 0), (0, 0, 0), (4, 0, 0), (13, 0, 5), (37, 0, 0), (90, 0, 0), (400, 0, 0),
                 (25, 1, 1), (60, 2, 1), (5000, 9, 0)]
ALIGNS = ["LEFT", "CENTER", "RIGHT", "CENTER_X", "RIGHT_X"]
DRAW_SCREEN_W, DRAW_WIN_H = 1280, 800
DRAW_COL = (0.5, 0.25, 0.75, 0.5)
DRAW_LEFT_POS = (7.4, 11.6)
DRAW_ALIGN_POS = (300.5, 40.5)

HAND_STRINGS = [
    "", "A", "Hello", "Hello World", "Path of Building", "The quick brown fox jumps over the lazy dog",
    "0123456789", "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~", "iiiiiiiiii", "WWWWWWWWWW", " ", "   ",
    "a b", "\t", "\tA", "a\tb", "a\t\tb", "\t\t\t", "a\nb", "\n", "a\n", "\nb", "\n\nb", "abc\n\ndefgh",
    "line one\nline two is longer\nthree", "^7", "^7Hello", "^1Red ^2Green ^3Blue", "^xFF0000red",
    "^x33fF77mixed", "^XAABBCCupper X", "^", "^^", "^^1x", "^x12345", "^x12345G", "^xGGGGGGg", "a^", "a^1",
    "^1\n^2b", "^8Gray^9Dark^0Black", "Life: ^x33FF77+120^7 to maximum Life",
    "^xE05030Fire ^x5080FFCold ^xD0D020Lightning", "é", "café", "naïve", "Ä", "日本", "😀", "a😀b",
    " ", "x\u0080y", "€100", "ö\tö", "é\né", "^1é^2", "\té", "Tabs\tand\nnewlines\tmixed",
    "+12% increased Attack Speed", "Adds 10 to 20 Physical Damage", "100% chance to Ignite",
    "{ }", "[U+00E9]", "_", "|||", "....", "mmmm", "Total DPS: 1,234,567.8", "Str: 100\tDex: 50",
    "\t^1x", "^1\t", "a\n\t", "  leading", "trailing  ", "MiXeD CaSe", "~`", "\\", "%d %s",
]


def lcg_strings(n, seed=12345):
    alphabet = (
        [chr(c) for c in range(32, 127)] * 3
        + ["\t", "\n", "^1", "^7", "^x3366FF", "é", "ü", "日", "😀", "^", "^x"]
    )
    out = []
    state = seed
    for _ in range(n):
        state = (state * 6364136223846793005 + 1442695040888963407) & 0xFFFFFFFFFFFFFFFF
        length = 1 + (state >> 33) % 40
        s = []
        for _ in range(length):
            state = (state * 6364136223846793005 + 1442695040888963407) & 0xFFFFFFFFFFFFFFFF
            s.append(alphabet[(state >> 33) % len(alphabet)])
        out.append("".join(s))
    return out


def golden_strings():
    s = list(HAND_STRINGS)
    s += lcg_strings(200 - len(s))
    assert len(s) == 200, len(s)
    return s


def goldens(fonts_dir):
    fonts = [Font(fonts_dir, i) for i in range(len(FONT_NAMES))]
    strings = golden_strings()
    results = []
    for fi, font_index in enumerate(GOLDEN_FONTS):
        font = fonts[font_index]
        for hi, h in enumerate(GOLDEN_HEIGHTS):
            for si, text in enumerate(strings):
                b = text.encode("utf-8")
                w = drawstring_width(font, h, b)
                cur = [drawstring_cursor(font, h, b, x, ky * h + ay) for (x, ky, ay) in GOLDEN_POINTS]
                out1, col = drawstring(font, DRAW_LEFT_POS, "LEFT", h, DRAW_COL, b, DRAW_SCREEN_W, DRAW_WIN_H)
                align = ALIGNS[(si + hi + fi) % 5]
                out2, col = drawstring(font, DRAW_ALIGN_POS, align, h, col, b, DRAW_SCREEN_W, DRAW_WIN_H)
                stream = out1 + out2
                results.append({
                    "font": FONT_NAMES[font_index][0], "height": h, "s": si, "width": w, "cursor": cur,
                    "align": align, "quads": sum(1 for k, *_ in stream if k == "Q"),
                    "cmds": len(stream), "digest": fnv1a64(canon(stream)),
                })
    return {
        "source": "crates/pob-ui-oracle/probe/rfont_ref.py (independent transcription of "
                  "SimpleGraphic r_font.cpp @ 3b1a346); regenerate with "
                  "`python3 rfont_ref.py --fonts <runtime/SimpleGraphic/Fonts> goldens <out>`",
        "fonts": [FONT_NAMES[i][0] for i in GOLDEN_FONTS],
        "heights": GOLDEN_HEIGHTS,
        "points": GOLDEN_POINTS,
        "draw": {"screen_w": DRAW_SCREEN_W, "win_h": DRAW_WIN_H, "col": DRAW_COL,
                 "left_pos": DRAW_LEFT_POS, "align_pos": DRAW_ALIGN_POS},
        "strings": strings,
        "results": results,
    }


def lua_quote(b):
    out = ['"']
    for c in b:
        if c in (34, 92) or c < 32 or c >= 127:
            # always 3 digits, so a following digit cannot extend the decimal escape
            out.append("\\%03d" % c)
        else:
            out.append(chr(c))
    out.append('"')
    return "".join(out)


def cases_lua():
    lines = [
        "-- Generated by crates/pob-ui-oracle/probe/rfont_ref.py (cases-lua); do not edit.",
        "-- The Wine probe (Launch.lua) measures these with the real SimpleGraphic.dll.",
        "return {",
        "\tfonts = { %s }," % ", ".join('"%s"' % FONT_NAMES[i][0] for i in GOLDEN_FONTS),
        "\theights = { %s }," % ", ".join(str(h) for h in GOLDEN_HEIGHTS),
        "\tpoints = { %s }," % ", ".join("{ %d, %d, %d }" % p for p in GOLDEN_POINTS),
        "\tstrings = {",
    ]
    for s in golden_strings():
        lines.append("\t\t%s," % lua_quote(s.encode("utf-8")))
    lines += ["\t},", "}", ""]
    return "\n".join(lines)


def main(argv):
    if len(argv) < 3 or argv[0] != "--fonts":
        print(__doc__)
        return 2
    fonts_dir, cmd, args = argv[1], argv[2], argv[3:]
    if cmd == "goldens":
        with open(args[0], "w") as f:
            json.dump(goldens(fonts_dir), f, separators=(",", ":"), ensure_ascii=False)
            f.write("\n")
        return 0
    if cmd == "cases-lua":
        with open(args[0], "w") as f:
            f.write(cases_lua())
        return 0
    names = [n for n, _ in FONT_NAMES]
    h = int(args[0])
    font = Font(fonts_dir, names.index(args[1]))
    b = ast.literal_eval(args[2])
    b = b.encode("utf-8") if isinstance(b, str) else b
    if cmd == "width":
        print(drawstring_width(font, h, b))
    elif cmd == "cursor":
        print(drawstring_cursor(font, h, b, int(args[3]), int(args[4])))
    elif cmd == "draw":
        out, col = drawstring(font, (float(args[3]), float(args[4])), args[5], h, (1.0, 1.0, 1.0, 1.0), b,
                              int(args[6]), int(args[7]))
        for k, v, *cp in out:
            print(k, v, *cp)
        print("col", col)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
