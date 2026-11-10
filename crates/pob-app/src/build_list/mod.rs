//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/BuildList.lua 1-220.
//! Owner: U18. Module declarations: UI-U0.
//!
//! @pob-file Modules/BuildList.lua @16de4b82d
//!
//! `listMode = new("ControlHost"):ControlHost()` is the [`ListMode`] node (class name
//! "ControlHost", what the control-path markers print), created the first time `main:CallMode`
//! reaches the mode and stored in `AppState::list_mode`. `main.modes.LIST` is [`mode()`]'s
//! [`Mode`] object (CCR-0096 A), which forwards to the node's methods.
//!
//! `main.filterBuildList` is read and written only by this file (grep), so it lives on the node
//! ([`ListMode::filter_build_list`]); `self.filterBuildList` (L96) is never assigned, so the search
//! box starts empty.
//!
//! `main:DrawBackground` (L179) is U17's `main_mode::background::draw_background`, which needs U23's
//! tree assets; CCR-0100 adds a test seam ([`set_draw_background_override`]) that the product never
//! sets.
//!
//! The `main.showPublicBuilds` branches are `if false` in upstream (L20, L72, L79, L92), so
//! `getPublicBuilds` (L111-L127, U52's ExtBuildListControl) is never called.
//!
//! DIVERGENCE(number-arg): a `subPath` / `selBuildName` mode argument read from Settings.xml as a
//! number (`<Arg number=..>`) stays a number in Lua and is only ever concatenated; the port
//! converts it with `tostring` once. The one visible difference would be `entry.subPath ==
//! subPath` in FilterList comparing a string with a number (always false in Lua). PoB never
//! writes such an argument.
//!
//! Props with side effects (ui doc §7.3): none.

pub mod build_list_control;
pub mod folder_list_control;
pub mod helpers;

use std::cell::RefCell;
use std::rc::Rc;

use pob_lua::Shared;
use pob_ui_core::controls::button::ButtonControl;
use pob_ui_core::controls::drop_down::{DropDownControl, SelFunc};
use pob_ui_core::controls::edit::{EditArgs, EditControl};
use pob_ui_core::controls::path::PathControl;
use pob_ui_core::popups;
use pob_ui_core::prelude::*;

use self::build_list_control::BuildListControl;
use self::helpers::{EntryRef, SortDropItem};
use crate::app::CxApp;
use crate::main_mode::on_frame::set_mode;
use crate::main_mode::{folders, mode_arg, Mode, ModeArg};
use crate::platform::storage::{self, build_path};

// @pob-skip Modules/BuildList.lua:1-5 reason="header comment"
// @pob Modules/BuildList.lua:6-13 h=9c4198f5 (locals, require, and the ControlHost: ListMode::node)

/// The BuildList mode object (`main.modes["LIST"]`, `listMode`): a ControlHost node.
#[derive(Default)]
pub struct ListMode {
    /// `self.initialised`
    pub initialised: bool,
    /// `self.anchor`
    pub anchor: Option<CtrlId>,
    /// `self.subPath` (nil before the first Init; Build.lua:71 reads it)
    pub sub_path: Option<String>,
    /// `self.list` (aliased by `controls.buildList`)
    pub list: Shared<Vec<EntryRef>>,
    /// `self.buildIndex`
    pub build_index: Vec<EntryRef>,
    /// `main.filterBuildList` (see the module notes)
    pub filter_build_list: Option<String>,
    /// `self.viewMode` (main:OnFrame L395 may assign it; nothing reads it here)
    pub view_mode: Option<String>,
}

impl Widget for ListMode {
    const NAME: &'static str = "ControlHost";
    const CAPS: Caps = Caps::empty();
}

thread_local! {
    // CCR-0100: test seam for main:DrawBackground (never set by the product).
    static DRAW_BACKGROUND: RefCell<Option<Rc<dyn Fn(&mut Cx, &ViewPort)>>> = const { RefCell::new(None) };
}

/// CCR-0100: replace `main:DrawBackground` in BuildList's OnFrame (tests only; `None` restores
/// the real one). The product never calls this.
pub fn set_draw_background_override(f: Option<Rc<dyn Fn(&mut Cx, &ViewPort)>>) {
    DRAW_BACKGROUND.with(|d| *d.borrow_mut() = f);
}

/// `main:DrawBackground(viewPort)`
fn draw_background(cx: &mut Cx, vp: &ViewPort) {
    if let Some(f) = DRAW_BACKGROUND.with(|d| d.borrow().clone()) {
        return f(cx, vp);
    }
    crate::main_mode::background::draw_background(cx, vp);
}

/// `ModeArg` in a concatenation (`..`): strings and numbers; anything else is a Lua error.
fn concat_arg(a: &ModeArg, what: &str) -> String {
    match a {
        ModeArg::Str(s) => s.clone(),
        ModeArg::Num(n) => pob_lua::tostring_num(*n),
        ModeArg::Bool(_) => lua_error!("attempt to concatenate local '{what}' (a boolean value)"),
        ModeArg::Nil => lua_error!("attempt to concatenate local '{what}' (a nil value)"),
    }
}

impl ListMode {
    /// The `listMode` node (`new("ControlHost"):ControlHost()`, L13), created on first use.
    pub fn node(cx: &mut Cx) -> Ctrl<ListMode> {
        if let Some(n) = cx.app().list_mode {
            return n;
        }
        // L13
        let n = cx.add_free(
            None,
            New::new(
                Base::new(Anchor::NONE, rect(0, 0, 0, 0)),
                ListMode::default(),
            )
            .host(),
        );
        cx.app_mut().list_mode = Some(n);
        n
    }

    /// `self.controls.buildList`
    fn build_list_ctrl(cx: &Cx, me: CtrlId) -> CtrlId {
        cx.child_as::<BuildListControl>(me, "buildList").id()
    }

    /// `selBuildName and main.buildPath..self.subPath..selBuildName..".xml"`
    fn sel_full_file_name(cx: &Cx, me: CtrlId, sel_build_name: &ModeArg) -> Option<String> {
        if !sel_build_name.truthy() {
            return None;
        }
        let sub_path = cx.w::<Self>(me).sub_path.clone().unwrap_or_default();
        Some(format!(
            "{}{sub_path}{}.xml",
            build_path(cx),
            concat_arg(sel_build_name, "selBuildName")
        ))
    }

    /// lua: BuildList.lua:15-109 listMode:Init(selBuildName, subPath)
    /// @pob Modules/BuildList.lua:15-109 h=6f6797f1
    pub fn init(cx: &mut Cx, me: CtrlId, sel_build_name: ModeArg, sub_path: ModeArg) {
        // `subPath or self.subPath` / `subPath or ""`
        let sub_path = sub_path.truthy().then(|| concat_arg(&sub_path, "subPath"));
        // L16
        if cx.w::<Self>(me).initialised {
            // L17
            let s = sub_path.or_else(|| cx.w::<Self>(me).sub_path.clone());
            cx.w_mut::<Self>(me).sub_path = s;
            // L18
            let path = cx.child_as::<PathControl>(Self::build_list_ctrl(cx, me), "path");
            let s = cx.w::<Self>(me).sub_path.clone().unwrap_or_else(|| {
                lua_error!("bad argument #1 to 'gmatch' (string expected, got nil)")
            });
            PathControl::set_sub_path(cx, path.id(), &s, false);
            // L19-L20 `if false` (main.showPublicBuilds)
            // L23
            cx.remove_child(me, &CtrlKey::from("ExtBuildList"));
            // L25
            Self::build_list(cx, me);
            // L26
            let f = Self::sel_full_file_name(cx, me, &sel_build_name);
            let bl = Self::build_list_ctrl(cx, me);
            BuildListControl::sel_by_full_file_name(cx, bl, f.as_deref());
            // L27
            cx.select_control(me, Some(bl));
            // L28
            return;
        }

        // L31
        let anchor = cx.add_free(Some(me), PlainControl::new(Anchor::NONE, rect(0, 4, 0, 0)));
        // L32
        cx.base_mut(anchor).x = Prop::f(|cx, _| {
            // L33
            cx.main.screen_w / 2.0
        });
        cx.w_mut::<Self>(me).anchor = Some(anchor.id());
        let anchor = anchor.id();

        // L36
        cx.w_mut::<Self>(me).sub_path = Some(sub_path.unwrap_or_default());
        // L37
        cx.w_mut::<Self>(me).list = Shared::new(Vec::new());

        // L39
        let new = cx.add(
            me,
            "new",
            ButtonControl::new(
                Anchor::new(Top, anchor, Top),
                rect(-259, 0, 60, 20),
                "New",
                |cx| {
                    // L40
                    set_mode(
                        cx,
                        "BUILD",
                        vec![ModeArg::Bool(false), ModeArg::from("Unnamed build")],
                    );
                },
            ),
        );
        // L42
        let new_folder = cx.add(
            me,
            "newFolder",
            ButtonControl::new(
                Anchor::new(Left, new, Right),
                rect(8, 0, 90, 20),
                "New Folder",
                move |cx| {
                    // L43
                    BuildListControl::new_folder(cx, Self::build_list_ctrl(cx, me));
                },
            ),
        );
        // `self.controls.buildList.selValue ~= nil`
        let has_sel = move |cx: &Cx, _: CtrlId| {
            BuildListControl::sel_value(cx, Self::build_list_ctrl(cx, me)).is_some()
        };
        // `self.controls.buildList.selValue` passed where the callee indexes it
        let sel = move |cx: &Cx| {
            BuildListControl::sel_value(cx, Self::build_list_ctrl(cx, me))
                .unwrap_or_else(|| lua_error!("attempt to index local 'build' (a nil value)"))
        };
        // L45
        let open = cx.add(
            me,
            "open",
            ButtonControl::new(
                Anchor::new(Left, new_folder, Right),
                rect(8, 0, 60, 20),
                "Open",
                move |cx| {
                    // L46
                    let b = sel(cx);
                    BuildListControl::load_build(cx, Self::build_list_ctrl(cx, me), &b);
                },
            ),
        );
        // L48
        cx.base_mut(open).enabled = Prop::f(has_sel);
        // L49
        let copy = cx.add(
            me,
            "copy",
            ButtonControl::new(
                Anchor::new(Left, open, Right),
                rect(8, 0, 60, 20),
                "Copy",
                move |cx| {
                    // L50
                    let b = sel(cx);
                    BuildListControl::rename_build(cx, Self::build_list_ctrl(cx, me), b, true);
                },
            ),
        );
        // L52
        cx.base_mut(copy).enabled = Prop::f(has_sel);
        // L53
        let rename = cx.add(
            me,
            "rename",
            ButtonControl::new(
                Anchor::new(Left, copy, Right),
                rect(8, 0, 60, 20),
                "Rename",
                move |cx| {
                    // L54
                    let b = sel(cx);
                    BuildListControl::rename_build(cx, Self::build_list_ctrl(cx, me), b, false);
                },
            ),
        );
        // L56
        cx.base_mut(rename).enabled = Prop::f(has_sel);
        // L57
        let delete = cx.add(
            me,
            "delete",
            ButtonControl::new(
                Anchor::new(Left, rename, Right),
                rect(8, 0, 60, 20),
                "Delete",
                move |cx| {
                    // L58
                    let b = sel(cx);
                    BuildListControl::delete_build(cx, Self::build_list_ctrl(cx, me), b);
                },
            ),
        );
        // L60
        cx.base_mut(delete).enabled = Prop::f(has_sel);
        // L61
        let sel_func: SelFunc = Rc::new(move |cx, _index, value| {
            let value =
                value.unwrap_or_else(|| lua_error!("attempt to index local 'value' (a nil value)"));
            let item = value
                .value
                .downcast_ref::<SortDropItem>()
                .expect("a buildSortDropList row");
            // L62
            cx.app_mut().main.build_sort_mode = item.sort_mode.to_owned();
            // L63
            Self::sort_list(cx, me);
        });
        let sort = cx.add(
            me,
            "sort",
            DropDownControl::new(
                Anchor::new(Left, delete, Right),
                rect(8, 0, 140, 20),
                helpers::build_sort_drop_list(),
                Some(sel_func),
                None,
                false,
            ),
        );
        // L65
        let mode = cx.app().main.build_sort_mode.clone();
        DropDownControl::sel_by_value(cx, sort.id(), &|i| {
            i.value
                .downcast_ref::<SortDropItem>()
                .is_some_and(|v| v.sort_mode == mode)
        });
        // L66
        let (list, sp) = {
            let w = cx.w::<Self>(me);
            (w.list.clone(), w.sub_path.clone())
        };
        let build_list = cx.add(
            me,
            "buildList",
            BuildListControl::new(
                Anchor::new(Top, anchor, Top),
                rect(0, 75, 900, 0),
                Ctrl::from_raw(me),
                list,
                sp,
            ),
        );
        // L67
        cx.base_mut(build_list).height = Prop::f(|cx, _| {
            // L68
            cx.main.screen_h - 80.0
        });
        // L70-L77 buildListWidth (`if false`: main.showPublicBuilds)
        let build_list_width = || Prop::f(|_cx, _| 900.0);
        // L78-L86 buildListOffset
        let build_list_offset = || Prop::f(|_cx, _| 0.0);

        // L88
        cx.base_mut(build_list).width = build_list_width();
        // L89
        cx.base_mut(build_list).x = build_list_offset();

        // L91-L94 `if false` (main.showPublicBuilds)

        // L96
        let search_text = cx.add(
            me,
            "searchText",
            EditControl::new(
                Anchor::new(Top, anchor, Top),
                rect(0, 25, 640, 20),
                EditArgs {
                    // `self.filterBuildList` (never assigned: nil)
                    init: None,
                    prompt: Some("Search".to_owned()),
                    filter: Some("%c%(%)".to_owned()),
                    limit: Some(100),
                    change_func: Some(Rc::new(move |cx, buf| {
                        // L97
                        cx.w_mut::<Self>(me).filter_build_list = Some(buf.to_owned());
                        // L98
                        Self::filter_build_list(cx, me);
                    })),
                    clearable: true,
                    ..EditArgs::default()
                },
            ),
        );
        // L100
        EditControl::set_placeholder(
            cx,
            search_text.id(),
            "(e.g. class:assassin myfilename)",
            false,
        );
        // L101
        cx.base_mut(search_text).width = build_list_width();
        // L102
        cx.base_mut(search_text).x = build_list_offset();

        // L104
        Self::build_list(cx, me);
        // L105
        let f = Self::sel_full_file_name(cx, me, &sel_build_name);
        BuildListControl::sel_by_full_file_name(cx, build_list.id(), f.as_deref());
        // L106
        cx.select_control(me, Some(build_list.id()));

        // L108
        cx.w_mut::<Self>(me).initialised = true;
    }

    // @pob-skip Modules/BuildList.lua:111-127 reason="getPublicBuilds: only called under `if false` (L20, L92); ExtBuildListControl is U52's"

    /// lua: BuildList.lua:131-133 listMode:GetArgs()
    /// @pob Modules/BuildList.lua:131-133 h=c0c8dada
    pub fn get_args(cx: &Cx, me: CtrlId) -> Vec<ModeArg> {
        // L132
        let bl = Self::build_list_ctrl(cx, me);
        let sel = BuildListControl::sel_value(cx, bl)
            .and_then(|b| b.build_name.clone())
            .map_or(ModeArg::Bool(false), ModeArg::Str);
        vec![sel, ModeArg::from(cx.w::<Self>(me).sub_path.clone())]
    }

    /// lua: BuildList.lua:135-182 listMode:OnFrame(inputEvents)
    /// @pob Modules/BuildList.lua:135-182 h=11991896
    pub fn on_frame(cx: &mut Cx, me: CtrlId, input_events: &mut InputEvents) {
        let bl = Self::build_list_ctrl(cx, me);
        // L136
        let mut i = 0;
        while let Some(event) = input_events.next_ipairs(&mut i) {
            // L137
            if let EventKind::KeyDown { .. } = event.kind {
                // L138
                if event.key == Key::Char(b'v') && cx.sys.is_key_down(Key::Ctrl) {
                    // L139
                    if let Some(build) = cx.w::<BuildListControl>(bl).copy_build.clone() {
                        // L140
                        let sub_path = cx.w::<Self>(me).sub_path.clone().unwrap_or_default();
                        // L141
                        if build.sub_path != sub_path {
                            // L142
                            if !helpers::can_move_to_sub_path(&build, &sub_path) {
                                // L143
                                popups::open_message_popup(
                                    cx,
                                    "Error",
                                    "A folder cannot be copied into itself.",
                                );
                            } else if let Some(folder_name) = &build.folder_name {
                                // L145
                                let dst = format!("{}{sub_path}{folder_name}", build_path(cx));
                                folders::copy_folder(cx, &build.full_file_name, &dst);
                            } else {
                                // L147
                                let file_name = build.file_name.clone().unwrap_or_default();
                                let dst = Self::get_dest_name(cx, me, &sub_path, &file_name);
                                storage::copy_file(cx, &build.full_file_name, &dst);
                            }
                            // L149
                            Self::build_list(cx, me);
                        } else {
                            // L151
                            BuildListControl::rename_build(cx, bl, build, true);
                        }
                        // L153
                        cx.w_mut::<BuildListControl>(bl).copy_build = None;
                    } else if let Some(build) = cx.w::<BuildListControl>(bl).cut_build.clone() {
                        // L155
                        let sub_path = cx.w::<Self>(me).sub_path.clone().unwrap_or_default();
                        // L156
                        if build.sub_path != sub_path {
                            // L157
                            if !helpers::can_move_to_sub_path(&build, &sub_path) {
                                // L158
                                popups::open_message_popup(
                                    cx,
                                    "Error",
                                    "A folder cannot be moved into itself.",
                                );
                            } else if let Some(folder_name) = &build.folder_name {
                                // L160
                                let bp = build_path(cx);
                                folders::move_folder(
                                    cx,
                                    folder_name,
                                    &format!("{bp}{}", build.sub_path),
                                    &format!("{bp}{sub_path}"),
                                );
                            } else {
                                // L162
                                let file_name = build.file_name.clone().unwrap_or_default();
                                let dst = Self::get_dest_name(cx, me, &sub_path, &file_name);
                                storage::os_rename(cx, &build.full_file_name, &dst);
                            }
                            // L164
                            Self::build_list(cx, me);
                        }
                        // L166
                        cx.w_mut::<BuildListControl>(bl).cut_build = None;
                    }
                } else if event.key == Key::Char(b'n') && cx.sys.is_key_down(Key::Ctrl) {
                    // L169
                    set_mode(
                        cx,
                        "BUILD",
                        vec![ModeArg::Bool(false), ModeArg::from("Unnamed build")],
                    );
                } else if event.key == Key::Mouse4 {
                    // L171
                    let path = cx.child_as::<PathControl>(bl, "path");
                    PathControl::undo(cx, path.id());
                } else if event.key == Key::Mouse5 {
                    // L173
                    let path = cx.child_as::<PathControl>(bl, "path");
                    PathControl::redo(cx, path.id());
                }
            }
        }
        // L177
        let vp = cx.main.view_port;
        cx.process_controls_input(me, input_events, &vp);

        // L179
        draw_background(cx, &vp);

        // L181
        cx.draw_controls(me, &vp, None);
    }

    /// lua: BuildList.lua:184-199 listMode:GetDestName(subPath, fileName)
    /// @pob Modules/BuildList.lua:184-199 h=59f1c1e3
    pub fn get_dest_name(cx: &Cx, _me: CtrlId, sub_path: &str, file_name: &str) -> String {
        let bp = build_path(cx);
        // L185
        let mut i = 2;
        // L186
        let mut dest_name = file_name.to_owned();
        // L187
        loop {
            // L188
            let test = storage::file_exists(cx, &format!("{bp}{sub_path}{dest_name}"));
            // L189
            if test {
                // L191
                let base_name = pob_lua::pattern::gsub(file_name, "%.xml$", "", None).0;
                // L192
                dest_name = format!("{base_name}[{i}].xml");
                // L193
                i += 1;
            } else {
                // L195
                break;
            }
        }
        // L198
        format!("{bp}{sub_path}{dest_name}")
    }

    /// lua: BuildList.lua:201-204 listMode:BuildList()
    /// @pob Modules/BuildList.lua:201-204 h=91c2785c
    pub fn build_list(cx: &mut Cx, me: CtrlId) {
        // L202
        let sub_path = cx.w::<Self>(me).sub_path.clone();
        let index = helpers::scan_folder(cx, sub_path.as_deref());
        cx.w_mut::<Self>(me).build_index = index;
        // L203
        Self::filter_build_list(cx, me);
    }

    /// lua: BuildList.lua:206-212 listMode:FilterBuildList()
    /// @pob Modules/BuildList.lua:206-212 h=bfb965aa
    pub fn filter_build_list(cx: &mut Cx, me: CtrlId) {
        let w = cx.w::<Self>(me);
        // L207
        w.list.with_mut(Vec::clear);
        // L208
        let filtered = helpers::filter_list(
            Some(&w.build_index),
            w.sub_path.as_deref().unwrap_or_default(),
            w.filter_build_list.as_deref(),
        );
        for entry in filtered {
            // L209
            w.list.push(entry);
        }
        // L211
        Self::sort_list(cx, me);
    }

    /// lua: BuildList.lua:214-218 listMode:SortList()
    /// @pob Modules/BuildList.lua:214-218 h=fb4366f5
    pub fn sort_list(cx: &mut Cx, me: CtrlId) {
        let bl = Self::build_list_ctrl(cx, me);
        // L215
        let old_sel_full_file_name =
            BuildListControl::sel_value(cx, bl).map(|b| b.full_file_name.clone());
        // L216
        let mode = cx.app().main.build_sort_mode.clone();
        cx.w::<Self>(me)
            .list
            .with_mut(|l| helpers::sort_list(l, Some(&mode)));
        // L217
        BuildListControl::sel_by_full_file_name(cx, bl, old_sel_full_file_name.as_deref());
    }
}

/// `main.modes.LIST` (CCR-0096 A): BuildList.lua's module table as a [`Mode`].
struct ListModeObj;

impl Mode for ListModeObj {
    fn init(&self, cx: &mut Cx, args: &[ModeArg]) {
        let me = ListMode::node(cx).id();
        ListMode::init(cx, me, mode_arg(args, 1), mode_arg(args, 2));
    }
    /// lua: BuildList.lua:128-129 listMode:Shutdown()
    /// @pob Modules/BuildList.lua:128-129 h=a3bdefb8
    fn shutdown(&self, _cx: &mut Cx) {}
    fn on_frame(&self, cx: &mut Cx, input_events: &mut InputEvents, _vp: &ViewPort) {
        let me = ListMode::node(cx).id();
        ListMode::on_frame(cx, me, input_events);
    }
    fn get_args(&self, cx: &Cx) -> Option<Vec<ModeArg>> {
        let me = cx
            .app()
            .list_mode
            .unwrap_or_else(|| lua_error!("attempt to index field 'buildList' (a nil value)"));
        Some(ListMode::get_args(cx, me.id()))
    }
    fn view_mode(&self, cx: &Cx) -> Option<String> {
        cx.app().list_mode.and_then(|m| cx.get(m).view_mode.clone())
    }
    fn set_view_mode(&self, cx: &mut Cx, v: &str) {
        let me = ListMode::node(cx);
        cx.get_mut(me).view_mode = Some(v.to_owned());
    }
}

// @pob-skip Modules/BuildList.lua:220-220 reason="return listMode (mode() below)"

/// `LoadModule("Modules/BuildList")` (Main.lua:62): `main.modes["LIST"]`.
pub fn mode() -> Rc<dyn Mode> {
    Rc::new(ListModeObj)
}

/// `main.modes.LIST.subPath` (Build.lua:71; nil before the list mode's first Init).
pub fn sub_path(cx: &Cx) -> Option<String> {
    cx.app().list_mode.and_then(|m| cx.get(m).sub_path.clone())
}

/// `main.modes.LIST:BuildList()` (Main.lua:1219, the Options popup, U17).
pub fn rebuild(cx: &mut Cx) {
    let me = ListMode::node(cx).id();
    ListMode::build_list(cx, me);
}
