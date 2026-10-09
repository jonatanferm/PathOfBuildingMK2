//! @pob-file Classes/ModStore.lua @16de4b82d
//! ModStore:EvalMod and getActor (Classes/ModStore.lua 48-54, 363-971; calc doc §5.3).
//! EvalMod MUST NOT mutate a tag; it returns a new Value wherever Lua copies and edits.
//!
//! Owner: MS-B. Stub created by U0: signatures are final (frozen; change through a CCR),
//! bodies are `unported!` until the owner ports them (calc doc D9, §10.2).
#![allow(
    non_snake_case,
    unused_variables,
    unused_imports,
    dead_code,
    clippy::all,
    clippy::pedantic
)]

use crate::prelude::*;

/// @pob Classes/ModStore.lua:48-54 h=ae416723
impl ModStore {
    /// ModStore.lua:48-54.
    pub(crate) fn getActor(&self, actorType: Option<Name>) -> Option<ActorRef> {
        unported!("Classes/ModStore.lua:48-54")
    }
}

/// @pob Classes/ModStore.lua:363-971 h=e55ec1f4
impl ModStore {
    /// `context` is the store the query started on (passed down the parent chain exactly as Lua does).
    pub fn EvalMod(
        &self,
        m: &Mod,
        cfg: Option<&ModCfg>,
        globalLimits: Option<&mut GlobalLimits>,
    ) -> Value {
        unported!("Classes/ModStore.lua:363-971")
    }
}
