//! @pob-file ../runtime/lua/xml.lua @16de4b82d
//! runtime/lua/xml.lua: PoB's XML reader/writer (nodes are [`XmlNode`]).
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

use crate::model::XmlNode;
use crate::prelude::*;

// @pob ../runtime/lua/xml.lua:6-6 h=5cce4e82
// (module-level statements: t_insert,t_remove)

/// @pob ../runtime/lua/xml.lua:9-15 h=2c54ba58
pub(crate) fn decodeContent(content: &str) -> String {
    unported!("../runtime/lua/xml.lua:9-15")
}

/// @pob ../runtime/lua/xml.lua:17-23 h=1bdcd3ce
pub(crate) fn encodeContent(text: &str) -> String {
    unported!("../runtime/lua/xml.lua:17-23")
}

/// @pob ../runtime/lua/xml.lua:25-31 h=99f11725
pub(crate) fn parseAttribs(str: &str) -> indexmap::IndexMap<LStr, LStr> {
    unported!("../runtime/lua/xml.lua:25-31")
}

// @pob ../runtime/lua/xml.lua:33-33 h=928320aa
// (module-level statements: xml)

/// @pob ../runtime/lua/xml.lua:35-95 h=2cf8ca6a
/// → (root nodes, error).
pub fn ParseXML(str: &str) -> (Vec<XmlNode>, Option<String>) {
    unported!("../runtime/lua/xml.lua:35-95")
}

/// @pob ../runtime/lua/xml.lua:97-110 h=d230d9c9
pub fn LoadXMLFile(fileName: &str) -> (Vec<XmlNode>, Option<String>) {
    unported!("../runtime/lua/xml.lua:97-110")
}

/// @pob ../runtime/lua/xml.lua:112-158 h=61f2c6cc
pub(crate) fn composeNode(frag: &mut Vec<String>, node: &XmlNode, lvl: usize) {
    unported!("../runtime/lua/xml.lua:112-158")
}

/// @pob ../runtime/lua/xml.lua:160-171 h=c472f833
pub fn ComposeXML(rootNode: &XmlNode) -> String {
    unported!("../runtime/lua/xml.lua:160-171")
}

/// @pob ../runtime/lua/xml.lua:173-185 h=6a525401
pub fn SaveXMLFile(rootNode: &XmlNode, fileName: &str) -> Option<String> {
    unported!("../runtime/lua/xml.lua:173-185")
}
