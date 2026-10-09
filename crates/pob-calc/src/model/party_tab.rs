//! @pob-file Classes/PartyTab.lua @16de4b82d
//! PartyTab model (Classes/PartyTab.lua 516-669, 718-1042).
//!
//! Owner: BM-B. Load/Save of the `<Party>` section, `ParseBuffs` (party
//! member stats, auras, curses, warcries, links, enemy conditions and mods)
//! and the buff export (`setBuffExports`, `exportBuffs`).
//!
//! Model representation (CCR-0019):
//! * `self.actor` is the typed [`Actor`]; its `Aura`/`Curse`/`Warcry`/`Link`
//!   tables are `actor.extra.Aura` …, created by [`PartyTab::ctor_model`]
//!   (the constructor's model half, PartyTab.lua:22-36).
//! * A buff entry is a table `{ modList = <ModList>, effectMult = …, isMark = … }`
//!   whose `modList` is a `Val::Obj(Rc<ModStore>)` ([`buff_modList`] reads it,
//!   or an array table of `Val::Mod` as some calc code may build).
//! * The edit/label/dropdown controls Load and Save touch are tables in
//!   `self.extra.controls` (`{ buf = … }`, `{ label = … }`, `{ list, selIndex }`,
//!   `{ state = … }`); `self.lastContent` is `self.extra.lastContent`.
//! * `ParseBuffs(list, …, label)` takes a [`BuffList`] (a mod store or a plain
//!   table) and an optional label table: the frozen signature
//!   (`list: &Table, label: Option<&str>`) cannot express the Lua (CCR-0019 §3).
#![allow(non_snake_case, clippy::all, clippy::pedantic)]

use pob_gamedata::mod_tools;

use crate::model::{XmlChild, XmlNode};
use crate::prelude::*;

// ---- Rust-only helpers -------------------------------------------------------------------

/// `list` of `ParseBuffs` (CCR-0019 §3).
#[derive(Clone, Copy)]
pub enum BuffList<'a> {
    /// `self.enemyModList`, `self.actor.modDB`.
    Store(&'a ModStoreRef),
    /// `self.actor.Aura` / `Curse` / `Warcry` / `Link`.
    Table(&'a Table),
}

impl BuffList<'_> {
    fn store(&self) -> &ModStoreRef {
        match self {
            BuffList::Store(s) => s,
            BuffList::Table(_) => lua_error!("attempt to call method 'NewMod' (a nil value)"),
        }
    }
    fn table(&self) -> &Table {
        match self {
            BuffList::Table(t) => t,
            BuffList::Store(_) => lua_error!("CCR-0019: ParseBuffs buff list is a mod store"),
        }
    }
}

/// `buff.modList` as a list of mods: a ModList object (`Val::Obj(Rc<ModStore>)`), or an
/// array table of mods.
pub fn buff_modList(buff: &Val) -> Vec<ModRef> {
    let v = buff.get(n!("modList"));
    if let Some(store) = v.obj::<ModStore>() {
        return store.ipairs();
    }
    match v {
        Val::Table(t) => t
            .ipairs()
            .into_iter()
            .map(|(_, m)| match m {
                Val::Mod(m) => m,
                _ => lua_error!("CCR-0019: a modList entry that is not a mod"),
            })
            .collect(),
        Val::Nil => lua_error!("attempt to get length of field 'modList' (a nil value)"),
        _ => lua_error!("CCR-0019: unexpected modList value"),
    }
}

/// A new buff entry `{ modList = new("ModList"):ModList(), effectMult = effectMult }`.
fn new_buff_entry(effectMult: &Val) -> Table {
    let t = Table::new();
    t.set(
        n!("modList"),
        Val::Obj(ModStore::ModList(None) as Rc<dyn std::any::Any>),
    );
    t.set(n!("effectMult"), effectMult.clone());
    t
}

/// The ModList of a buff entry created by [`new_buff_entry`].
fn entry_modList(entry: &Table) -> ModStoreRef {
    entry
        .obj::<ModStore>(n!("modList"))
        .unwrap_or_else(|| lua_error!("attempt to index field 'modList' (a nil value)"))
}

/// Lua `a .. b` operand: strings and numbers.
fn cat_val(v: &Val) -> String {
    match v {
        Val::Str(s) => s.to_string(),
        Val::Num(n) => tostring_num(*n),
        other => lua_error!("attempt to concatenate a {} value", other.lua_type()),
    }
}

/// Lua `a .. b` operand of a table key.
fn cat_key(k: &Key) -> String {
    match k {
        Key::S(s) => s.to_string(),
        Key::I(i) => tostring_num(*i as f64),
        Key::B(_) => lua_error!("attempt to concatenate a boolean value"),
    }
}

/// Lua `tostring(v)` of a data value.
fn tostring_value(v: &Value) -> String {
    match v {
        Value::Nil => "nil".to_owned(),
        Value::Bool(b) => if *b { "true" } else { "false" }.to_owned(),
        Value::Num(n) => tostring_num(*n),
        Value::Str(s) => s.to_string(),
        _ => "table".to_owned(),
    }
}

/// Lua `a < b` (numbers, or strings by byte order: `strcoll` in the C locale).
fn lua_lt(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Num(x), Val::Num(y)) => x < y,
        (Val::Str(x), Val::Str(y)) => x.as_bytes() < y.as_bytes(),
        (x, y) => lua_error!("attempt to compare {} with {}", x.lua_type(), y.lua_type()),
    }
}

/// `x * 100` in Lua arithmetic (strings coerce).
fn mul100(v: &Val) -> f64 {
    v.num() * 100.0
}

/// `line:gmatch("([^\n]*)\n?")`.
fn lines(buf: &str) -> Vec<String> {
    pob_lua::pattern::gmatch(buf, "([^\n]*)\n?")
        .map(|c| c[0].to_str().into_owned())
        .collect()
}

/// `table.sort(list)` of strings, then `table.concat(list)`.
fn sort_concat(mut labelList: Vec<String>) -> String {
    pob_lua::sort::sort(&mut labelList, |a, b| a.as_bytes() < b.as_bytes());
    labelList.concat()
}

const partyDestinations: [&str; 8] = [
    "All",
    "Party Member Stats",
    "Aura",
    "Curse",
    "Warcry Skills",
    "Link Skills",
    "EnemyConditions",
    "EnemyMods",
];

/// The edit controls whose `buf` Load sets and Save writes.
const EDIT_CONTROLS: [&str; 7] = [
    "editPartyMemberStats",
    "editAuras",
    "editCurses",
    "editWarcries",
    "editLinks",
    "enemyCond",
    "enemyMods",
];

/// @pob Classes/PartyTab.lua:22-36 h=800476ea
impl PartyTab {
    /// The constructor's model half, run once (idempotent): `self.actor`'s buff tables and
    /// `modDB.actor`, `self.buffExports = { }`, `self.lastContent`, and the initial state of
    /// the controls Load/Save use (PartyTab.lua:300-508). `PartyTab::default()` (U0) holds
    /// the actor, `enemyModList` and `enableExportBuffs` already (CCR-0019).
    pub fn ctor_model(&self) {
        if self.extra.truthy(n!("controls")) {
            return;
        }
        // L22
        for k in [n!("Aura"), n!("Curse"), n!("Warcry"), n!("Link")] {
            if !self.actor.extra.truthy(k) {
                self.actor.extra.set(k, Table::new());
            }
        }
        // L23
        self.actor.modDB.get().actor.set(Rc::downgrade(&self.actor));
        // L25
        if self.buffExports.get().is_none() {
            self.buffExports.set(Some(Table::new()));
        }
        // L28
        let lastContent = Table::new();
        for k in ["Aura", "Curse", "Warcry", "Link", "EnemyCond", "EnemyMods"] {
            lastContent.set(Name::from(k), "");
        }
        lastContent.set(n!("EnableExportBuffs"), false);
        lastContent.set(n!("showAdvancedTools"), false);
        self.extra.set(n!("lastContent"), lastContent);
        // controls (PartyTab.lua:300-508)
        let controls = Table::new();
        for k in EDIT_CONTROLS {
            let c = Table::new();
            c.set(n!("buf"), "");
            controls.set(Name::from(k), c);
        }
        for (k, label) in [
            ("simpleAuras", ""),
            ("simpleWarcries", ""),
            ("simpleLinks", ""),
            ("simpleEnemyCond", "^7---------------------------\n"),
            ("simpleEnemyMods", "\n"),
            ("simpleCurses", ""),
        ] {
            let c = Table::new();
            c.set(n!("label"), label);
            controls.set(Name::from(k), c);
        }
        let dest = Table::new();
        let list = Table::new();
        for d in partyDestinations {
            list.push(d);
        }
        dest.set(n!("list"), list);
        dest.set(n!("selIndex"), 1.0);
        controls.set(n!("importCodeDestination"), dest);
        for k in ["appendNotReplace", "ShowAdvanceTools"] {
            let c = Table::new();
            c.set(n!("state"), false);
            controls.set(Name::from(k), c);
        }
        self.extra.set(n!("controls"), controls);
    }

    /// `self.controls[name]` (see [`PartyTab::ctor_model`]).
    pub fn control(&self, name: &str) -> Table {
        self.ctor_model();
        self.extra
            .table(n!("controls"))
            .and_then(|c| c.table(Name::from(name)))
            .unwrap_or_else(|| lua_error!("attempt to index field '{name}' (a nil value)"))
    }

    /// `self.lastContent`.
    pub fn lastContent(&self) -> Table {
        self.ctor_model();
        self.extra.table(n!("lastContent")).expect("ctor_model")
    }

    /// `self.actor[k]` for the buff tables (`Aura`, `Curse`, `Warcry`, `Link`).
    pub fn actorList(&self, k: Name) -> Table {
        self.ctor_model();
        self.actor
            .extra
            .table(k)
            .unwrap_or_else(|| lua_error!("attempt to index field '{k}' (a nil value)"))
    }
}

/// `node[1] or ""` of an `<ImportedBuffs>` node.
fn node_text(node: &XmlNode) -> String {
    match node.children.first() {
        None => String::new(),
        Some(XmlChild::Text(s)) => s.to_string(),
        // `SetText(table)` stores "table: 0x…"; ParseBuffs then calls `buf:gmatch` on a table
        Some(XmlChild::Node(_)) => lua_error!("attempt to call method 'gmatch' (a nil value)"),
    }
}

/// @pob Classes/PartyTab.lua:516-573 h=df6a5f6f
impl PartyTab {
    pub fn Load(&self, build: &BuildRef, xml: &crate::model::XmlNode, fileName: &str) {
        let _ = fileName;
        self.ctor_model();
        let setText = |c: &str, s: &str| self.control(c).set(n!("buf"), s);
        // L517
        for node in &xml.children {
            let XmlChild::Node(node) = node else { continue };
            // L518
            if &*node.elem == "ImportedBuffs" {
                let name = node.attrib.get("name").map(|s| &**s);
                // L519
                match name {
                    None => {
                        // L520 ConPrintf("missing name") (console only)
                    }
                    Some("PartyMemberStats") => {
                        // L522
                        let text = node_text(node);
                        setText("editPartyMemberStats", &text);
                        // L523
                        let modDB = self.actor.modDB.get();
                        let output = self.actor.output.get();
                        self.ParseBuffs(
                            build,
                            BuffList::Store(&modDB),
                            &text,
                            n!("PartyMemberStats"),
                            Some(&output),
                        );
                    }
                    Some("Aura") => {
                        // L525
                        let text = node_text(node);
                        setText("editAuras", &text);
                        // L526
                        self.ParseBuffs(
                            build,
                            BuffList::Table(&self.actorList(n!("Aura"))),
                            &text,
                            n!("Aura"),
                            Some(&self.control("simpleAuras")),
                        );
                    }
                    Some("Curse") => {
                        // L528
                        let text = node_text(node);
                        setText("editCurses", &text);
                        // L529
                        self.ParseBuffs(
                            build,
                            BuffList::Table(&self.actorList(n!("Curse"))),
                            &text,
                            n!("Curse"),
                            Some(&self.control("simpleCurses")),
                        );
                    }
                    Some("Warcry Skills") => {
                        // L531
                        let text = node_text(node);
                        setText("editWarcries", &text);
                        // L532
                        self.ParseBuffs(
                            build,
                            BuffList::Table(&self.actorList(n!("Warcry"))),
                            &text,
                            n!("Warcry"),
                            Some(&self.control("simpleWarcries")),
                        );
                    }
                    Some("Link Skills") => {
                        // L534
                        let text = node_text(node);
                        setText("editLinks", &text);
                        // L535
                        self.ParseBuffs(
                            build,
                            BuffList::Table(&self.actorList(n!("Link"))),
                            &text,
                            n!("Link"),
                            Some(&self.control("simpleLinks")),
                        );
                    }
                    Some("EnemyConditions") => {
                        // L537
                        let text = node_text(node);
                        setText("enemyCond", &text);
                        // L538
                        let l = self.enemyModList.get();
                        self.ParseBuffs(
                            build,
                            BuffList::Store(&l),
                            &text,
                            n!("EnemyConditions"),
                            None,
                        );
                    }
                    Some("EnemyMods") => {
                        // L540
                        let text = node_text(node);
                        setText("enemyMods", &text);
                        // L541
                        let l = self.enemyModList.get();
                        self.ParseBuffs(
                            build,
                            BuffList::Store(&l),
                            &text,
                            n!("EnemyMods"),
                            Some(&self.control("simpleEnemyMods")),
                        );
                    }
                    Some(_) => {}
                }
            } else if &*node.elem == "ExportedBuffs" {
                // L544
                if !node.attrib.contains_key("name") {
                    // L545 ConPrintf("missing name") (console only)
                }
                //if node.attrib.name ~= "EnemyConditions" and node.attrib.name ~= "EnemyMods" then
                //---	self:ParseBuffs(self.buffExports, node[1] or "", node.attrib.name)
                //end
                //self:ParseBuffs(self.buffExports, node[1] or "", "PartyMemberStats")
                //self:ParseBuffs(self.buffExports, node[1] or "", "Aura")
                //self:ParseBuffs(self.buffExports, node[1] or "", "Curse")
                //self:ParseBuffs(self.buffExports, node[1] or "", "Warcry")
                //self:ParseBuffs(self.buffExports, node[1] or "", "Link")
                //self:ParseBuffs(self.buffExports, node[1] or "", "EnemyConditions")
                //self:ParseBuffs(self.buffExports, node[1] or "", "EnemyMods")
            }
        }
        let lastContent = self.lastContent();
        let buf = |c: &str| self.control(c).get(n!("buf"));
        // L560
        lastContent.set(n!("PartyMemberStats"), buf("editPartyMemberStats"));
        // L561
        lastContent.set(n!("Aura"), buf("editAuras"));
        // L562
        lastContent.set(n!("Curse"), buf("editCurses"));
        // L563
        lastContent.set(n!("Warcry"), buf("editWarcries"));
        // L564
        lastContent.set(n!("Link"), buf("editLinks"));
        // L565
        lastContent.set(n!("EnemyCond"), buf("enemyCond"));
        // L566
        lastContent.set(n!("EnemyMods"), buf("enemyMods"));
        // L567
        lastContent.set(n!("EnableExportBuffs"), self.enableExportBuffs.get());

        // L569 (DropDownControl:SelByValue without a key: the list holds strings)
        let dest = self.control("importCodeDestination");
        let want = node_attr(xml, "destination").unwrap_or("All");
        for (i, d) in partyDestinations.iter().enumerate() {
            if *d == want {
                dest.set(n!("selIndex"), (i + 1) as f64);
                break;
            }
        }
        // L570
        self.control("appendNotReplace")
            .set(n!("state"), node_attr(xml, "append") == Some("true"));
        // L571
        self.control("ShowAdvanceTools").set(
            n!("state"),
            node_attr(xml, "ShowAdvanceTools") == Some("true"),
        );

        // L573
        lastContent.set(
            n!("showAdvancedTools"),
            self.control("ShowAdvanceTools").get(n!("state")),
        );
    }
}

fn node_attr<'a>(node: &'a XmlNode, k: &str) -> Option<&'a str> {
    node.attrib.get(k).map(|v| &**v)
}

fn buffs_node(elem: &str, name: &str, text: String) -> XmlNode {
    let mut attrib = indexmap::IndexMap::new();
    attrib.insert(LStr::from("name"), LStr::from(name));
    XmlNode {
        elem: LStr::from(elem),
        attrib,
        children: vec![XmlChild::Text(LStr::from(text))],
    }
}

/// @pob Classes/PartyTab.lua:575-668 h=9b8d985e
impl PartyTab {
    pub fn Save(&self, xml: &mut crate::model::XmlNode) {
        self.ctor_model();
        // `self.controls.X.buf and self.controls.X.buf ~= ""`
        let buf = |c: &str| -> Option<String> {
            match self.control(c).get(n!("buf")) {
                Val::Nil | Val::Bool(false) => None,
                v => {
                    let s = crate::model::skills_tab::tostring_val(&v);
                    (!s.is_empty()).then_some(s)
                }
            }
        };
        // L576 local child
        // L577
        for (control, name) in [
            ("editPartyMemberStats", "PartyMemberStats"), // L577-581
            ("editAuras", "Aura"),                        // L582-586
            ("editCurses", "Curse"),                      // L587-591
            ("editWarcries", "Warcry Skills"),            // L592-596
            ("editLinks", "Link Skills"),                 // L597-601
            ("enemyCond", "EnemyConditions"),             // L602-606
            ("enemyMods", "EnemyMods"),                   // L607-611
        ] {
            if let Some(b) = buf(control) {
                xml.children
                    .push(XmlChild::Node(buffs_node("ImportedBuffs", name, b)));
            }
        }
        // L612-646
        for (buffType, name) in [
            (n!("PlayerMods"), "PartyMemberStats"),
            (n!("Aura"), "Aura"),
            (n!("Curse"), "Curse"),
            (n!("Warcry"), "Warcry Skills"),
            (n!("Link"), "Link Skills"),
            (n!("EnemyConditions"), "EnemyConditions"),
            (n!("EnemyMods"), "EnemyMods"),
        ] {
            let exportString = self.exportBuffs(buffType);
            if !exportString.is_empty() {
                xml.children.push(XmlChild::Node(buffs_node(
                    "ExportedBuffs",
                    name,
                    exportString,
                )));
            }
        }
        let lastContent = self.lastContent();
        let bufv = |c: &str| self.control(c).get(n!("buf"));
        // L654
        lastContent.set(n!("PartyMemberStats"), bufv("editPartyMemberStats"));
        // L655
        lastContent.set(n!("Aura"), bufv("editAuras"));
        // L656
        lastContent.set(n!("Curse"), bufv("editCurses"));
        // L657
        lastContent.set(n!("Warcry"), bufv("editWarcries"));
        // L658
        lastContent.set(n!("Link"), bufv("editLinks"));
        // L659
        lastContent.set(n!("EnemyCond"), bufv("enemyCond"));
        // L660
        lastContent.set(n!("EnemyMods"), bufv("enemyMods"));
        // L661
        lastContent.set(n!("EnableExportBuffs"), self.enableExportBuffs.get());
        // L662
        let dest = self.control("importCodeDestination");
        let list = dest.table(n!("list")).expect("ctor_model");
        let destination = list.get(dest.get(n!("selIndex")).num() as i64);
        xml.attrib = indexmap::IndexMap::new();
        if let Some(d) = destination.str() {
            xml.attrib.insert(LStr::from("destination"), d);
        }
        xml.attrib.insert(
            LStr::from("append"),
            LStr::from(crate::model::skills_tab::tostring_val(
                &self.control("appendNotReplace").get(n!("state")),
            )),
        );
        xml.attrib.insert(
            LStr::from("ShowAdvanceTools"),
            LStr::from(crate::model::skills_tab::tostring_val(
                &self.control("ShowAdvanceTools").get(n!("state")),
            )),
        );
        // L667
        lastContent.set(
            n!("showAdvancedTools"),
            self.control("ShowAdvanceTools").get(n!("state")),
        );
    }
}

/// @pob Classes/PartyTab.lua:718-982 h=9638cce9
impl PartyTab {
    /// `aura` at 919 is an undeclared global (CCR-0004): LUA-GLOBAL-NIL.
    ///
    /// CCR-0019 §3: `list` is a [`BuffList`] and `label` an optional table (a label
    /// control `{ label = … }`, or `self.actor.output` for `"PartyMemberStats"`).
    pub fn ParseBuffs(
        &self,
        build: &BuildRef,
        list: BuffList<'_>,
        buf: &str,
        buffType: Name,
        label: Option<&Table>,
    ) {
        let data = build.data;
        // L719
        if buffType.as_str() == "EnemyConditions" {
            // L720
            for line in lines(buf) {
                // L721
                if !line.is_empty() {
                    // L722
                    let name =
                        pob_lua::pattern::gsub(&line, "Condition:", "Condition:Party:", None).0;
                    list.store().NewMod(
                        Name::from(name.as_str()),
                        ModType::FLAG,
                        true,
                        "Party",
                        0,
                        0,
                        &[],
                    );
                }
            }
        } else if buffType.as_str() == "EnemyMods" {
            // L726 (keys in insertion order: LUA-ORDER for the label below)
            let mut enemyModList: indexmap::IndexMap<String, Vec<(Value, ModType)>> =
                indexmap::IndexMap::new();
            // L727
            let mut currentName: Option<String> = None;
            // L728
            for line in lines(buf) {
                // L729
                if pob_lua::pattern::find(&line, "|", None, false).is_none() {
                    // L730
                    currentName = Some(line.clone());
                    // L731
                    if label.is_some() && line != "" {
                        // L732
                        enemyModList.entry(line.clone()).or_default();
                    }
                } else {
                    // L735
                    let r#mod = mod_tools::parseFormattedSourceMod(&line);
                    // L736
                    if let Some(mut r#mod) = r#mod {
                        // L737
                        let src = r#mod
                            .source
                            .as_deref()
                            .unwrap_or_else(|| {
                                lua_error!("attempt to concatenate field 'source' (a nil value)")
                            })
                            .to_owned();
                        r#mod.source = Some(pob_gamedata::AStr::from(format!("Party{src}")));
                        let value = r#mod.value.clone();
                        let ty = r#mod.r#type;
                        // L738
                        list.store().AddMod(Arc::new(r#mod));
                        // L739
                        if label.is_some() {
                            // L740
                            match currentName.as_ref().and_then(|c| enemyModList.get_mut(c)) {
                                Some(l) => l.push((value, ty)),
                                None => lua_error!(
                                    "bad argument #1 to 'insert' (table expected, got nil)"
                                ),
                            }
                        }
                    }
                }
            }
            // L745
            if let Some(label) = label {
                // L746
                let mut count = 0;
                // L747
                let mut s = String::from("^7---------------------------");
                // L748 LUA-ORDER: pairs(enemyModList)
                for (modName, modList) in &enemyModList {
                    // L749
                    s.push_str(&format!("\n{modName}:"));
                    // L750
                    count += 1;
                    // L751
                    for (value, ty) in modList {
                        // L752 (`mod[1] and "True" or mod[1]`: a value is never nil/false here)
                        let v = if value.truthy() {
                            "True".to_owned()
                        } else {
                            lua_error!("attempt to concatenate a {} value", value.lua_type())
                        };
                        let t = if *ty == ModType::FLAG {
                            ""
                        } else {
                            ty.as_str()
                        };
                        s.push_str(&format!(" {v} {t}, "));
                    }
                }
                // L755
                if count > 0 {
                    // L756
                    s.push_str("\n---------------------------\n");
                } else {
                    // L758
                    s.push('\n');
                }
                label.set(n!("label"), s);
            }
        } else if buffType.as_str() == "PartyMemberStats" {
            // L762 (`if not list`: the store is always given)
            let label =
                label.unwrap_or_else(|| lua_error!("attempt to index local 'label' (a nil value)"));
            // L764
            for line in lines(buf) {
                // L765
                if pob_lua::pattern::find(&line, "=", None, false).is_some() {
                    // label is output for this type, as a special case
                    // L767
                    let m =
                        pob_lua::pattern::r#match(&line, "^([%w ]-%w+)%.([%w ]-%w+)=(.+)$", None);
                    // L768
                    if let Some(m) = m {
                        let (k1, k2, v) = (m[0].to_str(), m[1].to_str(), m[2].to_str());
                        let k1 = Name::from(&*k1);
                        // L769
                        if label.get(k1).table().is_none() {
                            // L770
                            label.set(k1, Table::new());
                        }
                        // L772
                        label
                            .table(k1)
                            .expect("set above")
                            .set(Name::from(&*k2), pob_lua::tonumber(&v));
                    } else if pob_lua::pattern::r#match(&line, "|", None).is_some() {
                        // L774
                        let m = pob_lua::pattern::r#match(&line, "([%w ]-%w+)|(.+)=(.+)", None)
                            .unwrap_or_else(|| lua_error!("table index is nil"));
                        let (k, tags, v) = (m[0].to_str(), m[1].to_str(), m[2].to_str());
                        let k = Name::from(&*k);
                        // L775
                        let mut v: Option<f64> = pob_lua::tonumber(&v);
                        // L776
                        for tag in pob_lua::pattern::gmatch(&tags, "([^|]*)|?") {
                            let tag = tag[0].to_str();
                            // L777
                            if tag == "percent" {
                                // L778
                                v = Some(Val::from(v).num() / 100.0);
                            } else if tag == "max" {
                                // L780
                                let cur = label.get(k).num_or(1.0);
                                let vv = v.unwrap_or_else(|| {
                                    lua_error!(
                                        "bad argument #1 to 'max' (number expected, got nil)"
                                    )
                                });
                                v = Some(m_max(vv, cur));
                            }
                        }
                        // L783
                        label.set(k, v);
                    } else {
                        // L785
                        let m = pob_lua::pattern::r#match(&line, "([%w ]-%w+)=(.+)", None)
                            .unwrap_or_else(|| lua_error!("table index is nil"));
                        // L786
                        label.set(
                            Name::from(&*m[0].to_str()),
                            pob_lua::tonumber(&m[1].to_str()),
                        );
                    }
                } else if !line.is_empty() {
                    // L789
                    list.store().NewMod(
                        Name::from(line.as_str()),
                        ModType::FLAG,
                        true,
                        "Party",
                        0,
                        0,
                        &[],
                    );
                }
            }
        } else {
            let list = list.table();
            // L794
            let mut mode = "Name";
            // L795
            if buffType.as_str() == "Curse" {
                // L796
                mode = "CurseLimit";
            }
            // L798
            let mut currentName: Option<String> = None;
            // L799
            let mut currentEffect: Val = Val::Nil;
            // L800
            let mut isMark = false;
            // L801
            let mut currentModType: Option<Name> = Some(if buffType.as_str() == "Link" {
                n!("Link")
            } else if buffType.as_str() == "Warcry" {
                n!("Warcry")
            } else {
                n!("Unknown")
            });
            // L802
            for line in lines(buf) {
                let mut line = line;
                // L803
                if line != "---" && pob_lua::pattern::r#match(&line, "%-%-%-", None).is_some() {
                    // comment but not divider, skip the line
                } else if mode == "CurseLimit" && !line.is_empty() {
                    // L806
                    list.set(n!("limit"), pob_lua::tonumber(&line));
                    // L807
                    mode = "Name";
                } else if mode == "Name" && !line.is_empty() {
                    // L809
                    currentName = Some(pob_lua::pattern::gsub(&line, "_Debuff", "", None).0);
                    // L810
                    currentEffect = Val::Num(0.0);
                    // L811
                    if line == "extraAura" || line == "otherEffects" {
                        // L812
                        currentModType = Some(Name::from(line.as_str()));
                        // L813
                        mode = "Stats";
                    } else {
                        // L815
                        mode = "Effect";
                        // L816
                        currentModType = Some(n!("Unknown"));
                    }
                } else if mode == "Effect" {
                    // L819
                    currentEffect = Val::from(pob_lua::tonumber(&line));
                    // L820
                    if buffType.as_str() == "Curse" {
                        // L821
                        mode = "isMark";
                    } else {
                        // L823
                        mode = "Stats";
                    }
                } else if mode == "isMark" {
                    // L826
                    isMark = line == "true";
                    // L827
                    mode = "Stats";
                } else if line == "---" {
                    // L829
                    mode = "Name";
                } else {
                    // L831
                    if pob_lua::pattern::find(&line, "|", None, false).is_some()
                        && currentName.as_deref() != Some("SKIP")
                        && pob_lua::pattern::find(&line, "MinionModifier|LIST", None, false)
                            .is_none()
                    {
                        // L832
                        if currentModType.is_some_and(|t| t.as_str() == "otherEffects") {
                            // L833
                            let caps =
                                pob_lua::pattern::r#match(&line, "([%w ']-%w+)|(%w+)|(.+)", None)
                                    .map(|m| {
                                        (
                                            m[0].to_str().into_owned(),
                                            m[1].to_str().into_owned(),
                                            m[2].to_str().into_owned(),
                                        )
                                    });
                            match caps {
                                Some((a, b, c)) => {
                                    currentName = Some(a);
                                    currentEffect = Val::from(b.as_str());
                                    line = c;
                                }
                                None => lua_error!(
                                    "bad argument #1 to 'gmatch' (string expected, got nil)"
                                ),
                            }
                        }
                        // L835
                        let r#mod = mod_tools::parseFormattedSourceMod(&line);
                        // L836
                        if let Some(mut r#mod) = r#mod {
                            // L837
                            for tag in r#mod.tags.iter() {
                                // L838
                                let cmt = currentModType.map(|t| t.as_str());
                                if tag.r#type == TagType::GlobalEffect
                                    && cmt != Some("Link")
                                    && cmt != Some("Warcry")
                                    && cmt != Some("otherEffects")
                                {
                                    // L839
                                    currentModType = tag.effectType;
                                }
                            }
                            let cmt =
                                currentModType.unwrap_or_else(|| lua_error!("table index is nil"));
                            // L842
                            let typeList = list.get_or_new_table(cmt);
                            // L843
                            let mut listElement = typeList.clone();
                            let cn = currentName.clone().unwrap_or_else(|| {
                                lua_error!("attempt to index local 'currentName' (a nil value)")
                            });
                            // L844
                            if pob_lua::string::sub(&cn, 1, Some(4)) == "Vaal" {
                                // L845
                                let vaal = typeList.get_or_new_table(n!("Vaal"));
                                // L846
                                listElement = vaal;
                            }
                            let cnk = Name::from(cn.as_str());
                            // L848
                            match listElement.get(cnk) {
                                Val::Nil | Val::Bool(false) => {
                                    // L849
                                    let e = new_buff_entry(&currentEffect);
                                    listElement.set(cnk, e.clone());
                                    // L853
                                    if isMark {
                                        // L854
                                        e.set(n!("isMark"), true);
                                    }
                                }
                                entry => {
                                    let em = entry.get(n!("effectMult"));
                                    // L856
                                    if !em.raw_eq(&currentEffect) {
                                        // L857
                                        if lua_lt(&em, &currentEffect) {
                                            // L858
                                            listElement.set(cnk, new_buff_entry(&currentEffect));
                                        } else {
                                            // L863
                                            currentName = Some("SKIP".to_owned());
                                        }
                                    }
                                }
                            }
                            // L866
                            if currentName.as_deref() != Some("SKIP") {
                                // L867
                                let src = |m: &Mod| -> String {
                                    m.source
                                        .as_deref()
                                        .unwrap_or_else(|| {
                                            lua_error!(
                                                "attempt to index field 'source' (a nil value)"
                                            )
                                        })
                                        .to_owned()
                                };
                                if pob_lua::pattern::r#match(&src(&r#mod), "Item", None).is_some() {
                                    // L868-869
                                    let s = src(&r#mod);
                                    let m = pob_lua::pattern::r#match(&s, "Item:(%d+):(.+)", None);
                                    let newSource = m.map(|m| m[1].to_str().into_owned());
                                    // L870
                                    let newSource = newSource.unwrap_or_else(|| {
                                        lua_error!(
                                            "attempt to concatenate field 'source' (a nil value)"
                                        )
                                    });
                                    r#mod.source = Some(pob_gamedata::AStr::from(format!(
                                        "Party - {newSource}"
                                    )));
                                }
                                // L872
                                if pob_lua::pattern::r#match(&src(&r#mod), "Skill", None).is_some()
                                {
                                    // L873
                                    let s = src(&r#mod);
                                    let skillId = pob_lua::pattern::r#match(&s, "Skill:(.+)", None)
                                        .map(|m| m[0].to_str().into_owned());
                                    // L874 (`data.skills[nil]` reads nil)
                                    let known = skillId.as_ref().is_some_and(|id| {
                                        data.skills.contains_key(&Name::from(id.as_str()))
                                    });
                                    if !known {
                                        // L875
                                        let up = pob_lua::pattern::gsub_fn(
                                            currentName.as_deref().expect("not SKIP"),
                                            " %l",
                                            |c| Some(pob_lua::string::upper(&c[0].to_str())),
                                            None,
                                        )
                                        .0;
                                        let minimisedName =
                                            pob_lua::pattern::gsub(&up, " ", "", None).0;
                                        // L876
                                        if data
                                            .skills
                                            .contains_key(&Name::from(minimisedName.as_str()))
                                        {
                                            // L877
                                            r#mod.source = Some(pob_gamedata::AStr::from(format!(
                                                "Skill:{minimisedName}"
                                            )));
                                        } else {
                                            // L879
                                            r#mod.source = skillId
                                                .map(|s| pob_gamedata::AStr::from(s.as_str()));
                                        }
                                    }
                                }
                                // L883
                                if buffType.as_str() == "Link" {
                                    // L884
                                    let nm = pob_lua::pattern::gsub(
                                        &r#mod.name,
                                        "Parent",
                                        "PartyMember",
                                        None,
                                    )
                                    .0;
                                    r#mod.name = Name::from(nm.as_str());
                                    // L885
                                    for modTag in r#mod.tags.iter_mut() {
                                        // L886
                                        if modTag.actor.is_some_and(|a| a.as_str() == "parent") {
                                            // L887
                                            modTag.actor = Some(n!("partyMembers"));
                                        }
                                    }
                                }
                                // L891
                                let entry = listElement
                                    .table(Name::from(currentName.as_deref().expect("not SKIP")))
                                    .expect("set above");
                                entry_modList(&entry).AddMod(Arc::new(r#mod));
                            }
                        }
                    }
                }
            }
            // L897
            if let Some(label) = label {
                let effectLine = |name: &Key, entry: &Val| -> String {
                    format!(
                        "{}: {}%\n",
                        cat_key(name),
                        cat_val(&entry.get(n!("effectMult")))
                    )
                };
                let modLine = |m: &ModRef| -> String {
                    let ty = if m.r#type == ModType::FLAG {
                        String::new()
                    } else {
                        format!("{} ", m.r#type.as_str())
                    };
                    format!("  {}{}: {}\n", ty, m.name, tostring_value(&m.value))
                };
                // L898
                if buffType.as_str() == "Aura" {
                    // L899
                    let mut labelList: Vec<String> = Vec::new();
                    // L900
                    let mut s = String::from("^7---------------------------\n");
                    let auraList = list.table(n!("Aura"));
                    // L901 LUA-ORDER (sorted below)
                    for (aura, auraMod) in auraList.clone().unwrap_or_default().pairs() {
                        // L902
                        if aura != Key::S(n!("Vaal")) {
                            // L903
                            labelList.push(effectLine(&aura, &auraMod));
                        }
                    }
                    // L906
                    if let Some(vaal) = auraList.as_ref().and_then(|a| a.table(n!("Vaal"))) {
                        // L907
                        for (aura, auraMod) in vaal.pairs() {
                            // L908
                            labelList.push(effectLine(&aura, &auraMod));
                        }
                    }
                    // L911
                    let auraDebuff = list.table(n!("AuraDebuff"));
                    for (aura, auraMod) in auraDebuff.clone().unwrap_or_default().pairs() {
                        // L912
                        if auraList.as_ref().map_or(true, |a| !a.get(aura).truthy()) {
                            // L913
                            if aura != Key::S(n!("Vaal")) {
                                // L914
                                labelList.push(effectLine(&aura, &auraMod));
                            }
                        }
                    }
                    // L918
                    if let Some(vaal) = auraDebuff.as_ref().and_then(|a| a.table(n!("Vaal"))) {
                        // L919: `list["Aura"]["Vaal"][aura]` — `aura` is the undeclared global
                        // LUA-GLOBAL-NIL: indexing with nil reads nil, so the test is true whenever
                        // `list["Aura"]["Vaal"]` can be indexed.
                        let _ = auraList.as_ref().and_then(|a| a.table(n!("Vaal")));
                        // L920
                        for (aura, auraMod) in vaal.pairs() {
                            // L921
                            labelList.push(effectLine(&aura, &auraMod));
                        }
                    }
                    // L925
                    if !labelList.is_empty() {
                        // L926-927
                        s.push_str(&sort_concat(labelList));
                        // L928
                        s.push_str("---------------------------\n");
                    }
                    // L930
                    if let Some(extra) = list
                        .table(n!("extraAura"))
                        .and_then(|e| e.table(n!("extraAura")))
                    {
                        // L931
                        s.push_str("extraAuras:\n");
                        // L932
                        for auraMod in buff_modList(&Val::Table(extra)) {
                            // L933
                            s.push_str(&modLine(&auraMod));
                        }
                        // L935
                        s.push_str("---------------------------\n");
                    }
                    // L937
                    if let Some(other) = list.table(n!("otherEffects")) {
                        // L938
                        s.push_str("otherEffects:\n");
                        // L939 LUA-ORDER
                        for (_buffName, buff) in other.pairs() {
                            // L940
                            for auraMod in buff_modList(&buff) {
                                // L941
                                s.push_str(&modLine(&auraMod));
                            }
                        }
                        // L944
                        s.push_str("---------------------------\n");
                    }
                    label.set(n!("label"), s);
                } else if ["Warcry", "Link", "Curse"].contains(&buffType.as_str()) {
                    // L946-978 (the three branches differ only in the list they read)
                    // L947
                    let mut labelList: Vec<String> = Vec::new();
                    // L948
                    for (k, m) in list.table(buffType).unwrap_or_default().pairs() {
                        // L949
                        labelList.push(effectLine(&k, &m));
                    }
                    // L951
                    if !labelList.is_empty() {
                        // L952-953
                        label.set(
                            n!("label"),
                            format!(
                                "^7---------------------------\n{}---------------------------\n",
                                sort_concat(labelList)
                            ),
                        );
                    } else {
                        // L955
                        label.set(n!("label"), "");
                    }
                }
            }
        }
    }
}

/// @pob Classes/PartyTab.lua:984-990 h=80f3f6ea
impl PartyTab {
    pub fn setBuffExports(&self, buffExports: &Table) {
        // L985
        if !self.enableExportBuffs.get() {
            // L986
            return;
        }
        // L988
        if let Some(old) = self.buffExports.get() {
            old.wipe();
        } else {
            lua_error!("bad argument #1 to 'pairs' (table expected, got nil)");
        }
        // L989
        self.buffExports.set(Some(buffExports.copyTable(true)));
    }
}

/// @pob Classes/PartyTab.lua:992-1043 h=3b532484
impl PartyTab {
    pub fn exportBuffs(&self, buffType: Name) -> String {
        // L993
        let Some(buffExports) = self.buffExports.get() else {
            return String::new();
        };
        if !self.enableExportBuffs.get() || !buffExports.truthy(buffType) {
            // L994
            return String::new();
        }
        let exports = buffExports.get(buffType);
        // L996
        if exports.get(n!("ConvertedToText")).truthy() {
            // L997
            return match exports.get(n!("string")) {
                Val::Str(s) => s.to_string(),
                Val::Nil => lua_error!("CCR-0019: exportBuffs returned nil"),
                v => crate::model::skills_tab::tostring_val(&v),
            };
        }
        let exports = exports
            .table()
            .unwrap_or_else(|| lua_error!("bad argument #1 to 'pairs' (table expected)"));
        let bt = buffType.as_str();
        // L999
        let mut buf = if bt == "Curse" {
            format!(
                "--- Curse Limit ---\n{}",
                crate::model::skills_tab::tostring_val(&buffExports.get(n!("CurseLimit")))
            )
        } else {
            String::new()
        };
        // L1000 LUA-ORDER: pairs over string keys (random in PoB)
        for (buffName, buff) in exports.pairs() {
            let isName = |s: &str| buffName == Key::S(Name::from(s));
            // L1001
            if !isName("extraAura") || !buff_modList(&buff).is_empty() {
                // L1002
                if !buf.is_empty() {
                    // L1003
                    buf.push('\n');
                }
                // L1005
                buf.push_str(&cat_key(&buffName));
                // L1006
                if bt == "Curse" {
                    // L1007
                    buf.push_str(&format!(
                        "\n{}",
                        tostring_num(mul100(&buff.get(n!("effectMult"))))
                    ));
                    // L1008
                    if buff.get(n!("isMark")).truthy() {
                        // L1009
                        buf.push_str("\ntrue");
                    } else {
                        // L1011
                        buf.push_str("\nfalse");
                    }
                } else if bt == "Link"
                    || bt == "Warcry"
                    || (bt == "Aura" && !isName("extraAura") && !isName("otherEffects"))
                {
                    // L1014
                    buf.push_str(&format!(
                        "\n{}",
                        tostring_num(mul100(&buff.get(n!("effectMult"))))
                    ));
                }
                // L1016
                if bt == "Aura" && isName("otherEffects") {
                    // L1017 LUA-ORDER
                    let inner = buff
                        .table()
                        .unwrap_or_else(|| lua_error!("bad argument #1 to 'pairs'"));
                    for (innerBuffName, innerBuff) in inner.pairs() {
                        // L1018
                        for r#mod in buff_modList(&innerBuff) {
                            // L1019
                            buf.push_str(&format!(
                                "\n{}|{}|{}",
                                cat_key(&innerBuffName),
                                tostring_num(mul100(&innerBuff.get(n!("effectMult")))),
                                mod_tools::formatSourceMod(&r#mod)
                            ));
                        }
                    }
                    // L1022
                    buf.push_str("\n---");
                } else if bt == "Curse" || bt == "Aura" || bt == "Warcry" || bt == "Link" {
                    // L1024
                    for r#mod in buff_modList(&buff) {
                        // L1025
                        buf.push_str(&format!("\n{}", mod_tools::formatSourceMod(&r#mod)));
                    }
                    // L1027
                    buf.push_str("\n---");
                } else if bt == "EnemyMods" {
                    // L1029
                    if buff.get(n!("MultiStat")).truthy() {
                        // L1030
                        let t = buff
                            .table()
                            .unwrap_or_else(|| lua_error!("bad argument #1 to 'ipairs'"));
                        for (_, buffInner) in t.ipairs() {
                            // L1031
                            let Val::Mod(m) = buffInner else {
                                lua_error!("CCR-0019: an EnemyMods export entry that is not a mod")
                            };
                            buf.push_str(&format!("\n{}", mod_tools::formatSourceMod(&m)));
                        }
                    } else {
                        // L1034
                        let Val::Mod(m) = buff else {
                            lua_error!("CCR-0019: an EnemyMods export entry that is not a mod")
                        };
                        buf.push_str(&format!("\n{}", mod_tools::formatSourceMod(&m)));
                    }
                }
            }
        }
        // L1039
        exports.wipe();
        // L1040
        let converted = Table::new();
        converted.set(n!("ConvertedToText"), true);
        buffExports.set(buffType, converted.clone());
        // L1041
        converted.set(n!("string"), buf.as_str());
        // L1042
        buf
    }
}
