//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/CalcSections.lua (as data, from calc:P1's dump schema `calc_sections`).
//! Owner: U39.
//!
//! @pob-file Modules/CalcSections.lua @16de4b82d
//!
//! `LoadModule("Modules/CalcSections")` (CalcsTab.lua:151) returns the section list: one
//! `{ width, id, group, colour, subSection[, updateFunc] }` array per section. The file is pure
//! data (its only function slot, `[6]`, is unused by every entry), so the dump
//! (`data().calcSections`, calc:P1) is the source: [`load`] turns it into fresh calc
//! [`Table`]s, which the Calcs tab then annotates in place the way the Lua does
//! (`colData.calcSection`, `colData.x`, `rowData.enabled`, `subSec.collapsed`, ...).
//!
//! Identity: the module's shared lists (`physicalHitTaken`, `manaCost`, ...; the dump's `$ref`s)
//! are one Lua table each, referenced from several cells. [`load`] keeps that: every reference to
//! one dumped table resolves to the same calc table. The colour escapes were evaluated when the
//! dump was made (`colorCodes.OFFENCE` etc., Data/Global.lua defaults), as `LoadModule` evaluates
//! them when the tab is constructed.
//!
//! DIVERGENCE(colours): a colour code changed by `updateColorCode` (Data/Global.lua, the Options
//! popup) before the tab is constructed changes the module's strings in PoB; the dump holds the
//! defaults.
//!
//! ORDER: no `pairs` loops (the sections, subsections, rows and cells are sequences).

use std::collections::HashMap;
use std::sync::Arc;

use pob_lua::{lua_error, Name};

use crate::model::{self, gamedata::DataValue, Table, Val};

// @pob-skip Modules/CalcSections.lua:1-2638 reason="data module: loaded from the calc:P1 dump (`data().calcSections`) by `load`"

/// One `CalcSections` entry, `unpack(section)` (CalcsTab.lua:153).
pub struct SectionDef {
    /// `[1]` width in columns
    pub width: f64,
    /// `[2]` section id
    pub id: String,
    /// `[3]` group
    pub group: f64,
    /// `[4]` colour escape
    pub colour: String,
    /// `[5]` the subsections (`{ defaultCollapsed, label, data }` tables)
    pub sub_section: Table,
}

/// The dumped table behind a `$ref` path (`CalcSections[1][5][1].data[6][2][2].modName`).
fn resolve(path: &str) -> DataValue {
    let root = &model::data().calcSections.0;
    let rest = path
        .strip_prefix("CalcSections")
        .unwrap_or_else(|| lua_error!("CalcSections: unresolvable reference {path}"));
    let mut cur = root.clone();
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let next = match b[i] {
            b'[' => {
                let j = rest[i..].find(']').map_or(b.len(), |j| i + j);
                let n: f64 = rest[i + 1..j].parse().unwrap_or(f64::NAN);
                i = j + 1;
                cur.table().map(|t| t.index(n).clone())
            }
            b'.' => {
                let j = rest[i + 1..]
                    .find(['.', '['])
                    .map_or(b.len(), |j| i + 1 + j);
                let k = &rest[i + 1..j];
                i = j;
                cur.table().map(|t| t.field(k).clone())
            }
            _ => None,
        };
        cur = match next {
            Some(DataValue::Ref(p)) => resolve(&p),
            Some(v) => v,
            None => lua_error!("CalcSections: unresolvable reference {path}"),
        };
    }
    cur
}

/// Dumped value -> calc value, one calc table per dumped table (keyed by the dump's `Arc`).
struct Conv {
    memo: HashMap<*const model::gamedata::DataTable, Table>,
}

impl Conv {
    fn val(&mut self, v: &DataValue) -> Val {
        match v {
            DataValue::Nil => Val::Nil,
            DataValue::Bool(b) => Val::Bool(*b),
            DataValue::Num(n) => Val::Num(*n),
            DataValue::Str(s) => Val::from(&**s),
            DataValue::Table(t) => Val::Table(self.table(t)),
            DataValue::Ref(p) => {
                let r = resolve(p);
                self.val(&r)
            }
            // CalcSections.lua holds no functions (field [6] is unused)
            DataValue::Fn(_) => lua_error!("CalcSections: unexpected function value"),
        }
    }

    fn table(&mut self, t: &Arc<model::gamedata::DataTable>) -> Table {
        let key = Arc::as_ptr(t);
        if let Some(out) = self.memo.get(&key) {
            return out.clone();
        }
        let out = Table::new();
        self.memo.insert(key, out.clone());
        for (k, x) in &t.fields {
            let v = self.val(x);
            out.set(Name::from(&k[..]), v);
        }
        for x in &t.seq {
            let v = self.val(x);
            out.push(v);
        }
        for (k, x) in &t.num {
            let v = self.val(x);
            out.set(*k, v);
        }
        out
    }
}

/// `LoadModule("Modules/CalcSections")` (CalcsTab.lua:151): fresh tables on every call, as each
/// `LoadModule` runs the chunk again.
pub fn load() -> Vec<SectionDef> {
    let root = &model::data().calcSections.0;
    let Some(sections) = root.table() else {
        lua_error!("CalcSections: not a table")
    };
    let mut conv = Conv {
        memo: HashMap::new(),
    };
    let mut out = Vec::with_capacity(sections.seq.len());
    for section in &sections.seq {
        let section = match section {
            DataValue::Ref(p) => resolve(p),
            s => s.clone(),
        };
        let Some(t) = section.table() else {
            lua_error!("CalcSections: a section is not a table")
        };
        let width = t.index(1.0).num().unwrap_or_else(|| {
            lua_error!("attempt to perform arithmetic on local 'width' (a nil value)")
        });
        let id = t.index(2.0).as_str().unwrap_or_default().to_owned();
        let group = t.index(3.0).num().unwrap_or(f64::NAN);
        let colour = t.index(4.0).as_str().unwrap_or_default().to_owned();
        let sub_section = match conv.val(t.index(5.0)) {
            Val::Table(s) => s,
            _ => lua_error!("attempt to index local 'subSection' (a nil value)"),
        };
        out.push(SectionDef {
            width,
            id,
            group,
            colour,
            sub_section,
        });
    }
    out
}
