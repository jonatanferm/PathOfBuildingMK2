//! @pob-file Classes/PassiveTree.lua @16de4b82d
//! @pob-file Modules/Data.lua @16de4b82d
//! Classes/PassiveTree.lua 1-908, 1041-1062; `data.setJewelRadiiGlobally` (Modules/Data.lua 583-600).
//!
//! Owner: PT. The calc half of the PassiveTree class: loading a tree version, the node
//! migration and classification, the node modifier lists (`ProcessStats`), node positions
//! (`ProcessNode`, `CalcOrbitAngles`), the jewel-radius node lists and the legion / tattoo
//! node lists. The image, sprite-sheet and connector parts of the constructor (and
//! `LoadImage`/`BuildConnector`, 909-1040) are the UI's (ui doc §10.2, U23
//! `tree_view/assets.rs`); they carry `@pob-skip` markers here.
//!
//! Representation (CCR-0024, `docs/port/ccr/0024-pt-passive-tree.md`):
//! * (§4) Nodes inside dynamic tables (`keystoneMap`, `notableMap`, `ascendancyMap`,
//!   `clusterNodeMap`, `masteryEffects`, `legion.nodes`, `tattoo.nodes`) are
//!   `Val::Obj(NodeRef)` ([`node_val`]; read back with `v.obj::<Node>()`).
//! * (§3) `node.nodesInRadius[radiusIndex]` is `Val::Obj(Rc<LjIntMap<NodeRef>>)` (calc doc §3.4
//!   lists `nodesInRadius` as a LuaJIT-ordered node-id map); use [`nodesInRadiusAt`].
//!   `attributesInRadius[radiusIndex]` stays an (empty) `Table`.
//! * (§2) A Lua `nil` position (`node.x`/`node.y`/`node.angle`/`node.o`/`node.oidx` of a node
//!   without a group) is `f64::NAN` in the `Cell<f64>` fields ([`node_xy`]).
//! * (§1) Legion and tattoo nodes have string ids (`"vaal_keystone_1"`): `Node.id` is `NaN` and
//!   the string is `node.extra.id` ([`node_id_val`]).
//! * (§4) Raw tree keys the census did not type stay in `node.extra` / `tree.extra` under their
//!   Lua names (`skill`, `name`, `stats`, `orbit`, `g`, `out`, `in`, `isProxy`, `groups`,
//!   `constants`, `classNotables`, `nodeOverlay`, …).
#![allow(
    non_snake_case,
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::explicit_auto_deref,
    clippy::unnecessary_unwrap
)]

use std::any::Any;
use std::cell::RefCell;

use pob_gamedata::mod_tools as modLib;
use pob_gamedata::{DataTable, JewelRadius};
use pob_lua::string::lower;

use crate::lua_table::Key;
use crate::prelude::*;

// @pob Classes/PassiveTree.lua:8-34 h=b3938374
// (module-level statements: pairs,ipairs,t_insert,t_remove,m_min,m_max,m_pi,m_rad,m_sin,m_cos,m_tan,m_sqrt,classArt,legacySkills…)

/// `classArt` (L22-30; the constructor shadows it with an identical local at L437).
const classArt: [&str; 7] = [
    "centerscion",
    "centermarauder",
    "centerranger",
    "centerwitch",
    "centerduelist",
    "centertemplar",
    "centershadow",
];

// These values are from the 3.6 tree; older trees are missing values for these constants
const legacySkillsPerOrbit: [f64; 5] = [1.0, 6.0, 12.0, 12.0, 40.0];
const legacyOrbitRadii: [f64; 5] = [0.0, 82.0, 162.0, 335.0, 493.0];

/// `m_rad` (LuaJIT `math.rad`: `x * (pi / 180)`, the constant pushed by lib_math.c).
#[inline]
fn m_rad(x: f64) -> f64 {
    x * (std::f64::consts::PI / 180.0)
}

/// `m_sin` / `m_cos` (LuaJIT calls libm).
#[inline]
fn m_sin(x: f64) -> f64 {
    x.sin()
}
#[inline]
fn m_cos(x: f64) -> f64 {
    x.cos()
}

// @pob-skip Classes/PassiveTree.lua:39-50 reason="getFile: curl download of the tree page; PoB keeps it disabled (L36-38) and the port reads the dumped tree data (AMENDMENTS A2)"
/// Retrieve the file at the given URL. Disabled in PoB (it is only reachable through
/// `main.allowTreeDownload`, which nothing sets); the port has no network access here.
pub fn getFile(URL: &str) -> Option<Vec<u8>> {
    let _ = URL;
    None
}

// @pob Classes/PassiveTree.lua:73-73 h=1ec45aa6
// (module-level statements: PassiveTreeClass)

// ---------------------------------------------------------------------------------------
// Port plumbing (snake_case): node values, data conversion, `require` cache
// ---------------------------------------------------------------------------------------

/// A node as a dynamic-table value (`t[k] = node`).
pub fn node_val(node: &NodeRef) -> Val {
    Val::Obj(node.clone() as Rc<dyn Any>)
}

/// `node.id` as a Lua value: the number, or the string id of a legion / tattoo node
/// (CCR-0024 §1).
pub fn node_id_val(node: &Node) -> Val {
    if node.id.is_nan() {
        node.extra.get(n!("id"))
    } else {
        Val::Num(node.id)
    }
}

/// `tostring(node.id)` (for `"Tree:"..node.id`).
fn node_id_text(node: &Node) -> String {
    match node_id_val(node) {
        Val::Num(x) => tostring_num(x),
        Val::Str(s) => s.to_string(),
        other => lua_error!(
            "attempt to concatenate field 'id' (a {} value)",
            other.lua_type()
        ),
    }
}

/// `node.x, node.y` when both are non-nil (CCR-0024 §2: nil is `NaN`).
pub fn node_xy(node: &Node) -> Option<(f64, f64)> {
    let (x, y) = (node.x.get(), node.y.get());
    (!x.is_nan() && !y.is_nan()).then_some((x, y))
}

/// `node.nodesInRadius and node.nodesInRadius[radiusIndex]` (CCR-0024 §3).
pub fn nodesInRadiusAt(node: &Node, radiusIndex: f64) -> Option<Rc<LjIntMap<NodeRef>>> {
    let t = node.nodesInRadius.get()?;
    t.get(radiusIndex).obj::<LjIntMap<NodeRef>>()
}

/// A dumped value as a mutable Lua value (`loadstring(treeText)()` builds fresh tables).
/// Members in dump order: sequence, other numeric keys (ascending), string keys.
fn dv_to_val(v: &DataValue) -> Val {
    match v {
        DataValue::Nil => Val::Nil,
        DataValue::Bool(b) => Val::Bool(*b),
        DataValue::Num(x) => Val::Num(*x),
        DataValue::Str(s) => Val::Str(LStr::from(&**s)),
        DataValue::Table(t) => Val::Table(dt_to_table(t)),
        DataValue::Fn(f) => lua_error!("PassiveTree: unexpected function {:?} in tree data", f),
        DataValue::Ref(p) => lua_error!("PassiveTree: unexpected $ref {p} in tree data"),
    }
}

fn dt_to_table(t: &DataTable) -> Table {
    let out = Table::new();
    for v in &t.seq {
        out.push(dv_to_val(v));
    }
    for (k, v) in &t.num {
        out.set(Key::from(*k), dv_to_val(v));
    }
    for (k, v) in &t.fields {
        out.set(Name::from(&**k), dv_to_val(v));
    }
    for (k, v) in &t.keys {
        match k {
            DataValue::Bool(b) => out.set(Key::B(*b), dv_to_val(v)),
            other => lua_error!(
                "PassiveTree: unsupported table key {:?} in tree data",
                other
            ),
        }
    }
    out
}

fn str_list(v: &Val) -> Vec<LStr> {
    match v {
        Val::Table(t) => t
            .ipairs()
            .into_iter()
            .map(|(_, s)| match s {
                Val::Str(s) => s,
                Val::Num(x) => LStr::from(tostring_num(x).as_str()),
                other => lua_error!("PassiveTree: stat line is a {} value", other.lua_type()),
            })
            .collect(),
        Val::Nil => Vec::new(),
        other => lua_error!("PassiveTree: stat list is a {} value", other.lua_type()),
    }
}

/// A new node table `{ id = id }`: every other field nil (positions `NaN`, CCR-0024 §2).
pub fn new_node_table(id: f64) -> Node {
    Node {
        id,
        x: Cell::new(f64::NAN),
        y: Cell::new(f64::NAN),
        o: Cell::new(f64::NAN),
        oidx: Cell::new(f64::NAN),
        angle: Cell::new(f64::NAN),
        ..Node::default()
    }
}

/// A raw node table (tree, legion or tattoo data) as a [`Node`]: the census-typed keys go
/// to their fields, every other key to `node.extra` under its Lua name. `skill` (new tree
/// format) also sets `Node.id`, which L530 assigns before anything reads it.
fn node_from_data(t: &DataTable) -> Node {
    let node = new_node_table(f64::NAN);
    let mut id = f64::NAN;
    for (k, v) in &t.fields {
        let val = dv_to_val(v);
        match &**k {
            "id" => match &val {
                Val::Num(x) => id = *x,
                _ => node.extra.set(n!("id"), val),
            },
            "skill" => {
                if let Val::Num(x) = &val {
                    id = *x;
                }
                node.extra.set(n!("skill"), val);
            }
            "dn" => node.dn.set(val.str().unwrap_or_else(|| LStr::from(""))),
            "sd" => node.sd.set(Shared::new(str_list(&val))),
            "o" => node.o.set(val.opt_num().unwrap_or(f64::NAN)),
            "oidx" => node.oidx.set(val.opt_num().unwrap_or(f64::NAN)),
            "isKeystone" => node.isKeystone.set(val.truthy()),
            "isNotable" => node.isNotable.set(val.truthy()),
            "isMastery" => node.isMastery.set(val.truthy()),
            "isJewelSocket" => node.isJewelSocket.set(val.truthy()),
            "isTattoo" => node.isTattoo.set(val.truthy()),
            "isMultipleChoiceOption" => node.isMultipleChoiceOption.set(val.truthy()),
            "ascendancyName" => node.ascendancyName.set(val.str().map(|s| Name::from(&*s))),
            "classStartIndex" => node.classStartIndex.set(val.opt_num()),
            "passivePointsGranted" => node.passivePointsGranted.set(val.opt_num()),
            "masteryEffects" => node.masteryEffects.set(val.table()),
            "expansionJewel" => node.expansionJewel.set(val.table()),
            "overrideType" => node.overrideType.set(val.str().map(|s| Name::from(&*s))),
            // `group` is a group id in the raw data (L614 replaces it with the group table)
            _ => node.extra.set(Name::from(&**k), val),
        }
    }
    assert!(
        t.seq.is_empty() && t.num.is_empty() && t.keys.is_empty(),
        "PassiveTree: node table with non-string keys"
    );
    Node { id, ..node }
}

thread_local! {
    /// `require` caches modules per Lua state: every tree shares (and re-processes) one
    /// LegionPassives / TattooPassives table, as PoB does.
    static REQUIRE_CACHE: RefCell<Option<(Table, Table)>> = const { RefCell::new(None) };
}

/// `require("Data.TimelessJewelData.LegionPassives")`, `require("Data.TattooPassives")`.
fn require_legion_tattoo() -> (Table, Table) {
    if let Some(r) = REQUIRE_CACHE.with(|c| c.borrow().clone()) {
        return r;
    }
    let d = data();
    // LegionPassives: { nodes = { node, … }, additions = { … }, groups = { … } }
    let lp =
        d.timeless.legionPassives.table().unwrap_or_else(|| {
            lua_error!("module 'Data.TimelessJewelData.LegionPassives' not found")
        });
    let legion = Table::new();
    for (k, v) in &lp.fields {
        if &**k == "nodes" {
            let nodes = Table::new();
            let lt = v.table().expect("LegionPassives.nodes");
            for nv in &lt.seq {
                let nt = nv.table().expect("LegionPassives node");
                nodes.push(node_val(&Rc::new(node_from_data(nt))));
            }
            assert!(
                lt.num.is_empty() && lt.fields.is_empty(),
                "LegionPassives.nodes is a list"
            );
            legion.set(n!("nodes"), nodes);
        } else {
            legion.set(Name::from(&**k), dv_to_val(v));
        }
    }
    // TattooPassives: { nodes = { [name] = node, … }, groups = { … } }
    let tp = d
        .other
        .get("files[\"Data/TattooPassives.lua\"]")
        .and_then(DataValue::table)
        .unwrap_or_else(|| lua_error!("module 'Data.TattooPassives' not found"));
    let tattoo = Table::new();
    for (k, v) in &tp.fields {
        if &**k == "nodes" {
            let nodes = Table::new();
            let nt = v.table().expect("TattooPassives.nodes");
            for (name, nv) in &nt.fields {
                let t = nv.table().expect("TattooPassives node");
                nodes.set(Name::from(&**name), node_val(&Rc::new(node_from_data(t))));
            }
            tattoo.set(n!("nodes"), nodes);
        } else {
            tattoo.set(Name::from(&**k), dv_to_val(v));
        }
    }
    REQUIRE_CACHE.with(|c| *c.borrow_mut() = Some((legion.clone(), tattoo.clone())));
    (legion, tattoo)
}

/// `treeVersions[treeVersion].num`.
fn versionNumOf(treeVersion: Name) -> f64 {
    data()
        .gameVersions
        .treeVersions
        .get(&treeVersion)
        .unwrap_or_else(|| {
            lua_error!("attempt to index field '?' (a nil value): treeVersions[{treeVersion}]")
        })
        .num
}

/// The loaded `tree.lua` (`assert(loadstring(treeText))()`): the dumped
/// `trees["<ver>"]` ([`pob_gamedata::tree_bytes`]).
fn load_tree_data(treeVersion: Name) -> Arc<DataTable> {
    let bytes = pob_gamedata::tree_bytes(&treeVersion)
        .unwrap_or_else(|| lua_error!("PassiveTree: no tree data for version '{treeVersion}'"));
    let dump = pob_gamedata::loader::decode_bin(&bytes)
        .unwrap_or_else(|e| lua_error!("PassiveTree: tree data for '{treeVersion}': {e}"));
    let root = dump
        .roots
        .values()
        .next()
        .and_then(DataValue::table)
        .unwrap_or_else(|| lua_error!("PassiveTree: tree data for '{treeVersion}' is not a table"));
    root.clone()
}

/// The phantom key that stands in for the string key `root` of `tree.nodes` (3.10+ trees):
/// LuaJIT keeps `root` in the hash part from the table constructor until L510 assigns it
/// nil (a dead key). A negative key is never an array-part candidate, so it has the same
/// effect on rehash sizing; its slot is one of the slots `root` can take (the string hash is
/// seeded per process, so PoB's own layout varies; CCR-0024 §5).
const ROOT_PHANTOM_KEY: i64 = -1;

/// `tree.nodes` as LuaJIT builds it from the `tree.lua` table constructor (CCR-0024 §5).
/// `order` is the source order of the node keys; the dump does not record it, so the
/// default is ascending id order.
fn build_nodes(raw: &DataTable, versionNum: f64, order: Option<&[i64]>) -> LjIntMap<NodeRef> {
    let mut entries: Vec<(i64, NodeRef)> = Vec::new();
    // 2_6: a positional list `{ {…}, {…} }`
    for (i, v) in raw.seq.iter().enumerate() {
        let t = v.table().expect("tree node");
        entries.push((i as i64 + 1, Rc::new(node_from_data(t))));
    }
    for (k, v) in &raw.num {
        let t = v.table().expect("tree node");
        entries.push((*k as i64, Rc::new(node_from_data(t))));
    }
    let hasRoot = raw.fields.iter().any(|(k, _)| &**k == "root");
    assert!(
        raw.fields.iter().all(|(k, _)| &**k == "root") && raw.keys.is_empty(),
        "tree.nodes: unexpected keys"
    );
    if let Some(order) = order {
        let pos: ahash::AHashMap<i64, usize> =
            order.iter().enumerate().map(|(i, k)| (*k, i)).collect();
        entries.sort_by_key(|(k, _)| pos.get(k).copied().unwrap_or(usize::MAX));
    }
    let mut nodes;
    if !raw.seq.is_empty() {
        // TNEW with the array part sized for the positional entries (lj_parse.c expr_table)
        nodes = LjIntMap::with_sizes(raw.seq.len().min(0x7ff) as u32, 0);
    } else if hasRoot {
        // TDUP of the template `{ root = <dummy> }` (one hash entry when `root` is parsed)
        nodes = LjIntMap::with_sizes(0, 1);
        nodes.set(ROOT_PHANTOM_KEY, None);
        // the constructor's runtime store `root = {…}` (TSETS) makes the key live
        // (Lua: a non-nil value; the port keeps the node out of the map, see L510)
        nodes.set(ROOT_PHANTOM_KEY, Some(Rc::new(Node::default())));
    } else {
        // `{ [id] = {…}, … }`: TNEW with the hash part sized for every keyed entry
        nodes = LjIntMap::with_sizes(0, raw.num.len() as u32);
    }
    let _ = versionNum;
    for (k, node) in entries {
        nodes.set(k, Some(node));
    }
    nodes
}

// ---------------------------------------------------------------------------------------
// PassiveTreeClass:PassiveTree
// ---------------------------------------------------------------------------------------

/// `new("PassiveTree", treeVersion)`: loads the tree from `pob_gamedata::tree_bytes(treeVersion)`.
/// The caller sets the jewel radii first ([`setJewelRadiiGlobally`]), as `main:LoadTree` does
/// (Main.lua:352-363).
pub fn new_PassiveTree(treeVersion: Name) -> PassiveTreeRef {
    new_passive_tree_with_node_order(treeVersion, None)
}

/// [`new_PassiveTree`] with the source order of the `tree.nodes` keys (CCR-0024 §5; tests).
#[allow(clippy::needless_pass_by_value)]
pub fn new_passive_tree_with_node_order(
    treeVersion: Name,
    nodeOrder: Option<&[i64]>,
) -> PassiveTreeRef {
    let mut this = PassiveTree::default();
    let raw = load_tree_data(treeVersion);
    PassiveTree_ctor_load(&mut this, treeVersion, &raw, nodeOrder);
    Rc::new(this)
}

/// @pob Classes/PassiveTree.lua:75-81 h=36edfedf
fn PassiveTree_ctor_load(
    this: &mut PassiveTree,
    treeVersion: Name,
    raw: &DataTable,
    nodeOrder: Option<&[i64]>,
) {
    // L76
    this.treeVersion = treeVersion;
    // L77
    let versionNum = versionNumOf(treeVersion);

    // L79-80
    let (legion, tattoo) = require_legion_tattoo();
    this.legion.set(Some(legion));
    this.tattoo.set(Some(tattoo));

    // @pob-skip Classes/PassiveTree.lua:82-111 reason="tree.lua / data.json file I/O and the disabled download; the port reads the dumped trees[\"<ver>\"] (pob_gamedata::tree_bytes, AMENDMENTS A2)"

    PassiveTree_ctor_classes(this, treeVersion, versionNum, raw, nodeOrder);
}

/// @pob Classes/PassiveTree.lua:112-233 h=d887c3a8
fn PassiveTree_ctor_classes(
    this: &mut PassiveTree,
    treeVersion: Name,
    versionNum: f64,
    raw: &DataTable,
    nodeOrder: Option<&[i64]>,
) {
    // L112-114: for k, v in pairs(tree) do self[k] = v end
    for (k, v) in &raw.fields {
        match &**k {
            "nodes" => {
                let t = v.table().expect("tree.nodes");
                this.nodes = build_nodes(t, versionNum, nodeOrder);
            }
            "classes" => this.classes = dv_to_val(v).table().expect("tree.classes"),
            "alternate_ascendancies" => this.alternate_ascendancies.set(dv_to_val(v).table()),
            // image data keyed by zoom level (fractional keys): the UI's (L234-435, U23)
            "assets" | "sprites" => {}
            _ => this.extra.set(Name::from(&**k), dv_to_val(v)),
        }
    }
    assert!(
        raw.seq.is_empty() && raw.num.is_empty(),
        "tree: unexpected keys"
    );
    let extra = this.extra.clone();

    // L116
    let size = m_min(
        extra.num(n!("max_x")) - extra.num(n!("min_x")),
        extra.num(n!("max_y")) - extra.num(n!("min_y")),
    ) * 1.1;
    extra.set(n!("size"), size);

    // L118
    if versionNum >= 3.10 {
        // Migrate to old format
        // L120
        for i in 0..=6_i64 {
            // L121
            let v = this.classes.get(i + 1);
            this.classes.set(i, v);
            // L122
            this.classes.set(i + 1, Val::Nil);
        }
    }

    // Build maps of class name -> class table
    // L127-130
    this.classNameMap = Table::new();
    this.ascendNameMap = Table::new();
    this.internalAscendNameMap = Table::new();
    let classNotables = Table::new();
    extra.set(n!("classNotables"), classNotables);

    // hide legacy alternate ascendancies that are no longer obtainable
    // L133
    if let Some(alternate_ascendancies) = this.alternate_ascendancies.get() {
        // L134
        let legacyAlternateAscendancyIds = tbl! { Warden = true, Warlock = true, Primalist = true };
        // L139-140
        let filteredAlternateAscendancies = Table::new();
        let legacyAscMap = Table::new();
        // L141
        for (ascendClassId, ascendClass) in alternate_ascendancies.pairs() {
            let id = ascendClass.get(n!("id"));
            // L142
            if legacyAlternateAscendancyIds.truthy(val_key(&id)) {
                // L143
                legacyAscMap.set(val_key(&id), true);
            } else {
                // L145
                filteredAlternateAscendancies.set(ascendClassId, ascendClass);
            }
        }
        // L148
        if legacyAscMap.pairs().next().is_some() {
            // L149-156: `launch.devMode` console listing (no output in the port)
            // L157
            let temp_groups = Table::new();
            // L158
            for (nodeId, node) in this.nodes.pairs() {
                // L159
                if let Some(an) = node.ascendancyName.get() {
                    if legacyAscMap.truthy(an) {
                        // L160
                        this.nodes.set(nodeId, None);
                        // L161 (node.group is still the raw group id here)
                        let g = node.extra.get(n!("group"));
                        if !g.is_nil() {
                            temp_groups.set(val_key(&g), true);
                        }
                    }
                }
            }
            // L164
            let groups = extra.table(n!("groups")).expect("tree.groups");
            for (groupId, _) in temp_groups.pairs() {
                // L165
                groups.set(groupId, Val::Nil);
            }
            // L167
            for (legacyId, _) in legacyAscMap.pairs() {
                // L168
                this.ascendNameMap.set(legacyId, Val::Nil);
            }
        }
        // L171
        if filteredAlternateAscendancies.pairs().next().is_some() {
            // L172
            this.alternate_ascendancies
                .set(Some(filteredAlternateAscendancies));
        } else {
            // L174
            this.alternate_ascendancies.set(None);
        }
    }

    // L178
    for (classId, class) in classes_pairs(&this.classes, versionNum) {
        let class = class.table().expect("class");
        // L179
        if versionNum >= 3.10 {
            // Migrate to old format
            // L181
            class.set(n!("classes"), class.get(n!("ascendancies")));
        }
        let classClasses = class.table(n!("classes")).expect("class.classes");
        // L183
        classClasses.set(0_i64, tbl! { name = "None" });
        // L184
        this.classNameMap
            .set(val_key(&class.get(n!("name"))), classId.to_val());
        // L185
        for (ascendClassId, ascendClass) in classClasses.pairs() {
            // L186
            let entry = tbl! {
                classId = classId.to_val(),
                class = class.clone(),
                ascendClassId = ascendClassId.to_val(),
                ascendClass = ascendClass.clone(),
                flavourText = ascendClass.get(n!("flavourText")),
                flavourTextRect = ascendClass.get(n!("flavourTextRect")),
            };
            // L194
            let id = ascendClass.get(n!("id"));
            if id.truthy() {
                // L195
                this.ascendNameMap.set(val_key(&id), entry.clone());
            }
            // L197
            this.ascendNameMap
                .set(val_key(&ascendClass.get(n!("name"))), entry);
            // L198
            let internalId = ascendClass.get(n!("internalId"));
            if internalId.truthy() {
                // L199
                this.internalAscendNameMap.set(
                    val_key(&internalId),
                    tbl! {
                        classId = classId.to_val(),
                        class = class.clone(),
                        ascendClassId = ascendClassId.to_val(),
                        ascendClass = ascendClass.clone(),
                    },
                );
            }
        }
    }
    // L208
    if let Some(alternate_ascendancies) = this.alternate_ascendancies.get() {
        // L209
        this.secondaryAscendNameMap = Table::new();
        // L210
        let alternate_ascendancies_class = tbl! {
            name = "alternate_ascendancies",
            classes = alternate_ascendancies.clone(),
        };
        // L214
        for (ascendClassId, ascendClass) in alternate_ascendancies.pairs() {
            let id = val_key(&ascendClass.get(n!("id")));
            // L215
            this.ascendNameMap.set(
                id,
                tbl! {
                    classId = "alternate_ascendancies",
                    class = alternate_ascendancies_class.clone(),
                    ascendClassId = ascendClassId.to_val(),
                    ascendClass = ascendClass.clone(),
                    flavourText = ascendClass.get(n!("flavourText")),
                    flavourTextRect = ascendClass.get(n!("flavourTextRect")),
                },
            );
            // L223
            this.secondaryAscendNameMap
                .set(id, this.ascendNameMap.get(id));
        }
    }

    // L227
    let constants = extra.get(n!("constants"));
    this.skillsPerOrbit = match constants.get(n!("skillsPerOrbit")) {
        Val::Nil => legacySkillsPerOrbit.to_vec(),
        v => num_list(&v),
    };
    // L228
    this.orbitRadii = match constants.get(n!("orbitRadii")) {
        Val::Nil => legacyOrbitRadii.to_vec(),
        v => num_list(&v),
    };
    // L229
    this.orbitAnglesByOrbit = Vec::new();
    // L230
    for skillsInOrbit in this.skillsPerOrbit.clone() {
        // L231
        let angles = this.CalcOrbitAngles(skillsInOrbit);
        this.orbitAnglesByOrbit.push(angles);
    }

    // @pob-skip Classes/PassiveTree.lua:234-435 reason="UI: tree assets, sprite sheets and sprite map, bloodline sprites, alternate ascendancy emblems and legion sprites (image handles; ui doc §10.2, U23 tree_view/assets.rs)"

    PassiveTree_ctor_nodes(this, treeVersion, versionNum);
}

/// The class keys in LuaJIT `pairs()` order. 3.10+ trees: the 0..6 keys left by the
/// migration (L120-123) all sit in the array part of the 7-element list, so `next()` visits
/// 0..6. Older trees spell the classes `{[1]=…, …, [6]=…, [0]=…}` (jsonToLua of a JSON
/// object; source order verified in TreeData/2_6..3_9/tree.lua): the constructor sizes the
/// array part for key 0 only (asize 3) and the hash part for six keys, so the order is the
/// one [`LjIntMap`] gives for that history.
fn classes_pairs(classes: &Table, versionNum: f64) -> Vec<(Key, Val)> {
    let keys: Vec<i64> = if versionNum >= 3.10 {
        (0..=6).collect()
    } else {
        let mut m: LjIntMap<()> = LjIntMap::with_sizes(2, 6);
        for k in [1, 2, 3, 4, 5, 6, 0] {
            m.set(k, Some(()));
        }
        m.keys()
    };
    let mut out: Vec<(Key, Val)> = keys
        .into_iter()
        .map(|k| (Key::I(k), classes.get(k)))
        .filter(|(_, v)| !v.is_nil())
        .collect();
    // any other key (none in the shipped trees) after them
    for (k, v) in classes.pairs() {
        if !out.iter().any(|(o, _)| *o == k) {
            out.push((k, v));
        }
    }
    out
}

fn num_list(v: &Val) -> Vec<f64> {
    v.table()
        .map(|t| t.ipairs().into_iter().map(|(_, x)| x.num()).collect())
        .unwrap_or_default()
}

/// A Lua value used as a table key.
fn val_key(v: &Val) -> Key {
    match v {
        Val::Str(s) => Key::S(Name::from(&**s)),
        Val::Num(x) => Key::from(*x),
        Val::Bool(b) => Key::B(*b),
        Val::Nil => lua_error!("table index is nil"),
        other => lua_error!("PassiveTree: {} table key", other.lua_type()),
    }
}

/// @pob Classes/PassiveTree.lua:436-625 h=30661900
fn PassiveTree_ctor_nodes(this: &mut PassiveTree, treeVersion: Name, versionNum: f64) {
    let extra = this.extra.clone();
    // L437-445: local classArt (same as the module-level table)
    // L446
    let nodeOverlay = tbl! {
        Normal = tbl! {
            artWidth = 40.0,
            alloc = "PSSkillFrameActive",
            path = "PSSkillFrameHighlighted",
            unalloc = "PSSkillFrame",
            allocAscend = if versionNum >= 3.10 { "AscendancyFrameSmallAllocated" } else { "PassiveSkillScreenAscendancyFrameSmallAllocated" },
            pathAscend = if versionNum >= 3.10 { "AscendancyFrameSmallCanAllocate" } else { "PassiveSkillScreenAscendancyFrameSmallCanAllocate" },
            unallocAscend = if versionNum >= 3.10 { "AscendancyFrameSmallNormal" } else { "PassiveSkillScreenAscendancyFrameSmallNormal" },
        },
        Notable = tbl! {
            artWidth = 58.0,
            alloc = "NotableFrameAllocated",
            path = "NotableFrameCanAllocate",
            unalloc = "NotableFrameUnallocated",
            allocAscend = if versionNum >= 3.10 { "AscendancyFrameLargeAllocated" } else { "PassiveSkillScreenAscendancyFrameLargeAllocated" },
            pathAscend = if versionNum >= 3.10 { "AscendancyFrameLargeCanAllocate" } else { "PassiveSkillScreenAscendancyFrameLargeCanAllocate" },
            unallocAscend = if versionNum >= 3.10 { "AscendancyFrameLargeNormal" } else { "PassiveSkillScreenAscendancyFrameLargeNormal" },
            allocBlighted = "BlightedNotableFrameAllocated",
            pathBlighted = "BlightedNotableFrameCanAllocate",
            unallocBlighted = "BlightedNotableFrameUnallocated",
        },
        Keystone = tbl! {
            artWidth = 84.0,
            alloc = "KeystoneFrameAllocated",
            path = "KeystoneFrameCanAllocate",
            unalloc = "KeystoneFrameUnallocated",
            allocBlighted = "KeystoneFrameAllocated",
            pathBlighted = "KeystoneFrameCanAllocate",
            unallocBlighted = "KeystoneFrameUnallocated",
        },
        Socket = tbl! {
            artWidth = 58.0,
            alloc = "JewelFrameAllocated",
            path = "JewelFrameCanAllocate",
            unalloc = "JewelFrameUnallocated",
            allocAlt = "JewelSocketAltActive",
            pathAlt = "JewelSocketAltCanAllocate",
            unallocAlt = "JewelSocketAltNormal",
        },
        Mastery = tbl! {
            artWidth = 65.0,
            alloc = "AscendancyFrameLargeAllocated",
            path = "AscendancyFrameLargeCanAllocate",
            unalloc = "AscendancyFrameLargeNormal",
        },
    };
    extra.set(n!("nodeOverlay"), nodeOverlay.clone());
    // L493
    for (_, data) in nodeOverlay.pairs() {
        let data = data.table().expect("overlay");
        // L494
        let size = data.num(n!("artWidth")) * 1.33;
        // L495
        data.set(n!("size"), size);
        // L496
        data.set(n!("rsq"), size * size);
    }

    let groups = extra.table(n!("groups")).expect("tree.groups");
    // L499
    if versionNum >= 3.10 {
        // Migrate groups to old format
        // L501
        for (_, group) in groups.pairs() {
            let group = group.table().expect("group");
            // L502
            group.set(n!("n"), group.get(n!("nodes")));
            // L503
            let oo = Table::new();
            group.set(n!("oo"), oo.clone());
            // L504
            for (_, orbit) in group
                .table(n!("orbits"))
                .map(|t| t.ipairs())
                .unwrap_or_default()
            {
                // L505
                oo.set(val_key(&orbit), true);
            }
        }

        // Go away
        // L510 (the port never stored the root node: only its phantom key, now dead)
        this.nodes.set(ROOT_PHANTOM_KEY, None);
    }

    // L513: ConPrintf("Processing tree...")
    // L514-519
    this.ascendancyMap = Table::new();
    this.keystoneMap = Table::new();
    this.notableMap = Table::new();
    this.clusterNodeMap = Table::new();
    this.sockets = LjIntMap::new();
    this.masteryEffects = Table::new();
    // L520
    let mut nodeMap: ahash::AHashMap<i64, NodeRef> = ahash::AHashMap::new();
    let classNotables = extra.table(n!("classNotables")).expect("classNotables");
    // L521
    for (_, node) in this.nodes.pairs() {
        // L524 Migration...
        if versionNum < 3.10 {
            // To new format
            // L527: node.classStartIndex = node.spc[0] and node.spc[0]
            let spc0 = node.extra.get(n!("spc")).get(0_i64);
            node.classStartIndex
                .set(spc0.opt_num().filter(|_| spc0.truthy()));
        } else {
            // To old format
            // L530: node.id = node.skill (Node.id was set from `skill` at load)
            // L531
            node.extra.set(n!("g"), node.extra.get(n!("group")));
            // L532
            node.o
                .set(node.extra.get(n!("orbit")).opt_num().unwrap_or(f64::NAN));
            // L533
            node.oidx.set(
                node.extra
                    .get(n!("orbitIndex"))
                    .opt_num()
                    .unwrap_or(f64::NAN),
            );
            // L534
            node.dn
                .set(node.extra.str(n!("name")).unwrap_or_else(|| LStr::from("")));
            // L535 (a copy of the list: CCR-0024 §6)
            node.sd
                .set(Shared::new(str_list(&node.extra.get(n!("stats")))));
            // L536
            node.passivePointsGranted.set(Some(
                node.extra
                    .get(n!("grantedPassivePoints"))
                    .opt_num()
                    .unwrap_or(0.0),
            ));
        }

        // L539
        let ppg = node.passivePointsGranted.get();
        if versionNum <= 3.09
            && ppg.unwrap_or_else(|| lua_error!("attempt to compare nil with number")) > 0.0
        {
            let ppg = ppg.expect("checked");
            // L540
            node.sd.get().push(LStr::from(
                cat!(
                    "Grants ",
                    ppg,
                    " Passive Skill Point",
                    if ppg > 1.0 { "s" } else { "" }
                )
                .as_str(),
            ));
        }
        // L542: node.__index = node (spec overlays use Node::tree)
        // L543
        node.linkedId.set(Shared::new(Vec::new()));
        // L544
        nodeMap.insert(node.id as i64, node.clone());

        // Determine node type
        // L547
        if let Some(classStartIndex) = node.classStartIndex.get() {
            // L548
            node.r#type.set(n!("ClassStart"));
            // L549
            let class = this.classes.table(classStartIndex).expect("class");
            // L550
            class.set(n!("startNodeId"), node.id);
            // L551
            node.extra.set(
                n!("startArt"),
                classArt
                    .get(classStartIndex as usize)
                    .copied()
                    .map_or(Val::Nil, Val::from),
            );
        } else if node.extra.truthy(n!("isAscendancyStart")) {
            // L553
            node.r#type.set(n!("AscendClassStart"));
            // L554
            let an = node.ascendancyName.get().map_or(Val::Nil, Val::from);
            let ascendClass = this.ascendNameMap.get(val_key(&an)).get(n!("ascendClass"));
            // L555
            ascendClass
                .table()
                .expect("ascendClass")
                .set(n!("startNodeId"), node.id);
        } else if node.extra.truthy(n!("m")) || node.isMastery.get() {
            // L557
            node.r#type.set(n!("Mastery"));
            // L558
            if let Some(masteryEffects) = node.masteryEffects.get() {
                // L559
                for (_, effect) in masteryEffects.pairs() {
                    let effect = effect.table().expect("mastery effect");
                    let effectId = val_key(&effect.get(n!("effect")));
                    // L560
                    if !this.masteryEffects.truthy(effectId) {
                        // L561
                        let me = new_node_table(effect.num(n!("effect")));
                        me.sd.set(Shared::new(str_list(&effect.get(n!("stats")))));
                        let me = Rc::new(me);
                        this.masteryEffects.set(effectId, node_val(&me));
                        // L562
                        this.ProcessStats(&me, None);
                        // (Lua: `sd` is `effect.stats`, which ProcessStats split in place)
                        effect.set(n!("stats"), sd_table(&me));
                    } else {
                        // Copy multiline stats from an earlier ProcessStats call
                        // L565
                        let me = this
                            .masteryEffects
                            .get(effectId)
                            .obj::<Node>()
                            .expect("mastery effect node");
                        effect.set(n!("stats"), sd_table(&me));
                    }
                }
            }
        } else if node.isJewelSocket.get() {
            // L570
            node.r#type.set(n!("Socket"));
            // L571
            this.sockets.set(node.id as i64, Some(node.clone()));
        } else if node.extra.truthy(n!("ks")) || node.isKeystone.get() {
            // L573
            node.r#type.set(n!("Keystone"));
            let dn = node.dn.get();
            // L574
            this.keystoneMap.set(Name::from(&*dn), node_val(&node));
            // L575
            this.keystoneMap
                .set(Name::from(lower(&dn).as_str()), node_val(&node));
        } else if node.extra.truthy(n!("not")) || node.isNotable.get() {
            // L577
            node.r#type.set(n!("Notable"));
            let dnLower = Name::from(lower(&node.dn.get()).as_str());
            // L578
            if node.ascendancyName.get().is_none() {
                // Some nodes have duplicate names in the tree data for some reason, even though they're not on the tree
                // Only add them if they're actually part of a group (i.e. in the tree)
                // Add everything otherwise, because cluster jewel notables don't have a group
                // L582
                if !this.notableMap.truthy(dnLower) {
                    // L583
                    this.notableMap.set(dnLower, node_val(&node));
                } else if node.extra.truthy(n!("g")) {
                    // L585
                    this.notableMap.set(dnLower, node_val(&node));
                }
            } else {
                // L588
                this.ascendancyMap.set(dnLower, node_val(&node));
                // L589
                let className = ascend_class_name(this, &node);
                // L590
                if !classNotables.truthy(className) {
                    // L591
                    classNotables.set(className, Table::new());
                }
                // L593
                if className != n!("Scion") {
                    // L594
                    classNotables
                        .table(className)
                        .expect("classNotables")
                        .push(node.dn.get());
                }
            }
        } else {
            // L598
            node.r#type.set(n!("Normal"));
            let an = node.ascendancyName.get();
            let dn = node.dn.get();
            // L599-601
            if ((an == Some(n!("Ascendant"))
                && !node.isMultipleChoiceOption.get()
                && !dn.contains("Dexterity")
                && !dn.contains("Intelligence")
                && !dn.contains("Strength")
                && !dn.contains("Passive"))
                || (node.isMultipleChoiceOption.get() && an.is_some()))
                && an != Some(n!("Reliquarian"))
                && an != Some(n!("Luminary"))
            {
                // L602
                let className = ascend_class_name(this, &node);
                // L603
                this.ascendancyMap
                    .set(Name::from(lower(&dn).as_str()), node_val(&node));
                // L604
                if !classNotables.truthy(className) {
                    // L605
                    classNotables.set(className, Table::new());
                }
                // L607
                classNotables
                    .table(className)
                    .expect("classNotables")
                    .push(dn);
            }
        }

        // Find the node group
        // L612
        let g = node.extra.get(n!("g"));
        let group = if g.is_nil() {
            Val::Nil
        } else {
            groups.get(val_key(&g))
        };
        // L613
        if let Some(group) = group.table() {
            // L614
            node.group.set(Some(group.clone()));
            node.extra.set(n!("group"), Val::Nil);
            // L615
            group.set(
                n!("ascendancyName"),
                node.ascendancyName.get().map_or(Val::Nil, Val::from),
            );
            // L616
            if node.extra.truthy(n!("isAscendancyStart")) {
                // L617
                group.set(n!("isAscendancyStart"), true);
            }
        } else if node.r#type.get() == n!("Notable") || node.r#type.get() == n!("Keystone") {
            // L620
            this.clusterNodeMap
                .set(Name::from(&*node.dn.get()), node_val(&node));
        }

        // L623
        this.ProcessNode(&node);
    }

    PassiveTree_ctor_links(this, treeVersion, versionNum, &nodeMap);
}

/// `self.ascendNameMap[node.ascendancyName].class.name`.
fn ascend_class_name(this: &PassiveTree, node: &Node) -> Name {
    let an = node.ascendancyName.get().map_or(Val::Nil, Val::from);
    let entry = this.ascendNameMap.get(val_key(&an));
    match entry.get(n!("class")).get(n!("name")) {
        Val::Str(s) => Name::from(&*s),
        other => lua_error!("className is a {} value", other.lua_type()),
    }
}

/// `node.sd` as a Lua list table.
fn sd_table(node: &Node) -> Table {
    let t = Table::new();
    for s in node.sd.get().snapshot() {
        t.push(s);
    }
    t
}

/// `tonumber(otherId)` of a link id (`node.out` / `node["in"]` entries).
fn link_id(otherId: &Val) -> Option<f64> {
    match otherId {
        // L630-631
        Val::Str(s) => pob_lua::tonumber(s),
        Val::Num(x) => Some(*x),
        _ => None,
    }
}

/// @pob Classes/PassiveTree.lua:628-634 h=f3878e68
fn PassiveTree_ctor_links(
    this: &mut PassiveTree,
    treeVersion: Name,
    versionNum: f64,
    nodeMap: &ahash::AHashMap<i64, NodeRef>,
) {
    // @pob-skip Classes/PassiveTree.lua:626-627 reason="UI: connector polygons (self.connectors; U23 tree_view/connectors.rs)"
    // L628
    for (_, node) in this.nodes.pairs() {
        // L629
        let out = node.extra.get(n!("out"));
        for (_, otherId) in out
            .table()
            .map(|t| t.pairs().collect::<Vec<_>>())
            .unwrap_or_default()
        {
            // L630-632
            let otherId = link_id(&otherId);
            // L633: local other = nodeMap[otherId] (read only by the connector code)
            // L634: t_insert(node.linkedId, otherId) (a nil id appends nothing)
            if let Some(otherId) = otherId {
                node.linkedId.get().push(otherId);
            }
            // @pob-skip Classes/PassiveTree.lua:635-645 reason="UI: connector polygons for links between tree nodes (BuildConnector; U23 tree_view/connectors.rs)"
        }
        // @pob Classes/PassiveTree.lua:646-747 h=8b405af3
        // L647
        let r#in = node.extra.get(n!("in"));
        for (_, otherId) in r#in
            .table()
            .map(|t| t.pairs().collect::<Vec<_>>())
            .unwrap_or_default()
        {
            // L648-650
            let otherId = link_id(&otherId);
            // L651
            if let Some(otherId) = otherId {
                node.linkedId.get().push(otherId);
            }
        }
    }

    // `data.jewelRadius` (nil until `setJewelRadiiGlobally`: Data.lua:630 assigns its nil
    // return value; `main:LoadTree` selects the radii before every tree load)
    let jr = pob_gamedata::jewelRadius();
    if jr.jewelRadius.is_empty() {
        lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)");
    }
    let jewelRadius: &[JewelRadius] = &jr.jewelRadius;
    let maxJewelRadius = jr.maxJewelRadius;
    let allNodes = this.nodes.pairs();

    // Precalculate the lists of nodes that are within each radius of each socket
    // L656
    for (_, socket) in this.sockets.pairs() {
        // L657
        if socket.extra.str(n!("name")).as_deref() == Some("Charm Socket") {
            // L658
            socket.extra.set(n!("charmSocket"), true);
        } else {
            // L660-661
            let nodesInRadius = Table::new();
            let attributesInRadius = Table::new();
            socket.nodesInRadius.set(Some(nodesInRadius.clone()));
            socket
                .attributesInRadius
                .set(Some(attributesInRadius.clone()));
            let mut inRadius: Vec<LjIntMap<NodeRef>> = Vec::new();
            // L662
            for _ in jewelRadius {
                // L663-664
                inRadius.push(LjIntMap::new());
                attributesInRadius.push(Table::new());
            }

            // L667-668 (a nil position is `NaN`, CCR-0024 §2; Lua raises on the arithmetic)
            let (sx, sy) = (socket.x.get(), socket.y.get());
            if sx.is_nan() {
                lua_error!("attempt to perform arithmetic on field 'x' (a nil value)");
            }
            if sy.is_nan() {
                lua_error!("attempt to perform arithmetic on field 'y' (a nil value)");
            }
            let (minX, maxX) = (sx - maxJewelRadius, sx + maxJewelRadius);
            let (minY, maxY) = (sy - maxJewelRadius, sy + maxJewelRadius);

            // L670
            for (_, node) in &allNodes {
                // L671-673
                if node_in_box(node, minX, maxX, minY, maxY)
                    && !Rc::ptr_eq(node, &socket)
                    && !node.extra.truthy(n!("isBlighted"))
                    && node.group.get().is_some()
                    && !node.extra.truthy(n!("isProxy"))
                    && !node.group.get().expect("group").truthy(n!("isProxy"))
                    && !node.isMastery.get()
                {
                    // L674
                    let (vX, vY) = (node.x.get() - socket.x.get(), node.y.get() - socket.y.get());
                    // L675
                    let distSquared = vX * vX + vY * vY;
                    // L676
                    for (radiusIndex, radiusInfo) in jewelRadius.iter().enumerate() {
                        // L677
                        if distSquared <= outerSquared(radiusInfo)
                            && innerSquared(radiusInfo) <= distSquared
                        {
                            // L678
                            inRadius[radiusIndex].set(node.id as i64, Some(node.clone()));
                        }
                    }
                }
            }
            for (i, m) in inRadius.into_iter().enumerate() {
                nodesInRadius.set(i as i64 + 1, Val::Obj(Rc::new(m) as Rc<dyn Any>));
            }
        }
    }

    // L686 (keystoneMap holds only tree keystones here; each node once via L687)
    for (_, keystone) in this.keystoneMap.pairs() {
        let keystone = keystone.obj::<Node>().expect("keystone node");
        // L687
        if keystone.nodesInRadius.get().is_none() {
            // L688
            let nodesInRadius = Table::new();
            keystone.nodesInRadius.set(Some(nodesInRadius.clone()));
            let mut inRadius: Vec<LjIntMap<NodeRef>> = Vec::new();
            // L689
            for _ in jewelRadius {
                // L690
                inRadius.push(LjIntMap::new());
            }

            // L693
            if let Some((kx, ky)) = node_xy(&keystone) {
                // L694-695
                let (minX, maxX) = (kx - maxJewelRadius, kx + maxJewelRadius);
                let (minY, maxY) = (ky - maxJewelRadius, ky + maxJewelRadius);

                // L697
                for (_, node) in &allNodes {
                    // L698-700
                    if node_in_box(node, minX, maxX, minY, maxY)
                        && !Rc::ptr_eq(node, &keystone)
                        && !node.extra.truthy(n!("isBlighted"))
                        && node.group.get().is_some()
                        && !node.extra.truthy(n!("isProxy"))
                        && !node.group.get().expect("group").truthy(n!("isProxy"))
                        && !node.isMastery.get()
                        && !node.extra.truthy(n!("isSocket"))
                    {
                        // L701
                        let (vX, vY) = (node.x.get() - kx, node.y.get() - ky);
                        // L702
                        let distSquared = vX * vX + vY * vY;
                        // L703
                        for (radiusIndex, radiusInfo) in jewelRadius.iter().enumerate() {
                            // L704
                            if distSquared <= outerSquared(radiusInfo)
                                && innerSquared(radiusInfo) <= distSquared
                            {
                                // L705
                                inRadius[radiusIndex].set(node.id as i64, Some(node.clone()));
                            }
                        }
                    }
                }
            }
            for (i, m) in inRadius.into_iter().enumerate() {
                nodesInRadius.set(i as i64 + 1, Val::Obj(Rc::new(m) as Rc<dyn Any>));
            }
        }
    }

    // L714
    for (_, class) in classes_pairs(&this.classes, versionNum) {
        // L715
        let startNodeId = class.get(n!("startNodeId")).num();
        let startNode = nodeMap
            .get(&(startNodeId as i64))
            .unwrap_or_else(|| lua_error!("attempt to index local 'startNode' (a nil value)"));
        let className = class
            .get(n!("name"))
            .str()
            .unwrap_or_else(|| LStr::from(""));
        // L716
        for nodeId in startNode.linkedId.get().snapshot() {
            // L717
            let node = nodeMap
                .get(&(nodeId as i64))
                .unwrap_or_else(|| lua_error!("attempt to index local 'node' (a nil value)"));
            // L718
            if node.r#type.get() == n!("Normal") {
                // L719
                node.modList.get().NewMod(
                    cat!("Condition:ConnectedTo", &*className, "Start"),
                    ModType::FLAG,
                    true,
                    &format!("Tree:{}", tostring_num(nodeId)),
                    0,
                    0,
                    &[],
                );
            }
        }
    }

    let legion = this.legion.get().expect("legion");
    // The game only gives these notables numbered names. Use the manual name for
    // their first stat, or show the full stat description when no name exists yet.
    // L726
    for (_, addition) in legion
        .table(n!("additions"))
        .map(|t| t.pairs().collect::<Vec<_>>())
        .unwrap_or_default()
    {
        let addition = addition.table().expect("legion addition");
        let id = addition
            .str(n!("id"))
            .unwrap_or_else(|| lua_error!("attempt to index field 'id' (a nil value)"));
        let dn = addition
            .str(n!("dn"))
            .unwrap_or_else(|| lua_error!("attempt to index field 'dn' (a nil value)"));
        // L727
        if pob_lua::lmatch(&id, "^abyss_.+_notable_%d+$", None).is_some()
            && pob_lua::lmatch(&dn, "^Notable %d+$", None).is_some()
        {
            // L728
            let first = addition.get(n!("sortedStats")).get(1_i64);
            let named = match &first {
                Val::Str(s) => data()
                    .abyssNotableNames
                    .get(&Name::from(&**s))
                    .map(|s| Val::from(&**s)),
                _ => None,
            };
            let v = named.unwrap_or_else(|| addition.get(n!("sd")).get(1_i64));
            addition.set(n!("dn"), v);
        }
    }

    // Build ModList for legion jewels
    // L733
    for (_, node) in legion
        .table(n!("nodes"))
        .map(|t| t.pairs().collect::<Vec<_>>())
        .unwrap_or_default()
    {
        let node = node.obj::<Node>().expect("legion node");
        // Determine node type
        // L735
        if node.extra.truthy(n!("m")) {
            // L736
            node.r#type.set(n!("Mastery"));
        } else if node.extra.truthy(n!("ks")) {
            // L738
            node.r#type.set(n!("Keystone"));
            let dn = Name::from(&*node.dn.get());
            // L739
            if !this.keystoneMap.truthy(dn) {
                // Don't override good tree data with legacy keystones
                // L740
                this.keystoneMap.set(dn, node_val(&node));
            }
        } else if node.extra.truthy(n!("not")) {
            // L743
            node.r#type.set(n!("Notable"));
        } else {
            // L745
            node.r#type.set(n!("Normal"));
        }

        // @pob-skip Classes/PassiveTree.lua:748-753 reason="UI: legion node sprites (self.spriteMap; U23 tree_view/assets.rs)"

        // @pob Classes/PassiveTree.lua:754-770 h=95a3f740
        // L755
        this.ProcessStats(&node, None);
    }

    let tattoo = this.tattoo.get().expect("tattoo");
    // Build ModList for tattoos
    // L759 (string keys: PoB visits them in its per-process hash order)
    for (_, node) in tattoo
        .table(n!("nodes"))
        .map(|t| t.pairs().collect::<Vec<_>>())
        .unwrap_or_default()
    {
        let node = node.obj::<Node>().expect("tattoo node");
        // Determine node type
        // L761
        if node.extra.truthy(n!("m")) {
            // L762
            node.r#type.set(n!("Mastery"));
        } else if node.extra.truthy(n!("ks")) {
            // L764
            node.r#type.set(n!("Keystone"));
        } else if node.extra.truthy(n!("not")) {
            // L766
            node.r#type.set(n!("Notable"));
        } else {
            // L768
            node.r#type.set(n!("Normal"));
        }

        // @pob-skip Classes/PassiveTree.lua:771-777 reason="UI: tattoo node sprites and effect sprites (self.spriteMap; U23 tree_view/assets.rs)"

        // @pob Classes/PassiveTree.lua:778-793 h=dc73e499
        // create id to dn map for calcs breakdown sourcing
        // L780
        if !tattoo.truthy(n!("idMap")) {
            // L781
            tattoo.set(n!("idMap"), Table::new());
        }
        // L783
        tattoo
            .table(n!("idMap"))
            .expect("idMap")
            .set(val_key(&node_id_val(&node)), node.dn.get());

        // L785
        this.ProcessStats(&node, None);
    }

    // Late load the Generated data so we can take advantage of a tree existing
    // L789
    if treeVersion == data().gameVersions.latestTreeVersion {
        // L790
        buildTreeDependentUniques(this);
    }
    // L792: return self
}

/// `buildTreeDependentUniques(tree)` (Data/Uniques/Special/Generated.lua:548-551), as far
/// as the tree sees it. `buildForbidden` sorts every class's notable list in place
/// (`table.sort(notableTable)`, Generated.lua:568-570 and 584-586; every class except
/// `alternate_ascendancies`, Generated.lua:561-565). The item texts it and
/// `buildKeystoneItems` append to `data.uniques.generated` belong to the immutable data
/// (CCR-0024 §7: the dump is to run them; Main.lua:152 always loads the latest tree at
/// startup, so PoB always has them).
fn buildTreeDependentUniques(this: &PassiveTree) {
    let classNotables = this
        .extra
        .table(n!("classNotables"))
        .expect("classNotables");
    for (className, notableTable) in classNotables.pairs() {
        if className == Key::S(n!("alternate_ascendancies")) {
            continue;
        }
        let notableTable = notableTable.table().expect("notableTable");
        let mut names: Vec<LStr> = notableTable
            .ipairs()
            .into_iter()
            .map(|(_, v)| {
                v.str()
                    .unwrap_or_else(|| lua_error!("attempt to compare non-string values"))
            })
            .collect();
        pob_lua::sort::sort_default(&mut names);
        for (i, name) in names.into_iter().enumerate() {
            notableTable.set(i as i64 + 1, name);
        }
    }
}

fn node_in_box(node: &Node, minX: f64, maxX: f64, minY: f64, maxY: f64) -> bool {
    // `node.x and node.x >= minX and node.x <= maxX and node.y and …` (nil = NaN: false)
    let (x, y) = (node.x.get(), node.y.get());
    !x.is_nan() && x >= minX && x <= maxX && !y.is_nan() && y >= minY && y <= maxY
}

/// `radiusInfo.outerSquared` (set by [`setJewelRadiiGlobally`]; nil raises in Lua).
fn outerSquared(r: &JewelRadius) -> f64 {
    r.outerSquared.unwrap_or_else(|| {
        lua_error!("attempt to compare nil with number (radiusInfo.outerSquared)")
    })
}

fn innerSquared(r: &JewelRadius) -> f64 {
    r.innerSquared.unwrap_or_else(|| {
        lua_error!("attempt to compare number with nil (radiusInfo.innerSquared)")
    })
}

// ---------------------------------------------------------------------------------------
// ProcessStats / ProcessNode / CalcOrbitAngles
// ---------------------------------------------------------------------------------------

/// @pob Classes/PassiveTree.lua:795-872 h=cfbf0e0d
impl PassiveTree {
    pub fn ProcessStats(&self, node: &NodeRef, startIndex: Option<f64>) {
        // L796
        let startIndex = startIndex.unwrap_or(1.0);
        // L797
        if startIndex == 1.0 {
            // L798
            node.modKey.set(LStr::from(""));
            // L799
            node.mods.set(Shared::new(Vec::new()));
            // L800
            node.modList.set(ModStore::ModList(None));
        }

        // L803: `if not node.sd then return end` — the port's `sd` is never nil (every tree,
        // legion and tattoo node and every mastery effect carries a stat list)

        // Parse node modifier lines
        let sd = node.sd.get();
        let modsH = node.mods.get();
        // `node.mods` while lines are combined: `node.mods[ci]` (L833) is written before
        // `node.mods[i]` (L851), so the list has transient holes
        let mut mods: Vec<Option<Table>> = modsH.snapshot().into_iter().map(Some).collect();
        let mods_get = |mods: &Vec<Option<Table>>, i: usize| -> bool {
            i >= 1 && mods.get(i - 1).is_some_and(Option::is_some)
        };
        let mods_set = |mods: &mut Vec<Option<Table>>, i: usize, t: Table| {
            if mods.len() < i {
                mods.resize(i, None);
            }
            mods[i - 1] = Some(t);
        };
        // L808
        let mut i = startIndex as usize;
        // L809
        while let Some(cur) = sd.get(i) {
            // L810
            if cur.contains('\n') {
                // L811
                let line = cur;
                // L812
                let mut il = i;
                // L813
                sd.remove(Some(i));
                // L814
                for caps in pob_lua::gmatch(&line, "[^\n]+") {
                    // L815
                    sd.insert(il, LStr::from(caps[0].as_str()));
                    // L816
                    il += 1;
                }
            }
            // L819
            let line = sd.get(i).unwrap_or_else(|| {
                lua_error!("bad argument #1 to 'parseMod' (string expected, got nil)")
            });
            // L820
            let (mut list, mut extra) = pob_modparser::parseMod(&line, false);
            // L821
            if list.is_none() || extra.is_some() {
                // Try to combine it with one or more of the lines that follow this one
                // L823
                let mut endI = i + 1;
                // L824
                while sd.get(endI).is_some() {
                    // L825
                    let mut comb = String::from(&*line);
                    // L826
                    for ci in i + 1..=endI {
                        // L827
                        comb.push(' ');
                        comb.push_str(&sd.get(ci).expect("sd[ci]"));
                    }
                    // L829
                    (list, extra) = pob_modparser::parseMod(&comb, true);
                    // L830
                    if list.is_some() && extra.is_none() {
                        // Success, add dummy mod lists to the other lines that were combined with this one
                        // L832
                        for ci in i + 1..=endI {
                            // L833
                            mods_set(&mut mods, ci, tbl! { list = Table::new(), combined = true });
                        }
                        // L835
                        break;
                    }
                    // L837
                    endI += 1;
                }
            }
            let listT = list.map(|l| {
                let t = Table::new();
                for m in l {
                    t.push(Val::Mod(Arc::new(m)));
                }
                t
            });
            // L840
            if listT.is_none() {
                // Parser had no idea how to read this modifier
                // L842
                node.extra.set(n!("unknown"), true);
            } else if extra.is_some() {
                // Parser recognised this as a modifier but couldn't understand all of it
                // L845
                node.extra.set(n!("extra"), true);
            } else {
                // L847
                let mut modKey = String::from(&*node.modKey.get());
                for (_, r#mod) in listT.as_ref().expect("list").ipairs() {
                    let Val::Mod(r#mod) = r#mod else {
                        unreachable!()
                    };
                    // L848
                    modKey.push('[');
                    modKey.push_str(&modLib::formatMod(&r#mod));
                    modKey.push(']');
                }
                node.modKey.set(LStr::from(modKey.as_str()));
            }
            // L851
            let entry = Table::new();
            if let Some(l) = &listT {
                entry.set(n!("list"), l.clone());
            }
            if let Some(e) = &extra {
                entry.set(n!("extra"), e.as_str());
            }
            mods_set(&mut mods, i, entry);
            // L852
            i += 1;
            // L853
            while mods_get(&mods, i) {
                // Skip any lines with dummy lists added by the line combining code
                // L855
                i += 1;
            }
        }
        modsH.with_mut(|v| {
            v.clear();
            for (k, m) in mods.into_iter().enumerate() {
                v.push(m.unwrap_or_else(|| {
                    lua_error!(
                        "PassiveTree: node.mods has a hole at {} (hole-free list contract)",
                        k + 1
                    )
                }));
            }
        });

        // Build unified list of modifiers from all recognised modifier lines
        let source = format!("Tree:{}", node_id_text(node));
        let modList = node.modList.get();
        // L860
        for i in startIndex as usize..=modsH.len() {
            // L861
            let r#mod = modsH.get(i).expect("node.mods[i]");
            // L862
            if let Some(list) = r#mod.table(n!("list")) {
                if !r#mod.truthy(n!("extra")) {
                    // L863
                    for (j, m) in list.ipairs() {
                        let Val::Mod(m) = m else {
                            lua_error!("node.mods[i].list holds a non-mod")
                        };
                        // L864 (setSource mutates the mod in the list: the list gets the copy)
                        let m: ModRef = Arc::new(modLib::setSource(&m, &source));
                        list.set(j, Val::Mod(m.clone()));
                        // L865
                        modList.AddMod(m);
                    }
                }
            }
        }
        // L869
        if node.r#type.get() == n!("Keystone") {
            // L870
            node.keystoneMod.set(Some(Arc::new(modLib::createMod(
                n!("Keystone"),
                ModType::LIST,
                &*node.dn.get(),
                Some(&format!("Tree{}", node_id_text(node))),
                0,
                0,
                &[],
            ))));
        }
    }
}

/// @pob Classes/PassiveTree.lua:875-875 h=6b68609d
impl PassiveTree {
    /// Common processing code for nodes (used for both real tree nodes and subgraph nodes)
    pub fn ProcessNode(&self, node: &NodeRef) {
        // @pob-skip Classes/PassiveTree.lua:876-885 reason="UI: node sprites and mastery sprites (self.spriteMap; U23 tree_view/assets.rs)"

        // @pob Classes/PassiveTree.lua:886-906 h=33a03e45
        // L886
        let overlay = self.extra.get(n!("nodeOverlay")).get(node.r#type.get());
        node.extra.set(n!("overlay"), overlay.clone());
        // L887
        if let Some(an) = node.ascendancyName.get() {
            // L888 (self.bloodlineSpritePrefixes is built from the sprite map, L355-370: UI)
            let prefixes = self.extra.get(n!("bloodlineSpritePrefixes"));
            let v = if prefixes.truthy() {
                prefixes.get(an)
            } else {
                prefixes
            };
            node.extra.set(n!("bloodlineOverlayPrefix"), v);
        } else {
            // L890
            node.extra.set(n!("bloodlineOverlayPrefix"), Val::Nil);
        }
        // L892
        if overlay.truthy() {
            // L893
            node.extra.set(n!("rsq"), overlay.get(n!("rsq")));
            // L894
            node.extra.set(n!("size"), overlay.get(n!("size")));
        }

        // Derive the true position of the node
        // L898
        if let Some(group) = node.group.get() {
            // L899
            let (o, oidx) = (node.o.get(), node.oidx.get());
            let angle = (o >= 0.0)
                .then(|| self.orbitAnglesByOrbit.get(o as usize))
                .flatten()
                .and_then(|a| (oidx >= 0.0).then(|| a.get(oidx as usize)).flatten())
                .copied()
                .unwrap_or_else(|| {
                    lua_error!("bad argument #1 to 'sin' (number expected, got nil)")
                });
            node.angle.set(angle);
            // L900
            let orbitRadius = *self.orbitRadii.get(o as usize).unwrap_or_else(|| {
                lua_error!("attempt to perform arithmetic on local 'orbitRadius' (a nil value)")
            });
            // L901
            node.x
                .set(group.num(n!("x")) + m_sin(node.angle.get()) * orbitRadius);
            // L902
            node.y
                .set(group.num(n!("y")) - m_cos(node.angle.get()) * orbitRadius);
        }

        // L905
        self.ProcessStats(node, None);
    }
}

/// @pob Classes/PassiveTree.lua:1041-1062 h=728596d0
impl PassiveTree {
    pub fn CalcOrbitAngles(&self, nodesInOrbit: f64) -> Vec<f64> {
        // L1042
        let mut orbitAngles: Vec<f64> = Vec::new();

        // L1044
        if nodesInOrbit == 16.0 {
            // Every 30 and 45 degrees, per https://github.com/grindinggear/skilltree-export/blob/3.17.0/README.md
            // L1046
            orbitAngles = vec![
                0.0, 30.0, 45.0, 60.0, 90.0, 120.0, 135.0, 150.0, 180.0, 210.0, 225.0, 240.0,
                270.0, 300.0, 315.0, 330.0,
            ];
        } else if nodesInOrbit == 40.0 {
            // Every 10 and 45 degrees
            // L1049
            orbitAngles = vec![
                0.0, 10.0, 20.0, 30.0, 40.0, 45.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0, 110.0,
                120.0, 130.0, 135.0, 140.0, 150.0, 160.0, 170.0, 180.0, 190.0, 200.0, 210.0, 220.0,
                225.0, 230.0, 240.0, 250.0, 260.0, 270.0, 280.0, 290.0, 300.0, 310.0, 315.0, 320.0,
                330.0, 340.0, 350.0,
            ];
        } else {
            // Uniformly spaced
            // L1052
            let mut i = 0.0;
            while i <= nodesInOrbit {
                // L1053
                orbitAngles.push(360.0 * i / nodesInOrbit);
                i += 1.0;
            }
        }

        // L1057
        for degrees in &mut orbitAngles {
            // L1058
            *degrees = m_rad(*degrees);
        }

        // L1061
        orbitAngles
    }
}

/// @pob Modules/Data.lua:583-600 h=4debba84
/// `data.setJewelRadiiGlobally(treeVersion)`: selects `data.jewelRadius` for the tree version
/// and derives `outerSquared`/`innerSquared`/`data.maxJewelRadius` (the thread-local
/// [`pob_gamedata::DataSel`]; Lua writes the squares into the shared `data.jewelRadii` rows).
pub fn setJewelRadiiGlobally(treeVersion: &str) {
    // L584
    let caps = pob_lua::lmatch(treeVersion, "(%d+)_(%d+)", None)
        .unwrap_or_else(|| lua_error!("attempt to compare nil with number"));
    let major = pob_lua::tonumber(caps[0].as_str()).expect("%d+");
    let minor = pob_lua::tonumber(caps[1].as_str()).expect("%d+");
    let d = data();
    // L585
    let key = if major <= 3.0 && minor <= 15.0 {
        // L586
        n!("3_15")
    } else {
        // L588
        n!("3_16")
    };
    let mut jewelRadius: Vec<JewelRadius> = d
        .jewelRadii
        .get(&key)
        .unwrap_or_else(|| lua_error!("data.jewelRadii[\"{key}\"] is nil"))
        .clone();

    // L591
    let mut maxJewelRadius = 0.0;
    // L592
    for radiusInfo in &mut jewelRadius {
        // L593
        radiusInfo.outerSquared = Some(radiusInfo.outer * radiusInfo.outer);
        // L594
        radiusInfo.innerSquared = Some(radiusInfo.inner * radiusInfo.inner);

        // L596
        if radiusInfo.outer > maxJewelRadius {
            // L597
            maxJewelRadius = radiusInfo.outer;
        }
    }
    // L600
    pob_gamedata::data::set_jewel_radius_sel(pob_gamedata::DataSel {
        jewelRadius,
        maxJewelRadius,
    });
}
