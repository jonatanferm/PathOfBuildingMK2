//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Main.lua:1317-1372 (manifest/update popup, display only).
//! Owner: U17.
//!
//! @pob-file Modules/Main.lua @16de4b82d
//!
//! `main:SetManifestBranch` (:1317-1335) and `main:OpenUpdatePopup` (:1337-1371). "Display
//! only" (ui doc §12 U17 row): the Update button calls `launch:ApplyUpdate`, which is U16's
//! stub (DIVERGENCE(update-stub) in launch.rs). The changelog is read through `cx.sys.fs`
//! (relative paths resolve against the script directory, as Lua's `io` does in SG).
//!
//! DIVERGENCE(lossy-utf8): [`read_lines`] turns bytes that are not UTF-8 into U+FFFD (the
//! port's strings are UTF-8); PoB's changelog.txt and help.txt are valid UTF-8, so no shipped
//! text differs.
//!
//! Props with side effects (ui doc §7.3): none.

use std::rc::Rc;

use pob_ui_core::controls::text_list::TextListRow;
use pob_ui_core::controls::{ButtonControl, TextListControl};
use pob_ui_core::popups::{close_popup, open_popup, PopupOpts};
use pob_ui_core::prelude::*;

use crate::app::CxApp;
use crate::launch;
use crate::main_mode::on_frame::call_mode_target;
use crate::main_mode::settings::{load_xml_file, save_xml_file};
use crate::model::XmlChild;

/// `io.open(name, "r")` + `io.lines(name)`: the file's lines (without the `\n`; a last line
/// without one is still a line), or `None` when it cannot be opened. Bytes that are not UTF-8
/// become U+FFFD (the port's strings are UTF-8; PoB's changelog and help files are).
pub fn read_lines(cx: &Cx, name: &str) -> Option<Vec<String>> {
    let bytes = cx.sys.fs.read(name).ok()?;
    Some(split_lines(&String::from_utf8_lossy(&bytes)))
}

/// The lines `io.lines` yields for a file's text (`\r` is kept, as LuaJIT's `io.lines` does).
fn split_lines(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    // io.lines yields no empty line after a final "\n" (and nothing for an empty file)
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

/// lua: Main.lua:1317-1335 main:SetManifestBranch(branchName)
/// @pob Modules/Main.lua:1317-1335 h=a756d3dc
pub fn set_manifest_branch(cx: &mut Cx, branch_name: &str) {
    // L1318 local xml = require("xml")
    // L1319
    let mut manifest_location = "manifest.xml";
    // L1320
    let mut local_man_xml = load_xml_file(cx, manifest_location).ok();
    // L1321
    if local_man_xml.is_none() {
        // L1322
        manifest_location = "../manifest.xml";
        // L1323
        local_man_xml = load_xml_file(cx, manifest_location).ok();
    }
    // L1325 if localManXML and localManXML[1].elem == "PoBVersion" then
    if let Some(man) = local_man_xml.as_mut() {
        let first = man
            .children
            .first_mut()
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
        if let XmlChild::Node(root) = first {
            if &*root.elem == "PoBVersion" {
                // L1326
                for node in &mut root.children {
                    // L1327
                    if let XmlChild::Node(node) = node {
                        // L1328
                        if &*node.elem == "Version" {
                            // L1329 (a new key goes last; an existing one keeps its place)
                            node.attrib.insert("branch".into(), branch_name.into());
                        }
                    }
                }
            }
        }
    }
    // L1334 xml.SaveXMLFile(localManXML[1], manifestLocation)
    let Some(man) = local_man_xml else {
        lua_error!("attempt to index local 'localManXML' (a nil value)")
    };
    let root = match man.children.first() {
        Some(XmlChild::Node(n)) => n.clone(),
        // xml.SaveXMLFile of a text run / nil: composeNode indexes `node.elem` of a string or nil
        _ => lua_error!("attempt to index local 'node' (a nil value)"),
    };
    // (SaveXMLFile's `nil, errMsg` return is discarded)
    let _ = save_xml_file(cx, &root, manifest_location);
}

/// lua: Main.lua:1337-1371 main:OpenUpdatePopup()
/// @pob Modules/Main.lua:1337-1371 h=a4f71daa
pub fn open_update_popup(cx: &mut Cx) {
    // L1338
    let mut change_list: Vec<Rc<TextListRow>> = Vec::new();
    // L1339
    let changelog_name = if cx.app().launch.dev_mode {
        "../changelog.txt"
    } else {
        "changelog.txt"
    };
    // L1340-1342 (io.open, then close; io.lines reads it again)
    if let Some(lines) = read_lines(cx, changelog_name) {
        let version_number = cx.app().launch.version_number.clone();
        // L1343
        for line in lines {
            // L1344
            let m = pob_lua::lmatch(&line, "^VERSION%[(.+)%]%[(.+)%]$", None);
            // L1345
            if let Some(c) = m {
                let ver = c[0].to_str().into_owned();
                let date = c[1].to_str().into_owned();
                // L1346
                if Some(&ver) == version_number.as_ref() {
                    // L1347
                    break;
                }
                // L1349
                if !change_list.is_empty() {
                    // L1350
                    change_list.push(Rc::new(TextListRow::new(12.0, &[])));
                }
                // L1352
                change_list.push(Rc::new(TextListRow::new(
                    20.0,
                    &[&format!("^7Version {ver} ({date})")],
                )));
            } else {
                // L1354
                change_list.push(Rc::new(TextListRow::new(14.0, &[&format!("^7{line}")])));
            }
        }
    }
    // L1359 local controls = { }
    // L1369
    open_popup(
        cx,
        800.0,
        600.0,
        "Update Available",
        move |cx, p| {
            // L1360
            cx.add(
                p,
                "changeLog",
                TextListControl::new(Anchor::NONE, rect(0, 20, 780, 542), None, change_list, None),
            );
            // L1361
            cx.add(
                p,
                "update",
                ButtonControl::new(Anchor::NONE, rect(-45, 570, 80, 20), "Update", |cx| {
                    // L1362
                    close_popup(cx);
                    // L1363 self:CallMode("CanExit", "UPDATE")
                    let ret = call_mode_target(cx).and_then(|m| m.can_exit(cx, "UPDATE"));
                    // L1364
                    if ret.is_none() || ret == Some(true) {
                        // L1365 launch:ApplyUpdate(launch.updateAvailable)
                        // (ApplyUpdate does nothing for a mode other than "basic"/"normal",
                        // nil included)
                        if let Some(mode) = cx.app().launch.update_available.clone() {
                            launch::apply_update(cx, &mode);
                        }
                    }
                }),
            );
            // L1368
            cx.add(
                p,
                "cancel",
                ButtonControl::new(Anchor::NONE, rect(45, 570, 80, 20), "Cancel", |cx| {
                    // L1369
                    close_popup(cx);
                }),
            );
        },
        PopupOpts::default(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An in-memory `Fs` (relative paths as keys).
    #[derive(Clone, Default)]
    struct MemFs(Rc<std::cell::RefCell<std::collections::HashMap<String, Vec<u8>>>>);
    impl pob_sg::Fs for MemFs {
        fn read(&self, path: &str) -> std::io::Result<Vec<u8>> {
            self.0
                .borrow()
                .get(path)
                .cloned()
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
        }
        fn write(&mut self, path: &str, data: &[u8]) -> std::io::Result<()> {
            self.0.borrow_mut().insert(path.to_owned(), data.to_vec());
            Ok(())
        }
        fn search(&self, _spec: &str, _find_dirs: bool) -> Vec<pob_sg::FileEntry> {
            Vec::new()
        }
        fn make_dir(&mut self, _path: &str) -> std::io::Result<()> {
            Ok(())
        }
        fn remove_dir(&mut self, _path: &str, _recursive: bool) -> std::io::Result<()> {
            Ok(())
        }
        fn rename(&mut self, _from: &str, _to: &str) -> std::io::Result<()> {
            Ok(())
        }
        fn remove(&mut self, _path: &str) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn cx_with(files: &[(&str, &str)]) -> (Cx, MemFs) {
        let mut cx = Cx::headless();
        let fs = MemFs::default();
        for (k, v) in files {
            fs.0.borrow_mut()
                .insert((*k).to_owned(), v.as_bytes().to_vec());
        }
        cx.sys.fs = Box::new(fs.clone());
        (cx, fs)
    }

    fn file(fs: &MemFs, name: &str) -> Option<String> {
        fs.0.borrow()
            .get(name)
            .map(|b| String::from_utf8(b.clone()).unwrap())
    }

    /// Main.lua:1317-1335: `branch` is set on every `<Version>` of a `<PoBVersion>` root (kept in
    /// place when present, appended otherwise) and the file is written back where it was found
    /// (`manifest.xml`, else `../manifest.xml`).
    #[test]
    fn set_manifest_branch_rewrites_version() {
        let man = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<PoBVersion>\n<Version number=\"2.67.2\"/>\n<Source part=\"program\" url=\"https://example/\"/>\n</PoBVersion>\n";
        let (mut cx, fs) = cx_with(&[("../manifest.xml", man)]);
        set_manifest_branch(&mut cx, "beta");
        assert!(file(&fs, "manifest.xml").is_none());
        let out = file(&fs, "../manifest.xml").unwrap();
        let root = crate::model::xml::ParseXML_root(&out).unwrap();
        let XmlChild::Node(top) = &root.children[0] else {
            panic!("no root element")
        };
        assert_eq!(&*top.elem, "PoBVersion");
        let version = top
            .children
            .iter()
            .find_map(|c| match c {
                XmlChild::Node(n) if &*n.elem == "Version" => Some(n),
                _ => None,
            })
            .unwrap();
        let attrs: Vec<(String, String)> = version
            .attrib
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        assert_eq!(
            attrs,
            vec![
                ("number".to_owned(), "2.67.2".to_owned()),
                ("branch".to_owned(), "beta".to_owned())
            ]
        );
        // a second call keeps the attribute's place and replaces its value
        set_manifest_branch(&mut cx, "master");
        let again = file(&fs, "../manifest.xml").unwrap();
        assert!(
            again.contains("number=\"2.67.2\" branch=\"master\""),
            "{again}"
        );

        // `manifest.xml` wins over `../manifest.xml`
        let (mut cx, fs) = cx_with(&[("manifest.xml", man), ("../manifest.xml", man)]);
        set_manifest_branch(&mut cx, "beta");
        assert!(file(&fs, "manifest.xml")
            .unwrap()
            .contains("branch=\"beta\""));
        assert_eq!(file(&fs, "../manifest.xml").unwrap(), man);

        // another root element is written back without a branch
        let other = "<Other>\n<Version number=\"1\"/>\n</Other>\n";
        let (mut cx, fs) = cx_with(&[("manifest.xml", other)]);
        set_manifest_branch(&mut cx, "beta");
        assert!(!file(&fs, "manifest.xml").unwrap().contains("branch"));
    }

    /// No manifest at all: `localManXML[1]` indexes nil (a Lua error).
    #[test]
    fn set_manifest_branch_without_manifest_errors() {
        let (mut cx, _fs) = cx_with(&[]);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            set_manifest_branch(&mut cx, "beta");
        }));
        let msg = pob_host::panic_message(&*r.unwrap_err());
        assert!(msg.contains("localManXML"), "{msg}");
    }

    #[test]
    fn io_lines_shape() {
        assert_eq!(split_lines("a\nb\n"), vec!["a", "b"]);
        assert_eq!(split_lines("a\n\nb"), vec!["a", "", "b"]);
        assert_eq!(split_lines("a\r\nb"), vec!["a\r", "b"]);
        assert_eq!(split_lines("").len(), 0);
        assert_eq!(split_lines("\n"), vec![""]);
    }
}
