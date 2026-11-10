//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/BuildListControl.lua 1-287
//! (`newClass("BuildListControl", "ListControl")`). Owner: U18.
//!
//! @pob-file Classes/BuildListControl.lua @16de4b82d
//!
//! The list is `listMode.list` itself (L17 passes the table): a [`SharedVecSource`] over the
//! mode's `Shared` vector, so BuildList.lua's `wipeTable`/`t_insert`/`SortList` on it are what the
//! control draws. Rows are [`EntryRef`]s (Lua table identity).
//!
//! ListControl hooks (U12's frozen table): `GetRowValue`, `GetDragValue`, `CanDragToValue`,
//! `OnSelClick`, `OnSelCopy`, `OnSelCut`, `OnSelDelete`, `OnSelKeyDown`. `CanReceiveDrag` /
//! `ReceiveDrag` are the framework's drag-target methods (`Caps::CAN_RECEIVE_DRAG`). The
//! `controls.path` PathControl gets per-instance `CanReceiveDrag`/`ReceiveDrag` overrides (L33-57).
//!
//! Popup closures capture the popup id and look later siblings up at call time (ui doc §6.10).
//! Props with side effects (ui doc §7.3): none.

use std::rc::Rc;

use pob_lua::pattern;
use pob_lua::string::lower;
use pob_lua::Shared;
use pob_ui_core::controls::button::ButtonControl;
use pob_ui_core::controls::edit::{EditArgs, EditControl};
use pob_ui_core::controls::label::LabelControl;
use pob_ui_core::controls::list::{
    ListColumn, ListControl, ListData, ListHooks, ListValue, SharedVecSource,
};
use pob_ui_core::controls::path::PathControl;
use pob_ui_core::popups::{self, PopupOpts};
use pob_ui_core::prelude::*;

use super::helpers::{can_move_to_sub_path, Entry, EntryRef};
use super::ListMode;
use crate::main_mode::folders;
use crate::main_mode::on_frame::set_mode;
use crate::main_mode::ModeArg;
use crate::platform::storage::{self, build_path};

// @pob-skip Classes/BuildListControl.lua:1-11 reason="header comment, locals, require and newClass (the Widget impl below)"

/// `BuildListControl`: a ListControl over `listMode.list`.
pub struct BuildListControl {
    pub list: ListControl,
    /// `self.listMode`
    pub list_mode: Ctrl<ListMode>,
    /// `self.copyBuild` (OnSelCopy; BuildList.lua's Ctrl+V reads it)
    pub copy_build: Option<EntryRef>,
    /// `self.cutBuild`
    pub cut_build: Option<EntryRef>,
}

/// The row value as an entry.
pub fn entry_of(v: &ListValue) -> EntryRef {
    v.rc::<Entry>()
        .unwrap_or_else(|| lua_error!("BuildListControl: a list value that is not a build entry"))
}

/// The drag payload (`"Build", build`) as an entry.
fn drag_entry(v: &DragValue) -> EntryRef {
    v.0.clone()
        .downcast::<Entry>()
        .unwrap_or_else(|_| lua_error!("BuildListControl: a drag value that is not a build entry"))
}

/// `listMode.subPath` (always a string once Init ran).
fn mode_sub_path(cx: &Cx, lm: Ctrl<ListMode>) -> String {
    cx.get(lm).sub_path.clone().unwrap_or_default()
}

/// `"Couldn't move '"..build.fullFileName.."' to '"..destPath.."': "..(msg or "")`, shared by the
/// two ReceiveDrag bodies (L41-L48 / L239-L247). Returns false when the move failed (the popup is
/// open and the caller returns).
fn move_build(cx: &mut Cx, lm: Ctrl<ListMode>, build: &Entry, target_sub_path: &str) -> bool {
    let bp = build_path(cx);
    if let Some(folder_name) = &build.folder_name {
        // L42 / L240
        folders::move_folder(
            cx,
            folder_name,
            &format!("{bp}{}", build.sub_path),
            &format!("{bp}{target_sub_path}"),
        );
    } else {
        let file_name = build
            .file_name
            .clone()
            .unwrap_or_else(|| lua_error!("attempt to concatenate field 'fileName' (a nil value)"));
        // L44 / L242
        let dest_path = ListMode::get_dest_name(cx, lm.id(), target_sub_path, &file_name);
        // L45 / L243
        let (res, msg) = storage::os_rename(cx, &build.full_file_name, &dest_path);
        // L46 / L244
        if !res {
            // L47 / L245
            popups::open_message_popup(
                cx,
                "Error",
                &format!(
                    "Couldn't move '{}' to '{dest_path}': {}",
                    build.full_file_name,
                    msg.unwrap_or_default()
                ),
            );
            // L48 / L246
            return false;
        }
    }
    true
}

impl BuildListControl {
    /// lua: BuildListControl.lua:16-63 BuildListControl(anchor, rect, listMode)
    /// `list` and `sub_path` are `listMode.list` and `listMode.subPath` (read by L17 and L24).
    /// @pob Classes/BuildListControl.lua:13-63 h=deb22e68
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        list_mode: Ctrl<ListMode>,
        list: Shared<Vec<EntryRef>>,
        sub_path: Option<String>,
    ) -> New<BuildListControl> {
        // L17
        let mut l = ListControl::state(
            &HOOKS,
            20.0,
            true,
            false,
            ListData::Source(Rc::new(SharedVecSource(list))),
            false,
        );
        // L19-L22
        l.col_list = vec![
            ListColumn {
                // L20
                width: Some(Prop::f(|cx, me| cx.width(me) - 172.0)),
                ..ListColumn::default()
            },
            // L21
            ListColumn::default(),
        ];
        // L23
        l.show_row_separators = true;
        New::new(
            Base::new(anchor, rect),
            BuildListControl {
                list: l,
                // L18
                list_mode,
                copy_build: None,
                cut_build: None,
            },
        )
        .host()
        .init(move |cx, me| {
            let me = me.id();
            // L17 (the scroll bars)
            ListControl::init(cx, me);
            // L24
            let width = cx.base(me).width.raw();
            let on_change: Rc<dyn Fn(&mut Cx, &str)> = Rc::new(move |cx, sub_path| {
                // L25
                cx.get_mut(list_mode).sub_path = Some(sub_path.to_owned());
                // L26
                ListMode::build_list(cx, list_mode.id());
                let l = cx.w_mut::<ListControl>(me);
                // L27
                l.sel_index = None;
                // L28
                l.sel_value = None;
                // L29
                l.sel_dragging = false;
                // L30
                l.sel_drag_active = false;
                // L31
                l.other_drag_source = None;
            });
            let path = cx.add(
                me,
                "path",
                PathControl::new(
                    Anchor::new(Bottom, me, Top),
                    pob_ui_core::layout::rect(0, -2, width, 24),
                    build_path(cx),
                    sub_path,
                    Some(on_change),
                ),
            );
            cx.set_ov(path, |ov| {
                // L33
                ov.can_receive_drag = Some(Rc::new(|cx, path, ty, _build| {
                    // L34
                    ty == "Build" && cx.w::<PathControl>(path).folder_list.len() > 1
                }));
                // L36
                ov.receive_drag = Some(Rc::new(move |cx, path, ty, build, _source| {
                    // L37
                    if ty == "Build" {
                        let build = drag_entry(build);
                        // L38
                        let mut index = 1;
                        while let Some(folder) = cx
                            .w::<PathControl>(path)
                            .folder_list
                            .get(index - 1)
                            .cloned()
                        {
                            let button = folder.button.unwrap_or_else(|| {
                                lua_error!("attempt to index field 'button' (a nil value)")
                            });
                            // L39
                            if cx.is_mouse_over(button.id()) {
                                // L40
                                if can_move_to_sub_path(&build, &folder.path) {
                                    // L41-L49
                                    if !move_build(cx, list_mode, &build, &folder.path) {
                                        return;
                                    }
                                    // L51
                                    ListMode::build_list(cx, list_mode.id());
                                }
                                // L53
                                break;
                            }
                            index += 1;
                        }
                    }
                }));
            });
            // L58
            cx.w_mut::<ListControl>(me).drag_target_list = Some(vec![path.id(), me]);
            // L59
            cx.base_mut(path).width = Prop::f(move |cx, _| {
                // L60 self.width()
                cx.width(me)
            });
        })
    }

    /// lua: BuildListControl.lua:65-76 SelByFullFileName(fullFileName)
    /// @pob Classes/BuildListControl.lua:65-76 h=5cdf35bf
    pub fn sel_by_full_file_name(cx: &mut Cx, me: CtrlId, full_file_name: Option<&str>) {
        // L66
        if let Some(full_file_name) = full_file_name {
            // L67
            let mut index = 1;
            while let Some(v) = ListControl::list_get(cx, me, index) {
                // L68
                if entry_of(&v).full_file_name == full_file_name {
                    // L69
                    ListControl::select_index(cx, me, index);
                    // L70
                    return;
                }
                index += 1;
            }
        }
        let l = cx.w_mut::<ListControl>(me);
        // L74
        l.sel_index = None;
        // L75
        l.sel_value = None;
    }

    /// lua: BuildListControl.lua:78-84 LoadBuild(build)
    /// @pob Classes/BuildListControl.lua:78-84 h=728a6aea
    pub fn load_build(cx: &mut Cx, me: CtrlId, build: &Entry) {
        // L79
        if let Some(folder_name) = &build.folder_name {
            // L80
            let path = cx.child_as::<PathControl>(me, "path");
            PathControl::set_sub_path(
                cx,
                path.id(),
                &format!("{}{folder_name}/", build.sub_path),
                false,
            );
        } else {
            // L82
            set_mode(
                cx,
                "BUILD",
                vec![
                    ModeArg::Str(build.full_file_name.clone()),
                    ModeArg::from(build.build_name.clone()),
                ],
            );
        }
    }

    /// lua: BuildListControl.lua:86-93 NewFolder()
    /// @pob Classes/BuildListControl.lua:86-93 h=667d1286
    pub fn new_folder(cx: &mut Cx, me: CtrlId) {
        let lm = cx.w::<Self>(me).list_mode;
        // L87
        let path = format!("{}{}", build_path(cx), mode_sub_path(cx, lm));
        popups::open_new_folder_popup(
            cx,
            &path,
            Some(Rc::new(move |cx, new_folder_name| {
                // L88
                if new_folder_name.is_some() {
                    // L89
                    ListMode::build_list(cx, lm.id());
                }
                // L91
                cx.select_control(lm.id(), Some(me));
            })),
        );
    }

    /// lua: BuildListControl.lua:95-160 RenameBuild(build, copyOnName)
    /// @pob Classes/BuildListControl.lua:95-160 h=3281a5c9
    pub fn rename_build(cx: &mut Cx, me: CtrlId, build: EntryRef, copy_on_name: bool) {
        let lm = cx.w::<Self>(me).list_mode;
        // L159 the size and title (Lua evaluates the arguments after the constructors; these
        // draw nothing)
        let title = format!(
            "{}{}",
            if copy_on_name { "Copy " } else { "Rename " },
            if build.folder_name.is_some() {
                "Folder"
            } else {
                "Build"
            }
        );
        popups::open_popup(
            cx,
            370.0,
            100.0,
            title,
            move |cx, p| {
                let p = p.id();
                // L96 local controls = { }
                // L97
                cx.add(
                    p,
                    "label",
                    LabelControl::new(
                        Anchor::NONE,
                        rect(0, 20, 0, 16),
                        format!(
                            "^7Enter the new name for this {}",
                            if build.folder_name.is_some() {
                                "folder:"
                            } else {
                                "build:"
                            }
                        ),
                    ),
                );
                // L98
                let b = build.clone();
                let init = b.folder_name.clone().or_else(|| b.build_name.clone());
                cx.add(
                    p,
                    "edit",
                    EditControl::new(
                        Anchor::NONE,
                        rect(0, 40, 350, 20),
                        EditArgs {
                            init,
                            prompt: None,
                            filter: Some("\\/:%*%?\"<>|%c".to_owned()),
                            limit: Some(100),
                            change_func: Some(Rc::new(move |cx, buf| {
                                let save = cx.child_as::<ButtonControl>(p, "save");
                                // L99
                                cx.base_mut(save).enabled = Prop::Val(false);
                                // L100
                                if b.folder_name.is_some() {
                                    // L101
                                    if pattern::find(buf, "%S", None, false).is_some() {
                                        // L102
                                        cx.base_mut(save).enabled = Prop::Val(true);
                                    }
                                } else {
                                    // L105
                                    if pattern::find(buf, "%S", None, false).is_some() {
                                        let build_name = b.build_name.clone().unwrap_or_else(|| {
                                            lua_error!("attempt to index field 'buildName' (a nil value)")
                                        });
                                        // L106
                                        if lower(buf) != lower(&build_name) {
                                            // L107
                                            let new_name = format!("{buf}.xml");
                                            // L108
                                            let new_file = storage::file_exists(
                                                cx,
                                                &format!("{}{}{new_name}", build_path(cx), b.sub_path),
                                            );
                                            // L109
                                            if !new_file {
                                                // L112
                                                cx.base_mut(save).enabled = Prop::Val(true);
                                            }
                                        } else if buf != build_name {
                                            // L115
                                            cx.base_mut(save).enabled = Prop::Val(true);
                                        }
                                    }
                                }
                            })),
                            ..EditArgs::default()
                        },
                    ),
                );
                // L120
                let b = build.clone();
                let save = cx.add(
                    p,
                    "save",
                    ButtonControl::new(Anchor::NONE, rect(-45, 70, 80, 20), "Save", move |cx| {
                        // L121
                        let new_build_name =
                            cx.get(cx.child_as::<EditControl>(p, "edit")).buf.clone();
                        let bp = build_path(cx);
                        // L122
                        if b.folder_name.is_some() {
                            // L123
                            if copy_on_name {
                                // L124
                                folders::copy_folder(
                                    cx,
                                    &b.full_file_name,
                                    &format!("{bp}{}{new_build_name}", b.sub_path),
                                );
                            } else {
                                // L126
                                let (res, msg) = storage::os_rename(
                                    cx,
                                    &b.full_file_name,
                                    &format!("{bp}{}{new_build_name}", b.sub_path),
                                );
                                // L127
                                if !res {
                                    // L128
                                    popups::open_message_popup(
                                        cx,
                                        "Error",
                                        &format!(
                                            "Couldn't rename '{}' to '{new_build_name}': {}",
                                            b.full_file_name,
                                            msg_of(msg)
                                        ),
                                    );
                                    // L129
                                    return;
                                }
                            }
                            // L132
                            ListMode::build_list(cx, lm.id());
                        } else {
                            // L134
                            let new_file_name = format!("{new_build_name}.xml");
                            // L135
                            if copy_on_name {
                                // L136
                                let (res, msg) = storage::copy_file(
                                    cx,
                                    &b.full_file_name,
                                    &format!("{bp}{}{new_file_name}", b.sub_path),
                                );
                                // L137
                                if !res {
                                    // L138
                                    popups::open_message_popup(
                                        cx,
                                        "Error",
                                        &format!("Couldn't copy build: {}", msg_of(msg)),
                                    );
                                    // L139
                                    return;
                                }
                            } else {
                                // L142
                                let (res, msg) = storage::os_rename(
                                    cx,
                                    &b.full_file_name,
                                    &format!("{bp}{}{new_file_name}", b.sub_path),
                                );
                                // L143
                                if !res {
                                    // L144
                                    popups::open_message_popup(
                                        cx,
                                        "Error",
                                        &format!(
                                            "Couldn't rename '{}' to '{new_file_name}': {}",
                                            b.full_file_name,
                                            msg_of(msg)
                                        ),
                                    );
                                    // L145
                                    return;
                                }
                            }
                            // L148
                            ListMode::build_list(cx, lm.id());
                            // L149
                            let bp = build_path(cx);
                            BuildListControl::sel_by_full_file_name(
                                cx,
                                me,
                                Some(&format!("{bp}{}{new_file_name}", b.sub_path)),
                            );
                        }
                        // L151
                        popups::close_popup(cx);
                        // L152
                        cx.select_control(lm.id(), Some(me));
                    }),
                );
                // L154
                cx.base_mut(save).enabled = Prop::Val(false);
                // L155
                cx.add(
                    p,
                    "cancel",
                    ButtonControl::new(Anchor::NONE, rect(45, 70, 80, 20), "Cancel", move |cx| {
                        // L156
                        popups::close_popup(cx);
                        // L157
                        cx.select_control(lm.id(), Some(me));
                    }),
                );
            },
            // L159
            PopupOpts {
                enter: Some("save".into()),
                default: Some("edit".into()),
                ..PopupOpts::default()
            },
        );
    }

    /// lua: BuildListControl.lua:162-189 DeleteBuild(build)
    /// @pob Classes/BuildListControl.lua:162-189 h=dd5a9539
    pub fn delete_build(cx: &mut Cx, me: CtrlId, build: EntryRef) {
        let lm = cx.w::<Self>(me).list_mode;
        // L163
        if let Some(folder_name) = &build.folder_name {
            // L164
            let spec = format!("{}/*", build.full_file_name);
            if storage::file_search_any(cx, &spec, false)
                || storage::file_search_any(cx, &spec, true)
            {
                // L165
                let b = build.clone();
                popups::open_confirm_popup(
                    cx,
                    "Confirm Folder Delete",
                    &format!(
                        "The folder is not empty.\nAre you sure you want to delete folder:\n{folder_name}\nThis cannot be undone."
                    ),
                    "Delete",
                    move |cx| {
                        // L166
                        storage::remove_dir_recursive(cx, &b.full_file_name);
                        // L167
                        ListMode::build_list(cx, lm.id());
                        let l = cx.w_mut::<ListControl>(me);
                        // L168
                        l.sel_index = None;
                        // L169
                        l.sel_value = None;
                    },
                    None,
                );
            } else {
                // L172
                let (res, msg) = storage::remove_dir(cx, &build.full_file_name);
                // L173
                if !res {
                    // L174
                    popups::open_message_popup(
                        cx,
                        "Error",
                        &format!(
                            "Couldn't delete '{}': {}",
                            build.full_file_name,
                            msg_of(msg)
                        ),
                    );
                    // L175
                    return;
                }
                // L177
                ListMode::build_list(cx, lm.id());
                let l = cx.w_mut::<ListControl>(me);
                // L178
                l.sel_index = None;
                // L179
                l.sel_value = None;
            }
        } else {
            let build_name = build.build_name.clone().unwrap_or_else(|| {
                lua_error!("attempt to concatenate field 'buildName' (a nil value)")
            });
            // L182
            let b = build.clone();
            popups::open_confirm_popup(
                cx,
                "Confirm Delete",
                &format!(
                    "Are you sure you want to delete build:\n{build_name}\nThis cannot be undone."
                ),
                "Delete",
                move |cx| {
                    // L183
                    storage::os_remove(cx, &b.full_file_name);
                    // L184
                    ListMode::build_list(cx, lm.id());
                    let l = cx.w_mut::<ListControl>(me);
                    // L185
                    l.sel_index = None;
                    // L186
                    l.sel_value = None;
                },
                None,
            );
        }
    }
}

/// `msg` in a concatenation (nil is a Lua error).
fn msg_of(msg: Option<String>) -> String {
    msg.unwrap_or_else(|| lua_error!("attempt to concatenate local 'msg' (a nil value)"))
}

/// lua: BuildListControl.lua:191-224 GetRowValue(column, index, build)
/// @pob Classes/BuildListControl.lua:191-224 h=a836908f
fn get_row_value(cx: &Cx, me: CtrlId, column: usize, _index: usize, v: &ListValue) -> String {
    let build = entry_of(v);
    let w = cx.w::<BuildListControl>(me);
    // L192
    if column == 1 {
        // L193
        let label;
        // L194
        let mut sub_path_prefix = String::new();
        // L195
        let base_sub = cx.get(w.list_mode).sub_path.as_deref();
        if let Some(base_sub) = base_sub.filter(|s| build.sub_path != *s) {
            // L196 local baseSub = self.listMode.subPath
            // L197
            if pob_ui_core::luastr::sub(&build.sub_path, 1.0, Some(base_sub.len() as f64))
                == base_sub
            {
                // L198
                sub_path_prefix =
                    pob_ui_core::luastr::sub(&build.sub_path, base_sub.len() as f64 + 1.0, None)
                        .into_owned();
            } else {
                // L200
                sub_path_prefix = build.sub_path.clone();
            }
        }
        // L203
        if let Some(folder_name) = &build.folder_name {
            // L204
            label = format!(">> {sub_path_prefix}{folder_name}");
        } else {
            // L206
            label = format!(
                "{sub_path_prefix}{}",
                build.build_name.as_deref().unwrap_or("?")
            );
        }
        // L208
        if let Some(cut) = &w.cut_build {
            if cut.build_name == build.build_name
                && cut.folder_name == build.folder_name
                && cut.sub_path == build.sub_path
            {
                // L209
                return format!("^xC0B0B0{label}");
            }
        }
        // L211
        label
    } else if column == 2 {
        // L214
        if build.build_name.is_some() {
            // L215-L219
            let color = build
                .class_name
                .as_deref()
                .and_then(|c| cx.main.cc.get(&pob_lua::string::upper(c)).cloned())
                .unwrap_or_else(|| "^7".to_owned());
            let class = match &build.ascend_class_name {
                Some(a) if a != "None" => a.as_str(),
                _ => build.class_name.as_deref().unwrap_or("?"),
            };
            pob_lua::lfmt!(
                "%sLevel %d %s",
                color.as_str(),
                build.level.unwrap_or(1.0),
                class
            )
        } else {
            // L221
            String::new()
        }
    } else {
        // L224 (no return value: nil; ListControl draws two columns only)
        String::new()
    }
}

/// lua: BuildListControl.lua:226-228 GetDragValue(index, build)
/// @pob Classes/BuildListControl.lua:226-228 h=1266ec2a
fn get_drag_value(
    _cx: &Cx,
    _me: CtrlId,
    _index: usize,
    v: &ListValue,
) -> Option<(Rc<str>, DragValue)> {
    // L227
    Some((Rc::from("Build"), DragValue(v.data.clone())))
}

/// lua: BuildListControl.lua:255-257 CanDragToValue(index, build, source)
/// @pob Classes/BuildListControl.lua:255-257 h=464af325
fn can_drag_to_value(
    cx: &Cx,
    _me: CtrlId,
    _index: usize,
    v: &ListValue,
    source: Option<CtrlId>,
) -> bool {
    let build = entry_of(v);
    // L256
    let Some(folder_name) = &build.folder_name else {
        return false;
    };
    let source =
        source.unwrap_or_else(|| lua_error!("attempt to index local 'source' (a nil value)"));
    let sel = cx.w::<ListControl>(source).sel_value.clone();
    if sel.as_ref() == Some(v) {
        return false;
    }
    let sel = sel.unwrap_or_else(|| lua_error!("attempt to index local 'build' (a nil value)"));
    can_move_to_sub_path(
        &entry_of(&sel),
        &format!("{}{folder_name}/", build.sub_path),
    )
}

/// lua: BuildListControl.lua:259-265 OnSelClick(index, build, doubleClick)
/// @pob Classes/BuildListControl.lua:259-265 h=cb503772
fn on_sel_click(
    cx: &mut Cx,
    me: CtrlId,
    _index: usize,
    v: &ListValue,
    double_click: bool,
) -> Option<bool> {
    // L260
    if double_click {
        // L261
        BuildListControl::load_build(cx, me, &entry_of(v));
        let l = cx.w_mut::<ListControl>(me);
        // L262
        l.sel_dragging = false;
        // L263
        l.sel_drag_active = false;
    }
    None
}

/// lua: BuildListControl.lua:267-270 OnSelCopy(index, build)
/// @pob Classes/BuildListControl.lua:267-270 h=3e6c2534
fn on_sel_copy(cx: &mut Cx, me: CtrlId, _index: usize, v: &ListValue) {
    let w = cx.w_mut::<BuildListControl>(me);
    // L268
    w.copy_build = Some(entry_of(v));
    // L269
    w.cut_build = None;
}

/// lua: BuildListControl.lua:272-275 OnSelCut(index, build)
/// @pob Classes/BuildListControl.lua:272-275 h=00c24825
fn on_sel_cut(cx: &mut Cx, me: CtrlId, _index: usize, v: &ListValue) {
    let w = cx.w_mut::<BuildListControl>(me);
    // L273
    w.copy_build = None;
    // L274
    w.cut_build = Some(entry_of(v));
}

/// lua: BuildListControl.lua:277-279 OnSelDelete(index, build)
/// @pob Classes/BuildListControl.lua:277-279 h=763e1210
fn on_sel_delete(cx: &mut Cx, me: CtrlId, _index: usize, v: &ListValue) {
    // L278
    BuildListControl::delete_build(cx, me, entry_of(v));
}

/// lua: BuildListControl.lua:281-287 OnSelKeyDown(index, build, key)
/// @pob Classes/BuildListControl.lua:281-287 h=68d3cf14
fn on_sel_key_down(cx: &mut Cx, me: CtrlId, _index: usize, v: &ListValue, key: Key) {
    // L282
    if key == Key::Return {
        // L283
        BuildListControl::load_build(cx, me, &entry_of(v));
    } else if key == Key::F(2) {
        // L285
        BuildListControl::rename_build(cx, me, entry_of(v), false);
    }
}

/// The class's ListControl hook table.
static HOOKS: ListHooks = ListHooks {
    get_row_value,
    get_drag_value: Some(get_drag_value),
    can_drag_to_value: Some(can_drag_to_value),
    on_sel_click: Some(on_sel_click),
    on_sel_copy: Some(on_sel_copy),
    on_sel_cut: Some(on_sel_cut),
    on_sel_delete: Some(on_sel_delete),
    on_sel_key_down: Some(on_sel_key_down),
    ..ListHooks::BASE
};

impl Widget for BuildListControl {
    const NAME: &'static str = "BuildListControl";
    const CAPS: Caps = <ListControl as Widget>::CAPS
        .union(Caps::CAN_RECEIVE_DRAG)
        .union(Caps::RECEIVE_DRAG);

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

    /// lua: BuildListControl.lua:230-232 CanReceiveDrag(type, build)
    /// @pob Classes/BuildListControl.lua:230-232 h=6b23894b
    fn can_receive_drag(_cx: &Cx, _me: CtrlId, ty: &str, _v: &DragValue) -> bool {
        // L231
        ty == "Build"
    }

    /// lua: BuildListControl.lua:234-253 ReceiveDrag(type, build, source)
    /// @pob Classes/BuildListControl.lua:234-253 h=2d7b2413
    fn receive_drag(cx: &mut Cx, me: CtrlId, ty: &str, v: &DragValue, _source: CtrlId) {
        // L235
        if ty == "Build" {
            // L236
            let hover = cx.w::<ListControl>(me).hover_value.as_ref().map(entry_of);
            if let Some(hover) = hover.filter(|h| h.folder_name.is_some()) {
                let build = drag_entry(v);
                let lm = cx.w::<BuildListControl>(me).list_mode;
                // L237
                let target_sub_path = format!(
                    "{}{}/",
                    hover.sub_path,
                    hover.folder_name.as_deref().unwrap_or_default()
                );
                // L238
                if can_move_to_sub_path(&build, &target_sub_path) {
                    // L239-L248
                    if !move_build(cx, lm, &build, &target_sub_path) {
                        return;
                    }
                    // L249
                    ListMode::build_list(cx, lm.id());
                }
            }
        }
    }

    pob_ui_core::impl_parts!(BuildListControl: list => ListControl);
}

/// `self.copyBuild`/`self.cutBuild` and the selection, for BuildList.lua.
impl BuildListControl {
    /// `self.selValue` as an entry.
    pub fn sel_value(cx: &Cx, me: CtrlId) -> Option<EntryRef> {
        cx.w::<ListControl>(me).sel_value.as_ref().map(entry_of)
    }
}
