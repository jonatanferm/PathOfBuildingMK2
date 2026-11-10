//! @pob-file Classes/ConfigTab.lua @16de4b82d
//! ConfigTab model (Classes/ConfigTab.lua 839-1129).
//!
//! Owner: BM-B. Load/Save of the `<Config>` section, `GetDefaultState`,
//! `UpdateControls` (over the control-state tables in `varControls`) and
//! `UpdateLevel`. `BuildModList` (1130-1204) is CO-C's.
//!
//! Model representation and contract gaps (CCR-0019):
//! * a config set is a table `{ id, title, input, placeholder, customModsList }`
//!   in `self.configSets`;
//! * Lua reassigns `self.input`/`self.placeholder` to the active set's tables
//!   (SetActiveConfigSet, ConfigTab.lua:1348-1349), but the frozen fields are
//!   plain `Table`s. Workaround: `self.input` *is* the active set's `input`
//!   table; switching sets gives the previously active set a copy of its
//!   contents and moves the new set's contents into `self.input`
//!   ([`ConfigTab::bind_active_set`]). Equivalent unless someone keeps the
//!   old table across a switch (an env built before it).
//! * `self.defaultState` (built by the constructor from `ConfigOptions`,
//!   ConfigTab.lua:683-692) is not a field: [`defaultState`] recomputes it from
//!   `data.configOptions`.
//! * `NewConfigSet` (1280-1300) and the model half of `SetActiveConfigSet`
//!   (1337-1362) are ported here because Load needs them.
#![allow(non_snake_case, clippy::all, clippy::pedantic)]

use std::cell::RefCell;

use crate::model::skills_tab::tostring_val;
use crate::model::{XmlChild, XmlNode};
use crate::prelude::*;

fn attr<'a>(node: &'a XmlNode, k: &str) -> Option<&'a str> {
    node.attrib.get(k).map(|v| &**v)
}

/// `self.configSets[id]` key (integral ids).
fn set_key(id: f64) -> Option<u32> {
    (id.fract() == 0.0 && id >= 0.0 && id <= f64::from(u32::MAX)).then_some(id as u32)
}

/// `node[1] or ""` (a text child; an element child would be a table).
fn first_text(node: &XmlNode) -> Val {
    match node.children.first() {
        None => Val::from(""),
        Some(XmlChild::Text(s)) => Val::Str(s.clone()),
        Some(XmlChild::Node(_)) => {
            lua_error!("CCR-0019: element child where config text was expected")
        }
    }
}

thread_local! {
    static DEFAULT_STATE: RefCell<Option<(usize, Table)>> = const { RefCell::new(None) };
}

/// @pob Classes/ConfigTab.lua:683-692 h=6a0e1445
/// `self.defaultState` as the constructor fills it from `varList` (every non-section
/// option with a `var`; later options with the same var overwrite earlier ones).
pub fn defaultState(data: &'static Data) -> Table {
    let key = data as *const Data as usize;
    if let Some(t) = DEFAULT_STATE.with(|d| {
        d.borrow()
            .as_ref()
            .and_then(|(k, t)| (*k == key).then(|| t.clone()))
    }) {
        return t;
    }
    let t = Table::new();
    for varData in &data.configOptions {
        if varData.section.is_some() {
            continue;
        }
        let Some(var) = varData.var else { continue };
        let ds = varData
            .defaultState
            .as_ref()
            .map_or(Val::Nil, Val::from_value);
        let ty = varData.r#type.map(|t| t.as_str());
        // L683
        let v = if ty == Some("check") {
            // L684
            lor(ds, || Val::Bool(false))
        } else if matches!(ty, Some("count" | "integer" | "countAllowZero" | "float")) {
            // L686
            lor(ds, || Val::Num(0.0))
        } else if ty == Some("list") {
            // L688
            let idx = varData.defaultIndex.unwrap_or(1.0);
            list_val(varData, idx)
        } else if ty == Some("text") {
            // L690
            lor(ds, || Val::from(""))
        } else {
            // L692
            ds
        };
        t.set(var, v);
    }
    DEFAULT_STATE.with(|d| *d.borrow_mut() = Some((key, t.clone())));
    t
}

/// `varData.list[i].val`.
fn list_val(varData: &pob_gamedata::schema::config_defs::ConfigOptionDef, i: f64) -> Val {
    let list = varData
        .list
        .as_ref()
        .unwrap_or_else(|| lua_error!("attempt to index field 'list' (a nil value)"));
    if i < 1.0 || i.fract() != 0.0 {
        lua_error!("attempt to index a nil value");
    }
    match list.get(i as usize - 1) {
        Some(item) => Val::from_value(&item.val),
        None => lua_error!("attempt to index a nil value"),
    }
}

impl ConfigTab {
    /// `self.configSets[id]`.
    pub fn configSet(&self, id: f64) -> Option<Table> {
        let k = set_key(id)?;
        self.configSets.with(|m| m.get(&k).cloned())
    }

    /// The constructor's config-set half (ConfigTab.lua:145-148): `configSets = { }`,
    /// `configSetOrderList = { 1 }`, `NewConfigSet(1)`, `SetActiveConfigSet(1, true)`.
    /// `ConfigTab::default()` leaves `configSets` empty, which PoB never does after its
    /// constructor (Load refills it, L886/L891), so an empty `configSets` means the
    /// constructor has not run (CCR-0019 §10). Idempotent.
    pub fn ctor_model(&self) {
        if self.configSets.with(|m| !m.is_empty()) {
            return;
        }
        // L146
        self.configSetOrderList.wipe();
        self.configSetOrderList.push(1);
        // L147
        let configSet = self.NewConfigSet(Some(1.0), None);
        // L148 (SetActiveConfigSet(1, true): bind input/placeholder; `build.buildFlag = true`
        // is moot in the constructor, the build has not computed yet)
        self.bind_active_set(&configSet);
        self.activeConfigSetId.set(1.0);
    }

    /// `self.configSets[self.activeConfigSetId]`.
    pub fn activeConfigSet(&self) -> Table {
        self.ctor_model();
        self.configSet(self.activeConfigSetId.get())
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
    }

    /// CCR-0019: make `self.input`/`self.placeholder` the tables of `configSet` (Lua:
    /// `self.input = configSet.input`). The set that held them so far keeps a copy.
    pub fn bind_active_set(&self, configSet: &Table) {
        for (field, live) in [
            (n!("input"), &self.input),
            (n!("placeholder"), &self.placeholder),
        ] {
            let Some(t) = configSet.table(field) else {
                lua_error!("attempt to index field '{field}' (a nil value)")
            };
            if t.ptr_eq(live) {
                continue;
            }
            let holders: Vec<Table> = self.configSets.with(|m| {
                m.values()
                    .filter(|s| s.table(field).is_some_and(|x| x.ptr_eq(live)))
                    .cloned()
                    .collect()
            });
            if !holders.is_empty() {
                let copy = live.copyTable(true);
                for h in holders {
                    h.set(field, copy.clone());
                }
            }
            live.wipe();
            for (k, v) in t.pairs() {
                live.set(k, v);
            }
            configSet.set(field, live.clone());
        }
    }
}

/// @pob Classes/ConfigTab.lua:672-681 h=c9190b17
/// The ConfigTab constructor's control half (CCR-0080 §1; the proposal of CCR-0037/CCR-0038):
/// one control-state table per option with a `var` in `self.varControls`, with the fields the
/// model reads (`_className`, `enabled`, `buf`, `state`, `placeholder`, `list`, `selIndex`).
/// Owner BM-B (CCR-0037); moved here from BM-A's `build_xml` as CCR-0080 §10 anticipates, so
/// a model built with `Build::new` can run the constructor too (BuildModList's applies index
/// these controls, e.g. ConfigOptions.lua:789 `varControls['enemyRadius']:SetPlaceholder`).
/// The control classes' constructors contribute `enabled = true` (Control.lua:60),
/// `buf = ""` (EditControl.lua:48 `SetText(init or "")`, init is `""`), `list = varData.list`
/// and `selIndex = 1` (DropDownControl.lua:45-46). The input/placeholder defaults of L673/L676/L679
/// are `NewConfigSet`'s (ConfigTab.lua:1289-1296, already applied by `ctor_model`), and
/// `defaultState` (L683-692) is `config_tab::defaultState`.
impl ConfigTab {
    pub fn ctor_controls_model(&self, data: &'static Data) {
        let varControls = &self.varControls;
        // L261 for _, varData in ipairs(varList) do (section headers carry no var)
        for varData in &data.configOptions {
            if varData.section.is_some() {
                continue;
            }
            let control = Table::new();
            // ConfigTab.lua:280-347: the control class the option's type selects
            let className = match varData.r#type.as_ref().map(|t| t.as_str()) {
                Some("check") => "CheckBoxControl",
                Some("count" | "integer" | "countAllowZero" | "float") => "EditControl",
                Some("list") => "DropDownControl",
                // `varData.resizable` is never set in ConfigOptions (the schema has no such field)
                Some("text") => "EditControl",
                _ => "Control",
            };
            control.set(n!("_className"), className);
            control.set(n!("enabled"), true);
            if className == "EditControl" {
                control.set(n!("buf"), "");
            } else if className == "DropDownControl" {
                let list = Table::new();
                for item in varData.list.iter().flatten() {
                    let e = Table::new();
                    e.set(n!("label"), &*item.label);
                    e.set(n!("val"), Val::from_value(&item.val));
                    list.push(e);
                }
                control.set(n!("list"), list);
                control.set(n!("selIndex"), 1.0);
            }
            // L672
            let Some(var) = varData.var else { continue };
            // L674
            control.set(
                n!("state"),
                varData
                    .defaultState
                    .as_ref()
                    .map_or(Val::Nil, Val::from_value),
            );
            // L677
            control.set(
                n!("placeholder"),
                varData
                    .defaultPlaceholderState
                    .as_ref()
                    .map_or(Val::Nil, Val::from_value),
            );
            // L678
            if let Some(defaultIndex) = varData.defaultIndex {
                // L680
                control.set(n!("selIndex"), defaultIndex);
            }
            // L675
            varControls.set(var, control);
        }
    }
}

/// @pob Classes/ConfigTab.lua:839-939 h=6d012674
impl ConfigTab {
    pub fn Load(&self, build: &BuildRef, xml: &crate::model::XmlNode, fileName: &str) {
        let _ = fileName;
        // L840
        self.activeConfigSetId.set(1.0);
        // L841
        self.configSets.with_mut(|m| m.clear());
        // L842 (`{ 1 }`; holes from L906 are kept in a Lua-like table, then projected below)
        let configSetOrderList = Table::new();
        configSetOrderList.push(1.0);

        let configSetTable = |id: f64| -> Table {
            self.configSet(id)
                .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
        };
        // L844
        let setInputAndPlaceholder = |node: Option<&XmlNode>, configSetId: f64| -> bool {
            // a text child has no `elem`
            let Some(node) = node else { return false };
            // L845
            if &*node.elem == "Input" {
                // L846
                let Some(name) = attr(node, "name") else {
                    // L847 launch:ShowErrMsg (UI)
                    // L848
                    return true;
                };
                let name = Name::from(name);
                let input = configSetTable(configSetId)
                    .table(n!("input"))
                    .expect("NewConfigSet");
                // L850
                if let Some(number) = attr(node, "number") {
                    // L851
                    input.set(name, pob_lua::tonumber(number));
                } else if let Some(string) = attr(node, "string") {
                    // L853
                    if name.as_str() == "enemyIsBoss" {
                        // L854
                        let s = pob_lua::string::lower(string);
                        let s = pob_lua::pattern::gsub_fn(
                            &s,
                            "(%l)(%w*)",
                            |c| {
                                Some(format!(
                                    "{}{}",
                                    pob_lua::string::upper(&c[0].to_str()),
                                    c[1].to_str()
                                ))
                            },
                            None,
                        )
                        .0;
                        // L855
                        let s = pob_lua::pattern::gsub(&s, "Uber Atziri", "Boss", None).0;
                        let s = pob_lua::pattern::gsub(&s, "Shaper", "Pinnacle", None).0;
                        let s = pob_lua::pattern::gsub(&s, "Sirus", "Pinnacle", None).0;
                        input.set(name, s.as_str());
                    // backwards compat <=3.20, Uber Atziri Flameblast -> Atziri Flameblast
                    } else if name.as_str() == "presetBossSkills" {
                        // L858
                        input.set(
                            name,
                            pob_lua::pattern::gsub(string, "^Uber ", "", None)
                                .0
                                .as_str(),
                        );
                    } else {
                        // L860
                        input.set(name, string);
                    }
                } else if let Some(boolean) = attr(node, "boolean") {
                    // L863
                    input.set(name, boolean == "true");
                } else {
                    // L865 launch:ShowErrMsg (UI)
                    // L866
                    return true;
                }
            } else if &*node.elem == "Placeholder" {
                // L869
                let Some(name) = attr(node, "name") else {
                    // L871
                    return true;
                };
                let name = Name::from(name);
                let set = configSetTable(configSetId);
                // L873
                if let Some(number) = attr(node, "number") {
                    // L874
                    set.table(n!("placeholder"))
                        .expect("NewConfigSet")
                        .set(name, pob_lua::tonumber(number));
                } else if let Some(string) = attr(node, "string") {
                    // L876 (sic: a string placeholder is stored in `input`)
                    set.table(n!("input"))
                        .expect("NewConfigSet")
                        .set(name, string);
                } else {
                    // L879
                    return true;
                }
            }
            false
        };

        let block = |node: &XmlNode| -> Table {
            let b = Table::new();
            b.set(n!("title"), attr(node, "title").unwrap_or("Default"));
            b.set(
                n!("enabled"),
                attr(node, "enabled") == Some("true") || attr(node, "enabled").is_none(),
            );
            b.set(n!("text"), first_text(node));
            b
        };

        // Catch special case of empty Config
        // L885 (`xml.empty`: set by ParseXML on an element without children)
        if xml.children.is_empty() {
            // L886
            self.NewConfigSet(Some(1.0), Some("Default"));
        }
        // L888
        for (i, node) in xml.children.iter().enumerate() {
            let index = (i + 1) as i64;
            let node = match node {
                XmlChild::Node(n) => Some(n),
                XmlChild::Text(_) => None,
            };
            // L889
            if node.map_or(true, |n| &*n.elem != "ConfigSet") {
                // L890
                if self.configSet(1.0).is_none() {
                    // L891
                    self.NewConfigSet(Some(1.0), Some("Default"));
                }
                // L893
                if let Some(node) = node.filter(|n| &*n.elem == "CustomModifierBlock") {
                    // L894
                    let b = block(node);
                    // L899
                    configSetTable(1.0)
                        .table(n!("customModsList"))
                        .unwrap_or_else(|| {
                            lua_error!("bad argument #1 to 'insert' (table expected, got nil)")
                        })
                        .push(b);
                } else {
                    // L901
                    setInputAndPlaceholder(node, 1.0);
                }
            } else {
                let node = node.expect("ConfigSet element");
                // L904
                let configSetId = attr(node, "id").and_then(pob_lua::tonumber);
                // L905
                let configSet =
                    self.NewConfigSet(configSetId, Some(attr(node, "title").unwrap_or("Default")));
                let _ = configSet;
                // L906
                configSetOrderList.set(index, configSetId);
                // L907
                let configSetId = configSetId
                    .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
                configSetTable(configSetId).set(n!("customModsList"), Table::new());
                // L908
                for child in &node.children {
                    let child = match child {
                        XmlChild::Node(c) => Some(c),
                        XmlChild::Text(_) => None,
                    };
                    // L909
                    if let Some(child) = child.filter(|c| &*c.elem == "CustomModifierBlock") {
                        // L910
                        let b = block(child);
                        // L915
                        configSetTable(configSetId)
                            .table(n!("customModsList"))
                            .expect("set above")
                            .push(b);
                    } else {
                        // L917
                        setInputAndPlaceholder(child, configSetId);
                    }
                }
            }
        }

        // `self.configSetOrderList` as `ipairs` sees it (a hole at L906 ends it: LUA-QUIRK)
        self.configSetOrderList.wipe();
        for (_, v) in configSetOrderList.ipairs() {
            let id = v.num();
            self.configSetOrderList.push(
                set_key(id)
                    .unwrap_or_else(|| lua_error!("CCR-0019: non-integer config set id {id}")),
            );
        }

        // Migration check for legacy builds
        // L924
        for_ipairs!((_i, configSetId) in self.configSetOrderList => {
            // L925
            let configSet = self
                .configSet(f64::from(configSetId))
                .unwrap_or_else(|| lua_error!("attempt to index local 'configSet' (a nil value)"));
            // L926
            let input = configSet.table(n!("input"));
            let legacyText = match &input {
                Some(i) => lor(i.get(n!("customMods")), || Val::from("")),
                None => Val::from(""),
            };
            let cml = configSet.table(n!("customModsList"));
            let cmlEmpty = cml.as_ref().map_or(true, |l| l.len() == 0);
            // L927
            let isEmptyText = || -> bool {
                let l = cml.as_ref().expect("checked");
                l.len() == 1 && lor(l.get(1i64).get(n!("text")), || Val::from("")).raw_eq(&Val::from(""))
            };
            if !legacyText.raw_eq(&Val::from("")) && (cmlEmpty || isEmptyText()) {
                // L928
                let b = Table::new();
                b.set(n!("title"), "Default");
                b.set(n!("enabled"), true);
                b.set(n!("text"), legacyText.clone());
                let l = Table::new();
                l.push(b);
                configSet.set(n!("customModsList"), l);
            } else if cmlEmpty {
                // L930
                let b = Table::new();
                b.set(n!("title"), "Default");
                b.set(n!("enabled"), true);
                b.set(n!("text"), "");
                let l = Table::new();
                l.push(b);
                configSet.set(n!("customModsList"), l);
            }
            // L932
            if let Some(input) = input {
                // L933
                input.set(n!("customMods"), Val::Nil);
            }
        });

        // L937
        self.SetActiveConfigSet(
            build,
            Some(
                attr(xml, "activeConfigSet")
                    .and_then(pob_lua::tonumber)
                    .unwrap_or(1.0),
            ),
            false,
        );
        // L938 UI: self:ResetUndo()
    }
}

/// @pob Classes/ConfigTab.lua:941-959 h=e4aec419
impl ConfigTab {
    /// `varType` is Lua's `type(v)` string; a nil `varType` (UpdateControls) is `"nil"`.
    pub fn GetDefaultState(&self, var: Name, varType: Name) -> Val {
        // L942
        let placeholder = self
            .activeConfigSet()
            .table(n!("placeholder"))
            .unwrap_or_else(|| lua_error!("attempt to index field 'placeholder' (a nil value)"));
        if !placeholder.get(var).is_nil() {
            // L943
            return placeholder.get(var);
        }

        // L946
        let ds = defaultState(pob_gamedata::data()).get(var);
        if !ds.is_nil() {
            // L947
            return ds;
        }

        // L950
        match varType.as_str() {
            // L951
            "number" => Val::Num(0.0),
            // L953
            "boolean" => Val::Bool(false),
            // L955
            "string" => Val::from(""),
            // L957
            _ => Val::Nil,
        }
    }
}

/// @pob Classes/ConfigTab.lua:961-1006 h=045df3cc
impl ConfigTab {
    pub fn Save(&self, xml: &mut crate::model::XmlNode) {
        let new_node = |elem: &str| XmlNode {
            elem: LStr::from(elem),
            attrib: indexmap::IndexMap::new(),
            children: Vec::new(),
        };
        // constructor state (ConfigTab.lua:145-148, CCR-0019 §10)
        self.ctor_model();
        // L962
        xml.attrib = indexmap::IndexMap::new();
        xml.attrib.insert(
            LStr::from("activeConfigSet"),
            LStr::from(tostring_num(self.activeConfigSetId.get())),
        );
        // L965
        for_ipairs!((_i, configSetId) in self.configSetOrderList => {
            // L966
            let configSet = self
                .configSet(f64::from(configSetId))
                .unwrap_or_else(|| lua_error!("attempt to index local 'configSet' (a nil value)"));
            // L967
            let mut child = new_node("ConfigSet");
            child
                .attrib
                .insert(LStr::from("id"), LStr::from(tostring_num(f64::from(configSetId))));
            if let Some(title) = configSet.get(n!("title")).str() {
                child.attrib.insert(LStr::from("title"), title);
            }
            // L968 t_insert(xml, child): pushed below, after its children (same tree)

            // L970 LUA-ORDER: pairs over string keys (random in PoB)
            let input = configSet
                .table(n!("input"))
                .unwrap_or_else(|| lua_error!("bad argument #1 to 'pairs' (table expected, got nil)"));
            for (k, v) in input.pairs() {
                let Key::S(k) = k else { lua_error!("invalid xml tree (attribute name is not a string)") };
                // L971
                if !v.raw_eq(&self.GetDefaultState(k, Name::from(v.lua_type()))) {
                    // L972
                    let mut node = new_node("Input");
                    node.attrib.insert(LStr::from("name"), LStr::from(k.as_str()));
                    // L973
                    let (key, s) = match &v {
                        // L974
                        Val::Num(n) => ("number", tostring_num(*n)),
                        // L976
                        Val::Bool(b) => ("boolean", tostring_val(&Val::Bool(*b))),
                        // L978
                        other => ("string", tostring_val(other)),
                    };
                    node.attrib.insert(LStr::from(key), LStr::from(s));
                    // L980
                    child.children.push(XmlChild::Node(node));
                }
            }
            // L983 LUA-ORDER
            let placeholder = configSet
                .table(n!("placeholder"))
                .unwrap_or_else(|| lua_error!("bad argument #1 to 'pairs' (table expected, got nil)"));
            for (k, v) in placeholder.pairs() {
                let Key::S(k) = k else { lua_error!("invalid xml tree (attribute name is not a string)") };
                // L984
                let mut node = new_node("Placeholder");
                node.attrib.insert(LStr::from("name"), LStr::from(k.as_str()));
                // L985
                if let Val::Num(n) = v {
                    // L986
                    node.attrib.insert(LStr::from("number"), LStr::from(tostring_num(n)));
                } else {
                    // L988
                    node.attrib.insert(LStr::from("string"), LStr::from(tostring_val(&v)));
                }
                // L990
                child.children.push(XmlChild::Node(node));
            }
            // L992
            if let Some(customModsList) = configSet.table(n!("customModsList")) {
                // L993
                for (_, block) in customModsList.ipairs() {
                    // L994
                    let mut blockNode = new_node("CustomModifierBlock");
                    let title = lor(block.get(n!("title")), || Val::from("Default"));
                    blockNode
                        .attrib
                        .insert(LStr::from("title"), LStr::from(tostring_val(&title)));
                    let enabled = !block.get(n!("enabled")).raw_eq(&Val::Bool(false));
                    blockNode
                        .attrib
                        .insert(LStr::from("enabled"), LStr::from(tostring_val(&Val::Bool(enabled))));
                    let text = lor(block.get(n!("text")), || Val::from(""));
                    blockNode
                        .children
                        .push(XmlChild::Text(LStr::from(tostring_val(&text))));
                    // L1002
                    child.children.push(XmlChild::Node(blockNode));
                }
            }
            xml.children.push(XmlChild::Node(child));
        });
    }
}

/// @pob Classes/ConfigTab.lua:1008-1022 h=8694db55
impl ConfigTab {
    /// Over the control-state tables of `self.varControls` (`_className`, `buf`,
    /// `placeholder`, `state`, `list`, `selIndex`).
    pub fn UpdateControls(&self, build: &BuildRef) {
        let _ = build;
        let set = self.activeConfigSet();
        let input = set.table(n!("input")).expect("config set input");
        let placeholder = set
            .table(n!("placeholder"))
            .expect("config set placeholder");
        // L1009 LUA-ORDER (each control is independent)
        for (var, control) in self.varControls.pairs() {
            let Some(control) = control.table() else {
                continue;
            };
            let className = control.get(n!("_className"));
            let cn = className.str();
            let cn = cn.as_deref();
            // L1010
            if cn == Some("EditControl") || cn == Some("ResizableEditControl") {
                // L1011 (EditControl:SetText: buf = tostring(text))
                let v = lor(input.get(var), || Val::from(""));
                control.set(n!("buf"), tostring_val(&v).as_str());
                // L1012
                if placeholder.get(var).truthy() {
                    // L1013
                    control.set(
                        n!("placeholder"),
                        tostring_val(&placeholder.get(var)).as_str(),
                    );
                }
            } else if cn == Some("CheckBoxControl") {
                // L1016
                control.set(n!("state"), input.get(var));
            } else if cn == Some("DropDownControl") {
                // L1018 (DropDownControl:SelByValue(value, "val"))
                let Key::S(varName) = var else { continue };
                let value = lor(input.get(var), || self.GetDefaultState(varName, n!("nil")));
                if let Some(list) = control.table(n!("list")) {
                    for (index, listVal) in list.ipairs() {
                        let hit = match &listVal {
                            Val::Table(t) => t.get(n!("val")).raw_eq(&value),
                            Val::Data(_) => listVal.get(n!("val")).raw_eq(&value),
                            other => other.raw_eq(&value),
                        };
                        if hit {
                            control.set(n!("selIndex"), index as f64);
                            break;
                        }
                    }
                }
            }
        }
        // L1021
        self.UpdateCustomModsControls();
    }
}

/// @pob Classes/ConfigTab.lua:1302-1334 h=378e2cbb
impl ConfigTab {
    /// The model half of `UpdateCustomModsControls`. PoB's constructor always creates the
    /// "Custom Modifiers" section (ConfigOptions.lua:2379 → ConfigTab.lua:292-294), so the
    /// `customSection` guard (L1303) never returns; the block controls (L1315-1333) are UI
    /// (ui doc U42).
    pub fn UpdateCustomModsControls(&self) {
        // L1306
        let Some(configSet) = self.configSet(self.activeConfigSetId.get()) else {
            // L1308
            return;
        };
        // L1310
        let customModsList = match configSet.table(n!("customModsList")) {
            Some(l) => l,
            None => {
                // L1311
                let l = Table::new();
                configSet.set(n!("customModsList"), l.clone());
                l
            }
        };
        // L1313
        if customModsList.len() == 0 {
            // L1314
            let text = match configSet.table(n!("input")) {
                Some(input) => lor(input.get(n!("customMods")), || Val::from("")),
                None => Val::from(""),
            };
            let b = Table::new();
            b.set(n!("title"), "Default");
            b.set(n!("enabled"), true);
            b.set(n!("text"), text);
            customModsList.push(b);
        }
        // L1317-1333 UI: the CustomModBlockControl list
    }
}

// @pob-skip Classes/ConfigTab.lua:1024-1116 reason="Draw: UI layout and input handling (ui doc U42); no model state without input events"
impl ConfigTab {
    /// UI drawing (the UI port owns it, ui doc U42). With no input events it changes no
    /// model state, so the model has nothing to do here.
    pub fn Draw(&self, build: &BuildRef) {
        let _ = build;
    }
}

/// @pob Classes/ConfigTab.lua:1118-1128 h=c4716077
impl ConfigTab {
    pub fn UpdateLevel(&self, build: &BuildRef) {
        let set = self.activeConfigSet();
        // L1119
        let input = set.table(n!("input")).expect("config set input");
        // L1120
        let placeholder = set
            .table(n!("placeholder"))
            .expect("config set placeholder");
        let maxLevel = build.data.misc.MaxEnemyLevel;
        let positive = |v: &Val| -> bool {
            match v {
                Val::Nil | Val::Bool(false) => false,
                Val::Num(n) => *n > 0.0,
                other => lua_error!("attempt to compare number with {}", other.lua_type()),
            }
        };
        // L1121
        if positive(&input.get(n!("enemyLevel"))) {
            // L1122
            self.enemyLevel
                .set(Some(m_min(maxLevel, input.num(n!("enemyLevel")))));
        } else if positive(&placeholder.get(n!("enemyLevel"))) {
            // L1124
            self.enemyLevel
                .set(Some(m_min(maxLevel, placeholder.num(n!("enemyLevel")))));
        } else {
            // L1126
            self.enemyLevel
                .set(Some(m_min(maxLevel, build.characterLevel.get())));
        }
    }
}

// Creates a new config set
/// @pob Classes/ConfigTab.lua:1280-1300 h=8f22b4ab
impl ConfigTab {
    pub fn NewConfigSet(&self, configSetId: Option<f64>, title: Option<&str>) -> Table {
        // L1281
        let configSet = Table::new();
        configSet.set(n!("id"), Val::from(configSetId));
        configSet.set(n!("title"), title.map_or(Val::Nil, Val::from));
        configSet.set(n!("input"), Table::new());
        configSet.set(n!("placeholder"), Table::new());
        let b = Table::new();
        b.set(n!("title"), "Default");
        b.set(n!("enabled"), true);
        b.set(n!("text"), "");
        let l = Table::new();
        l.push(b);
        configSet.set(n!("customModsList"), l);
        // L1282
        if configSetId.is_none() {
            // L1283
            configSet.set(n!("id"), 1.0);
            // L1284
            while self.configSet(configSet.num(n!("id"))).is_some() {
                // L1285
                let id = configSet.num(n!("id"));
                configSet.set(n!("id"), id + 1.0);
            }
        }
        // there are default values for input and placeholder that every new config set needs to have
        let input = configSet.table(n!("input")).expect("set above");
        let placeholder = configSet.table(n!("placeholder")).expect("set above");
        // L1289
        for varData in &pob_gamedata::data().configOptions {
            // L1290
            if let Some(var) = varData.var {
                // L1291
                input.set(
                    var,
                    varData
                        .defaultState
                        .as_ref()
                        .map_or(Val::Nil, Val::from_value),
                );
                // L1292
                placeholder.set(
                    var,
                    varData
                        .defaultPlaceholderState
                        .as_ref()
                        .map_or(Val::Nil, Val::from_value),
                );
                // L1293
                if let Some(defaultIndex) = varData.defaultIndex {
                    // L1294
                    input.set(var, list_val(varData, defaultIndex));
                }
            }
        }
        // L1298
        let id = configSet.num(n!("id"));
        let key =
            set_key(id).unwrap_or_else(|| lua_error!("CCR-0019: non-integer config set id {id}"));
        self.configSets.with_mut(|m| {
            m.insert(key, configSet.clone());
        });
        // L1299
        configSet
    }
}

// Changes the active config set
/// @pob Classes/ConfigTab.lua:1337-1362 h=c24a18fd
impl ConfigTab {
    /// The model half of `SetActiveConfigSet(configSetId, init)`.
    pub fn SetActiveConfigSet(&self, build: &BuildRef, configSetId: Option<f64>, init: bool) {
        // Initialize config sets if needed
        // L1339
        if self.configSetOrderList.get(1).is_none() {
            // L1340
            self.configSetOrderList.set(1, 1);
            // L1341
            self.NewConfigSet(Some(1.0), None);
        }

        // L1344
        let mut configSetId = configSetId.unwrap_or_else(|| self.activeConfigSetId.get());

        // L1348
        if self.configSet(configSetId).is_none() {
            // L1349
            configSetId = self
                .configSetOrderList
                .get(1)
                .map_or_else(|| lua_error!("attempt to index a nil value"), f64::from);
        }

        // L1352-1353 (CCR-0019: self.input / self.placeholder become the set's tables)
        let configSet = self
            .configSet(configSetId)
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
        self.bind_active_set(&configSet);
        // L1354
        self.activeConfigSetId.set(configSetId);

        // L1356
        if !init {
            // L1357
            self.UpdateControls(build);
            // L1358
            self.BuildModList(build);
        }
        // L1360
        build.buildFlag.set(true);
        // L1361 UI: self.build:SyncLoadouts()
    }
}
