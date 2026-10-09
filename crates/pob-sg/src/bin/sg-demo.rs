//! `sg-demo`: exercises the pob-sg renderer with PoB's own assets.
//!
//! Usage: `sg-demo [--pob-src <dir>] [--fonts <dir>] [--screenshot <out.png> [--frames N] [--scale S]]`
//!
//! Without `--screenshot` a 1280x800 window opens. With it, N frames
//! (default 3) are rendered offscreen (no window needed) and the last one is
//! saved as PNG.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pob_sg::host::{self, App, HostConfig, InputState};
use pob_sg::{Align, Font, FontSet, ImageHandle, Images, LoadFlags, Sg, ColorCodes};

fn locate_pob_src() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("POB_SRC") {
        return Some(PathBuf::from(p));
    }
    let mut starts = Vec::new();
    if let Ok(c) = std::env::current_dir() {
        starts.push(c);
    }
    starts.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    for s in starts {
        for d in s.ancestors() {
            let p = d.join(".PathOfBuilding/src");
            if p.join("Launch.lua").is_file() {
                return Some(p);
            }
        }
    }
    None
}

struct Demo {
    images: Images,
    background: ImageHandle,
    frame: ImageHandle,
    line: ImageHandle,
    skills: ImageHandle,
    ring: ImageHandle,
    clicks: u32,
    last_key: String,
    chars: String,
    button_down: bool,
    hover: bool,
    events: Vec<(bool, String)>,
    color_codes: ColorCodes,
}

impl Demo {
    fn new(images: Images) -> Demo {
        let h = || images.new_handle();
        Demo {
            background: h(),
            frame: h(),
            line: h(),
            skills: h(),
            ring: h(),
            images: images.clone(),
            clicks: 0,
            last_key: String::new(),
            chars: String::new(),
            button_down: false,
            hover: false,
            events: Vec::new(),
            color_codes: ColorCodes::default(),
        }
    }
}

/// PoB's standard widget look (ButtonControl.lua): border rect, inner rect inset by 1, label.
fn draw_button(sg: &mut Sg, x: f32, y: f32, w: f32, h: f32, label: &str, hover: bool, down: bool) {
    if hover {
        sg.set_draw_color(1.0, 1.0, 1.0, 1.0);
    } else {
        sg.set_draw_color(0.5, 0.5, 0.5, 1.0);
    }
    sg.draw_image(None, x, y, w, h, None);
    if down {
        sg.set_draw_color(0.5, 0.5, 0.5, 1.0);
    } else if hover {
        sg.set_draw_color(0.33, 0.33, 0.33, 1.0);
    } else {
        sg.set_draw_color(0.0, 0.0, 0.0, 1.0);
    }
    sg.draw_image(None, x + 1.0, y + 1.0, w - 2.0, h - 2.0, None);
    sg.set_draw_color(1.0, 1.0, 1.0, 1.0);
    sg.draw_string(x + w / 2.0, y + 2.0, Align::CenterX, h - 4.0, Font::Var, label);
}

impl App for Demo {
    fn on_init(&mut self, _sg: &mut Sg) {
        // PoB: main:DrawBackground tiles Background2 (REPEAT wrap).
        self.images.load(self.background, "TreeData/3_28/background-3.png", LoadFlags::NONE);
        self.images.load(self.frame, "TreeData/3_28/frame-3.png", LoadFlags::CLAMP | LoadFlags::ASYNC);
        self.images.load(self.line, "TreeData/3_28/line-3.png", LoadFlags::CLAMP | LoadFlags::ASYNC);
        self.images.load(self.skills, "TreeData/3_28/skills-3.jpg", LoadFlags::CLAMP | LoadFlags::MIPMAP | LoadFlags::ASYNC);
        self.images.load(self.ring, "Assets/ring.png", LoadFlags::CLAMP);
    }

    fn on_frame(&mut self, sg: &mut Sg, input: &mut InputState) {
        let (sw, sh) = sg.virtual_screen_size();
        let (sw, sh) = (sw as f32, sh as f32);
        let t = input.get_time() as f32 / 1000.0;
        let (cx, cy) = input.cursor_pos();
        let (cx, cy) = (cx as f32, cy as f32);

        // Background at sublayer -100, tex coords = size / 100 (Main.lua:1486).
        sg.set_draw_layer(None, Some(-100));
        sg.set_draw_color(1.0, 1.0, 1.0, 1.0);
        sg.draw_image(Some(self.background), 0.0, 0.0, sw, sh, Some([0.0, 0.0, sw / 100.0, sh / 100.0]));
        sg.set_draw_layer(None, Some(0));

        // Title.
        sg.set_draw_color(1.0, 1.0, 1.0, 1.0);
        sg.draw_string(10.0, 6.0, Align::Left, 32.0, Font::FontinSc, "^xC8C8C8pob-sg ^7SimpleGraphic demo");

        // Every face at heights 10..32 with escape colours.
        let mut y = 46.0;
        let faces = Font::ALL;
        for (i, f) in faces.iter().enumerate() {
            sg.set_draw_color(0.7, 0.7, 0.7, 1.0);
            sg.draw_string(10.0, y + 10.0, Align::Left, 14.0, Font::Var, f.name());
            let mut x = 150.0;
            for h in (10..=32).step_by(2) {
                let esc = (h / 2 + i) % 10;
                let s = format!("^{esc}Ag^7{h}");
                sg.set_draw_color(1.0, 1.0, 1.0, 1.0);
                sg.draw_string(x, y + (32 - h) as f32, Align::Left, h as f32, *f, &s);
                x += sg.draw_string_width(h as f32, *f, &s) + 6.0;
            }
            y += 36.0;
        }

        // Palette (^0..^9) on a mid-grey box, and some colorCodes.
        sg.set_draw_color(0.25, 0.25, 0.25, 0.9);
        sg.draw_image(None, 10.0, y, 520.0, 24.0, None);
        sg.set_draw_color(1.0, 1.0, 1.0, 1.0);
        sg.draw_string(16.0, y + 3.0, Align::Left, 18.0, Font::VarBold, "^7palette: ^00 ^11 ^22 ^33 ^44 ^55 ^66 ^77 ^88 ^99 ^7(^^N escapes)");
        y += 30.0;
        let mut x = 10.0;
        for name in ["NORMAL", "MAGIC", "RARE", "UNIQUE", "GEM", "CRAFTED", "FIRE", "COLD", "LIGHTNING", "CHAOS", "POSITIVE", "NEGATIVE"] {
            let s = format!("{}{}", self.color_codes.get(name).unwrap_or("^7"), name);
            sg.draw_string(x, y, Align::Left, 16.0, Font::Var, &s);
            x += sg.draw_string_width(16.0, Font::Var, &s) + 8.0;
        }
        y += 24.0;
        // Tofu for non-ASCII, tabs, multi-line.
        sg.set_draw_color(1.0, 1.0, 1.0, 1.0);
        sg.draw_string(10.0, y, Align::Left, 16.0, Font::Var, "Tofu: caf\u{e9} \u{2192} ok\tTAB\tTAB\n^8Second line ^xFF9922(multi-line, ^7DrawString \\n)");
        y += 40.0;

        // Alignment: anchor line at x = 270.
        sg.set_draw_color(0.6, 0.0, 0.0, 1.0);
        sg.draw_image(None, 270.0, y, 1.0, 72.0, None);
        sg.set_draw_color(1.0, 1.0, 1.0, 1.0);
        sg.draw_string(270.0, y, Align::Left, 16.0, Font::Var, "LEFT at anchor");
        sg.draw_string(270.0, y + 18.0, Align::CenterX, 16.0, Font::Var, "CENTER_X at anchor");
        sg.draw_string(270.0, y + 36.0, Align::RightX, 16.0, Font::Var, "RIGHT_X at anchor");
        sg.draw_string(0.0, y + 54.0, Align::Right, 16.0, Font::Var, "^8RIGHT (screen-relative, x = 0)");
        y += 80.0;

        // Layered / clipped rectangles inside a viewport.
        let (vx, vy, vw, vh) = (10.0, y, 330.0, 120.0);
        sg.set_draw_color(1.0, 1.0, 1.0, 1.0);
        sg.draw_image(None, vx - 1.0, vy - 1.0, vw + 2.0, vh + 2.0, None);
        sg.set_draw_color(0.05, 0.05, 0.05, 1.0);
        sg.draw_image(None, vx, vy, vw, vh, None);
        sg.set_viewport(Some((vx, vy, vw, vh)));
        sg.set_draw_layer(Some(2), Some(0));
        sg.set_draw_color(0.9, 0.2, 0.2, 1.0);
        sg.draw_image(None, 40.0, 20.0, 120.0, 70.0, None); // layer 2: on top though drawn first
        sg.set_draw_layer(Some(1), Some(0));
        sg.set_draw_color(0.2, 0.4, 0.95, 1.0);
        sg.draw_image(None, 100.0, 40.0, 120.0, 70.0, None); // layer 1
        sg.set_draw_layer(None, Some(5));
        sg.set_draw_color(0.2, 0.9, 0.3, 0.6);
        let ox = (t * 60.0) % 500.0 - 100.0;
        sg.draw_image(None, ox, 60.0, 140.0, 90.0, None); // layer 1/5, clipped by the viewport
        sg.set_draw_color(1.0, 1.0, 1.0, 1.0);
        sg.draw_string(-20.0, 4.0, Align::Left, 14.0, Font::Fixed, "clipped text runs past the viewport edge on both sides ->>>");
        sg.set_draw_layer(Some(0), Some(0));
        sg.set_viewport(None);
        y += vh + 10.0;

        // PoB-style button.
        let (bx, by, bw, bh) = (10.0, y, 140.0, 22.0);
        // Click handling is event-driven (on_key_down/on_key_up), like PoB's
        // ControlHost; hover is computed here from GetCursorPos.
        let hover = cx >= bx && cx < bx + bw && cy >= by && cy < by + bh;
        self.hover = hover;
        // Like PoB's main.inputEvents: callbacks queue, OnFrame processes.
        for (down, key) in std::mem::take(&mut self.events) {
            if key != "LEFTBUTTON" {
                continue;
            }
            if down && hover {
                self.button_down = true;
            } else if !down {
                if self.button_down && hover {
                    self.clicks += 1;
                }
                self.button_down = false;
            }
        }
        draw_button(sg, bx, by, bw, bh, "Click me", hover, self.button_down && hover);
        sg.draw_string(bx + bw + 10.0, by + 3.0, Align::Left, 16.0, Font::Var, &format!("^7clicks: ^x33FF77{}", self.clicks));

        // Right side: tree sprites via DrawImageQuad.
        let rx = 820.0;
        let ry = 80.0;
        let (fw, fh) = (383.0, 334.0); // frame-3.png
        // KeystoneFrameAllocated (166,0,83,85), rotated around its centre.
        let (ccx, ccy) = (rx + 60.0, ry + 60.0);
        let ang = t * 0.8;
        let corners = [(-41.5f32, -42.5f32), (41.5, -42.5), (41.5, 42.5), (-41.5, 42.5)];
        let mut pos = [0.0f32; 8];
        for (k, (dx, dy)) in corners.iter().enumerate() {
            pos[k * 2] = ccx + dx * ang.cos() - dy * ang.sin();
            pos[k * 2 + 1] = ccy + dx * ang.sin() + dy * ang.cos();
        }
        let (u0, v0, u1, v1) = (166.0 / fw, 0.0, (166.0 + 83.0) / fw, 85.0 / fh);
        // Skill icon under the frame: keystone 'AnointOnlyKeystone' (0,1300,52,53) in skills-3.jpg (962x1459).
        sg.set_draw_color(1.0, 1.0, 1.0, 1.0);
        sg.draw_image(Some(self.skills), ccx - 26.0, ccy - 26.5, 52.0, 53.0, Some([0.0, 1300.0 / 1459.0, 52.0 / 962.0, 1353.0 / 1459.0]));
        sg.draw_image_quad(Some(self.frame), pos, Some([u0, v0, u1, v0, u1, v1, u0, v1]));

        // LineConnectorActive (0,12,368,13) in line-3.png (368x1140) between two points.
        let (lw, lh) = (368.0, 1140.0);
        let (ax, ay, bx2, by2) = (rx + 140.0, ry + 20.0, rx + 400.0, ry + 140.0);
        let (dx, dy) = (bx2 - ax, by2 - ay);
        let len = (dx * dx + dy * dy).sqrt();
        let (nx, ny) = (-dy / len * 6.5, dx / len * 6.5);
        let lpos = [ax + nx, ay + ny, bx2 + nx, by2 + ny, bx2 - nx, by2 - ny, ax - nx, ay - ny];
        let (lv0, lv1) = (12.0 / lh, 25.0 / lh);
        sg.draw_image_quad(Some(self.line), lpos, Some([0.0, lv0, len / lw, lv0, len / lw, lv1, 0.0, lv1]));

        // Orbit3Active (231,231,136,136): a quarter arc; four mirrored quads make a circle.
        let (ox0, oy0, ow) = (231.0 / lw, 231.0 / lh, 136.0);
        let (ou1, ov1) = ((231.0 + ow) / lw, (231.0 + ow) / lh);
        let (ocx, ocy) = (rx + 120.0, ry + 330.0);
        sg.set_draw_color(1.0, 1.0, 1.0, 1.0);
        for (sx, sy) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            // The sprite's arc centre is at its bottom-right corner (u1, v1);
            // mirror the uvs for the other quadrants.
            let (xa, xb, ua, ub) = if sx < 0.0 { (ocx - ow, ocx, ox0, ou1) } else { (ocx, ocx + ow, ou1, ox0) };
            let (ya, yb, va, vb) = if sy < 0.0 { (ocy - ow, ocy, oy0, ov1) } else { (ocy, ocy + ow, ov1, oy0) };
            sg.draw_image_quad(Some(self.line), [xa, ya, xb, ya, xb, yb, xa, yb], Some([ua, va, ub, va, ub, vb, ua, vb]));
        }
        // Assets/ring.png tinted, solid-colour arrow quad (main:DrawArrow style).
        sg.set_draw_color(1.0, 0.8, 0.2, 0.9);
        sg.draw_image(Some(self.ring), rx + 300.0, ry + 260.0, 120.0, 120.0, None);
        sg.set_draw_color(0.9, 0.9, 0.9, 1.0);
        let (axc, ayc) = (rx + 360.0, ry + 450.0);
        sg.draw_image_quad(None, [axc - 12.0, ayc - 8.0, axc + 12.0, ayc - 8.0, axc, ayc + 8.0, axc, ayc + 8.0], None);

        // Status line (layer 10 like popups).
        sg.set_draw_layer(Some(10), Some(0));
        sg.set_draw_color(0.0, 0.0, 0.0, 0.7);
        sg.draw_image(None, 0.0, sh - 22.0, sw, 22.0, None);
        sg.set_draw_color(1.0, 1.0, 1.0, 1.0);
        let (pw, ph) = sg.screen_size();
        let status = format!(
            "^8cursor ^7{cx:.0},{cy:.0}  ^8screen ^7{pw}x{ph} ^8scale ^7{:.2}  ^8time ^7{:.1}s  ^8last key ^7{}  ^8typed ^7{}",
            sg.screen_scale(),
            t,
            self.last_key,
            self.chars
        );
        sg.draw_string(6.0, sh - 19.0, Align::Left, 16.0, Font::Fixed, &status);
        sg.set_draw_layer(Some(0), Some(0));
    }

    fn on_key_down(&mut self, key: &str, double_click: bool) {
        self.last_key = if double_click { format!("{key} (double)") } else { key.to_owned() };
        self.events.push((true, key.to_owned()));
    }

    fn on_key_up(&mut self, key: &str) {
        self.events.push((false, key.to_owned()));
        if key == "WHEELUP" || key == "WHEELDOWN" {
            self.last_key = key.to_owned();
        }
    }

    fn on_char(&mut self, ch: char) {
        if ch == '\u{8}' {
            self.chars.pop();
        } else if !ch.is_control() {
            self.chars.push(ch);
            if self.chars.chars().count() > 24 {
                self.chars.remove(0);
            }
        }
    }

    fn can_exit(&mut self) -> bool {
        true
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let pob_src = arg("--pob-src").map(PathBuf::from).or_else(locate_pob_src);
    let fonts_dir = arg("--fonts").map(PathBuf::from).or_else(FontSet::locate);
    let images = Images::new();
    match &pob_src {
        Some(p) => images.set_base_dir(p),
        None => eprintln!("warning: PoB src not found (set POB_SRC or --pob-src); images will fail to load"),
    }
    let fonts = match fonts_dir.as_deref().map(FontSet::load) {
        Some(Ok(f)) => Some(Arc::new(f)),
        Some(Err(e)) => {
            eprintln!("warning: fonts: {e}");
            None
        }
        None => {
            eprintln!("warning: fonts not found (set POB_SG_FONTS or --fonts)");
            None
        }
    };
    let demo = Demo::new(images.clone());
    if let Some(out) = arg("--screenshot") {
        let frames: u32 = arg("--frames").and_then(|s| s.parse().ok()).unwrap_or(3);
        let scale: f32 = arg("--scale").and_then(|s| s.parse().ok()).unwrap_or(1.0);
        let (w, h) = ((1280.0 * scale) as u32, (800.0 * scale) as u32);
        match host::run_headless(demo, images, fonts, w, h, scale, frames) {
            Ok(px) => {
                if let Err(e) = image::save_buffer(Path::new(&out), &px, w, h, image::ColorType::Rgba8) {
                    eprintln!("error: saving {out}: {e}");
                    std::process::exit(1);
                }
                println!("wrote {out} ({w}x{h}, {frames} frames)");
            }
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    let cfg = HostConfig { title: "pob-sg demo".into(), width: 1280, height: 800, images, fonts };
    if let Err(e) = host::run(demo, cfg) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
