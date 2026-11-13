//! `pob-lua-gui`: real PoB Lua on pob-sg-wgpu in a winit window (PoB natively on Linux), the
//! visual reference; `--record out.pobscript` records a human session (ui doc §11.2 CLI).
//! Owner: U8.
//!
//! ```text
//! cargo run -p pob-ui-oracle --bin pob-lua-gui -- [--user DIR] [--record out.pobscript]
//! ```
//!
//! PoB boots exactly as in the oracle ([`pob_ui_oracle::boot`]: our SG globals, the text
//! patches, the markers) but live: the real clock and random seed (no `lua/determinism.lua`),
//! the desktop host's input (`pob_host::run_desktop`), asynchronous image loads and the system
//! clipboard. `GetUserPath()` is `--user DIR` (PoB then uses `DIR/Path of Building/`), or by
//! default a fresh temporary copy of `fixtures/user1`, so the fixture builds are in the build
//! list and the real user data is never touched. Network sub-scripts are not started (patch 8:
//! downloads fail with "No net fixture for <url>").
//!
//! `--record FILE` writes the session as a `.pobscript` (`screen` from the first frame, `frames N`,
//! `move X Y` when the virtual cursor moved, `key-down`/`key-up`, `type` for typed characters;
//! the OnChar that SG sends after BACK/TAB/RETURN/ESCAPE is not recorded because the driver
//! re-creates it). The file is rewritten after every event, so it survives a crash. Replay with
//! `ui-parity FILE`. DIVERGENCE: the cursor is recorded in virtual px (`GetCursorPos`, rounded),
//! so at a non-integer DPI scale the replayed physical position may differ by 1 px.

use std::cell::RefCell;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::rc::Rc;

use pob_host::{App, HostConfig, PlatformParts};
use pob_sg::{Key, Platform, Sg, Sys};
use pob_ui_oracle::boot::{self, BootOptions, Entry};
use pob_ui_oracle::runner::{LuaApp, ScenarioApp, UserDir};

/// The desktop platform with the oracle's paths: `GetUserPath()` is the session's user dir,
/// `GetRuntimePath()` PoB's `runtime/` (SG's executable dir), `GetScriptPath()` PoB's `src/`.
struct GuiPlatform {
    inner: Box<dyn Platform>,
    script_path: String,
    runtime_path: String,
    user_path: String,
}

impl Platform for GuiPlatform {
    fn time_ms(&self) -> i64 {
        self.inner.time_ms()
    }
    fn copy(&mut self, text: &str) {
        self.inner.copy(text);
    }
    fn paste(&mut self) -> Option<String> {
        self.inner.paste()
    }
    fn open_url(&mut self, url: &str) {
        self.inner.open_url(url);
    }
    fn set_window_title(&mut self, title: &str) {
        self.inner.set_window_title(title);
    }
    fn take_screenshot(&mut self) {
        self.inner.take_screenshot();
    }
    fn restart(&mut self) {
        self.inner.restart();
    }
    fn exit(&mut self, msg: Option<&str>) {
        self.inner.exit(msg);
    }
    fn script_path(&self) -> String {
        self.script_path.clone()
    }
    fn runtime_path(&self) -> String {
        self.runtime_path.clone()
    }
    fn user_path(&self) -> Result<String, (String, String)> {
        Ok(self.user_path.clone())
    }
    fn con_print(&mut self, s: &str) {
        println!("{s}");
    }
}

/// `.pobscript` recorder state.
#[derive(Default)]
struct Recorder {
    path: Option<PathBuf>,
    text: String,
    pending_frames: u32,
    last_cursor: Option<(f64, f64)>,
    screen_written: bool,
}

impl Recorder {
    fn line(&mut self, l: &str) {
        if self.path.is_none() {
            return;
        }
        if self.pending_frames > 0 {
            let _ = writeln!(self.text, "frames {}", self.pending_frames);
            self.pending_frames = 0;
        }
        let _ = writeln!(self.text, "{l}");
        self.flush();
    }
    fn flush(&self) {
        if let Some(p) = &self.path {
            let mut t = self.text.clone();
            if self.pending_frames > 0 {
                let _ = writeln!(t, "frames {}", self.pending_frames);
            }
            if let Err(e) = std::fs::write(p, t) {
                eprintln!("pob-lua-gui: --record {}: {e}", p.display());
            }
        }
    }
}

/// `type "..."` argument: the parser's escapes (`\n \t \r \b \e \" \\`).
fn quote(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            '\r' => o.push_str("\\r"),
            '\u{8}' => o.push_str("\\b"),
            '\u{1b}' => o.push_str("\\e"),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

struct Gui {
    app: LuaApp,
    rec: Rc<RefCell<Recorder>>,
    /// Kept alive for the session (removed on drop).
    _user: Option<UserDir>,
}

impl Gui {
    fn report_error(&mut self) {
        if let Some(e) = self.app.take_error() {
            eprintln!("pob-lua-gui: Lua error outside Launch's PCall: {e}");
        }
    }
}

impl App for Gui {
    fn sg(&mut self) -> &mut Sg {
        self.app.sg()
    }
    fn sys(&mut self) -> &mut Sys {
        self.app.sys()
    }
    fn on_init(&mut self) {
        self.app.on_init();
        self.report_error();
    }
    fn on_frame(&mut self) {
        {
            let mut r = self.rec.borrow_mut();
            if r.path.is_some() {
                if !r.screen_written {
                    let (w, h) = self.app.sg().get_screen_size();
                    let s = self.app.sg().scale();
                    r.screen_written = true;
                    r.line(&format!("screen {w} {h} dpi {s}"));
                }
                let cur = self.app.sys().cursor_pos();
                if r.last_cursor != Some(cur) {
                    r.last_cursor = Some(cur);
                    // `move` runs no frame (U7's expansion): this frame follows it.
                    r.line(&format!("move {} {}", cur.0, cur.1));
                }
                r.pending_frames += 1;
            }
        }
        self.app.on_frame();
        self.report_error();
    }
    fn on_key_down(&mut self, key: Key, double_click: bool) {
        self.rec
            .borrow_mut()
            .line(&format!("key-down {}", key.name()));
        self.app.on_key_down(key, double_click);
        self.report_error();
    }
    fn on_key_up(&mut self, key: Key) {
        self.rec
            .borrow_mut()
            .line(&format!("key-up {}", key.name()));
        self.app.on_key_up(key);
        self.report_error();
    }
    fn on_char(&mut self, ch: Key) {
        // The driver re-creates the OnChar that follows a BACK/TAB/RETURN/ESCAPE KeyDown.
        let extra = matches!(ch, Key::Char(b'\x08' | b'\t' | b'\r' | b'\x1b'));
        if !extra {
            let s = match ch {
                Key::Char(c) if c < 128 => char::from(c).to_string(),
                _ => "?".to_owned(),
            };
            self.rec.borrow_mut().line(&format!("type {}", quote(&s)));
        }
        self.app.on_char(ch);
        self.report_error();
    }
    fn can_exit(&mut self) -> bool {
        self.app.can_exit()
    }
    fn on_exit(&mut self) {
        self.app.on_exit();
        self.rec.borrow().flush();
    }
    fn wants_frames(&self) -> bool {
        true
    }
    fn on_lua_error(&mut self, msg: String) {
        eprintln!("pob-lua-gui: {msg}");
    }
}

fn main() -> anyhow::Result<()> {
    let mut it = std::env::args().skip(1);
    let mut user: Option<PathBuf> = None;
    let mut record: Option<PathBuf> = None;
    while let Some(a) = it.next() {
        match a.as_str() {
            "--user" => {
                user =
                    Some(PathBuf::from(it.next().ok_or_else(|| {
                        anyhow::anyhow!("--user needs a directory")
                    })?));
            }
            "--record" => {
                record = Some(PathBuf::from(
                    it.next()
                        .ok_or_else(|| anyhow::anyhow!("--record needs a file"))?,
                ));
            }
            other => anyhow::bail!(
                "unknown argument {other}\nusage: pob-lua-gui [--user DIR] [--record out.pobscript]"
            ),
        }
    }
    let pob_root = boot::locate_pob_root()
        .ok_or_else(|| anyhow::anyhow!("PoB not found (set POB_ROOT to a .PathOfBuilding dir)"))?;
    let (user_dir, user_path) = if let Some(p) = user {
        std::fs::create_dir_all(&p)?;
        (None, std::fs::canonicalize(&p)?)
    } else {
        let d = UserDir::new(Some(
            &pob_ui_oracle::runner::crate_dir().join("fixtures/user1"),
        ))?;
        let p = d.path.clone();
        (Some(d), p)
    };
    let src = std::fs::canonicalize(pob_root.join("src"))?;
    let runtime = std::fs::canonicalize(pob_root.join("runtime"))?;
    let cfg = HostConfig {
        title: "Path of Building (pob-lua-gui)".into(),
        size: (1280, 800),
        min_size: (800, 600),
        script_dir: src.to_string_lossy().into_owned(),
        sg_runtime: runtime.join("SimpleGraphic").to_string_lossy().into_owned(),
    };
    let rec = Rc::new(RefCell::new(Recorder {
        path: record,
        ..Recorder::default()
    }));
    let rec2 = rec.clone();
    pob_host::run_desktop(cfg, move |parts: PlatformParts| {
        let PlatformParts { sg, mut sys } = parts;
        let inner = std::mem::replace(
            &mut sys.platform,
            Box::new(GuiPlatform {
                inner: Box::new(NullPlatform),
                script_path: String::new(),
                runtime_path: String::new(),
                user_path: String::new(),
            }),
        );
        sys.platform = Box::new(GuiPlatform {
            inner,
            script_path: src.to_string_lossy().into_owned(),
            runtime_path: runtime.to_string_lossy().into_owned(),
            user_path: user_path.to_string_lossy().into_owned(),
        });
        let pob = boot::boot_with(
            &src.to_string_lossy(),
            sg,
            sys,
            &BootOptions {
                entry: Entry::Launch,
                audit: false,
                live: true,
            },
        )
        .unwrap_or_else(|e| {
            eprintln!("pob-lua-gui: boot failed: {e:#}");
            std::process::exit(2);
        });
        Gui {
            app: LuaApp::new(pob),
            rec: rec2,
            _user: user_dir,
        }
    })?;
    rec.borrow().flush();
    Ok(())
}

/// A placeholder while the real platform is moved into [`GuiPlatform`].
struct NullPlatform;

impl Platform for NullPlatform {
    fn time_ms(&self) -> i64 {
        0
    }
    fn copy(&mut self, _text: &str) {}
    fn paste(&mut self) -> Option<String> {
        None
    }
    fn open_url(&mut self, _url: &str) {}
    fn set_window_title(&mut self, _title: &str) {}
    fn take_screenshot(&mut self) {}
    fn restart(&mut self) {}
    fn exit(&mut self, _msg: Option<&str>) {}
    fn script_path(&self) -> String {
        String::new()
    }
    fn runtime_path(&self) -> String {
        String::new()
    }
    fn user_path(&self) -> Result<String, (String, String)> {
        Ok(String::new())
    }
    fn con_print(&mut self, _s: &str) {}
}
