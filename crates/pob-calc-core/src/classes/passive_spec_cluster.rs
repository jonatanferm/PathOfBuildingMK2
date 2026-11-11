//! @pob-file Classes/PassiveSpec.lua @16de4b82d
//! Cluster jewel subgraphs and legacy remaps (Classes/PassiveSpec.lua 1697-2494).
//!
//! Owner: PS-C. `ReplaceNode`, `ReconnectNodeToClassStart`, the legacy (v1) cluster hash
//! conversion, `BuildClusterJewelGraphs` / `BuildSubgraph` and their helpers, the spec undo
//! state, `NodeAdditionOrReplacementFromString` and `NodeInKeystoneRadius`.
//!
//! Representation (CCR-0052 §7 and CCR-0072, `docs/port/ccr/0072-ps-c-cluster-subgraphs.md`):
//! * A subgraph (`self.subGraphs[id]`) is a [`Table`] `{ nodes = { node, … }, group = { oo =
//!   { … }, x = …, y = … }, connectors = { … }, parentSocket = node, entranceNode = node }`;
//!   nodes are `Val::Obj(NodeRef)`. `connectors` holds one `{ node1, node2 }` pair per
//!   `linkNodes` call, in call order: `PassiveTree:BuildConnector` is the UI's (U23), which
//!   builds the connector polygons from these pairs (CCR-0072 §3).
//! * `pairs(self.subGraphs)` follows LuaJIT's order through a key mirror (an
//!   `LjIntMap<()>` replayed with every store PoB makes to the table; CCR-0072 §4).
//! * `spec.legacyClusterNodeMap` is a `Val::Obj(RefCell<LjIntMap<f64>>)` in `spec.extra`
//!   (PoB iterates it with `pairs`); `spec.legacyClusterNodeMapReverse` is a `Table`
//!   (PS-A's `GetSocketedJewel` reads it). `spec.jewel_data` (a `Table` of the account
//!   JSON's jewel data by node id) and `spec.extended_hashes` (a list `Table`) are ImportTab's
//!   untyped fields, read from `spec.extra`.
//! * Cluster nodes are fresh [`Node`]s (`tree: None`); fields their Lua constructor leaves
//!   nil keep `Node`'s defaults (`NaN` positions until `ProcessNode`, CCR-0024 §2).
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
    clippy::unnecessary_unwrap,
    clippy::neg_cmp_op_on_partial_ord,
    clippy::only_used_in_recursion
)]

use std::cell::RefCell;

use pob_gamedata::mod_tools as modLib;
use pob_lua::bit;

use crate::classes::passive_tree::{new_node_table, node_val, nodesInRadiusAt};
use crate::prelude::*;

// ---------------------------------------------------------------------------------------
// Port plumbing (snake_case)
// ---------------------------------------------------------------------------------------

/// `spec.legacyClusterNodeMap` (legacy node id → current node id), in LuaJIT's layout.
type LegacyMap = RefCell<LjIntMap<f64>>;

/// The `pairs` mirror of `spec.subGraphs` (CCR-0072 §4).
type SubGraphKeys = RefCell<LjIntMap<()>>;

thread_local! {
    /// `main:SetWindowTitleSubtext(...)` (Modules/Main.lua, the UI's): the host registers a
    /// receiver; the calc has no window (CCR-0072 §6).
    static WINDOW_TITLE_SUBTEXT: RefCell<Option<Box<dyn Fn(&str)>>> = const { RefCell::new(None) };
}

/// Registers the receiver of `main:SetWindowTitleSubtext(subtext)` (UI host plumbing).
pub fn set_window_title_subtext_hook(f: Option<Box<dyn Fn(&str)>>) {
    WINDOW_TITLE_SUBTEXT.with(|h| *h.borrow_mut() = f);
}

/// `main:SetWindowTitleSubtext(subtext)`.
fn main_SetWindowTitleSubtext(subtext: &str) {
    let hook = WINDOW_TITLE_SUBTEXT.with(|h| h.borrow_mut().take());
    if let Some(f) = hook {
        f(subtext);
        WINDOW_TITLE_SUBTEXT.with(|h| {
            let mut h = h.borrow_mut();
            if h.is_none() {
                *h = Some(f);
            }
        });
    }
}

/// `launch:ShowErrMsg(fmt, ...)`: the launcher's error popup (UI); the calc prints it.
fn ShowErrMsg(s: &str) {
    eprintln!("{s}");
}

/// A node of a dynamic table (`Val::Obj(NodeRef)`).
fn val_node(v: &Val) -> Option<NodeRef> {
    v.obj::<Node>()
}

/// `tonumber(v)` (numbers and numeric strings; anything else is nil).
fn tonumber_val(v: &Val) -> Option<f64> {
    match v {
        Val::Num(x) => Some(*x),
        Val::Str(s) => pob_lua::tonumber(s),
        _ => None,
    }
}

/// A Lua number that may be `nil` (`NaN`, CCR-0052 §2) as a table value.
fn num_val(x: f64) -> Val {
    if x.is_nan() {
        Val::Nil
    } else {
        Val::Num(x)
    }
}

/// `v[k]` where Lua indexes `v` (nil raises).
fn index(v: &Val, k: Name, what: &str) -> Val {
    match v {
        Val::Nil => lua_error!("attempt to index field '{}' (a nil value)", what),
        _ => v.get(k),
    }
}

/// The values of `ipairs(v)` (a list table; nil raises as `ipairs(nil)` does).
fn ipairs_vals(v: &Val, what: &str) -> Vec<Val> {
    match v {
        Val::Table(t) => t.ipairs().into_iter().map(|(_, x)| x).collect(),
        Val::Data(_) => {
            let mut out = Vec::new();
            let mut i = 1i64;
            loop {
                let x = v.get(i);
                if x.is_nil() {
                    break;
                }
                out.push(x);
                i += 1;
            }
            out
        }
        other => lua_error!(
            "bad argument #1 to 'ipairs' (table expected, got {}): {}",
            other.lua_type(),
            what
        ),
    }
}

/// The list `Table` `{ v, … }` of a node list.
fn node_list_table(nodes: &[NodeRef]) -> Table {
    let t = Table::new();
    for n in nodes {
        t.push(node_val(n));
    }
    t
}

/// `isValueInArray(list, node)` for a node list (identity).
fn node_index(list: &Shared<Vec<NodeRef>>, node: &NodeRef) -> Option<usize> {
    list.snapshot()
        .iter()
        .position(|n| Rc::ptr_eq(n, node))
        .map(|i| i + 1)
}

/// `isValueInArray(list, id)` for a number list.
fn num_index(list: &Shared<Vec<f64>>, v: f64) -> Option<usize> {
    list.snapshot().iter().position(|&x| x == v).map(|i| i + 1)
}

/// `"Tree:"..node.id` (string ids of legion / tattoo nodes, CCR-0024 §1).
fn tree_source(node: &Node) -> String {
    if node.id.is_nan() {
        match node.extra.get(n!("id")) {
            Val::Str(s) => format!("Tree:{s}"),
            Val::Num(x) => format!("Tree:{}", tostring_num(x)),
            other => lua_error!(
                "attempt to concatenate field 'id' (a {} value)",
                other.lua_type()
            ),
        }
    } else {
        format!("Tree:{}", tostring_num(node.id))
    }
}

/// `old.<k> = new.<k>` for an untyped node field: assigning nil to a spec node makes its
/// `__index` read fall back to the tree node's value (CCR-0052 §1).
fn assign_extra(old: &Node, k: Name, v: Val) {
    if v.is_nil() {
        let fallback = old.tree.as_ref().map_or(Val::Nil, |t| t.extra.get(k));
        old.extra.set(k, fallback);
    } else {
        old.extra.set(k, v);
    }
}

/// A cluster node table (`{ type = …, id = …, dn = …, sd = …, icon = …, group = …, o = …,
/// oidx = … }`, PassiveSpec.lua 2073-2086 and the other constructors of BuildSubgraph).
fn new_cluster_node(
    ty: Name,
    id: f64,
    dn: LStr,
    sd: Shared<Vec<LStr>>,
    icon: Val,
    group: &Table,
    o: f64,
    oidx: f64,
) -> Node {
    let node = new_node_table(id);
    node.r#type.set(ty);
    node.dn.set(dn);
    node.sd.set(sd);
    node.extra.set(n!("icon"), icon);
    node.group.set(Some(group.clone()));
    node.o.set(o);
    node.oidx.set(oidx);
    node
}

/// A list `Table` of strings as a node `sd` list (`copyTable(skill.stats)`).
fn str_list(v: &Val, what: &str) -> Vec<LStr> {
    ipairs_vals(v, what)
        .into_iter()
        .map(|x| match x {
            Val::Str(s) => s,
            other => lua_error!("{}: a {} value in a stat list", what, other.lua_type()),
        })
        .collect()
}

impl PassiveSpec {
    /// `self.nodes[id]`.
    fn psc_node_at(&self, id: f64) -> Option<NodeRef> {
        if id.is_nan() {
            return None;
        }
        self.nodes.with(|m| m.get(id as i64))
    }

    /// `self.legacyClusterNodeMap` (nil → `None`).
    fn psc_legacy_map(&self) -> Option<Rc<LegacyMap>> {
        self.extra.obj::<LegacyMap>(n!("legacyClusterNodeMap"))
    }

    /// The `pairs` mirror of `self.subGraphs`, re-created when the table no longer has the
    /// keys the mirror saw (Init made a fresh `self.subGraphs = { }`; CCR-0072 §4).
    fn psc_subgraph_keys(&self) -> Rc<SubGraphKeys> {
        let mirror = self.extra.obj::<SubGraphKeys>(n!("__subGraphs_keys"));
        let live: Vec<Key> = self.subGraphs.pairs().map(|(k, _)| k).collect();
        if let Some(m) = mirror {
            let mk = m.borrow().keys();
            let same = mk.len() == live.len()
                && live.iter().all(|k| match k {
                    Key::I(i) => mk.contains(i),
                    _ => false,
                });
            if same {
                return m;
            }
        }
        let mut fresh = LjIntMap::new();
        for k in live {
            if let Key::I(i) = k {
                fresh.set(i, Some(()));
            }
        }
        let m = Rc::new(RefCell::new(fresh));
        self.extra.set(
            n!("__subGraphs_keys"),
            Val::Obj(m.clone() as Rc<dyn std::any::Any>),
        );
        m
    }

    /// `self.subGraphs[id] = v` (keeping the `pairs` mirror in step).
    fn psc_set_subgraph(&self, id: f64, v: Option<Table>) {
        let keys = self.psc_subgraph_keys();
        keys.borrow_mut().set(id as i64, v.as_ref().map(|_| ()));
        match v {
            Some(t) => self.subGraphs.set(id, t),
            None => self.subGraphs.set(id, Val::Nil),
        }
    }

    /// `self.build` (a Weak back-pointer, calc doc §3.3).
    fn psc_build(&self) -> BuildRef {
        self.build
            .upgrade()
            .unwrap_or_else(|| lua_error!("attempt to index field 'build' (a nil value)"))
    }
}

// ---------------------------------------------------------------------------------------
// ReplaceNode, ReconnectNodeToClassStart
// ---------------------------------------------------------------------------------------

/// @pob Classes/PassiveSpec.lua:1697-1718 h=75755a02
impl PassiveSpec {
    /// → `true` for Lua's `return 1` (the node already has this `sd`), `false` for no value.
    pub fn ReplaceNode(&self, old: &NodeRef, newNode: &NodeRef) -> bool {
        // oracle-test seam (CCR-0052 §12); never set in the product
        if crate::classes::passive_spec::spec_stubs().replace_node {
            return false;
        }
        // Edited nodes can share a name
        // L1699 (`sd` tables compare by identity)
        if old.sd.get().ptr_eq(&newNode.sd.get()) {
            // L1700
            return true;
        }
        // L1702
        old.dn.set(newNode.dn.get());
        // L1703
        old.sd.set(newNode.sd.get());
        // L1704
        assign_extra(old, n!("name"), newNode.extra.get(n!("name")));
        // L1705
        old.mods.set(newNode.mods.get());
        // L1706
        old.modKey.set(newNode.modKey.get());
        // L1707
        old.modList.set(ModStore::ModList(None));
        // L1708
        old.modList.get().AddList(&newNode.modList.get().ipairs());
        // L1709
        assign_extra(old, n!("sprites"), newNode.extra.get(n!("sprites")));
        // L1710
        assign_extra(
            old,
            n!("effectSprites"),
            newNode.extra.get(n!("effectSprites")),
        );
        // L1711 (nil restores the tree node's value; tree nodes are never tattoos)
        old.isTattoo.set(
            newNode.isTattoo.get() || old.tree.as_ref().is_some_and(|t| t.isTattoo.get()),
        );
        // L1712
        old.overrideType.set(
            newNode
                .overrideType
                .get()
                .or_else(|| old.tree.as_ref().and_then(|t| t.overrideType.get())),
        );
        // L1713
        old.keystoneMod.set(
            newNode
                .keystoneMod
                .get()
                .or_else(|| old.tree.as_ref().and_then(|t| t.keystoneMod.get())),
        );
        // L1714
        assign_extra(old, n!("icon"), newNode.extra.get(n!("icon")));
        // L1715
        assign_extra(old, n!("spriteId"), newNode.extra.get(n!("spriteId")));
        // L1716
        assign_extra(
            old,
            n!("activeEffectImage"),
            newNode.extra.get(n!("activeEffectImage")),
        );
        // L1717 (`wipeTable(old.reminderText)`: the `__index` read may be the tree node's
        // own list, which the spec node's `extra` aliases; wiped in place, as in Lua)
        let reminderText = lor(newNode.extra.get(n!("reminderText")), || {
            match old.extra.get(n!("reminderText")) {
                Val::Table(t) => {
                    t.wipe();
                    Val::Table(t)
                }
                Val::Nil | Val::Bool(false) => Val::Table(Table::new()),
                other => lua_error!(
                    "bad argument #1 to 'for iterator' (table expected, got {})",
                    other.lua_type()
                ),
            }
        });
        old.extra.set(n!("reminderText"), reminderText);
        false
    }
}

/// @pob Classes/PassiveSpec.lua:1722-1730 h=63ec140a
impl PassiveSpec {
    ///Reconnects altered timeless jewel to class start, for Pure Talent
    ///@param node table @ The node to add the Condition:ConnectedTo[Class] flag to, if applicable
    pub fn ReconnectNodeToClassStart(&self, node: &NodeRef) {
        let tree = self.tree.get();
        // L1723
        for_ipairs!((_, linkedNodeId) in node.linkedId.get() => {
            // L1724 (one class per start node: the order of the NewMod calls cannot differ)
            for (_, class) in tree.classes.pairs() {
                // L1725
                if num_val(linkedNodeId).raw_eq(&class.get(n!("startNodeId")))
                    && node.r#type.get() == n!("Normal")
                {
                    // L1726
                    let name = match class.get(n!("name")) {
                        Val::Str(s) => s,
                        other => lua_error!(
                            "attempt to concatenate field 'name' (a {} value)",
                            other.lua_type()
                        ),
                    };
                    node.modList.get().NewMod(
                        Name::from(format!("Condition:ConnectedTo{name}Start").as_str()),
                        ModType::FLAG,
                        true,
                        &format!("Tree:{}", tostring_num(linkedNodeId)),
                        0,
                        0,
                        &[],
                    );
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------------------
// Legacy (v1) cluster hash conversion
// ---------------------------------------------------------------------------------------

/// @pob Classes/PassiveSpec.lua:1734-1739 h=3b29a79d
impl PassiveSpec {
    /// Frozen signature: the Lua returns a boolean, see
    /// [`PassiveSpec::BeginLegacyClusterHashConversion_ret`] (CCR-0072 §1).
    pub fn BeginLegacyClusterHashConversion(&self) {
        // CCR-0072
        let _ = self.BeginLegacyClusterHashConversion_ret();
    }

    // Initializes temporary lookup tables used when loading legacy (v1) cluster hashes.
    // Returns true when legacy conversion is active for this graph rebuild.
    pub fn BeginLegacyClusterHashConversion_ret(&self) -> bool {
        // L1735
        let needsLegacyClusterHashConversion =
            self.extra.num_or(n!("clusterHashFormatVersion"), 2.0) < 2.0;
        // L1736
        if needsLegacyClusterHashConversion {
            let m: Rc<LegacyMap> = Rc::new(RefCell::new(LjIntMap::new()));
            self.extra.set(
                n!("legacyClusterNodeMap"),
                Val::Obj(m as Rc<dyn std::any::Any>),
            );
        } else {
            self.extra.set(n!("legacyClusterNodeMap"), Val::Nil);
        }
        // L1737
        self.extra.set(
            n!("legacyClusterNodeMapReverse"),
            if needsLegacyClusterHashConversion {
                Val::Table(Table::new())
            } else {
                Val::Nil
            },
        );
        // L1738
        needsLegacyClusterHashConversion
    }
}

/// @pob Classes/PassiveSpec.lua:1743-1751 h=ecad0e93
impl PassiveSpec {
    // Legacy conversion updates node IDs while rebuilding cluster subgraphs.
    // This helper keeps forward and reverse mappings in sync.
    pub fn RegisterLegacyClusterNodeMap(&self, legacyNodeId: f64, currentNodeId: f64) {
        // L1744
        let Some(map) = self.psc_legacy_map() else {
            // L1745
            return;
        };
        if legacyNodeId.is_nan() || currentNodeId.is_nan() {
            // L1745
            return;
        }
        // L1747
        map.borrow_mut()
            .set(legacyNodeId as i64, Some(currentNodeId));
        // L1748
        if let Some(rev) = self.extra.table(n!("legacyClusterNodeMapReverse")) {
            // L1749
            rev.set(currentNodeId, legacyNodeId);
        }
    }
}

/// @pob Classes/PassiveSpec.lua:1754-1760 h=57acfb06
impl PassiveSpec {
    // Returns the remapped node ID when a valid legacy -> current cluster mapping exists.
    pub fn GetMappedClusterNodeId(&self, nodeId: f64) -> f64 {
        // L1755
        let mappedNodeId = self.psc_legacy_map().and_then(|m| {
            if nodeId.is_nan() {
                None
            } else {
                m.borrow().get(nodeId as i64)
            }
        });
        // L1756
        if let Some(mappedNodeId) = mappedNodeId {
            if self.psc_node_at(mappedNodeId).is_some() {
                // L1757
                return mappedNodeId;
            }
        }
        // L1759
        nodeId
    }
}

/// @pob Classes/PassiveSpec.lua:1763-1797 h=67976b4c
impl PassiveSpec {
    // Applies legacy -> current remapping to both cluster allocations and socketed jewel ownership.
    pub fn ApplyLegacyClusterNodeRemap(&self) {
        // L1764
        let Some(map) = self.psc_legacy_map() else {
            // L1765
            return;
        };

        // L1768
        let mut convertedNodeIds: Vec<f64> = Vec::new();
        // L1769
        let mut seenNodeIds: LjIntMap<bool> = LjIntMap::new();
        // L1770
        for_ipairs!((_, nodeId) in self.allocSubgraphNodes.clone() => {
            // L1771
            let nodeId = self.GetMappedClusterNodeId(nodeId);
            // L1772
            if !seenNodeIds.contains_key(nodeId as i64) {
                // L1773
                seenNodeIds.set(nodeId as i64, Some(true));
                // L1774
                convertedNodeIds.push(nodeId);
            }
        });
        // L1777 (`self.allocSubgraphNodes = convertedNodeIds`)
        self.allocSubgraphNodes
            .with_mut(|v| *v = convertedNodeIds);

        // Legacy cluster socket IDs can be normal tree node IDs (< 65536), so they bypass allocSubgraphNodes.
        // Move any such allocations onto their mapped current cluster node IDs.
        // L1781 (LUA-ORDER: the map's LuaJIT order)
        let entries = map.borrow().pairs();
        for (legacyNodeId, currentNodeId) in entries {
            // L1782
            let legacyAlloc = self.allocNodes.with(|m| m.get(legacyNodeId));
            if legacyNodeId as f64 != currentNodeId
                && legacyAlloc.is_some()
                && self.psc_node_at(currentNodeId).is_some()
            {
                // L1783
                legacyAlloc.expect("allocated").alloc.set(false);
                // L1784
                self.allocNodes.with_mut(|m| m.set(legacyNodeId, None));
                // L1785
                if !seenNodeIds.contains_key(currentNodeId as i64) {
                    // L1786
                    seenNodeIds.set(currentNodeId as i64, Some(true));
                    // L1787
                    self.allocSubgraphNodes.push(currentNodeId);
                }
            }
        }

        // L1792
        let mut convertedJewels: LjIntMap<f64> = LjIntMap::new();
        // L1793
        for (nodeId, itemId) in self.jewels.with(LjIntMap::pairs) {
            // L1794
            let k = self.GetMappedClusterNodeId(nodeId as f64);
            convertedJewels.set(k as i64, Some(itemId));
        }
        // L1796
        self.jewels.with_mut(|m| *m = convertedJewels);
    }
}

/// @pob Classes/PassiveSpec.lua:1800-1804 h=e335efee
impl PassiveSpec {
    // Finalizes cluster hash conversion state after each graph rebuild.
    pub fn EndLegacyClusterHashConversion(&self) {
        // L1801
        self.extra.set(n!("clusterHashFormatVersion"), 2.0);
        // L1802
        self.extra.set(n!("legacyClusterNodeMap"), Val::Nil);
        // L1803
        self.extra.set(n!("legacyClusterNodeMapReverse"), Val::Nil);
    }
}

// ---------------------------------------------------------------------------------------
// Cluster jewel graphs
// ---------------------------------------------------------------------------------------

/// @pob Classes/PassiveSpec.lua:1806-1880 h=309d9367
impl PassiveSpec {
    pub fn BuildClusterJewelGraphs(&self) {
        // oracle-test seam (CCR-0052 §12); never set in the product
        if crate::classes::passive_spec::spec_stubs().build_cluster_jewel_graphs {
            return;
        }
        // L1807 (CCR-0072 §1)
        let needsLegacyClusterHashConversion = self.BeginLegacyClusterHashConversion_ret();

        // Mark that path building should clear out stale references to cluster nodes
        // L1810
        self.extra.set(n!("checkNodeLinks"), true);
        // Remove old subgraphs
        // L1812 (LUA-ORDER: `pairs(self.subGraphs)` through the key mirror, CCR-0072 §4)
        let keys = self.psc_subgraph_keys();
        let order = keys.borrow().keys();
        // CCR-0072 §8: the removed subgraph nodes, whose `Rc` cycles are broken below
        let mut removed: Vec<NodeRef> = Vec::new();
        for id in order {
            let Some(subGraph) = self.subGraphs.table(id) else {
                continue;
            };
            // L1813
            for (_, node) in subGraph.get(n!("nodes")).table().map_or_else(Vec::new, |t| t.ipairs()) {
                let node = val_node(&node).expect("subgraph node");
                removed.push(node.clone());
                // L1814 (cluster nodes always carry a number id)
                if !node.id.is_nan() {
                    // L1815
                    self.nodes.with_mut(|m| m.set(node.id as i64, None));
                    // L1816
                    if self.allocNodes.with(|m| m.contains_key(node.id as i64)) {
                        // Reserve the allocation in case the node is regenerated
                        // L1818
                        self.allocNodes.with_mut(|m| m.set(node.id as i64, None));
                        // L1819
                        if !self.extra.truthy(n!("ignoreAllocatingSubgraph")) {
                            // do not carry over alloc nodes, e.g. cluster jewels on Import when Delete Jewel is true
                            // L1820
                            self.allocSubgraphNodes.push(node.id);
                        }
                    }
                }
            }
            // L1825
            let parentSocket = val_node(&subGraph.get(n!("parentSocket")))
                .unwrap_or_else(|| lua_error!("attempt to index field 'parentSocket' (a nil value)"));
            let entranceNode = val_node(&subGraph.get(n!("entranceNode")));
            let index = entranceNode
                .as_ref()
                .and_then(|e| node_index(&parentSocket.linked.get(), e));
            // L1826
            let Some(index) = index else {
                lua_error!("Entrance for subGraph not linked to parent socket???")
            };
            // L1827
            parentSocket.linked.get().remove(Some(index));
        }
        // L1829
        self.subGraphs.wipe();
        keys.borrow_mut().wipe();
        // CCR-0072 §8: Lua's GC collects the old subgraph nodes once unreachable; their
        // `linked` rings (and PS-B's `depends`/`path`) are `Rc` cycles here. Each list is
        // replaced, never cleared in place. A node the rebuild regenerates is a new node.
        for node in removed {
            node.linked.set(Shared::new(Vec::new()));
            node.depends.set(Shared::new(Vec::new()));
            node.path.set(None);
        }
        // L1830
        self.extra.set(n!("ignoreAllocatingSubgraph"), false); // reset after subGraph logic

        // L1832
        let importedGroups = Table::new();
        // L1833
        let importedNodes = Table::new();
        // L1834
        let jewel_data = self.extra.get(n!("jewel_data"));
        if jewel_data.truthy() {
            let jewel_data = jewel_data.table().unwrap_or_else(|| {
                lua_error!("bad argument #1 to 'for iterator' (table expected, got {})", jewel_data.lua_type())
            });
            // L1835 (LUA-ORDER: integer keys; a later jewel's group / node of the same id wins)
            for (_, value) in jewel_data.pairs() {
                // L1836
                let subgraph = value.get(n!("subgraph"));
                if subgraph.truthy() {
                    // L1837
                    let groups = index(&subgraph, n!("groups"), "groups");
                    let groups = groups.table().unwrap_or_else(|| {
                        lua_error!("bad argument #1 to 'for iterator' (table expected, got {})", groups.lua_type())
                    });
                    for (groupId, groupData) in groups.pairs() {
                        // L1838
                        importedGroups.set(groupId, groupData);
                    }
                    // L1840
                    let nodes = index(&subgraph, n!("nodes"), "nodes");
                    let nodes = nodes.table().unwrap_or_else(|| {
                        lua_error!("bad argument #1 to 'for iterator' (table expected, got {})", nodes.lua_type())
                    });
                    for (nodeId, nodeValue) in nodes.pairs() {
                        // L1841
                        importedNodes.set(nodeId, nodeValue);
                    }
                }
            }
        }
        let tree = self.tree.get();
        // L1846 (LUA-ORDER: `tree.sockets` is the tree's LjIntMap)
        for (nodeId, _) in tree.sockets.pairs() {
            // L1847
            let node = tree.nodes.get(nodeId);
            // L1848
            let (jewel, _) = self.GetSocketedJewel(nodeId as f64);
            // L1849
            let Some(node) = node else { continue };
            if !self.allocNodes.with(|m| m.contains_key(node.id as i64)) {
                continue;
            }
            let Some(expansionJewel) = node.expansionJewel.get() else {
                continue;
            };
            if !num_val(2.0).raw_eq(&expansionJewel.get(n!("size"))) {
                continue;
            }
            let Some(jewel) = jewel else { continue };
            let jewelData = jewel.jewelData.get().expect("GetJewel returns jewels with jewelData");
            if jewelData.truthy(n!("clusterJewelValid")) {
                // This is a Large Jewel Socket, and it has a cluster jewel in it
                // L1851
                let parentSocket = self.psc_node_at(nodeId as f64).unwrap_or_else(|| {
                    lua_error!("attempt to index local 'parentSocket' (a nil value)")
                });
                self.BuildSubgraph(
                    &jewel,
                    &parentSocket,
                    None,
                    None,
                    Some(&importedNodes),
                    Some(&importedGroups),
                );
            }
        }

        // L1855
        if needsLegacyClusterHashConversion {
            // L1856
            self.ApplyLegacyClusterNodeRemap();
        }

        // (Re-)allocate subgraph nodes
        // L1860
        for_ipairs!((_, nodeId) in self.allocSubgraphNodes.clone() => {
            // L1861
            let node = self.psc_node_at(nodeId);
            // L1862
            if let Some(node) = node {
                // L1863
                node.alloc.set(true);
                // L1864
                if !self.allocNodes.with(|m| m.contains_key(nodeId as i64)) {
                    // L1865
                    self.allocNodes.with_mut(|m| m.set(nodeId as i64, Some(node.clone())));
                    // L1866
                    if num_index(&self.allocExtendedNodes, nodeId).is_none() {
                        // L1867
                        self.allocExtendedNodes.push(nodeId);
                    }
                }
            }
        });
        // L1872
        self.allocSubgraphNodes.wipe();

        // Rebuild paths to account for new/removed nodes
        // L1875
        self.BuildAllDependsAndPaths();

        // Rebuild node search cache because the tree might have changed
        // L1878: self.build.treeTab.viewer.searchStrCached = "" — the tree viewer's search
        // cache (UI, PassiveTreeView; `TreeTab` has no viewer in the model, CCR-0072 §6)
        // L1879
        self.EndLegacyClusterHashConversion();
    }
}

/// @pob Classes/PassiveSpec.lua:1883-1890 h=3accf14a
impl PassiveSpec {
    // Finds a specific expansion socket entry within a passive-tree group.
    pub fn FindClusterSocket(&self, group: &Table, index: f64) -> Option<NodeRef> {
        let tree = self.tree.get();
        // L1884
        for nodeId in ipairs_vals(&group.get(n!("n")), "group.n") {
            // L1885
            let node = tonumber_val(&nodeId)
                .filter(|x| !x.is_nan())
                .and_then(|x| tree.nodes.get(x as i64))
                .unwrap_or_else(|| {
                    lua_error!("attempt to index local 'node' (a nil value)")
                });
            // L1886
            if let Some(ej) = node.expansionJewel.get() {
                if num_val(index).raw_eq(&ej.get(n!("index"))) {
                    // L1887
                    return Some(node);
                }
            }
        }
        None
    }
}

/// @pob Classes/PassiveSpec.lua:1894-1912 h=1d6b0614
impl PassiveSpec {
    // Legacy parser behavior downsized the proxy group while descending into nested clusters.
    // Reproducing that traversal lets us recover legacy socket IDs for migration.
    pub fn BuildLegacyProxyGroup(
        &self,
        proxyGroup: &Table,
        expansionJewelSize: f64,
        clusterSizeIndex: f64,
    ) -> Table {
        let tree = self.tree.get();
        // L1895
        let mut legacyGroup = proxyGroup.clone();
        // L1896
        let mut groupSize = expansionJewelSize;
        // L1897
        let mut guard = 0.0;
        // L1898
        while clusterSizeIndex < groupSize && guard < 4.0 {
            // L1899
            let socket = self
                .FindClusterSocket(&legacyGroup, 1.0)
                .or_else(|| self.FindClusterSocket(&legacyGroup, 0.0));
            // L1900
            let Some(socket) = socket else {
                // L1901
                break;
            };
            let ej = socket.expansionJewel.get().expect("socket has expansionJewel");
            // L1903
            let legacyProxyNode = tonumber_val(&ej.get(n!("proxy")))
                .filter(|x| !x.is_nan())
                .and_then(|x| tree.nodes.get(x as i64));
            // L1904
            let Some(legacyProxyNode) = legacyProxyNode else {
                // L1905
                break;
            };
            let Some(group) = legacyProxyNode.group.get() else {
                // L1905
                break;
            };
            // L1907
            legacyGroup = group;
            // L1908
            groupSize = ej.num(n!("size"));
            // L1909
            guard += 1.0;
        }
        // L1911
        legacyGroup
    }
}

/// `({[0] = …, …})[srcOidx]` (nil → `NaN`).
fn orbit_lookup(t: &[f64], srcOidx: f64) -> f64 {
    if srcOidx >= 0.0 && srcOidx == srcOidx.trunc() && (srcOidx as usize) < t.len() {
        t[srcOidx as usize]
    } else {
        f64::NAN
    }
}

/// @pob Classes/PassiveSpec.lua:1916-1933 h=635f363d
impl PassiveSpec {
    // Converts cluster orbit indices between different node-count spaces.
    // 12<->16 mappings reflect the 3.17 cluster export change; 6<->16 supports legacy nested mapping.
    /// `NaN` is Lua's nil (an index outside the mapping table).
    pub fn TranslateClusterOrbitIndex(
        &self,
        srcOidx: f64,
        srcNodesPerOrbit: f64,
        destNodesPerOrbit: f64,
    ) -> f64 {
        // L1917
        if srcNodesPerOrbit == destNodesPerOrbit {
            // L1918
            srcOidx
        } else if srcNodesPerOrbit == 12.0 && destNodesPerOrbit == 16.0 {
            // L1920
            orbit_lookup(
                &[0., 1., 3., 4., 5., 7., 8., 9., 11., 12., 13., 15.],
                srcOidx,
            )
        } else if srcNodesPerOrbit == 16.0 && destNodesPerOrbit == 12.0 {
            // L1922
            orbit_lookup(
                &[
                    0., 1., 1., 2., 3., 4., 4., 5., 6., 7., 7., 8., 9., 10., 10., 11.,
                ],
                srcOidx,
            )
        } else if srcNodesPerOrbit == 6.0 && destNodesPerOrbit == 16.0 {
            // L1924
            orbit_lookup(&[0., 3., 5., 8., 11., 13.], srcOidx)
        } else if srcNodesPerOrbit == 16.0 && destNodesPerOrbit == 6.0 {
            // L1926
            orbit_lookup(
                &[
                    0., 0., 0., 1., 1., 2., 2., 2., 3., 3., 3., 4., 4., 5., 5., 5.,
                ],
                srcOidx,
            )
        } else {
            // there is no known case where this should happen...
            // L1929
            ShowErrMsg(&lfmt!(
                "^1Error: unexpected cluster jewel node counts %d -> %d",
                srcNodesPerOrbit,
                destNodesPerOrbit
            ));
            // ...but if a future patch adds one, this should end up only a little krangled, close enough for initial skill data imports:
            // L1931
            m_floor(srcOidx * destNodesPerOrbit / srcNodesPerOrbit)
        }
    }
}

/// @pob Classes/PassiveSpec.lua:1936-1941 h=a694ce1f
impl PassiveSpec {
    /// Frozen signature on a `Table` (`{ [nodeIndex] = node }`, nodes as `Val::Obj`); the
    /// result is `indicies` itself (Lua returns nothing). BuildSubgraph uses
    /// [`PassiveSpec::ApplyClusterOrbitIndexAdjustment_lj`] (CCR-0072 §2).
    pub fn ApplyClusterOrbitIndexAdjustment(
        &self,
        indicies: &Table,
        startOidx: f64,
        clusterTotalIndicies: f64,
        skillsPerOrbit: f64,
    ) -> Table {
        // CCR-0072
        let lj = table_indicies(indicies);
        self.ApplyClusterOrbitIndexAdjustment_lj(&lj, startOidx, clusterTotalIndicies, skillsPerOrbit);
        indicies.clone()
    }

    // Applies proxy orbit offsets and converts from cluster template indices into tree orbit-space indices.
    pub fn ApplyClusterOrbitIndexAdjustment_lj(
        &self,
        indicies: &LjIntMap<NodeRef>,
        startOidx: f64,
        clusterTotalIndicies: f64,
        skillsPerOrbit: f64,
    ) {
        // L1937
        for (_, node) in indicies.pairs() {
            // L1938
            let correctedNodeOidxRelativeToClusterIndicies =
                lmod(node.oidx.get() + startOidx, clusterTotalIndicies);
            // L1939
            node.oidx.set(self.TranslateClusterOrbitIndex(
                correctedNodeOidxRelativeToClusterIndicies,
                clusterTotalIndicies,
                skillsPerOrbit,
            ));
        }
    }
}

/// A `{ [nodeIndex] = node }` `Table` as the LjIntMap BuildSubgraph builds (its `pairs`
/// order inserted in order; CCR-0072 §2).
fn table_indicies(t: &Table) -> LjIntMap<NodeRef> {
    let mut out = LjIntMap::new();
    for (k, v) in t.pairs() {
        let Key::I(i) = k else {
            lua_error!("cluster orbit indices: a non-integer key")
        };
        out.set(
            i,
            Some(val_node(&v).unwrap_or_else(|| lua_error!("cluster orbit indices: a non-node value"))),
        );
    }
    out
}

/// @pob Classes/PassiveSpec.lua:1944-1968 h=61d1b75c
impl PassiveSpec {
    /// Frozen signature on a `Table`; the result is `indicies` itself (Lua returns nothing).
    /// BuildSubgraph uses [`PassiveSpec::BuildLegacyClusterOrbitMappings_lj`] (CCR-0072 §2).
    pub fn BuildLegacyClusterOrbitMappings(
        &self,
        indicies: &Table,
        proxyNode: &NodeRef,
        clusterTotalIndicies: f64,
        skillsPerOrbit: f64,
    ) -> Table {
        // CCR-0072
        let lj = table_indicies(indicies);
        self.BuildLegacyClusterOrbitMappings_lj(&lj, proxyNode, clusterTotalIndicies, skillsPerOrbit);
        indicies.clone()
    }

    // Builds additional legacy node mappings by matching equivalent nodes in legacy and current orbit spaces.
    pub fn BuildLegacyClusterOrbitMappings_lj(
        &self,
        indicies: &LjIntMap<NodeRef>,
        proxyNode: &NodeRef,
        clusterTotalIndicies: f64,
        skillsPerOrbit: f64,
    ) {
        // L1945
        if self.psc_legacy_map().is_none() {
            // L1946
            return;
        }

        let tree = self.tree.get();
        // L1949 (`skillsPerOrbit[o + 1]`; a missing entry is nil)
        let o = proxyNode.o.get();
        let legacySkillsPerOrbit = if o >= 0.0 && o == o.trunc() {
            tree.skillsPerOrbit.get(o as usize).copied().unwrap_or(f64::NAN)
        } else {
            f64::NAN
        };
        // L1950
        let legacyProxyNodeOidxRelativeToClusterIndicies = self.TranslateClusterOrbitIndex(
            proxyNode.oidx.get(),
            legacySkillsPerOrbit,
            clusterTotalIndicies,
        );
        // L1951
        let mut legacyNodeIdsByOidx: LjIntMap<f64> = LjIntMap::new();
        // L1952
        let mut currentNodeIdsByOidx: LjIntMap<f64> = LjIntMap::new();
        // L1953 (LUA-ORDER: colliding oidx keys keep the last node in LuaJIT's order)
        for (nodeIndex, node) in indicies.pairs() {
            // L1954
            let legacyNodeOidxRelativeToClusterIndicies = lmod(
                nodeIndex as f64 + legacyProxyNodeOidxRelativeToClusterIndicies,
                clusterTotalIndicies,
            );
            // L1955
            let legacyNodeOidx = self.TranslateClusterOrbitIndex(
                legacyNodeOidxRelativeToClusterIndicies,
                clusterTotalIndicies,
                legacySkillsPerOrbit,
            );
            // L1956
            lj_set_num_key(&mut legacyNodeIdsByOidx, legacyNodeOidx, node.id);

            // L1958
            let currentNodeOidxRelativeToClusterIndicies =
                self.TranslateClusterOrbitIndex(node.oidx.get(), skillsPerOrbit, clusterTotalIndicies);
            // L1959
            let currentNodeOidxInLegacySkillsPerOrbit = self.TranslateClusterOrbitIndex(
                currentNodeOidxRelativeToClusterIndicies,
                clusterTotalIndicies,
                legacySkillsPerOrbit,
            );
            // L1960
            lj_set_num_key(
                &mut currentNodeIdsByOidx,
                currentNodeOidxInLegacySkillsPerOrbit,
                node.id,
            );
        }
        // L1962 (LUA-ORDER: the registration order is the map's insertion order)
        for (oidx, legacyNodeId) in legacyNodeIdsByOidx.pairs() {
            // L1963
            let currentNodeId = currentNodeIdsByOidx.get(oidx);
            // L1964
            if let Some(currentNodeId) = currentNodeId {
                if legacyNodeId != currentNodeId {
                    // L1965
                    self.RegisterLegacyClusterNodeMap(legacyNodeId, currentNodeId);
                }
            }
        }
    }
}

/// `t[k] = v` for a number key that may be nil (`NaN`: "table index is nil") or
/// non-integral (none in practice: every translation returns an integer or nil).
fn lj_set_num_key(t: &mut LjIntMap<f64>, k: f64, v: f64) {
    if k.is_nan() {
        lua_error!("table index is nil");
    }
    if k != k.trunc() || !k.is_finite() {
        lua_error!(
            "cluster orbit index {} is not an integer (unsupported table key)",
            tostring_num(k)
        );
    }
    t.set(k as i64, Some(v));
}

/// @pob Classes/PassiveSpec.lua:1970-2359 h=3c5172e7
impl PassiveSpec {
    pub fn BuildSubgraph(
        &self,
        jewel: &ItemRef,
        parentSocket: &NodeRef,
        id: Option<f64>,
        upSize: Option<f64>,
        importedNodes: Option<&Table>,
        importedGroups: Option<&Table>,
    ) {
        let tree = self.tree.get();
        let d = data();
        // L1971
        let expansionJewel = parentSocket.expansionJewel.get().unwrap_or_else(|| {
            lua_error!("attempt to index local 'expansionJewel' (a nil value)")
        });
        // L1972
        let clusterJewel = jewel.clusterJewel.get().unwrap_or_else(|| {
            lua_error!("attempt to index local 'clusterJewel' (a nil value)")
        });
        // L1973
        let jewelData = jewel.jewelData.get().unwrap_or_else(|| {
            lua_error!("attempt to index local 'jewelData' (a nil value)")
        });

        // L1975
        let subGraph = Table::new();
        let subGraphNodes = Table::new();
        subGraph.set(n!("nodes"), subGraphNodes.clone());
        let subGraphGroup = Table::new();
        subGraphGroup.set(n!("oo"), Table::new());
        subGraph.set(n!("group"), subGraphGroup.clone());
        let subGraphConnectors = Table::new();
        subGraph.set(n!("connectors"), subGraphConnectors.clone());
        subGraph.set(n!("parentSocket"), node_val(parentSocket));

        // Make id for this subgraph (and nodes)
        // 0-3: Node index (0-11)
        // 4-5: Group size (0-2)
        // 6-8: Large index (0-5)
        // 9-10: Medium index (0-2)
        // 11-15: Unused
        // 16: 1 (signal bit, to prevent conflict with node hashes)
        // L1988
        let mut id = id.unwrap_or(f64::from(0x10000));
        let b_lshift = |x: f64, n: u32| -> f64 { f64::from(bit::lshift(x as i64 as u32, n) as i32) };
        // L1989
        let ejSize = expansionJewel.get(n!("size"));
        if num_val(2.0).raw_eq(&ejSize) {
            // L1990
            id += b_lshift(expansionJewel.num(n!("index")), 6);
        } else if num_val(1.0).raw_eq(&ejSize) {
            // L1992
            id += b_lshift(expansionJewel.num(n!("index")), 9);
        }
        // L1994
        let nodeId = id + b_lshift(clusterJewel.num(n!("sizeIndex")), 4);

        // L1996
        self.psc_set_subgraph(nodeId, Some(subGraph.clone()));

        // Locate the proxy group
        // L1999
        let proxyNode = tonumber_val(&expansionJewel.get(n!("proxy")))
            .filter(|x| !x.is_nan())
            .and_then(|x| tree.nodes.get(x as i64));
        // L2000
        let Some(proxyNode) = proxyNode else {
            lua_error!("Proxy node not found")
        };
        // L2001
        let proxyGroup = proxyNode.group.get();
        let proxyGroupT = || {
            proxyGroup
                .clone()
                .unwrap_or_else(|| lua_error!("attempt to index local 'proxyGroup' (a nil value)"))
        };

        // Actually, let's not, since the game doesn't handle this :D
        //	if upSize and upSize > 0 then
        //		-- We need to move inwards to account for the parent group being downsized
        //		-- So we position according to the parent's original group position
        //		assert(upSize == 1) -- Only handling 1 upsize, which is the most that is possible
        //		local parentGroup = self.tree.nodes[parentSocket.id].group
        //		subGraph.group.x = parentGroup.x
        //		subGraph.group.y = parentGroup.y
        //	else
        // Position the group using the original proxy's position
        // L2013
        subGraphGroup.set(n!("x"), proxyGroupT().get(n!("x")));
        // L2014
        subGraphGroup.set(n!("y"), proxyGroupT().get(n!("y")));
        //	end

        // L2017
        let linkNodes = |node1: &NodeRef, node2: &NodeRef| {
            // L2018
            node1.linked.get().push(node2.clone());
            // L2019
            node2.linked.get().push(node1.clone());
            // BuildConnector returns a table of objects, not a single object now
            // L2021-2025: `self.tree:BuildConnector(node1, node2)` is the UI's (connector
            // polygons, U23 tree_view/connectors.rs); the calc records the linked pair in
            // its place, in the same order (CCR-0072 §3)
            subGraphConnectors.push(node_list_table(&[node1.clone(), node2.clone()]));
        };

        // L2028
        let matchGroup = |proxyId: &Val| -> Val {
            let importedGroups = importedGroups.unwrap_or_else(|| {
                lua_error!("bad argument #1 to 'for iterator' (table expected, got nil)")
            });
            // L2029 (LUA-ORDER: string keys of the account JSON; the first match wins)
            for (groupId, groupData) in importedGroups.pairs() {
                // L2030
                if groupData.get(n!("proxy")).raw_eq(proxyId) {
                    // L2031
                    return groupId.to_val();
                }
            }
            Val::Nil
        };

        // L2036
        let inExtendedHashes = |nodeId: &Val| -> bool {
            // L2037
            for exID in ipairs_vals(&self.extra.get(n!("extended_hashes")), "extended_hashes") {
                // L2038
                if nodeId.raw_eq(&exID) {
                    // L2039
                    return true;
                }
            }
            // L2042
            false
        };

        // L2045
        let addToAllocatedSubgraphNodes = |node: &NodeRef| -> bool {
            // Don't add to allocSubgraphNodes if node already exists
            // L2047
            if num_index(&self.allocSubgraphNodes, node.id).is_some() {
                // L2048
                return false;
            }
            // L2050
            let proxyGroup = matchGroup(&expansionJewel.get(n!("proxy")));
            // L2051
            if proxyGroup.truthy() {
                let importedNodes = importedNodes.unwrap_or_else(|| {
                    lua_error!("bad argument #1 to 'for iterator' (table expected, got nil)")
                });
                let importedGroups = importedGroups.expect("matchGroup read it");
                // L2052 (LUA-ORDER: string keys; the first matching node decides)
                for (id, data) in importedNodes.pairs() {
                    // L2053
                    if proxyGroup.raw_eq(&data.get(n!("group"))) {
                        // L2054
                        let matches = (node.r#type.get() == n!("Keystone")
                            && data.get(n!("isKeystone")).truthy())
                            || (num_val(node.oidx.get()).raw_eq(&data.get(n!("orbitIndex")))
                                && !data.get(n!("isMastery")).truthy());
                        // L2056
                        if matches {
                            // L2057
                            let group = importedGroups.get(match &proxyGroup {
                                Val::Str(s) => Key::from(&**s),
                                Val::Num(x) => Key::from(*x),
                                other => lua_error!("table index is a {}", other.lua_type()),
                            });
                            let groupNodes = index(&group, n!("nodes"), "nodes");
                            let idv = id.to_val();
                            for extendedId in ipairs_vals(&groupNodes, "nodes") {
                                // L2058
                                if idv.raw_eq(&extendedId)
                                    && inExtendedHashes(&tonumber_val(&idv).map_or(Val::Nil, Val::Num))
                                {
                                    // L2059
                                    return true;
                                }
                            }
                            // L2062
                            return false;
                        }
                    }
                }
            }
            // L2067
            false
        };

        // L2070
        if jewelData.truthy(n!("clusterJewelKeystone")) {
            // Special handling for keystones
            // L2072
            let ksName = jewelData.get(n!("clusterJewelKeystone"));
            let keystoneNode = match &ksName {
                Val::Str(s) => tree.clusterNodeMap.get(&**s),
                _ => Val::Nil,
            };
            let keystoneNode = val_node(&keystoneNode);
            // L2073
            let Some(keystoneNode) = keystoneNode else {
                lua_error!(
                    "Keystone node not found: {}",
                    ksName.str().map_or_else(String::new, |s| s.to_string())
                )
            };

            // Construct the new node
            // L2076
            let node = new_cluster_node(
                n!("Keystone"),
                nodeId,
                keystoneNode.dn.get(),
                keystoneNode.sd.get(),
                keystoneNode.extra.get(n!("icon")),
                &subGraphGroup,
                0.0,
                1.0,
            );
            node.extra.set(n!("expansionSkill"), true);
            node.linked.set(Shared::new(Vec::new()));
            node.extra.set(n!("power"), Table::new());
            let node: NodeRef = Rc::new(node);
            // L2089
            subGraphNodes.push(node_val(&node));

            // Process and add it
            // L2092
            tree.ProcessNode(&node);
            // L2093
            linkNodes(&node, parentSocket);
            // L2094
            subGraph.set(n!("entranceNode"), node_val(&node));
            // L2095
            self.nodes.with_mut(|m| m.set(node.id as i64, Some(node.clone())));
            // L2096
            if addToAllocatedSubgraphNodes(&node) {
                // L2097
                self.allocSubgraphNodes.push(node.id);
            }
            // L2099
            return;
        }

        // L2102
        let mut legacyProxyGroup: Option<Table> = None;
        // L2103
        if self.psc_legacy_map().is_some() {
            // L2104
            legacyProxyGroup = Some(self.BuildLegacyProxyGroup(
                &proxyGroupT(),
                expansionJewel.num(n!("size")),
                clusterJewel.num(n!("sizeIndex")),
            ));
        }

        // Initialise orbit flags
        // L2108
        let nodeOrbit = clusterJewel.num(n!("sizeIndex")) + 1.0;
        // L2109
        subGraphGroup
            .table(n!("oo"))
            .expect("oo")
            .set(nodeOrbit, true);

        // Process list of notables
        // L2112
        let mut notableList: Vec<NodeRef> = Vec::new();
        // L2113
        let buildData: &'static Data = self.psc_build().data;
        let sortOrder = &buildData.clusterJewels.notableSortOrder;
        // L2114
        for name in ipairs_vals(&jewelData.get(n!("clusterJewelNotables")), "clusterJewelNotables") {
            // L2115
            let baseNode = match &name {
                Val::Str(s) => val_node(&tree.clusterNodeMap.get(&**s)),
                _ => None,
            };
            // Ignore subgraphs when loading old trees where certain notables don't exist
            // L2117
            let Some(baseNode) = baseNode else {
                // L2118
                self.psc_set_subgraph(nodeId, None);
                // L2119
                return;
            };
            // L2121: assert(baseNode, …) holds
            // L2122
            if !sortOrder.contains_key(&Name::from(&*baseNode.dn.get())) {
                lua_error!(
                    "Cluster notable has no sort order: {}",
                    name.str().map_or_else(String::new, |s| s.to_string())
                );
            }
            // L2123
            notableList.push(baseNode);
        }
        // L2125
        pob_lua::sort::sort(&mut notableList, |a, b| {
            sortOrder[&Name::from(&*a.dn.get())] < sortOrder[&Name::from(&*b.dn.get())]
        });

        // L2127
        let skillsT = clusterJewel.get(n!("skills"));
        let skill = {
            let s = match jewelData.get(n!("clusterJewelSkill")) {
                Val::Str(k) => skillsT.get(&*k),
                Val::Nil => Val::Nil,
                other => skillsT.get(match other {
                    Val::Num(x) => Key::from(x),
                    Val::Bool(b) => Key::from(b),
                    o => lua_error!("table index is a {}", o.lua_type()),
                }),
            };
            if s.truthy() {
                s
            } else {
                // L2127-2131
                let t = Table::new();
                t.set(n!("name"), "Nothingness");
                t.set(n!("icon"), "Art/2DArt/SkillIcons/passives/MasteryBlank.png");
                t.set(n!("stats"), Table::new());
                Val::Table(t)
            }
        };
        // L2132
        let socketCount = lor(jewelData.get(n!("clusterJewelSocketCountOverride")), || {
            jewelData.get(n!("clusterJewelSocketCount"))
        })
        .num_or(0.0);
        // L2133
        let notableCount = notableList.len() as f64;
        // L2134
        let nodeCount = {
            let v = jewelData.get(n!("clusterJewelNodeCount"));
            if v.truthy() {
                v.num()
            } else {
                socketCount
                    + notableCount
                    + jewelData.num_or(n!("clusterJewelNothingnessCount"), 0.0)
            }
        };
        // L2135
        let smallCount = nodeCount - socketCount - notableCount;

        // L2137
        if skill.get(n!("masteryIcon")).truthy() {
            // Add mastery node
            // L2139
            subGraphGroup
                .table(n!("oo"))
                .expect("oo")
                .set(0.0, true);
            // L2140
            let node = new_cluster_node(
                n!("Mastery"),
                nodeId + 12.0,
                LStr::from("Nothingness"),
                Shared::new(Vec::new()),
                skill.get(n!("masteryIcon")),
                &subGraphGroup,
                0.0,
                0.0,
            );
            subGraphNodes.push(node_val(&Rc::new(node)));
        }

        // L2151
        let mut indicies: LjIntMap<NodeRef> = LjIntMap::new();

        let size = clusterJewel.get(n!("size"));
        let sizeIs = |s: &str| size.str().is_some_and(|x| &*x == s);

        // L2153
        let makeJewel = |indicies: &mut LjIntMap<NodeRef>, nodeIndex: f64, jewelIndex: f64| {
            // Look for the socket
            // L2155
            let socket = self.FindClusterSocket(&proxyGroupT(), jewelIndex);
            // L2156
            let Some(socket) = socket else {
                lua_error!("Socket not found (ran out of sockets nani?)")
            };

            // Construct the new node
            // L2159
            let node = new_cluster_node(
                n!("Socket"),
                socket.id,
                socket.dn.get(),
                Shared::new(Vec::new()),
                socket.extra.get(n!("icon")),
                &subGraphGroup,
                nodeOrbit,
                nodeIndex,
            );
            node.expansionJewel.set(socket.expansionJewel.get());
            let node: NodeRef = Rc::new(node);
            // L2169
            subGraphNodes.push(node_val(&node));
            // L2170
            lj_set_node(indicies, nodeIndex, node.clone());

            // L2172
            if let Some(legacyProxyGroup) = &legacyProxyGroup {
                if self.psc_legacy_map().is_some() {
                    // L2173
                    let legacySocket = self.FindClusterSocket(legacyProxyGroup, jewelIndex);
                    // L2174
                    if let Some(legacySocket) = legacySocket {
                        // L2175
                        self.RegisterLegacyClusterNodeMap(legacySocket.id, node.id);
                    }
                }
            }
        };

        // First pass: sockets
        // L2181
        let socketIndicies = ipairs_vals(&clusterJewel.get(n!("socketIndicies")), "socketIndicies");
        if sizeIs("Large") && socketCount == 1.0 {
            // Large clusters always have the single jewel at index 6
            // L2183
            makeJewel(&mut indicies, 6.0, 1.0);
        } else {
            // L2185
            if !(socketCount <= socketIndicies.len() as f64) {
                lua_error!("Too many sockets!");
            }
            // L2186
            let getJewels = [0.0, 2.0, 1.0];
            // L2187
            let mut i = 1.0;
            while i <= socketCount {
                // L2188
                let k = i as usize;
                let nodeIndex = socketIndicies
                    .get(k - 1)
                    .map_or(f64::NAN, |v| v.opt_num().unwrap_or(f64::NAN));
                let jewelIndex = getJewels.get(k - 1).copied().unwrap_or(f64::NAN);
                makeJewel(&mut indicies, nodeIndex, jewelIndex);
                i += 1.0;
            }
        }

        // Second pass: notables

        // Gather notable indicies
        // L2195
        let mut notableIndexList: Vec<f64> = Vec::new();
        // L2196
        for nodeIndex in ipairs_vals(&clusterJewel.get(n!("notableIndicies")), "notableIndicies") {
            let mut nodeIndex = nodeIndex.num();
            // L2197
            if notableIndexList.len() as f64 == notableCount {
                // L2198
                break;
            }
            // L2200
            if sizeIs("Medium") {
                // L2201
                if socketCount == 0.0 && notableCount == 2.0 {
                    // Special rule for two notables in a Medium cluster
                    // L2203
                    if nodeIndex == 6.0 {
                        // L2204
                        nodeIndex = 4.0;
                    } else if nodeIndex == 10.0 {
                        // L2206
                        nodeIndex = 8.0;
                    }
                } else if nodeCount == 4.0 {
                    // Special rule for notables in a 4-node Medium cluster
                    // L2210
                    if nodeIndex == 10.0 {
                        // L2211
                        nodeIndex = 9.0;
                    } else if nodeIndex == 2.0 {
                        // L2213
                        nodeIndex = 3.0;
                    }
                }
            }
            // L2217
            if !lj_has(&indicies, nodeIndex) {
                // L2218
                notableIndexList.push(nodeIndex);
            }
        }
        // L2221
        pob_lua::sort::sort_default(&mut notableIndexList);

        // Create the notables
        // L2224
        for (index, baseNode) in notableList.iter().enumerate() {
            // Get the index
            // L2226
            let nodeIndex = notableIndexList.get(index).copied();
            // L2227
            let Some(nodeIndex) = nodeIndex else {
                // Silently fail to handle cases of jewels with more notables than should be allowed
                // L2229
                break;
            };

            // Construct the new node
            // L2233
            let node = new_cluster_node(
                n!("Notable"),
                nodeId + nodeIndex,
                baseNode.dn.get(),
                baseNode.sd.get(),
                baseNode.extra.get(n!("icon")),
                &subGraphGroup,
                nodeOrbit,
                nodeIndex,
            );
            node.extra.set(n!("expansionSkill"), true);
            let node: NodeRef = Rc::new(node);
            // L2243
            subGraphNodes.push(node_val(&node));
            // L2244
            lj_set_node(&mut indicies, nodeIndex, node);
        }

        // Third pass: small fill

        // Gather small indicies
        // L2250
        let mut smallIndexList: Vec<f64> = Vec::new();
        // L2251
        for nodeIndex in ipairs_vals(&clusterJewel.get(n!("smallIndicies")), "smallIndicies") {
            let mut nodeIndex = nodeIndex.num();
            // L2252
            if smallIndexList.len() as f64 == smallCount {
                // L2253
                break;
            }
            // L2255
            if sizeIs("Medium") {
                // Special rules for small nodes in Medium clusters
                // L2257
                if nodeCount == 5.0 && nodeIndex == 4.0 {
                    // L2258
                    nodeIndex = 3.0;
                } else if nodeCount == 4.0 {
                    // L2260
                    if nodeIndex == 8.0 {
                        // L2261
                        nodeIndex = 9.0;
                    } else if nodeIndex == 4.0 {
                        // L2263
                        nodeIndex = 3.0;
                    }
                }
            }
            // L2267
            if !lj_has(&indicies, nodeIndex) {
                // L2268
                smallIndexList.push(nodeIndex);
            }
        }

        // Create the small nodes
        // L2273
        let mut index = 1.0;
        while index <= smallCount {
            // Get the index
            // L2275
            let nodeIndex = if index == index.trunc() {
                smallIndexList.get(index as usize - 1).copied()
            } else {
                None
            };
            // L2276
            let Some(nodeIndex) = nodeIndex else {
                // L2277
                break;
            };

            // Construct the new node
            // L2281
            let node = new_cluster_node(
                n!("Normal"),
                nodeId + nodeIndex,
                skill
                    .get(n!("name"))
                    .str()
                    .unwrap_or_else(|| lua_error!("cluster skill without a name")),
                Shared::new(str_list(&skill.get(n!("stats")), "skill.stats")),
                skill.get(n!("icon")),
                &subGraphGroup,
                nodeOrbit,
                nodeIndex,
            );
            node.extra.set(n!("expansionSkill"), true);
            // L2291
            for line in ipairs_vals(&jewelData.get(n!("clusterJewelAddedMods")), "clusterJewelAddedMods") {
                // L2292
                match line {
                    Val::Str(s) => node.sd.get().push(s),
                    other => lua_error!("clusterJewelAddedMods: a {} line", other.lua_type()),
                }
            }
            let node: NodeRef = Rc::new(node);
            // L2294
            subGraphNodes.push(node_val(&node));
            // L2295
            lj_set_node(&mut indicies, nodeIndex, node);
            index += 1.0;
        }

        // L2298
        let entrance = indicies.get(0);
        let Some(entrance) = entrance else {
            lua_error!("No entrance to subgraph")
        };
        // L2299
        subGraph.set(n!("entranceNode"), node_val(&entrance));

        // Convert from cluster-template index space into the tree's orbit index space.
        // L2302 (`skillsPerOrbit[sizeIndex + 2]`)
        let sizeIndex = clusterJewel.num(n!("sizeIndex"));
        let skillsPerOrbit = tree
            .skillsPerOrbit
            .get((sizeIndex + 1.0) as usize)
            .copied()
            .unwrap_or(f64::NAN);
        // L2303
        let startOidx = d
            .clusterJewels
            .orbitOffsets
            .get(&(proxyNode.id as i64))
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
            .get(&(sizeIndex as i64))
            .copied()
            .unwrap_or(f64::NAN);
        if startOidx.is_nan() {
            lua_error!("attempt to perform arithmetic on local 'startOidx' (a nil value)");
        }
        let totalIndicies = clusterJewel.num(n!("totalIndicies"));
        // L2304
        self.ApplyClusterOrbitIndexAdjustment_lj(&indicies, startOidx, totalIndicies, skillsPerOrbit);
        // L2305
        self.BuildLegacyClusterOrbitMappings_lj(&indicies, &proxyNode, totalIndicies, skillsPerOrbit);

        // Perform processing on nodes to calculate positions, parse mods, and other goodies
        // L2308
        let nodes: Vec<NodeRef> = subGraphNodes
            .ipairs()
            .into_iter()
            .map(|(_, v)| val_node(&v).expect("subgraph node"))
            .collect();
        for node in &nodes {
            // L2309
            node.linked.set(Shared::new(Vec::new()));
            // L2310
            node.extra.set(n!("power"), Table::new());
            // L2311
            tree.ProcessNode(node);
            // L2312 (`node.modList` is set by ProcessStats)
            let incEffect = jewelData.get(n!("clusterJewelIncEffect"));
            if incEffect.truthy() && node.r#type.get() == n!("Normal") {
                // L2313 (`NewMod` with no source: `mod.source` is nil)
                node.modList.get().AddMod(Arc::new(modLib::createMod(
                    n!("PassiveSkillEffect"),
                    ModType::INC,
                    incEffect.num(),
                    None,
                    0,
                    0,
                    &[],
                )));
            }
        }

        // Generate connectors
        // L2318
        let mut firstNode: Option<NodeRef> = None;
        let mut lastNode: Option<NodeRef> = None;
        // L2319
        let mut i = 0.0;
        while i <= totalIndicies - 1.0 {
            // L2320
            let thisNode = indicies.get(i as i64);
            // L2321
            if let Some(thisNode) = thisNode {
                // L2322
                if firstNode.is_none() {
                    // L2323
                    firstNode = Some(thisNode.clone());
                }
                // L2325
                if let Some(lastNode) = &lastNode {
                    // L2326
                    linkNodes(&thisNode, lastNode);
                }
                // L2328
                lastNode = Some(thisNode);
            }
            i += 1.0;
        }
        // L2331
        let firstNode = firstNode.expect("indicies[0] exists");
        let lastNode = lastNode.expect("indicies[0] exists");
        if !Rc::ptr_eq(&firstNode, &lastNode) && !sizeIs("Small") {
            // Close the loop on non-small clusters
            // L2333
            linkNodes(&firstNode, &lastNode);
        }
        // L2335
        linkNodes(&entrance, parentSocket);

        // Add synthetic nodes to the main node list
        // L2338
        for node in &nodes {
            // L2339
            self.nodes.with_mut(|m| m.set(node.id as i64, Some(node.clone())));
            // L2340
            if addToAllocatedSubgraphNodes(node) {
                // L2341
                self.allocSubgraphNodes.push(node.id);
            }
            // L2343
            if node.r#type.get() == n!("Socket") {
                // Recurse to smaller jewels
                // L2345
                let (jewel, _) = self.GetSocketedJewel(node.id);
                // Allocated subgraph nodes are temporarily preserved outside allocNodes while rebuilding.
                // L2347
                if self.allocNodes.with(|m| m.contains_key(node.id as i64))
                    || num_index(&self.allocSubgraphNodes, node.id).is_some()
                {
                    if let Some(jewel) = jewel {
                        let valid = jewel
                            .jewelData
                            .get()
                            .expect("GetJewel returns jewels with jewelData")
                            .truthy(n!("clusterJewelValid"));
                        if valid {
                            // L2348
                            self.BuildSubgraph(
                                &jewel,
                                node,
                                Some(id),
                                upSize,
                                importedNodes,
                                importedGroups,
                            );
                        }
                    }
                }
            }
        }

        //ConPrintTable(subGraph)
    }
}

/// `indicies[nodeIndex] = node` (a nil index raises, as in Lua).
fn lj_set_node(t: &mut LjIntMap<NodeRef>, k: f64, v: NodeRef) {
    if k.is_nan() {
        lua_error!("table index is nil");
    }
    if k != k.trunc() || !k.is_finite() {
        lua_error!(
            "cluster node index {} is not an integer (unsupported table key)",
            tostring_num(k)
        );
    }
    t.set(k as i64, Some(v));
}

/// `indicies[nodeIndex]` truthy.
fn lj_has(t: &LjIntMap<NodeRef>, k: f64) -> bool {
    !k.is_nan() && k == k.trunc() && k.is_finite() && t.contains_key(k as i64)
}

// ---------------------------------------------------------------------------------------
// Undo state
// ---------------------------------------------------------------------------------------

/// @pob Classes/PassiveSpec.lua:2361-2383 h=11f62e74
impl PassiveSpec {
    /// The state table: `classId`, `ascendClassId`, `secondaryAscendClassId`, `hashList`
    /// (a list `Table`), `hashOverrides` (`{ [nodeId] = node }`, `Val::Obj` nodes) and
    /// `masteryEffects` (`{ [masteryId] = effectId }`), each filled in the `pairs` order of
    /// the spec's table (CCR-0072 §5), and `treeVersion`.
    pub fn CreateUndoState(&self) -> Table {
        // L2362
        let allocNodeIdList = Table::new();
        // L2363
        for (nodeId, _) in self.allocNodes.with(LjIntMap::pairs) {
            // L2364
            allocNodeIdList.push(nodeId as f64);
        }
        // L2366
        let selections = Table::new();
        // L2367
        for (mastery, effect) in self.masterySelections.with(LjIntMap::pairs) {
            // L2368
            selections.set(mastery, effect);
        }
        // L2370
        let hashOverridesCopy = Table::new();
        // L2371
        for (node, r#override) in self.hashOverrides.with(LjIntMap::pairs) {
            // L2372
            hashOverridesCopy.set(node, node_val(&r#override));
        }
        // L2374
        let state = Table::new();
        state.set(n!("classId"), num_val(self.curClassId.get()));
        state.set(n!("ascendClassId"), num_val(self.curAscendClassId.get()));
        // LUA-QUIRK: `self.secondaryAscendClassId` is a field PassiveSpec never sets (the class
        // id is `curSecondaryAscendClassId`), so the state's value is nil
        state.set(
            n!("secondaryAscendClassId"),
            self.extra.get(n!("secondaryAscendClassId")),
        );
        state.set(n!("hashList"), allocNodeIdList);
        state.set(n!("hashOverrides"), hashOverridesCopy);
        state.set(n!("masteryEffects"), selections);
        state.set(n!("treeVersion"), self.treeVersion.get());
        state
    }
}

/// @pob Classes/PassiveSpec.lua:2385-2388 h=9d8437a6
impl PassiveSpec {
    pub fn RestoreUndoState(&self, state: &Table, treeVersion: Option<Name>) {
        let opt = |k: Name| state.get(k).opt_num().unwrap_or(f64::NAN);
        let hashList: Vec<f64> = state
            .table(n!("hashList"))
            .map_or_else(Vec::new, |t| t.ipairs().into_iter().map(|(_, v)| v.num()).collect());
        // `pairs(state.hashOverrides)` / `pairs(state.masteryEffects)` in LuaJIT's order: the
        // copies were filled in this insertion order (CCR-0072 §5)
        let mut overrides: LjIntMap<NodeRef> = LjIntMap::new();
        if let Some(t) = state.table(n!("hashOverrides")) {
            for (k, v) in t.pairs() {
                let Key::I(id) = k else {
                    lua_error!("PassiveSpec:RestoreUndoState: hashOverrides key is not a node id")
                };
                overrides.set(
                    id,
                    Some(val_node(&v).unwrap_or_else(|| {
                        lua_error!("PassiveSpec:RestoreUndoState: hashOverrides value is not a node")
                    })),
                );
            }
        }
        let mut effects: LjIntMap<f64> = LjIntMap::new();
        if let Some(t) = state.table(n!("masteryEffects")) {
            for (k, v) in t.pairs() {
                let Key::I(m) = k else {
                    lua_error!("PassiveSpec:RestoreUndoState: masteryEffects key is not a node id")
                };
                effects.set(m, Some(v.num()));
            }
        }
        let effects: Vec<(f64, f64)> = effects
            .pairs()
            .into_iter()
            .map(|(k, v)| (k as f64, v))
            .collect();
        let stateTreeVersion = state
            .get(n!("treeVersion"))
            .str()
            .map(|s| Name::from(&*s));
        // L2386
        self.ImportFromNodeList_impl(
            None,
            opt(n!("classId")),
            opt(n!("ascendClassId")),
            opt(n!("secondaryAscendClassId")),
            &hashList,
            Shared::new(overrides),
            &effects,
            treeVersion.or(stateTreeVersion),
        );
        // L2387
        self.SetWindowTitleWithBuildClass();
    }
}

/// @pob Classes/PassiveSpec.lua:2390-2396 h=4d7d27cb
impl PassiveSpec {
    /// UI only (window title): the subtext goes to the host's receiver
    /// ([`set_window_title_subtext_hook`]); `build.buildName` is the UI's and not in the
    /// model, so the receiver gets `"%s (%s)"` with the class text only (CCR-0072 §6).
    pub fn SetWindowTitleWithBuildClass(&self) {
        // L2391
        let mut classText: Option<String> = if self.curAscendClassId.get() == 0.0 {
            self.curClassName.get().map(|n| n.to_string())
        } else {
            None
        };
        if classText.is_none() {
            classText = self.curAscendClassName.get().map(|n| n.to_string());
        }
        // L2392
        if !self.curSecondaryAscendClassId.get().is_nan()
            && self.curSecondaryAscendClassId.get() != 0.0
        {
            if let Some(sec) = self.curSecondaryAscendClassName.get() {
                // L2393
                let Some(ct) = classText else {
                    lua_error!("attempt to concatenate local 'classText' (a nil value)")
                };
                classText = Some(format!("{ct} + {sec}"));
            }
        }
        // L2395 (`self.build.buildName` is the UI's: the receiver adds it)
        main_SetWindowTitleSubtext(&classText.unwrap_or_else(|| "nil".to_owned()));
    }
}

// ---------------------------------------------------------------------------------------
// Node rewrites
// ---------------------------------------------------------------------------------------

/// @pob Classes/PassiveSpec.lua:2402-2483 h=5a99512a
impl PassiveSpec {
    /// Adds a line to or replaces a node given a line to add/replace with
    /// @param node table The node to replace/add to
    /// @param sd string The line being parsed and added
    /// @param replacement? boolean true to replace the node with the new mod, false to simply add it
    pub fn NodeAdditionOrReplacementFromString(&self, node: &NodeRef, sd: &str, replacement: bool) {
        // L2403 (the fields of `addition`)
        // L2404
        let addition_sd: Shared<Vec<LStr>> = Shared::new(vec![LStr::from(sd)]);
        // L2405 (`addition.mods[i]`: a LuaJIT table; dummy entries go in before the line's own)
        let mut addition_mods: LjIntMap<Table> = LjIntMap::new();
        // L2406
        let addition_modList = ModStore::ModList(None);
        // L2407
        let mut addition_modKey = String::new();
        let mut _addition_unknown = false;
        let mut _addition_extra = false;
        // L2408
        let mut i: usize = 1;
        // L2409
        while let Some(cur) = addition_sd.get(i) {
            // L2410
            if cur.contains('\n') {
                // L2411
                let line = cur;
                // L2412
                let mut lineIdx = i;
                // L2413
                addition_sd.remove(Some(i));
                // L2414
                for caps in pob_lua::gmatch(&line, "[^\n]+") {
                    // L2415
                    addition_sd.insert(lineIdx, LStr::from(caps[0].as_str()));
                    // L2416
                    lineIdx += 1;
                }
            }
            // L2419
            let line = addition_sd.get(i).unwrap_or_else(|| {
                lua_error!("bad argument #1 to 'parseMod' (string expected, got nil)")
            });
            // L2420
            let (mut parsedMod, mut unrecognizedMod) = pob_modparser::parseMod(&line, false);
            // L2421
            if parsedMod.is_none() || unrecognizedMod.is_some() {
                // Try to combine it with one or more of the lines that follow this one
                // L2423
                let mut endI = i + 1;
                // L2424
                while addition_sd.get(endI).is_some() {
                    // L2425
                    let mut comb = String::from(&*line);
                    // L2426
                    for ci in i + 1..=endI {
                        // L2427
                        comb.push(' ');
                        comb.push_str(&addition_sd.get(ci).expect("sd[ci]"));
                    }
                    // L2429
                    (parsedMod, unrecognizedMod) = pob_modparser::parseMod(&comb, true);
                    // L2430
                    if parsedMod.is_some() && unrecognizedMod.is_none() {
                        // Success, add dummy mod lists to the other lines that were combined with this one
                        // L2432
                        for ci in i + 1..=endI {
                            // L2433
                            addition_mods.set(ci as i64, Some(tbl! { list = Table::new() }));
                        }
                        // L2435
                        break;
                    }
                    // L2437
                    endI += 1;
                }
            }
            let listT = parsedMod.map(|l| {
                let t = Table::new();
                for m in l {
                    t.push(Val::Mod(Arc::new(m)));
                }
                t
            });
            // L2440
            if listT.is_none() {
                // Parser had no idea how to read this modifier
                // L2442
                _addition_unknown = true;
            } else if unrecognizedMod.is_some() {
                // Parser recognised this as a modifier but couldn't understand all of it
                // L2445
                _addition_extra = true;
            } else {
                // L2447
                for (_, r#mod) in listT.as_ref().expect("list").ipairs() {
                    let Val::Mod(r#mod) = r#mod else {
                        unreachable!()
                    };
                    // L2448
                    addition_modKey.push('[');
                    addition_modKey.push_str(&modLib::formatMod(&r#mod));
                    addition_modKey.push(']');
                }
            }
            // L2451
            let entry = Table::new();
            if let Some(l) = &listT {
                entry.set(n!("list"), l.clone());
            }
            if let Some(e) = &unrecognizedMod {
                entry.set(n!("extra"), e.as_str());
            }
            addition_mods.set(i as i64, Some(entry));
            // L2452
            i += 1;
            // L2453
            while addition_mods.contains_key(i as i64) {
                // Skip any lines with dummy lists added by the line combining code
                // L2455
                i += 1;
            }
        }

        // Build unified list of modifiers from all recognised modifier lines
        let source = tree_source(node);
        // L2460 (LUA-ORDER: the mods table's LuaJIT order)
        for (_, r#mod) in addition_mods.pairs() {
            // L2461
            if let Some(list) = r#mod.table(n!("list")) {
                if !r#mod.truthy(n!("extra")) {
                    // L2462
                    for (j, m) in list.ipairs() {
                        let Val::Mod(m) = m else {
                            lua_error!("addition.mods[i].list holds a non-mod")
                        };
                        // L2463 (setSource mutates the mod in the list: the list gets the copy)
                        let m: ModRef = Arc::new(modLib::setSource(&m, &source));
                        list.set(j, Val::Mod(m.clone()));
                        // L2464
                        addition_modList.AddMod(m);
                    }
                }
            }
        }
        // `addition.mods` as a list (`#t` and `t[1..n]`: tableConcat and node.mods)
        let modsList: Vec<Table> = {
            let n = addition_mods.len();
            (1..=n as i64)
                .map(|k| {
                    addition_mods.get(k).unwrap_or_else(|| {
                        lua_error!(
                            "PassiveSpec: addition.mods has a hole at {} (hole-free list contract)",
                            k
                        )
                    })
                })
                .collect()
        };
        // L2468
        if replacement {
            // L2469
            node.sd.set(addition_sd);
            // L2470
            node.mods.set(Shared::new(modsList));
            // L2471
            node.modKey.set(LStr::from(addition_modKey.as_str()));
        } else {
            // L2473 (tableConcat: a new list)
            let mut sdv = node.sd.get().snapshot();
            sdv.extend(addition_sd.snapshot());
            node.sd.set(Shared::new(sdv));
            // L2474
            let mut modsv = node.mods.get().snapshot();
            modsv.extend(modsList);
            node.mods.set(Shared::new(modsv));
            // L2475
            let mk = format!("{}{}", node.modKey.get(), addition_modKey);
            node.modKey.set(LStr::from(mk.as_str()));
        }
        // L2477
        let modList = ModStore::ModList(None);
        // L2478
        modList.AddList(&addition_modList.ipairs());
        // L2479
        if !replacement {
            // L2480
            modList.AddList(&node.modList.get().ipairs());
        }
        // L2482
        node.modList.set(modList);
    }
}

/// @pob Classes/PassiveSpec.lua:2485-2494 h=ecbda051
impl PassiveSpec {
    pub fn NodeInKeystoneRadius(
        &self,
        keystoneNames: &Table,
        nodeId: f64,
        radiusIndex: f64,
    ) -> bool {
        let tree = self.tree.get();
        // L2486
        for (keystoneName, _) in keystoneNames.pairs() {
            // L2487
            let keystoneNode = val_node(&tree.keystoneMap.get(keystoneName));
            // L2488
            let radius = keystoneNode
                .as_ref()
                .and_then(|n| nodesInRadiusAt(n, radiusIndex));
            // L2489
            if let Some(radius) = radius {
                if !nodeId.is_nan() && radius.contains_key(nodeId as i64) {
                    // L2490
                    return true;
                }
            }
        }
        // L2493
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translate_orbit_tables() {
        let spec = PassiveSpec::default();
        assert_eq!(spec.TranslateClusterOrbitIndex(5.0, 12.0, 12.0), 5.0);
        assert_eq!(spec.TranslateClusterOrbitIndex(0.0, 12.0, 16.0), 0.0);
        assert_eq!(spec.TranslateClusterOrbitIndex(11.0, 12.0, 16.0), 15.0);
        assert!(spec.TranslateClusterOrbitIndex(12.0, 12.0, 16.0).is_nan());
        assert_eq!(spec.TranslateClusterOrbitIndex(2.0, 16.0, 12.0), 1.0);
        assert_eq!(spec.TranslateClusterOrbitIndex(5.0, 6.0, 16.0), 13.0);
        assert_eq!(spec.TranslateClusterOrbitIndex(15.0, 16.0, 6.0), 5.0);
        assert!(spec.TranslateClusterOrbitIndex(-1.0, 16.0, 6.0).is_nan());
        assert_eq!(spec.TranslateClusterOrbitIndex(7.0, 12.0, 6.0), 3.0);
    }
}
