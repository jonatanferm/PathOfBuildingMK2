//! @pob-file Classes/PassiveSpec.lua @16de4b82d
//! Classes/PassiveSpec.lua 1-1091 (spec core: load/save, URLs, allocation).
//!
//! Owner: PS-A. The spec's node copies of the tree, the XML load/save, the GGG and
//! poeplanner tree links, class selection, allocation and the path/distance searches.
//! `BuildAllDependsAndPaths` is PS-B's (`passive_spec_paths.rs`); the cluster subgraphs,
//! `ReplaceNode` and the undo state are PS-C's (`passive_spec_cluster.rs`).
//!
//! Representation (CCR-0052, `docs/port/ccr/0052-ps-a-passive-spec.md`):
//! * (§1) A spec node (`setmetatable({ linked = { }, power = { } }, treeNode)`, L56) is a
//!   [`Node`] whose fields are a shallow copy of the tree node's (handles alias the tree
//!   node's tables, as `__index` reads do), with its own `linked` and `depends` (tree nodes
//!   have no `depends`, so Lua's is `nil` → a fresh `{ }`), its own `extra` (a shallow
//!   copy of the tree node's, plus `power`) and `Node::tree = Some(treeNode)`
//!   ([`new_spec_node`]). Writes go to the spec node, as `__newindex`-less rawsets do.
//! * (§2) `curClassId`, `curAscendClassId` and `curSecondaryAscendClassId` are `NaN` while
//!   Lua has them `nil` (before the first `SelectClass`; the secondary id on trees without
//!   alternate ascendancies).
//! * (§7) Untyped spec fields live in `spec.extra` under their Lua names: `curClass`,
//!   `curAscendClass`, `curSecondaryAscendClass` (tables), `clusterHashFormatVersion`,
//!   `ignoreAllocatingSubgraph`, `checkNodeLinks`, `legacyClusterNodeMapReverse` (PS-C:
//!   a table node id → legacy node id). Untyped node fields live in `node.extra`: `visited`,
//!   `power`, `reminderText` (a list of strings), `intuitiveLeapLikesAffecting` (PS-B: a list
//!   of `Val::Obj(NodeRef)`), and on tree nodes `masteryCache` (`Val::Obj(NodeRef)`).
#![allow(
    non_snake_case,
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::needless_pass_by_value,
    clippy::explicit_auto_deref,
    clippy::unnecessary_map_or,
    clippy::self_only_used_in_recursion,
    clippy::question_mark,
    clippy::len_zero
)]

use std::cell::RefCell;

use pob_lua::bit;
use pob_lua::string::{byte_bytes, sub_bytes};

use crate::classes::passive_tree::{
    new_PassiveTree, node_val, nodesInRadiusAt, setJewelRadiiGlobally,
};
use crate::model::{XmlChild, XmlNode};
use crate::prelude::*;

// @pob Classes/PassiveSpec.lua:7-22 h=2bd6e633
// (module-level statements: pairs,ipairs,t_insert,t_remove,m_min,m_max,m_floor,b_lshift,b_rshift,band,bor,PassiveSpecClass)

// ---------------------------------------------------------------------------------------
// Port plumbing (snake_case)
// ---------------------------------------------------------------------------------------

thread_local! {
    /// `main.tree`: the trees `main:LoadTree` has built, by version.
    static MAIN_TREE: RefCell<ahash::AHashMap<Name, PassiveTreeRef>> =
        RefCell::new(ahash::AHashMap::new());
    static SPEC_STUBS: std::cell::Cell<SpecStubs> = const { std::cell::Cell::new(SpecStubs::NONE) };
}

/// Oracle-test seam (CCR-0052 §12): the `PassiveSpec` methods of one unit replaced by no-ops on
/// this thread, as the PS-A / PS-B / PS-C oracle tests replace them in PoB
/// (`PassiveSpecClass.X = function() end`), so each test runs one unit's code on both sides.
/// The product never sets it: every flag is `false` and every method runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpecStubs {
    /// `BuildAllDependsAndPaths` (PS-B) returns at once; the counters it initialises are the
    /// port's fields, which start at 0.
    pub build_all_depends_and_paths: bool,
    /// `BuildClusterJewelGraphs` (PS-C) returns at once.
    pub build_cluster_jewel_graphs: bool,
    /// `ReplaceNode` (PS-C) returns `nil` (`false`) at once.
    pub replace_node: bool,
}

impl SpecStubs {
    /// Nothing stubbed (the product).
    pub const NONE: Self = Self {
        build_all_depends_and_paths: false,
        build_cluster_jewel_graphs: false,
        replace_node: false,
    };
}

/// Installs the [`SpecStubs`] of this thread ([`SpecStubs::NONE`] restores the product).
pub fn set_spec_stubs(stubs: SpecStubs) {
    SPEC_STUBS.with(|s| s.set(stubs));
}

/// The [`SpecStubs`] of this thread.
pub fn spec_stubs() -> SpecStubs {
    SPEC_STUBS.with(std::cell::Cell::get)
}

/// `main:LoadTree(treeVersion)` (Modules/Main.lua 352-363, outside the calc scope; CCR-0052
/// §8): the cached tree of that version, built on first use; the jewel radii of the version
/// are selected on every call. `nil` for a version not in `treeVersionList`.
pub fn main_LoadTree(treeVersion: Name) -> Option<PassiveTreeRef> {
    if let Some(tree) = MAIN_TREE.with(|m| m.borrow().get(&treeVersion).cloned()) {
        setJewelRadiiGlobally(&treeVersion);
        return Some(tree);
    } else if data().gameVersions.treeVersionList.contains(&treeVersion) {
        setJewelRadiiGlobally(&treeVersion);
        let tree = new_PassiveTree(treeVersion);
        MAIN_TREE.with(|m| m.borrow_mut().insert(treeVersion, tree.clone()));
        return Some(tree);
    }
    None
}

/// Puts `tree` into the `main.tree` cache (tests: a tree built with the `tree.lua` source
/// node order, CCR-0024 §5).
pub fn main_tree_insert(treeVersion: Name, tree: PassiveTreeRef) {
    MAIN_TREE.with(|m| m.borrow_mut().insert(treeVersion, tree));
}

/// `ConPrintf(...)`: PoB's console.
fn ConPrintf(s: &str) {
    eprintln!("{s}");
}

/// `launch:ShowErrMsg(fmt, ...)`: the launcher's error popup (UI); the calc prints it.
fn ShowErrMsg(s: &str) {
    eprintln!("{s}");
}

/// `t[k]` for a number key read (`t[nil]`/`t[NaN]` reads are nil in Lua).
fn tget(t: &Table, k: f64) -> Val {
    if k.is_nan() {
        Val::Nil
    } else {
        t.get(k)
    }
}

/// A `nil` number (CCR-0052 §2) as `tostring` prints it.
fn tostring_opt(x: f64) -> String {
    if x.is_nan() {
        "nil".to_owned()
    } else {
        tostring_num(x)
    }
}

/// `tostring(v)` for the scalar values Save writes.
fn tostring_val(v: &Val) -> String {
    match v {
        Val::Nil => "nil".to_owned(),
        Val::Bool(b) => b.to_string(),
        Val::Num(x) => tostring_num(*x),
        Val::Str(s) => s.to_string(),
        other => lua_error!(
            "PassiveSpec: tostring of a {} value in Save",
            other.lua_type()
        ),
    }
}

/// `tonumber(s)` of an attribute (`nil` → `None`).
fn tonumber_attr(s: Option<&LStr>) -> Option<f64> {
    s.and_then(|s| pob_lua::tonumber(s))
}

/// `common.base64.decode(data)` (runtime/lua/base64.lua 25-38): characters outside the
/// alphabet are dropped, `=` decodes to nothing, a trailing partial byte is dropped.
pub fn base64_decode(data: &str) -> Vec<u8> {
    const B: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let (mut acc, mut nbits) = (0u32, 0u32);
    for &c in data.as_bytes() {
        let Some(f) = B.iter().position(|&x| x == c) else {
            // '=' and every character outside the alphabet give no bits
            continue;
        };
        acc = (acc << 6) | f as u32;
        nbits += 6;
        if nbits >= 8 {
            nbits -= 8;
            out.push(((acc >> nbits) & 0xff) as u8);
        }
    }
    out
}

/// `common.base64.encode(data)` (runtime/lua/base64.lua 11-22).
pub fn base64_encode(data: &[u8]) -> String {
    const B: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let v = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        let n = chunk.len() + 1;
        for k in 0..n {
            out.push(B[((v >> (18 - 6 * k)) & 63) as usize] as char);
        }
    }
    out.push_str(["", "==", "="][data.len() % 3]);
    out
}

/// `url:gsub("^.+/",""):gsub("-","+"):gsub("_","/")`.
fn url_payload(url: &str) -> String {
    let (s, _) = pob_lua::gsub(url, "^.+/", "", None);
    let (s, _) = pob_lua::gsub(&s, "-", "+", None);
    let (s, _) = pob_lua::gsub(&s, "_", "/", None);
    s
}

/// `b:byte(i)` used in arithmetic (`nil` raises).
fn byte_num(b: &[u8], i: i64) -> f64 {
    match byte_bytes(b, i) {
        Some(x) => f64::from(x),
        None => lua_error!("attempt to perform arithmetic on a nil value"),
    }
}

/// A byte string as the `&[f64]` byte list the frozen signatures take.
fn byte_list(b: &[u8]) -> Vec<f64> {
    b.iter().map(|&x| f64::from(x)).collect()
}

/// `copyTable(node, true)` on a node table: a shallow copy (every field; tables alias).
pub fn copy_node(src: &Node) -> Node {
    Node {
        id: src.id,
        dn: src.dn.clone(),
        sd: src.sd.clone(),
        r#type: src.r#type.clone(),
        isKeystone: src.isKeystone.clone(),
        isNotable: src.isNotable.clone(),
        isMastery: src.isMastery.clone(),
        isJewelSocket: src.isJewelSocket.clone(),
        isTattoo: src.isTattoo.clone(),
        isMultipleChoiceOption: src.isMultipleChoiceOption.clone(),
        ascendancyName: src.ascendancyName.clone(),
        classStartIndex: src.classStartIndex.clone(),
        passivePointsGranted: src.passivePointsGranted.clone(),
        modList: src.modList.clone(),
        modKey: src.modKey.clone(),
        mods: src.mods.clone(),
        keystoneMod: src.keystoneMod.clone(),
        finalModList: src.finalModList.clone(),
        grantedSkills: src.grantedSkills.clone(),
        x: src.x.clone(),
        y: src.y.clone(),
        group: src.group.clone(),
        o: src.o.clone(),
        oidx: src.oidx.clone(),
        angle: src.angle.clone(),
        linked: src.linked.clone(),
        linkedId: src.linkedId.clone(),
        depends: src.depends.clone(),
        path: src.path.clone(),
        pathDist: src.pathDist.clone(),
        distanceToClassStart: src.distanceToClassStart.clone(),
        connectedToStart: src.connectedToStart.clone(),
        nodesInRadius: src.nodesInRadius.clone(),
        attributesInRadius: src.attributesInRadius.clone(),
        conqueredBy: src.conqueredBy.clone(),
        alloc: src.alloc.clone(),
        expansionJewel: src.expansionJewel.clone(),
        masteryEffects: src.masteryEffects.clone(),
        allMasteryOptions: src.allMasteryOptions.clone(),
        overrideType: src.overrideType.clone(),
        tree: src.tree.clone(),
        extra: src.extra.copyTable(true),
    }
}

/// `setmetatable({ linked = { }, power = { } }, treeNode)` (L56-59; CCR-0052 §1).
///
/// `depends` gets its own empty list too: tree nodes never have one (PassiveTree.lua
/// does not set it), so the spec node's `__index` read is `nil` and
/// `node.depends = wipeTable(node.depends)` (L1142) gives every spec node a fresh `{ }`.
/// Aliasing the tree node's (always-present, empty) `Node::depends` would make node N of
/// every spec on this tree version share one list.
pub fn new_spec_node(treeNode: &NodeRef) -> Node {
    let node = copy_node(treeNode);
    node.linked.set(Shared::new(Vec::new()));
    node.depends.set(Shared::new(Vec::new()));
    node.extra.set(n!("power"), Table::new());
    Node {
        tree: Some(treeNode.clone()),
        ..node
    }
}

/// A node of a dynamic table (`Val::Obj(NodeRef)`).
fn val_node(v: &Val) -> Option<NodeRef> {
    v.obj::<Node>()
}

/// `#node.intuitiveLeapLikesAffecting` (PS-B fills the list; nil raises, as in Lua).
fn intuitive_len(node: &Node) -> usize {
    match node.extra.get(n!("intuitiveLeapLikesAffecting")) {
        Val::Table(t) => t.len(),
        other => lua_error!(
            "attempt to get length of field 'intuitiveLeapLikesAffecting' (a {} value)",
            other.lua_type()
        ),
    }
}

impl PassiveSpec {
    /// `self.nodes[id]`.
    fn node_at(&self, id: f64) -> Option<NodeRef> {
        if id.is_nan() {
            return None;
        }
        self.nodes.with(|m| m.get(id as i64))
    }

    /// `self.nodes[id]` where Lua indexes the result (nil raises).
    fn node_must(&self, id: f64) -> NodeRef {
        self.node_at(id).unwrap_or_else(|| {
            lua_error!(
                "attempt to index field '?' (a nil value): spec.nodes[{}]",
                tostring_opt(id)
            )
        })
    }

    /// `self.allocNodes[id] = node` / `= nil`.
    fn set_alloc(&self, id: f64, node: Option<NodeRef>) {
        self.allocNodes.with_mut(|m| m.set(id as i64, node));
    }

    /// `self.build` (a Weak back-pointer, calc doc §3.3).
    fn build_ref(&self) -> BuildRef {
        self.build
            .upgrade()
            .unwrap_or_else(|| lua_error!("attempt to index field 'build' (a nil value)"))
    }

    /// `self.curClass` (a class table of `tree.classes`).
    fn curClass(&self) -> Table {
        self.extra
            .table(n!("curClass"))
            .unwrap_or_else(|| lua_error!("attempt to index field 'curClass' (a nil value)"))
    }

    /// `self.build.itemsTab.items[itemId]`.
    fn item_at(&self, itemId: f64) -> Option<ItemRef> {
        if !(itemId >= 0.0 && itemId == itemId.trunc() && itemId <= f64::from(u32::MAX)) {
            return None;
        }
        let build = self.build_ref();
        build
            .itemsTab
            .items
            .with(|items| items.get(&(itemId as u32)).cloned())
    }
}

/// Breaks the node reference cycles of a dropped spec (`linked`, `depends`, `path` hold
/// other spec nodes; calc doc §3.3 rule 4; CCR-0052 §10). Lua's GC collects them.
/// Breaks the `Rc` cycles of a spec's node set (`linked`, `depends`, `path`; CCR-0052 §10).
/// Each field is replaced by a new value, never cleared in place, so a list another
/// holder aliases keeps its contents.
fn release_node_links(nodes: Vec<(i64, NodeRef)>) {
    for (_, node) in nodes {
        node.linked.set(Shared::new(Vec::new()));
        node.depends.set(Shared::new(Vec::new()));
        node.path.set(None);
    }
}

impl Drop for PassiveSpec {
    fn drop(&mut self) {
        release_node_links(self.nodes.with(LjIntMap::pairs));
    }
}

// ---------------------------------------------------------------------------------------
// Constructor, Init, Load, Save
// ---------------------------------------------------------------------------------------

/// @pob Classes/PassiveSpec.lua:27-37 h=78cda35b
/// `new("PassiveSpec", build, treeVersion, convert)`.
pub fn new_PassiveSpec(build: &BuildRef, treeVersion: Name, convert: bool) -> PassiveSpecRef {
    let this = Rc::new(PassiveSpec {
        // L30
        build: Rc::downgrade(build),
        title: Field::new(None),
        tree: Field::new(Rc::new(PassiveTree::default())),
        treeVersion: Field::new(treeVersion),
        nodes: Shared::new(LjIntMap::new()),
        allocNodes: Shared::new(LjIntMap::new()),
        allocSubgraphNodes: Shared::new(Vec::new()),
        allocExtendedNodes: Shared::new(Vec::new()),
        // nil until SelectClass (CCR-0052 §2)
        curClassId: Cell::new(f64::NAN),
        curClassName: Field::new(None),
        curAscendClassId: Cell::new(f64::NAN),
        curAscendClassName: Field::new(None),
        curAscendClassBaseName: Field::new(None),
        curSecondaryAscendClassId: Cell::new(f64::NAN),
        curSecondaryAscendClassName: Field::new(None),
        masterySelections: Shared::new(LjIntMap::new()),
        jewels: Shared::new(LjIntMap::new()),
        subGraphs: Table::new(),
        hashOverrides: Shared::new(LjIntMap::new()),
        allocatedNotableCount: Cell::new(0.0),
        allocatedKeystoneCount: Cell::new(0.0),
        allocatedMasteryCount: Cell::new(0.0),
        allocatedMasteryTypeCount: Cell::new(0.0),
        allocatedMasteryTypes: Table::new(),
        allocatedTattooTypes: Table::new(),
        ignoredNodes: Table::new(),
        splitPersonalityPath: Field::new(None),
        extra: Table::new(),
    });
    // L28: self:UndoHandler() — the undo stack is the UI's (ui doc §10.3: U25 wires
    // `UndoOps` around PS-C's CreateUndoState/RestoreUndoState)

    // Initialise and build all tables
    // L33
    this.Init(treeVersion, convert);

    // L35
    this.SelectClass(0.0);
    // L36
    this
}

/// @pob Classes/PassiveSpec.lua:39-101 h=633a551e
impl PassiveSpec {
    pub fn Init(&self, treeVersion: Name, convert: bool) {
        // L40
        self.treeVersion.set(treeVersion);
        // L41
        let tree = main_LoadTree(treeVersion).unwrap_or_else(|| {
            lua_error!("bad argument #1 to 'for iterator' (table expected, got nil)")
        });
        self.tree.set(tree.clone());
        // L42
        self.ignoredNodes.wipe();
        // L43
        self.extra.set(n!("ignoreAllocatingSubgraph"), false);
        // L44
        self.extra.set(n!("checkNodeLinks"), false);
        // L45
        let mut previousTreeNodes: Option<Shared<LjIntMap<NodeRef>>> = None;
        let mut previousAllocNodes: Option<Shared<LjIntMap<NodeRef>>> = None;
        // L46
        if convert {
            // L47
            let prevSpec = self.build_ref().spec.get();
            previousTreeNodes = Some(prevSpec.nodes.clone());
            previousAllocNodes = Some(prevSpec.allocNodes.clone());
        }

        // Make a local copy of the passive tree that we can modify
        // L51
        let mut nodes: LjIntMap<NodeRef> = LjIntMap::new();
        // L52
        for (_, treeNode) in tree.nodes.pairs() {
            // Exclude proxy or groupless nodes, as well as expansion sockets
            // L54
            let group = treeNode.group.get();
            if let Some(group) = group {
                if !treeNode.extra.truthy(n!("isProxy"))
                    && !group.truthy(n!("isProxy"))
                    && treeNode
                        .expansionJewel
                        .get()
                        .map_or(true, |ej| !ej.truthy(n!("parent")))
                {
                    // L56
                    nodes.set(treeNode.id as i64, Some(Rc::new(new_spec_node(&treeNode))));
                }
            }
        }
        let oldNodes = self.nodes.with_mut(|m| std::mem::replace(m, nodes));
        // CCR-0052 §10: a re-Init (ImportFromNodeList switching tree versions) drops the old
        // node set, which Lua's GC collects; break its cycles so it is freed here too.
        release_node_links(oldNodes.pairs());
        drop(oldNodes);
        // L62
        for (id, node) in self.nodes.with(LjIntMap::pairs) {
            // if the node is allocated and between the old and new tree has the same ID but does not share the same name, add to list of nodes to be ignored
            // L64
            if convert {
                let prev = previousTreeNodes
                    .as_ref()
                    .expect("convert")
                    .with(|m| m.get(id));
                if let Some(prev) = prev {
                    if previousAllocNodes
                        .as_ref()
                        .expect("convert")
                        .with(|m| m.contains_key(id))
                        && !node
                            .extra
                            .get(n!("name"))
                            .raw_eq(&prev.extra.get(n!("name")))
                    {
                        // L65
                        self.ignoredNodes.set(id, node_val(&prev));
                    }
                }
            }
            // L67
            for otherId in node.linkedId.get().snapshot() {
                // L68 (t_insert of nil appends nothing)
                if let Some(other) = self.node_at(otherId) {
                    node.linked.get().push(other);
                }
            }
        }

        // List of currently allocated nodes
        // Keys are node IDs, values are nodes
        // L74
        self.allocNodes.with_mut(|m| *m = LjIntMap::new());

        // List of nodes allocated in subgraphs; used to maintain allocation when loading, and when rebuilding subgraphs
        // L77
        self.allocSubgraphNodes.with_mut(|v| *v = Vec::new());

        // List of cluster nodes to allocate
        // L80
        self.allocExtendedNodes.with_mut(|v| *v = Vec::new());

        // Table of jewels equipped in this tree
        // Keys are node IDs, values are items
        // L84
        self.jewels.with_mut(|m| *m = LjIntMap::new());

        // Tree graphs dynamically generated from cluster jewels
        // Keys are subgraph IDs, values are graphs
        // L88
        self.subGraphs.wipe();

        // Keys are mastery node IDs, values are mastery effect IDs
        // L91
        self.masterySelections.with_mut(|m| *m = LjIntMap::new());

        // Keys are node IDs, values are the replacement node
        // L94
        self.hashOverrides.with_mut(|m| *m = LjIntMap::new());

        // Cached highlight path for Split Personality jewels
        // L97
        self.splitPersonalityPath.set(Some(Table::new()));

        // Cluster hash format version used by saved builds; 2 is current.
        // L100
        self.extra.set(n!("clusterHashFormatVersion"), 2.0);
    }
}

/// @pob Classes/PassiveSpec.lua:103-195 h=7ca95d98
impl PassiveSpec {
    pub fn Load(&self, xml: &XmlNode, dbFileName: &str) {
        let attr = |k: &str| xml.attrib.get(k).cloned();
        // L104
        self.title.set(attr("title"));
        // Specs without this attribute predate the hash-fix migration and are treated as legacy.
        // L106
        let chfv = tonumber_attr(attr("clusterHashFormatVersion").as_ref())
            .unwrap_or(if attr("nodes").is_some() { 1.0 } else { 2.0 });
        self.extra.set(n!("clusterHashFormatVersion"), chfv);
        // L107
        let mut url: Option<LStr> = None;
        // L108 (`pairs(xml)`: the children in order; `attrib` has no `elem`)
        for child in &xml.children {
            // L109
            let XmlChild::Node(node) = child else {
                continue;
            };
            // L110
            if &*node.elem == "URL" {
                // Legacy format
                // L112
                let Some(XmlChild::Text(text)) = node.children.first() else {
                    // L113
                    ShowErrMsg(&lfmt!(
                        "^1Error parsing '%s': 'URL' element missing content",
                        dbFileName
                    ));
                    // L114
                    return;
                };
                // L116
                url = Some(text.clone());
            } else if &*node.elem == "Sockets" {
                // L118 (`child.elem` of a text child is nil)
                for child in &node.children {
                    let XmlChild::Node(child) = child else {
                        continue;
                    };
                    // L119
                    if &*child.elem == "Socket" {
                        // L120
                        let Some(nodeId) = child.attrib.get("nodeId") else {
                            // L121
                            ShowErrMsg(&lfmt!(
                                "^1Error parsing '%s': 'Socket' element missing 'nodeId' attribute",
                                dbFileName
                            ));
                            // L122
                            return;
                        };
                        // L124
                        let Some(itemId) = child.attrib.get("itemId") else {
                            // L125
                            ShowErrMsg(&lfmt!(
                                "^1Error parsing '%s': 'Socket' element missing 'itemId' attribute",
                                dbFileName
                            ));
                            // L126
                            return;
                        };
                        // there are files which have been saved poorly and have empty jewel sockets saved as sockets with itemId zero.
                        // this check filters them out to prevent dozens of invalid jewels
                        // L130 (`jewelIdNum` is a global in Lua; nothing else reads it)
                        let jewelIdNum = pob_lua::tonumber(itemId)
                            .unwrap_or_else(|| lua_error!("attempt to compare number with nil"));
                        // L131
                        if jewelIdNum > 0.0 {
                            // L132
                            let nodeId = pob_lua::tonumber(nodeId)
                                .unwrap_or_else(|| lua_error!("table index is nil"));
                            self.jewels
                                .with_mut(|m| m.set(nodeId as i64, Some(jewelIdNum)));
                        }
                    }
                }
            }
        }
        // L139
        if let Some(nodesAttr) = attr("nodes") {
            // New format
            // L141
            if attr("classId").is_none() {
                // L142
                ShowErrMsg(&lfmt!(
                    "^1Error parsing '%s': 'Spec' element missing 'classId' attribute",
                    dbFileName
                ));
                // L143
                return;
            }
            // L145
            if attr("ascendClassId").is_none() {
                // L146
                ShowErrMsg(&lfmt!(
                    "^1Error parsing '%s': 'Spec' element missing 'ascendClassId' attribute",
                    dbFileName
                ));
                // L147
                return;
            }
            // L149
            let mut hashList: Vec<f64> = Vec::new();
            // L150
            for hash in pob_lua::gmatch(&nodesAttr, "%d+") {
                // L151
                hashList.push(hash[0].num().expect("%d+"));
            }
            // L153
            let mut masteryEffects: LjIntMap<f64> = LjIntMap::new();
            // L154
            if let Some(me) = attr("masteryEffects") {
                // L155
                for caps in pob_lua::gmatch(&me, "{(%d+),(%d+)}") {
                    // L156
                    let mastery = caps[0].num().expect("%d+");
                    let effect = caps[1].num().expect("%d+");
                    masteryEffects.set(mastery as i64, Some(effect));
                }
            }
            // L159
            for child in &xml.children {
                // L160
                let XmlChild::Node(node) = child else {
                    continue;
                };
                // L161
                if &*node.elem == "Overrides" {
                    // L162
                    for child in &node.children {
                        let XmlChild::Node(child) = child else {
                            // `child.attrib` of a text child is nil
                            lua_error!("attempt to index field 'attrib' (a nil value)")
                        };
                        // L163
                        let Some(nodeIdAttr) = child.attrib.get("nodeId") else {
                            // L164
                            ShowErrMsg(&lfmt!(
                                "^1Error parsing '%s': 'Override' element missing 'nodeId' attribute",
                                dbFileName
                            ));
                            // L165
                            return;
                        };
                        let cattr = |k: &str| -> Val {
                            child
                                .attrib
                                .get(k)
                                .map_or(Val::Nil, |s| Val::Str(s.clone()))
                        };
                        let tattoo = self.tree.get().tattoo.get().unwrap_or_else(|| {
                            lua_error!("attempt to index field 'tattoo' (a nil value)")
                        });
                        let tattooNodes = tattoo.table(n!("nodes")).unwrap_or_else(|| {
                            lua_error!("attempt to index field 'nodes' (a nil value)")
                        });
                        let dn = cattr("dn");
                        let lookup = |dn: &Val| -> Val {
                            match dn {
                                Val::Str(s) => tattooNodes.get(&**s),
                                _ => Val::Nil,
                            }
                        };

                        // In case a tattoo has been replaced by a different one attempt to find the new name for it
                        // L169
                        if !lookup(&dn).truthy() {
                            // L170 LUA-ORDER: string keys (PoB's order is random per
                            // process); with several matches the last one wins
                            for (name, data) in tattooNodes.pairs() {
                                let dataNode = val_node(&data).expect("tattoo node");
                                // L171
                                if dataNode
                                    .extra
                                    .get(n!("activeEffectImage"))
                                    .raw_eq(&cattr("activeEffectImage"))
                                    && dataNode.extra.get(n!("icon")).raw_eq(&cattr("icon"))
                                {
                                    // L172
                                    match &dn {
                                        Val::Str(s) => tattooNodes.set(&**s, data.clone()),
                                        _ => lua_error!("table index is nil"),
                                    }
                                    // L173
                                    ConPrintf(&format!(
                                        "[PassiveSpecClass:Load] {} tattoo has been substituted with {}",
                                        tostring_val(&dn),
                                        tostring_val(&name.to_val())
                                    ));
                                }
                            }
                        }

                        // If the above failed remove the tattoo to avoid crashing
                        // L179
                        let found = lookup(&dn);
                        if found.truthy() {
                            // L180
                            let nodeId = pob_lua::tonumber(nodeIdAttr)
                                .unwrap_or_else(|| lua_error!("table index is nil"));
                            // L181-182 (copyTable(node, true), then `.id = nodeId`)
                            let src = val_node(&found).expect("tattoo node");
                            let copy = copy_node(&src);
                            // tattoo nodes have no `linked`/`depends` in Lua (CCR-0052 §1)
                            copy.linked.set(Shared::new(Vec::new()));
                            copy.depends.set(Shared::new(Vec::new()));
                            copy.extra.set(n!("id"), Val::Nil);
                            let copy = Rc::new(Node { id: nodeId, ..copy });
                            self.hashOverrides
                                .with_mut(|m| m.set(nodeId as i64, Some(copy)));
                        } else {
                            // L184
                            ConPrintf(&format!(
                                "[PassiveSpecClass:Load] Failed to find a tattoo with dn of: {}",
                                match &dn {
                                    Val::Str(s) => s.to_string(),
                                    _ => lua_error!(
                                        "attempt to concatenate field 'dn' (a nil value)"
                                    ),
                                }
                            ));
                        }
                    }
                }
            }
            // L190
            let secondary = attr("secondaryAscendClassId").unwrap_or_else(|| LStr::from("0"));
            let masteryEffectsList = masteryEffects.pairs();
            self.ImportFromNodeList_impl(
                None,
                tonumber_attr(attr("classId").as_ref()).unwrap_or(f64::NAN),
                tonumber_attr(attr("ascendClassId").as_ref()).unwrap_or(f64::NAN),
                pob_lua::tonumber(&secondary).unwrap_or(f64::NAN),
                &hashList,
                self.hashOverrides.clone(),
                &masteryEffectsList
                    .into_iter()
                    .map(|(k, v)| (k as f64, v))
                    .collect::<Vec<_>>(),
                None,
            );
        } else if let Some(url) = url {
            // L192
            self.DecodeURL(&url);
        }
        // L194: self:ResetUndo() — the undo stack is the UI's (ui doc §10.3)
    }
}

/// @pob Classes/PassiveSpec.lua:197-249 h=28e40f84
impl PassiveSpec {
    pub fn Save(&self, xml: &mut XmlNode) {
        // L198
        let mut allocNodeIdList: Vec<String> = Vec::new();
        // L199
        for (nodeId, _) in self.allocNodes.with(LjIntMap::pairs) {
            // L200
            allocNodeIdList.push(tostring_num(nodeId as f64));
        }
        // L202
        let mut masterySelections: Vec<String> = Vec::new();
        // L203
        for (mastery, effect) in self.masterySelections.with(LjIntMap::pairs) {
            // L204
            masterySelections.push(format!(
                "{{{},{}}}",
                tostring_num(mastery as f64),
                tostring_num(effect)
            ));
        }
        // L206
        let mut attrib: indexmap::IndexMap<LStr, LStr> = indexmap::IndexMap::new();
        // L207 (a nil title is no attribute)
        if let Some(title) = self.title.get() {
            attrib.insert(LStr::from("title"), title);
        }
        // L208
        attrib.insert(
            LStr::from("treeVersion"),
            LStr::from(&*self.treeVersion.get()),
        );
        // L209
        let chfv = self.extra.get(n!("clusterHashFormatVersion"));
        let chfv = if chfv.truthy() { chfv } else { Val::Num(2.0) };
        attrib.insert(
            LStr::from("clusterHashFormatVersion"),
            LStr::from(tostring_val(&chfv).as_str()),
        );
        // New format
        // L211
        attrib.insert(
            LStr::from("classId"),
            LStr::from(tostring_opt(self.curClassId.get()).as_str()),
        );
        // L212
        attrib.insert(
            LStr::from("ascendClassId"),
            LStr::from(tostring_opt(self.curAscendClassId.get()).as_str()),
        );
        // L213
        attrib.insert(
            LStr::from("secondaryAscendClassId"),
            LStr::from(tostring_opt(self.curSecondaryAscendClassId.get()).as_str()),
        );
        // L214
        attrib.insert(
            LStr::from("nodes"),
            LStr::from(allocNodeIdList.join(",").as_str()),
        );
        // L215
        attrib.insert(
            LStr::from("masteryEffects"),
            LStr::from(masterySelections.join(",").as_str()),
        );
        xml.attrib = attrib;
        // L217
        xml.children.push(XmlChild::Node(XmlNode {
            // Legacy format
            elem: LStr::from("URL"),
            attrib: indexmap::IndexMap::new(),
            children: vec![XmlChild::Text(LStr::from(
                self.EncodeURL(Some("https://www.pathofexile.com/passive-skill-tree/"))
                    .as_str(),
            ))],
        }));

        // L223
        let mut sockets = XmlNode {
            elem: LStr::from("Sockets"),
            attrib: indexmap::IndexMap::new(),
            children: Vec::new(),
        };
        // L226
        for (nodeId, itemId) in self.jewels.with(LjIntMap::pairs) {
            // jewel socket contents should not be saved unless they contain a valid jewel
            // L228
            if itemId > 0.0 {
                // L229
                let mut a = indexmap::IndexMap::new();
                a.insert(
                    LStr::from("nodeId"),
                    LStr::from(tostring_num(nodeId as f64).as_str()),
                );
                a.insert(
                    LStr::from("itemId"),
                    LStr::from(tostring_num(itemId).as_str()),
                );
                // L230
                sockets.children.push(XmlChild::Node(XmlNode {
                    elem: LStr::from("Socket"),
                    attrib: a,
                    children: Vec::new(),
                }));
            }
        }
        // L233
        xml.children.push(XmlChild::Node(sockets));

        // L235
        let mut overrides = XmlNode {
            elem: LStr::from("Overrides"),
            attrib: indexmap::IndexMap::new(),
            children: Vec::new(),
        };
        // L238 (`self.hashOverrides` is never nil in the port)
        // L239
        for (nodeId, node) in self.hashOverrides.with(LjIntMap::pairs) {
            // L240
            let mut a = indexmap::IndexMap::new();
            a.insert(
                LStr::from("nodeId"),
                LStr::from(tostring_num(nodeId as f64).as_str()),
            );
            a.insert(
                LStr::from("icon"),
                LStr::from(tostring_val(&node.extra.get(n!("icon"))).as_str()),
            );
            a.insert(
                LStr::from("activeEffectImage"),
                LStr::from(tostring_val(&node.extra.get(n!("activeEffectImage"))).as_str()),
            );
            a.insert(LStr::from("dn"), node.dn.get());
            let mut r#override = XmlNode {
                elem: LStr::from("Override"),
                attrib: a,
                children: Vec::new(),
            };
            // L241
            for modLine in node.sd.get().snapshot() {
                // L242
                r#override.children.push(XmlChild::Text(modLine));
            }
            // L244
            overrides.children.push(XmlChild::Node(r#override));
        }
        // L247
        xml.children.push(XmlChild::Node(overrides));
    }
}

/// @pob Classes/PassiveSpec.lua:251-253 h=35cc6604
impl PassiveSpec {
    pub fn PostLoad(&self) {
        // L252
        self.BuildClusterJewelGraphs();
    }
}

// ---------------------------------------------------------------------------------------
// Import, URL decode/encode
// ---------------------------------------------------------------------------------------

/// @pob Classes/PassiveSpec.lua:256-312 h=836ed7ce
impl PassiveSpec {
    /// Import passive spec from the provided class IDs and node hash list.
    ///
    /// CCR-0052 §5: `hashOverrides` (`{ [nodeId] = node }`, nodes as `Val::Obj(NodeRef)`)
    /// and `masteryEffects` (`{ [masteryId] = effectId }`) are iterated in their `Table`
    /// `pairs` order (insertion order); PoB iterates them in LuaJIT's integer-key order. A
    /// caller that needs PoB's order inserts in that order (Load does, through
    /// [`PassiveSpec::ImportFromNodeList_impl`]).
    pub fn ImportFromNodeList(
        &self,
        className: Option<Name>,
        classId: f64,
        ascendClassId: f64,
        secondaryAscendClassId: f64,
        hashList: &[f64],
        hashOverrides: &Table,
        masteryEffects: &Table,
        treeVersion: Option<Name>,
    ) {
        // CCR-0052 §5: the tables as the spec's own representation
        let mut overrides: LjIntMap<NodeRef> = LjIntMap::new();
        for (k, v) in hashOverrides.pairs() {
            let Key::I(id) = k else {
                lua_error!("PassiveSpec:ImportFromNodeList: hashOverrides key is not a node id")
            };
            overrides.set(
                id,
                Some(val_node(&v).unwrap_or_else(|| {
                    lua_error!("PassiveSpec:ImportFromNodeList: hashOverrides value is not a node")
                })),
            );
        }
        let effects: Vec<(f64, f64)> = masteryEffects
            .pairs()
            .map(|(k, v)| {
                let Key::I(m) = k else {
                    lua_error!(
                        "PassiveSpec:ImportFromNodeList: masteryEffects key is not a node id"
                    )
                };
                // `tonumber(effect)`. Lua stores the raw `effect` (L279) but the frozen
                // `masterySelections` holds numbers; ImportTab passes the account JSON's
                // values, which are numbers (CCR-0052 §5), so the two agree there.
                let e = match &v {
                    Val::Num(x) => *x,
                    Val::Str(s) => pob_lua::tonumber(s)
                        .unwrap_or_else(|| lua_error!("attempt to compare nil with number")),
                    _ => lua_error!("attempt to compare nil with number"),
                };
                (m as f64, e)
            })
            .collect();
        self.ImportFromNodeList_impl(
            className,
            classId,
            ascendClassId,
            secondaryAscendClassId,
            hashList,
            Shared::new(overrides),
            &effects,
            treeVersion,
        );
    }

    /// The body of `ImportFromNodeList` on the spec's own table types (CCR-0052 §5):
    /// `hashOverrides` is assigned to `self.hashOverrides` (aliasing when it is that table),
    /// `masteryEffects` lists `pairs(masteryEffects)` in order.
    pub fn ImportFromNodeList_impl(
        &self,
        className: Option<Name>,
        mut classId: f64,
        mut ascendClassId: f64,
        secondaryAscendClassId: f64,
        hashList: &[f64],
        hashOverrides: Shared<LjIntMap<NodeRef>>,
        masteryEffects: &[(f64, f64)],
        treeVersion: Option<Name>,
    ) {
        // L257: `if hashOverrides == nil then hashOverrides = {} end` (never nil here)
        // L258
        if let Some(treeVersion) = treeVersion {
            if treeVersion != self.treeVersion.get() {
                // L259
                self.Init(treeVersion, false);
                // L260: self.build.treeTab.showConvert = self.treeVersion ~= latestTreeVersion
                // CCR-0052 §9: `TreeTab` has no `showConvert` (a UI flag); not stored
            }
        }
        // L262
        self.ResetNodes();
        // L263
        if let Some(className) = className {
            let tree = self.tree.get();
            let asc = tree.ascendNameMap.get(className);
            let iasc = tree.internalAscendNameMap.get(className);
            // L264-266
            classId = lor(tree.classNameMap.get(className), || {
                lor(land(asc.clone(), || asc.get(n!("classId"))), || {
                    land(iasc.clone(), || iasc.get(n!("classId")))
                })
            })
            .opt_num()
            .unwrap_or(f64::NAN);
            // L267-268
            ascendClassId = lor(
                lor(land(asc.clone(), || asc.get(n!("ascendClassId"))), || {
                    land(iasc.clone(), || iasc.get(n!("ascendClassId")))
                }),
                || Val::Num(0.0),
            )
            .opt_num()
            .unwrap_or(f64::NAN);
        }
        // L270
        self.SelectClass(classId);
        // L271
        self.SelectAscendClass(ascendClassId);
        // L272
        self.SelectSecondaryAscendClass(secondaryAscendClassId);
        // L273
        if !hashOverrides.ptr_eq(&self.hashOverrides) {
            let h = hashOverrides.with(Clone::clone);
            self.hashOverrides.with_mut(|m| *m = h);
        }
        // move above setting allocNodes so we can compare mastery with selection
        // L275
        self.masterySelections.with_mut(LjIntMap::wipe);
        // L276
        for &(mastery, effect) in masteryEffects {
            // ignore ggg codes from profile import
            // L278
            if effect < 65536.0 {
                // L279
                self.masterySelections
                    .with_mut(|m| m.set(mastery as i64, Some(effect)));
            }
        }
        // L282
        for (id, r#override) in self.hashOverrides.with(LjIntMap::pairs) {
            // L283
            let node = self.node_at(id as f64);
            // L284
            if let Some(node) = node {
                // L285-286: the sprite sheets are the UI's (PT does not load
                // `tree.spriteMap`, CCR-0024 §4): `spriteMap[...]` is nil
                let spriteMap = self.tree.get().extra.get(n!("spriteMap"));
                let sm = |k: Val| -> Val {
                    match (&spriteMap, k) {
                        (Val::Table(t), Val::Str(s)) => t.get(&*s),
                        _ => Val::Nil,
                    }
                };
                r#override.extra.set(
                    n!("effectSprites"),
                    sm(r#override.extra.get(n!("activeEffectImage"))),
                );
                r#override
                    .extra
                    .set(n!("sprites"), sm(r#override.extra.get(n!("icon"))));
                // L287
                self.ReplaceNode(&node, &r#override);
            }
        }
        // L290
        for &id in hashList {
            // L291
            let node = self.node_at(id);
            // L292
            if let Some(node) = node {
                // check to make sure the mastery node has a corresponding selection, if not do not allocate
                // L294
                let ty = node.r#type.get();
                if ty != n!("Mastery")
                    || (ty == n!("Mastery")
                        && self.masterySelections.with(|m| m.contains_key(id as i64)))
                {
                    // L295
                    node.alloc.set(true);
                    // L296
                    self.set_alloc(id, Some(node));
                }
            } else {
                // L299
                self.allocSubgraphNodes.push(id);
            }
        }
        // L302
        for id in self.allocExtendedNodes.snapshot() {
            // L303
            let node = self.node_at(id);
            // L304
            if let Some(node) = node {
                // L305
                node.alloc.set(true);
                // L306
                self.set_alloc(id, Some(node));
            }
        }

        // Rebuild all the node paths and dependencies
        // L311
        self.BuildAllDependsAndPaths();
    }
}

/// @pob Classes/PassiveSpec.lua:314-331 h=226da948
impl PassiveSpec {
    /// `nodes` is the byte string as byte values.
    pub fn AllocateDecodedNodes(&self, nodes: &[f64], isCluster: bool, endian: &str) {
        let byte = |i: usize| -> f64 {
            *nodes
                .get(i - 1)
                .unwrap_or_else(|| lua_error!("attempt to perform arithmetic on a nil value"))
        };
        // L315
        let mut i = 1usize;
        while i < nodes.len() {
            // L316-321
            let mut id = if endian == "big" {
                // L318
                byte(i) * 256.0 + byte(i + 1)
            } else {
                // L320
                byte(i) + byte(i + 1) * 256.0
            };
            // L322
            if isCluster {
                // L323
                id += 65536.0;
            }
            // L325
            let node = self.node_at(id);
            // L326
            if let Some(node) = node {
                // L327
                node.alloc.set(true);
                // L328
                self.set_alloc(id, Some(node));
            }
            i += 2;
        }
    }
}

/// @pob Classes/PassiveSpec.lua:333-374 h=641deb19
impl PassiveSpec {
    /// `masteryEffects` is the byte string as byte values.
    pub fn AllocateMasteryEffects(&self, masteryEffects: &[f64], endian: &str) {
        let byte = |i: usize| -> f64 {
            *masteryEffects
                .get(i - 1)
                .unwrap_or_else(|| lua_error!("attempt to perform arithmetic on a nil value"))
        };
        let allocNode = |id: f64| -> NodeRef {
            self.allocNodes
                .with(|m| m.get(id as i64))
                .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
        };
        // L334
        let mut i = 1usize;
        while i < masteryEffects.len() {
            // L335
            let effectId;
            let id;
            // L336
            if endian == "big" {
                // L337
                effectId = byte(i) * 256.0 + byte(i + 1);
                // L338
                id = byte(i + 2) * 256.0 + byte(i + 3);
            } else {
                // "little". NOTE: poeplanner swap effectId and id too.
                // L341
                effectId = byte(i + 2) + byte(i + 3) * 256.0;
                // L342
                id = byte(i) + byte(i + 1) * 256.0;
                // Assign the node, representing the Mastery, not required for GGG urls.
                // L344
                let node = self.node_at(id);
                // L345
                if let Some(node) = node {
                    // L346
                    node.alloc.set(true);
                    // L347
                    self.set_alloc(id, Some(node));
                }
            }
            // L350
            let tree = self.tree.get();
            let effect = val_node(&tget(&tree.masteryEffects, effectId));
            // L351
            if let Some(effect) = effect {
                // L352
                allocNode(id).sd.set(effect.sd.get());
                // L353
                allocNode(id).allMasteryOptions.set(false);
                // L354
                allocNode(id).extra.set(
                    n!("reminderText"),
                    tbl!["Tip: Right click to select a different effect"],
                );
                // L355
                tree.ProcessStats(&allocNode(id), None);
                // L356
                self.masterySelections
                    .with_mut(|m| m.set(id as i64, Some(effectId)));
                // L357
                self.allocatedMasteryCount
                    .set(self.allocatedMasteryCount.get() + 1.0);
                // L358
                let name = allocNode(id).extra.get(n!("name"));
                let nameKey = match &name {
                    Val::Str(s) => Key::from(&**s),
                    _ => lua_error!("table index is nil"),
                };
                if !self.allocatedMasteryTypes.truthy(nameKey) {
                    // L359
                    self.allocatedMasteryTypes.set(nameKey, 1.0);
                    // L360
                    self.allocatedMasteryTypeCount
                        .set(self.allocatedMasteryTypeCount.get() + 1.0);
                } else {
                    // L362
                    let prevCount = self.allocatedMasteryTypes.num(nameKey);
                    // L363
                    self.allocatedMasteryTypes.set(nameKey, prevCount + 1.0);
                    // L364
                    if prevCount == 0.0 {
                        // L365
                        self.allocatedMasteryTypeCount
                            .set(self.allocatedMasteryTypeCount.get() + 1.0);
                    }
                }
            } else {
                // if there is no effect/selection on the latest tree then we do not want to allocate the mastery
                // L370
                self.set_alloc(id, None);
                // L371
                self.node_must(id).alloc.set(false);
            }
            i += 4;
        }
    }
}

/// @pob Classes/PassiveSpec.lua:377-477 h=7d56c8dc
impl PassiveSpec {
    /// Decode the given poeplanner passive tree URL. Returns the error message, the tree
    /// version (`return_tree_version_only`) or `None` (the Lua function returns nothing).
    pub fn DecodePoePlannerURL(&self, url: &str, return_tree_version_only: bool) -> Option<String> {
        // poeplanner uses little endian numbers (GGG using BIG).
        // If return_tree_version_only is True, then the return value will either be an error message or the tree version.
        // both error messages begin with 'Invalid'
        // L381
        let byteToInt = |bytes: &[u8], start: i64| -> f64 {
            // get a little endian number from two bytes
            // L383
            byte_num(bytes, start) + byte_num(bytes, start + 1) * 256.0
        };

        // L386
        let b = base64_decode(&url_payload(url));
        // L387
        if b.len() < 15 {
            // L388
            return Some("Invalid tree link (unrecognised format).".to_owned());
        }

        // If we only want the tree version, exit now
        // L428
        let version = data()
            .gameVersions
            .poePlannerVersions
            .get(&(byteToInt(&b, 7) as i64))
            .copied();
        let Some(version) = version else {
            // L429
            return Some("Invalid tree version found in link.".to_owned());
        };
        // L431
        if return_tree_version_only {
            // L432
            return Some(version.to_string());
        }

        // 9 is Class, 10 is Ascendancy
        // L436
        let classId = byte_bytes(&b, 9).map_or(f64::NAN, f64::from);
        // L437
        let ascendClassId = byte_bytes(&b, 10).map_or(f64::NAN, f64::from);

        // 11 is Bandit

        // L444
        self.ResetNodes();
        // L445
        self.SelectClass(classId);
        // L446
        self.SelectAscendClass(ascendClassId);

        // 12-13 is node count
        // L449 (`idx` is a global in Lua; nothing else reads it)
        let mut idx: f64 = 12.0;
        // L450
        let nodesCount = byteToInt(&b, idx as i64);
        // L451
        let nodesEnd = idx + 2.0 + (nodesCount * 2.0);
        // L452
        let nodes = sub_bytes(&b, (idx + 2.0) as i64, Some((nodesEnd - 1.0) as i64)).to_vec();
        // L454
        self.AllocateDecodedNodes(&byte_list(&nodes), false, "little");

        // L456
        idx = nodesEnd;
        // L457
        let clusterCount = byteToInt(&b, idx as i64);
        // L458
        let clusterEnd = idx + 2.0 + (clusterCount * 2.0);
        // L459
        let clusterNodes =
            sub_bytes(&b, (idx + 2.0) as i64, Some((clusterEnd - 1.0) as i64)).to_vec();
        // L461
        self.AllocateDecodedNodes(&byte_list(&clusterNodes), true, "little");

        // poeplanner has Ascendancy nodes in a separate array
        // L464
        idx = clusterEnd;
        // L465
        let ascendancyCount = byteToInt(&b, idx as i64);
        // L466
        let ascendancyEnd = idx + 2.0 + (ascendancyCount * 2.0);
        // L467
        let ascendancyNodes =
            sub_bytes(&b, (idx + 2.0) as i64, Some((ascendancyEnd - 1.0) as i64)).to_vec();
        // L469
        self.AllocateDecodedNodes(&byte_list(&ascendancyNodes), false, "little");

        // L471
        idx = ascendancyEnd;
        // L472
        let masteryCount = byteToInt(&b, idx as i64);
        // L473
        let masteryEnd = idx + 2.0 + (masteryCount * 4.0);
        // L474
        let masteryEffects =
            sub_bytes(&b, (idx + 2.0) as i64, Some((masteryEnd - 1.0) as i64)).to_vec();
        // L476
        self.AllocateMasteryEffects(&byte_list(&masteryEffects), "little");
        None
    }
}

/// @pob Classes/PassiveSpec.lua:480-523 h=3dcb536a
impl PassiveSpec {
    /// Decode the given GGG passive tree URL. → error message (nil on success).
    pub fn DecodeURL(&self, url: &str) -> Option<String> {
        // L481
        let b = base64_decode(&url_payload(url));
        // L482
        if b.len() < 6 {
            // L483
            return Some("Invalid tree link (unrecognised format)".to_owned());
        }
        // L485
        let ver = byte_num(&b, 1) * 16_777_216.0
            + byte_num(&b, 2) * 65536.0
            + byte_num(&b, 3) * 256.0
            + byte_num(&b, 4);
        // L486
        if ver > 6.0 {
            // L487
            return Some(format!(
                "Invalid tree link (unknown version number '{}')",
                tostring_num(ver)
            ));
        }
        // L489
        let classId = byte_num(&b, 5);
        // L490
        let ascendancyIds = if ver >= 4.0 { byte_num(&b, 6) } else { 0.0 };
        // L491
        let ascendClassId = bit::tonum(bit::band(bit::tou32(ascendancyIds), 3));
        // L492
        let secondaryAscendClassId =
            bit::tonum(bit::rshift(bit::band(bit::tou32(ascendancyIds), 12), 2));
        // L493
        if !tget(&self.tree.get().classes, classId).truthy() {
            // L494
            return Some(format!(
                "Invalid tree link (bad class ID '{}')",
                tostring_num(classId)
            ));
        }
        // L496
        self.ResetNodes();
        // L497
        self.SelectClass(classId);
        // L498
        self.SelectAscendClass(ascendClassId);
        // L499
        self.SelectSecondaryAscendClass(secondaryAscendClassId);

        // L501
        let nodesStart: f64 = if ver >= 4.0 { 8.0 } else { 7.0 };
        // L502
        let nodesEnd: f64 = if ver >= 5.0 {
            7.0 + (byte_num(&b, 7) * 2.0)
        } else {
            -1.0
        };
        // L503
        let nodes = sub_bytes(&b, nodesStart as i64, Some(nodesEnd as i64)).to_vec();
        // L504
        self.AllocateDecodedNodes(&byte_list(&nodes), false, "big");

        // L506
        if ver < 5.0 {
            // L507
            return None;
        }

        // L510
        let clusterStart = nodesEnd + 1.0;
        // L511
        let clusterEnd = clusterStart + (byte_num(&b, clusterStart as i64) * 2.0);
        // L512
        let clusterNodes =
            sub_bytes(&b, (clusterStart + 1.0) as i64, Some(clusterEnd as i64)).to_vec();
        // L513
        self.AllocateDecodedNodes(&byte_list(&clusterNodes), true, "big");

        // L515
        if ver < 6.0 {
            // L516
            return None;
        }

        // L519
        let masteryStart = clusterEnd + 1.0;
        // L520
        let masteryEnd = masteryStart + (byte_num(&b, masteryStart as i64) * 4.0);
        // L521
        let masteryEffects =
            sub_bytes(&b, (masteryStart + 1.0) as i64, Some(masteryEnd as i64)).to_vec();
        // L522
        self.AllocateMasteryEffects(&byte_list(&masteryEffects), "big");
        None
    }
}

/// @pob Classes/PassiveSpec.lua:527-570 h=8fec1270
impl PassiveSpec {
    /// Encodes the current spec into a URL, using the official skill tree's format.
    /// Prepends the URL with an optional prefix.
    pub fn EncodeURL(&self, prefix: Option<&str>) -> String {
        let sec = self.curSecondaryAscendClassId.get();
        let sec = if sec.is_nan() { 0.0 } else { sec };
        let asc = self.curAscendClassId.get();
        if asc.is_nan() {
            lua_error!("bad argument #2 to 'bor' (number expected, got nil)");
        }
        // L528
        let mut a: Vec<f64> = vec![
            0.0,
            0.0,
            0.0,
            6.0,
            self.curClassId.get(),
            bit::tonum(bit::bor(bit::lshift(bit::tou32(sec), 2), bit::tou32(asc))),
        ];

        // L530
        let mut nodeCount = 0.0;
        // L531
        let mut clusterCount = 0.0;
        // L532
        let mut masteryCount = 0.0;

        // L534
        let mut clusterNodeIds: Vec<f64> = Vec::new();
        // L535
        let mut masteryNodeIds: Vec<f64> = Vec::new();

        // L537
        for (id, node) in self.allocNodes.with(LjIntMap::pairs) {
            let id = id as f64;
            let ty = node.r#type.get();
            // L538
            if ty != n!("ClassStart")
                && ty != n!("AscendClassStart")
                && id < 65536.0
                && nodeCount < 255.0
            {
                // L539
                a.push(m_floor(id / 256.0));
                // L540
                a.push(lmod(id, 256.0));
                // L541
                nodeCount += 1.0;
                // L542
                let sel = self.masterySelections.with(|m| m.get(node.id as i64));
                if let Some(effect_id) = sel {
                    // L543
                    // L544
                    masteryNodeIds.push(m_floor(effect_id / 256.0));
                    // L545
                    masteryNodeIds.push(lmod(effect_id, 256.0));
                    // L546
                    masteryNodeIds.push(m_floor(node.id / 256.0));
                    // L547
                    masteryNodeIds.push(lmod(node.id, 256.0));
                    // L548
                    masteryCount += 1.0;
                }
            } else if id >= 65536.0 {
                // L551
                let clusterId = id - 65536.0;
                // L552
                clusterNodeIds.push(m_floor(clusterId / 256.0));
                // L553
                clusterNodeIds.push(lmod(clusterId, 256.0));
                // L554
                clusterCount += 1.0;
            }
        }
        // L557
        a.insert(6, nodeCount);

        // L559
        a.push(clusterCount);
        // L560
        for id in clusterNodeIds {
            // L561
            a.push(id);
        }

        // L564
        a.push(masteryCount);
        // L565
        for id in masteryNodeIds {
            // L566
            a.push(id);
        }

        // L569: string.char(unpack(a))
        let bytes: Vec<u8> = a
            .iter()
            .enumerate()
            .map(|(i, &x)| {
                if x.is_nan() || !(0.0..=255.0).contains(&x.trunc()) {
                    lua_error!("bad argument #{} to 'char' (invalid value)", i + 1)
                }
                x as u8
            })
            .collect();
        let enc = base64_encode(&bytes);
        let (enc, _) = pob_lua::gsub(&enc, "+", "-", None);
        let (enc, _) = pob_lua::gsub(&enc, "/", "_", None);
        format!("{}{}", prefix.unwrap_or(""), enc)
    }
}

// ---------------------------------------------------------------------------------------
// Class selection
// ---------------------------------------------------------------------------------------

/// @pob Classes/PassiveSpec.lua:573-596 h=98421186
impl PassiveSpec {
    /// Change the current class, preserving currently allocated nodes if they connect to
    /// the new class's starting node.
    pub fn SelectClass(&self, classId: f64) {
        // L574
        if !self.curClassId.get().is_nan() {
            // Deallocate the current class's starting node
            // L576
            let oldStartNodeId = self.curClass().num(n!("startNodeId"));
            // L577
            self.node_must(oldStartNodeId).alloc.set(false);
            // L578
            self.set_alloc(oldStartNodeId, None);
        }

        // L581
        self.ResetAscendClass();

        // L583
        self.curClassId.set(classId);
        // L584
        let class = tget(&self.tree.get().classes, classId)
            .table()
            .unwrap_or_else(|| lua_error!("attempt to index local 'class' (a nil value)"));
        // L585
        self.extra.set(n!("curClass"), class.clone());
        // L586
        self.curClassName
            .set(class.str(n!("name")).map(|s| Name::from(&*s)));

        // Allocate the new class's starting node
        // L589
        let startNode = self.node_must(class.num(n!("startNodeId")));
        // L590
        startNode.alloc.set(true);
        // L591
        self.set_alloc(startNode.id, Some(startNode.clone()));

        // Reset the ascendancy class
        // This will also rebuild the node paths and dependencies
        // L595
        self.SelectAscendClass(0.0);
    }
}

/// @pob Classes/PassiveSpec.lua:598-608 h=e5ac1323
impl PassiveSpec {
    pub fn ResetAscendClass(&self) {
        // L599
        if !self.curAscendClassId.get().is_nan() {
            // Deallocate the current ascendancy class's start node
            // L601
            let classes = self
                .curClass()
                .table(n!("classes"))
                .unwrap_or_else(|| lua_error!("attempt to index field 'classes' (a nil value)"));
            let ascendClass = lor(tget(&classes, self.curAscendClassId.get()), || {
                classes.get(0.0)
            })
            .table()
            .unwrap_or_else(|| lua_error!("attempt to index local 'ascendClass' (a nil value)"));
            // L602
            let oldStartNodeId = ascendClass.opt_num(n!("startNodeId"));
            // L603
            if let Some(oldStartNodeId) = oldStartNodeId {
                // L604
                self.node_must(oldStartNodeId).alloc.set(false);
                // L605
                self.set_alloc(oldStartNodeId, None);
            }
        }
    }
}

/// @pob Classes/PassiveSpec.lua:610-628 h=1ba91446
impl PassiveSpec {
    pub fn SelectAscendClass(&self, ascendClassId: f64) {
        // L611
        self.ResetAscendClass();

        // L613
        self.curAscendClassId.set(ascendClassId);
        // L614
        let classes = self
            .curClass()
            .table(n!("classes"))
            .unwrap_or_else(|| lua_error!("attempt to index field 'classes' (a nil value)"));
        let ascendClass = lor(tget(&classes, ascendClassId), || classes.get(0.0))
            .table()
            .unwrap_or_else(|| lua_error!("attempt to index local 'ascendClass' (a nil value)"));
        // L615
        self.extra.set(n!("curAscendClass"), ascendClass.clone());
        // L616
        self.curAscendClassName
            .set(ascendClass.str(n!("name")).map(|s| Name::from(&*s)));
        // L617
        self.curAscendClassBaseName
            .set(ascendClass.str(n!("id")).map(|s| Name::from(&*s)));

        // L619
        if let Some(startNodeId) = ascendClass.opt_num(n!("startNodeId")) {
            // Allocate the new ascendancy class's start node
            // L621
            let startNode = self.node_must(startNodeId);
            // L622
            startNode.alloc.set(true);
            // L623
            self.set_alloc(startNode.id, Some(startNode.clone()));
        }

        // Rebuild all the node paths and dependencies
        // L627
        self.BuildAllDependsAndPaths();
    }
}

/// @pob Classes/PassiveSpec.lua:630-666 h=f74d7e40
impl PassiveSpec {
    pub fn SelectSecondaryAscendClass(&self, ascendClassId: f64) {
        // if Secondary Ascendancy does not exist on this tree version
        // L632
        let Some(alt) = self.tree.get().alternate_ascendancies.get() else {
            // L633
            return;
        };
        // L635
        if !self.curSecondaryAscendClassId.get().is_nan() {
            // Deallocate the current ascendancy class's start node
            // L637
            let ascendClass = tget(&alt, self.curSecondaryAscendClassId.get());
            // L638
            if let Some(ascendClass) = ascendClass.table() {
                // L639
                let oldStartNodeId = ascendClass.opt_num(n!("startNodeId"));
                // L640
                if let Some(oldStartNodeId) = oldStartNodeId {
                    // L641
                    self.node_must(oldStartNodeId).alloc.set(false);
                    // L642
                    self.set_alloc(oldStartNodeId, None);
                }
            }
        }

        // L647
        self.curSecondaryAscendClassId.set(ascendClassId);
        // L648
        if ascendClassId == 0.0 {
            // L649
            self.extra.set(n!("curSecondaryAscendClass"), Val::Nil);
            // L650
            self.curSecondaryAscendClassName.set(Some(n!("None")));
        } else if let Some(ascendClass) = tget(&alt, self.curSecondaryAscendClassId.get()).table() {
            // L652
            // L653
            self.extra
                .set(n!("curSecondaryAscendClass"), ascendClass.clone());
            // L654
            self.curSecondaryAscendClassName
                .set(ascendClass.str(n!("name")).map(|s| Name::from(&*s)));

            // L656
            if let Some(startNodeId) = ascendClass.opt_num(n!("startNodeId")) {
                // Allocate the new ascendancy class's start node
                // L658
                let startNode = self.node_must(startNodeId);
                // L659
                startNode.alloc.set(true);
                // L660
                self.set_alloc(startNode.id, Some(startNode.clone()));
            }
        }

        // Rebuild all the node paths and dependencies
        // L665
        self.BuildAllDependsAndPaths();
    }
}

/// @pob Classes/PassiveSpec.lua:670-691 h=9451597a
impl PassiveSpec {
    /// Determines if the given class's start node is connected to the current class's start
    /// node. Attempts to find a path between the nodes which doesn't pass through any
    /// ascendancy nodes (i.e. Ascendant).
    pub fn IsClassConnected(&self, classId: f64) -> bool {
        let class = tget(&self.tree.get().classes, classId)
            .table()
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
        // L671
        for other in self
            .node_must(class.num(n!("startNodeId")))
            .linked
            .get()
            .snapshot()
        {
            // For each of the nodes to which the given class's start node connects...
            // L673
            if other.alloc.get() {
                // If the node is allocated, try to find a path back to the current class's starting node
                // L675
                other.extra.set(n!("visited"), true);
                // L676
                let visited = Table::new();
                // L677
                let found = self.FindStartFromNode(&other, &visited, true);
                // L678
                for (_, n) in visited.ipairs() {
                    // L679
                    val_node(&n)
                        .expect("visited node")
                        .extra
                        .set(n!("visited"), false);
                }
                // L681
                other.extra.set(n!("visited"), false);
                // L682
                if found {
                    // Found a path, so the given class's start node is definitely connected to the current class's start node
                    // There might still be nodes which are connected to the current tree by an entirely different path though
                    // E.g. via Ascendant or by connecting to another "first passive node"
                    // L686
                    return true;
                }
            }
        }
        // L690
        false
    }
}

/// @pob Classes/PassiveSpec.lua:694-765 h=540a823c
impl PassiveSpec {
    /// Find and allocate the shortest path to connect to a target class's starting node.
    /// CCR-0052 §3: the Lua returns a boolean (Build.lua:273 reads it), the frozen
    /// signature none; [`PassiveSpec::ConnectToClass_ret`] returns it.
    pub fn ConnectToClass(&self, classId: f64) {
        self.ConnectToClass_ret(classId);
    }

    /// `ConnectToClass` with its return value (CCR-0052 §3).
    pub fn ConnectToClass_ret(&self, classId: f64) -> bool {
        // L695
        let classData = tget(&self.tree.get().classes, classId);
        // L696
        let Some(classData) = classData.table() else {
            // L697
            return false;
        };
        // L699
        let targetStartNode = classData
            .opt_num(n!("startNodeId"))
            .and_then(|id| self.node_at(id));
        // L700
        let Some(targetStartNode) = targetStartNode else {
            // L701
            return false;
        };

        // L704
        let isMainTreeNode = |node: &Node| -> bool {
            // L705-709
            !node.extra.truthy(n!("isProxy"))
                && node.ascendancyName.get().is_none()
                && node.r#type.get() != n!("ClassStart")
                && node.r#type.get() != n!("AscendClassStart")
        };

        // L712
        let mut visited: PtrSet<Node> = PtrSet::new();
        // L713
        let mut prev: PtrMap<Node, NodeRef> = PtrMap::new();
        // L714
        let mut queue: Vec<NodeRef> = vec![targetStartNode.clone()];
        // L715
        visited.insert(&targetStartNode);
        // L716
        let mut head = 1usize;
        // L717
        let mut foundNode: Option<NodeRef> = None;

        // L719
        while head <= queue.len() && foundNode.is_none() {
            // L720
            let node = queue[head - 1].clone();
            // L721
            head += 1;

            // L723
            let ty = node.r#type.get();
            if !Rc::ptr_eq(&node, &targetStartNode)
                && node.alloc.get()
                && node.connectedToStart.get()
                && ty != n!("ClassStart")
                && ty != n!("AscendClassStart")
            {
                // L724
                foundNode = Some(node);
                // L725
                break;
            }

            // L728
            for linked in node.linked.get().snapshot() {
                // L729
                if isMainTreeNode(&linked) && !visited.contains(&linked) {
                    // L730
                    visited.insert(&linked);
                    // L731
                    prev.set(&linked, Some(node.clone()));
                    // L732
                    queue.push(linked);
                }
            }
        }

        // L737
        let Some(foundNode) = foundNode else {
            // L738
            return false;
        };

        // L741
        let mut pathBack: Vec<NodeRef> = Vec::new();
        // L742
        let mut current = Some(foundNode);
        // L743
        while let Some(cur) = current {
            // L744
            pathBack.push(cur.clone());
            // L745
            if Rc::ptr_eq(&cur, &targetStartNode) {
                // L746
                break;
            }
            // L748
            current = prev.get(&cur);
        }

        // L751
        if !pathBack
            .last()
            .is_some_and(|n| Rc::ptr_eq(n, &targetStartNode))
        {
            // L752
            return false;
        }

        // L755
        let altPath: Shared<Vec<NodeRef>> = Shared::new(vec![pathBack[0].clone()]);
        // L756
        for idx in 2..pathBack.len() {
            // L757
            altPath.set(idx, pathBack[idx - 1].clone());
            // L758
            let node = pathBack[idx - 1].clone();
            // L759
            if !node.alloc.get() {
                // L760
                self.AllocNode(&node, Some(&altPath));
            }
        }

        // L764
        true
    }
}

// ---------------------------------------------------------------------------------------
// Allocation
// ---------------------------------------------------------------------------------------

/// @pob Classes/PassiveSpec.lua:768-776 h=e76c08e6
impl PassiveSpec {
    /// Clear the allocated status of all non-class-start nodes.
    pub fn ResetNodes(&self) {
        // L769
        for (id, node) in self.nodes.with(LjIntMap::pairs) {
            // L770
            let ty = node.r#type.get();
            if ty != n!("ClassStart") && ty != n!("AscendClassStart") {
                // L771
                node.alloc.set(false);
                // L772
                self.set_alloc(id as f64, None);
            }
        }
        // L775
        self.masterySelections.with_mut(LjIntMap::wipe);
    }
}

/// @pob Classes/PassiveSpec.lua:781-818 h=961561ce
impl PassiveSpec {
    /// Allocate the given node, if possible, and all nodes along the path to the node.
    /// An alternate path to the node may be provided, otherwise the default path will be
    /// used. The path must always contain the given node, as will be the case for the
    /// default path.
    pub fn AllocNode(&self, node: &NodeRef, altPath: Option<&Shared<Vec<NodeRef>>>) {
        // L782
        let Some(nodePath) = node.path.get() else {
            // Node cannot be connected to the tree as there is no possible path
            // L784
            return;
        };

        // Allocate all nodes along the path
        // L788
        let mut rebuildClusterJewelGraphs = false;
        // L789
        if intuitive_len(node) > 0 {
            // L790
            rebuildClusterJewelGraphs = !node.alloc.get() && node.expansionJewel.get().is_some();
            // L791
            node.alloc.set(true);
            // L792
            self.set_alloc(node.id, Some(node.clone()));
        } else {
            // L794 (ipairs reads the list live; nothing here changes it)
            let path = altPath.cloned().unwrap_or(nodePath);
            for pathNode in path.snapshot() {
                // L795
                rebuildClusterJewelGraphs = rebuildClusterJewelGraphs
                    || !pathNode.alloc.get() && pathNode.expansionJewel.get().is_some();
                // L796
                pathNode.alloc.set(true);
                // L797
                self.set_alloc(pathNode.id, Some(pathNode.clone()));
            }
        }

        // L801
        if node.isMultipleChoiceOption.get() {
            // For multiple choice passives, make sure no other choices are allocated
            // L803
            let parent = node
                .linked
                .get()
                .get(1)
                .unwrap_or_else(|| lua_error!("attempt to index local 'parent' (a nil value)"));
            // L804
            for optNode in parent.linked.get().snapshot() {
                // L805
                if optNode.isMultipleChoiceOption.get()
                    && optNode.alloc.get()
                    && !Rc::ptr_eq(&optNode, node)
                {
                    // L806
                    optNode.alloc.set(false);
                    // L807
                    self.set_alloc(optNode.id, None);
                }
            }
        }

        // L812
        if rebuildClusterJewelGraphs {
            // L813
            self.BuildClusterJewelGraphs();
        } else {
            // Rebuild all dependencies and paths for all allocated nodes
            // L816
            self.BuildAllDependsAndPaths();
        }
    }
}

/// @pob Classes/PassiveSpec.lua:820-827 h=2bef35bc
impl PassiveSpec {
    pub fn DeallocSingleNode(&self, node: &NodeRef) {
        // L821
        node.alloc.set(false);
        // L822
        self.set_alloc(node.id, None);
        // L823
        if node.r#type.get() == n!("Mastery") {
            // L824
            self.AddMasteryEffectOptionsToNode(node);
            // L825
            self.masterySelections
                .with_mut(|m| m.set(node.id as i64, None));
        }
    }
}

/// @pob Classes/PassiveSpec.lua:830-843 h=50673e99
impl PassiveSpec {
    /// Deallocate the given node, and all nodes which depend on it (i.e. which are only
    /// connected to the tree through this node).
    pub fn DeallocNode(&self, node: &NodeRef) {
        // L831
        let mut rebuildClusterJewelGraphs = false;
        // L832 (DeallocSingleNode does not change `node.depends`)
        for depNode in node.depends.get().snapshot() {
            // L833
            rebuildClusterJewelGraphs = rebuildClusterJewelGraphs
                || depNode.alloc.get() && depNode.expansionJewel.get().is_some();
            // L834
            self.DeallocSingleNode(&depNode);
        }

        // L837
        if rebuildClusterJewelGraphs {
            // L838
            self.BuildClusterJewelGraphs();
        } else {
            // Rebuild all paths and dependencies for all allocated nodes
            // L841
            self.BuildAllDependsAndPaths();
        }
    }
}

/// @pob Classes/PassiveSpec.lua:846-866 h=d2ab6728
impl PassiveSpec {
    /// Count the number of allocated nodes and allocated ascendancy nodes.
    /// → (used, ascUsed, secondaryAscUsed, sockets, weaponSet1Used, weaponSet2Used): the Lua
    /// returns the first four; the last two are `0` (CCR-0052 §6).
    pub fn CountAllocNodes(&self) -> (f64, f64, f64, f64, f64, f64) {
        // L847
        let (mut used, mut ascUsed, mut secondaryAscUsed, mut sockets) = (0.0, 0.0, 0.0, 0.0);
        let tree = self.tree.get();
        // L848
        for (_, node) in self.allocNodes.with(LjIntMap::pairs) {
            // L849
            let ty = node.r#type.get();
            if ty != n!("ClassStart") && ty != n!("AscendClassStart") {
                // L850
                if let Some(ascendancyName) = node.ascendancyName.get() {
                    // L851
                    if !node.isMultipleChoiceOption.get() {
                        // L852
                        ascUsed += 1.0;
                        // L853
                        if tree.secondaryAscendNameMap.truthy(ascendancyName) {
                            // L854
                            secondaryAscUsed += 1.0;
                        }
                    }
                } else {
                    // L858
                    used += 1.0;
                }
                // L860
                if ty == n!("Socket") {
                    // L861
                    sockets += 1.0;
                }
            }
        }
        // L865
        (used, ascUsed, secondaryAscUsed, sockets, 0.0, 0.0)
    }
}

/// @pob Classes/PassiveSpec.lua:870-899 h=d3187816
impl PassiveSpec {
    /// Attempt to find a class start node starting from the given node.
    /// Unless noAscent == true it will also look for an ascendancy class start node.
    /// `visited` is a list of `Val::Obj(NodeRef)`.
    pub fn FindStartFromNode(&self, node: &NodeRef, visited: &Table, noAscend: bool) -> bool {
        // Mark the current node as visited so we don't go around in circles
        // L872
        node.extra.set(n!("visited"), true);
        // L873
        visited.push(node_val(node));
        // For each node which is connected to this one, check if...
        // L875
        let nodeAscendancy = node.ascendancyName.get();
        // L876 (the recursion does not change `node.linked`)
        for other in node.linked.get().snapshot() {
            // Either:
            //  - the other node is a start node, or
            //  - there is a path to a start node through the other node which didn't pass through any nodes which have already been visited
            // L880
            let startIndex = nodeAscendancy.map(|_| visited.len() + 1);
            // L881
            if other.alloc.get() {
                // L882
                let otherType = other.r#type.get();
                // L883-886
                if otherType == n!("ClassStart")
                    || otherType == n!("AscendClassStart")
                    || (!other.extra.truthy(n!("visited"))
                        && node.r#type.get() != n!("Mastery")
                        && self.FindStartFromNode(&other, visited, noAscend))
                {
                    // L887
                    if nodeAscendancy.is_some() && other.ascendancyName.get().is_none() {
                        // Pathing out of Ascendant, un-visit the outside nodes
                        // L889
                        let startIndex = startIndex.expect("nodeAscendancy");
                        let last = visited.len();
                        for i in startIndex..=last {
                            // L890
                            if let Some(n) = val_node(&visited.get(i as f64)) {
                                n.extra.set(n!("visited"), false);
                            } else {
                                lua_error!("attempt to index field '?' (a nil value)");
                            }
                            // L891
                            visited.set(i as f64, Val::Nil);
                        }
                    } else if !noAscend || otherType != n!("AscendClassStart") {
                        // L894
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// @pob Classes/PassiveSpec.lua:901-910 h=4df6fc8b
impl PassiveSpec {
    pub fn GetJewel(&self, itemId: Option<f64>) -> Option<ItemRef> {
        // L902
        let itemId = match itemId {
            Some(x) if x != 0.0 => x,
            // L903
            _ => return None,
        };
        // L905
        let item = self.item_at(itemId);
        // L906
        let item = item?;
        if item.jewelData.get().is_none() {
            // L907
            return None;
        }
        // L909
        Some(item)
    }
}

/// @pob Classes/PassiveSpec.lua:912-921 h=63afb853
impl PassiveSpec {
    /// → the socketed jewel; the second value is always `None` (the Lua returns one value).
    pub fn GetSocketedJewel(&self, nodeId: f64) -> (Option<ItemRef>, Option<Table>) {
        // L913
        let mut itemId = self.jewels.with(|m| m.get(nodeId as i64));
        // L914
        if itemId.map_or(true, |x| x == 0.0) {
            if let Some(rev) = self.extra.table(n!("legacyClusterNodeMapReverse")) {
                // L915
                let legacyNodeId = tget(&rev, nodeId).opt_num();
                // L916
                if let Some(legacyNodeId) = legacyNodeId {
                    // L917
                    itemId = self.jewels.with(|m| m.get(legacyNodeId as i64));
                }
            }
        }
        // L920
        (self.GetJewel(itemId), None)
    }
}

// ---------------------------------------------------------------------------------------
// Distances and paths
// ---------------------------------------------------------------------------------------

/// @pob Classes/PassiveSpec.lua:925-964 h=0b0f9e48
impl PassiveSpec {
    /// Determine this node's distance from the class' start.
    /// Only allocated nodes can be traversed.
    pub fn SetNodeDistanceToClassStart(&self, root: &NodeRef) {
        // L926
        root.distanceToClassStart.set(Some(0.0));
        // L927
        if !root.alloc.get() || !root.connectedToStart.get() {
            // L928
            return;
        }

        // Stop once the current class' starting node is reached
        // L932
        let targetNodeId = self.curClass().opt_num(n!("startNodeId"));

        // L934
        let mut nodeDistanceToRoot: ahash::AHashMap<i64, f64> = ahash::AHashMap::new();
        // L935
        nodeDistanceToRoot.insert(root.id as i64, 0.0);

        // L937
        let mut queue: Vec<NodeRef> = vec![root.clone()];
        // L938
        let (mut o, mut i) = (1usize, 2usize); // Out, in
                                               // L939
        while o < i {
            // Nodes are processed in a queue, until there are no nodes left or the starting node is reached
            // All nodes that are 1 node away from the root will be processed first, then all nodes that are 2 nodes away, etc
            // Only allocated nodes are queued
            // L943
            let node = queue[o - 1].clone();
            // L944
            o += 1;
            // L945
            let curDist = nodeDistanceToRoot
                .get(&(node.id as i64))
                .copied()
                .unwrap_or_else(|| lua_error!("attempt to perform arithmetic on a nil value"))
                + 1.0;
            // Iterate through all nodes that are connected to this one
            // L947
            for other in node.linked.get().snapshot() {
                // If this connected node is the correct class start node, then record the distance to the node and return
                // L949
                if Some(other.id) == targetNodeId {
                    // L950
                    root.distanceToClassStart.set(Some(curDist - 1.0));
                    // L951
                    return;
                }

                // Otherwise, record the distance to this node if it hasn't already been visited
                // L955
                let otherType = other.r#type.get();
                if other.alloc.get()
                    && node.r#type.get() != n!("Mastery")
                    && otherType != n!("ClassStart")
                    && otherType != n!("AscendClassStart")
                    && !nodeDistanceToRoot.contains_key(&(other.id as i64))
                {
                    // L956
                    nodeDistanceToRoot.insert(other.id as i64, curDist);

                    // Add the other node to the end of the queue
                    // L959
                    queue.push(other);
                    // L960
                    i += 1;
                }
            }
        }
    }
}

/// @pob Classes/PassiveSpec.lua:968-1009 h=b51b3a8c
impl PassiveSpec {
    /// Determine the shortest path from the given node to the class' start (the path's
    /// nodes, in the `pairs()` order of the Lua set; CCR-0020 §7, CCR-0052 §4: the set itself
    /// is [`PassiveSpec::GetShortestPathToClassStart_set`]).
    pub fn GetShortestPathToClassStart(&self, rootId: f64) -> Option<Shared<Vec<NodeRef>>> {
        let path = self.GetShortestPathToClassStart_set(rootId)?;
        Some(Shared::new(
            path.keys()
                .into_iter()
                .filter_map(|id| self.node_at(id as f64))
                .collect(),
        ))
    }

    /// `GetShortestPathToClassStart` with the Lua result: the set `{ [nodeId] = true }` in
    /// LuaJIT's layout (CCR-0020 §7, CCR-0052 §4). Only allocated nodes can be traversed.
    pub fn GetShortestPathToClassStart_set(&self, rootId: f64) -> Option<LjIntMap<bool>> {
        // L969
        let root = self.node_at(rootId);
        // L970
        let root = match root {
            Some(r) if r.alloc.get() => r,
            // L971
            _ => return None,
        };

        // Stop once the current class' starting node is reached
        // L975
        let targetNodeId = self.curClass().opt_num(n!("startNodeId"));

        // L977
        let mut parent: ahash::AHashMap<i64, NodeRef> = ahash::AHashMap::new();
        // L978: parent[root.id] = nil

        // L980
        let mut queue: Vec<NodeRef> = vec![root.clone()];
        // L981
        let (mut o, mut i) = (1usize, 2usize); // Out, in
                                               // L982
        while o < i {
            // L983
            let node = queue[o - 1].clone();
            // L984
            o += 1;
            // Iterate through all nodes that are connected to this one
            // L986
            for other in node.linked.get().snapshot() {
                // If this connected node is the correct class start node, then construct and return the path
                // L988
                if Some(other.id) == targetNodeId {
                    // L989: `{ [root.id] = true, [other.id] = true }` (TNEW with two hash
                    // slots, then two run-time stores)
                    let mut path: LjIntMap<bool> = LjIntMap::with_sizes(0, 2);
                    path.set(root.id as i64, Some(true));
                    path.set(other.id as i64, Some(true));
                    // L990
                    let mut cur = Some(node.clone());
                    // L991
                    while let Some(c) = cur {
                        // L992
                        path.set(c.id as i64, Some(true));
                        // L993
                        cur = parent.get(&(c.id as i64)).cloned();
                    }
                    // L995
                    return Some(path);
                }

                // Otherwise, record the parent of this node if it hasn't already been visited
                // L999
                let otherType = other.r#type.get();
                if other.alloc.get()
                    && node.r#type.get() != n!("Mastery")
                    && otherType != n!("ClassStart")
                    && otherType != n!("AscendClassStart")
                    && !parent.contains_key(&(other.id as i64))
                    && other.id != root.id
                {
                    // L1000
                    parent.insert(other.id as i64, node.clone());

                    // Add the other node to the end of the queue
                    // L1003
                    queue.push(other);
                    // L1004
                    i += 1;
                }
            }
        }
        // L1008
        None
    }
}

/// @pob Classes/PassiveSpec.lua:1011-1025 h=47124b86
impl PassiveSpec {
    pub fn BuildSplitPersonalityPath(&self) {
        // L1012
        let splitPersonalityPath = Table::new();
        // L1013
        for (nodeId, itemId) in self.jewels.with(LjIntMap::pairs) {
            // L1014
            let item = self.item_at(itemId);
            // L1015
            if let Some(item) = item {
                if item
                    .jewelData
                    .get()
                    .is_some_and(|jd| jd.truthy(n!("jewelIncEffectFromClassStart")))
                {
                    // L1016
                    let path = self.GetShortestPathToClassStart_set(nodeId as f64);
                    // L1017
                    if let Some(path) = path {
                        // L1018
                        for id in path.keys() {
                            // L1019
                            splitPersonalityPath.set(id, true);
                        }
                    }
                }
            }
        }
        // L1024
        self.splitPersonalityPath.set(Some(splitPersonalityPath));
    }
}

/// @pob Classes/PassiveSpec.lua:1029-1060 h=4c8cbe9d
impl PassiveSpec {
    /// Cache the unallocated mastery option data as it doesn't change based on the build;
    /// adds stat descriptions and other node data for unallocated mastery nodes.
    pub fn AddMasteryEffectOptionsToNode(&self, node: &NodeRef) {
        let tree = self.tree.get();
        // L1030
        let treeNode = tree.nodes.get(node.id as i64);
        // L1031
        let mut cacheNode: Option<NodeRef> = treeNode
            .as_ref()
            .and_then(|t| val_node(&t.extra.get(n!("masteryCache"))));
        // L1032
        if cacheNode.is_none() {
            // L1033
            let c = Rc::new(crate::classes::passive_tree::new_node_table(node.id));
            c.sd.set(Shared::new(Vec::new()));
            // L1034
            let masteryEffects = node.masteryEffects.get();
            if let Some(me) = masteryEffects.filter(|me| me.len() > 0) {
                // L1035
                for (_, effect) in me.ipairs() {
                    // L1036
                    let effectId = effect.get(n!("effect"));
                    let effect = match effectId {
                        Val::Num(x) => val_node(&tget(&tree.masteryEffects, x)),
                        _ => None,
                    }
                    .unwrap_or_else(|| lua_error!("attempt to index local 'effect' (a nil value)"));
                    // L1037
                    for sd in effect.sd.get().snapshot() {
                        // L1038
                        c.sd.get().push(sd);
                    }
                    // L1040
                    tree.ProcessStats(&c, Some(1.0));
                }
            } else {
                // L1043
                tree.ProcessStats(&c, None);
            }
            // L1045
            if let Some(treeNode) = &treeNode {
                // L1046
                treeNode.extra.set(n!("masteryCache"), node_val(&c));
            }
            cacheNode = Some(c);
        }
        let cacheNode = cacheNode.expect("cacheNode");
        // Cached options do not include the reminder from a previously selected effect
        // L1050
        node.extra.set(n!("reminderText"), Val::Nil);
        // L1051: `pairs(cacheNode)`: id, sd and the ProcessStats fields (modKey, mods,
        // modList, unknown, extra); `node.id` already equals `cacheNode.id`
        node.sd.set(cacheNode.sd.get());
        node.modKey.set(cacheNode.modKey.get());
        node.mods.set(cacheNode.mods.get());
        // L1052
        // L1053
        node.modList.set(ModStore::ModList(None));
        // L1054
        node.modList
            .get()
            .AddList(&cacheNode.modList.get().ipairs());
        // L1056
        for (k, v) in cacheNode.extra.pairs() {
            node.extra.set(k, v);
        }
        // L1059
        node.allMasteryOptions.set(true);
    }
}

/// @pob Classes/PassiveSpec.lua:1061-1089 h=97a87698
impl PassiveSpec {
    pub fn NodesInIntuitiveLeapLikeRadius(&self, node: &NodeRef) -> Vec<NodeRef> {
        // L1062
        let mut result: Vec<NodeRef> = Vec::new();
        // L1063
        let jewel = self.jewels.with(|m| m.get(node.id as i64));
        if let Some(jewelId) = jewel.filter(|&x| x > 0.0) {
            // L1064
            let item = self.item_at(jewelId);
            // L1065
            let radiusIndex = item
                .as_ref()
                .unwrap_or_else(|| lua_error!("attempt to index local 'item' (a nil value)"))
                .jewelRadiusIndex
                .get();
            let item = item.expect("item");
            let jewelData = item.jewelData.get();
            // L1066
            if jewelData
                .as_ref()
                .is_some_and(|jd| jd.truthy(n!("intuitiveLeapLike")))
            {
                // L1067
                let inRadius =
                    radiusIndex.and_then(|ri| nodesInRadiusAt(&self.node_must(node.id), ri));
                // L1068
                for (affectedNodeId, _) in inRadius.map(|m| m.pairs()).unwrap_or_default() {
                    // L1069
                    let affected = self.node_must(affectedNodeId as f64);
                    if affected.alloc.get() {
                        // L1070
                        result.push(affected);
                    }
                }
            }

            // L1075
            if let Some(jd) = jewelData.filter(|jd| jd.truthy(n!("impossibleEscapeKeystone"))) {
                let keystones = jd
                    .table(n!("impossibleEscapeKeystones"))
                    .unwrap_or_else(|| {
                        lua_error!("bad argument #1 to 'for iterator' (table expected, got nil)")
                    });
                let tree = self.tree.get();
                // L1076 LUA-ORDER: string keys (PoB's order is random per process); the order
                // of `result` follows it
                for (keyName, _) in keystones.pairs() {
                    // L1077
                    let keyNode = val_node(&tree.keystoneMap.get(keyName));
                    if let Some(keyNode) = keyNode.filter(|k| k.nodesInRadius.get().is_some()) {
                        // L1078
                        let inRadius = radiusIndex
                            .and_then(|ri| nodesInRadiusAt(&keyNode, ri))
                            .unwrap_or_else(|| {
                                lua_error!(
                                    "bad argument #1 to 'for iterator' (table expected, got nil)"
                                )
                            });
                        for (affectedNodeId, _) in inRadius.pairs() {
                            // L1079
                            let affected = self.node_must(affectedNodeId as f64);
                            if affected.alloc.get() {
                                // L1080
                                result.push(affected);
                            }
                        }
                    }
                }
            }
        }

        // L1088
        result
    }
}

#[cfg(test)]
mod tests {
    use super::{base64_decode, base64_encode, url_payload};

    #[test]
    fn base64_round_trip_and_lenient_decode() {
        for data in [
            &b""[..],
            b"a",
            b"ab",
            b"abc",
            b"\x00\x00\x00\x06\x03\x01\xff\x10",
        ] {
            assert_eq!(base64_decode(&base64_encode(data)), data);
        }
        assert_eq!(base64_encode(b"ab"), "YWI=");
        assert_eq!(base64_encode(b"a"), "YQ==");
        // characters outside the alphabet are dropped, '=' gives nothing
        assert_eq!(base64_decode("YW?I=\n"), b"ab");
        assert_eq!(base64_decode("Y=W=I"), b"ab");
    }

    #[test]
    fn url_payload_strips_up_to_the_last_slash() {
        assert_eq!(url_payload("https://x/y/AA-_"), "AA+/");
        assert_eq!(url_payload("/AA"), "/AA");
        assert_eq!(url_payload("AA"), "AA");
    }
}
