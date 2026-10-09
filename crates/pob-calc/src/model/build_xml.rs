//! @pob-file Modules/Build.lua @16de4b82d
//! Build load/save (Modules/Build.lua 976-1099, 2222-2335): the calc half of `build:Load`/`Save`
//! and the database text entry points.
//!
//! Owner: BM-A. Stub created by U0: signatures are final (frozen; change through a CCR),
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

/// @pob Modules/Build.lua:976-1015 h=f404e6dd
impl Build {
    pub fn Load(self: &Rc<Self>, xml: &crate::model::XmlNode, fileName: &str) {
        unported!("Modules/Build.lua:976-1015")
    }
}

/// @pob Modules/Build.lua:1017-1098 h=8bce71b8
impl Build {
    pub fn Save(&self, xml: &mut crate::model::XmlNode) {
        unported!("Modules/Build.lua:1017-1098")
    }
}

/// @pob Modules/Build.lua:2222-2260 h=dc6cc23e
impl Build {
    /// → true on error (Lua returns `true` after showing the error).
    pub fn LoadDB(self: &Rc<Self>, xmlText: &str, fileName: &str) -> bool {
        unported!("Modules/Build.lua:2222-2260")
    }
}

/// @pob Modules/Build.lua:2262-2275 h=0ffb09a9
impl Build {
    pub fn LoadDBFile(self: &Rc<Self>) -> bool {
        unported!("Modules/Build.lua:2262-2275")
    }
}

/// @pob Modules/Build.lua:2277-2301 h=e164c85c
impl Build {
    pub fn SaveDB(&self, fileName: &str) -> Option<String> {
        unported!("Modules/Build.lua:2277-2301")
    }
}

/// @pob Modules/Build.lua:2304-2333 h=1efe5bf2
impl Build {
    pub fn SaveDBFile(&self) -> bool {
        unported!("Modules/Build.lua:2304-2333")
    }
}

// @pob Modules/Build.lua:2335-2335 h=a3bceb2e
// (module-level statements: Return)

/// Rust entry point used by tests and the UI: a build from PoB build XML text
/// (`Build:Init` + `LoadDB`).
pub fn loadBuildFromXML(xmlText: &str, data: &'static Data) -> BuildRef {
    let build = Build::new(data);
    let _ = build.LoadDB(xmlText, "");
    build
}
