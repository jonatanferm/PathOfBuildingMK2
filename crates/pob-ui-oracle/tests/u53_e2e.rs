//! U53 acceptance, end to end (ui doc §12.7 U53 row, §11.3): the whole app on both sides of the
//! UI oracle with **no stand-ins**: unmodified PoB Lua under LuaJIT, and `pob_app::PobApp` with
//! its default hooks and the **Rust calc** (`RustCalc`, calc:PWR/CA; no LuaCalcBridge). Owner: U53.
//!
//! `u53_e2e`: every `scenarios/e2e/*.pobscript` at 1280x800 @1.0 and 1920x1080 @1.5 (a script
//! that starts with its own `screen` step runs once). The scripts start at the build list of
//! `fixtures/user1` or open one of its builds and drive every tab. Per run: zero T3 divergences
//! (draw ops per `(layer, sub)` bucket and side effects, frame by frame), the same window titles,
//! no error on either side, the `#! expect-text` lines of `tests/parity.rs`,
//! and T4 at every `snapshot` step and for every file named by `#! e2e-saved FILE` (written by
//! Ctrl+S), after the normalisation below.
//!
//! What is not PoB's on one side (both are oracle limits, not stand-ins for unported code):
//! * the oracle's `lua-utf8` lacks `utf8.next` (CCR-0033 item 4): luautf8's `Lutf8_next` is
//!   installed on the Lua side, as every app unit's test does (u22); the oracle's VM lacks the
//!   `debug` library, so `debug.traceback` is installed (see [`LUA_UTF8_NEXT`]);
//! * `launch.force_release_install` on the Rust side is pob-ui-oracle patch 4's equivalent
//!   (CCR-0091), and the Rust platform records the side effects the Lua side's bridge records
//!   (CCR-0028 item 2).
//!
//! `ImageLoad` effects are not compared (as in u16-u50): the oracle records only the Lua side's
//! loads (its `NewImageHandle` bridge), not `pob_sg::ImageHandle::load`; the images are covered by
//! the draws, each of which names its image path and flags.
//!
//! T4 normalisation (u20's rules, see u20/u22): attributes as sets; the top-level sections as a
//! multiset (`pairs(self.savers)`); the children of `Calcs`, `ConfigSet`, `ItemSet`, `Sockets`
//! and `Overrides` as multisets; the `pairs`-chosen gem variants not compared. Stricter than u20:
//! the Lua `nil` gem/group fields are compared as written (the model keeps them, CCR-0019 §6), and
//! every saver is ported now, so no section is set aside.
//!
//! Masks: none. A text mask would have to name an open calc issue (ui doc §11.3); there is none
//! (docs/port/STATUS.md: INT-1 exact on all57 and on the fresh-100 corpus).
//!
//! Filters: `U53_FILTER=<substring of the script stem>`.

#![allow(clippy::too_many_lines)]

use std::cell::RefCell;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_app::app::CxApp;
use pob_calc::model::{XmlChild, XmlNode};
use pob_host::{App, PlatformParts};
use pob_sg::trace::{ApiCall, Buckets, DiffConfig, Divergence, DrawOp, FrameTrace, SideEffect};
use pob_sg::{Key, Sg, Sys};
use pob_ui_oracle::boot::{self, Entry, OracleHandles};
use pob_ui_oracle::diff_report;
use pob_ui_oracle::runner::{self, Driver, LuaApp, RunResult, ScenarioApp, UserDir};

const SCREENS: [(i32, i32, f32); 2] = [(1280, 800, 1.0), (1920, 1080, 1.5)];

fn inputs() -> Option<PathBuf> {
    let root = pob_testpaths::require(pob_testpaths::pob_root(), "PoB checkout (.PathOfBuilding)")?;
    let dump = pob_testpaths::require(pob_testpaths::dump_dir(), "data/dump")?;
    assert!(pob_oracle::testkit::init_data(&dump), "data dump");
    let src = std::fs::canonicalize(root.join("src")).unwrap_or_else(|_| root.join("src"));
    pob_gamedata::timeless_lut::set_jewel_fs(std::sync::Arc::new(
        pob_gamedata::timeless_lut::StdJewelFs {
            script_path: src.to_string_lossy().into_owned(),
            writable: false,
        },
    ));
    Some(root)
}

fn scenario_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("scenarios/e2e")
}

/// The oracle's `lua-utf8` stand-in has no `utf8.next` (EditControl.lua:558/:572; CCR-0033
/// item 4): luautf8's `Lutf8_next` for EditControl's call shape (as u22).
///
/// Also the oracle's VM has no `debug` library (mlua's safe mode; pob-oracle `jit.rs`), which
/// real PoB's LuaJIT has: ItemsTab.lua:4175 passes `debug.traceback` to `xpcall` as the message
/// handler. It is installed with LuaJIT's behaviour for a string message without the stack
/// lines (the handler only runs when `BuildAllDependsAndPaths` raises).
const LUA_UTF8_NEXT: &str = r#"
if not debug then
	debug = { traceback = function(msg) return msg end }
end
local lutf8 = require("lua-utf8")
if not lutf8.next then
	local function iscont(s, p)
		local b = s:byte(p + 1) or 0
		return b >= 0x80 and b < 0xC0
	end
	lutf8.next = function(s, i, off)
		local e = #s
		local p = i - 1
		local idx = off
		if idx >= 0 then
			while p < e and idx > 0 do
				while p < e and iscont(s, p + 1) do
					p = p + 1
				end
				p = (p < e) and p + 1 or e
				idx = idx - 1
			end
		else
			while 0 < p and idx < 0 do
				local q = p
				while 0 < q and iscont(s, q - 1) do
					q = q - 1
				end
				p = (0 < q) and q - 1 or 0
				idx = idx + 1
			end
		end
		if idx ~= 0 or p == e then
			return
		end
		return p + 1, s:byte(p + 1)
	end
end
"#;

/// `#! NAME ARG` directives.
fn directives(text: &str, name: &str) -> Vec<String> {
    let tag = format!("#! {name} ");
    text.lines()
        .filter_map(|l| l.trim().strip_prefix(tag.as_str()))
        .map(|s| s.trim().to_owned())
        .collect()
}

// ---- the Lua side ----

struct LuaSide {
    app: LuaApp,
}

impl App for LuaSide {
    fn sg(&mut self) -> &mut Sg {
        self.app.sg()
    }
    fn sys(&mut self) -> &mut Sys {
        self.app.sys()
    }
    fn on_init(&mut self) {
        self.app.on_init();
        self.app
            .with_vm(|pob| pob.eval::<()>(LUA_UTF8_NEXT))
            .unwrap_or_else(|e| panic!("utf8.next: {e}"));
    }
    fn on_frame(&mut self) {
        self.app.on_frame();
    }
    fn on_key_down(&mut self, key: Key, double_click: bool) {
        self.app.on_key_down(key, double_click);
    }
    fn on_key_up(&mut self, key: Key) {
        self.app.on_key_up(key);
    }
    fn on_char(&mut self, ch: Key) {
        self.app.on_char(ch);
    }
    fn on_lua_error(&mut self, msg: String) {
        self.app.on_lua_error(msg);
    }
}

impl ScenarioApp for LuaSide {
    fn load_build(&mut self, name: &str, xml: &str) -> anyhow::Result<()> {
        self.app.load_build(name, xml)
    }
    fn snapshot(&mut self) -> anyhow::Result<Option<String>> {
        self.app.snapshot()
    }
    fn after_frame(&mut self) -> anyhow::Result<()> {
        self.app.after_frame()
    }
    fn take_error(&mut self) -> Option<String> {
        self.app.take_error()
    }
}

// ---- the Rust side ----

/// Records the platform side effects the Lua side's bridge records (CCR-0028 item 2), as u16-u50.
struct RecordingPlatform {
    inner: Box<dyn pob_sg::Platform>,
    effects: Rc<RefCell<Vec<SideEffect>>>,
}

impl RecordingPlatform {
    fn push(&self, e: SideEffect) {
        self.effects.borrow_mut().push(e);
    }
}

impl pob_sg::Platform for RecordingPlatform {
    fn time_ms(&self) -> i64 {
        self.inner.time_ms()
    }
    fn copy(&mut self, text: &str) {
        self.push(SideEffect::Copy(text.to_owned()));
        self.inner.copy(text);
    }
    fn paste(&mut self) -> Option<String> {
        self.inner.paste()
    }
    fn open_url(&mut self, url: &str) {
        self.push(SideEffect::OpenUrl(url.to_owned()));
        self.inner.open_url(url);
    }
    fn set_window_title(&mut self, title: &str) {
        self.push(SideEffect::WindowTitle(title.to_owned()));
        self.inner.set_window_title(title);
    }
    fn take_screenshot(&mut self) {
        self.push(SideEffect::Screenshot);
        self.inner.take_screenshot();
    }
    fn restart(&mut self) {
        self.inner.restart();
    }
    fn exit(&mut self, msg: Option<&str>) {
        self.push(SideEffect::Exit);
        self.inner.exit(msg);
    }
    fn script_path(&self) -> String {
        self.inner.script_path()
    }
    fn runtime_path(&self) -> String {
        self.inner.runtime_path()
    }
    fn user_path(&self) -> Result<String, (String, String)> {
        self.inner.user_path()
    }
    fn con_print(&mut self, s: &str) {
        self.inner.con_print(s);
    }
}

struct NullPlatform;
impl pob_sg::Platform for NullPlatform {
    fn time_ms(&self) -> i64 {
        0
    }
    fn copy(&mut self, _: &str) {}
    fn paste(&mut self) -> Option<String> {
        None
    }
    fn open_url(&mut self, _: &str) {}
    fn set_window_title(&mut self, _: &str) {}
    fn take_screenshot(&mut self) {}
    fn restart(&mut self) {}
    fn exit(&mut self, _: Option<&str>) {}
    fn script_path(&self) -> String {
        String::new()
    }
    fn runtime_path(&self) -> String {
        String::new()
    }
    fn user_path(&self) -> Result<String, (String, String)> {
        Ok(String::new())
    }
    fn con_print(&mut self, _: &str) {}
}

struct RustSide {
    app: pob_app::PobApp,
    effects: Rc<RefCell<Vec<SideEffect>>>,
}

impl RustSide {
    /// `PobApp::new`: the shipping constructor, whose calc is `RustCalc`.
    fn new(mut parts: PlatformParts) -> RustSide {
        let effects = Rc::new(RefCell::new(Vec::new()));
        let inner = std::mem::replace(&mut parts.sys.platform, Box::new(NullPlatform));
        parts.sys.platform = Box::new(RecordingPlatform {
            inner,
            effects: effects.clone(),
        });
        let mut app = pob_app::PobApp::new(parts);
        effects.borrow_mut().clear(); // Launch.lua:11 runs at script load, before tracing
                                      // pob-ui-oracle patch 4's equivalent (CCR-0091)
        app.cx.app_mut().launch.force_release_install = true;
        RustSide { app, effects }
    }
    fn flush(&mut self) {
        let e: Vec<_> = self.effects.borrow_mut().drain(..).collect();
        if let Some(t) = self.app.cx.sg.trace.as_mut() {
            for e in e {
                t.effect(e);
            }
        }
    }
}

impl App for RustSide {
    fn sg(&mut self) -> &mut Sg {
        self.app.sg()
    }
    fn sys(&mut self) -> &mut Sys {
        self.app.sys()
    }
    fn on_init(&mut self) {
        self.app.on_init();
        self.flush();
    }
    fn on_frame(&mut self) {
        self.app.on_frame();
        self.flush();
    }
    fn on_key_down(&mut self, key: Key, double_click: bool) {
        self.app.on_key_down(key, double_click);
        self.flush();
    }
    fn on_key_up(&mut self, key: Key) {
        self.app.on_key_up(key);
        self.flush();
    }
    fn on_char(&mut self, ch: Key) {
        self.app.on_char(ch);
        self.flush();
    }
    fn on_lua_error(&mut self, msg: String) {
        self.app.on_lua_error(msg);
    }
}

impl ScenarioApp for RustSide {
    fn load_build(&mut self, name: &str, xml: &str) -> anyhow::Result<()> {
        self.app.load_build(name, xml);
        Ok(())
    }
    fn snapshot(&mut self) -> anyhow::Result<Option<String>> {
        Ok(self.app.snapshot())
    }
    fn after_frame(&mut self) -> anyhow::Result<()> {
        // the Lua side fails the run on PoB's error prompt; so does this one
        if let Some(p) = self
            .app
            .cx
            .app()
            .launch
            .prompt_msg
            .clone()
            .filter(|p| p.starts_with("^1Error"))
        {
            anyhow::bail!("the error prompt (launch:ShowErrMsg): {p}");
        }
        Ok(())
    }
}

// ---- T4 (u20's rules) ----

fn unordered_children(elem: &str) -> bool {
    matches!(
        elem,
        "Calcs" | "ConfigSet" | "ItemSet" | "Sockets" | "Overrides"
    )
}

#[derive(Default)]
struct T4Notes {
    ambiguous_variants: usize,
}

fn ambiguous_variant(node: &XmlNode) -> bool {
    let (Some(game_id), Some(skill_id)) = (node.attrib.get("gemId"), node.attrib.get("skillId"))
    else {
        return false;
    };
    pob_gamedata::data()
        .gems
        .values()
        .filter(|g| *g.gameId == **game_id && *g.grantedEffectId == **skill_id)
        .count()
        > 1
}

fn normalise(node: &XmlNode, top: bool, notes: &mut T4Notes) -> String {
    let mut attrs: Vec<(String, String)> = Vec::new();
    for (k, v) in &node.attrib {
        if &*node.elem == "Gem"
            && matches!(&**k, "variantId" | "nameSpec")
            && ambiguous_variant(node)
        {
            if k.as_ref() == "variantId" {
                notes.ambiguous_variants += 1;
            }
            attrs.push((k.to_string(), "<pairs-chosen variant>".to_owned()));
        } else {
            attrs.push((k.to_string(), v.to_string()));
        }
    }
    attrs.sort();
    let mut kids: Vec<String> = Vec::new();
    for c in &node.children {
        match c {
            XmlChild::Node(n) => kids.push(normalise(n, false, notes)),
            XmlChild::Text(t) => kids.push(format!("{t:?}")),
        }
    }
    if top || unordered_children(&node.elem) {
        kids.sort();
    }
    format!("<{} {:?}>[{}]", node.elem, attrs, kids.join(","))
}

fn root_element(text: &str) -> Result<XmlNode, String> {
    let root = pob_calc::model::xml::ParseXML_root(text)?;
    root.children
        .into_iter()
        .find_map(|c| match c {
            XmlChild::Node(n) => Some(n),
            XmlChild::Text(_) => None,
        })
        .ok_or_else(|| "no root element".to_owned())
}

fn first_diff(a: &str, b: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (a.split('<').collect(), b.split('<').collect());
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        if x != y {
            return format!("element {i}: lua <{x} | rust <{y}");
        }
    }
    format!("lengths {} vs {}", a.len(), b.len())
}

/// Compare one T4 pair; `Ok(byte_identical)`.
fn compare_t4(what: &str, lua: &str, rust: &str, notes: &mut T4Notes) -> Result<bool, String> {
    let lr = root_element(lua).map_err(|e| format!("{what}: Lua XML: {e}"))?;
    let rr = root_element(rust).map_err(|e| format!("{what}: Rust XML: {e}"))?;
    let ln = normalise(&lr, true, notes);
    let rn = normalise(&rr, true, notes);
    if ln != rn {
        return Err(format!("{what}: T4 differs: {}", first_diff(&ln, &rn)));
    }
    Ok(lua == rust)
}

// ---- runs ----

/// Take the `ImageLoad` effects out of the frames (not compared, module notes).
fn take_image_loads(frames: &mut [FrameTrace]) -> Vec<String> {
    let mut out = Vec::new();
    for f in frames {
        f.effects.retain(|e| match e {
            SideEffect::ImageLoad(p) => {
                out.push(format!("{p:?}"));
                false
            }
            _ => true,
        });
    }
    out.sort();
    out
}

struct Side {
    frames: Vec<FrameTrace>,
    images: Vec<String>,
    error: Option<String>,
    snapshots: Vec<(String, String)>,
    saved: Vec<(String, Option<String>)>,
}

fn read_saved(user: &UserDir, names: &[String]) -> Vec<(String, Option<String>)> {
    names
        .iter()
        .map(|n| {
            let p = user.path.join("Path of Building").join(n);
            (n.clone(), std::fs::read_to_string(p).ok())
        })
        .collect()
}

/// Run `script` on both sides with `fixture` as the user dir (fresh copies per side). The Lua
/// VM is dropped before the Rust side starts (memory).
fn run_sides(
    root: &Path,
    script: &pob_host::script::Script,
    fixture: Option<&Path>,
    screen: pob_sg::ScreenInfo,
    saved: &[String],
) -> anyhow::Result<(Side, Side)> {
    let user = UserDir::new(fixture)?;

    // Lua
    let handles = OracleHandles::default();
    let lua_app = runner::lua_side(root, &user, &handles, Entry::Launch, false)?;
    let mut lua_side = LuaSide { app: lua_app };
    let mut d = Driver::new(screen, handles, user.path.clone());
    let (mut frames, r) = runner::drive(&mut lua_side, script, &mut d);
    drop(lua_side);
    let images = take_image_loads(&mut frames);
    let lua = Side {
        frames,
        images,
        error: r.err().map(|e| e.to_string()),
        snapshots: std::mem::take(&mut d.snapshots),
        saved: read_saved(&user, saved),
    };

    // Rust
    user.reset()?;
    let handles = OracleHandles::default();
    let (sg, sys) = boot::oracle_parts(root, &user.path, &handles)?;
    let mut rust_side = RustSide::new(PlatformParts { sg, sys });
    let mut d = Driver::new(screen, handles, user.path.clone());
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runner::drive(&mut rust_side, script, &mut d)
    }));
    let (mut frames, error) = match res {
        Ok((frames, r)) => (frames, r.err().map(|e| e.to_string())),
        Err(p) => (
            Vec::new(),
            Some(format!("panicked: {}", pob_host::panic_message(&*p))),
        ),
    };
    drop(rust_side);
    let images = take_image_loads(&mut frames);
    let rust = Side {
        frames,
        images,
        error,
        snapshots: std::mem::take(&mut d.snapshots),
        saved: read_saved(&user, saved),
    };
    Ok((lua, rust))
}

fn out_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(
            || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"),
            PathBuf::from,
        )
        .join("ui-parity")
}

fn draw_ops(frames: &[FrameTrace]) -> usize {
    frames
        .iter()
        .flat_map(|f| &f.calls)
        .filter(|c| {
            matches!(
                c.call,
                ApiCall::DrawImage { .. }
                    | ApiCall::DrawImageQuad { .. }
                    | ApiCall::DrawString { .. }
            )
        })
        .count()
}

fn window_titles(frames: &[FrameTrace]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in frames
        .iter()
        .flat_map(|f| &f.effects)
        .filter_map(|e| match e {
            SideEffect::WindowTitle(t) => Some(t.clone()),
            _ => None,
        })
    {
        if out.last() != Some(&t) {
            out.push(t);
        }
    }
    out
}

// ---- T3 with the tree's multiset buckets (u23's rule) ----

/// The sub-layers the tree view fills from `pairs(tree.groups)`, `pairs(tree.connectors)`,
/// `pairs(spec.nodes)` and `pairs(tree.sockets)` (ui doc §8 rule 6 (b); the ORDER notes of
/// `tree_view/draw.rs`; u23's multiset buckets).
fn multiset_bucket(k: (i32, i32)) -> bool {
    k.0 == 0 && (0..=30).contains(&k.1)
}

/// Whether PoB drew the passive tree in this frame (only then are the tree's buckets multisets).
fn draws_tree(f: &FrameTrace) -> bool {
    f.calls.iter().any(|c| {
        c.src
            .as_deref()
            .is_some_and(|s| s.contains("PassiveTreeView.lua"))
    })
}

/// A sort key of an op: its arguments at 1e-3 (u23's).
fn op_key(op: &DrawOp) -> String {
    let r = |x: f64| format!("{x:.3}");
    let rs = |v: &[f64]| v.iter().map(|x| r(*x)).collect::<Vec<_>>().join(",");
    let call = match &op.call {
        ApiCall::DrawImage { img, rect, tc } => format!(
            "I {:?} {} {}",
            img.as_ref().map(|i| (&i.path, i.flags)),
            rs(rect),
            tc.as_ref().map_or_else(String::new, |t| rs(t))
        ),
        ApiCall::DrawImageQuad { img, p, tc } => format!(
            "Q {:?} {} {}",
            img.as_ref().map(|i| (&i.path, i.flags)),
            rs(p),
            tc.as_ref().map_or_else(String::new, |t| rs(t))
        ),
        ApiCall::DrawString {
            x,
            y,
            align,
            height,
            font,
            text,
        } => format!(
            "S {} {} {align:?} {} {font:?} {text}",
            r(*x),
            r(*y),
            r(*height)
        ),
        other => format!("{other:?}"),
    };
    format!("{:?} {} {call}", op.color, rs(&op.viewport))
}

/// One frame's divergences: in order, except the tree's buckets in a frame that draws the tree
/// (sorted by [`op_key`], with the O(n²) multiset match where that finds a difference).
fn diff_frame(lua: &FrameTrace, rust: &FrameTrace) -> Vec<Divergence> {
    let screen = lua
        .screen
        .or(rust.screen)
        .unwrap_or_else(|| runner::screen(1280, 800, 1.0));
    let tree = draws_tree(lua);
    let sort = |mut b: Buckets| {
        if tree {
            for (k, ops) in &mut b {
                if multiset_bucket(*k) {
                    ops.sort_by_cached_key(op_key);
                }
            }
        }
        b
    };
    let lb = sort(pob_sg::trace::normalize(lua, &screen));
    let rb = sort(pob_sg::trace::normalize(rust, &screen));
    let mut out = pob_sg::trace::diff(&lb, &rb, &DiffConfig::default());
    if tree
        && out.iter().any(|d| match d {
            Divergence::Missing { bucket, .. }
            | Divergence::Extra { bucket, .. }
            | Divergence::Changed { bucket, .. } => multiset_bucket(*bucket),
            Divergence::Effect { .. } => false,
        })
    {
        let cfg = DiffConfig {
            multiset: vec![((0, 0), (0, 30))],
            ..DiffConfig::default()
        };
        out = pob_sg::trace::diff(&lb, &rb, &cfg);
    }
    out.extend(pob_sg::trace::diff_effects(&lua.effects, &rust.effects));
    out
}

/// Every frame's divergences: `(frame, divergences)` for the frames with any.
fn diff_run(lua: &[FrameTrace], rust: &[FrameTrace]) -> Vec<(usize, Vec<Divergence>)> {
    let empty = FrameTrace::default();
    let mut out = Vec::new();
    for i in 0..lua.len().max(rust.len()) {
        let (a, b) = (lua.get(i).unwrap_or(&empty), rust.get(i).unwrap_or(&empty));
        let d = diff_frame(a, b);
        if !d.is_empty() || i >= lua.len() || i >= rust.len() {
            out.push((i, d));
        }
    }
    out
}

fn describe(d: &Divergence) -> String {
    let op = |o: &DrawOp| {
        format!(
            "{:?} color {:?} vp {:?} ({:?} {:?})",
            o.call, o.color, o.viewport, o.src, o.ctl
        )
    };
    match d {
        Divergence::Missing { bucket, idx, op: o } => {
            format!("missing on rust {bucket:?}#{idx}: {}", op(o))
        }
        Divergence::Extra { bucket, idx, op: o } => {
            format!("extra on rust {bucket:?}#{idx}: {}", op(o))
        }
        Divergence::Changed {
            bucket,
            idx,
            lua,
            rust,
        } => format!(
            "changed {bucket:?}#{idx}: lua {} | rust {}",
            op(lua),
            op(rust)
        ),
        Divergence::Effect { lua, rust } => format!("effect: lua {lua:?} rust {rust:?}"),
    }
}

/// Every T4 pair of a run; `Ok(number of pairs, byte-identical pairs)`.
fn t4(lua: &Side, rust: &Side, notes: &mut T4Notes) -> Result<(usize, usize), Vec<String>> {
    let mut errs = Vec::new();
    let (mut n, mut same) = (0, 0);
    if lua.snapshots.len() != rust.snapshots.len() {
        errs.push(format!(
            "snapshots: lua {} rust {}",
            lua.snapshots.len(),
            rust.snapshots.len()
        ));
    }
    for ((ln, lx), (_, rx)) in lua.snapshots.iter().zip(&rust.snapshots) {
        n += 1;
        match compare_t4(&format!("snapshot {ln}"), lx, rx, notes) {
            Ok(b) => same += usize::from(b),
            Err(e) => errs.push(e),
        }
    }
    for ((name, l), (_, r)) in lua.saved.iter().zip(&rust.saved) {
        match (l, r) {
            (Some(l), Some(r)) => {
                n += 1;
                match compare_t4(&format!("saved {name}"), l, r, notes) {
                    Ok(b) => same += usize::from(b),
                    Err(e) => errs.push(e),
                }
            }
            (None, None) => errs.push(format!("saved {name}: written by neither side")),
            (l, r) => errs.push(format!(
                "saved {name}: lua {} rust {}",
                if l.is_some() { "wrote it" } else { "did not" },
                if r.is_some() { "wrote it" } else { "did not" }
            )),
        }
    }
    if errs.is_empty() {
        Ok((n, same))
    } else {
        Err(errs)
    }
}

/// `#! expect-text TEXT` (tests/parity.rs's rule): some DrawString of the Lua side's last frame
/// contains TEXT (guards a script that silently stopped reaching its target, which both sides
/// would agree on). Also `#! expect-snapshot NAME`, `#! expect-src FILE` (a call of the last
/// frame comes from a Lua file whose name contains FILE) and `#! expect-snapshot-differs A B` /
/// `#! expect-snapshot-same A B` (below).
fn check_expectations(text: &str, lua: &Side) -> Vec<String> {
    let mut errs = Vec::new();
    let Some(last) = lua.frames.last() else {
        return vec!["no frames".into()];
    };
    for want in directives(text, "expect-text") {
        let found = last.calls.iter().any(|c| match &c.call {
            ApiCall::DrawString { text, .. } => text.contains(want.as_str()),
            _ => false,
        });
        if !found {
            errs.push(format!("last frame has no DrawString containing {want:?}"));
        }
    }
    for want in directives(text, "expect-snapshot") {
        if !lua
            .snapshots
            .iter()
            .any(|(n, x)| *n == want && x.contains("<PathOfBuilding"))
        {
            errs.push(format!("no build XML snapshot {want}"));
        }
    }
    for want in directives(text, "expect-src") {
        let found = last
            .calls
            .iter()
            .any(|c| c.src.as_deref().is_some_and(|s| s.contains(want.as_str())));
        if !found {
            errs.push(format!("last frame has no call from {want:?}"));
        }
    }
    // `#! expect-snapshot-differs A B` / `#! expect-snapshot-same A B`: the Lua side's build XML
    // at snapshot A and at snapshot B differ / are equal after the T4 normalisation (guards a
    // click that missed its target: both sides would agree on a no-op).
    let snap = |name: &str| -> Result<String, String> {
        let x = lua
            .snapshots
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, x)| x)
            .ok_or_else(|| format!("no snapshot {name}"))?;
        let root = root_element(x).map_err(|e| format!("snapshot {name}: {e}"))?;
        Ok(normalise(&root, true, &mut T4Notes::default()))
    };
    for (dir, want_same) in [
        ("expect-snapshot-differs", false),
        ("expect-snapshot-same", true),
    ] {
        for arg in directives(text, dir) {
            let Some((a, b)) = arg.split_once(' ') else {
                errs.push(format!("#! {dir} {arg}: expected two snapshot names"));
                continue;
            };
            match (snap(a), snap(b.trim())) {
                (Ok(x), Ok(y)) if (x == y) != want_same => errs.push(format!(
                    "snapshots {a} and {b} {}",
                    if want_same {
                        format!("differ ({})", first_diff(&x, &y))
                    } else {
                        "are equal (the step between them changed nothing)".to_owned()
                    }
                )),
                (Ok(_), Ok(_)) => {}
                (Err(e), _) | (_, Err(e)) => errs.push(e),
            }
        }
    }
    errs
}

#[test]
fn u53_e2e() {
    let Some(root) = inputs() else { return };
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(scenario_dir())
        .expect("scenarios/e2e")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no scenarios/e2e/*.pobscript");
    let filter = std::env::var("U53_FILTER").ok();
    let mut failures = Vec::new();
    let mut ran = 0;
    let (mut t4_pairs, mut t4_bytes, mut total_ops) = (0, 0, 0);
    for path in scripts {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if filter.as_ref().is_some_and(|f| !stem.contains(f.as_str())) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let (script, base_dir) = runner::load_script(&path).unwrap();
        let fx = runner::fixtures(&script, &base_dir).unwrap();
        let own_screen = matches!(
            script.steps.first(),
            Some(pob_host::script::Step::Screen { .. })
        );
        let screens: Vec<(i32, i32, f32)> = if own_screen {
            vec![(1280, 800, 1.0)]
        } else {
            SCREENS.to_vec()
        };
        let saved = directives(&text, "e2e-saved");
        for (w, h, s) in screens {
            let tag = if own_screen {
                format!("u53_{stem}")
            } else {
                format!("u53_{stem}@{w}x{h}@{s}")
            };
            let started = std::time::Instant::now();
            let (lua, rust) = match run_sides(
                &root,
                &script,
                fx.user_dir.as_deref(),
                runner::screen(w, h, s),
                &saved,
            ) {
                Ok(s) => s,
                Err(e) => {
                    failures.push(format!("{tag}: {e:#}"));
                    continue;
                }
            };
            ran += 1;
            let titles = (window_titles(&lua.frames), window_titles(&rust.frames));
            let mut notes = T4Notes::default();
            let t4r = t4(&lua, &rust, &mut notes);
            let expect = check_expectations(&text, &lua);
            let run = RunResult {
                lua: lua.frames,
                rust: rust.frames,
                rust_error: rust.error.clone(),
                lua_snapshots: lua.snapshots.clone(),
                rust_snapshots: rust.snapshots.clone(),
                audit: Vec::new(),
                compared: true,
            };
            // the traces and the in-order report (diff_report), then this test's count
            let _ = diff_report::write_report(&out_dir(), &tag, &run, false);
            let divs = diff_run(&run.lua, &run.rust);
            let n: usize = divs.iter().map(|(_, d)| d.len().max(1)).sum();
            {
                let mut txt = String::new();
                for (f, d) in &divs {
                    for x in d.iter().take(40) {
                        let _ = writeln!(txt, "frame {f}: {}", describe(x));
                    }
                }
                let _ = std::fs::write(out_dir().join(&tag).join("e2e_divergences.txt"), txt);
            }
            let ops = draw_ops(&run.lua);
            total_ops += ops;
            let t4_line = match &t4r {
                Ok((p, b)) => {
                    t4_pairs += p;
                    t4_bytes += b;
                    format!(
                        "T4 {p} identical ({b} byte-identical; set aside: {} pairs-chosen gem variants)",
                        notes.ambiguous_variants
                    )
                }
                Err(e) => format!("T4 FAILED: {}", e.join("; ")),
            };
            println!(
                "{tag}: {} frames, {ops} Lua draw ops, {n} divergences, {t4_line}, {} Lua image loads, titles {:?}, {:.1}s{}{}{}",
                run.lua.len(),
                lua.images.len(),
                titles.0,
                started.elapsed().as_secs_f64(),
                lua.error
                    .as_ref()
                    .map_or_else(String::new, |e| format!(", Lua error: {e}")),
                rust.error
                    .as_ref()
                    .map_or_else(String::new, |e| format!(", Rust error: {e}")),
                if expect.is_empty() {
                    String::new()
                } else {
                    format!(", expectations: {}", expect.join("; "))
                }
            );
            if n != 0
                || rust.error.is_some()
                || lua.error.is_some()
                || run.lua.len() != run.rust.len()
                || ops == 0
                || titles.0 != titles.1
                || t4r.is_err()
                || !expect.is_empty()
            {
                failures.push(format!(
                    "{tag}: {n} divergences, frames lua {} rust {}, {ops} ops, {t4_line}, titles lua {:?} rust {:?}, errors lua {:?} rust {:?}, expectations {expect:?} (see {})",
                    run.lua.len(),
                    run.rust.len(),
                    titles.0,
                    titles.1,
                    lua.error,
                    rust.error,
                    out_dir().join(&tag).join("e2e_divergences.txt").display()
                ));
            }
        }
    }
    println!(
        "u53_e2e: {ran} runs, {total_ops} Lua draw ops, T4 {t4_pairs} pairs ({t4_bytes} byte-identical), {} failed",
        failures.len()
    );
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
