//! `Character` — the user's configuration. Phase 2 minimal version: class, level, and
//! allocated tree node ids.

use std::collections::HashSet;

use pob_data::{Class, ItemSet, NodeId, PassiveTree};

/// Reference to a class within a `PassiveTree`. Either the index (faster, fragile across
/// tree versions) or the name (slower, version-portable). We canonicalise on name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClassRef(pub String);

impl ClassRef {
    pub fn marauder() -> Self { Self("Marauder".into()) }
    pub fn ranger() -> Self { Self("Ranger".into()) }
    pub fn witch() -> Self { Self("Witch".into()) }
    pub fn duelist() -> Self { Self("Duelist".into()) }
    pub fn templar() -> Self { Self("Templar".into()) }
    pub fn shadow() -> Self { Self("Shadow".into()) }
    pub fn scion() -> Self { Self("Scion".into()) }
}

#[derive(Debug, Clone, Default)]
pub struct Character {
    pub class: ClassRef,
    pub ascendancy: Option<String>,
    pub level: u32,
    pub allocated: HashSet<NodeId>,
    pub items: ItemSet,
}

impl Default for ClassRef {
    fn default() -> Self {
        Self::scion()
    }
}

impl Character {
    pub fn new(class: ClassRef, level: u32) -> Self {
        Self {
            class,
            ascendancy: None,
            level,
            allocated: HashSet::new(),
            items: ItemSet::new(),
        }
    }

    pub fn allocate(&mut self, node: NodeId) {
        self.allocated.insert(node);
    }

    /// Find the `Class` definition in the tree.
    pub fn resolve_class<'a>(&self, tree: &'a PassiveTree) -> Option<&'a Class> {
        tree.classes.iter().find(|c| c.name == self.class.0)
    }
}
