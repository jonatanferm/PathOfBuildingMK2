//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/NotesTab.lua 1-112. Owner: U22.
//!
//! @pob-file Classes/NotesTab.lua @16de4b82d
//!
//! The binding body is the ui doc §9.2 reference. The tab is a free node owned by the build node
//! (`self.notesTab`, a field of the build, not one of its `controls`); BuildMode reaches it
//! through `BuildState::tabs.notes`. The functions at the bottom ([`ctor_hook`],
//! [`load_ui_section`], [`save_ui_section`], [`draw_tab`], [`mod_flag`], [`set_mod_flag`]) are
//! what U19's `BuildHooks` and U20's `ShellHooks` call for the Notes tab (one match arm each
//! in `init::default_hooks` / `on_frame::default_shell_hooks`; the template for the other tabs).
//!
//! Lua semantics:
//! * `buf:gsub(p, r)` assigned to a field or concatenated keeps the first result
//!   (`luastr::gsub`); `GetSelText():match(p)` is `luastr::lmatch` (first capture or nil).
//! * `SetShowColorCodes` writes `edit.buf` directly, leaving `caret`/`sel` where they were (the
//!   Lua does the same; EditControl clamps nothing here, nor does the port).
//! * The colour buttons' labels read `colorCodes.X` at construction; their click handlers read it
//!   at click time (a closure over the global table), as the Lua does.
//! * `for id, event in ipairs(inputEvents)` stops at the first hole (`next_ipairs`).
//!
//! DIVERGENCE: none. ORDER: no `pairs` loops.
//!
//! Props with side effects (ui doc §7.3): none (`edit.width`/`edit.height` read the tab's raw
//! fields, set by Draw).

use pob_ui_core::color_codes::ColorCodes;
use pob_ui_core::controls::{ButtonControl, EditArgs, EditControl, LabelControl};
use pob_ui_core::luastr::{gsub, lmatch};
use pob_ui_core::prelude::*;

use crate::app::CxApp;
use crate::main_mode::background::draw_background;
use crate::model::xml::{XmlChild, XmlNode};

// @pob Classes/NotesTab.lua:1-10 h=9fc2438f
// (`local t_insert = table.insert`; `newClass("NotesTab", "ControlHost", "Control")`: the
// `Widget` impl below with `.host()` in the constructor)

/// `notesDesc` (NotesTab.lua:20-22; the long string keeps its newlines and the trailing tab).
const NOTES_DESC: &str = "^7You can use Ctrl +/- (or Ctrl+Scroll) to zoom in and out and Ctrl+0 to reset.\n\
This field also supports different colors.  Using the caret symbol (^) followed by a Hex code or a number (0-9) will set the color.\n\
Below are some common color codes PoB uses:\t";

pub struct NotesTab {
    /// `self.build`
    pub build: CtrlId,
    /// `self.lastContent`
    pub last_content: String,
    /// `self.showColorCodes`
    pub show_color_codes: bool,
    /// `self.modFlag` (nil until the first Draw: false)
    pub mod_flag: bool,
}

/// drawn by BuildMode
impl Widget for NotesTab {
    const NAME: &'static str = "NotesTab";
    const CAPS: Caps = Caps::empty();
}

/// `colorCodes.X` (read at construction for the label and at click time for SetColor).
type CcField = fn(&ColorCodes) -> &String;

impl NotesTab {
    /// lua: NotesTab.lua:12-51
    /// @pob Classes/NotesTab.lua:12-51 h=5bbf5c3a
    pub fn new(build: CtrlId) -> New<NotesTab> {
        // L13-14 self:ControlHost(); self:Control()
        New::new(
            Base::new(Anchor::NONE, rect(0, 0, 0, 0)),
            NotesTab {
                // L16
                build,
                // L18
                last_content: String::new(),
                // L19
                show_color_codes: false,
                mod_flag: false,
            },
        )
        .host()
        .init(|cx, me| {
            let me = me.id();
            // L21-24
            let notes_desc = cx.add(
                me,
                "notesDesc",
                LabelControl::new(
                    Anchor::new(TopLeft, me, TopLeft),
                    rect(8, 8, 150, 16),
                    NOTES_DESC,
                ),
            );
            let btn = |cx: &mut Cx,
                       key: &'static str,
                       to: CtrlId,
                       x: i32,
                       y: i32,
                       cc: CcField,
                       name: &str| {
                let label = format!("{}{}", cc(&cx.main.cc), name);
                cx.add(
                    me,
                    key,
                    ButtonControl::new(
                        Anchor::new(TopLeft, to, TopLeft),
                        rect(x, y, 100, 18),
                        label,
                        move |cx| {
                            let c = cc(&cx.main.cc).clone();
                            NotesTab::set_color(cx, me, &c);
                        },
                    ),
                )
                .id()
            };
            // L25
            let normal = btn(
                cx,
                "normal",
                notes_desc.id(),
                0,
                48,
                |c| &c.NORMAL,
                "NORMAL",
            );
            // L26
            let magic = btn(cx, "magic", normal, 120, 0, |c| &c.MAGIC, "MAGIC");
            // L27
            let rare = btn(cx, "rare", magic, 120, 0, |c| &c.RARE, "RARE");
            // L28
            btn(cx, "unique", rare, 120, 0, |c| &c.UNIQUE, "UNIQUE");
            // L29
            let fire = btn(cx, "fire", normal, 0, 18, |c| &c.FIRE, "FIRE");
            // L30
            let cold = btn(cx, "cold", fire, 120, 0, |c| &c.COLD, "COLD");
            // L31
            let lightning = btn(cx, "lightning", cold, 120, 0, |c| &c.LIGHTNING, "LIGHTNING");
            // L32
            btn(cx, "chaos", lightning, 120, 0, |c| &c.CHAOS, "CHAOS");
            // L33
            let strength = btn(cx, "strength", fire, 0, 18, |c| &c.STRENGTH, "STRENGTH");
            // L34
            let dexterity = btn(
                cx,
                "dexterity",
                strength,
                120,
                0,
                |c| &c.DEXTERITY,
                "DEXTERITY",
            );
            // L35
            let intelligence = btn(
                cx,
                "intelligence",
                dexterity,
                120,
                0,
                |c| &c.INTELLIGENCE,
                "INTELLIGENCE",
            );
            // L36
            cx.add(
                me,
                "default",
                ButtonControl::new(
                    Anchor::new(TopLeft, intelligence, TopLeft),
                    rect(120, 0, 100, 18),
                    "^7DEFAULT",
                    move |cx| NotesTab::set_color(cx, me, "^7"),
                ),
            );
            // L38 EditControl(anchor, rect, init, prompt, filter, limit, changeFunc, lineHeight,
            // allowZoom)
            let edit = cx.add(
                me,
                "edit",
                EditControl::new(
                    Anchor::new(TopLeft, fire, TopLeft),
                    rect(0, 48, 0, 0),
                    EditArgs {
                        init: Some(String::new()),
                        filter: Some("^%C\t\n".into()),
                        line_height: Some(16.0),
                        allow_zoom: true,
                        ..Default::default()
                    },
                ),
            );
            // L39-41 `self.width - 16` (the field, a number once Draw ran)
            cx.base_mut(edit).width = Prop::f(move |cx, _| cx.base(me).width.raw() - 16.0);
            // L42-44
            cx.base_mut(edit).height = Prop::f(move |cx, _| cx.base(me).height.raw() - 128.0);
            // L45-48
            cx.add(
                me,
                "toggleColorCodes",
                ButtonControl::new(
                    Anchor::new(TopRight, me, TopRight),
                    rect(-10, 70, 160, 20),
                    "Show Color Codes",
                    move |cx| {
                        // L46
                        let s = !cx.w::<NotesTab>(me).show_color_codes;
                        cx.w_mut::<NotesTab>(me).show_color_codes = s;
                        // L47
                        NotesTab::set_show_color_codes(cx, me, s);
                    },
                ),
            );
            // L49
            cx.select_control(me, Some(edit.id()));
        })
    }

    /// lua: NotesTab.lua:53-62
    /// @pob Classes/NotesTab.lua:53-62 h=811d6886
    pub fn set_show_color_codes(cx: &mut Cx, me: CtrlId, setting: bool) {
        // L54
        cx.w_mut::<NotesTab>(me).show_color_codes = setting;
        let toggle = cx.child_as::<ButtonControl>(me, "toggleColorCodes");
        let edit = cx.child_as::<EditControl>(me, "edit");
        // L55
        if setting {
            // L56
            cx.get_mut(toggle).label = "Hide Color Codes".into();
            // L57
            let buf = gsub(
                &gsub(&cx.get(edit).buf, "%^x(%x%x%x%x%x%x)", "^_x%1"),
                "%^(%d)",
                "^_%1",
            );
            cx.get_mut(edit).buf = buf;
        } else {
            // L59
            cx.get_mut(toggle).label = "Show Color Codes".into();
            // L60
            let buf = gsub(
                &gsub(&cx.get(edit).buf, "%^_x(%x%x%x%x%x%x)", "^x%1"),
                "%^_(%d)",
                "^%1",
            );
            cx.get_mut(edit).buf = buf;
        }
    }

    /// lua: NotesTab.lua:64-73
    /// @pob Classes/NotesTab.lua:64-73 h=820a6124
    pub fn set_color(cx: &mut Cx, me: CtrlId, color: &str) {
        let show = cx.w::<NotesTab>(me).show_color_codes;
        // L65
        let mut text = color.to_owned();
        // L66
        if show {
            text = gsub(&gsub(color, "%^x(%x%x%x%x%x%x)", "^_x%1"), "%^(%d)", "^_%1");
        }
        let edit = cx.child_as::<EditControl>(me, "edit");
        let (sel, caret) = {
            let e = cx.get(edit);
            (e.sel, e.caret)
        };
        // L67
        if sel.is_none() || sel == Some(caret) {
            // L68
            EditControl::insert(cx, edit.id(), &text);
        } else {
            // L70
            let last_color = lmatch(
                &EditControl::get_sel_text(cx, edit.id()),
                if show {
                    "^.*(%^_x%x%x%x%x%x%x)"
                } else {
                    "^.*(%^x%x%x%x%x%x%x)"
                },
            )
            .unwrap_or_else(|| "^7".to_owned());
            // L71 (`text .. (GetSelText():gsub(...)) .. lastColor`: the concatenation's middle
            // call keeps its first result)
            let stripped = gsub(
                &EditControl::get_sel_text(cx, edit.id()),
                if show {
                    "%^_x%x%x%x%x%x%x"
                } else {
                    "%^x%x%x%x%x%x%x"
                },
                "",
            );
            EditControl::replace_sel(cx, edit.id(), &format!("{text}{stripped}{last_color}"));
        }
    }

    /// lua: NotesTab.lua:75-82
    /// @pob Classes/NotesTab.lua:75-82 h=0c53ef93
    pub fn load(cx: &mut Cx, me: CtrlId, xml: &XmlNode, _file_name: Option<&str>) {
        let edit = cx.child_as::<EditControl>(me, "edit");
        // L76
        for node in &xml.children {
            // L77
            if let XmlChild::Text(t) = node {
                // L78
                EditControl::set_text(cx, edit.id(), t, false);
            }
        }
        // L81
        let buf = cx.get(edit).buf.clone();
        cx.w_mut::<NotesTab>(me).last_content = buf;
    }

    /// lua: NotesTab.lua:84-88
    /// @pob Classes/NotesTab.lua:84-88 h=06238154
    pub fn save(cx: &mut Cx, me: CtrlId, xml: &mut XmlNode) {
        // L85
        Self::set_show_color_codes(cx, me, false);
        // L86
        let buf = cx.get(cx.child_as::<EditControl>(me, "edit")).buf.clone();
        xml.children.push(XmlChild::Text(buf.as_str().into()));
        // L87
        cx.w_mut::<NotesTab>(me).last_content = buf;
    }

    /// lua: NotesTab.lua:90-112
    /// @pob Classes/NotesTab.lua:90-112 h=4a664a1c
    pub fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, ev: &mut InputEvents) {
        {
            let b = cx.base_mut(me);
            // L91-94
            b.x = vp.x.into();
            b.y = vp.y.into();
            b.width = vp.width.into();
            b.height = vp.height.into();
        }
        // L96
        let mut i = 0;
        while let Some(e) = ev.next_ipairs(&mut i) {
            // L97
            if let EventKind::KeyDown { .. } = e.kind {
                // L98
                if e.key == Key::Char(b'z') && cx.sys.is_key_down(Key::Ctrl) {
                    // L99
                    let edit = cx.child_as::<EditControl>(me, "edit");
                    EditControl::undo(cx, edit.id());
                // L100
                } else if e.key == Key::Char(b'y') && cx.sys.is_key_down(Key::Ctrl) {
                    // L101
                    let edit = cx.child_as::<EditControl>(me, "edit");
                    EditControl::redo(cx, edit.id());
                }
            }
        }
        // L105
        cx.process_controls_input(me, ev, vp);
        // L107
        draw_background(cx, vp);
        // L109
        cx.draw_controls(me, vp, None);
        // L111
        let edit = cx.child_as::<EditControl>(me, "edit");
        let mod_flag = cx.w::<NotesTab>(me).last_content != cx.get(edit).buf;
        cx.w_mut::<NotesTab>(me).mod_flag = mod_flag;
    }
}

// ---- the BuildMode seams (CCR-0098 §3, CCR-0104 §1) ------------------------------------------

/// `self.notesTab` of the open build, when the tab exists (a build whose Notes constructor ran).
fn tab(cx: &Cx) -> Option<CtrlId> {
    let id = cx.app().build.as_ref()?.tabs.notes.id();
    cx.ui.try_w::<NotesTab>(id).map(|_| id)
}

fn tab_or_err(cx: &Cx) -> CtrlId {
    tab(cx).unwrap_or_else(|| lua_error!("attempt to index field 'notesTab' (a nil value)"))
}

/// `BuildHooks::notes_tab`: `self.notesTab = new("NotesTab"):NotesTab(self)` (Build.lua:609).
pub fn ctor_hook(cx: &mut Cx, build: CtrlId) -> CtrlId {
    cx.add_free(Some(build), NotesTab::new(build)).id()
}

/// `BuildHooks::load_ui_section` for `Notes`: `self.notesTab:Load(node, self.dbFileName)`
/// (Build.lua:665; NotesTab:Load returns nothing, so the build is never closed).
pub fn load_ui_section(cx: &mut Cx, node: &XmlNode, db_file_name: Option<&str>) -> bool {
    let me = tab_or_err(cx);
    NotesTab::load(cx, me, node, db_file_name);
    false
}

/// `ShellHooks::save_ui_section` for `Notes`: `local node = { elem = "Notes" };
/// self.notesTab:Save(node)` (Build.lua:2289-2290). `None` when the tab does not exist.
pub fn save_ui_section(cx: &mut Cx) -> Option<XmlNode> {
    let me = tab(cx)?;
    let mut node = XmlNode {
        elem: "Notes".into(),
        ..Default::default()
    };
    NotesTab::save(cx, me, &mut node);
    Some(node)
}

/// `ShellHooks::draw_tab` for `BuildTab::Notes`: `self.notesTab:Draw(tabViewPort, inputEvents)`
/// (Build.lua:1303).
pub fn draw_tab(cx: &mut Cx, vp: &ViewPort, ev: &mut InputEvents) {
    let me = tab_or_err(cx);
    NotesTab::draw(cx, me, vp, ev);
}

/// `ShellHooks::tab_flag` for `TabFlag::Notes`: `self.notesTab.modFlag` (nil before the first
/// Draw reads as false).
pub fn mod_flag(cx: &Cx) -> bool {
    tab(cx).is_some_and(|me| cx.w::<NotesTab>(me).mod_flag)
}

/// `ShellHooks::set_tab_flag` for `TabFlag::Notes`: `self.notesTab.modFlag = v`
/// (ResetModFlags, Build.lua:1102).
pub fn set_mod_flag(cx: &mut Cx, v: bool) {
    if let Some(me) = tab(cx) {
        cx.w_mut::<NotesTab>(me).mod_flag = v;
    }
}
