//! `sg-demo`: the SimpleGraphic test card, drawn with PoB's own assets (moved from the pre-port
//! `pob-sg/src/bin/sg-demo.rs` by UI-U0 and adapted to the pob-sg / pob-host / pob-sg-wgpu
//! surfaces; U7 owns it).
//!
//! Usage: `sg-demo [--assets <bundle dir>] [--screenshot <out.png> [--frames N] [--scale S]]`
//!
//! The assets are the app's asset bundle (VENDOR, `pob_desktop::locate_assets`; `--assets`
//! overrides it).
//!
//! Without `--screenshot` a 1280x800 window opens. With it, N frames (default 3) are rendered
//! offscreen (no window needed) and the last one is saved as PNG.

use std::path::{Path, PathBuf};

use pob_host::{App, HostConfig, PlatformParts};
use pob_sg::{Align, Font, ImageFlags, ImageHandle, Key, ScreenInfo, Sg, Sys};
use pob_ui_core::color_codes::ColorCodes;

struct Demo {
    sg: Sg,
    sys: Sys,
    background: ImageHandle,
    frame: ImageHandle,
    line: ImageHandle,
    skills: ImageHandle,
    ring: ImageHandle,
    clicks: u32,
    last_key: String,
    chars: String,
    button_down: bool,
    events: Vec<(bool, Key)>,
}

impl Demo {
    fn new(parts: PlatformParts) -> Demo {
        let PlatformParts { sg, sys } = parts;
        let h = ImageHandle::default;
        Demo {
            sg,
            sys,
            background: h(),
            frame: h(),
            line: h(),
            skills: h(),
            ring: h(),
            clicks: 0,
            last_key: String::new(),
            chars: String::new(),
            button_down: false,
            events: Vec::new(),
        }
    }
}

/// PoB's standard widget look (ButtonControl.lua): border rect, inner rect inset by 1, label.
fn draw_button(sg: &mut Sg, x: f64, y: f64, w: f64, h: f64, label: &str, hover: bool, down: bool) {
    if hover {
        sg.set_draw_color(1.0, 1.0, 1.0);
    } else {
        sg.set_draw_color(0.5, 0.5, 0.5);
    }
    sg.draw_image(None, x, y, w, h);
    if down {
        sg.set_draw_color(0.5, 0.5, 0.5);
    } else if hover {
        sg.set_draw_color(0.33, 0.33, 0.33);
    } else {
        sg.set_draw_color(0.0, 0.0, 0.0);
    }
    sg.draw_image(None, x + 1.0, y + 1.0, w - 2.0, h - 2.0);
    sg.set_draw_color(1.0, 1.0, 1.0);
    sg.draw_string(
        x + w / 2.0,
        y + 2.0,
        Align::CenterX,
        h - 4.0,
        Font::Var,
        label,
    );
}

impl App for Demo {
    fn sg(&mut self) -> &mut Sg {
        &mut self.sg
    }
    fn sys(&mut self) -> &mut Sys {
        &mut self.sys
    }

    fn on_init(&mut self) {
        let sg = &mut self.sg;
        // PoB: main:DrawBackground tiles Background2 (REPEAT wrap).
        self.background
            .load(sg, "TreeData/3_28/background-3.png", ImageFlags::empty());
        self.frame.load(
            sg,
            "TreeData/3_28/frame-3.png",
            ImageFlags::CLAMP | ImageFlags::ASYNC,
        );
        self.line.load(
            sg,
            "TreeData/3_28/line-3.png",
            ImageFlags::CLAMP | ImageFlags::ASYNC,
        );
        self.skills.load(
            sg,
            "TreeData/3_28/skills-3.jpg",
            ImageFlags::CLAMP | ImageFlags::MIPMAP | ImageFlags::ASYNC,
        );
        self.ring.load(sg, "Assets/ring.png", ImageFlags::CLAMP);
    }

    fn on_frame(&mut self) {
        let (sw, sh) = self.sg.get_virtual_screen_size();
        let t = self.sys.time_ms() as f64 / 1000.0;
        let (cx, cy) = self.sys.cursor_pos();
        let sg = &mut self.sg;

        // Background at sublayer -100, tex coords = size / 100 (Main.lua:1486).
        sg.set_draw_layer(None, Some(-100));
        sg.set_draw_color(1.0, 1.0, 1.0);
        sg.draw_image_tc(
            Some(&self.background),
            0.0,
            0.0,
            sw,
            sh,
            0.0,
            0.0,
            sw / 100.0,
            sh / 100.0,
        );
        sg.set_draw_layer(None, Some(0));

        // Title.
        sg.set_draw_color(1.0, 1.0, 1.0);
        sg.draw_string(
            10.0,
            6.0,
            Align::Left,
            32.0,
            Font::FontinSc,
            "^xC8C8C8pob-sg ^7SimpleGraphic demo",
        );

        // Every face at heights 10..32 with escape colours.
        let mut y = 46.0;
        for (i, f) in Font::ALL.iter().enumerate() {
            sg.set_draw_color(0.7, 0.7, 0.7);
            sg.draw_string(10.0, y + 10.0, Align::Left, 14.0, Font::Var, f.lua_name());
            let mut x = 150.0;
            for h in (10..=32).step_by(2) {
                let esc = (h / 2 + i) % 10;
                let s = format!("^{esc}Ag^7{h}");
                sg.set_draw_color(1.0, 1.0, 1.0);
                sg.draw_string(
                    x,
                    y + f64::from(32 - h as i32),
                    Align::Left,
                    h as f64,
                    *f,
                    &s,
                );
                x += sg.draw_string_width(h as f64, *f, &s) + 6.0;
            }
            y += 36.0;
        }

        // Palette (^0..^9) on a mid-grey box, and some colorCodes.
        sg.set_draw_color_a(0.25, 0.25, 0.25, 0.9);
        sg.draw_image(None, 10.0, y, 520.0, 24.0);
        sg.set_draw_color(1.0, 1.0, 1.0);
        sg.draw_string(
            16.0,
            y + 3.0,
            Align::Left,
            18.0,
            Font::VarBold,
            "^7palette: ^00 ^11 ^22 ^33 ^44 ^55 ^66 ^77 ^88 ^99 ^7(^^N escapes)",
        );
        y += 30.0;
        let mut x = 10.0;
        let cc = ColorCodes::default();
        for name in [
            "NORMAL",
            "MAGIC",
            "RARE",
            "UNIQUE",
            "GEM",
            "CRAFTED",
            "FIRE",
            "COLD",
            "LIGHTNING",
            "CHAOS",
            "POSITIVE",
            "NEGATIVE",
        ] {
            let s = format!("{}{}", cc.get(name).map_or("^7", String::as_str), name);
            sg.draw_string(x, y, Align::Left, 16.0, Font::Var, &s);
            x += sg.draw_string_width(16.0, Font::Var, &s) + 8.0;
        }
        y += 24.0;
        // Tofu for non-ASCII, tabs, multi-line.
        sg.set_draw_color(1.0, 1.0, 1.0);
        sg.draw_string(10.0, y, Align::Left, 16.0, Font::Var, "Tofu: caf\u{e9} \u{2192} ok\tTAB\tTAB\n^8Second line ^xFF9922(multi-line, ^7DrawString \\n)");
        y += 40.0;

        // Alignment: anchor line at x = 270.
        sg.set_draw_color(0.6, 0.0, 0.0);
        sg.draw_image(None, 270.0, y, 1.0, 72.0);
        sg.set_draw_color(1.0, 1.0, 1.0);
        sg.draw_string(270.0, y, Align::Left, 16.0, Font::Var, "LEFT at anchor");
        sg.draw_string(
            270.0,
            y + 18.0,
            Align::CenterX,
            16.0,
            Font::Var,
            "CENTER_X at anchor",
        );
        sg.draw_string(
            270.0,
            y + 36.0,
            Align::RightX,
            16.0,
            Font::Var,
            "RIGHT_X at anchor",
        );
        sg.draw_string(
            0.0,
            y + 54.0,
            Align::Right,
            16.0,
            Font::Var,
            "^8RIGHT (screen-relative, x = 0)",
        );
        y += 80.0;

        // Layered / clipped rectangles inside a viewport.
        let (vx, vy, vw, vh) = (10.0, y, 330.0, 120.0);
        sg.set_draw_color(1.0, 1.0, 1.0);
        sg.draw_image(None, vx - 1.0, vy - 1.0, vw + 2.0, vh + 2.0);
        sg.set_draw_color(0.05, 0.05, 0.05);
        sg.draw_image(None, vx, vy, vw, vh);
        sg.set_viewport(vx, vy, vw, vh);
        sg.set_draw_layer(Some(2), Some(0));
        sg.set_draw_color(0.9, 0.2, 0.2);
        sg.draw_image(None, 40.0, 20.0, 120.0, 70.0); // layer 2: on top though drawn first
        sg.set_draw_layer(Some(1), Some(0));
        sg.set_draw_color(0.2, 0.4, 0.95);
        sg.draw_image(None, 100.0, 40.0, 120.0, 70.0); // layer 1
        sg.set_draw_layer(None, Some(5));
        sg.set_draw_color_a(0.2, 0.9, 0.3, 0.6);
        let ox = (t * 60.0) % 500.0 - 100.0;
        sg.draw_image(None, ox, 60.0, 140.0, 90.0); // layer 1/5, clipped by the viewport
        sg.set_draw_color(1.0, 1.0, 1.0);
        sg.draw_string(
            -20.0,
            4.0,
            Align::Left,
            14.0,
            Font::Fixed,
            "clipped text runs past the viewport edge on both sides ->>>",
        );
        sg.set_draw_layer(Some(0), Some(0));
        sg.reset_viewport();
        y += vh + 10.0;

        // PoB-style button; clicks are event-driven like PoB's ControlHost.
        let (bx, by, bw, bh) = (10.0, y, 140.0, 22.0);
        let hover = cx >= bx && cx < bx + bw && cy >= by && cy < by + bh;
        for (down, key) in std::mem::take(&mut self.events) {
            if key != Key::LeftButton {
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
        draw_button(
            sg,
            bx,
            by,
            bw,
            bh,
            "Click me",
            hover,
            self.button_down && hover,
        );
        sg.draw_string(
            bx + bw + 10.0,
            by + 3.0,
            Align::Left,
            16.0,
            Font::Var,
            &format!("^7clicks: ^x33FF77{}", self.clicks),
        );

        // Right side: tree sprites via DrawImageQuad.
        let (rx, ry) = (820.0, 80.0);
        let (fw, fh) = (383.0, 334.0); // frame-3.png
                                       // KeystoneFrameAllocated (166,0,83,85), rotated around its centre.
        let (ccx, ccy) = (rx + 60.0, ry + 60.0);
        let ang = t * 0.8;
        let corners = [
            (-41.5f64, -42.5f64),
            (41.5, -42.5),
            (41.5, 42.5),
            (-41.5, 42.5),
        ];
        let mut pos = [0.0f64; 8];
        for (k, (dx, dy)) in corners.iter().enumerate() {
            pos[k * 2] = ccx + dx * ang.cos() - dy * ang.sin();
            pos[k * 2 + 1] = ccy + dx * ang.sin() + dy * ang.cos();
        }
        let (u0, v0, u1, v1) = (166.0 / fw, 0.0, (166.0 + 83.0) / fw, 85.0 / fh);
        // Skill icon under the frame: keystone 'AnointOnlyKeystone' (0,1300,52,53) in skills-3.jpg (962x1459).
        sg.set_draw_color(1.0, 1.0, 1.0);
        sg.draw_image_tc(
            Some(&self.skills),
            ccx - 26.0,
            ccy - 26.5,
            52.0,
            53.0,
            0.0,
            1300.0 / 1459.0,
            52.0 / 962.0,
            1353.0 / 1459.0,
        );
        sg.draw_image_quad_tc(Some(&self.frame), pos, [u0, v0, u1, v0, u1, v1, u0, v1]);

        // LineConnectorActive (0,12,368,13) in line-3.png (368x1140) between two points.
        let (lw, lh) = (368.0, 1140.0);
        let (ax, ay, bx2, by2) = (rx + 140.0, ry + 20.0, rx + 400.0, ry + 140.0);
        let (dx, dy) = (bx2 - ax, by2 - ay);
        let len = (dx * dx + dy * dy).sqrt();
        let (nx, ny) = (-dy / len * 6.5, dx / len * 6.5);
        let lpos = [
            ax + nx,
            ay + ny,
            bx2 + nx,
            by2 + ny,
            bx2 - nx,
            by2 - ny,
            ax - nx,
            ay - ny,
        ];
        let (lv0, lv1) = (12.0 / lh, 25.0 / lh);
        sg.draw_image_quad_tc(
            Some(&self.line),
            lpos,
            [0.0, lv0, len / lw, lv0, len / lw, lv1, 0.0, lv1],
        );

        // Orbit3Active (231,231,136,136): a quarter arc; four mirrored quads make a circle.
        let (ox0, oy0, ow) = (231.0 / lw, 231.0 / lh, 136.0);
        let (ou1, ov1) = ((231.0 + ow) / lw, (231.0 + ow) / lh);
        let (ocx, ocy) = (rx + 120.0, ry + 330.0);
        sg.set_draw_color(1.0, 1.0, 1.0);
        for (sx, sy) in [(-1.0f64, -1.0f64), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let (xa, xb, ua, ub) = if sx < 0.0 {
                (ocx - ow, ocx, ox0, ou1)
            } else {
                (ocx, ocx + ow, ou1, ox0)
            };
            let (ya, yb, va, vb) = if sy < 0.0 {
                (ocy - ow, ocy, oy0, ov1)
            } else {
                (ocy, ocy + ow, ov1, oy0)
            };
            sg.draw_image_quad_tc(
                Some(&self.line),
                [xa, ya, xb, ya, xb, yb, xa, yb],
                [ua, va, ub, va, ub, vb, ua, vb],
            );
        }
        // Assets/ring.png tinted, solid-colour arrow quad (main:DrawArrow style).
        sg.set_draw_color_a(1.0, 0.8, 0.2, 0.9);
        sg.draw_image(Some(&self.ring), rx + 300.0, ry + 260.0, 120.0, 120.0);
        sg.set_draw_color(0.9, 0.9, 0.9);
        let (axc, ayc) = (rx + 360.0, ry + 450.0);
        sg.draw_image_quad(
            None,
            [
                axc - 12.0,
                ayc - 8.0,
                axc + 12.0,
                ayc - 8.0,
                axc,
                ayc + 8.0,
                axc,
                ayc + 8.0,
            ],
        );

        // Status line (layer 10 like popups).
        sg.set_draw_layer(Some(10), Some(0));
        sg.set_draw_color_a(0.0, 0.0, 0.0, 0.7);
        sg.draw_image(None, 0.0, sh - 22.0, sw, 22.0);
        sg.set_draw_color(1.0, 1.0, 1.0);
        let (pw, ph) = sg.get_screen_size();
        let status = format!(
            "^8cursor ^7{cx:.0},{cy:.0}  ^8screen ^7{pw}x{ph} ^8scale ^7{:.2}  ^8time ^7{:.1}s  ^8last key ^7{}  ^8typed ^7{}",
            sg.get_screen_scale(),
            t,
            self.last_key,
            self.chars
        );
        sg.draw_string(6.0, sh - 19.0, Align::Left, 16.0, Font::Fixed, &status);
        sg.set_draw_layer(Some(0), Some(0));
    }

    fn on_key_down(&mut self, key: Key, double_click: bool) {
        self.last_key = if double_click {
            format!("{key} (double)")
        } else {
            key.name().into_owned()
        };
        self.events.push((true, key));
    }

    fn on_key_up(&mut self, key: Key) {
        self.events.push((false, key));
        if key == Key::WheelUp || key == Key::WheelDown {
            self.last_key = key.name().into_owned();
        }
    }

    fn on_char(&mut self, ch: Key) {
        match ch {
            Key::Char(8) => {
                self.chars.pop();
            }
            Key::Char(b) if !b.is_ascii_control() => {
                self.chars.push(char::from(b));
                if self.chars.chars().count() > 24 {
                    self.chars.remove(0);
                }
            }
            Key::Unknown => self.chars.push('?'),
            _ => {}
        }
    }

    fn on_lua_error(&mut self, msg: String) {
        eprintln!("sg-demo: {msg}");
    }
}

fn screenshot(cfg: &HostConfig, out: &str, frames: u32, scale: f32) -> anyhow::Result<()> {
    let (parts, _requests) = pob_host::desktop::build_parts(cfg, true)?;
    let mut demo = Demo::new(parts);
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let (adapter, device, queue) = pollster::block_on(pob_sg_wgpu::request_device(&instance, None))
        .map_err(anyhow::Error::msg)?;
    let mut renderer =
        pob_sg_wgpu::SgRenderer::new(&device, &adapter.limits(), pob_sg_wgpu::RT_FORMAT);
    let (w, h) = ((1280.0 * scale) as u32, (800.0 * scale) as u32);
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("sg-demo output"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: pob_sg_wgpu::RT_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    demo.on_init();
    for _ in 0..frames.max(1) {
        demo.sg.begin_frame(ScreenInfo {
            fb_w: w as i32,
            fb_h: h as i32,
            win_h: h as i32,
            dpi_scale: scale,
        });
        let s = demo.sg.scale();
        demo.sys.begin_frame((0, 0), s);
        demo.on_frame();
        let frame = demo.sg.end_frame();
        renderer.prepare(&device, &queue, &frame);
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("sg-demo"),
        });
        renderer.render(&mut enc, &view);
        queue.submit(Some(enc.finish()));
    }
    let (rw, rh, px) = renderer.read_back(&device, &queue);
    image::save_buffer(Path::new(out), &px, rw, rh, image::ColorType::Rgba8)?;
    println!("wrote {out} ({rw}x{rh}, {frames} frames)");
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let assets = match arg("--assets") {
        Some(a) => PathBuf::from(a),
        None => pob_desktop::locate_assets()?,
    };
    let dir = assets.to_string_lossy().into_owned();
    let cfg = HostConfig {
        title: "pob-sg demo".into(),
        size: (1280, 800),
        min_size: (640, 400),
        script_dir: dir.clone(),
        sg_runtime: dir,
    };
    if let Some(out) = arg("--screenshot") {
        let frames: u32 = arg("--frames").and_then(|s| s.parse().ok()).unwrap_or(3);
        let scale: f32 = arg("--scale").and_then(|s| s.parse().ok()).unwrap_or(1.0);
        return screenshot(&cfg, &out, frames, scale);
    }
    pob_host::run_desktop(cfg, Demo::new)
}
