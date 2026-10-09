//! `StatDescriptions.<scope>` (Data/StatDescriptions/*.lua): kept in dumped
//! form per scope; the stat describer (UI / item tooltips) types them.

use crate::value::DataValue;
use crate::Name;

/// Scope name → dumped table, in dump order.
#[derive(Debug, Clone, Default)]
pub struct StatDescriptionsLazy(pub indexmap::IndexMap<Name, DataValue>);

impl StatDescriptionsLazy {
    pub fn scope(&self, name: &str) -> Option<&DataValue> {
        self.0.get(&Name::from(name))
    }
}
