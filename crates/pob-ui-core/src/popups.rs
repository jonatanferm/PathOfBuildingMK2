//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Main.lua:1660-1785 (main:OpenPopup,
//! ClosePopup, OpenMessagePopup, OpenConfirmPopup, OpenNewFolderPopup, OpenCloudErrorPopup),
//! Main.lua:437-443 (the popup layer and dim of main:OnFrame), and the PopupDialog constructor
//! and ProcessInput (Classes/PopupDialog.lua:11-42, :72-97). Ui doc §6.10. Owner: U14.
//!
//! @pob-file Modules/Main.lua @16de4b82d
//! @pob-file Classes/PopupDialog.lua @16de4b82d
//!
//! Lua builds popup `controls` tables before the dialog exists, and their closures reach siblings
//! through the local `controls` table when they are called. In Rust, closures capture the popup
//! id `p`; a sibling created later is looked up with `cx.child_as::<W>(p, "key")` at call time.
//!
//! ORDER: the dialog node is created before its children (Lua: the children first, then
//! `new("PopupDialog")`). Only the children's relative creation order is observable (the canonical
//! `controls` order, D7), and that is Lua's. The `DrawStringWidth` calls that size a popup run
//! before `build`, in Lua's order (Lua evaluates them between the constructors; constructors
//! draw nothing).
//!
//! ORDER: the anchor loop (PopupDialog.lua:25) iterates `pairs(self.controls)`; each step only
//! rewrites its own control's anchor, so the order is unobservable (ui doc §11.2 patch 6 leaves
//! it unpatched).
//!
//! Surface notes (owner's amendments to the UI-U0 stub, CCR-0084):
//! - `open_cloud_error_popup` takes the `launch` fields and `APP_NAME` it reads as a
//!   [`LaunchInfo`] (pob-ui-core cannot see pob-app's `LaunchState`), and returns the popup as
//!   Lua does.
//! - `draw_popup_layer` is Main.lua:437-443 (listed in U14's sources; pob-app's main:OnFrame calls
//!   it).
//! - `GetCloudProvider` and the Lua-level `MakeDir` have no `Sys` counterpart; local ports are
//!   marked `// CCR-0084`.

use std::rc::Rc;

use pob_lua::math::{m_ceil, m_floor, m_max};
use pob_sg::Font;

use crate::controls::button::ButtonControl;
use crate::controls::edit::{EditArgs, EditControl};
use crate::controls::label::LabelControl;
use crate::controls::popup_dialog::PopupDialog;
use crate::controls::scroll_bar::ScrollBarControl;
use crate::prelude::*;

/// The trailing arguments of main:OpenPopup.
#[derive(Default)]
pub struct PopupOpts {
    pub enter: Option<CtrlKey>,
    pub default: Option<CtrlKey>,
    pub escape: Option<CtrlKey>,
    pub scroll_bar_func: Option<Rc<dyn Fn(&mut Cx)>>,
    pub resize_func: Option<Rc<dyn Fn(&mut Cx)>>,
}

/// The `launch` fields and the `APP_NAME` global that main:OpenCloudErrorPopup reads
/// (Launch.lua:9 `APP_NAME = "Path of Building"`; `launch.versionNumber`, `launch.versionBranch`,
/// `launch.devMode`). CCR-0084.
#[derive(Clone, Debug)]
pub struct LaunchInfo {
    pub app_name: String,
    pub version_number: String,
    pub version_branch: Option<String>,
    pub dev_mode: bool,
}

/// main:OpenPopup. Creates the dialog node first (centred; x/y props as in PopupDialog.lua). Then `build` adds the
/// children into the popup's host (they are owned by the popup). Then the PopupDialog.lua:25-33 anchor rules apply:
/// no point => TOP/TOP of the popup; `other` nil => the popup; `other_key` => the sibling with that key.
/// Then `default` is selected. Finally the popup is inserted at main.popups[0] (Lua popups[1]).
/// lua: Main.lua:1660-1664
/// @pob Modules/Main.lua:1660-1664 h=c9e67adf
pub fn open_popup(
    cx: &mut Cx,
    width: f64,
    height: f64,
    title: impl Into<Prop<String>>,
    build: impl FnOnce(&mut Cx, Ctrl<PopupDialog>),
    opts: PopupOpts,
) -> Ctrl<PopupDialog> {
    // L1661
    let popup = popup_dialog(cx, width, height, title.into(), build, opts);
    // L1662
    cx.main.popups.insert(0, popup);
    // L1663
    popup
}

/// PopupDialog:PopupDialog(width, height, title, controls, enterControl, defaultControl,
/// escapeControl, scrollBarFunc, resizeFunc).
/// lua: PopupDialog.lua:11-42
/// @pob Classes/PopupDialog.lua:11-42 h=2c2b6ea9
fn popup_dialog(
    cx: &mut Cx,
    width: f64,
    height: f64,
    title: Prop<String>,
    build: impl FnOnce(&mut Cx, Ctrl<PopupDialog>),
    opts: PopupOpts,
) -> Ctrl<PopupDialog> {
    let PopupOpts {
        enter,
        default,
        escape,
        scroll_bar_func,
        resize_func,
    } = opts;
    // L13 self:ControlHost() / L14 self:Control(nil, {0, 0, width, height})
    let mut base = Base::new(Anchor::NONE, rect(0, 0, width, height));
    // L15
    base.x = Prop::f(move |cx, _| {
        // L16
        m_floor((cx.main.screen_w - width) / 2.0)
    });
    // L18
    base.y = Prop::f(move |cx, _| {
        // L19
        m_floor((cx.main.screen_h - height) / 2.0)
    });
    let p = cx.add_free(
        None,
        New::new(
            base,
            PopupDialog {
                // L21
                title,
                // L23
                enter_control: enter,
                // L24
                escape_control: escape,
                // allow scrollbar functionality inside of popups
                // L38
                scroll_bar_func,
                // allow resizing of popup
                // L40
                resize_func,
            },
        )
        .host(),
    );
    // L22 self.controls = controls
    build(cx, p);
    // L25
    for id in cx.children(p) {
        let (has_point, has_other, other_key) = {
            let a = &cx.base(id).anchor;
            (a.point.is_some(), a.other.is_some(), a.other_key.clone())
        };
        // L26
        if !has_point {
            // L27
            cx.set_anchor(id, Top, Some(p.id()), Top, None, false);
        } else if !has_other && other_key.is_none() {
            // L28-29
            cx.base_mut(id).anchor.other = Some(p.id());
        } else if let Some(key) = other_key {
            // L30-31 (a string `other`: the sibling with that key, nil when there is none)
            let other = cx.child(p, key);
            let a = &mut cx.base_mut(id).anchor;
            a.other = other;
            a.other_key = None;
        }
    }
    // L34
    if let Some(key) = default {
        // L35
        let c = cx.child(p, key);
        cx.select_control(p.id(), c);
    }
    // L41
    p
}

/// main:ClosePopup: removes popups[0] and buries it (freed at end of frame, D8).
/// lua: Main.lua:1666-1668
/// @pob Modules/Main.lua:1666-1668 h=94d7fcdd
pub fn close_popup(cx: &mut Cx) {
    // L1667 (t_remove of an empty table is a no-op)
    if !cx.main.popups.is_empty() {
        let p = cx.main.popups.remove(0);
        cx.bury(p);
    }
}

/// `for line in string.gmatch(msg .. "\n", "([^\n]*)\n")`
fn msg_lines(msg: &str) -> Vec<String> {
    let s = format!("{msg}\n");
    pob_lua::gmatch(&s, "([^\n]*)\n")
        .map(|c| c[0].to_str().into_owned())
        .collect()
}

/// The message labels of the Open*Popup family: `t_insert(controls, LabelControl(nil, {0, 20 +
/// numMsgLines * 16, 0, 16}, line))`, keys 1..n.
fn add_msg_labels(cx: &mut Cx, p: Ctrl<PopupDialog>, lines: &[String]) {
    for (num_msg_lines, line) in lines.iter().enumerate() {
        cx.add(
            p,
            CtrlKey::Idx(num_msg_lines as i64 + 1),
            LabelControl::new(
                Anchor::NONE,
                rect(0, 20.0 + num_msg_lines as f64 * 16.0, 0, 16),
                line.as_str(),
            ),
        );
    }
}

/// lua: Main.lua:1670-1681
/// @pob Modules/Main.lua:1670-1681 h=09e1734d
pub fn open_message_popup(cx: &mut Cx, title: &str, msg: &str) -> Ctrl<PopupDialog> {
    // L1671 local controls = { }
    // L1672-1676
    let lines = msg_lines(msg);
    let num_msg_lines = lines.len() as f64;
    // L1680 (the width argument)
    let width = m_max(cx.sg.draw_string_width(16.0, Font::Var, msg) + 30.0, 190.0);
    open_popup(
        cx,
        width,
        70.0 + num_msg_lines * 16.0,
        title,
        move |cx, p| {
            add_msg_labels(cx, p, &lines);
            // L1677
            cx.add(
                p,
                "close",
                ButtonControl::new(
                    Anchor::NONE,
                    rect(0, 40.0 + num_msg_lines * 16.0, 80, 20),
                    "Ok",
                    |cx| {
                        // L1678
                        close_popup(cx);
                    },
                ),
            );
        },
        PopupOpts {
            enter: Some("close".into()),
            ..PopupOpts::default()
        },
    )
}

/// lua: Main.lua:1683-1728
/// @pob Modules/Main.lua:1683-1728 h=0ea57390
pub fn open_confirm_popup(
    cx: &mut Cx,
    title: &str,
    msg: &str,
    confirm_label: &str,
    on_confirm: impl Fn(&mut Cx) + 'static,
    extra: Option<(&str, Rc<dyn Fn(&mut Cx)>)>,
) -> Ctrl<PopupDialog> {
    // L1684 local controls = { }
    // L1685-1689
    let lines = msg_lines(msg);
    let num_msg_lines = lines.len() as f64;
    let n = lines.len() as i64;
    // L1690
    let confirm_width = m_max(
        80.0,
        cx.sg.draw_string_width(16.0, Font::Var, confirm_label) + 10.0,
    );
    let on_confirm: Rc<dyn Fn(&mut Cx)> = Rc::new(on_confirm);
    let confirm_label = confirm_label.to_owned();

    // L1692
    if let Some((extra_label, on_extra)) = extra {
        // Three button layout: Continue (left), Connect Path (center), Cancel (right)
        // L1694
        let extra_width = m_max(
            80.0,
            cx.sg.draw_string_width(16.0, Font::Var, extra_label) + 10.0,
        );
        // L1695
        let cancel_width = 80.0;
        // L1696
        let spacing = 10.0;
        // L1697
        let total_width = confirm_width + extra_width + cancel_width + (spacing * 2.0);
        // L1698
        let left_edge = -total_width / 2.0;
        // L1699
        let button_y = 40.0 + num_msg_lines * 16.0;
        let extra_label = extra_label.to_owned();
        // L1716 (the width argument)
        let width = m_max(
            cx.sg.draw_string_width(16.0, Font::Var, msg) + 30.0,
            total_width + 40.0,
        );
        open_popup(
            cx,
            width,
            70.0 + num_msg_lines * 16.0,
            title,
            move |cx, p| {
                add_msg_labels(cx, p, &lines);
                let mut left_edge = left_edge;
                let mut next_idx = n;
                // L1700
                let mut place_button = |cx: &mut Cx,
                                        width: f64,
                                        label: &str,
                                        on_click: Rc<dyn Fn(&mut Cx)>,
                                        is_confirm: bool| {
                    // L1701
                    let center_x = left_edge + width / 2.0;
                    // L1702
                    let ctrl = ButtonControl::new(
                        Anchor::NONE,
                        rect(center_x, button_y, width, 20),
                        label,
                        move |cx| {
                            // L1703
                            close_popup(cx);
                            // L1704
                            on_click(cx);
                        },
                    );
                    // L1706
                    if is_confirm {
                        // L1707
                        cx.add(p, "confirm", ctrl);
                    } else {
                        // L1709
                        next_idx += 1;
                        cx.add(p, CtrlKey::Idx(next_idx), ctrl);
                    }
                    // L1711
                    left_edge = left_edge + width + spacing;
                };
                // L1713
                place_button(cx, confirm_width, &confirm_label, on_confirm, true);
                // L1714
                place_button(cx, extra_width, &extra_label, on_extra, false);
                // L1715
                place_button(cx, cancel_width, "Cancel", Rc::new(|_| {}), false);
            },
            PopupOpts {
                enter: Some("confirm".into()),
                ..PopupOpts::default()
            },
        )
    } else {
        // Two button layout (original)
        // L1726 (the width argument)
        let width = m_max(cx.sg.draw_string_width(16.0, Font::Var, msg) + 30.0, 190.0);
        open_popup(
            cx,
            width,
            70.0 + num_msg_lines * 16.0,
            title,
            move |cx, p| {
                add_msg_labels(cx, p, &lines);
                // L1719
                cx.add(
                    p,
                    "confirm",
                    ButtonControl::new(
                        Anchor::NONE,
                        rect(
                            -5.0 - m_ceil(confirm_width / 2.0),
                            40.0 + num_msg_lines * 16.0,
                            confirm_width,
                            20,
                        ),
                        confirm_label.as_str(),
                        move |cx| {
                            // L1720
                            close_popup(cx);
                            // L1721
                            on_confirm(cx);
                        },
                    ),
                );
                // L1723
                cx.add(
                    p,
                    CtrlKey::Idx(n + 1),
                    ButtonControl::new(
                        Anchor::NONE,
                        rect(
                            5.0 + m_ceil(confirm_width / 2.0),
                            40.0 + num_msg_lines * 16.0,
                            confirm_width,
                            20,
                        ),
                        "Cancel",
                        |cx| {
                            // L1724
                            close_popup(cx);
                        },
                    ),
                );
            },
            PopupOpts {
                enter: Some("confirm".into()),
                ..PopupOpts::default()
            },
        )
    }
}

/// `MakeDir(path)` as SimpleGraphic's `l_MakeDir` returns it (ui_api.cpp:1768-1786, as the
/// oracle's bridge models it: `std::filesystem::create_directory`, not recursive; `false` for an
/// existing directory gives `nil, strerror(0)` = "Success"; an error gives `nil, strerror(ec)`).
/// `Sys` has only `Fs::make_dir`, whose native impls differ on an existing directory, so the
/// existence test goes through `Fs::search` (exact name). CCR-0084.
fn make_dir(cx: &mut Cx, path: &str) -> Result<(), String> {
    // CCR-0084
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let exists = !name.is_empty()
        && cx
            .sys
            .fs
            .search(path, true)
            .iter()
            .any(|e| e.is_dir && e.name == name);
    if exists {
        return Err("Success".to_owned());
    }
    cx.sys.fs.make_dir(path).map_err(|e| {
        // strerror(ec.value()): io::Error's text without the " (os error N)" suffix
        let s = e.to_string();
        match s.rfind(" (os error ") {
            Some(i) => s[..i].to_owned(),
            None => s,
        }
    })
}

/// lua: Main.lua:1730-1756
/// @pob Modules/Main.lua:1730-1756 h=30fc7f7b
pub fn open_new_folder_popup(
    cx: &mut Cx,
    path: &str,
    on_close: Option<Rc<dyn Fn(&mut Cx, Option<String>)>>,
) {
    let path = path.to_owned();
    // L1755
    open_popup(
        cx,
        370.0,
        100.0,
        "New Folder",
        move |cx, p| {
            // L1731 local controls = { }
            // L1732
            cx.add(
                p,
                "label",
                LabelControl::new(Anchor::NONE, rect(0, 20, 0, 16), "^7Enter folder name:"),
            );
            // L1733
            cx.add(
                p,
                "edit",
                EditControl::new(
                    Anchor::NONE,
                    rect(0, 40, 350, 20),
                    EditArgs {
                        filter: Some(r#"\/:%*%?"<>|%c"#.to_owned()),
                        limit: Some(100),
                        change_func: Some(Rc::new(move |cx: &mut Cx, buf: &str| {
                            // L1734 controls.create.enabled = buf:match("%S")
                            let enabled = pob_lua::lmatch(buf, "%S", None).is_some();
                            let create = cx.child_as::<ButtonControl>(p, "create");
                            cx.base_mut(create).enabled = Prop::Val(enabled);
                        })),
                        ..EditArgs::default()
                    },
                ),
            );
            // L1736
            let on_close_create = on_close.clone();
            let create = cx.add(
                p,
                "create",
                ButtonControl::new(Anchor::NONE, rect(-45, 70, 80, 20), "Create", move |cx| {
                    // L1737
                    let new_folder_name = cx.get(cx.child_as::<EditControl>(p, "edit")).buf.clone();
                    // L1738
                    let res = make_dir(cx, &format!("{path}{new_folder_name}"));
                    // L1739
                    if let Err(msg) = res {
                        // L1740
                        open_message_popup(
                            cx,
                            "Error",
                            &format!("Couldn't create '{new_folder_name}': {msg}"),
                        );
                        // L1741
                        return;
                    }
                    // L1743
                    if let Some(f) = &on_close_create {
                        // L1744
                        f(cx, Some(new_folder_name));
                    }
                    // L1746
                    close_popup(cx);
                }),
            );
            // L1748
            cx.base_mut(create).enabled = Prop::Val(false);
            // L1749
            cx.add(
                p,
                "cancel",
                ButtonControl::new(Anchor::NONE, rect(45, 70, 80, 20), "Cancel", move |cx| {
                    // L1750
                    if let Some(f) = &on_close {
                        // L1751
                        f(cx, None);
                    }
                    // L1753
                    close_popup(cx);
                }),
            );
        },
        PopupOpts {
            enter: Some("create".into()),
            default: Some("edit".into()),
            escape: Some("cancel".into()),
            ..PopupOpts::default()
        },
    );
}

/// `GetCloudProvider(path)` -> name, version, status. SimpleGraphic implements it on Windows
/// only (the Cloud Files API); `_SimpleGraphic.def.lua:333-335` documents `nil, nil, nil`, which
/// is what every platform the port targets returns. `Sys` has no such query. CCR-0084.
fn get_cloud_provider(_path: &str) -> (Option<String>, Option<String>, Option<f64>) {
    // CCR-0084
    (None, None, None)
}

/// Show an error popup if a file cannot be read due to cloud provider unavailability.
/// Help button opens a URL to PoB's GitHub wiki.
/// lua: Main.lua:1760-1785
/// @pob Modules/Main.lua:1760-1785 h=1aed15a2
pub fn open_cloud_error_popup(
    cx: &mut Cx,
    file_name: &str,
    launch: &LaunchInfo,
) -> Ctrl<PopupDialog> {
    // L1761
    let (provider, _, status) = get_cloud_provider(file_name);
    // L1762 ConPrintf('^1Error: file offline "%s" provider: "%s" status: "%s"', fileName or "?", provider, status)
    let status_str = status.map_or_else(|| "nil".to_owned(), pob_lua::tostring_num);
    let line = format!(
        "^1Error: file offline \"{}\" provider: \"{}\" status: \"{}\"",
        file_name,
        provider.as_deref().unwrap_or("nil"),
        status_str
    );
    cx.sys.platform.con_print(&line);
    // L1763
    let file_name = format!("\n\n^8'{file_name}'");
    // L1764
    let version = format!(
        "^8v{}{}{}",
        launch.version_number,
        launch
            .version_branch
            .as_ref()
            .map_or_else(String::new, |b| format!(" {b}")),
        if launch.dev_mode { " (dev)" } else { "" }
    );
    // L1765
    let title = " ^1Error ";
    // L1766
    let provider = provider.unwrap_or_else(|| "your cloud provider".to_owned());
    // L1767 tostring(status) or "nil"
    let status_text = status_str;
    // L1768
    let msg = format!(
        "\n^7Cannot read file.\n\nMake sure {provider} is running then restart {} and try again.{file_name}\nstatus: {status_text}\n\n{version}",
        launch.app_name
    );
    // L1770
    let url = "https://github.com/PathOfBuildingCommunity/PathOfBuilding/wiki/CloudError";
    // L1771 local controls = { }
    // L1772-1776
    let lines = msg_lines(&msg);
    let num_msg_lines = lines.len() as f64;
    // L1784 (the width argument)
    let width = m_max(cx.sg.draw_string_width(16.0, Font::Var, &msg) + 30.0, 190.0);
    open_popup(
        cx,
        width,
        70.0 + num_msg_lines * 16.0,
        title,
        move |cx, p| {
            add_msg_labels(cx, p, &lines);
            // L1777
            let help = cx.add(
                p,
                "help",
                ButtonControl::new(
                    Anchor::NONE,
                    rect(-55, 40.0 + num_msg_lines * 16.0, 80, 20),
                    "Help (web)",
                    move |cx| {
                        // L1778
                        cx.sys.platform.open_url(url);
                    },
                ),
            );
            // L1780
            cx.get_mut(help).tt.tooltip_text = Some(Prop::Val(Some(url.to_owned())));
            // L1781
            cx.add(
                p,
                "close",
                ButtonControl::new(
                    Anchor::NONE,
                    rect(55, 40.0 + num_msg_lines * 16.0, 80, 20),
                    "Ok",
                    |cx| {
                        // L1782
                        close_popup(cx);
                    },
                ),
            );
        },
        PopupOpts {
            enter: Some("close".into()),
            ..PopupOpts::default()
        },
    )
}

/// `self.controls[key]:Click()`: a missing key is Lua's index-a-nil error.
fn click_child(cx: &mut Cx, popup: CtrlId, key: &CtrlKey) {
    let Some(c) = cx.child(popup, key.clone()) else {
        lua_error!("attempt to index field '?' (a nil value) (controls[{key:?}] of a PopupDialog)")
    };
    cx.click(c);
}

/// PopupDialog:ProcessInput (ESCAPE => escape:Click() or ClosePopup; RETURN => enter:Click()).
/// lua: PopupDialog.lua:72-97
/// @pob Classes/PopupDialog.lua:72-97 h=fc4f3601
pub fn popup_process_input(cx: &mut Cx, popup: CtrlId, ev: &mut InputEvents, vp: &ViewPort) {
    // L73
    cx.process_controls_input(popup, ev, vp);
    // L74
    let mut id = 0;
    while let Some(event) = ev.next_ipairs(&mut id) {
        // L75
        if let EventKind::KeyDown { .. } = event.kind {
            // L76
            if event.key == Key::Escape {
                // L77
                if let Some(k) = cx.w::<PopupDialog>(popup).escape_control.clone() {
                    // L78
                    click_child(cx, popup, &k);
                } else {
                    // L80
                    close_popup(cx);
                }
                // L82
                return;
            } else if event.key == Key::Return {
                // L84
                if let Some(k) = cx.w::<PopupDialog>(popup).enter_control.clone() {
                    // L85
                    click_child(cx, popup, &k);
                    // L86
                    return;
                }
            }
        } else if cx.w::<PopupDialog>(popup).scroll_bar_func.is_some()
            && event.kind == EventKind::KeyUp
        {
            // L89
            let scroll_bar = cx.child_as::<ScrollBarControl>(popup, "scrollBar").id();
            // L90
            if ScrollBarControl::is_scroll_down_key(cx, scroll_bar, event.key) {
                // L91
                ScrollBarControl::scroll(cx, scroll_bar, 1.0);
            } else if ScrollBarControl::is_scroll_up_key(cx, scroll_bar, event.key) {
                // L93
                ScrollBarControl::scroll(cx, scroll_bar, -1.0);
            }
        }
    }
}

/// main:OnFrame's popup layer: the topmost popup over a 50 % black dim, at layer 10.
/// lua: Main.lua:437-443
/// @pob Modules/Main.lua:437-443 h=b0076251
pub fn draw_popup_layer(cx: &mut Cx, vp: &ViewPort) {
    // L437
    if let Some(p) = cx.main.popups.first().copied() {
        // L438
        cx.sg.set_draw_layer(Some(10), None);
        // L439
        cx.sg.set_draw_color_a(0.0, 0.0, 0.0, 0.5);
        // L440
        let (w, h) = (cx.main.screen_w, cx.main.screen_h);
        cx.sg.draw_image(None, 0.0, 0.0, w, h);
        // L441
        cx.draw(p.id(), vp, false);
        // L442
        cx.sg.set_draw_layer(Some(0), None);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeSet;

    use pob_sg::FileEntry;

    use super::*;

    fn cx() -> Cx {
        let mut cx = Cx::headless();
        cx.main.screen_w = 1000.0;
        cx.main.screen_h = 600.0;
        cx
    }

    fn down(ev: &mut InputEvents, key: Key) {
        ev.push(InputEvent {
            kind: EventKind::KeyDown {
                double_click: false,
            },
            key,
        });
    }

    /// One `main:OnFrame` input step with a popup open (Main.lua:402-404).
    fn frame(cx: &mut Cx, keys: &[Key]) {
        let mut ev = InputEvents::default();
        for &k in keys {
            down(&mut ev, k);
        }
        let vp = ViewPort {
            x: 0.0,
            y: 0.0,
            width: cx.main.screen_w,
            height: cx.main.screen_h,
        };
        if let Some(p) = cx.main.popups.first().copied() {
            popup_process_input(cx, p.id(), &mut ev, &vp);
            ev.wipe();
        }
    }

    fn noop() -> impl Fn(&mut Cx) {
        |_| {}
    }

    #[test]
    fn anchor_rules_default_focus_and_centring() {
        let mut cx = cx();
        let p = open_popup(
            &mut cx,
            300.0,
            200.0,
            "T",
            |cx, p| {
                cx.add(
                    p,
                    "none",
                    ButtonControl::new(Anchor::NONE, rect(0, 10, 40, 16), "a", noop()),
                );
                cx.add(
                    p,
                    "nil",
                    ButtonControl::new(
                        Anchor::nil(TopLeft, TopLeft),
                        rect(5, 5, 20, 20),
                        "b",
                        noop(),
                    ),
                );
                cx.add(
                    p,
                    "key",
                    ButtonControl::new(
                        Anchor::key(Top, "nil", Bottom),
                        rect(0, 4, 20, 20),
                        "c",
                        noop(),
                    ),
                );
                cx.add(
                    p,
                    "missing",
                    ButtonControl::new(
                        Anchor::key(Top, "nope", Bottom),
                        rect(7, 8, 20, 20),
                        "d",
                        noop(),
                    ),
                );
            },
            PopupOpts {
                default: Some("nil".into()),
                ..PopupOpts::default()
            },
        );
        assert_eq!(cx.main.popups, vec![p]);
        // centred: floor((1000 - 300) / 2), floor((600 - 200) / 2)
        assert_eq!(cx.pos(p), (350.0, 200.0));
        let none = cx.child(p, "none").unwrap();
        let nil = cx.child(p, "nil").unwrap();
        let key = cx.child(p, "key").unwrap();
        let missing = cx.child(p, "missing").unwrap();
        // no point: TOP of the popup's TOP
        assert_eq!(cx.base(none).anchor.other, Some(p.id()));
        assert_eq!(cx.pos(none), (350.0 + 150.0 - 20.0, 210.0));
        // nil other: the popup
        assert_eq!(cx.base(nil).anchor.other, Some(p.id()));
        assert_eq!(cx.pos(nil), (355.0, 205.0));
        // string key: the sibling
        assert_eq!(cx.base(key).anchor.other, Some(nil));
        assert_eq!(cx.pos(key), (355.0, 229.0));
        // a missing key: nil, so the rect is absolute
        assert_eq!(cx.base(missing).anchor.other, None);
        assert!(cx.base(missing).anchor.other_key.is_none());
        assert_eq!(cx.pos(missing), (7.0, 8.0));
        // default focus
        assert_eq!(cx.host(p).sel_control, Some(nil));
        assert_eq!(cx.base(nil).has_focus, Some(true));
        // the centring follows the screen size (x/y are functions)
        cx.main.screen_w = 500.0;
        assert_eq!(cx.pos(p), (100.0, 200.0));
    }

    #[test]
    fn stacked_popups_close_topmost_first_and_free_at_end_of_frame() {
        let mut cx = cx();
        let a = open_message_popup(&mut cx, "A", "first");
        let b = open_message_popup(&mut cx, "B", "second\nline");
        assert_eq!(cx.main.popups, vec![b, a]);
        let b_close = cx.child(b, "close").unwrap();
        // two lines: labels 1 and 2 plus "close"
        assert_eq!(cx.children(b).len(), 3);
        assert_eq!(cx.size(b), (190.0, 70.0 + 32.0));
        close_popup(&mut cx);
        assert_eq!(cx.main.popups, vec![a]);
        // deferred free (D8): still readable this frame
        assert!(cx.try_w::<PopupDialog>(b.id()).is_some());
        cx.end_frame_gc();
        assert!(cx.try_w::<PopupDialog>(b.id()).is_none());
        assert!(cx.try_w::<ButtonControl>(b_close).is_none());
        assert!(cx.try_w::<PopupDialog>(a.id()).is_some());
        close_popup(&mut cx);
        // an empty stack: no-op
        close_popup(&mut cx);
        assert!(cx.main.popups.is_empty());
    }

    #[test]
    fn enter_and_escape_routing() {
        let mut cx = cx();
        // RETURN clicks the enter control ("close" closes the message popup)
        open_message_popup(&mut cx, "M", "msg");
        frame(&mut cx, &[Key::Return]);
        assert!(cx.main.popups.is_empty());
        // ESCAPE without an escape control closes
        open_message_popup(&mut cx, "M", "msg");
        frame(&mut cx, &[Key::Escape]);
        assert!(cx.main.popups.is_empty());
        // ESCAPE with one clicks it; RETURN without an enter control does nothing
        let hits = Rc::new(Cell::new(0));
        let h = hits.clone();
        open_popup(
            &mut cx,
            200.0,
            100.0,
            "E",
            move |cx, p| {
                cx.add(
                    p,
                    "esc",
                    ButtonControl::new(Anchor::NONE, rect(0, 40, 80, 20), "Esc", move |_| {
                        h.set(h.get() + 1);
                    }),
                );
            },
            PopupOpts {
                escape: Some("esc".into()),
                ..PopupOpts::default()
            },
        );
        frame(&mut cx, &[Key::Return]);
        assert_eq!((hits.get(), cx.main.popups.len()), (0, 1));
        frame(&mut cx, &[Key::Escape, Key::Escape]);
        // `return` after the first ESCAPE
        assert_eq!((hits.get(), cx.main.popups.len()), (1, 1));
    }

    #[test]
    fn confirm_two_buttons_close_from_own_button() {
        let mut cx = cx();
        let seen: Rc<RefCell<Vec<CtrlId>>> = Rc::new(RefCell::new(Vec::new()));
        let s = seen.clone();
        let p = open_confirm_popup(
            &mut cx,
            "C",
            "sure?",
            "Delete",
            move |cx| {
                // ClosePopup ran first; the popup is buried but alive until the end of frame
                let p = s.borrow()[0];
                assert!(cx.try_w::<PopupDialog>(p).is_some());
                assert!(cx.main.popups.is_empty());
                s.borrow_mut().push(p);
            },
            None,
        );
        seen.borrow_mut().push(p.id());
        // keys: label 1, "confirm", cancel 2
        let confirm = cx.child(p, "confirm").unwrap();
        let cancel = cx.child(p, 2_i64).unwrap();
        // confirmWidth = max(80, 0 + 10) with the inert font set
        assert_eq!(cx.base(confirm).x.raw(), -5.0 - 40.0);
        assert_eq!(cx.base(cancel).x.raw(), 5.0 + 40.0);
        frame(&mut cx, &[Key::Return]);
        assert_eq!(seen.borrow().len(), 2);
        cx.end_frame_gc();
        assert!(cx.try_w::<PopupDialog>(p.id()).is_none());
    }

    #[test]
    fn confirm_three_buttons_layout() {
        let mut cx = cx();
        let extra_hits = Rc::new(Cell::new(0));
        let e = extra_hits.clone();
        let p = open_confirm_popup(
            &mut cx,
            "C",
            "a\nb",
            "Continue",
            |_| {},
            Some((
                "Connect Path",
                Rc::new(move |cx: &mut Cx| {
                    assert!(cx.main.popups.is_empty());
                    e.set(e.get() + 1);
                }),
            )),
        );
        // labels 1, 2; "confirm"; extra 3; cancel 4
        assert_eq!(cx.children(p).len(), 5);
        let confirm = cx.child(p, "confirm").unwrap();
        let extra = cx.child(p, 3_i64).unwrap();
        let cancel = cx.child(p, 4_i64).unwrap();
        // widths 80 each, spacing 10: totalWidth 260, leftEdge -130
        assert_eq!(cx.base(confirm).x.raw(), -90.0);
        assert_eq!(cx.base(extra).x.raw(), 0.0);
        assert_eq!(cx.base(cancel).x.raw(), 90.0);
        assert_eq!(cx.base(cancel).y.raw(), 40.0 + 32.0);
        // width: max(0 + 30, 260 + 40)
        assert_eq!(cx.size(p), (300.0, 70.0 + 32.0));
        cx.click(extra);
        assert_eq!(extra_hits.get(), 1);
    }

    /// An `Fs` with a set of directories (exact names).
    struct DirSet(Rc<RefCell<BTreeSet<String>>>);
    impl pob_sg::Fs for DirSet {
        fn read(&self, _: &str) -> std::io::Result<Vec<u8>> {
            Err(std::io::ErrorKind::NotFound.into())
        }
        fn write(&mut self, _: &str, _: &[u8]) -> std::io::Result<()> {
            Ok(())
        }
        fn search(&self, spec: &str, find_dirs: bool) -> Vec<FileEntry> {
            if !find_dirs || !self.0.borrow().contains(spec) {
                return Vec::new();
            }
            vec![FileEntry {
                name: spec.rsplit('/').next().unwrap().to_owned(),
                is_dir: true,
                size: 0,
                modified: 0,
            }]
        }
        fn make_dir(&mut self, path: &str) -> std::io::Result<()> {
            self.0.borrow_mut().insert(path.to_owned());
            Ok(())
        }
        fn remove_dir(&mut self, _: &str, _: bool) -> std::io::Result<()> {
            Ok(())
        }
        fn rename(&mut self, _: &str, _: &str) -> std::io::Result<()> {
            Ok(())
        }
        fn remove(&mut self, _: &str) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn new_folder_popup() {
        let mut cx = cx();
        let dirs = Rc::new(RefCell::new(BTreeSet::new()));
        cx.sys.fs = Box::new(DirSet(dirs.clone()));
        let closed = Rc::new(RefCell::new(Vec::new()));
        let c = closed.clone();
        let on_close: Rc<dyn Fn(&mut Cx, Option<String>)> =
            Rc::new(move |_, name| c.borrow_mut().push(name));
        open_new_folder_popup(&mut cx, "/u/", Some(on_close.clone()));
        let p = cx.main.popups[0];
        let edit = cx.child(p, "edit").unwrap();
        let create = cx.child(p, "create").unwrap();
        // default focus on the edit; Create starts disabled
        assert_eq!(cx.host(p).sel_control, Some(edit));
        assert!(!cx.is_enabled(create));
        // the focused edit eats ESCAPE (it drops the focus); RETURN on a disabled Create does
        // nothing
        frame(&mut cx, &[Key::Escape]);
        assert_eq!(cx.main.popups, vec![p]);
        assert_eq!(cx.host(p).sel_control, None);
        frame(&mut cx, &[Key::Return]);
        assert_eq!(cx.main.popups, vec![p]);
        // typing enables Create (changeFunc: buf:match("%S"))
        EditControl::insert(&mut cx, edit, "x");
        assert_eq!(cx.w::<EditControl>(edit).buf, "x");
        assert!(cx.is_enabled(create));
        frame(&mut cx, &[Key::Return]);
        assert!(cx.main.popups.is_empty());
        assert!(dirs.borrow().contains("/u/x"));
        assert_eq!(*closed.borrow(), vec![Some("x".to_owned())]);
        // an existing directory: MakeDir gives nil, "Success"; the error popup stacks on top
        open_new_folder_popup(&mut cx, "/u/", Some(on_close));
        let p = cx.main.popups[0];
        let edit = cx.child(p, "edit").unwrap();
        EditControl::insert(&mut cx, edit, "x");
        cx.select_control(p.id(), None);
        frame(&mut cx, &[Key::Return]);
        assert_eq!(cx.main.popups.len(), 2);
        assert_eq!(cx.main.popups[1], p);
        let err = cx.main.popups[0];
        let label = cx.child(err, 1_i64).unwrap();
        assert_eq!(
            cx.prop::<LabelControl, _>(label, |w| &w.label),
            "Couldn't create 'x': Success"
        );
        // close the error, then ESCAPE (no focus) clicks Cancel: onClose() and ClosePopup
        frame(&mut cx, &[Key::Return]);
        frame(&mut cx, &[Key::Escape]);
        assert!(cx.main.popups.is_empty());
        assert_eq!(*closed.borrow(), vec![Some("x".to_owned()), None]);
    }

    #[test]
    fn cloud_error_popup() {
        let mut cx = cx();
        let launch = LaunchInfo {
            app_name: "Path of Building".to_owned(),
            version_number: "2.67.2".to_owned(),
            version_branch: Some("beta".to_owned()),
            dev_mode: true,
        };
        let p = open_cloud_error_popup(&mut cx, "C:/b.xml", &launch);
        let lines: Vec<String> = (1..=9)
            .map(|i: i64| {
                let l = cx.child(p, i).unwrap();
                cx.prop::<LabelControl, _>(l, |w| &w.label)
            })
            .collect();
        assert_eq!(
            lines,
            [
                "",
                "^7Cannot read file.",
                "",
                "Make sure your cloud provider is running then restart Path of Building and try again.",
                "",
                "^8'C:/b.xml'",
                "status: nil",
                "",
                "^8v2.67.2 beta (dev)",
            ]
        );
        assert!(cx.child(p, 10_i64).is_none());
        assert_eq!(cx.prop::<PopupDialog, _>(p.id(), |w| &w.title), " ^1Error ");
        frame(&mut cx, &[Key::Return]);
        assert!(cx.main.popups.is_empty());
    }

    #[test]
    fn message_lines_follow_gmatch() {
        assert_eq!(msg_lines("a"), ["a"]);
        assert_eq!(msg_lines("a\n"), ["a", ""]);
        assert_eq!(msg_lines(""), [""]);
        assert_eq!(msg_lines("\na\n\nb"), ["", "a", "", "b"]);
    }
}
