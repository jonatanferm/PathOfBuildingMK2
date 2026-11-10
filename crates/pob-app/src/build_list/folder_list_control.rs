//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/FolderListControl.lua 1-99
//! (`newClass("FolderListControl", "ListControl")`). Owner: U18.
//!
//! @pob-file Classes/FolderListControl.lua @16de4b82d
//!
//! The folder picker of Build.lua's "Save As" popup (Build.lua:1481, U19-U21). The list is the
//! control's own table (`{ }`, L13); rows are [`Folder`]s (`{ name, fullFileName, modified }`).
//!
//! ListControl hooks: `GetRowValue`, `OnSelClick`, `OnSelDelete`.
//!
//! L65-L67 test methods ListControl does not have: `handle.Close` (SG search handles have no
//! `Close`), `self.UpdateScrollbar`, `self.Redraw` are nil in PoB, so those calls never run.
//! Props with side effects (ui doc §7.3): none.

use std::rc::Rc;

use pob_ui_core::controls::list::{ListControl, ListData, ListHooks, ListValue};
use pob_ui_core::controls::path::PathControl;
use pob_ui_core::popups;
use pob_ui_core::prelude::*;

use super::helpers::natural_sort_compare;
use crate::platform::storage::{self, build_path};

// @pob-skip Classes/FolderListControl.lua:1-11 reason="header comment, locals and newClass (the Widget impl below)"

/// One row: `{ name = .., fullFileName = .., modified = .. }`.
#[derive(Clone, Debug, PartialEq)]
pub struct Folder {
    pub name: String,
    pub full_file_name: String,
    pub modified: f64,
}

fn folder_of(v: &ListValue) -> Rc<Folder> {
    v.rc::<Folder>()
        .unwrap_or_else(|| lua_error!("FolderListControl: a list value that is not a folder"))
}

pub struct FolderListControl {
    pub list: ListControl,
    /// `self.subPath`
    pub sub_path: String,
    /// `self.sortMode` (Build.lua:1484 may assign nil)
    pub sort_mode: Option<String>,
    /// `self.onChangeCallback`
    pub on_change_callback: Option<Rc<dyn Fn(&mut Cx, &str)>>,
}

impl FolderListControl {
    /// lua: FolderListControl.lua:12-29 FolderListControl(anchor, rect, subPath, onChange)
    /// @pob Classes/FolderListControl.lua:12-29 h=6d393cfc
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        sub_path: Option<String>,
        on_change: Option<Rc<dyn Fn(&mut Cx, &str)>>,
    ) -> New<FolderListControl> {
        // L13
        let l = ListControl::state(
            &HOOKS,
            16.0,
            true,
            false,
            ListData::Local(Vec::new()),
            false,
        );
        // L14
        let sub_path = sub_path.unwrap_or_default();
        New::new(
            Base::new(anchor, rect),
            FolderListControl {
                list: l,
                sub_path: sub_path.clone(),
                // L15
                sort_mode: Some("NAME".to_owned()),
                // L16
                on_change_callback: on_change,
            },
        )
        .host()
        .init(move |cx, me| {
            let me = me.id();
            // L13 (the scroll bars)
            ListControl::init(cx, me);
            // L18
            let width = cx.base(me).width.raw();
            let on_change: Rc<dyn Fn(&mut Cx, &str)> = Rc::new(move |cx, new_sub_path| {
                // L19
                cx.w_mut::<FolderListControl>(me).sub_path = new_sub_path.to_owned();
                // L20
                FolderListControl::build_list(cx, me);
                let l = cx.w_mut::<ListControl>(me);
                // L21
                l.sel_index = None;
                // L22
                l.sel_value = None;
                // L23
                if let Some(f) = cx.w::<FolderListControl>(me).on_change_callback.clone() {
                    // L24
                    f(cx, new_sub_path);
                }
            });
            cx.add(
                me,
                "path",
                PathControl::new(
                    Anchor::new(Bottom, me, Top),
                    pob_ui_core::layout::rect(0, -2, width, 24),
                    build_path(cx),
                    Some(sub_path),
                    Some(on_change),
                ),
            );
            // L27
            FolderListControl::build_list(cx, me);
        })
    }

    /// lua: FolderListControl.lua:31-47 SortList()
    /// @pob Classes/FolderListControl.lua:31-47 h=cb50ec7f
    pub fn sort_list(cx: &mut Cx, me: CtrlId) {
        // L32 (`self.list` is always a table)
        // L33
        let sort_mode = cx.w::<Self>(me).sort_mode.clone();
        // L35
        let ListData::Local(list) = &mut cx.w_mut::<ListControl>(me).list else {
            lua_error!("FolderListControl: the list is not the control's own")
        };
        pob_lua::sort(list, |a, b| {
            let (a, b) = (folder_of(a), folder_of(b));
            // L36
            if sort_mode.as_deref() == Some("EDITED") {
                // L37
                let mod_a = a.modified;
                // L38
                let mod_b = b.modified;
                // L39
                if mod_a != mod_b {
                    // L40
                    return mod_a > mod_b;
                }
                // L42
                natural_sort_compare(&a.name, &b.name)
            } else {
                // L44
                natural_sort_compare(&a.name, &b.name)
            }
        });
    }

    /// lua: FolderListControl.lua:49-68 BuildList()
    /// @pob Classes/FolderListControl.lua:49-68 h=9359e31d
    pub fn build_list(cx: &mut Cx, me: CtrlId) {
        // L50
        let mut list: Vec<ListValue> = Vec::new();
        let bp = build_path(cx);
        let sub_path = cx.w::<Self>(me).sub_path.clone();
        // L51
        let handle = storage::new_file_search(cx, &format!("{bp}{sub_path}*"), true);
        // L52
        for h in handle {
            // L53
            let file_name = h.name;
            // L54-L58
            list.push(ListValue::of(Rc::new(Folder {
                full_file_name: format!("{bp}{sub_path}{file_name}"),
                name: file_name,
                modified: h.modified as f64,
            })));
            // L59 (NextFile)
        }
        cx.w_mut::<ListControl>(me).list = ListData::Local(list);
        // L63 (`handle.Close` is nil)

        // L65
        Self::sort_list(cx, me);
        // L66-L67 (`self.UpdateScrollbar` and `self.Redraw` are nil)
    }

    /// lua: FolderListControl.lua:70-72 OpenFolder(folderName)
    /// @pob Classes/FolderListControl.lua:70-72 h=3718c649
    pub fn open_folder(cx: &mut Cx, me: CtrlId, folder_name: &str) {
        // L71
        let path = cx.child_as::<PathControl>(me, "path");
        let sub_path = format!("{}{folder_name}/", cx.w::<Self>(me).sub_path);
        PathControl::set_sub_path(cx, path.id(), &sub_path, false);
    }
}

/// lua: FolderListControl.lua:74-78 GetRowValue(column, index, folder)
/// @pob Classes/FolderListControl.lua:74-78 h=5c9ff6b4
fn get_row_value(_cx: &Cx, _me: CtrlId, column: usize, _index: usize, v: &ListValue) -> String {
    // L75
    if column == 1 {
        // L76
        folder_of(v).name.clone()
    } else {
        // (nil: the list has one column)
        String::new()
    }
}

/// lua: FolderListControl.lua:80-84 OnSelClick(index, folder, doubleClick)
/// @pob Classes/FolderListControl.lua:80-84 h=ce42c124
fn on_sel_click(
    cx: &mut Cx,
    me: CtrlId,
    _index: usize,
    v: &ListValue,
    double_click: bool,
) -> Option<bool> {
    // L81
    if double_click {
        // L82
        FolderListControl::open_folder(cx, me, &folder_of(v).name);
    }
    None
}

/// lua: FolderListControl.lua:86-99 OnSelDelete(index, folder)
/// @pob Classes/FolderListControl.lua:86-99 h=e48b5dfc
fn on_sel_delete(cx: &mut Cx, me: CtrlId, _index: usize, v: &ListValue) {
    let folder = folder_of(v);
    // L87
    let spec = format!("{}/*", folder.full_file_name);
    if storage::file_search_any(cx, &spec, false) || storage::file_search_any(cx, &spec, true) {
        // L88
        popups::open_message_popup(cx, "Delete Folder", "The folder is not empty.");
    } else {
        // L90
        let (res, msg) = storage::remove_dir(cx, &folder.full_file_name);
        // L91
        if !res {
            // L92
            let msg = msg
                .unwrap_or_else(|| lua_error!("attempt to concatenate local 'msg' (a nil value)"));
            popups::open_message_popup(
                cx,
                "Error",
                &format!("Couldn't delete '{}': {msg}", folder.full_file_name),
            );
            // L93
            return;
        }
        // L95
        FolderListControl::build_list(cx, me);
        let l = cx.w_mut::<ListControl>(me);
        // L96
        l.sel_index = None;
        // L97
        l.sel_value = None;
    }
}

static HOOKS: ListHooks = ListHooks {
    get_row_value,
    on_sel_click: Some(on_sel_click),
    on_sel_delete: Some(on_sel_delete),
    ..ListHooks::BASE
};

impl Widget for FolderListControl {
    const NAME: &'static str = "FolderListControl";
    const CAPS: Caps = <ListControl as Widget>::CAPS;

    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        ListControl::draw(cx, me, vp, no_tooltip);
    }
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        ListControl::is_mouse_over(cx, me)
    }
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, double_click: bool) -> Focus {
        ListControl::on_key_down(cx, me, key, double_click)
    }
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        ListControl::on_key_up(cx, me, key)
    }
    fn duck(cx: &Cx, me: CtrlId, q: Duck) -> bool {
        ListControl::duck(cx, me, q)
    }

    pob_ui_core::impl_parts!(FolderListControl: list => ListControl);
}
