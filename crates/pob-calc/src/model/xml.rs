//! @pob-file ../runtime/lua/xml.lua @16de4b82d
//! runtime/lua/xml.lua: PoB's XML reader/writer (nodes are [`XmlNode`]).
//!
//! Owner: BM-A. Every pattern goes through `pob_lua::pattern` (the lstrlib port), so the
//! reader accepts and rejects exactly what PoB's does.
//!
//! Representation (CCR-0080 §7): Lua returns the parse result as a pseudo-root table
//! (`stack[1]`, no `elem`) whose array part holds the top-level element nodes *and* any
//! top-level text runs. The frozen [`ParseXML`] returns only the element nodes;
//! [`ParseXML_root`] keeps the pseudo-root (elem `""`) with both kinds of children, for
//! callers that index `dbXML[1]` (Build.lua:2225). The `empty` flag Lua sets on empty
//! elements is not stored: its one reader (ConfigTab.lua:885) is ported as
//! `children.is_empty()`, which is equivalent (xml.lua sets it exactly when `#top == 0`). Attribute values are always strings in
//! [`XmlNode`], so the type errors of `composeNode` (112-158) cannot occur; a caller writes a
//! Lua `nil`/`false` attribute by leaving the key out.
#![allow(non_snake_case, clippy::all, clippy::pedantic)]

use pob_lua::pattern::{self, Capture};

use crate::model::{XmlChild, XmlNode};
use crate::prelude::*;

// @pob ../runtime/lua/xml.lua:6-6 h=5cce4e82
// (module-level statements: t_insert,t_remove)

/// @pob ../runtime/lua/xml.lua:9-15 h=2c54ba58
pub(crate) fn decodeContent(content: &str) -> String {
    // Decode content, replacing entities
    // L11 local entTbl = {["lt"] = "<", ["gt"] = ">", ["amp"] = "&", ["apos"] = "'", ["quot"] = '"'}
    // L12
    pattern::gsub_fn(
        content,
        "&(.-);",
        |caps| {
            // L13
            let ent = caps.first().map_or("", |c| c.as_str());
            Some(
                match ent {
                    "lt" => "<",
                    "gt" => ">",
                    "amp" => "&",
                    "apos" => "'",
                    "quot" => "\"",
                    _ => "",
                }
                .to_owned(),
            )
        },
        None,
    )
    .0
}

/// @pob ../runtime/lua/xml.lua:17-23 h=1bdcd3ce
pub(crate) fn encodeContent(text: &str) -> String {
    // Encode content, escaping forbidden characters
    // L19 local subTbl = {["<"] = "lt", [">"] = "gt", ["&"] = "amp", ["'"] = "apos", ['"'] = "quot"}
    // L20
    pattern::gsub_fn(
        text,
        "[<>&'\"]",
        |caps| {
            // L21
            let chr = caps.first().map_or("", |c| c.as_str());
            let sub = match chr {
                "<" => "lt",
                ">" => "gt",
                "&" => "amp",
                "'" => "apos",
                "\"" => "quot",
                // the class matches nothing else; `subTbl[chr]` would be nil (a concat error)
                _ => lua_error!("attempt to concatenate a nil value"),
            };
            Some(format!("&{sub};"))
        },
        None,
    )
    .0
}

/// @pob ../runtime/lua/xml.lua:25-31 h=99f11725
pub(crate) fn parseAttribs(str: &str) -> indexmap::IndexMap<LStr, LStr> {
    // L26
    let mut attrib: indexmap::IndexMap<LStr, LStr> = indexmap::IndexMap::new();
    // L27
    let _ = pattern::gsub_fn(
        str,
        "(%w+)=([\"'])(.-)%2",
        |caps| {
            let key = caps[0].to_str();
            let val = caps[2].to_str();
            // L28 (a repeated key keeps its first position: string keys iterate in
            // insertion order, calc doc §3.4)
            attrib.insert(LStr::from(&*key), LStr::from(decodeContent(&val)));
            None
        },
        None,
    );
    // L30
    attrib
}

// @pob ../runtime/lua/xml.lua:33-33 h=928320aa
// (module-level statements: xml)

/// `t_insert(top, v)` on the node at the top of the stack.
fn push_child(stack: &mut [XmlNode], child: XmlChild) {
    stack
        .last_mut()
        .expect("the stack always holds the pseudo-root")
        .children
        .push(child);
}

/// A position capture (`()`) as a 1-based index.
fn pos(c: &Capture<'_>) -> i64 {
    match c {
        Capture::Pos(p) => *p as i64,
        Capture::Str(_) => unreachable!("position capture expected"),
    }
}

/// `xml.ParseXML(str)` keeping Lua's pseudo-root: `(stack[1], nil)` or `(nil, errMsg)`.
/// The root's `elem` is `""`; its children are the top-level nodes and text runs in order.
pub fn ParseXML_root(str: &str) -> Result<XmlNode, String> {
    // L36 Strip comments
    let str = pattern::gsub(str, "<!%-%-.-%-%->", "", None).0;
    // L37
    let top = XmlNode::default();
    // L38 (the stack holds the open nodes; `top` is always its last entry)
    let mut stack: Vec<XmlNode> = vec![top];
    // L39
    let mut i: i64 = 1;
    // L40 local content, cdata, ie, cs, elem, attrib, ce
    // L41
    loop {
        // L42
        let m = pattern::r#match(&str, "^%s*([^<]-)%s*<!%[CDATA%[(.-)%]%]>()", Some(i));
        // L43
        let ie;
        if let Some(caps) = m {
            // Found CDATA section
            let content = caps[0].to_str();
            let cdata = caps[1].to_str();
            ie = pos(&caps[2]);
            // L45
            if pattern::find(&content, "%S", None, false).is_some() {
                // L46
                push_child(
                    &mut stack,
                    XmlChild::Text(LStr::from(decodeContent(&content))),
                );
            }
            // L48
            if pattern::find(&cdata, "%S", None, false).is_some() {
                // L49
                push_child(&mut stack, XmlChild::Text(LStr::from(&*cdata)));
            }
        } else {
            // L52
            let Some(caps) = pattern::r#match(
                &str,
                "^%s*([^<]-)%s*<([%?/]?)([%w:]+)(.-)([%?/]?)>()",
                Some(i),
            ) else {
                // L53 No more tags found
                // L55
                break;
            };
            let content = caps[0].to_str();
            let cs = caps[1].to_str();
            let elem = caps[2].to_str();
            let attrib = caps[3].to_str();
            let ce = caps[4].to_str();
            ie = pos(&caps[5]);
            // L57
            if pattern::find(&content, "%S", None, false).is_some() {
                // L58
                push_child(
                    &mut stack,
                    XmlChild::Text(LStr::from(decodeContent(&content))),
                );
            }
            // L60
            if cs == "?" || ce == "?" {
                // Processing instruction, ignore
            } else if ce == "/" {
                // L62 Empty element
                // L64
                push_child(
                    &mut stack,
                    XmlChild::Node(XmlNode {
                        elem: LStr::from(&*elem),
                        attrib: parseAttribs(&attrib),
                        children: Vec::new(),
                    }),
                );
            } else if cs.is_empty() {
                // L65 Start tag
                // L67
                let top = XmlNode {
                    elem: LStr::from(&*elem),
                    attrib: parseAttribs(&attrib),
                    children: Vec::new(),
                };
                // L68
                stack.push(top);
            } else {
                // L69 End tag
                // L71
                if stack.len() == 1 {
                    // L72
                    return Err(format!("nothing to close with '{elem}'"));
                }
                // L74
                let top_elem = stack.last().expect("non-empty").elem.clone();
                if *top_elem != *elem {
                    // L75
                    return Err(format!("trying to close <{top_elem}> with '{elem}'"));
                }
                // L77 `if #top == 0 then top.empty = true end` (not stored: ConfigTab.lua:885 reads it as `children.is_empty()`)
                // L80
                let closed = stack.pop().expect("non-empty");
                // L81 top = stack[#stack]
                // L82
                push_child(&mut stack, XmlChild::Node(closed));
            }
        }
        // L85
        i = ie;
    }
    // L87
    if stack.len() > 1 {
        // L88
        return Err(format!(
            "unclosed element '{}'",
            stack.last().expect("non-empty").elem
        ));
    }
    // L90
    let content = pattern::r#match(&str, "^%s*(.-)%s*$", Some(i))
        .map(|c| c[0].to_str().into_owned())
        .unwrap_or_default();
    // L91
    if pattern::find(&content, "%S", None, false).is_some() {
        // L92
        stack[0]
            .children
            .push(XmlChild::Text(LStr::from(decodeContent(&content))));
    }
    // L94
    Ok(stack.pop().expect("the pseudo-root"))
}

/// @pob ../runtime/lua/xml.lua:35-95 h=2cf8ca6a
/// → (root nodes, error). Top-level text runs are dropped (see [`ParseXML_root`]).
pub fn ParseXML(str: &str) -> (Vec<XmlNode>, Option<String>) {
    match ParseXML_root(str) {
        Ok(root) => (
            root.children
                .into_iter()
                .filter_map(|c| match c {
                    XmlChild::Node(n) => Some(n),
                    XmlChild::Text(_) => None,
                })
                .collect(),
            None,
        ),
        Err(e) => (Vec::new(), Some(e)),
    }
}

/// @pob ../runtime/lua/xml.lua:97-110 h=d230d9c9
pub fn LoadXMLFile(fileName: &str) -> (Vec<XmlNode>, Option<String>) {
    // L98
    let fileText = match std::fs::read(fileName) {
        Ok(b) => b,
        // L99-100 (`io.open` message: "<name>: <strerror>")
        Err(e) => return (Vec::new(), Some(format!("{fileName}: {e}"))),
    };
    // L102-103 (io.read "*a" reads bytes; the port's strings are UTF-8, invalid bytes become
    // U+FFFD)
    let fileText = String::from_utf8_lossy(&fileText).into_owned();
    // L104 `if not fileText` cannot happen once the file is open
    if fileText.is_empty() {
        // L106-107
        return (Vec::new(), Some(format!("{fileName} file is empty")));
    }
    // L109
    ParseXML(&fileText)
}

/// @pob ../runtime/lua/xml.lua:112-158 h=61f2c6cc
pub(crate) fn composeNode(frag: &mut Vec<String>, node: &XmlNode, lvl: usize) {
    // L113 `type(node.elem) ~= "string"`: always a string here
    // L116
    frag.push("\t".repeat(lvl));
    // L117
    frag.push("<".to_owned());
    // L118
    frag.push(node.elem.to_string());
    // L119 (attributes in insertion order; PoB's `pairs` order is random per process, §3.4)
    // LUA-ORDER
    for (key, val) in &node.attrib {
        // L121 `if val then`: nil/false attributes are absent (module doc)
        // L127
        frag.push(" ".to_owned());
        // L128
        frag.push(key.to_string());
        // L129
        frag.push("=\"".to_owned());
        // L130
        frag.push(encodeContent(val));
        // L131
        frag.push("\"".to_owned());
    }
    // L135
    if node.children.is_empty() {
        // L136
        frag.push("/>\n".to_owned());
        // L137
        return;
    }
    // L139
    frag.push(">\n".to_owned());
    // L140
    for n in &node.children {
        match n {
            // L141
            XmlChild::Node(n) => {
                // L142 (composeNode cannot fail on a typed tree)
                composeNode(frag, n, lvl + 1);
            }
            // L146
            XmlChild::Text(n) => {
                // L147
                frag.push("\t".repeat(lvl + 1));
                // L148
                frag.push(encodeContent(n));
                // L149
                frag.push("\n".to_owned());
            }
        }
    }
    // L154
    frag.push("\t".repeat(lvl));
    // L155
    frag.push("</".to_owned());
    // L156
    frag.push(node.elem.to_string());
    // L157
    frag.push(">\n".to_owned());
}

/// @pob ../runtime/lua/xml.lua:160-171 h=c472f833
pub fn ComposeXML(rootNode: &XmlNode) -> String {
    // L161 `type(rootNode) ~= "table"`: always a node here
    // L164
    let mut frag: Vec<String> = vec!["<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n".to_owned()];
    // L165
    composeNode(&mut frag, rootNode, 0);
    // L169
    frag.concat()
}

/// @pob ../runtime/lua/xml.lua:173-185 h=6a525401
/// → `None` on success (Lua `true`), `Some(errMsg)` on failure (Lua `nil, errMsg`).
pub fn SaveXMLFile(rootNode: &XmlNode, fileName: &str) -> Option<String> {
    // L174
    let text = ComposeXML(rootNode);
    // L178
    let mut fileHnd = match std::fs::File::create(fileName) {
        Ok(f) => f,
        // L179-180
        Err(e) => return Some(format!("{fileName}: {e}")),
    };
    // L182
    if let Err(e) = std::io::Write::write_all(&mut fileHnd, text.as_bytes()) {
        return Some(format!("{fileName}: {e}"));
    }
    // L183 fileHnd:close() (drop)
    // L184
    None
}
