//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/BuildListHelpers.lua 1-201, plus
//! Common.lua's `naturalSortCompare` (672-704). Owner: U18.
//!
//! @pob-file Modules/BuildListHelpers.lua @16de4b82d
//! @pob-file Modules/Common.lua @16de4b82d
//!
//! The module is `require`d by BuildList.lua, BuildListControl.lua and CompareTab.lua; its
//! returned table is the `pub` items here. A build-list entry (`{ fileName = .., subPath = .., ..
//! }`) is an [`Entry`] behind an `Rc`: Lua compares entries by identity (`selValue ~= build`,
//! `copyBuild`), which [`pob_ui_core::controls::list::ListValue`] keys on the `Rc` pointer.
//!
//! `naturalSortCompare` (Common.lua:673-704) has no owner yet (ui doc §13 item 15: the first
//! consuming unit); U18 is the first consumer that ports it.
//!
//! DIVERGENCE(none-observable): `table.sort` is `pob_lua::sort` (LuaJIT's `auxsort`, comparison
//! for comparison), so ties sort exactly as PoB sorts them.

use std::rc::Rc;

use pob_lua::pattern;
use pob_lua::string::lower;
use pob_ui_core::popups;
use pob_ui_core::prelude::*;

use crate::launch::launch_info;
use crate::platform::storage::{self, build_path};

// @pob-skip Modules/BuildListHelpers.lua:1-9 reason="header comment and locals"

/// One entry of the build index (`ScanFolder`'s list): a build file or a folder.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    /// `fileName` (builds only)
    pub file_name: Option<String>,
    /// `subPath`
    pub sub_path: String,
    /// `fullFileName`
    pub full_file_name: String,
    /// `modified` (`GetFileModifiedTime()`)
    pub modified: f64,
    /// `buildName` (builds only)
    pub build_name: Option<String>,
    /// `level` (builds; nil when the header has none)
    pub level: Option<f64>,
    /// `className`
    pub class_name: Option<String>,
    /// `ascendClassName`
    pub ascend_class_name: Option<String>,
    /// `folderName` (folders only)
    pub folder_name: Option<String>,
}

/// An entry as the lists hold it (Lua table identity).
pub type EntryRef = Rc<Entry>;

/// One `buildSortDropList` row: `{ label = .., sortMode = .. }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SortDropItem {
    pub label: &'static str,
    pub sort_mode: &'static str,
}

/// `buildSortDropList`
/// @pob Modules/BuildListHelpers.lua:10-15 h=bd75bcd1
pub const BUILD_SORT_DROP_LIST: [SortDropItem; 4] = [
    // L11
    SortDropItem {
        label: "Sort by Name",
        sort_mode: "NAME",
    },
    // L12
    SortDropItem {
        label: "Sort by Class",
        sort_mode: "CLASS",
    },
    // L13
    SortDropItem {
        label: "Sort by Last Edited",
        sort_mode: "EDITED",
    },
    // L14
    SortDropItem {
        label: "Sort by Level",
        sort_mode: "LEVEL",
    },
];

/// `buildSortDropList` as DropDownControl rows (the value of each is its [`SortDropItem`]).
pub fn build_sort_drop_list() -> Vec<pob_ui_core::controls::drop_down::DropItem> {
    BUILD_SORT_DROP_LIST
        .iter()
        .map(|i| pob_ui_core::controls::drop_down::DropItem::new(i.label, Rc::new(i.clone())))
        .collect()
}

/// `ReadBuildHeader`'s table.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BuildHeader {
    pub level: Option<f64>,
    pub class_name: Option<String>,
    pub ascend_class_name: Option<String>,
}

/// The first capture of `s:match(p)` as a string.
fn match1(s: &[u8], p: &str) -> Option<String> {
    pattern::match_bytes(s, p.as_bytes(), None)
        .map(|c| String::from_utf8_lossy(c[0].as_bytes()).into_owned())
}

/// `ReadBuildHeader(fullFileName)`: `None` is Lua's nil (an empty file: the cloud error popup
/// opened).
/// @pob Modules/BuildListHelpers.lua:17-35 h=3313ce57
pub fn read_build_header(cx: &mut Cx, full_file_name: &str) -> Option<BuildHeader> {
    // L18-L21
    let header_text = match storage::read_head(cx, full_file_name, 2048) {
        // L19
        Err(_) => return Some(BuildHeader::default()),
        Ok(t) => t,
    };
    // L22
    let Some(header_text) = header_text else {
        // L23
        let info = launch_info(cx);
        popups::open_cloud_error_popup(cx, full_file_name, &info);
        // L24
        return None;
    };

    // L27
    let Some(build_tag) = pattern::match_bytes(&header_text, b"<Build%s+[^>]->", None)
        .map(|c| c[0].as_bytes().to_vec())
    else {
        // L28
        return Some(BuildHeader::default());
    };

    // L30
    Some(BuildHeader {
        // L31
        level: pattern::match_bytes(&build_tag, br#"level="([^"]+)""#, None)
            .and_then(|c| pob_lua::format::tonumber_bytes(c[0].as_bytes())),
        // L32
        class_name: match1(&build_tag, r#"className="([^"]+)""#),
        // L33
        ascend_class_name: match1(&build_tag, r#"ascendClassName="([^"]+)""#),
    })
}

/// `s:lower():find(term, 1, true)` of an optional field (nil short-circuits the `and`).
fn field_has(field: Option<&str>, term: &str) -> bool {
    field.is_some_and(|f| pattern::find(&lower(f), term, Some(1), true).is_some())
}

/// `MatchEntry(entry, terms)`
/// @pob Modules/BuildListHelpers.lua:37-58 h=67b6c0b0
pub fn match_entry(entry: &Entry, terms: &[String]) -> bool {
    // L38
    for term in terms {
        // L39
        let term_lower = lower(term);
        // L40
        let val =
            pattern::r#match(&term_lower, "^class:(.*)$", None).map(|c| c[0].to_str().into_owned());
        // L41
        if let Some(val) = val {
            // L42
            if !val.is_empty() {
                // L43-L45
                let m = field_has(entry.class_name.as_deref(), &val)
                    || field_has(entry.ascend_class_name.as_deref(), &val)
                    || field_has(entry.folder_name.as_deref(), &val);
                // L46
                if !m {
                    return false;
                }
            }
        } else {
            // L49-L53
            let m = field_has(entry.build_name.as_deref(), &term_lower)
                || field_has(entry.folder_name.as_deref(), &term_lower)
                || field_has(entry.class_name.as_deref(), &term_lower)
                || field_has(entry.ascend_class_name.as_deref(), &term_lower)
                || field_has(Some(&entry.sub_path), &term_lower);
            // L54
            if !m {
                return false;
            }
        }
    }
    // L57
    true
}

/// `scanDir(currentSubPath)` (the local function of ScanFolder, L65-L107).
fn scan_dir(cx: &mut Cx, list: &mut Vec<EntryRef>, current_sub_path: &str) {
    let bp = build_path(cx);
    // L66
    let handle = storage::new_file_search(cx, &format!("{bp}{current_sub_path}*.xml"), false);
    // L67
    for h in handle {
        // L68
        let file_name = h.name;
        // L69
        let full_file_name = format!("{bp}{current_sub_path}{file_name}");
        // L70
        let header = read_build_header(cx, &full_file_name);
        // L71
        if let Some(header) = header {
            // L72-L81
            let build_name = pattern::gsub(&file_name, "%.xml$", "", None).0;
            list.push(Rc::new(Entry {
                file_name: Some(file_name),
                sub_path: current_sub_path.to_owned(),
                full_file_name,
                modified: h.modified as f64,
                build_name: Some(build_name),
                level: header.level,
                class_name: header.class_name,
                ascend_class_name: header.ascend_class_name,
                folder_name: None,
            }));
        }
        // L84 (NextFile)
    }

    // L87
    let handle = storage::new_file_search(cx, &format!("{bp}{current_sub_path}*"), true);
    // L88
    let mut sub_folders: Vec<(String, f64)> = Vec::new();
    // L89
    for h in handle {
        // L90-L92
        sub_folders.push((h.name, h.modified as f64));
        // L93 (NextFile)
    }

    // L96
    for (folder_name, modified) in sub_folders {
        // L98
        let next_sub_path = format!("{current_sub_path}{folder_name}/");
        // L99-L104
        list.push(Rc::new(Entry {
            full_file_name: format!("{bp}{current_sub_path}{folder_name}"),
            folder_name: Some(folder_name),
            sub_path: current_sub_path.to_owned(),
            modified,
            ..Entry::default()
        }));
        // L105
        scan_dir(cx, list, &next_sub_path);
    }
}

/// `ScanFolder(subPath)`: recursively index builds and folders below `main.buildPath..subPath`.
/// @pob Modules/BuildListHelpers.lua:60-111 h=96f5f216
pub fn scan_folder(cx: &mut Cx, sub_path: Option<&str>) -> Vec<EntryRef> {
    // L62
    let sub_path = sub_path.unwrap_or("").to_owned();
    // L63
    let mut list = Vec::new();
    // L109
    scan_dir(cx, &mut list, &sub_path);
    // L110
    list
}

/// `FilterList(index, subPath, filterText)`: filtering the cached index avoids filesystem work on
/// every keystroke.
/// @pob Modules/BuildListHelpers.lua:113-127 h=aa4e6327
pub fn filter_list(
    index: Option<&[EntryRef]>,
    sub_path: &str,
    filter_text: Option<&str>,
) -> Vec<EntryRef> {
    // L115
    let mut terms: Vec<String> = Vec::new();
    // L116
    for term in pattern::gmatch(filter_text.unwrap_or(""), "%S+") {
        // L117
        terms.push(term[0].to_str().into_owned());
    }

    // L120
    let mut list = Vec::new();
    // L121
    for entry in index.unwrap_or(&[]) {
        // L122
        if (terms.is_empty() && entry.sub_path == sub_path)
            || (!terms.is_empty() && match_entry(entry, &terms))
        {
            // L123
            list.push(entry.clone());
        }
    }
    // L126
    list
}

/// `CanMoveToSubPath(build, targetSubPath)`
/// @pob Modules/BuildListHelpers.lua:129-137 h=27ca5a54
pub fn can_move_to_sub_path(build: &Entry, target_sub_path: &str) -> bool {
    // L130
    if build.sub_path == target_sub_path {
        return false;
    }
    // L131
    if let Some(folder_name) = &build.folder_name {
        // L132-L133 MoveFolder and CopyFolder recurse, so their destination cannot be inside the source.
        let source_sub_path = format!("{}{folder_name}/", build.sub_path);
        // L134
        if pob_ui_core::luastr::sub(target_sub_path, 1.0, Some(source_sub_path.len() as f64))
            == source_sub_path.as_str()
        {
            return false;
        }
    }
    // L136
    true
}

/// `(a.subPath or "") .. (a.folderName or a.fileName or "")`
fn path_name(e: &Entry) -> String {
    format!(
        "{}{}",
        e.sub_path,
        e.folder_name
            .as_deref()
            .or(e.file_name.as_deref())
            .unwrap_or("")
    )
}

/// The comparator of `SortList` (L142-L192).
fn sort_lt(a: &Entry, b: &Entry, sort_mode: Option<&str>) -> bool {
    // L143
    let a_is_folder = a.folder_name.is_some();
    // L144
    let b_is_folder = b.folder_name.is_some();

    // L146
    if a_is_folder && !b_is_folder {
        return true;
    }
    // L147
    if !a_is_folder && b_is_folder {
        return false;
    }

    // L149
    let a_path_name = path_name(a);
    // L150
    let b_path_name = path_name(b);

    // L152
    if sort_mode == Some("EDITED") {
        // L153
        let mod_a = a.modified;
        // L154
        let mod_b = b.modified;
        // L155
        if mod_a != mod_b {
            // L156
            return mod_a > mod_b;
        }
        // L158
        return natural_sort_compare(&a_path_name, &b_path_name);
    }

    // L161
    if a_is_folder {
        // L162
        natural_sort_compare(&a_path_name, &b_path_name)
    } else if sort_mode == Some("CLASS") {
        // L165
        let a_has_class = a.class_name.is_some();
        // L166
        let b_has_class = b.class_name.is_some();
        // L167
        if !a_has_class && b_has_class {
            return true;
        } else if a_has_class && !b_has_class {
            // L168
            return false;
        } else if a_has_class && b_has_class && a.class_name != b.class_name {
            // L169-L170
            return a.class_name < b.class_name;
        }

        // L173
        let a_has_asc = a.ascend_class_name.is_some();
        // L174
        let b_has_asc = b.ascend_class_name.is_some();
        // L175
        if !a_has_asc && b_has_asc {
            return true;
        } else if a_has_asc && !b_has_asc {
            // L176
            return false;
        } else if a_has_asc && b_has_asc && a.ascend_class_name != b.ascend_class_name {
            // L177-L178
            return a.ascend_class_name < b.ascend_class_name;
        }
        // L180
        natural_sort_compare(&a_path_name, &b_path_name)
    } else if sort_mode == Some("LEVEL") {
        // L182
        match (a.level, b.level) {
            (Some(_), None) => return false,
            // L183
            (None, Some(_)) => return true,
            // L184
            (Some(al), Some(bl)) => {
                // L185
                if al != bl {
                    return al < bl;
                }
            }
            (None, None) => {}
        }
        // L187
        natural_sort_compare(&a_path_name, &b_path_name)
    } else {
        // L189
        natural_sort_compare(&a_path_name, &b_path_name)
    }
}

/// `SortList(list, sortMode)`: sort the given list in place using the same rules as the startup
/// build list. sortMode: "NAME" (default), "CLASS", "EDITED", or "LEVEL".
/// @pob Modules/BuildListHelpers.lua:139-193 h=79a2bca6
pub fn sort_list(list: &mut [EntryRef], sort_mode: Option<&str>) {
    // L142
    pob_lua::sort(list, |a, b| sort_lt(a, b, sort_mode));
}

// @pob-skip Modules/BuildListHelpers.lua:195-201 reason="the module's return table (the pub items above)"

/// `aStr`, `aNum`, `aEnd` of `a:match("(.-)(%d+)()", aIndex)`.
fn nat_part(s: &str, index: usize) -> Option<(Vec<u8>, Vec<u8>, usize)> {
    let c = pattern::match_bytes(s.as_bytes(), b"(.-)(%d+)()", Some(index as i64))?;
    let end = match c[2] {
        pattern::Capture::Pos(p) => p,
        pattern::Capture::Str(_) => unreachable!("position capture"),
    };
    Some((c[0].as_bytes().to_vec(), c[1].as_bytes().to_vec(), end))
}

/// `naturalSortCompare(a, b)`: Natural sort comparator (Common.lua). Lua string `<` is a byte
/// comparison (`strcoll` in the C locale).
/// @pob Modules/Common.lua:672-704 h=639c32a8
pub fn natural_sort_compare(a: &str, b: &str) -> bool {
    // L674
    let (mut a_index, mut b_index) = (1usize, 1usize);
    // L675
    loop {
        // L676
        let am = nat_part(a, a_index);
        // L677
        let bm = nat_part(b, b_index);
        let (mut a_str, mut b_str) = (
            am.as_ref().map(|m| m.0.clone()),
            bm.as_ref().map(|m| m.0.clone()),
        );
        let a_num = am.as_ref().map(|m| m.1.clone());
        let b_num = bm.as_ref().map(|m| m.1.clone());
        let a_end = am.as_ref().map(|m| m.2);
        let b_end = bm.as_ref().map(|m| m.2);
        // L678
        if a_str.is_none() || b_str.is_none() {
            // L679
            a_str =
                Some(pob_ui_core::luastr::sub_bytes(a.as_bytes(), a_index as f64, None).to_vec());
            // L680
            b_str =
                Some(pob_ui_core::luastr::sub_bytes(b.as_bytes(), b_index as f64, None).to_vec());
        }
        let (a_str, b_str) = (a_str.expect("set"), b_str.expect("set"));
        // L682
        let al = pob_lua::string::upper_bytes(&a_str);
        // L683
        let bl = pob_lua::string::upper_bytes(&b_str);
        // L684
        if al != bl {
            // L685
            return al < bl;
        }
        // L687
        if a_str != b_str {
            // L688
            return a_str < b_str;
        }
        // L690
        let Some(a_num) = a_num else {
            // L691
            return a.as_bytes() < b.as_bytes();
        };
        // L693
        let an = pob_lua::format::tonumber_bytes(&a_num);
        // L694
        let b_num = b_num.unwrap_or_else(|| lua_error!("attempt to compare number with nil"));
        let bn = pob_lua::format::tonumber_bytes(&b_num);
        // L695
        if an != bn {
            // L696
            return an.expect("digits") < bn.expect("digits");
        }
        // L698
        if a_num != b_num {
            // L699
            return a_num < b_num;
        }
        // L701
        a_index = a_end.expect("matched");
        // L702
        b_index = b_end.expect("matched");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(sub: &str, file: &str) -> Entry {
        Entry {
            file_name: Some(file.to_owned()),
            sub_path: sub.to_owned(),
            build_name: Some(file.trim_end_matches(".xml").to_owned()),
            ..Entry::default()
        }
    }

    #[test]
    fn natural_sort() {
        assert!(natural_sort_compare("a2", "a10"));
        assert!(!natural_sort_compare("a10", "a2"));
        assert!(natural_sort_compare("Abc", "abd"));
        // upper-case ties: "ABC" == "ABC", then the raw bytes ('A' < 'a')
        assert!(natural_sort_compare("Abc", "abc"));
        assert!(natural_sort_compare("x01", "x1"));
        assert!(!natural_sort_compare("x", "x"));
        assert!(natural_sort_compare("build", "build2"));
    }

    #[test]
    fn match_and_filter() {
        let mut e = build("Sub/", "My Witch.xml");
        e.class_name = Some("Witch".into());
        e.ascend_class_name = Some("Necromancer".into());
        assert!(match_entry(&e, &["class:necro".into()]));
        assert!(match_entry(&e, &["class:".into(), "my".into()]));
        assert!(!match_entry(&e, &["class:ranger".into()]));
        assert!(match_entry(&e, &["sub/".into()]));
        let idx = vec![Rc::new(e), Rc::new(build("", "Other.xml"))];
        assert_eq!(filter_list(Some(&idx), "", None).len(), 1);
        assert_eq!(filter_list(Some(&idx), "", Some("  witch ")).len(), 1);
        assert_eq!(filter_list(None, "", Some("x")).len(), 0);
    }

    #[test]
    fn can_move() {
        let f = Entry {
            folder_name: Some("A".into()),
            sub_path: String::new(),
            ..Entry::default()
        };
        assert!(!can_move_to_sub_path(&f, ""));
        assert!(!can_move_to_sub_path(&f, "A/"));
        assert!(!can_move_to_sub_path(&f, "A/B/"));
        assert!(can_move_to_sub_path(&f, "B/"));
        assert!(can_move_to_sub_path(&build("", "x.xml"), "A/"));
    }

    #[test]
    fn sort_modes() {
        let mut a = build("", "b10.xml");
        a.level = Some(90.0);
        a.class_name = Some("Witch".into());
        let mut b = build("", "b2.xml");
        b.level = Some(80.0);
        b.class_name = Some("Duelist".into());
        let c = build("", "a.xml");
        let f = Entry {
            folder_name: Some("Z".into()),
            ..Entry::default()
        };
        let mut l: Vec<EntryRef> = vec![Rc::new(a), Rc::new(b), Rc::new(c), Rc::new(f)];
        let names = |l: &[EntryRef]| l.iter().map(|e| path_name(e)).collect::<Vec<_>>().join(",");
        sort_list(&mut l, Some("NAME"));
        assert_eq!(names(&l), "Z,a.xml,b2.xml,b10.xml");
        sort_list(&mut l, Some("CLASS"));
        assert_eq!(names(&l), "Z,a.xml,b2.xml,b10.xml");
        sort_list(&mut l, Some("LEVEL"));
        assert_eq!(names(&l), "Z,a.xml,b2.xml,b10.xml");
        let l2 = &mut l[1..];
        sort_list(l2, Some("LEVEL"));
        assert_eq!(names(l2), "a.xml,b2.xml,b10.xml");
    }
}
