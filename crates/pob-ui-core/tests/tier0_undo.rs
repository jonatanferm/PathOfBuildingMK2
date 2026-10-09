//! U4 T0: `UndoOps`/`UndoStack` against the real UndoHandler.lua under LuaJIT (ui doc §11.1 T0):
//! random sequences of state edits, AddUndoState (with and without noClearRedo), Undo, Redo,
//! ResetUndo and modFlag resets, compared after every step (both stacks in order, modFlag, the
//! restored state). The undo cap (101 entries) is reached in most sequences. The state records
//! `#self.undo` at creation, so the ResetUndo statement order (wipe, then CreateUndoState) is
//! checked too.
#![allow(
    clippy::many_single_char_names,
    clippy::range_plus_one,
    clippy::needless_raw_string_hashes,
    clippy::semicolon_if_nothing_returned
)]

use std::path::PathBuf;

use mlua::{Function, Lua};
use pob_ui_core::layout::{rect, Anchor, Base};
use pob_ui_core::prelude::*;
use pob_ui_core::undo::{UndoOps, UndoStack};

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

fn load_file(lua: &Lua, rel: &str) {
    let text = std::fs::read_to_string(pob_src().join(rel)).expect("read PoB source");
    lua.load(text).set_name(format!("@{rel}")).exec().unwrap();
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
}

// ---- Rust side: an UndoHandler "class" ----

#[derive(Clone, Debug)]
struct St {
    v: i64,
    n: i64,
    u: i64,
}

struct UndoObj {
    stack: UndoStack<St>,
    state: i64,
    creates: i64,
}
impl Widget for UndoObj {
    const NAME: &'static str = "UndoObj";
    const CAPS: Caps = Caps::empty();
}

fn stack(cx: &mut Cx, me: CtrlId) -> &mut UndoStack<St> {
    &mut cx.w_mut::<UndoObj>(me).stack
}
fn create(cx: &mut Cx, me: CtrlId) -> St {
    let o = cx.w_mut::<UndoObj>(me);
    o.creates += 1;
    St {
        v: o.state,
        n: o.creates,
        u: o.stack.undo.len() as i64,
    }
}
fn restore(cx: &mut Cx, me: CtrlId, s: St) {
    cx.w_mut::<UndoObj>(me).state = s.v;
}
const OPS: UndoOps<St> = UndoOps {
    stack,
    create,
    restore,
};

fn snapshot(cx: &Cx, me: CtrlId) -> String {
    let o = cx.w::<UndoObj>(me);
    let f = |q: &std::collections::VecDeque<St>| {
        q.iter()
            .map(|s| format!("{}.{}.{}", s.v, s.n, s.u))
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        "state={} mod={} undo=[{}] redo=[{}]",
        o.state,
        o.stack.mod_flag,
        f(&o.stack.undo),
        f(&o.stack.redo)
    )
}

const LUA_OBJ: &str = r#"
local UndoObjClass = newClass("UndoObj", "UndoHandler")
function UndoObjClass:UndoObj()
	self:UndoHandler()
	self.state = 0
	self.creates = 0
	return self
end
function UndoObjClass:CreateUndoState()
	self.creates = self.creates + 1
	return { v = self.state, n = self.creates, u = #self.undo }
end
function UndoObjClass:RestoreUndoState(s)
	self.state = s.v
end
local function list(t)
	local out = { }
	for i, s in ipairs(t) do out[i] = s.v .. "." .. s.n .. "." .. s.u end
	return table.concat(out, ",")
end
function fresh() O = new("UndoObj"):UndoObj() end
function snapshot()
	return "state=" .. O.state .. " mod=" .. tostring(O.modFlag and true or false)
		.. " undo=[" .. list(O.undo) .. "] redo=[" .. list(O.redo) .. "]"
end
function setState(v) O.state = v end
function addUndo(noClear) O:AddUndoState(noClear) end
function undo() O:Undo() end
function redo() O:Redo() end
function resetUndo() O:ResetUndo() end
function clearMod() O.modFlag = false end
"#;

#[test]
fn undo_sequences_vs_lua() {
    let lua = Lua::new();
    lua.load("if jit then jit.off() end").exec().unwrap();
    lua.load(include_str!("lua/shim.lua"))
        .set_name("@tests/lua/shim.lua")
        .exec()
        .unwrap();
    // Common.lua:549-557 wipeTable, verbatim
    let common = std::fs::read_to_string(pob_src().join("Modules/Common.lua")).unwrap();
    let wipe: Vec<&str> = common.lines().skip(548).take(9).collect();
    assert!(wipe[0].starts_with("function wipeTable"));
    lua.load(wipe.join("\n")).exec().unwrap();
    load_file(&lua, "Classes/UndoHandler.lua");
    lua.load(LUA_OBJ).exec().unwrap();
    let g = lua.globals();
    let f = |n: &str| -> Function { g.get(n).unwrap() };
    let (fresh, snap, set_state, add, undo, redo, reset, clear_mod) = (
        f("fresh"),
        f("snapshot"),
        f("setState"),
        f("addUndo"),
        f("undo"),
        f("redo"),
        f("resetUndo"),
        f("clearMod"),
    );

    let mut cx = Cx::headless();
    let mut rng = Rng(0x0dd0_0101);
    let sequences = 200;
    let mut steps = 0;
    let mut max_undo = 0;
    for s in 0..sequences {
        fresh.call::<()>(()).unwrap();
        let me = cx
            .add_free(
                None,
                New::new(
                    Base::new(Anchor::NONE, rect(0, 0, 0, 0)),
                    UndoObj {
                        stack: UndoStack::default(),
                        state: 0,
                        creates: 0,
                    },
                ),
            )
            .id();
        let n = 50 + rng.below(350);
        // Every third sequence is edit-heavy (no resets), so the 101 cap is hit and kept.
        let heavy = s % 3 == 0;
        for i in 0..n {
            let mut roll = rng.below(100);
            if heavy && (65..=96).contains(&roll) {
                roll = if roll < 70 {
                    70
                } else if roll < 74 {
                    85
                } else {
                    40
                };
            }
            let what = match roll {
                0..=29 => {
                    let v = rng.below(1000) as i64;
                    set_state.call::<()>(v).unwrap();
                    cx.w_mut::<UndoObj>(me).state = v;
                    format!("set {v}")
                }
                30..=64 => {
                    let nc = rng.below(4) == 0;
                    add.call::<()>(nc).unwrap();
                    OPS.add_undo_state(&mut cx, me, nc);
                    format!("add {nc}")
                }
                65..=79 => {
                    undo.call::<()>(()).unwrap();
                    OPS.undo(&mut cx, me);
                    "undo".to_owned()
                }
                80..=92 => {
                    redo.call::<()>(()).unwrap();
                    OPS.redo(&mut cx, me);
                    "redo".to_owned()
                }
                93..=96 => {
                    reset.call::<()>(()).unwrap();
                    OPS.reset_undo(&mut cx, me);
                    "reset".to_owned()
                }
                _ => {
                    clear_mod.call::<()>(()).unwrap();
                    cx.w_mut::<UndoObj>(me).stack.mod_flag = false;
                    "clearMod".to_owned()
                }
            };
            steps += 1;
            max_undo = max_undo.max(cx.w::<UndoObj>(me).stack.undo.len());
            let l: String = snap.call(()).unwrap();
            let r = snapshot(&cx, me);
            assert_eq!(l, r, "sequence {s} step {i} ({what})");
        }
        cx.bury(me);
        cx.end_frame_gc();
    }
    eprintln!("tier0_undo: {sequences} sequences, {steps} steps compared, max #undo {max_undo}, 0 divergences");
    assert_eq!(max_undo, 101, "the cap was exercised");
}
