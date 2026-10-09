//! U4 T0: GetPos/GetSize/IsShown/IsMouseInBounds/SetAnchor against the real Control.lua under
//! LuaJIT (ui doc §11.1 T0), over 500 random anchor graphs: all 9×9 point pairs, chains, collapse
//! with hidden and shown `other` (and collapseX/Y), Prop closures (live globals, sizes of other
//! controls, the control's own width through `self`), negative and fractional values (floor),
//! per-instance GetSize overrides, nil `other`, and SetAnchor with both/one/no x,y. Each graph is
//! evaluated three times (globals and cursor changed; then after random SetAnchor calls).
//! Bit-exact: every number must be equal (a ±0 sign difference would be counted separately as the
//! dual-number host artefact of CCR-0002; there are none).
#![allow(
    clippy::many_single_char_names,
    clippy::range_plus_one,
    clippy::needless_raw_string_hashes,
    clippy::semicolon_if_nothing_returned
)]

use std::cell::RefCell;
use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::rc::Rc;

use mlua::Lua;
use pob_ui_core::layout::{rect, Anchor};
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
        assert!(p.pop(), "PathOfBuilding checkout not found (set POB_DIR)");
    }
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
    /// Integers, quarters, thirds (inexact) and tenths (inexact), negatives included.
    fn val(&mut self, lo: i64, hi: i64) -> f64 {
        match self.below(4) {
            0 => self.range(lo, hi) as f64,
            1 => self.range(lo * 4, hi * 4) as f64 / 4.0,
            2 => self.range(lo * 3, hi * 3) as f64 / 3.0,
            _ => self.range(lo * 10, hi * 10) as f64 * 0.1,
        }
    }
}

const POINTS: [(AnchorPoint, &str); 9] = [
    (AnchorPoint::TopLeft, "TOPLEFT"),
    (AnchorPoint::Top, "TOP"),
    (AnchorPoint::TopRight, "TOPRIGHT"),
    (AnchorPoint::Right, "RIGHT"),
    (AnchorPoint::BottomRight, "BOTTOMRIGHT"),
    (AnchorPoint::Bottom, "BOTTOM"),
    (AnchorPoint::BottomLeft, "BOTTOMLEFT"),
    (AnchorPoint::Left, "LEFT"),
    (AnchorPoint::Center, "CENTER"),
];

thread_local! {
    static G: RefCell<Vec<f64>> = RefCell::new(vec![0.0; 5]);
    static GB: RefCell<Vec<bool>> = RefCell::new(vec![true; 5]);
}

/// A Lua `Prop<number>` (value or function) and its Rust twin.
#[derive(Clone, Copy)]
enum Num {
    Val(f64),
    /// function(self) return G[g] * a + b end
    Glob {
        g: usize,
        a: f64,
        b: f64,
    },
    /// function(self) local w, h = C[j]:GetSize() return (w|h) * a + b end
    Size {
        j: usize,
        w: bool,
        a: f64,
        b: f64,
    },
    /// function(self) return self:GetProperty("width") * a + b end
    SelfW {
        a: f64,
        b: f64,
    },
}

fn lf(v: f64) -> String {
    format!("{v:?}")
}

impl Num {
    fn lua(self) -> String {
        match self {
            Num::Val(v) => lf(v),
            Num::Glob { g, a, b } => {
                format!("function(self) return G[{g}] * {} + {} end", lf(a), lf(b))
            }
            Num::Size { j, w, a, b } => format!(
                "function(self) local w, h = C[{j}]:GetSize() return {} * {} + {} end",
                if w { "w" } else { "h" },
                lf(a),
                lf(b)
            ),
            Num::SelfW { a, b } => format!(
                "function(self) return self:GetProperty(\"width\") * {} + {} end",
                lf(a),
                lf(b)
            ),
        }
    }
    fn prop(self, ids: &[CtrlId]) -> Prop<f64> {
        match self {
            Num::Val(v) => Prop::Val(v),
            Num::Glob { g, a, b } => Prop::f(move |_, _| G.with(|x| x.borrow()[g]) * a + b),
            Num::Size { j, w, a, b } => {
                let o = ids[j - 1];
                Prop::f(move |cx, _| {
                    let (sw, sh) = cx.size(o);
                    (if w { sw } else { sh }) * a + b
                })
            }
            Num::SelfW { a, b } => Prop::f(move |cx, me| cx.width(me) * a + b),
        }
    }
}

#[derive(Clone, Copy)]
enum Shown {
    Val(bool),
    Glob(usize),
}

struct Spec {
    /// (point, other index (0 = nil), other point, collapse)
    anchor: Option<(usize, usize, usize, bool)>,
    rect: Option<[Num; 4]>,
    collapse_xy: (Option<f64>, Option<f64>),
    shown: Shown,
    size_ov: Option<(f64, f64)>,
}

fn gen_num(rng: &mut Rng, i: usize, pos: bool, size: bool) -> Num {
    let (lo, hi) = if size { (-5, 80) } else { (-40, 120) };
    match rng.below(10) {
        0..=5 => Num::Val(rng.val(lo, hi)),
        6 | 7 => Num::Glob {
            g: 1 + rng.below(4) as usize,
            a: rng.val(-2, 2),
            b: rng.val(lo, hi),
        },
        8 if i > 1 => Num::Size {
            j: 1 + rng.below(i as u64 - 1) as usize,
            w: rng.chance(50),
            a: rng.val(-1, 2),
            b: rng.val(-10, 10),
        },
        9 if pos => Num::SelfW {
            a: rng.val(-1, 1),
            b: rng.val(-10, 10),
        },
        _ => Num::Val(rng.val(lo, hi)),
    }
}

fn gen_spec(rng: &mut Rng, i: usize, pairs: &mut HashSet<(usize, usize)>) -> Spec {
    let anchor = if i > 1 && rng.chance(85) {
        let p = rng.below(9) as usize;
        let op = rng.below(9) as usize;
        pairs.insert((p, op));
        // mostly the previous control (chains), sometimes any earlier one, rarely nil
        let j = if rng.chance(5) {
            0
        } else if rng.chance(60) {
            i - 1
        } else {
            1 + rng.below(i as u64 - 1) as usize
        };
        Some((p, j, op, rng.chance(25)))
    } else {
        None
    };
    let rect = if rng.chance(95) {
        Some([
            gen_num(rng, i, true, false),
            gen_num(rng, i, true, false),
            gen_num(rng, i, false, true),
            gen_num(rng, i, false, true),
        ])
    } else {
        None
    };
    let cxy = |rng: &mut Rng| rng.chance(50).then(|| rng.val(-10, 10));
    let collapse_xy = if anchor.is_some_and(|a| a.3) {
        (cxy(rng), cxy(rng))
    } else {
        (None, None)
    };
    let shown = match rng.below(10) {
        0..=4 => Shown::Val(true),
        5 => Shown::Val(false),
        _ => Shown::Glob(1 + rng.below(4) as usize),
    };
    let size_ov = rng.chance(8).then(|| (rng.val(0, 50), rng.val(0, 50)));
    Spec {
        anchor,
        rect,
        collapse_xy,
        shown,
        size_ov,
    }
}

/// `C[i]:SetAnchor(point, other, otherPoint, x, y, collapse)`.
struct SetAnchorCall {
    i: usize,
    p: usize,
    j: usize,
    op: usize,
    x: Option<Num>,
    y: Option<Num>,
    collapse: Option<bool>,
}

fn lua_build(specs: &[Spec]) -> String {
    let mut s = String::from("C = { }\n");
    for (k, sp) in specs.iter().enumerate() {
        let i = k + 1;
        let anchor = match sp.anchor {
            None => "nil".to_owned(),
            Some((p, j, op, col)) => format!(
                "{{ \"{}\", {}, \"{}\"{} }}",
                POINTS[p].1,
                if j == 0 {
                    "nil".to_owned()
                } else {
                    format!("C[{j}]")
                },
                POINTS[op].1,
                if col { ", true" } else { "" }
            ),
        };
        match sp.rect {
            Some(r) => writeln!(
                s,
                "C[{i}] = new(\"Control\"):Control({anchor}, {{ {}, {}, {}, {} }})",
                r[0].lua(),
                r[1].lua(),
                r[2].lua(),
                r[3].lua()
            )
            .unwrap(),
            None => writeln!(s, "C[{i}] = new(\"Control\"):Control({anchor})").unwrap(),
        }
        if let Some(v) = sp.collapse_xy.0 {
            writeln!(s, "C[{i}].collapseX = {}", lf(v)).unwrap();
        }
        if let Some(v) = sp.collapse_xy.1 {
            writeln!(s, "C[{i}].collapseY = {}", lf(v)).unwrap();
        }
        match sp.shown {
            Shown::Val(true) => {}
            Shown::Val(false) => writeln!(s, "C[{i}].shown = false").unwrap(),
            Shown::Glob(g) => {
                writeln!(s, "C[{i}].shown = function(self) return GB[{g}] end").unwrap()
            }
        }
        if let Some((w, h)) = sp.size_ov {
            writeln!(
                s,
                "C[{i}].GetSize = function(self) return {}, {} end",
                lf(w),
                lf(h)
            )
            .unwrap();
        }
    }
    s
}

fn rust_build(cx: &mut Cx, specs: &[Spec]) -> Vec<CtrlId> {
    let mut ids: Vec<CtrlId> = Vec::new();
    for sp in specs {
        let anchor = match sp.anchor {
            None => Anchor::NONE,
            Some((p, j, op, col)) => {
                let a = if j == 0 {
                    Anchor::nil(POINTS[p].0, POINTS[op].0)
                } else {
                    Anchor::new(POINTS[p].0, ids[j - 1], POINTS[op].0)
                };
                if col {
                    a.collapse()
                } else {
                    a
                }
            }
        };
        let r = match sp.rect {
            Some(r) => [
                r[0].prop(&ids),
                r[1].prop(&ids),
                r[2].prop(&ids),
                r[3].prop(&ids),
            ],
            None => rect(0, 0, 0, 0),
        };
        let id = cx.add_free(None, PlainControl::new(anchor, r)).id();
        if let Some(v) = sp.collapse_xy.0 {
            cx.base_mut(id).collapse_x = v;
        }
        if let Some(v) = sp.collapse_xy.1 {
            cx.base_mut(id).collapse_y = v;
        }
        match sp.shown {
            Shown::Val(v) => cx.base_mut(id).shown = Prop::Val(v),
            Shown::Glob(g) => {
                cx.base_mut(id).shown = Prop::f(move |_, _| GB.with(|x| x.borrow()[g]))
            }
        }
        if let Some((w, h)) = sp.size_ov {
            cx.set_ov(id, |ov| ov.get_size = Some(Rc::new(move |_, _| (w, h))));
        }
        ids.push(id);
    }
    ids
}

const LUA_EVAL: &str = r#"
function eval()
	local out = { }
	for i = 1, #C do
		local x, y = C[i]:GetPos()
		local w, h = C[i]:GetSize()
		out[#out + 1] = x
		out[#out + 1] = y
		out[#out + 1] = w
		out[#out + 1] = h
		out[#out + 1] = C[i]:IsShown() and 1 or 0
		out[#out + 1] = C[i]:IsMouseInBounds() and 1 or 0
		out[#out + 1] = C[i]:IsEnabled() and 1 or 0
	end
	return out
end
"#;

fn rust_eval(cx: &Cx, ids: &[CtrlId]) -> Vec<f64> {
    let mut out = Vec::new();
    for &id in ids {
        let (x, y) = cx.pos(id);
        let (w, h) = cx.size(id);
        out.extend([x, y, w, h]);
        out.push(if cx.is_shown(id) { 1.0 } else { 0.0 });
        out.push(if cx.is_mouse_in_bounds(id) { 1.0 } else { 0.0 });
        out.push(if cx.is_enabled(id) { 1.0 } else { 0.0 });
    }
    out
}

fn set_globals(lua: &Lua, cx: &mut Cx, rng: &mut Rng) {
    let g: Vec<f64> = (0..5).map(|_| rng.val(-30, 60)).collect();
    let gb: Vec<bool> = (0..5).map(|_| rng.chance(70)).collect();
    let (cxp, cyp) = (rng.range(-20, 200) as i32, rng.range(-20, 200) as i32);
    let lg = lua.create_table().unwrap();
    let lgb = lua.create_table().unwrap();
    for k in 1..5 {
        lg.set(k, g[k]).unwrap();
        lgb.set(k, gb[k]).unwrap();
    }
    let globals = lua.globals();
    globals.set("G", lg).unwrap();
    globals.set("GB", lgb).unwrap();
    globals.set("CURSOR_X", cxp).unwrap();
    globals.set("CURSOR_Y", cyp).unwrap();
    G.with(|x| *x.borrow_mut() = g);
    GB.with(|x| *x.borrow_mut() = gb);
    cx.sys.begin_frame((cxp, cyp), 1.0);
}

fn gen_set_anchor(rng: &mut Rng, n: usize, pairs: &mut HashSet<(usize, usize)>) -> SetAnchorCall {
    let i = 2 + rng.below(n as u64 - 1) as usize;
    let p = rng.below(9) as usize;
    let op = rng.below(9) as usize;
    pairs.insert((p, op));
    let j = if rng.chance(10) {
        0
    } else {
        1 + rng.below(i as u64 - 1) as usize
    };
    let xy = |rng: &mut Rng| match rng.below(3) {
        0 => None,
        1 => Some(Num::Val(rng.val(-20, 20))),
        _ => Some(Num::Glob {
            g: 1 + rng.below(4) as usize,
            a: rng.val(-1, 1),
            b: rng.val(-5, 5),
        }),
    };
    let (x, y) = (xy(rng), xy(rng));
    let collapse = match rng.below(3) {
        0 => None,
        1 => Some(false),
        _ => Some(true),
    };
    SetAnchorCall {
        i,
        p,
        j,
        op,
        x,
        y,
        collapse,
    }
}

impl SetAnchorCall {
    fn lua(&self) -> String {
        let n = |v: Option<Num>| v.map_or("nil".to_owned(), Num::lua);
        format!(
            "C[{}]:SetAnchor(\"{}\", {}, \"{}\", {}, {}, {})",
            self.i,
            POINTS[self.p].1,
            if self.j == 0 {
                "nil".to_owned()
            } else {
                format!("C[{}]", self.j)
            },
            POINTS[self.op].1,
            n(self.x),
            n(self.y),
            self.collapse.map_or("nil".to_owned(), |c| c.to_string())
        )
    }
    fn apply(&self, cx: &mut Cx, ids: &[CtrlId]) {
        let other = (self.j != 0).then(|| ids[self.j - 1]);
        let xy = match (self.x, self.y) {
            (Some(x), Some(y)) => Some((x.prop(ids), y.prop(ids))),
            _ => None,
        };
        cx.set_anchor(
            ids[self.i - 1],
            POINTS[self.p].0,
            other,
            POINTS[self.op].0,
            xy,
            self.collapse == Some(true),
        );
    }
}

#[test]
fn anchor_graphs_vs_control_lua() {
    let lua = Lua::new();
    lua.load("if jit then jit.off() end").exec().unwrap();
    lua.load(include_str!("lua/shim.lua"))
        .set_name("@tests/lua/shim.lua")
        .exec()
        .unwrap();
    let control = std::fs::read_to_string(pob_src().join("Classes/Control.lua")).unwrap();
    lua.load(control)
        .set_name("@Classes/Control.lua")
        .exec()
        .unwrap();
    lua.load(LUA_EVAL).exec().unwrap();
    let eval: mlua::Function = lua.globals().get("eval").unwrap();

    let mut rng = Rng(0x1a70_0500);
    let mut pairs = HashSet::new();
    let graphs = 500;
    let mut values = 0usize;
    let mut zero_sign = 0usize;
    let mut collapsed_hidden = 0usize;
    for gi in 0..graphs {
        let mut cx = Cx::headless();
        let n = 1 + rng.below(9) as usize;
        let specs: Vec<Spec> = (1..=n).map(|i| gen_spec(&mut rng, i, &mut pairs)).collect();
        let src = lua_build(&specs);
        lua.load(&src)
            .exec()
            .unwrap_or_else(|e| panic!("graph {gi}: {e}\n{src}"));
        let ids = rust_build(&mut cx, &specs);
        for round in 0..3 {
            let mut calls = String::new();
            set_globals(&lua, &mut cx, &mut rng);
            if round == 2 && n > 1 {
                for _ in 0..1 + rng.below(3) {
                    let c = gen_set_anchor(&mut rng, n, &mut pairs);
                    let l = c.lua();
                    lua.load(&l).exec().unwrap();
                    c.apply(&mut cx, &ids);
                    calls.push_str(&l);
                    calls.push('\n');
                }
            }
            let l: Vec<f64> = eval.call(()).unwrap();
            let r = rust_eval(&cx, &ids);
            assert_eq!(l.len(), r.len());
            for (k, (a, b)) in l.iter().zip(&r).enumerate() {
                values += 1;
                if a.to_bits() != b.to_bits() {
                    if a == b {
                        zero_sign += 1;
                        continue;
                    }
                    panic!(
                        "graph {gi} round {round}: control {} value {} lua={a:?} rust={b:?}\n{src}{calls}",
                        k / 7 + 1,
                        ["x", "y", "w", "h", "shown", "inBounds", "enabled"][k % 7]
                    );
                }
            }
            for (k, sp) in specs.iter().enumerate() {
                if let Some((_, j, _, true)) = sp.anchor {
                    if j > 0 && !cx.base(ids[j - 1]).shown.get(&cx, ids[j - 1]) {
                        collapsed_hidden += 1;
                        let _ = k;
                    }
                }
            }
        }
    }
    eprintln!(
        "tier0_layout: {graphs} graphs x 3 rounds, {values} values compared, {} of 81 point pairs, \
         {collapsed_hidden} collapsed-with-hidden-other evaluations, {zero_sign} ±0 differences, 0 divergences",
        pairs.len()
    );
    assert_eq!(pairs.len(), 81, "all 9x9 anchor point pairs covered");
    assert!(collapsed_hidden > 50);
    assert_eq!(zero_sign, 0);
}

/// AddToTabGroup / TabAdvance (Control.lua:151-178) and isValueInArray (Common.lua:569-575)
/// against the real Lua: random groups (re-masters, duplicates, a control moved to a second
/// group), per-instance OnKeyDown, hidden members (own `shown` or a hidden anchor), steps 1, -1, 0.
#[test]
fn tab_groups_vs_control_lua() {
    let lua = Lua::new();
    lua.load("if jit then jit.off() end").exec().unwrap();
    lua.load(include_str!("lua/shim.lua")).exec().unwrap();
    let common = std::fs::read_to_string(pob_src().join("Modules/Common.lua")).unwrap();
    let iva: Vec<&str> = common.lines().skip(568).take(7).collect(); // Common.lua:569-575
    assert!(iva[0].starts_with("function isValueInArray"));
    lua.load(iva.join("\n")).exec().unwrap();
    let control = std::fs::read_to_string(pob_src().join("Classes/Control.lua")).unwrap();
    lua.load(control)
        .set_name("@Classes/Control.lua")
        .exec()
        .unwrap();
    lua.load(
        r#"
        function adv(i, step)
            local r = C[i]:TabAdvance(step)
            for k = 1, #C do if C[k] == r then return k end end
            return -1
        end
        "#,
    )
    .exec()
    .unwrap();
    let adv: mlua::Function = lua.globals().get("adv").unwrap();
    let mut rng = Rng(0x7ab0_0178);
    let mut compared = 0;
    for gi in 0..400 {
        let n = 1 + rng.below(8) as usize;
        let mut cx = Cx::headless();
        let mut src = String::from("C = { }\n");
        let mut ids = Vec::new();
        for i in 1..=n {
            // anchored (to a possibly hidden earlier control) or free
            let anchor_to = (i > 1 && rng.chance(30)).then(|| 1 + rng.below(i as u64 - 1) as usize);
            let shown = rng.chance(75);
            let kd = rng.chance(70);
            match anchor_to {
                Some(j) => writeln!(
                    src,
                    "C[{i}] = new(\"Control\"):Control({{ \"TOPLEFT\", C[{j}], \"TOPLEFT\" }})"
                )
                .unwrap(),
                None => writeln!(src, "C[{i}] = new(\"Control\"):Control()").unwrap(),
            }
            writeln!(src, "C[{i}].shown = {shown}").unwrap();
            if kd {
                writeln!(src, "C[{i}].OnKeyDown = function(self) return self end").unwrap();
            }
            let anchor = anchor_to.map_or(Anchor::NONE, |j| {
                Anchor::new(AnchorPoint::TopLeft, ids[j - 1], AnchorPoint::TopLeft)
            });
            let id = cx
                .add_free(None, PlainControl::new(anchor, rect(0, 0, 0, 0)))
                .id();
            cx.base_mut(id).shown = Prop::Val(shown);
            if kd {
                cx.set_ov(id, |ov| {
                    ov.on_key_down = Some(Rc::new(|_, me, _, _| Some(me)))
                });
            }
            ids.push(id);
        }
        for _ in 0..rng.below(2 * n as u64 + 1) {
            let (i, m) = (
                1 + rng.below(n as u64) as usize,
                1 + rng.below(n as u64) as usize,
            );
            writeln!(src, "C[{i}]:AddToTabGroup(C[{m}])").unwrap();
            cx.add_to_tab_group(ids[i - 1], ids[m - 1]);
        }
        lua.load(&src).exec().unwrap();
        for i in 1..=n {
            for step in [1, -1, 0] {
                let l: i64 = adv.call((i, step)).unwrap();
                let r = cx.tab_advance(ids[i - 1], step);
                let r = ids
                    .iter()
                    .position(|&x| x == r)
                    .map_or(-1, |p| p as i64 + 1);
                assert_eq!(l, r, "graph {gi}: C[{i}]:TabAdvance({step})\n{src}");
                compared += 1;
            }
        }
    }
    eprintln!("tier0_layout tab groups: 400 graphs, {compared} TabAdvance results compared, 0 divergences");
}
