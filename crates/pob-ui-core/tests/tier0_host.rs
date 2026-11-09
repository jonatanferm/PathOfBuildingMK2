//! U4 T0: ProcessControlsInput / SelectControl / GetMouseOverControl / DrawControls / SetFocus
//! against the real ControlHost.lua + Control.lua under LuaJIT (ui doc §11.1 T0).
//!
//! Random scenarios of mock controls whose optional methods exist per instance (Lua: methods
//! assigned on the object; Rust: `Overrides`), each logging every call. The mocks' handlers return
//! `self`, nil or another control (including one outside the host: a child's child), and may have
//! side effects: set `dropped` (imbued early return), clear `selDragActive` (drag lock), remove a
//! control from the host during dispatch, or write `selControl` raw (EditControl.lua:448). Event
//! tables are processed once or twice (holes hide later events from the second loop). After every
//! step the call logs, `selControl`, every `hasFocus` (nil/true/false), the remaining events and
//! any Lua error (a selected control without OnKeyDown) must be identical.
//!
//! The Lua side runs ControlHost.lua with the oracle's patch 6 (`pairs(self.controls)` ->
//! `__canonPairs(self.controls)` at :32 and :110), the canonical order of D7.
#![allow(
    clippy::many_single_char_names,
    clippy::range_plus_one,
    clippy::needless_raw_string_hashes,
    clippy::semicolon_if_nothing_returned
)]

use std::cell::RefCell;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::rc::Rc;

use mlua::{Function, Lua, Table};
use pob_ui_core::layout::{rect, Base};
use pob_ui_core::prelude::*;

fn pob_src() -> PathBuf {
    if let Ok(d) = std::env::var("POB_DIR") {
        return PathBuf::from(d).join("src");
    }
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let src = p.join(".PathOfBuilding/src");
        if src.join("Classes/Control.lua").exists() {
            return src;
        }
        if !p.pop() {
            break;
        }
    }
    // Git worktrees lack the gitignored checkout: fall back to the main checkout, like
    // scripts/port/pobpaths.py main_checkout().
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("PathOfBuilding checkout not found (set POB_DIR)");
    let common = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    let src = common
        .parent()
        .expect("git common dir has a parent")
        .join(".PathOfBuilding/src");
    assert!(
        src.join("Classes/Control.lua").exists(),
        "PathOfBuilding checkout not found (set POB_DIR)"
    );
    src
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn range(&mut self, a: i64, b: i64) -> i64 {
        a + self.below((b - a + 1) as u64) as i64
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
}

// ---------------------------------------------------------------- scenario description

#[derive(Clone, Copy, Debug)]
enum Target {
    Me,
    Nil,
    C(usize),
}
#[derive(Clone, Copy, Debug)]
enum Fx {
    No,
    Drop,
    Undrag,
    Rm(usize),
    RawSel(usize),
}
#[derive(Clone, Copy, Debug)]
struct Resp {
    r: Target,
    fx: Fx,
}
#[derive(Clone, Copy, Debug)]
enum Clip {
    No,
    Val(Option<[i64; 4]>),
    Fn(Option<[i64; 4]>),
}

#[derive(Debug)]
struct MSpec {
    name: String,
    key: Option<CtrlKey>,
    rect: [i64; 4],
    shown: bool,
    imo: bool,
    kd: Option<Vec<Resp>>,
    ku: Option<Vec<Resp>>,
    ch: Option<Vec<Resp>>,
    hku: bool,
    fg: bool,
    fl: bool,
    draw: bool,
    sda: Option<bool>,
    dtl: bool,
    imbued: bool,
    dropped: bool,
    clip: Clip,
    /// Draw removes this host control (removal during the DrawControls loop)
    drawrm: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
struct Ev {
    kind: EventKind,
    key: Key,
}

#[derive(Debug)]
enum Step {
    Process { evs: Vec<Ev>, twice: bool },
    Draw { sel: Option<usize> },
    Select { k: Option<usize> },
    MouseOver,
    Cursor { x: i64, y: i64 },
}

#[derive(Debug)]
struct Scenario {
    specs: Vec<MSpec>,
    vp: [i64; 4],
    cursor: (i64, i64),
    steps: Vec<Step>,
}

const NAMES: [&str; 8] = ["alpha", "b", "c9", "zeta", "m", "q", "edit", "list"];
const KEYS: [Key; 5] = [
    Key::LeftButton,
    Key::RightButton,
    Key::MiddleButton,
    Key::Mouse4,
    Key::Char(b'a'),
];

fn gen_resps(rng: &mut Rng, total: usize, in_host: usize) -> Vec<Resp> {
    (0..1 + rng.below(3))
        .map(|_| {
            let r = match rng.below(100) {
                0..=44 => Target::Me,
                45..=79 => Target::Nil,
                _ => Target::C(1 + rng.below(total as u64) as usize),
            };
            let fx = match rng.below(100) {
                0..=4 => Fx::Drop,
                5..=8 => Fx::Undrag,
                9..=14 => Fx::Rm(1 + rng.below(in_host as u64) as usize),
                15..=19 => Fx::RawSel(1 + rng.below(total as u64) as usize),
                _ => Fx::No,
            };
            Resp { r, fx }
        })
        .collect()
}

fn gen_rect(rng: &mut Rng) -> [i64; 4] {
    [
        rng.range(0, 90),
        rng.range(0, 90),
        rng.range(0, 60),
        rng.range(0, 60),
    ]
}

fn gen_scenario(rng: &mut Rng) -> Scenario {
    let in_host = 1 + rng.below(6) as usize;
    let outside = rng.below(3) as usize;
    let total = in_host + outside;
    let mut names: Vec<&str> = NAMES.to_vec();
    let mut idxs: Vec<i64> = vec![1, 2, 3, 4, 7, 10];
    let mut specs = Vec::new();
    for i in 1..=total {
        let key = if i > in_host {
            None
        } else if rng.chance(40) {
            let k = rng.below(idxs.len() as u64) as usize;
            Some(CtrlKey::Idx(idxs.remove(k)))
        } else {
            let k = rng.below(names.len() as u64) as usize;
            Some(CtrlKey::from(names.remove(k)))
        };
        let clip = match rng.below(100) {
            0..=69 => Clip::No,
            70..=84 => Clip::Val(rng.chance(70).then(|| gen_rect(rng))),
            _ => Clip::Fn(rng.chance(70).then(|| gen_rect(rng))),
        };
        specs.push(MSpec {
            name: format!("c{i}"),
            key,
            rect: gen_rect(rng),
            shown: rng.chance(85),
            imo: rng.chance(75),
            kd: rng.chance(70).then(|| gen_resps(rng, total, in_host)),
            ku: rng.chance(50).then(|| gen_resps(rng, total, in_host)),
            ch: rng.chance(30).then(|| gen_resps(rng, total, in_host)),
            hku: rng.chance(25),
            fg: rng.chance(40),
            fl: rng.chance(40),
            draw: rng.chance(70),
            sda: match rng.below(10) {
                0..=6 => None,
                7 | 8 => Some(true),
                _ => Some(false),
            },
            dtl: rng.chance(30),
            imbued: rng.chance(15),
            dropped: rng.chance(20),
            clip,
            drawrm: rng
                .chance(10)
                .then(|| 1 + rng.below(in_host as u64) as usize),
        });
    }
    let pick = |rng: &mut Rng| rng.chance(80).then(|| 1 + rng.below(total as u64) as usize);
    let mut steps = Vec::new();
    if rng.chance(40) {
        steps.push(Step::Select { k: pick(rng) });
    }
    for _ in 0..2 + rng.below(5) {
        let step = match rng.below(100) {
            0..=59 => {
                let evs = (0..1 + rng.below(5))
                    .map(|_| {
                        let kind = match rng.below(100) {
                            0..=54 => EventKind::KeyDown {
                                double_click: rng.chance(20),
                            },
                            55..=84 => EventKind::KeyUp,
                            _ => EventKind::Char,
                        };
                        let key = if kind == EventKind::Char {
                            Key::Char(if rng.chance(50) { b'x' } else { b'y' })
                        } else if rng.chance(50) {
                            Key::LeftButton
                        } else {
                            KEYS[rng.below(KEYS.len() as u64) as usize]
                        };
                        Ev { kind, key }
                    })
                    .collect();
                Step::Process {
                    evs,
                    twice: rng.chance(25),
                }
            }
            60..=79 => Step::Draw { sel: pick(rng) },
            80..=86 => Step::Select { k: pick(rng) },
            87..=92 => Step::MouseOver,
            _ => Step::Cursor {
                x: rng.range(-5, 160),
                y: rng.range(-5, 160),
            },
        };
        steps.push(step);
    }
    Scenario {
        specs,
        vp: [
            rng.range(-10, 30),
            rng.range(-10, 30),
            rng.range(40, 160),
            rng.range(40, 160),
        ],
        cursor: (rng.range(-5, 160), rng.range(-5, 160)),
        steps,
    }
}

// ---------------------------------------------------------------- Lua side

const LUA_MOCK: &str = r#"
LOG = { }
local function log(s) LOG[#LOG + 1] = s end
local function nm(c) if c == nil then return "nil" end return c.__name end
local function respond(self, list, count)
	local e = list[(count - 1) % #list + 1]
	if e.fx == "drop" then self.dropped = true
	elseif e.fx == "undrag" then self.selDragActive = false
	elseif e.fx == "rm" then HOST.controls[KEYS[e.k]] = nil
	elseif e.fx == "rawsel" then HOST.selControl = C[e.k] end
	if e.r == "self" then return self elseif e.r == "nil" then return nil else return C[e.r] end
end
local MockClass = newClass("Mock", "Control")
function MockClass:Mock(s)
	self:Control(nil, s.rect)
	self.__name = s.name
	self.shown = s.shown
	self.selDragActive = s.sda
	if s.dtl then self.dragTargetList = { } end
	self.imbuedSelect = s.imbued
	self.dropped = s.dropped
	local n = { kd = 0, ku = 0, ch = 0 }
	if s.clip == "fn" then
		self.mouseClipRect = function(c) log(c.__name .. ":clip") return s.cliprect end
	elseif s.clip == "val" then
		self.mouseClipRect = s.cliprect or false
	end
	if s.imo then
		function self:IsMouseOver()
			local r = self:IsMouseInBounds()
			log(self.__name .. ":IsMouseOver=" .. tostring(r))
			return r
		end
	end
	if s.kd then
		function self:OnKeyDown(key, dbl)
			n.kd = n.kd + 1
			local r = respond(self, s.kd, n.kd)
			log(self.__name .. ":OnKeyDown(" .. key .. "," .. tostring(dbl and true or false) .. ")=" .. nm(r))
			return r
		end
	end
	if s.ku then
		function self:OnKeyUp(key)
			n.ku = n.ku + 1
			local r = respond(self, s.ku, n.ku)
			log(self.__name .. ":OnKeyUp(" .. key .. ")=" .. nm(r))
			return r
		end
	end
	if s.ch then
		function self:OnChar(key)
			n.ch = n.ch + 1
			local r = respond(self, s.ch, n.ch)
			log(self.__name .. ":OnChar(" .. key .. ")=" .. nm(r))
			return r
		end
	end
	if s.hku then function self:OnHoverKeyUp(key) log(self.__name .. ":OnHoverKeyUp(" .. key .. ")") end end
	if s.fg then function self:OnFocusGained() log(self.__name .. ":OnFocusGained") end end
	if s.fl then function self:OnFocusLost() log(self.__name .. ":OnFocusLost") end end
	if s.draw then
		function self:Draw(viewPort, noTooltip)
			log(self.__name .. ":Draw(" .. tostring(noTooltip and true or false) .. ")")
			if s.drawrm then HOST.controls[KEYS[s.drawrm]] = nil end
		end
	end
	return self
end
function snapshot()
	local out = { "sel=" .. nm(HOST.selControl) }
	for i = 1, #C do out[#out + 1] = C[i].__name .. "=" .. tostring(C[i].hasFocus) end
	local ev = { }
	for i = 1, NEV do ev[#ev + 1] = EV[i] and "1" or "0" end
	out[#out + 1] = "ev=" .. table.concat(ev)
	return table.concat(out, " ")
end
function logText() return table.concat(LOG, "\n") end
function mouseOver() log("mo=" .. nm(HOST:GetMouseOverControl())) end
"#;

fn lua_resps(v: &[Resp]) -> String {
    let mut s = String::from("{ ");
    for e in v {
        let r = match e.r {
            Target::Me => "\"self\"".to_owned(),
            Target::Nil => "\"nil\"".to_owned(),
            Target::C(k) => k.to_string(),
        };
        let fx = match e.fx {
            Fx::No => String::new(),
            Fx::Drop => ", fx = \"drop\"".to_owned(),
            Fx::Undrag => ", fx = \"undrag\"".to_owned(),
            Fx::Rm(k) => format!(", fx = \"rm\", k = {k}"),
            Fx::RawSel(k) => format!(", fx = \"rawsel\", k = {k}"),
        };
        write!(s, "{{ r = {r}{fx} }}, ").unwrap();
    }
    s.push('}');
    s
}

fn lua_rect(r: [i64; 4]) -> String {
    format!("{{ {}, {}, {}, {} }}", r[0], r[1], r[2], r[3])
}

fn lua_key(k: &CtrlKey) -> String {
    match k {
        CtrlKey::Idx(i) => i.to_string(),
        CtrlKey::Name(s) => format!("\"{s}\""),
    }
}

fn lua_build(sc: &Scenario) -> String {
    let mut s = String::from(
        "LOG = { }\nHOST = new(\"ControlHost\"):ControlHost()\nC = { }\nKEYS = { }\nEV = { }\nNEV = 0\n",
    );
    for (k, m) in sc.specs.iter().enumerate() {
        let i = k + 1;
        let opt = |v: &Option<Vec<Resp>>| v.as_ref().map_or("nil".to_owned(), |v| lua_resps(v));
        let (clip, cliprect) = match m.clip {
            Clip::No => ("nil", None),
            Clip::Val(r) => ("\"val\"", r),
            Clip::Fn(r) => ("\"fn\"", r),
        };
        writeln!(
            s,
            "C[{i}] = new(\"Mock\"):Mock({{ name = \"{}\", rect = {}, shown = {}, imo = {}, kd = {}, ku = {}, ch = {}, \
             hku = {}, fg = {}, fl = {}, draw = {}, sda = {}, dtl = {}, imbued = {}, dropped = {}, clip = {}, cliprect = {}, drawrm = {} }})",
            m.name,
            lua_rect(m.rect),
            m.shown,
            m.imo,
            opt(&m.kd),
            opt(&m.ku),
            opt(&m.ch),
            m.hku,
            m.fg,
            m.fl,
            m.draw,
            m.sda.map_or("nil".to_owned(), |b| b.to_string()),
            m.dtl,
            m.imbued,
            m.dropped,
            clip,
            cliprect.map_or("nil".to_owned(), lua_rect),
            m.drawrm.map_or("nil".to_owned(), |k| k.to_string()),
        )
        .unwrap();
        if let Some(key) = &m.key {
            writeln!(
                s,
                "KEYS[{i}] = {}\nHOST.controls[KEYS[{i}]] = C[{i}]",
                lua_key(key)
            )
            .unwrap();
        }
    }
    writeln!(
        s,
        "VP = {{ x = {}, y = {}, width = {}, height = {} }}\nCURSOR_X, CURSOR_Y = {}, {}",
        sc.vp[0], sc.vp[1], sc.vp[2], sc.vp[3], sc.cursor.0, sc.cursor.1
    )
    .unwrap();
    s
}

fn lua_step(st: &Step) -> String {
    let c = |k: Option<usize>| k.map_or("nil".to_owned(), |k| format!("C[{k}]"));
    match st {
        Step::Process { evs, twice } => {
            let mut s = String::from("EV = { }\n");
            for e in evs {
                let (ty, dbl) = match e.kind {
                    EventKind::KeyDown { double_click } => ("KeyDown", double_click),
                    EventKind::KeyUp => ("KeyUp", false),
                    EventKind::Char => ("Char", false),
                };
                writeln!(
                    s,
                    "table.insert(EV, {{ type = \"{ty}\", key = \"{}\", doubleClick = {dbl} }})",
                    e.key
                )
                .unwrap();
            }
            writeln!(s, "NEV = {}", evs.len()).unwrap();
            s.push_str("HOST:ProcessControlsInput(EV, VP)\n");
            if *twice {
                s.push_str("HOST:ProcessControlsInput(EV, VP)\n");
            }
            s
        }
        Step::Draw { sel } => format!("HOST:DrawControls(VP, {})", c(*sel)),
        Step::Select { k } => format!("HOST:SelectControl({})", c(*k)),
        Step::MouseOver => "mouseOver()".to_owned(),
        Step::Cursor { x, y } => format!("CURSOR_X, CURSOR_Y = {x}, {y}"),
    }
}

// ---------------------------------------------------------------- Rust side

thread_local! {
    static LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}
fn log(s: String) {
    LOG.with(|l| l.borrow_mut().push(s));
}

struct Mock {
    spec: Rc<MSpec>,
    n: [usize; 3],
    sda: Option<bool>,
    dtl: bool,
    imbued: bool,
    dropped: bool,
}
impl Widget for Mock {
    const NAME: &'static str = "Mock";
    const CAPS: Caps = Caps::empty();
    fn duck(cx: &Cx, me: CtrlId, q: Duck) -> bool {
        let m = cx.w::<Mock>(me);
        match q {
            Duck::SelDragActiveWithTargets => m.sda == Some(true) && m.dtl,
            Duck::ImbuedSelectDropped => m.imbued && m.dropped,
        }
    }
}

#[derive(Default)]
struct World {
    host: CtrlId,
    ids: Vec<CtrlId>,
}

fn nm(cx: &Cx, c: Option<CtrlId>) -> String {
    c.map_or("nil".to_owned(), |c| cx.w::<Mock>(c).spec.name.clone())
}

fn respond(cx: &mut Cx, me: CtrlId, w: &World, list: &[Resp], count: usize) -> Focus {
    let e = list[(count - 1) % list.len()];
    match e.fx {
        Fx::No => {}
        Fx::Drop => cx.w_mut::<Mock>(me).dropped = true,
        Fx::Undrag => cx.w_mut::<Mock>(me).sda = Some(false),
        Fx::Rm(k) => {
            let key = cx.w::<Mock>(w.ids[k - 1]).spec.key.clone().unwrap();
            cx.detach_child(w.host, &key);
        }
        Fx::RawSel(k) => cx.host_mut(w.host).sel_control = Some(w.ids[k - 1]),
    }
    match e.r {
        Target::Me => Some(me),
        Target::Nil => None,
        Target::C(k) => Some(w.ids[k - 1]),
    }
}

fn rust_build(cx: &mut Cx, sc: &Scenario) -> Rc<RefCell<World>> {
    let world = Rc::new(RefCell::new(World::default()));
    let host = cx
        .add_free(
            None,
            PlainControl::new(Anchor::NONE, rect(0, 0, 0, 0)).host(),
        )
        .id();
    world.borrow_mut().host = host;
    for m in &sc.specs {
        let spec = Rc::new(MSpec { ..clone_spec(m) });
        let r = m.rect;
        let n = New::new(
            Base::new(
                Anchor::NONE,
                rect(r[0] as f64, r[1] as f64, r[2] as f64, r[3] as f64),
            ),
            Mock {
                spec: spec.clone(),
                n: [0; 3],
                sda: m.sda,
                dtl: m.dtl,
                imbued: m.imbued,
                dropped: m.dropped,
            },
        );
        let id = match &m.key {
            Some(k) => cx.add(host, k.clone(), n).id(),
            None => cx.add_free(None, n).id(),
        };
        world.borrow_mut().ids.push(id);
        install(cx, id, &spec, &world);
    }
    cx.sys
        .begin_frame((sc.cursor.0 as i32, sc.cursor.1 as i32), 1.0);
    world
}

fn clone_spec(m: &MSpec) -> MSpec {
    MSpec {
        name: m.name.clone(),
        key: m.key.clone(),
        rect: m.rect,
        shown: m.shown,
        imo: m.imo,
        kd: m.kd.clone(),
        ku: m.ku.clone(),
        ch: m.ch.clone(),
        hku: m.hku,
        fg: m.fg,
        fl: m.fl,
        draw: m.draw,
        sda: m.sda,
        dtl: m.dtl,
        imbued: m.imbued,
        dropped: m.dropped,
        clip: m.clip,
        drawrm: m.drawrm,
    }
}

fn install(cx: &mut Cx, id: CtrlId, s: &Rc<MSpec>, world: &Rc<RefCell<World>>) {
    cx.base_mut(id).shown = Prop::Val(s.shown);
    let to_rect = |r: Option<[i64; 4]>| r.map(|r| r.map(|v| v as f64));
    match s.clip {
        Clip::No => {}
        Clip::Val(r) => cx.base_mut(id).mouse_clip_rect = Some(Prop::Val(to_rect(r))),
        Clip::Fn(r) => {
            let name = s.name.clone();
            cx.base_mut(id).mouse_clip_rect = Some(Prop::f(move |_, _| {
                log(format!("{name}:clip"));
                to_rect(r)
            }));
        }
    }
    let s = s.clone();
    cx.set_ov(id, |ov| {
        if s.imo {
            let s = s.clone();
            ov.is_mouse_over = Some(Rc::new(move |cx, me| {
                let r = cx.is_mouse_in_bounds(me);
                log(format!("{}:IsMouseOver={r}", s.name));
                r
            }));
        }
        if s.kd.is_some() {
            let (s, w) = (s.clone(), world.clone());
            ov.on_key_down = Some(Rc::new(move |cx, me, key, dbl| {
                let n = {
                    let m = cx.w_mut::<Mock>(me);
                    m.n[0] += 1;
                    m.n[0]
                };
                let r = respond(cx, me, &w.borrow(), s.kd.as_ref().unwrap(), n);
                log(format!("{}:OnKeyDown({key},{dbl})={}", s.name, nm(cx, r)));
                r
            }));
        }
        if s.ku.is_some() {
            let (s, w) = (s.clone(), world.clone());
            ov.on_key_up = Some(Rc::new(move |cx, me, key| {
                let n = {
                    let m = cx.w_mut::<Mock>(me);
                    m.n[1] += 1;
                    m.n[1]
                };
                let r = respond(cx, me, &w.borrow(), s.ku.as_ref().unwrap(), n);
                log(format!("{}:OnKeyUp({key})={}", s.name, nm(cx, r)));
                r
            }));
        }
        if s.ch.is_some() {
            let (s, w) = (s.clone(), world.clone());
            ov.on_char = Some(Rc::new(move |cx, me, key| {
                let n = {
                    let m = cx.w_mut::<Mock>(me);
                    m.n[2] += 1;
                    m.n[2]
                };
                let r = respond(cx, me, &w.borrow(), s.ch.as_ref().unwrap(), n);
                log(format!("{}:OnChar({key})={}", s.name, nm(cx, r)));
                r
            }));
        }
        if s.hku {
            let s = s.clone();
            ov.on_hover_key_up = Some(Rc::new(move |_, _, key| {
                log(format!("{}:OnHoverKeyUp({key})", s.name));
            }));
        }
        if s.fg {
            let s = s.clone();
            ov.on_focus_gained = Some(Rc::new(move |_, _| {
                log(format!("{}:OnFocusGained", s.name))
            }));
        }
        if s.fl {
            let s = s.clone();
            ov.on_focus_lost = Some(Rc::new(move |_, _| log(format!("{}:OnFocusLost", s.name))));
        }
        if s.draw {
            let (s, w) = (s.clone(), world.clone());
            ov.draw = Some(Rc::new(move |cx, _, _, no_tooltip| {
                log(format!("{}:Draw({no_tooltip})", s.name));
                if let Some(k) = s.drawrm {
                    let w = w.borrow();
                    let key = cx.w::<Mock>(w.ids[k - 1]).spec.key.clone().unwrap();
                    cx.detach_child(w.host, &key);
                }
            }));
        }
    });
}

fn rust_snapshot(cx: &Cx, w: &World, ev: &InputEvents, nev: usize) -> String {
    let mut out = vec![format!("sel={}", nm(cx, cx.host(w.host).sel_control))];
    for &id in &w.ids {
        let f = match cx.base(id).has_focus {
            None => "nil",
            Some(true) => "true",
            Some(false) => "false",
        };
        out.push(format!("{}={f}", cx.w::<Mock>(id).spec.name));
    }
    let evs: String = (1..=nev)
        .map(|i| if ev.get(i).is_some() { '1' } else { '0' })
        .collect();
    out.push(format!("ev={evs}"));
    out.join(" ")
}

fn rust_step(cx: &mut Cx, w: &World, st: &Step, ev: &mut InputEvents, nev: &mut usize) {
    let vp = cx.main.view_port;
    let c = |k: Option<usize>| k.map(|k| w.ids[k - 1]);
    match st {
        Step::Process { evs, twice } => {
            *ev = InputEvents::default();
            for e in evs {
                ev.push(InputEvent {
                    kind: e.kind,
                    key: e.key,
                });
            }
            *nev = evs.len();
            cx.process_controls_input(w.host, ev, &vp);
            if *twice {
                cx.process_controls_input(w.host, ev, &vp);
            }
        }
        Step::Draw { sel } => cx.draw_controls(w.host, &vp, c(*sel)),
        Step::Select { k } => cx.select_control(w.host, c(*k)),
        Step::MouseOver => {
            let m = cx.get_mouse_over_control(w.host);
            log(format!("mo={}", nm(cx, m)));
        }
        Step::Cursor { x, y } => cx.sys.begin_frame((*x as i32, *y as i32), 1.0),
    }
}

// ---------------------------------------------------------------- the test

#[test]
fn dispatch_sequences_vs_control_host_lua() {
    // Lua errors raised on purpose (nil OnKeyDown) are compared, not printed.
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if info.payload().downcast_ref::<pob_lua::LuaError>().is_none() {
            prev(info);
        }
    }));

    let lua = Lua::new();
    lua.load("if jit then jit.off() end").exec().unwrap();
    lua.load(include_str!("lua/shim.lua"))
        .set_name("@tests/lua/shim.lua")
        .exec()
        .unwrap();
    let common = std::fs::read_to_string(pob_src().join("Modules/Common.lua")).unwrap();
    let region: Vec<&str> = common.lines().skip(412).take(4).collect(); // Common.lua:413-416
    assert!(region[0].starts_with("function isMouseInRegion"));
    lua.load(region.join("\n")).exec().unwrap();
    let control = std::fs::read_to_string(pob_src().join("Classes/Control.lua")).unwrap();
    lua.load(control)
        .set_name("@Classes/Control.lua")
        .exec()
        .unwrap();
    // Oracle patch 6 (ui doc §11.2): canonical child order at ControlHost.lua:32 and :110.
    let host_src = std::fs::read_to_string(pob_src().join("Classes/ControlHost.lua")).unwrap();
    assert_eq!(host_src.matches("pairs(self.controls)").count(), 2);
    let host_src = host_src.replace("pairs(self.controls)", "__canonPairs(self.controls)");
    lua.load(host_src)
        .set_name("@Classes/ControlHost.lua")
        .exec()
        .unwrap();
    lua.load(LUA_MOCK).exec().unwrap();
    let g: Table = lua.globals();
    let snapshot: Function = g.get("snapshot").unwrap();
    let log_text: Function = g.get("logText").unwrap();

    let mut rng = Rng(0xc0de_0049);
    let scenarios = 1500;
    let (mut steps_total, mut errors, mut calls, mut consumed) = (0, 0, 0, 0);
    for si in 0..scenarios {
        let sc = gen_scenario(&mut rng);
        let mut cx = Cx::headless();
        LOG.with(|l| l.borrow_mut().clear());
        let src = lua_build(&sc);
        lua.load(&src)
            .exec()
            .unwrap_or_else(|e| panic!("scenario {si}: {e}\n{src}"));
        let world = rust_build(&mut cx, &sc);
        cx.main.view_port = ViewPort {
            x: sc.vp[0] as f64,
            y: sc.vp[1] as f64,
            width: sc.vp[2] as f64,
            height: sc.vp[3] as f64,
        };
        let mut ev = InputEvents::default();
        let mut nev = 0usize;
        let mut done = String::new();
        for (k, st) in sc.steps.iter().enumerate() {
            let code = lua_step(st);
            done.push_str(&code);
            done.push('\n');
            let lres = lua.load(&code).exec();
            let rres = pob_lua::error::catch(|| {
                rust_step(&mut cx, &world.borrow(), st, &mut ev, &mut nev)
            });
            steps_total += 1;
            let llog: String = log_text.call(()).unwrap();
            let rlog = LOG.with(|l| l.borrow().join("\n"));
            let ctx = || {
                format!("scenario {si} step {k}\n--- setup\n{src}--- steps\n{done}--- lua log\n{llog}\n--- rust log\n{rlog}")
            };
            assert_eq!(
                lres.is_err(),
                rres.is_err(),
                "error mismatch: lua={lres:?} rust={:?}\n{}",
                rres.err(),
                ctx()
            );
            assert_eq!(llog, rlog, "log mismatch\n{}", ctx());
            if lres.is_err() {
                errors += 1;
                break;
            }
            let ls: String = snapshot.call(()).unwrap();
            let rs = rust_snapshot(&cx, &world.borrow(), &ev, nev);
            assert_eq!(ls, rs, "state mismatch\n{}", ctx());
            if let Step::Process { evs, .. } = st {
                consumed += (1..=evs.len()).filter(|&i| ev.get(i).is_none()).count();
            }
        }
        calls += LOG.with(|l| l.borrow().len());
    }
    eprintln!(
        "tier0_host: {scenarios} scenarios, {steps_total} steps, {calls} logged calls compared, \
         {consumed} events consumed, {errors} Lua errors reproduced, 0 divergences"
    );
    assert!(errors > 0, "the nil-OnKeyDown error path was exercised");
}
