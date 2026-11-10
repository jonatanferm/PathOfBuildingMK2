//! @pob-file Classes/CalcsTab.lua @16de4b82d
//! CalcsTab model load/save (Classes/CalcsTab.lua 163-225).
//!
//! Owner: BM-A. `CalcsTab` (model/mod.rs, U0) has no field for the section list Load and Save
//! read (`self.sectionList`: the `CalcSectionControl`s of the constructor, CalcsTab.lua 45-155,
//! whose `subSection[i].collapsed` state is saved). [`CalcsTab::sectionList`] keeps the model
//! half of that list (section `id`, subsection `id` and `collapsed`) in a per-tab side table
//! (CCR-0080 §3). [`CalcsTab::ctor_model`] runs the constructor's model half (`input`
//! defaults, the section list).
#![allow(non_snake_case, clippy::all, clippy::pedantic)]

use std::cell::RefCell;

use pob_gamedata::DataValue;

use crate::model::skills_tab::tostring_val;
use crate::model::{XmlChild, XmlNode};
use crate::prelude::*;

thread_local! {
    /// Per-tab section lists, keyed by the tab's address; the owner handle (`calcsTab.input`)
    /// detects a reused address.
    static SECTION_LISTS: RefCell<ahash::AHashMap<usize, (Table, Table)>> =
        RefCell::new(ahash::AHashMap::new());
}

/// `launch:ShowErrMsg(fmt, ...)`: the launcher's error popup (UI); the calc prints it.
fn ShowErrMsg(s: &str) {
    eprintln!("{s}");
}

/// `subSec.label:gsub("%W", "")` (CalcSectionControl.lua:34).
fn subsection_id(label: &str) -> String {
    pob_lua::pattern::gsub(label, "%W", "", None).0
}

/// @pob Classes/CalcSectionControl.lua:20-49 h=cfdd67db
/// One `CalcSectionControl` model: `{ id = …, subSection = { { id = …, collapsed = … }, … } }`.
fn new_section(id: &str, subSections: &[(String, Val)]) -> Table {
    let section = Table::new();
    section.set(n!("id"), id);
    let list = Table::new();
    for (subId, collapsed) in subSections {
        let sub = Table::new();
        sub.set(n!("id"), &**subId);
        // CalcSectionControl.lua:49 subSec.collapsed = subSec.defaultCollapsed
        sub.set(n!("collapsed"), collapsed.clone());
        list.push(sub);
    }
    section.set(n!("subSection"), list);
    section
}

/// @pob Classes/CalcsTab.lua:24-40 h=79e5157a
// @pob-skip Classes/CalcsTab.lua:41-42 reason="the search EditControl (UI, ui doc CalcsTab unit)"
/// @pob Classes/CalcsTab.lua:45-45 h=6c3c4839
/// @pob Classes/CalcsTab.lua:151-154 h=f22e4c5a
/// @pob Classes/CalcsTab.lua:370-375 h=d92da281
impl CalcsTab {
    /// The constructor's model half (idempotent): `input.skill_number = 1`,
    /// `input.misc_buffMode = "EFFECTIVE"` and the section list (the "SkillSelect" section,
    /// CalcsTab.lua:45, then every `Modules/CalcSections` entry, 151-154, through
    /// `NewSection` 370-375 and `CalcSectionControl` 20-49). Controls are the UI's.
    pub fn ctor_model(&self) {
        let key = self as *const CalcsTab as usize;
        let fresh = SECTION_LISTS.with(|m| {
            let m = m.borrow();
            !m.get(&key)
                .is_some_and(|(owner, _)| owner.ptr_eq(&self.input))
        });
        if !fresh {
            return;
        }
        // L34 self.input = { } (the frozen `input` handle)
        // L35
        self.input.set(n!("skill_number"), 1.0);
        // L36
        self.input.set(n!("misc_buffMode"), "EFFECTIVE");
        // L39
        let sectionList = Table::new();
        // L45 Special section for skill/mode selection
        sectionList.push(new_section(
            "SkillSelect",
            &[(subsection_id("View Skill Details"), Val::Bool(false))],
        ));
        // L150 Add sections from the CalcSections module
        let sectionData = &data().calcSections.0;
        let Some(sectionData) = sectionData.table() else {
            lua_error!("CalcSections: not a table")
        };
        // L152
        for section in &sectionData.seq {
            // L153 self:NewSection(unpack(section)): width, id, group, colour, subSection
            let Some(section) = section.table() else {
                lua_error!("CalcSections: a section is not a table")
            };
            let id = section.index(2.0).as_str().unwrap_or_default().to_owned();
            let mut subs = Vec::new();
            if let Some(subSection) = section.index(5.0).table() {
                for subSec in &subSection.seq {
                    let label = match subSec.field("label") {
                        DataValue::Str(s) => s.to_string(),
                        _ => lua_error!("attempt to index field 'label' (a nil value)"),
                    };
                    let collapsed = match subSec.field("defaultCollapsed") {
                        DataValue::Bool(b) => Val::Bool(*b),
                        DataValue::Nil => Val::Nil,
                        _ => Val::Bool(true),
                    };
                    subs.push((subsection_id(&label), collapsed));
                }
            }
            sectionList.push(new_section(&id, &subs));
        }
        SECTION_LISTS.with(|m| {
            let mut m = m.borrow_mut();
            m.insert(key, (self.input.clone(), sectionList));
        });
    }

    /// `self.sectionList` (model half; see the module doc).
    pub fn sectionList(&self) -> Table {
        self.ctor_model();
        let key = self as *const CalcsTab as usize;
        SECTION_LISTS
            .with(|m| m.borrow().get(&key).map(|(_, l)| l.clone()))
            .expect("ctor_model ran")
    }
}

/// @pob Classes/CalcsTab.lua:163-201 h=cc26990e
impl CalcsTab {
    /// Lua returns `true` on a malformed section (Build:Init then closes the build); the frozen
    /// signature returns `()`, so the error is reported through [`CalcsTab::Load_ret`].
    pub fn Load(&self, build: &BuildRef, xml: &crate::model::XmlNode, dbFileName: &str) {
        let _ = self.Load_ret(build, xml, dbFileName);
    }

    /// `CalcsTab:Load` with its return value (`true` = error; CCR-0080 §4).
    pub fn Load_ret(&self, build: &BuildRef, xml: &XmlNode, dbFileName: &str) -> bool {
        let _ = build;
        let sectionList = self.sectionList();
        // L164
        for node in &xml.children {
            // L165
            let XmlChild::Node(node) = node else { continue };
            // L166
            if &*node.elem == "Input" {
                // L167
                let Some(name) = node.attrib.get("name") else {
                    // L168
                    ShowErrMsg(&format!(
                        "^1Error parsing '{dbFileName}': 'Input' element missing name attribute"
                    ));
                    // L169
                    return true;
                };
                let name = Name::from(&**name);
                // L171
                if let Some(number) = node.attrib.get("number") {
                    // L172
                    self.input
                        .set(name, pob_lua::tonumber(number).map_or(Val::Nil, Val::Num));
                } else if let Some(string) = node.attrib.get("string") {
                    // L174
                    self.input.set(name, Val::Str(string.clone()));
                } else if let Some(boolean) = node.attrib.get("boolean") {
                    // L176
                    self.input.set(name, &**boolean == "true");
                } else {
                    // L178
                    ShowErrMsg(&format!(
                        "^1Error parsing '{dbFileName}': 'Input' element missing number, string or boolean attribute"
                    ));
                    // L179
                    return true;
                }
            } else if &*node.elem == "Section" {
                // L182
                let Some(id) = node.attrib.get("id") else {
                    // L183
                    ShowErrMsg(&format!(
                        "^1Error parsing '{dbFileName}': 'Section' element missing id attribute"
                    ));
                    // L184
                    return true;
                };
                // L186
                for (_, section) in sectionList.ipairs() {
                    let Some(section) = section.table() else {
                        continue;
                    };
                    // L187
                    if section.str(n!("id")).is_some_and(|s| *s == **id)
                        && node.attrib.get("subsection").is_some()
                    {
                        let subsection = node.attrib.get("subsection").expect("checked");
                        // L188
                        let subs = section.table(n!("subSection")).unwrap_or_default();
                        for (_, sub) in subs.ipairs() {
                            let Some(sub) = sub.table() else { continue };
                            // L189
                            if sub.str(n!("id")).is_some_and(|s| *s == **subsection) {
                                // L190
                                sub.set(
                                    n!("collapsed"),
                                    node.attrib.get("collapsed").is_some_and(|c| &**c == "true"),
                                );
                                // L191
                                break;
                            }
                        }
                        // L194
                        break;
                    }
                }
            }
        }
        // L200 UI: self:ResetUndo()
        false
    }
}

/// @pob Classes/CalcsTab.lua:203-224 h=5c8ce9dd
impl CalcsTab {
    pub fn Save(&self, xml: &mut crate::model::XmlNode) {
        let sectionList = self.sectionList();
        // L204 (string keys iterate in insertion order; PoB's `pairs` order is random, §3.4)
        // LUA-ORDER
        for (k, v) in self.input.pairs() {
            // L205
            let mut child = XmlNode {
                elem: LStr::from("Input"),
                attrib: indexmap::IndexMap::new(),
                children: Vec::new(),
            };
            let kname: LStr = match k {
                Key::S(n) => LStr::from(&*n),
                Key::I(i) => LStr::from(tostring_num(i as f64)),
                Key::B(b) => LStr::from(if b { "true" } else { "false" }),
            };
            child.attrib.insert(LStr::from("name"), kname);
            match &v {
                // L206
                Val::Num(x) => {
                    // L207
                    child
                        .attrib
                        .insert(LStr::from("number"), LStr::from(tostring_num(*x)));
                }
                // L208
                Val::Bool(b) => {
                    // L209
                    child.attrib.insert(
                        LStr::from("boolean"),
                        LStr::from(if *b { "true" } else { "false" }),
                    );
                }
                _ => {
                    // L211
                    let s = match &v {
                        Val::Str(s) => s.to_string(),
                        other => lua_error!(
                            "CalcsTab:Save: tostring of a {} input value",
                            other.lua_type()
                        ),
                    };
                    child.attrib.insert(LStr::from("string"), LStr::from(s));
                }
            }
            // L213
            xml.children.push(XmlChild::Node(child));
        }
        // L215
        for (_, section) in sectionList.ipairs() {
            let Some(section) = section.table() else {
                continue;
            };
            let subs = section.table(n!("subSection")).unwrap_or_default();
            // L216
            for (_, subSection) in subs.ipairs() {
                let Some(subSection) = subSection.table() else {
                    continue;
                };
                // L217
                let mut attrib = indexmap::IndexMap::new();
                // L218-219 (a nil id leaves the attribute out, as a nil table field does)
                for (name, owner) in [("id", &section), ("subsection", &subSection)] {
                    match owner.get(n!("id")) {
                        Val::Nil => {}
                        v => {
                            attrib.insert(LStr::from(name), LStr::from(tostring_val(&v)));
                        }
                    }
                }
                // L220
                let collapsed = tostring_val(&subSection.get(n!("collapsed")));
                attrib.insert(LStr::from("collapsed"), LStr::from(collapsed));
                xml.children.push(XmlChild::Node(XmlNode {
                    elem: LStr::from("Section"),
                    attrib,
                    children: Vec::new(),
                }));
            }
        }
    }
}
