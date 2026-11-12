//! Unit DF-A acceptance (calc doc §11.1, row DF-A): the CalcDefence.lua 1-650 helpers
//! against PoB itself.
//!
//! * `dfa_random_vs_luajit`: PoB booted once under LuaJIT with the JIT off
//!   (`JitPob::boot_oracle`, A6); the same random inputs go through the Lua and the Rust
//!   port and the results are compared as `%.17g` text, byte for byte:
//!   - `hitChance`, `armourReductionF`, `armourReduction`: scalar inputs;
//!   - `applyDmgTakenConversion`: a random skill mod list (taken-as, taken INC/MORE,
//!     self-ignore and armour flags, ward-based reduction), a random output (resists,
//!     reductions, armour, ward; keys sometimes missing) and breakdown on/off;
//!   - `takenHitFromDamage`: a random actor (output, modDB, damage shift table). The shift
//!     table is iterated with `pairs`, whose order is PoB's own (random per process, calc
//!     doc §3.4) and the sum depends on it, so the Lua side reports its order and the Rust
//!     table is built in that order;
//!   - `reducePoolsByDamage` (+ `buildAllyLifePools`, `calcLifeHitPoolWithLossPrevention`):
//!     random pools, damage tables, flags and given pool tables (ward, ally pools with a
//!     damage type), then a second hit on the returned pools (the in-place mutation of the
//!     shared sub-tables is compared too) and `incomingDamageBreakdown` of the result
//!     (PoB's module-local function, exported by a second, otherwise unchanged copy of the
//!     module loaded in the oracle). The Rust damage tables are built in the Lua tables'
//!     `pairs` order: with a NaN pool the final `m_min` over `pairs(damageTable)` depends
//!     on it.
//!
//!   `DFA_N` sets the cases per function (default 100000), `DFA_ONLY=a,b` a subset.
//! * `dfa_resistances_probe`: for every corpus build with `defence:in` and
//!   `defence:1211` dumps of the same hit, inject `defence:in`, run the Rust
//!   `calcs.resistances` on the dumped actor and compare every output key it writes, the
//!   breakdown rows and the `<elem>Resist`/`<elem>ResistMax` mods it adds with the
//!   `defence:1211` state (CalcDefence 666-1210 writes none of those). The same
//!   dumps also check `defenceForConditionals` (its `<X>On<slot>` outputs, written by
//!   CalcPerform 3484 before `defence:in`). The dumps are the main env's, from one
//!   `pob_oracle probe` run per build selecting `defence:in@main,defence:1211@main` under
//!   `docs/port/stages.toml` (`testkit::probe_dumps`: the shared probe cache, keyed by the patched
//!   source, the build's bytes, the selection and the probe code; probed when absent). CK2
//!   (CCR-0147 §3): this read the keyless `.parity/probe/<stem>/state/` of a 2026-10-09
//!   `scripts/port/oracle.sh corpus` run, whose `defence:recovery:in` line probe (before
//!   CalcDefence.lua:1211, P3's stages) is `defence:1211` in today's `docs/port/stages.toml`
//!   (before 1212, the first statement after the `-- Recovery modifiers` comment: the same
//!   state).
//!
//! A difference only in the sign of a formatted zero is a host artefact (the oracle host is a
//! dual-number ARM64 LuaJIT; CCR-0002) and is counted apart, as is a `%d` of NaN, which the
//! port formats as x64's `cvttsd2si` MIN and the ARM64 oracle as 0 (CCR-0001). Errors PoB
//! raises are compared as such (`ERR` on both sides) and their messages summarised. A
//! missing input fails (`pob_testpaths::require`; `POB_ALLOW_SKIP=1` skips instead).
#![allow(non_snake_case)]
#![allow(
    clippy::too_many_lines,
    clippy::format_push_string,
    clippy::type_complexity,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::many_single_char_names
)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_calc::modules::calc_defence as calcs;
use pob_calc::prelude::*;
use pob_oracle::headless::{Engine, Ret};
use pob_oracle::jit::JitPob;
use pob_oracle::oracle::{find_parity_dir, find_pob_src};
use pob_oracle::testkit;

/// Lua `pcall`: a `lua_error!` becomes `Err(())`.
fn pcall<R>(f: impl FnOnce() -> R) -> Result<R, ()> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => Ok(r),
        Err(p) if p.downcast_ref::<LuaError>().is_some() => Err(()),
        Err(p) => std::panic::resume_unwind(p),
    }
}

/// splitmix64.
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
    fn chance(&mut self, p: f64) -> bool {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64 <= p
    }
    /// A number from a mix of edge values and random magnitudes.
    fn num(&mut self) -> f64 {
        const EDGE: &[f64] = &[
            0.0, 1.0, -1.0, 0.5, 2.25, 10.0, 37.5, -12.75, 100.0, 1234.5678, 0.001, 75.0, -30.0,
            0.125, 99.995, 1e6, -0.5, 3.0, 0.7, 14.0, 90.0, 50.0, 200.0,
        ];
        match self.below(4) {
            0 => EDGE[self.below(EDGE.len() as u64) as usize],
            1 => (self.below(20001) as f64 - 10000.0) / 100.0,
            2 => self.below(20001) as f64,
            _ => (self.below(2_000_001) as f64) / 7.0,
        }
    }
    /// A percentage-like value (mostly 0..100, sometimes outside).
    fn pct(&mut self) -> f64 {
        match self.below(6) {
            0 => 0.0,
            1 => 100.0,
            2 => (self.below(20001) as f64 - 5000.0) / 100.0,
            _ => self.below(101) as f64,
        }
    }
    /// A non-negative amount (pools, damage).
    fn amount(&mut self) -> f64 {
        match self.below(6) {
            0 => 0.0,
            1 => self.below(100) as f64,
            2 => (self.below(2_000_001) as f64) / 13.0,
            _ => self.below(20001) as f64,
        }
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len() as u64) as usize]
    }
}

/// A Lua number literal that reads back as exactly `x` (finite).
fn lnum(x: f64) -> String {
    format!("{x:?}")
}

fn f17(x: f64) -> String {
    lfmt!("%.17g", x)
}

/// `s` with every standalone `-0` number written as `0` (CT.rs).
fn unsign_zero(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'-'
            && b.get(i + 1) == Some(&b'0')
            && !b
                .get(i + 2)
                .is_some_and(|c| c.is_ascii_digit() || *c == b'.')
            && !(i > 0 && (b[i - 1].is_ascii_digit() || b[i - 1] == b'.'))
        {
            i += 1;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// One check's tally.
struct Check {
    name: &'static str,
    total: usize,
    errors: usize,
    bad: Vec<String>,
    zero_sign: usize,
    /// `%d` of NaN/inf (float→int cast) host artefacts.
    cast: usize,
}

const X64_MIN_INT: &str = "-9223372036854775808";

impl Check {
    fn new(name: &'static str) -> Self {
        Check {
            name,
            total: 0,
            errors: 0,
            bad: Vec::new(),
            zero_sign: 0,
            cast: 0,
        }
    }
    fn cmp(&mut self, what: &str, lua: &str, rust: &str) {
        self.total += 1;
        if lua.contains("ERR") {
            self.errors += 1;
        }
        if lua != rust {
            if unsign_zero(lua) == unsign_zero(rust) {
                self.zero_sign += 1;
            } else if unsign_zero(lua) == unsign_zero(&rust.replace(X64_MIN_INT, "0")) {
                // `%d` of a NaN: x64 `cvttsd2si` gives MIN (the port), the ARM64 oracle 0
                // (CCR-0001 table row 1)
                self.cast += 1;
            } else {
                // show the text around the first difference
                let at = lua
                    .bytes()
                    .zip(rust.bytes())
                    .position(|(a, b)| a != b)
                    .unwrap_or(lua.len().min(rust.len()));
                let from = at.saturating_sub(120);
                let cut = |t: &str| {
                    let b = t.as_bytes();
                    String::from_utf8_lossy(&b[from.min(b.len())..(at + 120).min(b.len())])
                        .into_owned()
                };
                self.bad.push(format!(
                    "{what} (first difference at byte {at})\n    lua  …{:?}…\n    rust …{:?}…",
                    cut(lua),
                    cut(rust)
                ));
            }
        }
    }
}

// ---- canonical serialisation (same text on both sides) ----------------------------------------

fn ser(v: &Val) -> String {
    match v {
        Val::Nil => "nil".to_owned(),
        Val::Bool(b) => b.to_string(),
        Val::Num(n) => f17(*n),
        Val::Str(s) => format!("\"{s}\""),
        Val::Table(t) => ser_table(t),
        other => format!("{other:?}"),
    }
}

fn ser_table(t: &Table) -> String {
    let mut rows: Vec<(Key, Val)> = t.pairs().collect();
    // Lua: numbers before strings (type names compare), then `<`.
    rows.sort_by(|(a, _), (b, _)| match (a, b) {
        (Key::S(x), Key::S(y)) => x.as_str().as_bytes().cmp(y.as_str().as_bytes()),
        (Key::S(_), _) => std::cmp::Ordering::Greater,
        (_, Key::S(_)) => std::cmp::Ordering::Less,
        (x, y) => {
            let (Val::Num(x), Val::Num(y)) = (x.to_val(), y.to_val()) else {
                return std::cmp::Ordering::Equal;
            };
            x.total_cmp(&y)
        }
    });
    let parts: Vec<String> = rows
        .into_iter()
        .map(|(k, v)| {
            let ks = match k {
                Key::S(s) => s.as_str().to_owned(),
                other => match other.to_val() {
                    Val::Num(n) => f17(n),
                    v => format!("{v:?}"),
                },
            };
            format!("{ks}={}", ser(&v))
        })
        .collect();
    format!("{{{}}}", parts.join(","))
}

const LUA_PRELUDE: &str = r#"
local calcs = require("Modules.CalcBase")
local function f17(x) return string.format("%.17g", x) end
local function ser(v)
  local t = type(v)
  if t == "number" then return f17(v)
  elseif t == "string" then return '"' .. v .. '"'
  elseif t == "boolean" then return tostring(v)
  elseif t == "nil" then return "nil"
  elseif t == "table" then
    local keys = {}
    for k in pairs(v) do keys[#keys+1] = k end
    table.sort(keys, function(a, b)
      local ta, tb = type(a), type(b)
      if ta ~= tb then return ta < tb end
      return a < b
    end)
    local parts = {}
    for _, k in ipairs(keys) do
      parts[#parts+1] = (type(k) == "string" and k or f17(k)) .. "=" .. ser(v[k])
    end
    return "{" .. table.concat(parts, ",") .. "}"
  end
  return t
end
local incomingDamageBreakdown = DFA_incomingDamageBreakdown
local function mkStore(cls, mods)
  local o = new(cls)
  local ml = o[cls](o)
  for _, m in ipairs(mods) do ml:AddMod(modLib.createMod(m[1], m[2], m[3], "DFA")) end
  return ml
end
DFA_ERRS = {}
local function err(e)
  local k = tostring(e):gsub("^.-:%d+: ", "")
  DFA_ERRS[k] = (DFA_ERRS[k] or 0) + 1
  return "ERR"
end
function DFA_ERR_SUMMARY()
  local rows = {}
  for k, v in pairs(DFA_ERRS) do rows[#rows+1] = { k, v } end
  table.sort(rows, function(a, b) return a[2] > b[2] end)
  local out = {}
  for i = 1, math.min(#rows, 6) do out[i] = rows[i][2] .. " x " .. rows[i][1] end
  DFA_ERRS = {}
  return table.concat(out, "\n")
end
function DFA_S(f, a, b) return f17(calcs[f](a, b)) end
function DFA_A(mods, out, bd, src, base)
  local ok, r = pcall(function()
    local skill = { skillModList = mkStore("ModList", mods) }
    local b, total = calcs.applyDmgTakenConversion(skill, out, bd and {} or nil, src, base)
    return ser(b) .. "|" .. f17(total)
  end)
  return ok and r or err(r)
end
function DFA_T(mods, out, dtype, shift, raw)
  local actor = { output = out, modDB = mkStore("ModDB", mods), damageShiftTable = { [dtype] = shift } }
  local order = {}
  for k in pairs(shift) do order[#order+1] = k end
  local ok, r = pcall(function()
    local s, d = calcs.takenHitFromDamage(raw, dtype, actor)
    return f17(s) .. "|" .. ser(d)
  end)
  return table.concat(order, ",") .. "\31" .. (ok and r or err(r))
end
local function keyOrder(t)
  local o = {}
  if t then for k in pairs(t) do o[#o+1] = k end end
  return table.concat(o, ",")
end
function DFA_P(mods, out, dmg, pool, dmg2)
  local order = keyOrder(dmg) .. ";" .. keyOrder(dmg2)
  local actor = { output = out, modDB = mkStore("ModDB", mods) }
  local r1
  local ok, r = pcall(function()
    r1 = calcs.reducePoolsByDamage(pool, dmg, actor)
    return ser(dmg) .. "|" .. ser(r1)
  end)
  local s = ok and r or err(r)
  if ok then
    if incomingDamageBreakdown then
      local okb, rb = pcall(function() return ser(incomingDamageBreakdown({ "x" }, r1, out)) end)
      s = s .. "|" .. (okb and rb or err(rb))
    else
      s = s .. "|-"
    end
    if dmg2 then
      local ok2, r2 = pcall(function()
        local p2 = calcs.reducePoolsByDamage(r1, dmg2, actor)
        return ser(dmg2) .. "|" .. ser(p2) .. "|" .. ser(r1)
      end)
      s = s .. "|" .. (ok2 and r2 or err(r2))
    end
  end
  return order .. "\31" .. s
end
"#;

/// PoB under LuaJIT.
struct Lua(JitPob);

impl Lua {
    fn eval(&mut self, code: &str) -> String {
        let r = self.0.run(code).unwrap_or_else(|e| panic!("lua: {e}"));
        match r.first() {
            Some(Ret::Str(s)) => s.clone(),
            other => panic!("lua returned {other:?}"),
        }
    }
    /// Run `calls` (Lua expressions returning a string) in batches; one result each.
    fn batch(&mut self, calls: &[String]) -> Vec<String> {
        let mut out = Vec::with_capacity(calls.len());
        for chunk in calls.chunks(400) {
            let mut code = String::from("local R = {}\n");
            for c in chunk {
                code.push_str("R[#R+1] = ");
                code.push_str(c);
                code.push('\n');
            }
            code.push_str("return table.concat(R, \"\\30\")");
            let r = self.eval(&code);
            let parts: Vec<String> = r.split('\x1e').map(str::to_owned).collect();
            assert_eq!(parts.len(), chunk.len(), "batch result count");
            out.extend(parts);
        }
        out
    }
}

// ---- inputs --------------------------------------------------------------------------------------

/// A mod `{ name, type, value }` (FLAG mods have value `true`).
#[derive(Clone)]
struct M(String, ModType, Option<f64>);

fn lua_mods(mods: &[M]) -> String {
    let parts: Vec<String> = mods
        .iter()
        .map(|M(n, t, v)| {
            format!(
                "{{\"{n}\",\"{}\",{}}}",
                t.as_str(),
                v.map_or("true".to_owned(), lnum)
            )
        })
        .collect();
    format!("{{{}}}", parts.join(","))
}

fn rust_store(db: bool, mods: &[M]) -> ModStoreRef {
    let s = if db {
        ModStore::ModDB(None)
    } else {
        ModStore::ModList(None)
    };
    for M(n, t, v) in mods {
        match v {
            Some(v) => s.NewMod(Name::from(n.as_str()), *t, *v, "DFA", 0, 0, &[]),
            None => s.NewMod(Name::from(n.as_str()), *t, true, "DFA", 0, 0, &[]),
        }
    }
    s
}

/// `{ ["k"] = v, … }`.
fn lua_kv(kv: &[(String, f64)]) -> String {
    let parts: Vec<String> = kv
        .iter()
        .map(|(k, v)| format!("[\"{k}\"]={}", lnum(*v)))
        .collect();
    format!("{{{}}}", parts.join(","))
}

fn rust_kv(kv: &[(String, f64)]) -> Table {
    let t = Table::new();
    for (k, v) in kv {
        t.set(Name::from(k.as_str()), *v);
    }
    t
}

const DT: [&str; 5] = ["Physical", "Lightning", "Cold", "Fire", "Chaos"];

fn probe_skill(player: &ActorRef) -> ActiveSkillRef {
    ActiveSkill::new(
        SkillEffect::new(
            pob_gamedata::data().skills[&n!("Fireball")].clone(),
            1.0,
            0.0,
        ),
        Shared::new(Vec::new()),
        Rc::downgrade(player),
    )
}

// ---- checks --------------------------------------------------------------------------------------

fn case_count() -> usize {
    std::env::var("DFA_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000)
}

fn check_scalar(lua: &mut Lua) -> Vec<Check> {
    let n = case_count();
    let mut rng = Rng(0xDF_A001);
    let mut checks = Vec::new();
    for f in ["hitChance", "armourReductionF", "armourReduction"] {
        let mut c = Check::new(match f {
            "hitChance" => "hitChance",
            "armourReductionF" => "armourReductionF",
            _ => "armourReduction",
        });
        let mut calls = Vec::with_capacity(n);
        let mut rust = Vec::with_capacity(n);
        let mut names = Vec::with_capacity(n);
        for i in 0..n {
            let (a, b) = if f == "hitChance" {
                let ev = match rng.below(5) {
                    0 => 0.0,
                    1 => -(rng.below(1000) as f64),
                    _ => rng.amount() * 3.0,
                };
                let acc = match rng.below(6) {
                    0 => 0.0,
                    1 => -(rng.below(100) as f64) - 0.5,
                    _ => rng.amount(),
                };
                (ev, acc)
            } else {
                let ar = match rng.below(6) {
                    0 => 0.0,
                    1 => -(rng.amount()),
                    _ => rng.amount() * 5.0,
                };
                let raw = match rng.below(6) {
                    0 => 0.0,
                    1 => -(rng.amount()),
                    2 => rng.num(),
                    _ => rng.amount(),
                };
                (ar, raw)
            };
            calls.push(format!("DFA_S(\"{f}\",{},{})", lnum(a), lnum(b)));
            let r = match f {
                "hitChance" => calcs::hitChance(a, b),
                "armourReductionF" => calcs::armourReductionF(a, b),
                _ => calcs::armourReduction(a, b),
            };
            rust.push(f17(r));
            names.push(format!("{f}({a:?}, {b:?}) #{i}"));
        }
        let l = lua.batch(&calls);
        let errs = lua.eval("return DFA_ERR_SUMMARY()");
        if !errs.is_empty() {
            eprintln!("  errors PoB raised (count x message):\n{errs}");
        }
        for ((w, a), b) in names.iter().zip(&l).zip(&rust) {
            c.cmp(w, a, b);
        }
        checks.push(c);
    }
    checks
}

fn check_adtc(lua: &mut Lua) -> Check {
    let n = case_count();
    let mut c = Check::new("applyDmgTakenConversion");
    let mut rng = Rng(0xDF_A002);
    let player = Actor::new();
    let skill = probe_skill(&player);
    let bdStore = ModStore::ModDB(None);
    let bd = newBreakdown(&bdStore, &Table::new(), &player);
    let sources = ["Physical", "Lightning", "Cold", "Fire", "Chaos"];
    let mut calls = Vec::with_capacity(n);
    let mut rust = Vec::with_capacity(n);
    let mut names = Vec::with_capacity(n);
    for i in 0..n {
        let src = *rng.pick(&sources);
        let mut mods: Vec<M> = Vec::new();
        for _ in 0..rng.below(4) {
            let to = *rng.pick(&DT);
            let name = match rng.below(4) {
                0 => format!("{src}DamageTakenAs{to}"),
                1 => format!("{src}DamageFromHitsTakenAs{to}"),
                2 => format!("ElementalDamageTakenAs{to}"),
                _ => format!("ElementalDamageFromHitsTakenAs{to}"),
            };
            mods.push(M(name, ModType::BASE, Some(rng.pct())));
        }
        for _ in 0..rng.below(4) {
            let dt = *rng.pick(&DT);
            let name = match rng.below(6) {
                0 => "DamageTaken".to_owned(),
                1 => format!("{dt}DamageTaken"),
                2 => "DamageTakenWhenHit".to_owned(),
                3 => format!("{dt}DamageTakenWhenHit"),
                4 => "ElementalDamageTaken".to_owned(),
                _ => "ElementalDamageTakenWhenHit".to_owned(),
            };
            let t = if rng.chance(0.5) {
                ModType::INC
            } else {
                ModType::MORE
            };
            mods.push(M(name, t, Some(rng.pct() - 30.0)));
        }
        for _ in 0..rng.below(3) {
            let dt = *rng.pick(&DT);
            let m = match rng.below(5) {
                0 => M(
                    format!("SelfIgnoreBase{dt}DamageReduction"),
                    ModType::FLAG,
                    None,
                ),
                1 => M(format!("SelfIgnore{dt}Resistance"), ModType::FLAG, None),
                2 => M(
                    format!("ArmourDoesNotApplyTo{dt}DamageTaken"),
                    ModType::FLAG,
                    None,
                ),
                _ => M(
                    format!("ArmourAppliesTo{dt}DamageTaken"),
                    ModType::BASE,
                    Some(rng.pct()),
                ),
            };
            mods.push(m);
        }
        if rng.chance(0.4) {
            mods.push(M(
                "ArmourAppliesToPhysicalDamageTaken".to_owned(),
                ModType::BASE,
                Some(100.0),
            ));
        }
        if rng.chance(0.08) {
            mods.push(M(
                "PhysicalReductionBasedOnWard".to_owned(),
                ModType::FLAG,
                None,
            ));
            if rng.chance(0.9) {
                mods.push(M(
                    "PhysicalReductionBasedOnWardPercent".to_owned(),
                    ModType::OVERRIDE,
                    Some(rng.pct()),
                ));
            }
        }
        let mut out: Vec<(String, f64)> = Vec::new();
        let miss = rng.chance(0.03);
        for dt in DT {
            if rng.chance(0.3) {
                out.push((format!("Base{dt}DamageReductionWhenHit"), rng.pct()));
            }
            if !(miss && rng.chance(0.3)) {
                out.push((format!("Base{dt}DamageReduction"), rng.pct()));
            }
            if rng.chance(0.3) {
                out.push((format!("{dt}ResistWhenHit"), rng.pct()));
            }
            if !(miss && rng.chance(0.3)) {
                out.push((format!("{dt}Resist"), rng.pct() - 20.0));
            }
        }
        if !(miss && rng.chance(0.3)) {
            out.push(("Armour".to_owned(), rng.amount() * 3.0));
        }
        out.push(("ArmourDefense".to_owned(), rng.below(3) as f64 * 0.5));
        out.push(("Ward".to_owned(), rng.amount()));
        out.push((
            "DamageReductionMax".to_owned(),
            *rng.pick(&[90.0, 75.0, 50.0, 100.0]),
        ));
        let useBd = rng.chance(0.5);
        let base = if rng.chance(0.1) {
            0.0
        } else {
            rng.num() * 3.0
        };
        calls.push(format!(
            "DFA_A({},{},{},\"{src}\",{})",
            lua_mods(&mods),
            lua_kv(&out),
            useBd,
            lnum(base)
        ));
        skill.skillModList.set(rust_store(false, &mods));
        let output = rust_kv(&out);
        let r = pcall(|| {
            let (b, total) = calcs::applyDmgTakenConversion(
                &skill,
                &output,
                if useBd { Some(&bd) } else { None },
                Name::from(src),
                base,
            );
            format!("{}|{}", ser_table(&b), f17(total))
        })
        .unwrap_or_else(|()| "ERR".to_owned());
        rust.push(r);
        names.push(format!("adtc #{i} src={src} base={base:?}"));
    }
    let l = lua.batch(&calls);
    let errs = lua.eval("return DFA_ERR_SUMMARY()");
    if !errs.is_empty() {
        eprintln!("  errors PoB raised (count x message):\n{errs}");
    }
    for ((w, a), b) in names.iter().zip(&l).zip(&rust) {
        c.cmp(w, a, b);
    }
    c
}

struct ThCase {
    mods: Vec<M>,
    out: Vec<(String, f64)>,
    dtype: &'static str,
    shift: Vec<(String, f64)>,
    raw: f64,
}

fn check_taken_hit(lua: &mut Lua) -> Check {
    let n = case_count();
    let mut c = Check::new("takenHitFromDamage");
    let mut rng = Rng(0xDF_A003);
    let mut cases = Vec::with_capacity(n);
    let mut calls = Vec::with_capacity(n);
    for _ in 0..n {
        let dtype = *rng.pick(&DT);
        let mut shift: Vec<(String, f64)> = Vec::new();
        for dt in DT {
            if dt == dtype || rng.chance(0.4) {
                let p = match rng.below(4) {
                    0 => 0.0,
                    1 => 100.0,
                    _ => rng.below(101) as f64,
                };
                shift.push((dt.to_owned(), p));
            }
        }
        let mut out: Vec<(String, f64)> = Vec::new();
        let miss = rng.chance(0.03);
        for (dt, _) in &shift {
            // PoB always sets `<type>takenFlat` (a nil one raises in `+ takenFlat`)
            if !(miss && rng.chance(0.5)) {
                out.push((
                    format!("{dt}takenFlat"),
                    if rng.chance(0.6) { 0.0 } else { rng.num() },
                ));
            }
            if !(miss && rng.chance(0.3)) {
                out.push((
                    format!("{dt}ResistTakenHitMulti"),
                    rng.below(300) as f64 / 100.0,
                ));
            }
            if !(miss && rng.chance(0.3)) {
                out.push((
                    format!("{dt}EffectiveAppliedArmour"),
                    if rng.chance(0.3) {
                        0.0
                    } else {
                        rng.amount() * 3.0
                    },
                ));
            }
            if rng.chance(0.3) {
                out.push((format!("Base{dt}DamageReductionWhenHit"), rng.pct()));
            }
            if !(miss && rng.chance(0.3)) {
                out.push((format!("Base{dt}DamageReduction"), rng.pct()));
            }
            if !(miss && rng.chance(0.3)) {
                out.push((format!("{dt}EnemyOverwhelm"), rng.pct() / 2.0));
            }
            if !(miss && rng.chance(0.3)) {
                out.push((
                    format!("{dt}AfterReductionTakenHitMulti"),
                    rng.below(300) as f64 / 100.0,
                ));
            }
        }
        out.push((
            "DamageReductionMax".to_owned(),
            *rng.pick(&[90.0, 75.0, 50.0, 100.0]),
        ));
        let mut mods: Vec<M> = Vec::new();
        for _ in 0..rng.below(3) {
            let dt = *rng.pick(&DT);
            mods.push(if rng.chance(0.5) {
                M(
                    format!("SelfIgnoreBase{dt}DamageReduction"),
                    ModType::FLAG,
                    None,
                )
            } else {
                M(
                    format!("SelfIgnore{dt}DamageReduction"),
                    ModType::FLAG,
                    None,
                )
            });
        }
        if rng.chance(0.2) {
            mods.push(M(
                "VaalArcticArmourMitigation".to_owned(),
                ModType::MORE,
                Some(-(rng.below(150) as f64)),
            ));
        }
        let raw = if rng.chance(0.1) { 0.0 } else { rng.amount() };
        calls.push(format!(
            "DFA_T({},{},\"{dtype}\",{},{})",
            lua_mods(&mods),
            lua_kv(&out),
            lua_kv(&shift),
            lnum(raw)
        ));
        cases.push(ThCase {
            mods,
            out,
            dtype,
            shift,
            raw,
        });
    }
    let l = lua.batch(&calls);
    let errs = lua.eval("return DFA_ERR_SUMMARY()");
    if !errs.is_empty() {
        eprintln!("  errors PoB raised (count x message):\n{errs}");
    }
    for (i, (case, lr)) in cases.iter().zip(&l).enumerate() {
        let (order, lres) = lr.split_once('\x1f').expect("order separator");
        // the Rust shift table in PoB's pairs order
        let shift = Table::new();
        for k in order.split(',').filter(|k| !k.is_empty()) {
            let v = case
                .shift
                .iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| *v)
                .expect("shift key");
            shift.set(Name::from(k), v);
        }
        let actor = Actor::new();
        actor.output.set(rust_kv(&case.out));
        actor.modDB.set(rust_store(true, &case.mods));
        let dst = Table::new();
        dst.set(Name::from(case.dtype), shift);
        actor.damageShiftTable.set(Some(dst));
        let r = pcall(|| {
            let (s, d) = calcs::takenHitFromDamage(case.raw, Name::from(case.dtype), &actor);
            format!("{}|{}", f17(s), ser_table(&d))
        })
        .unwrap_or_else(|()| "ERR".to_owned());
        c.cmp(
            &format!("takenHit #{i} {} raw={:?}", case.dtype, case.raw),
            lres,
            &r,
        );
    }
    c
}

/// A given `poolTable`: scalar fields plus ally pools `{ remaining, percent, damageType? }`.
struct PoolSpec {
    fields: Vec<(String, f64)>,
    allies: Vec<(String, f64, f64, Option<&'static str>)>,
}

fn lua_pool(p: &PoolSpec) -> String {
    let mut s = String::from("{");
    for (k, v) in &p.fields {
        let _ = write!(s, "[\"{k}\"]={},", lnum(*v));
    }
    if !p.allies.is_empty() {
        s.push_str("AlliesTakenBeforeYou={");
        for (k, rem, pct, dt) in &p.allies {
            let _ = write!(
                s,
                "[\"{k}\"]={{remaining={},percent={}{}}},",
                lnum(*rem),
                lnum(*pct),
                dt.map_or(String::new(), |d| format!(",damageType=\"{d}\""))
            );
        }
        s.push('}');
    }
    s.push('}');
    s
}

fn rust_pool(p: &PoolSpec) -> Table {
    let t = rust_kv(&p.fields);
    if !p.allies.is_empty() {
        let a = Table::new();
        for (k, rem, pct, dt) in &p.allies {
            let e = Table::new();
            e.set(n!("remaining"), *rem);
            e.set(n!("percent"), *pct);
            if let Some(d) = dt {
                e.set(n!("damageType"), *d);
            }
            a.set(Name::from(k.as_str()), e);
        }
        t.set(n!("AlliesTakenBeforeYou"), a);
    }
    t
}

const ALLIES: [(&str, &str, &str); 9] = [
    ("minion", "TotalMinionLife", "MinionAllyDamageMitigation"),
    (
        "radianceSentinel",
        "TotalRadianceSentinelLife",
        "RadianceSentinelAllyDamageMitigation",
    ),
    (
        "spectres",
        "TotalSpectreLife",
        "SpectreAllyDamageMitigation",
    ),
    ("totems", "TotalTotemLife", "TotemAllyDamageMitigation"),
    (
        "vaalRejuvenationTotems",
        "TotalVaalRejuvenationTotemLife",
        "VaalRejuvenationTotemAllyDamageMitigation",
    ),
    (
        "voidSpawn",
        "TotalVoidSpawnLife",
        "VoidSpawnAllyDamageMitigation",
    ),
    (
        "stoneGolem",
        "TotalStoneGolemLife",
        "StoneGolemAllyDamageMitigation",
    ),
    ("soulLink", "AlliedEnergyShield", "SoulLinkMitigation"),
    (
        "frostShield",
        "FrostShieldLife",
        "FrostShieldDamageMitigation",
    ),
];

fn rand_damage(rng: &mut Rng) -> Vec<(String, f64)> {
    let mut d = Vec::new();
    for dt in DT {
        if rng.chance(0.5) {
            let v = match rng.below(8) {
                0 => 0.0,
                1 => -(rng.below(100) as f64),
                2 => rng.below(100) as f64 + 0.25,
                _ => rng.amount(),
            };
            d.push((dt.to_owned(), v));
        }
    }
    d
}

fn check_pools(lua: &mut Lua) -> Check {
    let n = case_count();
    let mut c = Check::new("reducePoolsByDamage");
    let mut rng = Rng(0xDF_A004);
    let mut calls = Vec::with_capacity(n);
    let mut rust = Vec::with_capacity(n);
    let mut names = Vec::with_capacity(n);
    let mut cases = Vec::with_capacity(n);
    for i in 0..n {
        let mut out: Vec<(String, f64)> = Vec::new();
        let miss = rng.chance(0.02);
        let life = (rng.below(10000) + 1) as f64;
        out.push(("Life".to_owned(), life));
        out.push((
            "LifeRecoverable".to_owned(),
            if rng.chance(0.7) {
                life
            } else {
                rng.below(life as u64 + 1) as f64
            },
        ));
        let prevented = if rng.chance(0.6) {
            0.0
        } else {
            rng.below(60) as f64
        };
        out.push(("preventedLifeLoss".to_owned(), prevented));
        let below = if rng.chance(0.6) {
            0.0
        } else {
            rng.below(60) as f64
        };
        if !rng.chance(0.05) {
            out.push(("preventedLifeLossBelowHalf".to_owned(), below));
        }
        out.push((
            "preventedLifeLossTotal".to_owned(),
            if rng.chance(0.2) {
                rng.below(50) as f64
            } else {
                prevented + below
            },
        ));
        out.push(("ManaUnreserved".to_owned(), rng.amount()));
        if !(miss && rng.chance(0.5)) {
            out.push(("EnergyShieldRecoveryCap".to_owned(), rng.amount()));
        }
        if rng.chance(0.4) {
            out.push(("Ward".to_owned(), rng.amount()));
        }
        out.push((
            "sharedMindOverMatter".to_owned(),
            if rng.chance(0.5) {
                0.0
            } else {
                rng.below(101) as f64
            },
        ));
        for dt in DT {
            if !(miss && rng.chance(0.2)) {
                out.push((
                    format!("{dt}MindOverMatter"),
                    if rng.chance(0.6) {
                        0.0
                    } else {
                        rng.below(101) as f64
                    },
                ));
            }
            if !(miss && rng.chance(0.2)) {
                out.push((
                    format!("{dt}EnergyShieldBypass"),
                    match rng.below(5) {
                        0 | 1 => 0.0,
                        2 => 100.0,
                        _ => rng.below(101) as f64,
                    },
                ));
            }
            if rng.chance(0.15) {
                out.push((format!("{dt}Aegis"), rng.amount()));
            }
            if rng.chance(0.15) {
                out.push((format!("{dt}GuardAbsorb"), rng.amount()));
            }
            if !(miss && rng.chance(0.2)) {
                out.push((format!("{dt}GuardAbsorbRate"), rng.below(101) as f64));
            }
        }
        if rng.chance(0.2) {
            out.push(("sharedAegis".to_owned(), rng.amount()));
        }
        if rng.chance(0.2) {
            out.push(("sharedElementalAegis".to_owned(), rng.amount()));
        }
        if rng.chance(0.2) {
            out.push(("sharedGuardAbsorb".to_owned(), rng.amount()));
        }
        out.push(("sharedGuardAbsorbRate".to_owned(), rng.below(101) as f64));
        for (_, l, m) in ALLIES {
            if rng.chance(0.15) {
                out.push((
                    l.to_owned(),
                    if rng.chance(0.1) { 0.0 } else { rng.amount() },
                ));
                out.push((m.to_owned(), rng.pct()));
            }
        }
        let mut mods: Vec<M> = Vec::new();
        if rng.chance(0.15) {
            mods.push(M("Condition:WardNotBreak".to_owned(), ModType::FLAG, None));
        }
        if rng.chance(0.2) {
            mods.push(M(
                "EnergyShieldProtectsMana".to_owned(),
                ModType::FLAG,
                None,
            ));
        }
        for name in [
            "WardAvoidBreakChance",
            "WardBypassBelowPercent",
            "LifeLossBelowHalfPrevented",
            "WardBypass",
        ] {
            if rng.chance(0.2) {
                mods.push(M(
                    name.to_owned(),
                    ModType::BASE,
                    Some(rng.below(101) as f64),
                ));
            }
        }
        let dmg = rand_damage(&mut rng);
        let pool = if rng.chance(0.3) {
            let mut fields = Vec::new();
            for k in [
                "Life",
                "Mana",
                "EnergyShield",
                "Ward",
                "WardActiveChance",
                "LifeLossLostOverTime",
                "LifeBelowHalfLossLostOverTime",
            ] {
                if rng.chance(0.4) {
                    let v = if k == "WardActiveChance" {
                        rng.below(101) as f64 / 100.0
                    } else if rng.chance(0.05) {
                        -(rng.below(100) as f64)
                    } else {
                        rng.amount()
                    };
                    fields.push((k.to_owned(), v));
                }
            }
            let mut allies = Vec::new();
            if rng.chance(0.3) {
                for (k, l, m) in ALLIES {
                    if rng.chance(0.2) {
                        // incomingDamageBreakdown reads the ally's life from the output
                        if !out.iter().any(|(o, _)| o == l) && rng.chance(0.9) {
                            out.push((l.to_owned(), rng.amount()));
                            out.push((m.to_owned(), rng.pct()));
                        }
                        allies.push((
                            k.to_owned(),
                            rng.amount(),
                            rng.below(101) as f64 / 100.0,
                            if rng.chance(0.5) {
                                Some(*rng.pick(&DT))
                            } else {
                                None
                            },
                        ));
                    }
                }
            }
            Some(PoolSpec { fields, allies })
        } else {
            None
        };
        // a second hit on the returned pools: mostly the same damage types (the returned
        // Aegis/Guard tables only hold the first hit's positive types; another raises in PoB)
        let dmg2 = rng.chance(0.4).then(|| {
            if rng.chance(0.85) {
                dmg.iter()
                    .filter(|(_, v)| *v > 0.0)
                    .map(|(k, _)| (k.clone(), rng.amount()))
                    .collect()
            } else {
                rand_damage(&mut rng)
            }
        });
        calls.push(format!(
            "DFA_P({},{},{},{},{})",
            lua_mods(&mods),
            lua_kv(&out),
            lua_kv(&dmg),
            pool.as_ref().map_or("nil".to_owned(), lua_pool),
            dmg2.as_ref().map_or("nil".to_owned(), |d| lua_kv(d)),
        ));
        cases.push((mods, out, dmg, pool, dmg2));
        names.push(format!("pools #{i}"));
    }
    let l = lua.batch(&calls);
    let errs = lua.eval("return DFA_ERR_SUMMARY()");
    if !errs.is_empty() {
        eprintln!("  errors PoB raised (count x message):\n{errs}");
    }
    // The Rust damage tables are built in PoB's `pairs` order of the Lua ones: with a NaN
    // pool, `m_min` over `pairs(damageTable)` (CalcDefence 401-404) depends on it.
    let mut lres: Vec<String> = Vec::with_capacity(n);
    for ((mods, out, dmg, pool, dmg2), lr) in cases.iter().zip(&l) {
        let (orders, lr) = lr.split_once('\x1f').expect("order separator");
        lres.push(lr.to_owned());
        let (o1, o2) = orders.split_once(';').expect("order pair");
        let ordered = |kv: &[(String, f64)], order: &str| -> Table {
            let t = Table::new();
            for k in order.split(',').filter(|k| !k.is_empty()) {
                let v = kv
                    .iter()
                    .find(|(n, _)| n == k)
                    .map(|(_, v)| *v)
                    .expect("damage key");
                t.set(Name::from(k), v);
            }
            t
        };
        let actor = Actor::new();
        let output = rust_kv(out);
        actor.output.set(output.clone());
        actor.modDB.set(rust_store(true, mods));
        let dmgT = ordered(dmg, o1);
        let poolT = pool.as_ref().map(rust_pool);
        let s = match pcall(|| calcs::reducePoolsByDamage(poolT.as_ref(), &dmgT, &actor)) {
            Ok(r1) => {
                let mut s = format!("{}|{}", ser_table(&dmgT), ser_table(&r1));
                let bd = pcall(|| {
                    let t = Table::new();
                    t.push("x");
                    ser_table(&calcs::helpers::incoming_damage_breakdown_probe(
                        &t, &r1, &output,
                    ))
                })
                .unwrap_or_else(|()| "ERR".to_owned());
                s.push('|');
                s.push_str(&bd);
                if let Some(d2) = dmg2 {
                    let d2T = ordered(d2, o2);
                    let r2 = pcall(|| {
                        let p2 = calcs::reducePoolsByDamage(Some(&r1), &d2T, &actor);
                        format!("{}|{}|{}", ser_table(&d2T), ser_table(&p2), ser_table(&r1))
                    })
                    .unwrap_or_else(|()| "ERR".to_owned());
                    s.push('|');
                    s.push_str(&r2);
                }
                s
            }
            Err(()) => "ERR".to_owned(),
        };
        rust.push(s);
    }
    let l = lres;
    if let Some(k) = std::env::var("DFA_SHOW")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
    {
        eprintln!("call {}\nlua  {}\nrust {}", calls[k], l[k], rust[k]);
    }
    for ((w, a), b) in names.iter().zip(&l).zip(&rust) {
        c.cmp(w, a, b);
    }
    c
}

#[test]
fn dfa_random_vs_luajit() {
    if pob_testpaths::require(
        testkit::init_default_data().then_some(()),
        "data/dump not found",
    )
    .is_none()
    {
        return; // POB_ALLOW_SKIP=1
    }
    let Some(src) = pob_testpaths::require(find_pob_src(), "PoB checkout not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    let _lock = pob_oracle::runner::OracleLock::acquire().unwrap_or_else(|e| panic!("{e}"));
    let t0 = std::time::Instant::now();
    let pob = JitPob::boot_oracle(&src, None).unwrap_or_else(|e| panic!("boot: {e}"));
    let mut lua = Lua(pob);
    // `incomingDamageBreakdown` is a module local of CalcDefence.lua and the oracle has no
    // `debug` library: load a second copy of the module (unchanged, onto a table that
    // inherits from PoB's `calcs`, so PoB's own functions stay as they are) that exports it.
    let text = std::fs::read_to_string(src.join("Modules/CalcDefence.lua"))
        .unwrap_or_else(|e| panic!("CalcDefence.lua: {e}"));
    assert!(!text.contains("]=====]"));
    let head = "local calcs = require(\"Modules.CalcBase\")";
    assert!(text.contains(head), "CalcDefence.lua header changed");
    let tail = text
        .rfind("return calcs")
        .expect("CalcDefence.lua ends with `return calcs`");
    let copy = format!(
        "{}DFA_incomingDamageBreakdown = incomingDamageBreakdown\n{}",
        &text[..tail],
        &text[tail..]
    )
    .replacen(
        head,
        "local calcs = setmetatable({ }, { __index = require(\"Modules.CalcBase\") })",
        1,
    );
    lua.0
        .run(&format!(
            "assert(loadstring([=====[{copy}]=====], \"=CalcDefence.lua(copy)\"))()\nreturn 'ok'"
        ))
        .unwrap_or_else(|e| panic!("CalcDefence copy: {e}"));
    lua.0
        .run(&format!("{LUA_PRELUDE}\nreturn 'ok'"))
        .unwrap_or_else(|e| panic!("prelude: {e}"));
    assert_eq!(
        lua.eval("return type(DFA_incomingDamageBreakdown)"),
        "function",
        "incomingDamageBreakdown export"
    );
    eprintln!("PoB booted in {:?}", t0.elapsed());
    let only = std::env::var("DFA_ONLY").ok();
    let want = |name: &str| {
        only.as_deref()
            .is_none_or(|o| o.split(',').any(|x| x == name))
    };
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut checks: Vec<Check> = Vec::new();
    let run = |name: &str, f: &mut dyn FnMut() -> Vec<Check>| -> Vec<Check> {
        let t = std::time::Instant::now();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        match r {
            Ok(cs) => {
                eprintln!("{name}: {:?}", t.elapsed());
                cs
            }
            Err(p) => {
                let msg = p
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                    .or_else(|| {
                        p.downcast_ref::<LuaError>()
                            .map(|e| format!("LuaError: {}", e.0))
                    })
                    .unwrap_or_default();
                eprintln!("check {name} panicked: {msg}");
                panic!("check {name} panicked: {msg}");
            }
        }
    };
    if want("scalar") {
        checks.extend(run("scalar", &mut || check_scalar(&mut lua)));
    }
    if want("adtc") {
        checks.extend(run("adtc", &mut || vec![check_adtc(&mut lua)]));
    }
    if want("taken") {
        checks.extend(run("taken", &mut || vec![check_taken_hit(&mut lua)]));
    }
    if want("pools") {
        checks.extend(run("pools", &mut || vec![check_pools(&mut lua)]));
    }
    std::panic::set_hook(hook);
    let mut failed = false;
    for c in &checks {
        eprintln!(
            "== {}: {} compared ({} raise a Lua error in PoB), {} differ, {} sign-of-zero and {} `%d`-of-NaN host artefacts",
            c.name,
            c.total,
            c.errors,
            c.bad.len(),
            c.zero_sign,
            c.cast
        );
        for b in c.bad.iter().take(8) {
            eprintln!("  {b}");
        }
        failed |= !c.bad.is_empty();
    }
    assert!(!failed, "DF-A oracle mismatches (see above)");
    // coverage floors and bucket bounds per check (HYG2/V2)
    if only.is_none() {
        let mut names: Vec<&str> = checks.iter().map(|c| c.name).collect();
        names.sort_unstable();
        let mut want: Vec<&str> = DFA_BOUNDS.iter().map(|b| b.0).collect();
        want.sort_unstable();
        assert_eq!(names, want, "checks run");
    }
    for c in &checks {
        let &(_, min_total, max_errors, max_zero, max_cast) = DFA_BOUNDS
            .iter()
            .find(|b| b.0 == c.name)
            .unwrap_or_else(|| panic!("no DFA_BOUNDS row for {}", c.name));
        assert!(
            c.total >= min_total,
            "{}: {} compared, expected at least {min_total}",
            c.name,
            c.total
        );
        assert!(
            c.errors <= max_errors && c.zero_sign <= max_zero && c.cast <= max_cast,
            "{}: {} raise in PoB / {} sign-of-zero / {} %d-of-NaN, bounds {max_errors} / {max_zero} / {max_cast}",
            c.name,
            c.errors,
            c.zero_sign,
            c.cast
        );
    }
}

/// `(check, min compared, max raise-in-PoB, max sign-of-zero, max %d-of-NaN)`; measured by HYG2
/// on rust-port 746b103.
const DFA_BOUNDS: &[(&str, usize, usize, usize, usize)] = &[
    ("hitChance", 100_000, 0, 0, 0),
    ("armourReductionF", 100_000, 0, 1947, 0),
    ("armourReduction", 100_000, 0, 0, 0),
    ("applyDmgTakenConversion", 100_000, 1723, 0, 0),
    ("takenHitFromDamage", 100_000, 2770, 0, 0),
    ("reducePoolsByDamage", 100_000, 6774, 28, 34),
];

// ---- resistances / defenceForConditionals on the corpus dumps --------------------------------------

fn corpus(parity: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(parity.join("builds"))
        .map(|r| {
            r.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "xml"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    if let Ok(list) = std::env::var("DFA_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| want.contains(&stem(p)));
    }
    v
}

fn stem(p: &Path) -> &str {
    p.file_stem().and_then(|s| s.to_str()).unwrap_or_default()
}

/// The line probe before CalcDefence.lua:1212 (`docs/port/stages.toml`; module docs).
const RECOVERY_IN: &str = "defence:1211";

fn hit_no(hit: &str) -> &str {
    hit.rsplit_once('#').map_or("", |(_, n)| n)
}

/// The rows of `modDB:Tabulate(nil, nil, name)` as sorted `type value source` text.
fn mod_rows(db: &ModStoreRef, name: Name) -> Vec<String> {
    let mut v: Vec<String> = db
        .Tabulate(None, None, &[name])
        .into_iter()
        .map(|r| {
            format!(
                "{} {} {}",
                r.r#mod.r#type.as_str(),
                r.value.num().map_or_else(|| format!("{:?}", r.value), f17),
                r.r#mod.source.as_deref().unwrap_or("nil")
            )
        })
        .collect();
    v.sort();
    v
}

const RESIST_KEYS: [&str; 10] = [
    "{e}Resist",
    "{e}ResistTotal",
    "{e}ResistOverCap",
    "{e}ResistOver75",
    "Missing{e}Resist",
    "{e}ResistOverTime",
    "Totem{e}Resist",
    "Totem{e}ResistTotal",
    "Totem{e}ResistOverCap",
    "MissingTotem{e}Resist",
];

const SLOTS: [&str; 6] = [
    "Helmet",
    "Gloves",
    "Boots",
    "Body Armour",
    "Weapon 2",
    "Weapon 3",
];

#[test]
fn dfa_resistances_probe() {
    if pob_testpaths::require(
        testkit::init_default_data().then_some(()),
        "data/dump not found",
    )
    .is_none()
    {
        return; // POB_ALLOW_SKIP=1
    }
    let Some(parity) = pob_testpaths::require(find_parity_dir(), "no .parity directory") else {
        return; // POB_ALLOW_SKIP=1
    };
    let builds = corpus(&parity);
    if builds.is_empty() {
        let _ = pob_testpaths::require(None::<()>, "no .parity/builds/*.xml");
        return; // POB_ALLOW_SKIP=1
    }
    let filtered = std::env::var_os("DFA_BUILDS").is_some() || pob_testpaths::quick_corpus();
    if !filtered {
        assert_eq!(builds.len(), CORPUS_BUILDS, ".parity/builds/*.xml count");
    }
    let mut compared = 0usize;
    let mut keys = 0usize;
    let mut cond_keys = 0usize;
    let mut converted = 0usize;
    let mut missing: Vec<String> = Vec::new();
    let mut bad: Vec<String> = Vec::new();
    let mut cond_bad: Vec<String> = Vec::new();
    for xml in &builds {
        let st = stem(xml);
        let held = match testkit::probe_dumps(
            Path::new(env!("CARGO_BIN_EXE_pob_oracle")),
            xml,
            Some(&testkit::default_stages()),
            &testkit::selection(&["defence:in@main", &format!("{RECOVERY_IN}@main")]),
        ) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("{st}: probe failed: {e}");
                missing.push(st.to_owned());
                continue;
            }
        };
        let (Some(din), Some(dout)) = (
            testkit::load_dump(held.dirs(), "defence:in"),
            testkit::load_dump(held.dirs(), RECOVERY_IN),
        ) else {
            missing.push(st.to_owned());
            continue;
        };
        assert_eq!(
            hit_no(&din.hit),
            hit_no(&dout.hit),
            "{st}: defence:in / {RECOVERY_IN} hits differ"
        );
        let build = testkit::empty_build();
        let inj_out = pob_oracle::inject::inject(&dout, &build)
            .unwrap_or_else(|e| panic!("{st}: inject {}: {e}", dout.hit));
        let actor_out = inj_out.locals["actor"].obj::<Actor>().expect("actor local");
        let inj = pob_oracle::inject::inject(&din, &build)
            .unwrap_or_else(|e| panic!("{st}: inject {}: {e}", din.hit));
        let actor = inj.locals["actor"].obj::<Actor>().expect("actor local");
        let env = inj.locals["env"].obj::<Env>().or(inj.env.clone());
        // defenceForConditionals: drop its outputs, run it again, compare.
        let output = actor.output.get();
        let mut cond: Vec<(Name, Val)> = Vec::new();
        for kind in ["Ward", "EnergyShield", "Armour", "Evasion"] {
            for slot in SLOTS {
                let k = cat!(kind, "On", slot);
                cond.push((k, output.get(k)));
                output.set(k, Val::Nil);
            }
        }
        if let Some(env) = &env {
            if let Err(()) = pcall(|| calcs::defenceForConditionals(env, &actor)) {
                cond_bad.push(format!("{st}: defenceForConditionals raised"));
            }
        }
        for (k, v) in cond {
            if !v.is_nil() {
                cond_keys += 1;
            }
            let ours = output.get(k);
            if ser(&ours) != ser(&v) {
                cond_bad.push(format!("{st} {k}: pob={} ours={}", ser(&v), ser(&ours)));
            }
            // keep the dumped value for the rest of the run
            output.set(k, v);
        }
        // resistances (its output keys cleared first, so a stale value cannot pass)
        output.set(n!("PhysicalResist"), Val::Nil);
        for e in ["Fire", "Cold", "Lightning", "Chaos"] {
            for k in RESIST_KEYS {
                output.set(Name::from(k.replace("{e}", e).as_str()), Val::Nil);
            }
        }
        if pcall(|| calcs::resistances(&actor)).is_err() {
            bad.push(format!("{st}: resistances raised"));
            continue;
        }
        compared += 1;
        let po = actor_out.output.get();
        let mut kl: Vec<Name> = vec![n!("PhysicalResist")];
        for e in ["Fire", "Cold", "Lightning", "Chaos"] {
            for k in RESIST_KEYS {
                kl.push(Name::from(k.replace("{e}", e).as_str()));
            }
        }
        for k in kl {
            keys += 1;
            let (a, b) = (ser(&po.get(k)), ser(&output.get(k)));
            if a != b {
                bad.push(format!("{st} {} output.{k}: pob={a} ours={b}", din.hit));
            }
        }
        match (actor_out.breakdown.get(), actor.breakdown.get()) {
            (Some(pb), Some(ob)) => {
                for e in ["Fire", "Cold", "Lightning", "Chaos"] {
                    for k in [cat!(e, "Resist"), cat!("Totem", e, "Resist")] {
                        let (a, b) = (ser(&pb.get(k)), ser(&ob.get(k)));
                        keys += 1;
                        if a != b {
                            bad.push(format!("{st} breakdown.{k}: pob={a} ours={b}"));
                        }
                    }
                }
            }
            (None, None) => {}
            _ => bad.push(format!("{st}: breakdown presence differs")),
        }
        let (pdb, odb) = (actor_out.modDB.get(), actor.modDB.get());
        for e in ["Fire", "Cold", "Lightning", "Chaos"] {
            for k in [cat!(e, "Resist"), cat!(e, "ResistMax")] {
                let (a, b) = (mod_rows(&pdb, k), mod_rows(&odb, k));
                converted += b
                    .iter()
                    .filter(|r| r.contains("Conversion") || r.contains("Melding"))
                    .count();
                if a != b {
                    bad.push(format!("{st} mods {k}:\n    pob  {a:?}\n    ours {b:?}"));
                }
            }
        }
    }
    eprintln!(
        "resistances: {compared} actors ({} builds without defence dumps), {keys} values compared, {} differ; \
         {converted} conversion/Melding mods added; defenceForConditionals: {cond_keys} slot outputs, {} differ",
        missing.len(),
        bad.len(),
        cond_bad.len()
    );
    if !missing.is_empty() {
        eprintln!("  no defence:in/{RECOVERY_IN} dump: {missing:?}");
    }
    for b in bad.iter().chain(&cond_bad).take(30) {
        eprintln!("  {b}");
    }
    if compared == 0 {
        let _ = pob_testpaths::require(
            None::<()>,
            &format!("no defence:in / {RECOVERY_IN} dumps (probe failures above)"),
        );
        return; // POB_ALLOW_SKIP=1
    }
    assert!(
        bad.is_empty() && cond_bad.is_empty(),
        "DF-A probe mismatches (see above)"
    );
    // coverage (HYG2/V2): every build has its defence dumps, and every one was compared
    assert!(
        missing.is_empty(),
        "builds without defence:in / {RECOVERY_IN} dumps: {missing:?}"
    );
    assert_eq!(compared, builds.len(), "actors compared");
    if !filtered {
        assert!(
            keys >= DFA_MIN_RESIST_VALUES,
            "{keys} resistance values compared, expected at least {DFA_MIN_RESIST_VALUES}"
        );
    }
}

/// The corpus size (`.parity/builds/*.xml`).
const CORPUS_BUILDS: usize = 57;
/// `dfa_resistances_probe` values compared on the full corpus; measured by HYG2 on rust-port
/// 746b103.
const DFA_MIN_RESIST_VALUES: usize = 2793;
