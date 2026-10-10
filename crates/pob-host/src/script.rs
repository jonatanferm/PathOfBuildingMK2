//! The `.pobscript` scenario DSL shared by `run_headless` and the oracle (ui doc §5.2). Owner: U7.
//!
//! ```text
//! screen 1280 800 dpi 1.0
//! fixture userdir=tests/ui-fixtures/user1        # Settings.xml + Builds/
//! load-build "Builds/witch arc.xml"              # == main:SetMode("BUILD", false, name, xml)
//! frames 2
//! move 650 300
//! click LEFTBUTTON                               # move+frame, down+frame, up+frame
//! key-down CTRL ; key-down 6 ; key-up 6 ; key-up CTRL
//! type "Hello ^1world"                           # per byte: Char (+KeyDown/KeyUp for named keys), as SG delivers
//! press TAB                                      # KeyDown TAB + Char "\t" + frame + KeyUp TAB
//! wheel down 3                                   # 3x (KeyDown+KeyUp WHEELDOWN)
//! paste-clipboard "..."                          # sets the virtual clipboard
//! frames 3
//! snapshot notes_typed                           # dumps build XML from both sides too
//! ```
//!
//! # Syntax
//!
//! One or more steps per line, separated by `;`; `#` starts a comment. Arguments are words or
//! `"quoted strings"` (escapes `\\ \" \n \r \t \b \e \xHH` (ASCII) `\u{HEX}`); quoted and bare
//! parts of one word concatenate (`userdir="a b"`). Keys are SG key names (`CTRL`, `F5`,
//! `LEFTBUTTON`, case-insensitive) or one character taken verbatim (`6`, `a`, `" "`; SG delivers
//! letters lowercase).
//!
//! # Steps and their expansion ([`Script::expand`])
//!
//! | step | expands to |
//! |---|---|
//! | `screen W H [dpi S]` | the framebuffer size (physical px) and OS content scale of the following frames (default dpi 1) |
//! | `fixture KEY=VALUE` | host configuration (`userdir`, `net`; see `crate::headless`); other keys go to the app |
//! | `load-build "path"` | an app step (`App::on_script_step`) |
//! | `frames N` | N frames |
//! | `move X Y` | the cursor moves to virtual (X, Y); no frame |
//! | `click KEY` | frame, KeyDown KEY, frame, KeyUp KEY, frame |
//! | `key-down KEY` | KeyDown KEY (+ the extra Char `\b` `\t` `\r` `\x1b` for BACK/TAB/RETURN/ESCAPE); no frame |
//! | `key-up KEY` | KeyUp KEY; no frame |
//! | `press KEY` | `key-down KEY`, frame, `key-up KEY` |
//! | `type "text"` | per codepoint: `\b` `\t` `\r` `\x1b` are KeyDown+Char+KeyUp of BACK/TAB/RETURN/ESCAPE; other codepoints one Char as SG delivers it (`input::char_key`: control characters dropped, U+00A0..U+00AA as key names, other non-ASCII `?`); no frame |
//! | `wheel up\|down [N]` | N × (KeyDown+KeyUp WHEELUP/WHEELDOWN), default 1; no frame |
//! | `paste-clipboard "text"` | sets the virtual clipboard |
//! | `snapshot NAME` | an app step (`App::on_script_step`) |
//! | `exit` | `CanExit`, then `OnExit` when it allows; the run ends |
//!
//! Mouse-button KeyDowns become double clicks when the host's `DoubleClick` says so (virtual
//! clock, cursor position), as in SG.

use std::fmt::Write as _;

use pob_sg::Key;

use crate::input::{char_key, Input};

/// One scenario step.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// `screen W H dpi S`
    Screen { w: i32, h: i32, dpi: f32 },
    /// `fixture key=value`
    Fixture { key: String, value: String },
    /// `load-build "path"`
    LoadBuild(String),
    /// `frames N`
    Frames(u32),
    /// `move X Y` (virtual px)
    Move(f64, f64),
    /// `click KEY`
    Click(Key),
    /// `key-down KEY`
    KeyDown(Key),
    /// `key-up KEY`
    KeyUp(Key),
    /// `type "text"`
    Type(String),
    /// `press KEY`
    Press(Key),
    /// `wheel up|down N`
    Wheel { up: bool, count: u32 },
    /// `paste-clipboard "text"`
    PasteClipboard(String),
    /// `snapshot NAME`
    Snapshot(String),
    /// `exit`
    Exit,
}

/// A parsed `.pobscript`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Script {
    pub steps: Vec<Step>,
    /// The directory relative fixture paths resolve against ([`Script::load`] sets it to the
    /// script's directory; `None`: the process working directory).
    pub base_dir: Option<std::path::PathBuf>,
}

/// One primitive host action: what a [`Step`] expands to.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Screen {
        w: i32,
        h: i32,
        dpi: f32,
    },
    Fixture {
        key: String,
        value: String,
    },
    /// Run one frame.
    Frame,
    /// Move the cursor (virtual px).
    Move(f64, f64),
    /// Deliver one SG input event.
    Input(Input),
    PasteClipboard(String),
    /// A step only the app can perform (`load-build`, `snapshot`).
    App(Step),
    Exit,
}

/// One token: its text after unquoting.
type Tok = String;

/// Split one line into statements of tokens.
fn tokenize(line: &str) -> Result<Vec<Vec<Tok>>, String> {
    let mut stmts: Vec<Vec<Tok>> = vec![Vec::new()];
    let mut cur: Option<String> = None;
    let mut chars = line.chars().peekable();
    let flush = |cur: &mut Option<String>, stmts: &mut Vec<Vec<Tok>>| {
        if let Some(t) = cur.take() {
            stmts.last_mut().expect("one statement").push(t);
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '#' => break,
            ';' => {
                flush(&mut cur, &mut stmts);
                stmts.push(Vec::new());
            }
            c if c.is_whitespace() => flush(&mut cur, &mut stmts),
            '"' => {
                let tok = cur.get_or_insert_with(String::new);
                loop {
                    let Some(c) = chars.next() else {
                        return Err("unterminated string".into());
                    };
                    match c {
                        '"' => break,
                        '\\' => tok.push(unescape(&mut chars)?),
                        c => tok.push(c),
                    }
                }
            }
            c => cur.get_or_insert_with(String::new).push(c),
        }
    }
    flush(&mut cur, &mut stmts);
    stmts.retain(|s| !s.is_empty());
    Ok(stmts)
}

fn unescape(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Result<char, String> {
    let Some(c) = chars.next() else {
        return Err("unterminated escape".into());
    };
    Ok(match c {
        '\\' => '\\',
        '"' => '"',
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        'b' => '\x08',
        'e' => '\x1b',
        'x' => {
            let h: String = chars.by_ref().take(2).collect();
            let v = u8::from_str_radix(&h, 16).map_err(|_| format!("bad escape \\x{h}"))?;
            if v >= 0x80 {
                return Err(format!("\\x{h} is not ASCII (use \\u{{..}})"));
            }
            char::from(v)
        }
        'u' => {
            if chars.next() != Some('{') {
                return Err("expected \\u{HEX}".into());
            }
            let mut h = String::new();
            loop {
                match chars.next() {
                    Some('}') => break,
                    Some(c) => h.push(c),
                    None => return Err("unterminated \\u{".into()),
                }
            }
            u32::from_str_radix(&h, 16)
                .ok()
                .and_then(char::from_u32)
                .ok_or_else(|| format!("bad escape \\u{{{h}}}"))?
        }
        c => return Err(format!("unknown escape \\{c}")),
    })
}

/// Quote `s` so that [`tokenize`] gives it back.
fn quoted(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            '\x08' => o.push_str("\\b"),
            '\x1b' => o.push_str("\\e"),
            c if (c as u32) < 0x80 && c.is_control() => {
                let _ = write!(o, "\\x{:02X}", c as u32);
            }
            c if c.is_control() => {
                let _ = write!(o, "\\u{{{:X}}}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// A bare word when that round-trips, else [`quoted`].
fn word(s: &str) -> String {
    let bare = !s.is_empty()
        && s.chars()
            .all(|c| !c.is_whitespace() && !c.is_control() && !matches!(c, ';' | '#' | '"' | '\\'));
    if bare {
        s.to_owned()
    } else {
        quoted(s)
    }
}

/// A key argument: a name (`CTRL`, `F5`, case-insensitive) or one character, verbatim.
fn parse_key(s: &str) -> Result<Key, String> {
    let mut it = s.chars();
    if let (Some(c), None) = (it.next(), it.next()) {
        if c.is_ascii() {
            return Ok(Key::Char(c as u8));
        }
        return Err(format!("key {s:?} is not ASCII"));
    }
    match Key::from_name(s) {
        Some(k) => Ok(k),
        None => Err(format!("unknown key {s:?}")),
    }
}

fn key_word(k: Key) -> String {
    word(&k.name())
}

fn is_mouse_key(k: Key) -> bool {
    matches!(
        k,
        Key::LeftButton | Key::MiddleButton | Key::RightButton | Key::Mouse4 | Key::Mouse5
    )
}

/// The extra char that follows KeyDown of BACK/TAB/RETURN/ESCAPE (`GlfwKeyExtraChar`).
fn extra_char_of(k: Key) -> Option<Key> {
    match k {
        Key::Back => Some(Key::Char(b'\x08')),
        Key::Tab => Some(Key::Char(b'\t')),
        Key::Return => Some(Key::Char(b'\r')),
        Key::Escape => Some(Key::Char(0x1b)),
        _ => None,
    }
}

fn parse_step(t: &[Tok]) -> Result<Step, String> {
    let n = |i: usize| -> Result<&str, String> {
        t.get(i)
            .map(String::as_str)
            .ok_or_else(|| format!("`{}`: missing argument {i}", t[0]))
    };
    let num = |i: usize| -> Result<f64, String> {
        let s = n(i)?;
        s.parse::<f64>()
            .map_err(|_| format!("`{}`: {s:?} is not a number", t[0]))
    };
    let int = |i: usize| -> Result<i64, String> {
        let s = n(i)?;
        s.parse::<i64>()
            .map_err(|_| format!("`{}`: {s:?} is not an integer", t[0]))
    };
    let arity = |k: usize| -> Result<(), String> {
        if t.len() == k + 1 {
            Ok(())
        } else {
            Err(format!(
                "`{}` takes {k} argument(s), got {}",
                t[0],
                t.len() - 1
            ))
        }
    };
    let step = match t[0].as_str() {
        "screen" => {
            let dpi = match t.len() {
                3 => 1.0,
                5 if t[3] == "dpi" => n(4)?
                    .parse::<f32>()
                    .map_err(|_| format!("`screen`: bad dpi {:?}", t[4]))?,
                _ => return Err("usage: screen W H [dpi S]".into()),
            };
            let w = i32::try_from(int(1)?).map_err(|e| e.to_string())?;
            let h = i32::try_from(int(2)?).map_err(|e| e.to_string())?;
            Step::Screen { w, h, dpi }
        }
        "fixture" => {
            arity(1)?;
            let (k, v) = t[1]
                .split_once('=')
                .ok_or_else(|| format!("`fixture`: expected KEY=VALUE, got {:?}", t[1]))?;
            if k.is_empty() {
                return Err("`fixture`: empty key".into());
            }
            Step::Fixture {
                key: k.to_owned(),
                value: v.to_owned(),
            }
        }
        "load-build" => {
            arity(1)?;
            Step::LoadBuild(t[1].clone())
        }
        "frames" => {
            arity(1)?;
            Step::Frames(u32::try_from(int(1)?).map_err(|e| e.to_string())?)
        }
        "move" => {
            arity(2)?;
            Step::Move(num(1)?, num(2)?)
        }
        "click" => {
            arity(1)?;
            Step::Click(parse_key(&t[1])?)
        }
        "key-down" => {
            arity(1)?;
            Step::KeyDown(parse_key(&t[1])?)
        }
        "key-up" => {
            arity(1)?;
            Step::KeyUp(parse_key(&t[1])?)
        }
        "press" => {
            arity(1)?;
            Step::Press(parse_key(&t[1])?)
        }
        "type" => {
            arity(1)?;
            Step::Type(t[1].clone())
        }
        "wheel" => {
            if t.len() != 2 && t.len() != 3 {
                return Err("usage: wheel up|down [N]".into());
            }
            let up = match t[1].as_str() {
                "up" => true,
                "down" => false,
                s => return Err(format!("`wheel`: expected up|down, got {s:?}")),
            };
            let count = if t.len() == 3 {
                u32::try_from(int(2)?).map_err(|e| e.to_string())?
            } else {
                1
            };
            Step::Wheel { up, count }
        }
        "paste-clipboard" => {
            arity(1)?;
            Step::PasteClipboard(t[1].clone())
        }
        "snapshot" => {
            arity(1)?;
            Step::Snapshot(t[1].clone())
        }
        "exit" => {
            arity(0)?;
            Step::Exit
        }
        s => return Err(format!("unknown step {s:?}")),
    };
    Ok(step)
}

impl Script {
    /// Parse a `.pobscript` (`#` comments, `;` separates steps on one line).
    pub fn parse(text: &str) -> Result<Script, String> {
        let mut steps = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let stmts = tokenize(line).map_err(|e| format!("line {}: {e}", i + 1))?;
            for st in stmts {
                steps.push(parse_step(&st).map_err(|e| format!("line {}: {e}", i + 1))?);
            }
        }
        Ok(Script {
            steps,
            base_dir: None,
        })
    }

    /// Read and parse a file; relative fixture paths resolve against its directory.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load(path: &std::path::Path) -> Result<Script, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut s = Script::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        s.base_dir = path.parent().map(std::path::Path::to_path_buf);
        Ok(s)
    }

    /// Render back to text (round-trip tests): one step per line.
    pub fn to_text(&self) -> String {
        let mut o = String::new();
        for s in &self.steps {
            let line = match s {
                Step::Screen { w, h, dpi } => format!("screen {w} {h} dpi {dpi}"),
                Step::Fixture { key, value } => {
                    format!("fixture {}", word(&format!("{key}={value}")))
                }
                Step::LoadBuild(p) => format!("load-build {}", quoted(p)),
                Step::Frames(n) => format!("frames {n}"),
                Step::Move(x, y) => format!("move {x} {y}"),
                Step::Click(k) => format!("click {}", key_word(*k)),
                Step::KeyDown(k) => format!("key-down {}", key_word(*k)),
                Step::KeyUp(k) => format!("key-up {}", key_word(*k)),
                Step::Type(t) => format!("type {}", quoted(t)),
                Step::Press(k) => format!("press {}", key_word(*k)),
                Step::Wheel { up, count } => {
                    format!("wheel {} {count}", if *up { "up" } else { "down" })
                }
                Step::PasteClipboard(t) => format!("paste-clipboard {}", quoted(t)),
                Step::Snapshot(n) => format!("snapshot {}", word(n)),
                Step::Exit => "exit".to_owned(),
            };
            o.push_str(&line);
            o.push('\n');
        }
        o
    }

    /// The primitive actions of the whole script, in order (the table in the module docs).
    pub fn expand(&self) -> Vec<Action> {
        let mut out = Vec::new();
        for s in &self.steps {
            expand_step(s, &mut out);
        }
        out
    }
}

fn key_down(k: Key, out: &mut Vec<Action>) {
    out.push(Action::Input(Input::Key {
        key: k,
        down: true,
        double_click: false,
    }));
    if let Some(c) = extra_char_of(k) {
        out.push(Action::Input(Input::Char(c)));
    }
}

fn key_up(k: Key, out: &mut Vec<Action>) {
    out.push(Action::Input(Input::Key {
        key: k,
        down: false,
        double_click: false,
    }));
}

/// One step's actions.
pub fn expand_step(s: &Step, out: &mut Vec<Action>) {
    match s {
        Step::Screen { w, h, dpi } => out.push(Action::Screen {
            w: *w,
            h: *h,
            dpi: *dpi,
        }),
        Step::Fixture { key, value } => out.push(Action::Fixture {
            key: key.clone(),
            value: value.clone(),
        }),
        Step::LoadBuild(_) | Step::Snapshot(_) => out.push(Action::App(s.clone())),
        Step::Frames(n) => out.extend((0..*n).map(|_| Action::Frame)),
        Step::Move(x, y) => out.push(Action::Move(*x, *y)),
        Step::Click(k) => {
            out.push(Action::Frame);
            key_down(*k, out);
            out.push(Action::Frame);
            key_up(*k, out);
            out.push(Action::Frame);
        }
        Step::KeyDown(k) => key_down(*k, out),
        Step::KeyUp(k) => key_up(*k, out),
        Step::Press(k) => {
            key_down(*k, out);
            out.push(Action::Frame);
            key_up(*k, out);
        }
        Step::Type(text) => {
            for c in text.chars() {
                let named = match c {
                    '\x08' => Some(Key::Back),
                    '\t' => Some(Key::Tab),
                    '\r' => Some(Key::Return),
                    '\x1b' => Some(Key::Escape),
                    _ => None,
                };
                if let Some(k) = named {
                    key_down(k, out);
                    key_up(k, out);
                } else if let Some(k) = char_key(c as u32) {
                    out.push(Action::Input(Input::Char(k)));
                }
            }
        }
        Step::Wheel { up, count } => {
            out.extend((0..*count).map(|_| Action::Input(Input::Wheel { up: *up })));
        }
        Step::PasteClipboard(t) => out.push(Action::PasteClipboard(t.clone())),
        Step::Exit => out.push(Action::Exit),
    }
}

/// Whether `key` is a mouse button (the host applies double-click detection to its KeyDown).
pub fn is_mouse_button(key: Key) -> bool {
    is_mouse_key(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC_EXAMPLE: &str = r#"screen 1280 800 dpi 1.0
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
"#;

    #[test]
    fn doc_example_parses() {
        let s = Script::parse(DOC_EXAMPLE).unwrap();
        assert_eq!(
            s.steps,
            vec![
                Step::Screen {
                    w: 1280,
                    h: 800,
                    dpi: 1.0
                },
                Step::Fixture {
                    key: "userdir".into(),
                    value: "tests/ui-fixtures/user1".into()
                },
                Step::LoadBuild("Builds/witch arc.xml".into()),
                Step::Frames(2),
                Step::Move(650.0, 300.0),
                Step::Click(Key::LeftButton),
                Step::KeyDown(Key::Ctrl),
                Step::KeyDown(Key::Char(b'6')),
                Step::KeyUp(Key::Char(b'6')),
                Step::KeyUp(Key::Ctrl),
                Step::Type("Hello ^1world".into()),
                Step::Press(Key::Tab),
                Step::Wheel {
                    up: false,
                    count: 3
                },
                Step::PasteClipboard("...".into()),
                Step::Frames(3),
                Step::Snapshot("notes_typed".into()),
            ]
        );
        let again = Script::parse(&s.to_text()).unwrap();
        assert_eq!(again, s);
    }

    #[test]
    fn syntax_details() {
        let s = Script::parse(
            "fixture userdir=\"a b\";key-down ctrl;key-up \" \" ; type \"a;b#c\\t\\u{e9}\\x41\\e\" # c\nwheel up\nscreen 1920 1080\nexit",
        )
        .unwrap();
        assert_eq!(
            s.steps,
            vec![
                Step::Fixture {
                    key: "userdir".into(),
                    value: "a b".into()
                },
                Step::KeyDown(Key::Ctrl),
                Step::KeyUp(Key::Char(b' ')),
                Step::Type("a;b#c\t\u{e9}A\x1b".into()),
                Step::Wheel { up: true, count: 1 },
                Step::Screen {
                    w: 1920,
                    h: 1080,
                    dpi: 1.0
                },
                Step::Exit,
            ]
        );
        for bad in [
            "frames",
            "frames x",
            "frames -1",
            "bogus 1",
            "type \"abc",
            "type \"\\q\"",
            "type \"\\x80\"",
            "key-down NOPE",
            "key-down é",
            "fixture nokey",
            "fixture =v",
            "wheel sideways 1",
            "screen 1 2 3",
            "exit now",
            "move 1",
        ] {
            assert!(Script::parse(bad).is_err(), "{bad}");
        }
    }

    /// A small deterministic generator (no proptest in the workspace).
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
        fn text(&mut self) -> String {
            const ALPHABET: &[char] = &[
                'a', 'Z', ' ', ';', '#', '"', '\\', '\n', '\r', '\t', '\x08', '\x1b', '\x01',
                '\x7f', '^', '=', 'é', '£', '\u{85}', '\u{2192}', '0',
            ];
            let len = self.below(8);
            (0..len)
                .map(|_| ALPHABET[self.below(ALPHABET.len() as u64) as usize])
                .collect()
        }
        fn key(&mut self) -> Key {
            match self.below(4) {
                0 => Key::F(1 + self.below(15) as u8),
                1 => [
                    Key::LeftButton,
                    Key::Ctrl,
                    Key::Return,
                    Key::WheelDown,
                    Key::ScrollLock,
                ][self.below(5) as usize],
                _ => Key::Char(self.below(128) as u8),
            }
        }
        fn step(&mut self) -> Step {
            match self.below(14) {
                0 => Step::Screen {
                    w: self.below(4000) as i32 - 100,
                    h: self.below(3000) as i32,
                    dpi: [1.0f32, 1.25, 1.5, 0.75, 2.0][self.below(5) as usize],
                },
                1 => {
                    let key: String = self.text().chars().filter(|c| *c != '=').collect();
                    let key = if key.is_empty() { "k".to_owned() } else { key };
                    Step::Fixture {
                        key,
                        value: self.text(),
                    }
                }
                2 => Step::LoadBuild(self.text()),
                3 => Step::Frames(self.below(100) as u32),
                4 => Step::Move(
                    (self.below(20000) as f64 - 1000.0) / 8.0,
                    self.below(1_000_000) as f64 / 3.0,
                ),
                5 => Step::Click(self.key()),
                6 => Step::KeyDown(self.key()),
                7 => Step::KeyUp(self.key()),
                8 => Step::Type(self.text()),
                9 => Step::Press(self.key()),
                10 => Step::Wheel {
                    up: self.below(2) == 0,
                    count: self.below(10) as u32,
                },
                11 => Step::PasteClipboard(self.text()),
                12 => Step::Snapshot({
                    let t = self.text();
                    if t.is_empty() {
                        "s".into()
                    } else {
                        t
                    }
                }),
                _ => Step::Exit,
            }
        }
    }

    /// `.pobscript` parser round trip: parse(to_text(s)) == s for 2000 random scripts.
    #[test]
    fn round_trip_random() {
        let mut g = Lcg(0x5eed);
        for i in 0..2000 {
            let n = g.below(12);
            let s = Script {
                steps: (0..n).map(|_| g.step()).collect(),
                base_dir: None,
            };
            let text = s.to_text();
            let back = Script::parse(&text).unwrap_or_else(|e| panic!("#{i}: {e}\n{text}"));
            assert_eq!(back, s, "#{i}:\n{text}");
            assert_eq!(back.to_text(), text);
        }
    }

    #[test]
    fn expansion() {
        use Action::{Frame, Input as I};
        let k = |key, down| {
            I(Input::Key {
                key,
                down,
                double_click: false,
            })
        };
        let s = Script::parse("click LEFTBUTTON\npress TAB\nkey-down RETURN\nkey-up RETURN\ntype \"a\\tb\\n£é\"\nwheel down 2\nframes 2\nkey-down a").unwrap();
        assert_eq!(
            s.expand(),
            vec![
                Frame,
                k(Key::LeftButton, true),
                Frame,
                k(Key::LeftButton, false),
                Frame,
                k(Key::Tab, true),
                I(Input::Char(Key::Char(b'\t'))),
                Frame,
                k(Key::Tab, false),
                k(Key::Return, true),
                I(Input::Char(Key::Char(b'\r'))),
                k(Key::Return, false),
                I(Input::Char(Key::Char(b'a'))),
                k(Key::Tab, true),
                I(Input::Char(Key::Char(b'\t'))),
                k(Key::Tab, false),
                I(Input::Char(Key::Char(b'b'))),
                // '\n' is a control character: GLFW drops it
                I(Input::Char(Key::F(10))),   // '£'
                I(Input::Char(Key::Unknown)), // 'é'
                I(Input::Wheel { up: false }),
                I(Input::Wheel { up: false }),
                Frame,
                Frame,
                k(Key::Char(b'a'), true),
            ]
        );
        assert!(is_mouse_button(Key::Mouse5) && !is_mouse_button(Key::WheelUp));
    }
}
