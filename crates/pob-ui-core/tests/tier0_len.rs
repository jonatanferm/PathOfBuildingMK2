//! U4 T0: `InputEvents` (`main.inputEvents`) against a real LuaJIT table (ui doc §11.1 T0, §13
//! risk 5): `#t` borders (`lj_tab_len`), `t_insert` at `#t + 1`, holes from `t[i] = nil`,
//! `ipairs` stopping at the first hole, and Common.lua's own `wipeTable` (the array part keeps
//! its size, so later borders depend on the history). Random operation sequences, compared after
//! every operation. The LuaJIT interpreter is the reference (`jit.off()`, AMENDMENTS A6).
#![allow(clippy::needless_raw_string_hashes)]

use std::path::PathBuf;

use mlua::{Function, Lua};
use pob_sg::Key;
use pob_ui_core::input::{EventKind, InputEvent, InputEvents};

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

/// Lines `a..=b` of a PoB source file, verbatim.
fn lua_lines(file: &str, a: usize, b: usize) -> String {
    let text = std::fs::read_to_string(pob_src().join(file)).expect("read PoB source");
    text.lines()
        .skip(a - 1)
        .take(b - a + 1)
        .collect::<Vec<_>>()
        .join("\n")
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        // splitmix64
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

struct LuaSide {
    ins: Function,
    cons: Function,
    wipe: Function,
    fresh: Function,
    len: Function,
    present: Function,
    ipairs_n: Function,
}

fn boot() -> (Lua, LuaSide) {
    let lua = Lua::new();
    lua.load("if jit then jit.off() end").exec().unwrap();
    // Common.lua:549-557 wipeTable, verbatim
    lua.load(lua_lines("Modules/Common.lua", 549, 557))
        .set_name("@Modules/Common.lua")
        .exec()
        .unwrap();
    lua.load(
        r#"
        t = { }
        local t_insert = table.insert
        function ins() t_insert(t, { }) end                -- Main.lua:496 t_insert(self.inputEvents, ..)
        function cons(i) t[i] = nil end                   -- ControlHost.lua inputEvents[id] = nil
        function wipe() wipeTable(t) end                  -- Main.lua:487
        function fresh() t = { } end                      -- Launch.lua:367
        function len() return #t end                      -- Build.lua:1216
        function present(i) return t[i] ~= nil end
        function ipairs_n() local n = 0 for _ in ipairs(t) do n = n + 1 end return n end
        "#,
    )
    .exec()
    .unwrap();
    let g = lua.globals();
    let side = LuaSide {
        ins: g.get("ins").unwrap(),
        cons: g.get("cons").unwrap(),
        wipe: g.get("wipe").unwrap(),
        fresh: g.get("fresh").unwrap(),
        len: g.get("len").unwrap(),
        present: g.get("present").unwrap(),
        ipairs_n: g.get("ipairs_n").unwrap(),
    };
    (lua, side)
}

fn ev(n: u64) -> InputEvent {
    InputEvent {
        kind: EventKind::KeyDown {
            double_click: n % 2 == 0,
        },
        key: Key::Char(b'a' + (n % 26) as u8),
    }
}

fn rust_ipairs_n(e: &InputEvents) -> usize {
    let mut i = 0;
    let mut n = 0;
    while e.next_ipairs(&mut i).is_some() {
        n += 1;
    }
    n
}

/// Compare `#t`, presence of keys 0..=#t+6 and the `ipairs` count. Returns the number of
/// mismatching quantities.
fn compare(l: &LuaSide, r: &InputEvents, ctx: &str) -> usize {
    let mut bad = 0;
    let llen: i64 = l.len.call(()).unwrap();
    if llen as usize != r.lua_len() {
        eprintln!("{ctx}: #t lua={llen} rust={}", r.lua_len());
        bad += 1;
    }
    for i in 0..=(llen as usize + 6) {
        let lp: bool = l.present.call(i as i64).unwrap();
        if lp != r.get(i).is_some() {
            eprintln!("{ctx}: t[{i}] lua={lp} rust={}", r.get(i).is_some());
            bad += 1;
        }
    }
    let ln: i64 = l.ipairs_n.call(()).unwrap();
    if ln as usize != rust_ipairs_n(r) {
        eprintln!("{ctx}: ipairs lua={ln} rust={}", rust_ipairs_n(r));
        bad += 1;
    }
    bad
}

#[test]
fn lua_len_property_vs_luajit() {
    let (_lua, l) = boot();
    let mut rng = Rng(0x5eed_0004);
    let mut bad = 0;
    let mut checks = 0;
    let mut max_len = 0;
    let sequences = 2000;
    for s in 0..sequences {
        l.fresh.call::<()>(()).unwrap();
        let mut r = InputEvents::default();
        let ops = 10 + rng.below(60);
        // Bias some sequences towards long runs (array part growth: 3, 5, 9, 17, 33, ...).
        let ins_weight = if s % 4 == 0 { 85 } else { 55 };
        for op in 0..ops {
            let roll = rng.below(100);
            let what = if roll < ins_weight {
                l.ins.call::<()>(()).unwrap();
                r.push(ev(op));
                "ins".to_owned()
            } else if roll < 93 {
                // consume: mostly existing events, sometimes beyond the border or key 0
                let len = r.lua_len() as u64;
                let i = rng.below(len + 4);
                l.cons.call::<()>(i as i64).unwrap();
                r.consume(i as usize);
                format!("cons({i})")
            } else if roll < 98 {
                l.wipe.call::<()>(()).unwrap();
                r.wipe();
                "wipe".to_owned()
            } else {
                l.fresh.call::<()>(()).unwrap();
                r = InputEvents::default();
                "fresh".to_owned()
            };
            max_len = max_len.max(r.lua_len());
            checks += 1;
            bad += compare(&l, &r, &format!("seq {s} op {op} {what}"));
            assert_eq!(bad, 0, "divergence (see stderr)");
        }
    }
    eprintln!("tier0_len: {sequences} sequences, {checks} operations compared, max #t {max_len}, {bad} divergences");
    assert!(max_len >= 33);
}

/// The frame pattern PoB actually produces: events appended, some consumed by
/// ProcessControlsInput (holes), `#inputEvents` read (Build.lua:1216), then wiped (Main.lua:487).
#[test]
fn frame_pattern_borders() {
    let (_lua, l) = boot();
    let mut r = InputEvents::default();
    let mut rng = Rng(42);
    for frame in 0..300 {
        let n = rng.below(12);
        for k in 0..n {
            l.ins.call::<()>(()).unwrap();
            r.push(ev(k));
        }
        // ControlHost-style consumption while iterating with ipairs
        let mut i = 0;
        while r.next_ipairs(&mut i).is_some() {
            if rng.below(3) == 0 {
                l.cons.call::<()>(i as i64).unwrap();
                r.consume(i);
            }
        }
        assert_eq!(compare(&l, &r, &format!("frame {frame}")), 0);
        l.wipe.call::<()>(()).unwrap();
        r.wipe();
        assert_eq!(compare(&l, &r, &format!("frame {frame} wiped")), 0);
        assert_eq!(r.lua_len(), 0);
    }
}

/// A hole hides later events from a later `ipairs` loop, but not from the running one.
#[test]
fn holes_and_ipairs() {
    let mut r = InputEvents::default();
    for k in 0..4 {
        r.push(ev(k));
    }
    let mut i = 0;
    let mut seen = 0;
    while r.next_ipairs(&mut i).is_some() {
        if i == 2 {
            r.consume(i);
        }
        seen += 1;
    }
    assert_eq!(seen, 4);
    assert_eq!(rust_ipairs_n(&r), 1);
    // #t still sees the last element: the array part holds keys 1..4 with a hole at 2.
    assert_eq!(r.lua_len(), 4);
    r.push(ev(9));
    assert_eq!(r.get(5), Some(ev(9)));
}
