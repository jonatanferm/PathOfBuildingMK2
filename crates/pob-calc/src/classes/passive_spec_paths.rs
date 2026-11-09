//! @pob-file Classes/PassiveSpec.lua @16de4b82d
//! PassiveSpec:BuildAllDependsAndPaths (Classes/PassiveSpec.lua 1092-1696).
//!
//! Owner: PS-B. Rebuilds, for every node of the spec, the timeless / abyss conquest and the
//! node rewrites it implies, the mastery selections and counters, the dependency lists
//! (`depends`, intuitive-leap-like jewels), the orphan pruning and the 0-1 BFS paths.
//!
//! Representation (CCR-0052 §7, CCR-0070):
//! * `node.intuitiveLeapLikesAffecting` is a list `Table` of `Val::Obj(NodeRef)` in
//!   `node.extra`; `node.visited` is a boolean in `node.extra`.
//! * `node.conqueredBy` is a `Table` (frozen `Field<Option<Table>>`). A jewel's
//!   `jewelData.conqueredBy` built by IT-C is an immutable data table (`Val::Data`); PS-B then
//!   stores a shallow `Table` copy of it, one per jewel and call (CCR-0070 §1). Every reader
//!   reads it by value (PassiveSpec.lua:1224-1401, PassiveTreeView.lua:24).
//! * The abyss conquest `modification` stays the immutable `Val::Data` that
//!   `readAbyssJewelLUT` returns.
//! * `data.resolveAbyssJewelComponent(component, self.tree.legion)` runs against the tree's
//!   mutable legion `Table` (CCR-0020 §8): [`resolve_abyss_component`].
//! * PS-B's calls into PS-C (`ReplaceNode`, `NodeAdditionOrReplacementFromString`,
//!   `ReconnectNodeToClassStart`, `NodeInKeystoneRadius`) and its `ConPrintf` go through
//!   [`PscCalls`] when a test installs one (CCR-0070 §2); otherwise they call the methods.
#![allow(
    non_snake_case,
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::float_cmp,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::len_zero,
    clippy::collapsible_else_if,
    clippy::if_same_then_else,
    clippy::unnecessary_unwrap,
    clippy::explicit_auto_deref,
    clippy::no_effect_underscore_binding
)]

use std::cell::RefCell;
use std::collections::VecDeque;

use crate::classes::passive_tree::{node_id_val, node_val, nodesInRadiusAt};
use crate::prelude::*;

// ---------------------------------------------------------------------------------------
// Port plumbing (snake_case)
// ---------------------------------------------------------------------------------------

/// PS-B's calls into PS-C and its console (CCR-0070 §2). The product never installs one:
/// every call then goes to the `PassiveSpec` method (or stderr). The PS-B oracle test
/// installs a recorder that also stands in for PS-C's bodies while they are `unported!`.
pub trait PscCalls {
    fn ReplaceNode(&self, spec: &PassiveSpec, old: &NodeRef, newNode: &NodeRef);
    fn NodeAdditionOrReplacementFromString(
        &self,
        spec: &PassiveSpec,
        node: &NodeRef,
        sd: &str,
        replacement: bool,
    );
    fn ReconnectNodeToClassStart(&self, spec: &PassiveSpec, node: &NodeRef);
    fn NodeInKeystoneRadius(
        &self,
        spec: &PassiveSpec,
        keystoneNames: &Table,
        nodeId: f64,
        radiusIndex: f64,
    ) -> bool;
    fn ConPrintf(&self, msg: &str);
}

thread_local! {
    static PSC_CALLS: RefCell<Option<Rc<dyn PscCalls>>> = const { RefCell::new(None) };
}

/// Installs (or, with `None`, removes) the [`PscCalls`] of this thread.
pub fn set_psc_calls(calls: Option<Rc<dyn PscCalls>>) {
    PSC_CALLS.with(|c| *c.borrow_mut() = calls);
}

fn psc_calls() -> Option<Rc<dyn PscCalls>> {
    PSC_CALLS.with(|c| c.borrow().clone())
}

/// `ConPrintf(msg)` (the messages here contain no format directives).
fn ConPrintf(msg: &str) {
    match psc_calls() {
        Some(c) => c.ConPrintf(msg),
        None => eprintln!("{msg}"),
    }
}

impl PassiveSpec {
    /// `self:ReplaceNode(old, newNode)` (PS-C; the result is unused here).
    fn psc_ReplaceNode(&self, old: &NodeRef, newNode: &NodeRef) {
        match psc_calls() {
            Some(c) => c.ReplaceNode(self, old, newNode),
            None => {
                self.ReplaceNode(old, newNode);
            }
        }
    }

    /// `self:NodeAdditionOrReplacementFromString(node, sd, replacement)` (PS-C).
    fn psc_NodeAddition(&self, node: &NodeRef, sd: &str, replacement: bool) {
        match psc_calls() {
            Some(c) => c.NodeAdditionOrReplacementFromString(self, node, sd, replacement),
            None => self.NodeAdditionOrReplacementFromString(node, sd, replacement),
        }
    }

    /// `self:ReconnectNodeToClassStart(node)` (PS-C).
    fn psc_ReconnectNodeToClassStart(&self, node: &NodeRef) {
        match psc_calls() {
            Some(c) => c.ReconnectNodeToClassStart(self, node),
            None => self.ReconnectNodeToClassStart(node),
        }
    }

    /// `self:NodeInKeystoneRadius(keystoneNames, nodeId, radiusIndex)` (PS-C).
    fn psc_NodeInKeystoneRadius(
        &self,
        keystoneNames: &Table,
        nodeId: f64,
        radiusIndex: f64,
    ) -> bool {
        match psc_calls() {
            Some(c) => c.NodeInKeystoneRadius(self, keystoneNames, nodeId, radiusIndex),
            None => self.NodeInKeystoneRadius(keystoneNames, nodeId, radiusIndex),
        }
    }

    /// `self.nodes[id]`.
    fn psb_node(&self, id: f64) -> Option<NodeRef> {
        if id.is_nan() {
            return None;
        }
        self.nodes.with(|m| m.get(id as i64))
    }

    /// `self.allocNodes[id]`.
    fn psb_alloc(&self, id: f64) -> Option<NodeRef> {
        if id.is_nan() {
            return None;
        }
        self.allocNodes.with(|m| m.get(id as i64))
    }

    /// `self.build.itemsTab.items[itemId]`.
    fn psb_item(&self, itemId: f64) -> Option<ItemRef> {
        if !(itemId >= 0.0 && itemId == itemId.trunc() && itemId <= f64::from(u32::MAX)) {
            return None;
        }
        let build = self
            .build
            .upgrade()
            .unwrap_or_else(|| lua_error!("attempt to index field 'build' (a nil value)"));
        build
            .itemsTab
            .items
            .with(|items| items.get(&(itemId as u32)).cloned())
    }
}

/// `timelessJewelTypeByConqueror[t]` (L1093-1105).
fn timelessJewelTypeByConqueror(t: &Val) -> Option<f64> {
    let Val::Str(s) = t else { return None };
    Some(match &**s {
        "vaal" => 1.0,
        "karui" => 2.0,
        "maraketh" => 3.0,
        "templar" => 4.0,
        "eternal" => 5.0,
        "kalguur" => 6.0,
        "abyss_murderous" => 7.0,
        "abyss_searching" => 8.0,
        "abyss_hypnotic" => 9.0,
        "abyss_ghastly" => 10.0,
        "abyss_special" => 11.0,
        _ => return None,
    })
}

/// `item.jewelData` as a table (`nil` → `None`).
fn jewel_data(item: &Item) -> Option<Table> {
    item.jewelData.get()
}

/// A conquest value (`jewelData.conqueredBy`) as the `Table` `node.conqueredBy` holds: a
/// calc table itself, or a shallow copy of an immutable data table (CCR-0070 §1).
fn conquest_table(v: &Val) -> Option<Table> {
    match v {
        Val::Table(t) => Some(t.clone()),
        Val::Data(Value::Table(t)) => Some(Table::from_value_table(t)),
        Val::Nil | Val::Bool(false) => None,
        other => lua_error!(
            "PassiveSpec: conqueredBy is a {} value (a table is expected)",
            other.lua_type()
        ),
    }
}

/// `x.conqueror.type` (indexing a nil `conqueror` raises).
fn conqueror_type(conqueredBy: &Val) -> Val {
    conqueredBy.get(n!("conqueror")).get(n!("type"))
}

/// `#node.intuitiveLeapLikesAffecting` (nil raises, as in Lua).
fn ill_list(node: &Node) -> Table {
    match node.extra.get(n!("intuitiveLeapLikesAffecting")) {
        Val::Table(t) => t,
        other => lua_error!(
            "attempt to get length of field 'intuitiveLeapLikesAffecting' (a {} value)",
            other.lua_type()
        ),
    }
}

fn ill_len(node: &Node) -> usize {
    ill_list(node).len()
}

/// The nodes of `node.intuitiveLeapLikesAffecting` (`ipairs`).
fn ill_nodes(node: &Node) -> Vec<NodeRef> {
    ill_list(node)
        .ipairs()
        .into_iter()
        .map(|(_, v)| {
            v.obj::<Node>()
                .unwrap_or_else(|| lua_error!("intuitiveLeapLikesAffecting holds a non-node value"))
        })
        .collect()
}

/// `isValueInArray(list, node)` on a node list (identity).
fn in_node_list(list: &Shared<Vec<NodeRef>>, node: &NodeRef) -> bool {
    list.with(|v| v.iter().any(|x| Rc::ptr_eq(x, node)))
}

/// `t[i]` of a list table of nodes (`legion.nodes`); `nil` → `None`.
fn node_at_index(t: &Table, i: f64) -> Option<NodeRef> {
    if i.is_nan() {
        return None;
    }
    match t.get(i) {
        Val::Nil => None,
        v => Some(
            v.obj::<Node>()
                .unwrap_or_else(|| lua_error!("legion.nodes[{}] is not a node", tostring_num(i))),
        ),
    }
}

/// A node `self:ReplaceNode(node, x)` receives: `x` nil raises there (`newNode.sd`).
fn must_node(n: Option<NodeRef>) -> NodeRef {
    n.unwrap_or_else(|| lua_error!("attempt to index local 'newNode' (a nil value)"))
}

/// The keys of a (string-keyed) table value, in `pairs` order.
fn table_keys(v: &Val) -> Vec<Val> {
    match v {
        Val::Table(t) => t.pairs().map(|(k, _)| k.to_val()).collect(),
        Val::Data(Value::Table(t)) => (1..=t.array.len())
            .map(|i| Val::Num(i as f64))
            .chain(t.fields.iter().map(|(k, _)| Val::from(*k)))
            .collect(),
        other => lua_error!(
            "bad argument #1 to 'for iterator' (table expected, got {})",
            other.lua_type()
        ),
    }
}

/// `pairs(t)` of a table value: (key, value).
fn table_pairs(v: &Val) -> Vec<(Val, Val)> {
    match v {
        Val::Table(t) => t.pairs().map(|(k, v)| (k.to_val(), v)).collect(),
        Val::Data(Value::Table(t)) => t
            .array
            .iter()
            .enumerate()
            .map(|(i, x)| (Val::Num(i as f64 + 1.0), Val::from_value(x)))
            .chain(
                t.fields
                    .iter()
                    .map(|(k, x)| (Val::from(*k), Val::from_value(x))),
            )
            .collect(),
        other => lua_error!(
            "bad argument #1 to 'for iterator' (table expected, got {})",
            other.lua_type()
        ),
    }
}

/// `ipairs(t)` of a table value.
fn table_ipairs(v: &Val) -> Vec<Val> {
    match v {
        Val::Table(t) => t.ipairs().into_iter().map(|(_, x)| x).collect(),
        Val::Data(Value::Table(t)) => t
            .array
            .iter()
            .take_while(|x| !matches!(x, Value::Nil))
            .map(Val::from_value)
            .collect(),
        other => lua_error!(
            "bad argument #1 to 'ipairs' (table expected, got {})",
            other.lua_type()
        ),
    }
}

/// A string (or number) value in a concatenation.
fn concat_str(v: &Val, what: &str) -> String {
    match v {
        Val::Str(s) => s.to_string(),
        Val::Num(x) => tostring_num(*x),
        other => lua_error!(
            "attempt to concatenate {what} (a {} value)",
            other.lua_type()
        ),
    }
}

/// A number value in arithmetic or a comparison.
fn need_num(v: &Val, what: &str) -> f64 {
    match v {
        Val::Num(x) => *x,
        Val::Str(_) => v.num(),
        other => lua_error!(
            "attempt to perform arithmetic on {what} (a {} value)",
            other.lua_type()
        ),
    }
}

/// `data.timelessJewelSeedMin[t]` / `SeedMax[t]` / `timelessJewelTypes[t]`.
fn timeless_meta(t: &'static DataValue, jewelType: f64) -> &'static DataValue {
    static DNIL: DataValue = DataValue::Nil;
    t.table().map_or(&DNIL, |t| t.index(jewelType))
}

fn seed_bound(t: &'static DataValue, jewelType: f64) -> f64 {
    timeless_meta(t, jewelType)
        .num()
        .unwrap_or_else(|| lua_error!("bad argument #2 to 'min' (number expected, got nil)"))
}

/// `.. data.timelessJewelTypes[jewelType]` / `.. data.timelessJewelSeedMin[jewelType]` …
fn meta_str(t: &'static DataValue, jewelType: f64) -> String {
    match timeless_meta(t, jewelType) {
        DataValue::Str(s) => s.to_string(),
        DataValue::Num(x) => tostring_num(*x),
        _ => lua_error!("attempt to concatenate field '?' (a nil value)"),
    }
}

/// The `sd` and `stats` of what `resolveAbyssJewelComponent` returned: a legion node
/// (`Node`, `stats` in `extra`) or a legion addition (a table).
enum Changed {
    Node(NodeRef),
    Addition(Table),
}

impl Changed {
    fn sd(&self) -> Vec<LStr> {
        match self {
            Changed::Node(n) => n.sd.get().snapshot(),
            Changed::Addition(t) => str_list(&t.get(n!("sd")), "sd"),
        }
    }
    fn stats(&self) -> Val {
        match self {
            Changed::Node(n) => n.extra.get(n!("stats")),
            Changed::Addition(t) => t.get(n!("stats")),
        }
    }
}

/// A list of strings (`ipairs(x.sd)`).
fn str_list(v: &Val, what: &str) -> Vec<LStr> {
    table_ipairs(v)
        .into_iter()
        .map(|s| match s {
            Val::Str(s) => s,
            Val::Num(x) => LStr::from(tostring_num(x).as_str()),
            other => lua_error!("{what} holds a {} value", other.lua_type()),
        })
        .collect()
}

/// `data.resolveAbyssJewelComponent(component, self.tree.legion)`
/// (Modules/DataAbyssJewelLookUpTableHelper.lua 48-57, ported by LUT against the immutable
/// LegionPassives value) on the tree's legion `Table` (CCR-0020 §8): type 1 → the legion
/// node `component.id + 1 - data.timelessJewelAdditions` (replaces the node), type 2 → the
/// addition `component.id + 1` (adds to it), any other type → nothing.
fn resolve_abyss_component(component: &Val, legion: &Table) -> (Option<Changed>, bool) {
    if !matches!(component, Val::Table(_) | Val::Data(Value::Table(_))) {
        lua_error!("attempt to index local 'component' (a nil value)");
    }
    let ty = component.get(n!("type"));
    let id = || need_num(&component.get(n!("id")), "field 'id'");
    if ty.opt_num() == Some(1.0) {
        let nodes = legion
            .table(n!("nodes"))
            .unwrap_or_else(|| lua_error!("attempt to index field 'nodes' (a nil value)"));
        let node = node_at_index(&nodes, id() + 1.0 - data().timeless.timelessJewelAdditions);
        (node.map(Changed::Node), true)
    } else if ty.opt_num() == Some(2.0) {
        let additions = legion
            .table(n!("additions"))
            .unwrap_or_else(|| lua_error!("attempt to index field 'additions' (a nil value)"));
        let addition = additions.get(id() + 1.0).table();
        (addition.map(Changed::Addition), false)
    } else {
        (None, false)
    }
}

/// `replaceHelperFunc(statToFix, statKey, statMod, value)` (L1234-1251): the local closure
/// of the conquest branch. Returns the first value of `gsub` (callers keep only that).
fn replaceHelperFunc(statToFix: &str, statKey: &Val, statMod: &Val, value: Val) -> String {
    let mut value = value;
    // L1235
    if statMod.get(n!("fmt")).raw_eq(&Val::from("g")) {
        // note the only one we actually care about is "Ritual of Flesh" life regen
        let key = match statKey {
            Val::Str(s) => s.to_string(),
            other => lua_error!(
                "attempt to index local 'statKey' (a {} value)",
                other.lua_type()
            ),
        };
        // L1236
        if pob_lua::find(&key, "per_minute", None, false).is_some() {
            // L1237
            value = Val::Num(round(need_num(&value, "local 'value'") / 60.0, Some(1)));
        } else if pob_lua::find(&key, "permyriad", None, false).is_some() {
            // L1239
            value = Val::Num(need_num(&value, "local 'value'") / 100.0);
        } else if pob_lua::find(&key, "_ms", None, false).is_some() {
            // L1241
            value = Val::Num(need_num(&value, "local 'value'") / 1000.0);
        }
    }
    //if statMod.fmt == "d" then -- only ever d or g, and we want both past here
    let min = statMod.get(n!("min"));
    let max = statMod.get(n!("max"));
    // `gsub`'s replacement argument (a number converts to its `tostring`)
    let repl = |value: &Val| -> String {
        match value {
            Val::Str(s) => s.to_string(),
            Val::Num(x) => tostring_num(*x),
            other => lua_error!(
                "bad argument #3 to 'gsub' (string/function/table expected, got {})",
                other.lua_type()
            ),
        }
    };
    // L1245
    if !min.raw_eq(&max) {
        // L1246
        let pat = format!(
            "%({}%-{}%)",
            concat_str(&min, "field 'min'"),
            concat_str(&max, "field 'max'")
        );
        return pob_lua::gsub(statToFix, &pat, &repl(&value), None).0;
    } else if !min.raw_eq(&value) {
        // only true for might/legacy of the vaal which can combine stats
        // L1248
        let pat = match &min {
            Val::Str(s) => s.to_string(),
            Val::Num(x) => tostring_num(*x),
            other => lua_error!(
                "bad argument #1 to 'gsub' (string expected, got {})",
                other.lua_type()
            ),
        };
        return pob_lua::gsub(statToFix, &pat, &repl(&value), None).0;
    }
    // L1250
    statToFix.to_owned() // if it doesn't need to be changed
}

/// `m_max(m_min(seed, max), min)`: the seed range check of L1272/L1368.
fn seed_in_range(seed: f64, jewelType: f64) -> bool {
    let tm = &data().timeless;
    seed == m_max(
        m_min(seed, seed_bound(&tm.timelessJewelSeedMax, jewelType)),
        seed_bound(&tm.timelessJewelSeedMin, jewelType),
    )
}

/// The `ConPrintf` of an out-of-range seed (L1273/L1369).
fn seed_error(seed: f64, jewelType: f64) {
    let tm = &data().timeless;
    ConPrintf(&format!(
        "ERROR: Seed {} is outside of valid range [{} - {}] for jewel type: {}",
        tostring_num(seed),
        meta_str(&tm.timelessJewelSeedMin, jewelType),
        meta_str(&tm.timelessJewelSeedMax, jewelType),
        meta_str(&tm.timelessJewelTypes, jewelType)
    ));
}

/// `isValueInArray(attributes, node.dn)`.
fn is_attribute(attributes: &[&str], dn: &str) -> bool {
    attributes.contains(&dn)
}

/// One element of `radiusJewels` (L1134-1137); `conqueredBy` is the item's conquest as a
/// table (CCR-0070 §1), converted once.
struct RadiusJewel {
    socket: Option<NodeRef>,
    item: ItemRef,
    conqueredBy: Option<Table>,
}

// ---------------------------------------------------------------------------------------
// BuildAllDependsAndPaths
// ---------------------------------------------------------------------------------------

/// @pob Classes/PassiveSpec.lua:1092-1695 h=a50e22ab
impl PassiveSpec {
    pub fn BuildAllDependsAndPaths(&self) {
        let tree = self.tree.get();
        // L1093: timelessJewelTypeByConqueror (the fn above)
        // This table will keep track of which nodes have been visited during each path-finding attempt
        // L1107
        let visited = Table::new();
        // L1108
        let attributes = ["Dexterity", "Intelligence", "Strength"];
        // Read Abyss changes before resetting the nodes. Zorath needs the currently
        // allocated path from its socket to the class starting node.
        // L1111 (looked up only, never iterated)
        let mut abyssConquests: ahash::AHashMap<i64, Table> = ahash::AHashMap::new();
        // L1112
        for socketId in self.jewels.with(LjIntMap::keys) {
            let Some(itemId) = self.jewels.with(|m| m.get(socketId)) else {
                continue;
            };
            let socketId = socketId as f64;
            // L1113
            let item = self.psb_item(itemId);
            // L1114
            let conqueredBy = item
                .as_ref()
                .and_then(|i| jewel_data(i))
                .map_or(Val::Nil, |jd| jd.get(n!("conqueredBy")));
            // L1115
            let jewelType = if conqueredBy.truthy() {
                timelessJewelTypeByConqueror(&conqueror_type(&conqueredBy))
            } else {
                None
            };
            // L1116
            if let Some(jewelType) = jewelType {
                let item = item.as_ref().expect("conqueredBy implies an item");
                if jewelType >= 7.0
                    && self.psb_alloc(socketId).is_some()
                    && !jewel_data(item)
                        .expect("conqueredBy implies jewelData")
                        .truthy(n!("limitDisabled"))
                {
                    // L1117
                    let path = if jewelType == 11.0 {
                        self.GetShortestPathToClassStart_set(socketId)
                    } else {
                        None
                    };
                    let pathKeys: Option<Vec<f64>> =
                        path.map(|p| p.keys().into_iter().map(|k| k as f64).collect());
                    let seed = need_num(&conqueredBy.get(n!("id")), "field 'id'");
                    // L1118
                    let affected = pob_gamedata::timeless_lut::readAbyssJewelLUT(
                        seed,
                        socketId,
                        jewelType,
                        pathKeys.as_deref(),
                        None,
                    );
                    for (nodeId, modification) in affected.pairs() {
                        // L1119-1123
                        let t = Table::new();
                        t.set(n!("id"), conqueredBy.get(n!("id")));
                        t.set(n!("conqueror"), conqueredBy.get(n!("conqueror")));
                        t.set(n!("modification"), Val::from_value(&modification));
                        abyssConquests.insert(nodeId, t);
                    }
                }
            }
        }

        // gather list of radius jewels
        // L1129
        let mut radiusJewels: Vec<RadiusJewel> = Vec::new();
        // L1130
        for nodeId in self.jewels.with(LjIntMap::keys) {
            let Some(itemId) = self.jewels.with(|m| m.get(nodeId)) else {
                continue;
            };
            let nodeId = nodeId as f64;
            // L1131
            let item = self.psb_item(itemId);
            // L1132
            if let Some(item) = item {
                let jd = jewel_data(&item);
                if item.jewelRadiusIndex.get().is_some()
                    && self.psb_alloc(nodeId).is_some()
                    && jd
                        .as_ref()
                        .is_some_and(|jd| !jd.truthy(n!("limitDisabled")))
                {
                    // L1133
                    let socket = self.psb_node(nodeId);
                    // L1134
                    let conqueredBy =
                        conquest_table(&jd.expect("jewelData").get(n!("conqueredBy")));
                    radiusJewels.push(RadiusJewel {
                        socket,
                        item,
                        conqueredBy,
                    });
                }
            }
        }
        // Check all nodes for other nodes which depend on them (i.e. are only connected to the tree through that node)
        // L1141
        for id in self.nodes.with(LjIntMap::keys) {
            let Some(node) = self.nodes.with(|m| m.get(id)) else {
                continue;
            };
            // L1142
            node.depends.get().wipe();
            // L1143
            match node.extra.get(n!("intuitiveLeapLikesAffecting")) {
                Val::Table(t) => t.wipe(),
                _ => node
                    .extra
                    .set(n!("intuitiveLeapLikesAffecting"), Table::new()),
            }
            // L1144
            node.conqueredBy.set(None);

            // L1146
            if self.extra.truthy(n!("checkNodeLinks")) {
                // Cluster subgraph rebuilds can leave links to replaced node objects,
                // so normalize them to the canonical nodes before rebuilding paths.
                // L1149
                let linked = node.linked.get();
                // L1150
                let mut index = linked.len();
                while index >= 1 {
                    // L1151
                    let other = linked.get(index).expect("index <= #linked");
                    // L1152
                    let canonicalNode = self.psb_node(other.id);
                    // L1153
                    match canonicalNode {
                        None => {
                            // L1154
                            linked.remove(Some(index));
                        }
                        Some(c) if !Rc::ptr_eq(&c, &other) => {
                            // L1156
                            linked.set(index, c);
                        }
                        Some(_) => {}
                    }
                    index -= 1;
                }
            }
            // ignore cluster jewel nodes that don't have an id in the tree
            // L1161
            let treeNode = tree.nodes.get(id);
            // skip updating unallocated masteries which don't have a runegraft
            // L1163
            let isUnallocatedMastery = node.allMasteryOptions.get()
                && node.r#type.get() == n!("Mastery")
                && self.hashOverrides.with(|m| m.get(id)).is_none();
            // L1164
            if let Some(treeNode) = treeNode.filter(|_| !isUnallocatedMastery) {
                // L1165
                self.psc_ReplaceNode(&node, &treeNode);
            }
            // L1167
            node.conqueredBy.set(abyssConquests.get(&id).cloned());

            // L1169
            let ty = node.r#type.get();
            if radiusJewels.len() > 0
                && ty != n!("ClassStart")
                && ty != n!("Socket")
                && node.ascendancyName.get().is_none()
            {
                // L1170
                for radiusJewel in &radiusJewels {
                    // L1171
                    let item = &radiusJewel.item;
                    // L1172
                    let socket = radiusJewel.socket.as_ref();
                    // L1173
                    let radiusIndex = item.jewelRadiusIndex.get();
                    // L1174
                    let nodesInRadius = match (radiusIndex, socket) {
                        (Some(ri), Some(s)) => nodesInRadiusAt(s, ri),
                        (Some(_), None) => {
                            lua_error!("attempt to index local 'socket' (a nil value)")
                        }
                        (None, _) => None,
                    };
                    // L1175
                    if nodesInRadius.is_some_and(|m| m.contains_key(node.id as i64)) {
                        // L1176
                        if item.id.get() != Some(0.0) {
                            let jd = jewel_data(item).expect("radius jewels have jewelData");
                            let socket = socket.expect("in radius of a socket");
                            // L1177
                            if jd.truthy(n!("intuitiveLeapLike"))
                                && !(jd.truthy(n!("intuitiveLeapKeystoneOnly"))
                                    && node.r#type.get() != n!("Keystone"))
                            {
                                // This node depends on Intuitive Leap-like behaviour
                                // This flag:
                                // 1. Prevents generation of paths from this node unless it's also connected to the start
                                // 2. Prevents allocation of path nodes when this node is being allocated
                                // L1182
                                ill_list(&node).push(node_val(socket));
                            }
                            // L1184
                            if let Some(conqueredBy) = &radiusJewel.conqueredBy {
                                // L1185
                                let radiusJewelType = timelessJewelTypeByConqueror(
                                    &conqueredBy.get(n!("conqueror")).get(n!("type")),
                                );
                                // L1186
                                if radiusJewelType.map_or(true, |t| t < 7.0) {
                                    // L1187
                                    node.conqueredBy.set(Some(conqueredBy.clone()));
                                }
                            }
                        }
                    }

                    // L1193
                    let impossibleEscapeKeystones = jewel_data(item)
                        .map_or(Val::Nil, |jd| jd.get(n!("impossibleEscapeKeystones")));
                    // L1194
                    if impossibleEscapeKeystones.truthy() {
                        // L1195 LUA-ORDER: string keys (random in PoB); only the order of
                        // `intuitiveLeapLikesAffecting` can follow it
                        for keyName in table_keys(&impossibleEscapeKeystones) {
                            // L1196
                            let keyNode = match &keyName {
                                Val::Str(s) => tree.keystoneMap.get(Name::from(&**s)).obj::<Node>(),
                                _ => None,
                            };
                            // L1197
                            let inRadius = match (&keyNode, radiusIndex) {
                                (Some(k), Some(ri)) => nodesInRadiusAt(k, ri),
                                _ => None,
                            };
                            // L1198
                            if inRadius.is_some_and(|m| m.contains_key(node.id as i64)) {
                                // L1199 (`t_insert(t, nil)` appends nothing)
                                if let Some(socket) = socket {
                                    ill_list(&node).push(node_val(socket));
                                }
                            }
                        }
                    }
                }
            }
            // L1206
            if node.alloc.get() {
                // L1207
                node.depends.get().set(1, node.clone()); // All nodes depend on themselves
            }
        }
        // L1210
        self.extra.set(n!("checkNodeLinks"), false);
        // L1211
        for id in self.nodes.with(LjIntMap::keys) {
            let Some(node) = self.nodes.with(|m| m.get(id)) else {
                continue;
            };
            // If node is tattooed, replace it
            // L1213
            if let Some(r#override) = self.hashOverrides.with(|m| m.get(node.id as i64)) {
                // L1214
                self.psc_ReplaceNode(&node, &r#override);
                // Runegrafts use mastery nodes, but represent one modifier rather than
                // the usual list of mastery options
                // L1217
                if node.overrideType.get() == Some(n!("AlternateMastery")) {
                    // L1218
                    node.allMasteryOptions.set(false);
                }
            }

            // If node is conquered, replace it or add mods
            // L1223
            let conquered = node.conqueredBy.get();
            if let Some(conqueredBy) = conquered.filter(|_| node.r#type.get() != n!("Socket")) {
                self.conquer_node(&tree, &node, &conqueredBy, &attributes);
            }
        }

        // Add selected mastery effect mods to mastery nodes
        // L1411
        self.allocatedMasteryCount.set(0.0);
        // L1412
        self.allocatedNotableCount.set(0.0);
        // L1413
        self.allocatedKeystoneCount.set(0.0);
        // L1414 (the frozen field is a `Table`: its contents are replaced; CalcSetup.lua:706
        // copies it)
        self.allocatedMasteryTypes.wipe();
        // L1415
        self.allocatedMasteryTypeCount.set(0.0);
        // L1416
        self.allocatedTattooTypes.wipe();
        // L1417
        let masteryReminderText = Table::new();
        masteryReminderText.push("Tip: Right click to select a different effect");
        // L1418
        for id in self.nodes.with(LjIntMap::keys) {
            let Some(node) = self.nodes.with(|m| m.get(id)) else {
                continue;
            };
            let idf = id as f64;
            // L1419
            if self.ignoredNodes.truthy(idf) && self.psb_alloc(idf).is_some() {
                // L1420
                node.alloc.set(false);
                // L1421
                self.allocNodes.with_mut(|m| m.set(id, None));
                // remove once processed to avoid allocation issue after convert
                // L1423
                self.ignoredNodes.set(idf, Val::Nil);
            } else {
                // L1425
                let ty = node.r#type.get();
                let selection = self.masterySelections.with(|m| m.get(id));
                if ty == n!("Mastery") && selection.is_some() {
                    // L1426
                    let effect = tree
                        .masteryEffects
                        .get(selection.expect("selection"))
                        .obj::<Node>();
                    // L1427
                    if let Some(effect) = effect.filter(|_| self.psb_alloc(idf).is_some()) {
                        // L1428
                        if let Some(r#override) = self.hashOverrides.with(|m| m.get(id)) {
                            // L1429
                            self.psc_ReplaceNode(&node, &r#override);
                        } else {
                            // L1431
                            node.sd.set(effect.sd.get());
                        }
                        // L1433
                        node.allMasteryOptions.set(false);
                        // L1434
                        node.extra
                            .set(n!("reminderText"), masteryReminderText.clone());
                        // L1435
                        tree.ProcessStats(&node, None);
                        // L1436
                        self.allocatedMasteryCount
                            .set(self.allocatedMasteryCount.get() + 1.0);
                        // L1437
                        let name = self
                            .psb_alloc(idf)
                            .expect("allocated")
                            .extra
                            .get(n!("name"));
                        let key = match &name {
                            Val::Str(s) => Name::from(&**s),
                            Val::Nil => lua_error!("table index is nil"),
                            other => lua_error!(
                                "PassiveSpec: mastery name is a {} value",
                                other.lua_type()
                            ),
                        };
                        if !self.allocatedMasteryTypes.truthy(key) {
                            // L1438
                            self.allocatedMasteryTypes.set(key, 1.0);
                            // L1439
                            self.allocatedMasteryTypeCount
                                .set(self.allocatedMasteryTypeCount.get() + 1.0);
                        } else {
                            // L1441
                            let prevCount = self.allocatedMasteryTypes.num(key);
                            // L1442
                            self.allocatedMasteryTypes.set(key, prevCount + 1.0);
                            // L1443
                            if prevCount == 0.0 {
                                // L1444
                                self.allocatedMasteryTypeCount
                                    .set(self.allocatedMasteryTypeCount.get() + 1.0);
                            }
                        }
                    // if the tree doesn't recognize the mastery selection or if we have a selection on a mastery that isn't allocated
                    } else {
                        // L1449
                        node.alloc.set(false);
                        // L1450
                        self.allocNodes.with_mut(|m| m.set(id, None));
                        // L1451
                        self.masterySelections.with_mut(|m| m.set(id, None));
                    }
                } else if ty == n!("Mastery")
                    && !node.allMasteryOptions.get()
                    && node.overrideType.get() != Some(n!("AlternateMastery"))
                {
                    // L1454
                    self.AddMasteryEffectOptionsToNode(&node);
                } else if ty == n!("Notable") && node.alloc.get() {
                    // L1456
                    self.allocatedNotableCount
                        .set(self.allocatedNotableCount.get() + 1.0);
                } else if ty == n!("Keystone") && node.alloc.get() {
                    // L1458
                    self.allocatedKeystoneCount
                        .set(self.allocatedKeystoneCount.get() + 1.0);
                }
                // L1460
                if node.isTattoo.get() && node.alloc.get() {
                    if let Some(overrideType) = node.overrideType.get() {
                        // L1461
                        if !self.allocatedTattooTypes.truthy(overrideType) {
                            // L1462
                            self.allocatedTattooTypes.set(overrideType, 1.0);
                        } else {
                            // L1464
                            let c = self.allocatedTattooTypes.num(overrideType);
                            self.allocatedTattooTypes.set(overrideType, c + 1.0);
                        }
                    }
                }
            }
        }

        // L1470
        let mut potentialDeps: LjIntMap<Shared<Vec<NodeRef>>> = LjIntMap::new();
        // L1471
        let mut intuitiveLeaps: LjIntMap<Shared<Vec<NodeRef>>> = LjIntMap::new();
        // L1472 (DeallocSingleNode below sets keys of this table to nil: `next` then skips them)
        for id in self.allocNodes.with(LjIntMap::keys) {
            let Some(node) = self.allocNodes.with(|m| m.get(id)) else {
                continue;
            };
            // L1473
            node.extra.set(n!("visited"), true);
            // L1474
            node.connectedToStart.set(false);
            // L1475
            let ty = node.r#type.get();
            let mut anyStartFound = ty == n!("ClassStart") || ty == n!("AscendClassStart");
            // L1476
            let linked = node.linked.get();
            for_ipairs!((_i, other) in linked => {
                // L1477
                if other.alloc.get() && !in_node_list(&node.depends.get(), &other) {
                    // The other node is allocated and isn't already dependent on this node, so try and find a path to a start node through it
                    // L1479
                    let otherType = other.r#type.get();
                    if otherType == n!("ClassStart") || otherType == n!("AscendClassStart") {
                        // Well that was easy!
                        // L1481
                        anyStartFound = true;
                        // L1482
                        node.connectedToStart.set(true);
                    } else if self.FindStartFromNode(&other, &visited, false) {
                        // We found a path through the other node, therefore the other node cannot be dependent on this node
                        // L1485
                        anyStartFound = true;
                        // L1486
                        node.connectedToStart.set(true);
                        // L1487
                        for_ipairs!((i, n) in visited => {
                            // L1488
                            visited_node(&n).extra.set(n!("visited"), false);
                            // L1489
                            visited.set(i as f64, Val::Nil);
                        });
                    } else {
                        // No path was found, so all the nodes visited while trying to find the path must be dependent on this node
                        // except for mastery nodes that have linked allocated nodes that weren't visited
                        // L1494 (looked up only)
                        let mut depIds: ahash::AHashSet<i64> = ahash::AHashSet::new();
                        // L1495
                        for_ipairs!((_j, n) in visited => {
                            let n = visited_node(&n);
                            // L1496
                            if ill_len(&n) == 0 {
                                // L1497
                                depIds.insert(n.id as i64);
                            }
                        });
                        // L1500
                        for_ipairs!((i, n) in visited => {
                            let n = visited_node(&n);
                            // L1501
                            if n.r#type.get() == n!("Mastery") {
                                // L1502
                                let mut otherPath = false;
                                // L1503
                                let mut allocatedLinkCount = 0.0;
                                // L1504
                                let nlinked = n.linked.get();
                                for_ipairs!((_k, linkedNode) in nlinked => {
                                    // L1505
                                    if linkedNode.alloc.get() {
                                        // L1506
                                        allocatedLinkCount += 1.0;
                                    }
                                });
                                // L1509
                                if allocatedLinkCount > 1.0 {
                                    // L1510
                                    for_ipairs!((_k, linkedNode) in nlinked => {
                                        // L1511
                                        if linkedNode.alloc.get()
                                            && !depIds.contains(&(linkedNode.id as i64))
                                        {
                                            // L1512
                                            otherPath = true;
                                        }
                                    });
                                }
                                // L1516
                                if !otherPath {
                                    // L1517
                                    node.depends.get().push(n.clone());
                                }
                            } else {
                                // If n is dependent on intuitive leap then it may be dependent on this node
                                // L1521
                                if ill_len(&n) > 0 {
                                    // L1522
                                    let nid = n.id as i64;
                                    if potentialDeps.get(nid).is_none() {
                                        // L1523
                                        potentialDeps.set(nid, Some(Shared::new(Vec::new())));
                                    }

                                    // L1526
                                    potentialDeps
                                        .get(nid)
                                        .expect("just set")
                                        .push(node.clone());
                                } else {
                                    // L1528
                                    node.depends.get().push(n.clone());
                                }

                                // If n is a jewel socket containing an intuitive leap-like jewel, nodes in its radius (or the radius of the keystone)
                                // may be dependent on this node if they're found to be unconnected to the start
                                // L1533
                                let key = node.id as i64;
                                if intuitiveLeaps.get(key).is_none() {
                                    // L1534
                                    let r = self.NodesInIntuitiveLeapLikeRadius(&n);
                                    intuitiveLeaps.set(key, Some(Shared::new(r)));
                                } else {
                                    // L1536
                                    for affectedNode in self.NodesInIntuitiveLeapLikeRadius(&n) {
                                        // L1537
                                        intuitiveLeaps
                                            .get(key)
                                            .expect("present")
                                            .push(affectedNode);
                                    }
                                }
                            }
                            // L1541
                            n.extra.set(n!("visited"), false);
                            // L1542
                            visited.set(i as f64, Val::Nil);
                        });
                    }
                }
            });
            // L1547
            node.extra.set(n!("visited"), false);
            // L1548
            if !anyStartFound {
                // No start nodes were found through ANY nodes
                // Therefore this node and all nodes depending on it are orphans and should be pruned
                // L1551
                let depends = node.depends.get();
                for_ipairs!((_i, depNode) in depends => {
                    // L1552
                    let mut prune = true;
                    // L1553
                    for nodeId in self.jewels.with(LjIntMap::keys) {
                        let Some(itemId) = self.jewels.with(|m| m.get(nodeId)) else {
                            continue;
                        };
                        let nodeIdf = nodeId as f64;
                        // L1554
                        if self.psb_alloc(nodeIdf).is_some() {
                            // L1555-1567
                            if itemId != 0.0 && self.supports_dep(nodeIdf, itemId, &depNode) {
                                // Hold off on the pruning; this node could be supported by Intuitive Leap-like jewel
                                // L1569
                                prune = false;
                                // L1570
                                if intuitiveLeaps.get(nodeId).is_none() {
                                    // L1571
                                    intuitiveLeaps.set(nodeId, Some(Shared::new(Vec::new())));
                                }
                                // L1573
                                intuitiveLeaps
                                    .get(nodeId)
                                    .expect("just set")
                                    .push(depNode.clone());
                                // L1574
                                break;
                            }
                        }
                    }
                    // L1578
                    if prune {
                        // L1579
                        self.DeallocSingleNode(&depNode);
                    }
                });
            }
        }

        // All other dependencies resolved, add dependencies to nodes affected by intuitive leap-like jewels
        // Nodes that are unconnected to the start depend on all nodes that connect the jewel socket to the start
        // Nodes that are connected to the start depend on all nodes that connect them *and* the jewel socket to the start
        // In both cases, the nodes can be affected by multiple jewels at the same time
        // L1589
        for (id, deps) in potentialDeps.pairs() {
            // L1590
            let potentialNode = self.psb_node(id as f64);
            // L1591 (looked up only)
            let mut seen: ahash::AHashSet<i64> = ahash::AHashSet::new();
            // L1592
            for_ipairs!((_i, node) in deps => {
                // L1593
                let mut allDep = true;
                // L1594 (`pairs` of a list: index order)
                let pn = potentialNode.as_ref().unwrap_or_else(|| {
                    lua_error!("attempt to index local 'potentialNode' (a nil value)")
                });
                for intuitiveLeapLikeProvider in ill_nodes(pn) {
                    // L1595
                    if !in_node_list(&node.depends.get(), &intuitiveLeapLikeProvider) {
                        // L1596
                        allDep = false;
                    }
                }
                // L1599
                if allDep && !seen.contains(&(node.id as i64)) {
                    // L1600
                    node.depends.get().push(pn.clone());
                    // L1601
                    seen.insert(node.id as i64);
                }
            });
        }

        // L1606
        for (id, deps) in intuitiveLeaps.pairs() {
            // L1607
            let node = self.psb_node(id as f64);
            // L1608 (looked up only)
            let mut seen: ahash::AHashSet<i64> = ahash::AHashSet::new();
            // L1609
            for_ipairs!((_i, dep) in deps => {
                // L1610
                if !dep.connectedToStart.get() {
                    // L1611
                    let mut allDep = true;
                    let node = node.as_ref().unwrap_or_else(|| {
                        lua_error!("attempt to index local 'node' (a nil value)")
                    });
                    // L1612
                    for intuitiveDep in ill_nodes(&dep) {
                        // L1613
                        if !in_node_list(&node.depends.get(), &intuitiveDep) {
                            // L1614
                            allDep = false;
                            // L1615
                            break;
                        }
                    }
                    // L1618
                    if allDep && !seen.contains(&(dep.id as i64)) {
                        // L1619
                        node.depends.get().push(dep.clone());
                        // L1620
                        seen.insert(dep.id as i64);
                    }
                }
            });
        }

        // Reset and rebuild all node paths
        // L1627
        for (_, node) in self.nodes.with(LjIntMap::pairs) {
            // L1628
            node.pathDist
                .set(Some(if node.alloc.get() && ill_len(&node) == 0 {
                    0.0
                } else {
                    1000.0
                }));
            // L1629
            node.path.set(None);
            // L1630
            if node.isJewelSocket.get() || node.expansionJewel.get().is_some() {
                // L1631
                node.distanceToClassStart.set(Some(0.0));
            }
        }
        // L1634
        let mut rootList: Vec<NodeRef> = Vec::new();
        // L1635
        for (_, node) in self.allocNodes.with(LjIntMap::pairs) {
            // L1636
            if ill_len(&node) == 0 || node.connectedToStart.get() {
                // L1637
                rootList.push(node);
            }
        }

        // Use a multi-source 0-1 BFS to find the closest allocated node. Allocated
        // nodes have zero weight, while each unallocated node costs one passive point.
        // L1643: `queue[queueStart..queueLength]` is the live part of the Lua table; a
        // front insertion writes `queue[queueStart - 1]` and a back one `queue[queueLength + 1]`,
        // so a deque of that range is the same queue.
        let mut queue: VecDeque<NodeRef> = VecDeque::new();
        // L1644
        for node in &rootList {
            // L1645
            node.pathDist.set(Some(0.0));
            // L1646
            node.path.set(Some(wipe_path(node.path.get())));
            // L1647
            queue.push_back(node.clone());
        }
        // L1649-1651
        while let Some(node) = queue.pop_front() {
            // L1652-1653
            // L1654
            let linked = node.linked.get().snapshot();
            // L1655
            let nodeDist = node.pathDist.get().unwrap_or_else(|| {
                lua_error!("attempt to perform arithmetic on local 'nodeDist' (a nil value)")
            });
            // L1656
            let nodePath = node.path.get().unwrap_or_else(|| {
                lua_error!("attempt to get length of local 'nodePath' (a nil value)")
            });
            // L1657
            for other in linked {
                // L1658
                // L1659
                let weight = if other.alloc.get() { 0.0 } else { 1.0 };
                // L1660
                let distViaNode = nodeDist + weight;
                // Paths cannot pass through start nodes, cross ascendancies, or move
                // away from masteries. Ascendant paths may leave at distance zero.
                // L1663-1666
                let otherType = other.r#type.get();
                let canTraverse = node.r#type.get() != n!("Mastery")
                    && otherType != n!("ClassStart")
                    && otherType != n!("AscendClassStart")
                    && (node.ascendancyName.get() == other.ascendancyName.get()
                        || (nodeDist == 0.0 && other.ascendancyName.get().is_none()));
                // L1667
                if distViaNode < other.pathDist.get().unwrap_or(f64::INFINITY) && canTraverse {
                    // L1668
                    if weight == 0.0 {
                        // Free nodes go to the front so they can shorten paid paths immediately.
                        // L1670-1671
                        queue.push_front(other.clone());
                    } else {
                        // L1673-1674
                        queue.push_back(other.clone());
                    }

                    // L1677
                    other.pathDist.set(Some(distViaNode));
                    // L1678
                    let path = wipe_path(other.path.get());
                    // L1679-1682 (`nodePath` is `node`'s own list, never `other`'s)
                    let src = nodePath.snapshot();
                    path.with_mut(|p| {
                        p.push(other.clone());
                        p.extend(src);
                    });
                    // L1683
                    other.path.set(Some(path));
                }
            }
        }

        // L1688
        for node in &rootList {
            // L1689
            if node.isJewelSocket.get() || node.expansionJewel.get().is_some() {
                // L1690
                self.SetNodeDistanceToClassStart(node);
            }
        }

        // L1694
        self.BuildSplitPersonalityPath();
    }

    /// L1555-1567: the jewel in socket `nodeId` (item `itemId`) could support `depNode`
    /// (`items[itemId] and (intuitive-leap radius) or (impossible-escape keystone radius)`).
    fn supports_dep(&self, nodeId: f64, itemId: f64, depNode: &NodeRef) -> bool {
        let item = self.psb_item(itemId);
        // `items[itemId] and ( … )`
        let first = match &item {
            Some(item) => {
                let jd = jewel_data(item);
                match (jd, item.jewelRadiusIndex.get()) {
                    (Some(jd), Some(ri)) if jd.truthy(n!("intuitiveLeapLike")) => {
                        let socket = self.psb_node(nodeId).unwrap_or_else(|| {
                            lua_error!("attempt to index field '?' (a nil value)")
                        });
                        match socket.nodesInRadius.get() {
                            None => false,
                            Some(nir) => match nir.get(ri) {
                                Val::Nil => lua_error!("attempt to index field '?' (a nil value)"),
                                v => v
                                    .obj::<LjIntMap<NodeRef>>()
                                    .is_some_and(|m| m.contains_key(depNode.id as i64)),
                            },
                        }
                    }
                    _ => false,
                }
            }
            None => false,
        };
        if first {
            return true;
        }
        // `or ( items[itemId].jewelData and … and self:NodeInKeystoneRadius(…) )`
        let item = item.unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
        let Some(jd) = jewel_data(&item) else {
            return false;
        };
        let keys = jd.get(n!("impossibleEscapeKeystones"));
        if !keys.truthy() {
            return false;
        }
        let keys = match keys {
            Val::Table(t) => t,
            Val::Data(Value::Table(t)) => Table::from_value_table(&t),
            other => lua_error!(
                "bad argument #1 to 'for iterator' (table expected, got {})",
                other.lua_type()
            ),
        };
        self.psc_NodeInKeystoneRadius(
            &keys,
            depNode.id,
            item.jewelRadiusIndex.get().unwrap_or(f64::NAN),
        )
    }

    /// L1224-1406: the conquest branch for one conquered, non-socket node.
    fn conquer_node(
        &self,
        tree: &PassiveTreeRef,
        node: &NodeRef,
        conqueredBy: &Table,
        attributes: &[&str],
    ) {
        let cb = Val::Table(conqueredBy.clone());
        // L1224
        let legion = tree
            .legion
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'legion' (a nil value)"));
        // L1225
        let legionNodes = legion.table(n!("nodes"));
        // L1226
        let legionAdditions = legion.table(n!("additions"));
        let legionNodes = || {
            legionNodes
                .clone()
                .unwrap_or_else(|| lua_error!("attempt to index local 'legionNodes' (a nil value)"))
        };
        let legionAdditions = || {
            legionAdditions.clone().unwrap_or_else(|| {
                lua_error!("attempt to index local 'legionAdditions' (a nil value)")
            })
        };
        let additionsCount = data().timeless.timelessJewelAdditions;

        // L1228
        let conquerorType = conqueror_type(&cb);
        let jewelType = timelessJewelTypeByConqueror(&conquerorType).unwrap_or(5.0);
        // L1229
        let mut seed = cb.get(n!("id"));
        // L1230
        if jewelType == 5.0 {
            // L1231
            seed = Val::Num(need_num(&seed, "local 'seed'") / 20.0);
        }
        let seedNum = || need_num(&seed, "local 'seed'");
        let conqId = || cb.get(n!("id"));

        // L1234: replaceHelperFunc (the fn above)

        let ty = node.r#type.get();
        // L1253
        if jewelType >= 7.0 && ty != n!("Mastery") {
            // L1254
            for component in table_ipairs(&cb.get(n!("modification"))) {
                // L1255
                let (changedNode, replacesNode) = resolve_abyss_component(&component, &legion);
                // L1256
                if let Some(changedNode) = changedNode {
                    // L1257
                    if replacesNode {
                        // L1258
                        if let Changed::Node(n) = &changedNode {
                            self.psc_ReplaceNode(node, n);
                        }
                    }
                    // L1260
                    let stats = changedNode.stats();
                    for (statIndex, statLine) in changedNode.sd().into_iter().enumerate() {
                        let statIndex = statIndex + 1;
                        let mut statLine = statLine.to_string();
                        // L1261 LUA-ORDER: string keys (stat ids; random in PoB, dump order
                        // here); each replacement only touches its own stat's range
                        for (statKey, statMod) in table_pairs(&stats) {
                            // L1262
                            let idx = statMod.get(n!("index"));
                            let roll = component
                                .get(n!("rolls"))
                                .get(Key::from(need_num(&idx, "field 'index'")));
                            statLine = replaceHelperFunc(&statLine, &statKey, &statMod, roll);
                        }
                        // L1264
                        let prefix = if replacesNode { "" } else { " \n" };
                        self.psc_NodeAddition(
                            node,
                            &format!("{prefix}{statLine}"),
                            replacesNode && statIndex == 1,
                        );
                    }
                } else {
                    // L1267
                    ConPrintf(&format!(
                        "Unhandled Abyss component ID: {}",
                        concat_str(&component.get(n!("id")), "field 'id'")
                    ));
                }
            }
        } else if ty == n!("Notable") {
            // L1271
            let mut jewelDataTbl: Vec<f64> = Vec::new();
            // L1272
            if !seed_in_range(seedNum(), jewelType) {
                // L1273
                seed_error(seedNum(), jewelType);
            } else {
                // L1275
                jewelDataTbl = pob_gamedata::timeless_lut::readLUT(
                    need_num(&conqId(), "field 'id'"),
                    node.id,
                    jewelType,
                );
            }
            //print("Need to Update: " .. node.id .. " [" .. node.dn .. "]")
            // L1278
            if jewelDataTbl.is_empty() {
                // L1279
                ConPrintf(&format!(
                    "Missing LUT: {}",
                    meta_str(&data().timeless.timelessJewelTypes, jewelType)
                ));
            } else {
                // L1281
                if jewelType == 1.0 {
                    // L1282
                    let headerSize = jewelDataTbl.len();
                    let tbl = |i: usize| -> Val {
                        jewelDataTbl
                            .get(i.wrapping_sub(1))
                            .map_or(Val::Nil, |&x| Val::Num(x))
                    };
                    // FIXME: complete implementation of this. Need to set roll values for stats
                    //        based on their `fmt` specification
                    // L1285
                    if headerSize == 2 || headerSize == 3 {
                        let legionNode = || {
                            must_node(node_at_index(
                                &legionNodes(),
                                jewelDataTbl[0] + 1.0 - additionsCount,
                            ))
                        };
                        // L1286
                        self.psc_ReplaceNode(node, &legionNode());

                        // L1288
                        for (i, repStat) in legionNode().sd.get().snapshot().into_iter().enumerate()
                        {
                            let i = i + 1;
                            // L1289
                            let statKey = legionNode()
                                .extra
                                .get(n!("sortedStats"))
                                .get(Key::from(i as f64));
                            // L1290
                            let statMod = match &statKey {
                                Val::Nil => Val::Nil,
                                k => legionNode().extra.get(n!("stats")).get(match k {
                                    Val::Str(s) => Key::from(Name::from(&**s)),
                                    Val::Num(x) => Key::from(*x),
                                    _ => lua_error!("bad stat key"),
                                }),
                            };
                            if statMod.is_nil() {
                                lua_error!("attempt to index local 'statMod' (a nil value)");
                            }
                            // L1291
                            let idx = need_num(&statMod.get(n!("index")), "field 'index'");
                            let repStat = replaceHelperFunc(
                                &repStat,
                                &statKey,
                                &statMod,
                                tbl((idx + 1.0) as usize),
                            );
                            // L1292
                            self.psc_NodeAddition(node, &repStat, i == 1); // wipe mods on first run
                        }
                        // should fix the stat values here (note headerSize == 3 has 2 values)
                    } else if headerSize == 6 || headerSize == 8 {
                        // L1296
                        let mut bias = 0.0;
                        // L1297
                        for (i, &val) in jewelDataTbl.iter().enumerate() {
                            let i = (i + 1) as f64;
                            // L1298
                            if i > (headerSize as f64 / 2.0) {
                                // L1299
                                break;
                            } else if val <= 21.0 {
                                // L1301
                                bias += 1.0;
                            } else {
                                // L1303
                                bias -= 1.0;
                            }
                        }
                        // L1306
                        if bias >= 0.0 {
                            // L1307
                            self.psc_ReplaceNode(
                                node,
                                &must_node(node_at_index(&legionNodes(), 77.0)),
                            ); // might of the vaal
                        } else {
                            // L1309
                            self.psc_ReplaceNode(
                                node,
                                &must_node(node_at_index(&legionNodes(), 78.0)),
                            ); // legacy of the vaal
                        }
                        // L1311
                        let mut additions: LjIntMap<f64> = LjIntMap::new();
                        // L1312
                        for (i, &val) in jewelDataTbl.iter().enumerate() {
                            let i = i + 1;
                            // L1313
                            if (i as f64) <= (headerSize as f64 / 2.0) {
                                // L1314
                                let roll = need_num(&tbl(i + headerSize / 2), "local 'roll'");
                                // L1315
                                let key = val as i64;
                                match additions.get(key) {
                                    // L1316
                                    None => additions.set(key, Some(roll)),
                                    // L1318
                                    Some(prev) => additions.set(key, Some(prev + roll)),
                                }
                            } else {
                                // L1321
                                break;
                            }
                        }
                        // L1324
                        for (add, val) in additions.pairs() {
                            // L1325
                            let addition = legionAdditions().get(add as f64 + 1.0);
                            // L1326
                            for addStat in str_list(&addition.get(n!("sd")), "addition.sd") {
                                let mut addStat = addStat.to_string();
                                // L1327 LUA-ORDER: string keys (should only be 1 big)
                                for (k, statMod) in table_pairs(&addition.get(n!("stats"))) {
                                    // should only be 1 big, these didn't get changed so can't just grab index
                                    // L1328
                                    addStat =
                                        replaceHelperFunc(&addStat, &k, &statMod, Val::Num(val));
                                }
                                // L1330
                                self.psc_NodeAddition(node, &addStat, false);
                            }
                        }
                    } else {
                        // L1334
                        ConPrintf(&format!(
                            "Unhandled Glorious Vanity headerSize: {headerSize}"
                        ));
                    }
                } else {
                    // L1337
                    for &jewelData in &jewelDataTbl {
                        // L1338
                        if jewelData >= additionsCount {
                            // replace
                            // L1339
                            let jewelData = jewelData + 1.0 - additionsCount;
                            // L1340
                            let legionNode = node_at_index(&legionNodes(), jewelData);
                            // L1341
                            if let Some(legionNode) = legionNode {
                                // L1342
                                self.psc_ReplaceNode(node, &legionNode);
                            } else {
                                // L1344
                                ConPrintf(&format!(
                                    "Unhandled 'replace' ID: {}",
                                    tostring_num(jewelData)
                                ));
                            }
                        } else {
                            // `elseif jewelData then` (a number: always true)
                            // add
                            // L1347
                            let addition = legionAdditions().get(jewelData + 1.0);
                            // L1348
                            for addStat in str_list(&addition.get(n!("sd")), "addition.sd") {
                                // L1349
                                self.psc_NodeAddition(node, &format!(" \n{addStat}"), false);
                            }
                        }
                        // L1351-1352 (`elseif next(jewelData)`): unreachable for numbers
                    }
                }
            }
        } else if ty == n!("Keystone") {
            // L1358
            let matchStr = format!(
                "{}_keystone_{}",
                concat_str(&conquerorType, "field 'type'"),
                concat_str(&cb.get(n!("conqueror")).get(n!("id")), "field 'id'")
            );
            // L1359
            for (_, legionNode) in legionNodes().ipairs() {
                let legionNode = legionNode
                    .obj::<Node>()
                    .unwrap_or_else(|| lua_error!("legion.nodes holds a non-node value"));
                // L1360
                if node_id_val(&legionNode).raw_eq(&Val::from(matchStr.as_str())) {
                    // L1361
                    self.psc_ReplaceNode(node, &legionNode);
                    // L1362
                    break;
                }
            }
        } else if ty == n!("Normal") {
            let isAttr = || is_attribute(attributes, &node.dn.get()) || node.isTattoo.get();
            // L1366
            if conquerorType.raw_eq(&Val::from("vaal")) {
                // L1367
                let mut jewelDataTbl: Vec<f64> = Vec::new();
                // L1368
                if !seed_in_range(seedNum(), jewelType) {
                    // L1369
                    seed_error(seedNum(), jewelType);
                } else {
                    // L1371
                    jewelDataTbl = pob_gamedata::timeless_lut::readLUT(
                        need_num(&conqId(), "field 'id'"),
                        node.id,
                        jewelType,
                    );
                }
                //print("Need to Update: " .. node.id .. " [" .. node.dn .. "]")
                // L1374
                if jewelDataTbl.is_empty() {
                    // L1375
                    ConPrintf(&format!(
                        "Missing LUT: {}",
                        meta_str(&data().timeless.timelessJewelTypes, jewelType)
                    ));
                } else {
                    let legionNode = || {
                        must_node(node_at_index(
                            &legionNodes(),
                            jewelDataTbl[0] + 1.0 - additionsCount,
                        ))
                    };
                    // L1377
                    self.psc_ReplaceNode(node, &legionNode());
                    // L1378 (`ipairs(node.sd)` iterates the list `node.sd` holds now; the
                    // replacements below assign new lists)
                    for (i, repStat) in node.sd.get().snapshot().into_iter().enumerate() {
                        let i = i + 1;
                        // L1379
                        let statKey = legionNode()
                            .extra
                            .get(n!("sortedStats"))
                            .get(Key::from(i as f64));
                        // L1380
                        let statMod = match &statKey {
                            Val::Nil => Val::Nil,
                            k => legionNode().extra.get(n!("stats")).get(match k {
                                Val::Str(s) => Key::from(Name::from(&**s)),
                                Val::Num(x) => Key::from(*x),
                                _ => lua_error!("bad stat key"),
                            }),
                        };
                        if statMod.is_nil() {
                            lua_error!("attempt to index local 'statMod' (a nil value)");
                        }
                        // L1381
                        let v2 = jewelDataTbl.get(1).map_or(Val::Nil, |&x| Val::Num(x));
                        let repStat = replaceHelperFunc(&repStat, &statKey, &statMod, v2);
                        // L1382
                        self.psc_NodeAddition(node, &repStat, true);
                    }
                }
            } else if conquerorType.raw_eq(&Val::from("karui")) {
                // L1386
                let str = if isAttr() { "2" } else { "4" };
                // L1387
                self.psc_NodeAddition(node, &format!(" \n+{str} to Strength"), false);
            } else if conquerorType.raw_eq(&Val::from("maraketh")) {
                // L1389
                let dex = if isAttr() { "2" } else { "4" };
                // L1390
                self.psc_NodeAddition(node, &format!(" \n+{dex} to Dexterity"), false);
            } else if conquerorType.raw_eq(&Val::from("kalguur")) {
                // L1392
                let ward = if isAttr() { "1" } else { "2" };
                // L1393
                self.psc_NodeAddition(node, &format!(" \n{ward}% increased Ward"), false);
            } else if conquerorType.raw_eq(&Val::from("templar")) {
                // L1395
                if isAttr() {
                    // L1396
                    let legionNode = node_at_index(&legionNodes(), 91.0); // templar_devotion_node
                                                                          // L1397
                    self.psc_ReplaceNode(node, &must_node(legionNode));
                } else {
                    // L1399
                    self.psc_NodeAddition(node, " \n+5 to Devotion", false);
                }
            } else if conquerorType.raw_eq(&Val::from("eternal")) {
                // L1402
                let legionNode = node_at_index(&legionNodes(), 110.0); // eternal_small_blank
                                                                       // L1403
                self.psc_ReplaceNode(node, &must_node(legionNode));
            }
        }
        // L1406
        self.psc_ReconnectNodeToClassStart(node);
    }
}

/// An element of `visited` (a node).
fn visited_node(v: &Val) -> NodeRef {
    v.obj::<Node>()
        .unwrap_or_else(|| lua_error!("attempt to index local 'n' (a nil value)"))
}

/// `wipeTable(t)` of a node's `path` (nil → a fresh list).
fn wipe_path(p: Option<Shared<Vec<NodeRef>>>) -> Shared<Vec<NodeRef>> {
    match p {
        Some(p) => {
            p.wipe();
            p
        }
        None => Shared::new(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::replaceHelperFunc;
    use crate::prelude::*;

    fn stat_mod(fmt: &str, min: f64, max: f64) -> Val {
        let t = Table::new();
        t.set(n!("fmt"), fmt);
        t.set(n!("min"), min);
        t.set(n!("max"), max);
        Val::Table(t)
    }

    #[test]
    fn replace_helper_ranges_and_scaling() {
        let key = Val::from("base_life_regeneration_rate_per_minute");
        // a range is replaced by the (scaled) value
        let s = replaceHelperFunc(
            "Regenerate (1-2) Life per second",
            &key,
            &stat_mod("g", 1.0, 2.0),
            Val::Num(90.0),
        );
        assert_eq!(s, "Regenerate 1.5 Life per second");
        // min == max: the min text is the pattern (only might/legacy of the vaal)
        let key = Val::from("strength");
        let s = replaceHelperFunc(
            "+5 to Strength",
            &key,
            &stat_mod("d", 5.0, 5.0),
            Val::Num(12.0),
        );
        assert_eq!(s, "+12 to Strength");
        // min == value: unchanged
        let s = replaceHelperFunc(
            "+5 to Strength",
            &key,
            &stat_mod("d", 5.0, 5.0),
            Val::Num(5.0),
        );
        assert_eq!(s, "+5 to Strength");
        // permyriad scaling only for fmt "g"
        let key = Val::from("x_permyriad");
        let s = replaceHelperFunc(
            "(10-20)% x",
            &key,
            &stat_mod("g", 10.0, 20.0),
            Val::Num(1500.0),
        );
        assert_eq!(s, "15% x");
        let s = replaceHelperFunc(
            "(10-20)% x",
            &key,
            &stat_mod("d", 10.0, 20.0),
            Val::Num(15.0),
        );
        assert_eq!(s, "15% x");
    }
}
