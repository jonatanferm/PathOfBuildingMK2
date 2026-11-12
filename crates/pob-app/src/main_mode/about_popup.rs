//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Main.lua:1373-1485 (OpenAboutPopup).
//! Owner: U17.
//!
//! @pob-file Modules/Main.lua @16de4b82d
//!
//! The changelog and help files are read through `cx.sys.fs` (relative to the script directory,
//! as Lua's `io` in SG): `changelog.txt`/`help.txt`, or `../changelog.txt`/`../help.txt` in dev
//! mode. The two buttons swap the TextListControl's `list`/`sectionHeights` between the two
//! lists; the rows are shared (`Rc`), as the Lua tables are.
//!
//! `helpSectionIndex` is a [`ModeArg`]: nil, the number of a section (Build.lua:316 passes 7) or
//! a lower-case section title (main:OnFrame's F1 passes `"<view mode> tab"`). The Lua indexes
//! `helpSections[helpSectionIndex]` with it: a number names a section; a string names nothing
//! except `"total"` (the `helpSections.total` field of L1449, a number), which the Lua then
//! fails to index (L1481); [`HelpIndex`] keeps that case.
//!
//! DIVERGENCE(insert-range): `t_insert(helpList, pos, row)` (L1438, L1440) with `pos` outside
//! `1..#helpList+1` would store past the border in LuaJIT; the port raises instead
//! ([`t_insert_at`]). The Contents rows always land right after the Contents header, so no help
//! file reaches it. DIVERGENCE(lossy-utf8): see update_popup.rs.
//!
//! DIVERGENCE(branding): user decision 2026-11-12. The `version` label (L1466) reads
//! `^7Path of Building Rust Port v<PORT_VERSION>` ([`about_version_text`]) instead of
//! `^7Path of Building Community Fork v<launch.versionNumber>`; the UI oracle's patch 9 makes the
//! Lua draw the same.
//!
//! Props with side effects (ui doc §7.3): none.

use std::rc::Rc;

use pob_sg::Align;
use pob_ui_core::controls::text_list::{TextListColumn, TextListRow};
use pob_ui_core::controls::{ButtonControl, LabelControl, ScrollBarControl, TextListControl};
use pob_ui_core::popups::{close_popup, open_popup, PopupOpts};
use pob_ui_core::prelude::*;
use pob_ui_core::wrap::wrap_string;

use crate::app::CxApp;
use crate::main_mode::update_popup::read_lines;
use crate::main_mode::ModeArg;

/// The About popup's `version` label (L1466). DIVERGENCE(branding): user decision 2026-11-12
/// (PoB: `"^7Path of Building Community Fork v"..launch.versionNumber`).
pub fn about_version_text() -> String {
    format!("^7Path of Building Rust Port v{}", crate::launch::PORT_VERSION)
}

/// `helpSections[helpSectionIndex]` after L1451-1460.
enum HelpIndex {
    /// a 1-based index into helpSections
    Section(usize),
    /// `helpSections.total` (a number)
    Total,
}

/// One `helpSections[i]` (`{ title = title, height = #helpList }`).
struct HelpSection {
    title: String,
    height: f64,
}

fn row(height: f64, cells: &[&str]) -> Rc<TextListRow> {
    Rc::new(TextListRow::new(height, cells))
}

/// `t_insert(t, pos, v)` (lib_table.c `tinsert` with a position): the elements from `pos` on
/// move up. Every call site inserts at most one past the end (the Contents rows land right after
/// the Contents header).
fn t_insert_at(t: &mut Vec<Rc<TextListRow>>, pos: usize, v: Rc<TextListRow>) {
    if pos == 0 || pos > t.len() + 1 {
        // LuaJIT would store `t[pos]` past the border (or at 0); help.txt never leads here
        lua_error!(
            "OpenAboutPopup: table.insert position {pos} outside 1..{}",
            t.len() + 1
        );
    }
    t.insert(pos - 1, v);
}

/// lua: Main.lua:1373-1484 main:OpenAboutPopup(helpSectionIndex)
/// @pob Modules/Main.lua:1373-1484 h=6d5e9410
pub fn open_about_popup(cx: &mut Cx, help_section_index: ModeArg) {
    // L1374
    let (text_size, sub_title_size, title_size, popup_width) = (16.0, 20.0, 24.0, 810.0);
    // L1375
    let mut change_list: Vec<Rc<TextListRow>> = Vec::new();
    // L1376
    let mut change_version_heights: Vec<f64> = Vec::new();
    let dev_mode = cx.app().launch.dev_mode;
    // L1377
    let changelog_name = if dev_mode {
        "../changelog.txt"
    } else {
        "changelog.txt"
    };
    // L1378-1380 (io.open, then close; io.lines reads it again)
    if let Some(lines) = read_lines(cx, changelog_name) {
        // L1381
        for line in lines {
            // L1382
            let m = pob_lua::lmatch(&line, "^VERSION%[(.+)%]%[(.+)%]$", None);
            // L1383
            if let Some(c) = m {
                let (ver, date) = (c[0].to_str().into_owned(), c[1].to_str().into_owned());
                // L1384
                if !change_list.is_empty() {
                    // L1385
                    change_list.push(row(text_size / 2.0, &[]));
                }
                // L1387
                change_version_heights.push(change_list.len() as f64 * text_size);
                // L1388
                change_list.push(row(title_size, &[&format!("^7Version {ver} ({date})")]));
            } else if pob_lua::lmatch(&line, "^---", None).is_some() {
                // L1389-1390
                change_list.push(row(sub_title_size, &[&format!("^7{line}")]));
            } else {
                // L1392
                change_list.push(row(text_size, &[&format!("^7{line}")]));
            }
        }
    }
    // L1396
    let mut help_list: Vec<Rc<TextListRow>> = Vec::new();
    // L1397
    let mut help_sections: Vec<HelpSection> = Vec::new();
    // L1398
    let mut help_section_heights: Vec<f64> = Vec::new();
    // `helpSections.total` is set (L1449)
    let mut has_total = false;
    // L1399 do
    {
        // L1400
        let help_name = if dev_mode { "../help.txt" } else { "help.txt" };
        // L1401-1403
        if let Some(lines) = read_lines(cx, help_name) {
            // L1404
            for line in lines {
                // L1405
                let title = pob_lua::lmatch(&line, "^---%[(.+)%]$", None)
                    .map(|c| c[0].to_str().into_owned());
                // L1406
                if let Some(title) = title {
                    // L1407
                    if !help_list.is_empty() {
                        // L1408
                        help_list.push(row(text_size / 2.0, &[]));
                    }
                    // L1410
                    help_sections.push(HelpSection {
                        title: title.clone(),
                        height: help_list.len() as f64,
                    });
                    // L1411
                    help_list.push(row(
                        title_size,
                        &[&format!("^7{title} ({})", help_sections.len())],
                    ));
                } else {
                    // L1413
                    let dev = pob_lua::lmatch(&line, "^DEV%[(.+)%]$", None)
                        .map(|c| c[0].to_str().into_owned());
                    // L1414
                    // not ( dev and not launch.devMode )
                    if dev.is_none() || dev_mode {
                        // L1415
                        let line = dev.clone().unwrap_or(line);
                        let col = if dev.is_some() { "^x8888FF" } else { "^7" };
                        // L1416
                        let m = pob_lua::lmatch(&line, "(.*)\t+(.*)", None)
                            .map(|c| (c[0].to_str().into_owned(), c[1].to_str().into_owned()));
                        // L1417
                        if let Some((outdent, indent)) = m {
                            // L1418
                            let indent_lines =
                                wrap_string(cx, &indent, text_size, popup_width - 190.0);
                            // L1419
                            if indent_lines.len() > 1 {
                                // L1420
                                for (i, indent_line) in indent_lines.iter().enumerate() {
                                    // L1421
                                    let first = if i == 0 { outdent.as_str() } else { " " };
                                    help_list.push(row(
                                        text_size,
                                        &[first, &format!("{col}{indent_line}")],
                                    ));
                                }
                            } else {
                                // L1424
                                help_list.push(row(
                                    text_size,
                                    &[&format!("{col}{outdent}"), &format!("{col}{indent}")],
                                ));
                            }
                        } else {
                            // L1427
                            let lines2 = wrap_string(cx, &line, text_size, popup_width - 135.0);
                            // L1428
                            for (i, line2) in lines2.iter().enumerate() {
                                // L1429
                                let pad = if i > 0 { "    " } else { "" };
                                help_list.push(row(text_size, &[&format!("{col}{pad}{line2}")]));
                            }
                        }
                    }
                }
            }
            // L1435
            let mut contents_done = false;
            let n_sections = help_sections.len();
            // L1436
            for section_index in 1..=n_sections {
                let is_contents = help_sections[section_index - 1].title == "Contents";
                // L1437
                if is_contents {
                    let height = help_sections[section_index - 1].height;
                    // L1438
                    t_insert_at(
                        &mut help_list,
                        (height + section_index as f64) as usize,
                        row(text_size, &["^7 "]),
                    );
                    // L1439
                    for i in 1..=n_sections {
                        // L1440
                        let entry = format!("^7{}. {}", i, help_sections[i - 1].title);
                        t_insert_at(
                            &mut help_list,
                            (height + i as f64 + section_index as f64) as usize,
                            row(text_size, &[&entry]),
                        );
                    }
                }
                // L1443
                let add = if contents_done {
                    (n_sections + 1) as f64
                } else {
                    0.0
                };
                help_sections[section_index - 1].height += add;
                // L1444 helpSectionHeights[sectionIndex] = ... (sequential: index == #t + 1)
                help_section_heights.push(help_sections[section_index - 1].height * text_size);
                // L1445
                if is_contents {
                    // L1446
                    contents_done = true;
                }
            }
            // L1449 helpSections.total = #helpList + #helpSections + 1 (nothing reads it; its
            // presence is what `helpSections[helpSectionIndex]` sees for "total")
            has_total = true;
        }
    }
    // L1452 `helpSectionIndex and not helpSections[helpSectionIndex]`
    let resolved: Option<HelpIndex> = match &help_section_index {
        ModeArg::Nil | ModeArg::Bool(false) => None,
        arg => {
            let direct = match arg {
                ModeArg::Num(n)
                    if n.fract() == 0.0 && *n >= 1.0 && *n <= n_sections_f(&help_sections) =>
                {
                    Some(HelpIndex::Section(*n as usize))
                }
                ModeArg::Str(s) if s == "total" && has_total => Some(HelpIndex::Total),
                _ => None,
            };
            if direct.is_some() {
                direct
            } else {
                // L1453
                let mut new_index = 1;
                // L1454
                for (section_index, section_values) in help_sections.iter().enumerate() {
                    // L1455 (a number never equals a string)
                    if Some(pob_lua::string::lower(&section_values.title).as_str()) == arg.as_str()
                    {
                        // L1456
                        new_index = section_index + 1;
                        // L1457
                        break;
                    }
                }
                // L1460
                Some(HelpIndex::Section(new_index))
            }
        }
    };
    let help_list = help_list;
    let change_list = change_list;
    let help_section_heights = help_section_heights;
    let change_version_heights = change_version_heights;
    // L1481 helpSections[helpSectionIndex].height (indexed before the popup opens)
    let scroll_offset = resolved.as_ref().map(|r| match r {
        HelpIndex::Section(i) => match help_sections.get(i - 1) {
            Some(s) => s.height * text_size,
            None => lua_error!("attempt to index field '?' (a nil value)"),
        },
        HelpIndex::Total => lua_error!("attempt to index field '?' (a number value)"),
    });
    // L1462 local controls = { }
    // L1483
    open_popup(
        cx,
        popup_width,
        628.0,
        "About",
        move |cx, p| {
            // L1463
            cx.add(
                p,
                "close",
                ButtonControl::new(
                    Anchor::nil(TopRight, TopRight),
                    rect(-10, 10, 50, 20),
                    "Close",
                    |cx| {
                        // L1464
                        close_popup(cx);
                    },
                ),
            );
            // L1466
            cx.add(
                p,
                "version",
                LabelControl::new(
                    Anchor::NONE,
                    rect(0, 18, 0, 18),
                    // DIVERGENCE(branding): user decision 2026-11-12
                    about_version_text(),
                ),
            );
            // L1467
            cx.add(
                p,
                "forum",
                LabelControl::new(
                    Anchor::NONE,
                    rect(0, 36, 0, 18),
                    "^7Based on Openarl's Path of Building",
                ),
            );
            // L1468
            cx.add(
                p,
                "github",
                ButtonControl::new(
                    Anchor::NONE,
                    rect(0, 62, 438, 18),
                    "^7GitHub page: ^x4040FFhttps://github.com/PathOfBuildingCommunity/PathOfBuilding",
                    |cx| {
                        // L1469
                        cx.sys
                            .platform
                            .open_url("https://github.com/PathOfBuildingCommunity/PathOfBuilding");
                    },
                ),
            );
            let (cl, cvh) = (change_list.clone(), change_version_heights.clone());
            // L1471
            cx.add(
                p,
                "verLabel",
                ButtonControl::new(
                    Anchor::nil(TopLeft, TopLeft),
                    rect(10, 85, 100, 18),
                    "^7Version history:",
                    move |cx| {
                        // forward reference: `controls.changelog` is created below
                        let changelog = cx.child_as::<TextListControl>(p, "changelog");
                        let w = cx.get_mut(changelog);
                        // L1472
                        w.list = cl.clone();
                        // L1473
                        w.section_heights = Some(cvh.clone());
                    },
                ),
            );
            let (hl, hsh) = (help_list.clone(), help_section_heights.clone());
            // L1475
            cx.add(
                p,
                "helpLabel",
                ButtonControl::new(
                    Anchor::nil(TopRight, TopRight),
                    rect(-10, 85, 40, 18),
                    "^7Help:",
                    move |cx| {
                        let changelog = cx.child_as::<TextListControl>(p, "changelog");
                        let w = cx.get_mut(changelog);
                        // L1476
                        w.list = hl.clone();
                        // L1477
                        w.section_heights = Some(hsh.clone());
                    },
                ),
            );
            // L1479
            let help = resolved.is_some();
            let changelog = cx.add(
                p,
                "changelog",
                TextListControl::new(
                    Anchor::NONE,
                    rect(0, 103, popup_width - 20.0, 515),
                    Some(vec![
                        TextListColumn {
                            x: 1.0,
                            align: Align::Left,
                        },
                        TextListColumn {
                            x: 135.0,
                            align: Align::Left,
                        },
                    ]),
                    if help { help_list } else { change_list },
                    Some(if help {
                        help_section_heights
                    } else {
                        change_version_heights
                    }),
                ),
            );
            // L1480
            if let Some(offset) = scroll_offset {
                // L1481 controls.changelog.controls.scrollBar.offset = ...
                let sb = cx.child_as::<ScrollBarControl>(changelog, "scrollBar");
                cx.get_mut(sb).offset = offset;
            }
        },
        PopupOpts::default(),
    );
}

/// `#helpSections` as a number.
fn n_sections_f(help_sections: &[HelpSection]) -> f64 {
    help_sections.len() as f64
}

#[cfg(test)]
mod tests {
    /// DIVERGENCE(branding): the About popup's version line (Main.lua L1466) on the port.
    #[test]
    fn about_version_line() {
        assert_eq!(
            super::about_version_text(),
            format!("^7Path of Building Rust Port v{}", crate::launch::PORT_VERSION)
        );
        assert!(!super::about_version_text().contains("Community"));
    }
}
