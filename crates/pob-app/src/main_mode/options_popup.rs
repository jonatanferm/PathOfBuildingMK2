//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Main.lua:865-1316 (OpenOptionsPopup; instance DrawControls/GetMouseOverControl overrides :1297, :1307).
//! Owner: U17.
//!
//! @pob-file Modules/Main.lua @16de4b82d
//!
//! Notes:
//! - The Lua builds the `controls` table, then computes `popupHeight` from the final
//!   `currentY`, then opens the popup. `pob_ui_core::popups::open_popup` creates the dialog node
//!   before the children (U14's ORDER note), so the dialog is opened with `popupHeight` unknown
//!   and its `height` (PopupDialog.lua:14) and centred `y` (:18-20) are set from `popupHeight`
//!   right after the builder ran, before anything reads them. Nothing in between reads the
//!   dialog's size (the constructors draw nothing; the anchor rules of PopupDialog.lua:25-33 only
//!   rewrite anchors).
//! - `savedState` is a [`SavedState`] shared by the reopened popup (the DPI dropdown passes it
//!   on, L979).
//! - The `main` fields live in two places (ui doc §6.11): the display options generic controls
//!   read (`showThousandsSeparators`, `thousandsSeparator`, `decimalSeparator`,
//!   `showFlavourText`, `notSupportedModTooltips`, `disableScrollControlInteraction`) are
//!   `cx.main` (MainCore); the rest are `cx.app().main` (MainState).
//! - `main.invertSliderScrollDirection` is also written to SliderControl's stand-in
//!   `INVERT_SLIDER_SCROLL_DIRECTION` (CCR-0069 A), which SliderControl reads.
//! - CCR-0099 B: `self.modes.LIST:BuildList()` (L1219) has no counterpart on the frozen
//!   [`crate::main_mode::Mode`] trait (CCR-0096 A); it calls U18's
//!   [`crate::build_list::rebuild`] (CCR-0100 B; FOLLOWUPS "CCRDOC" 1).
//! - The changeFuncs of the colour edits call `updateColorCode` (Data/Global.lua:80-87); for
//!   HIGHLIGHT it also sets the `rgbColor` global (`MainState::rgb_color`), as U16's
//!   LoadSettings does.
//!
//! ORDER: the instance `DrawControls` (L1293-1304) and `GetMouseOverControl` (L1307-1313)
//! iterate `pairs(self.controls)`: canonical order (D7), as the UI oracle's patches 6 and 7 make
//! the Lua do (pob-ui-oracle/src/patches.rs, "Main.lua:1298", "Main.lua:1308"); the draw loop
//! carries the control-path markers.
//!
//! Props with side effects (ui doc §7.3): none (no property closure in this range assigns).

use std::cell::Cell;
use std::rc::Rc;

use pob_lua::math::{m_floor, m_max, m_min};
use pob_ui_core::color_codes::ColorCodes;
use pob_ui_core::controls::drop_down::DropItem;
use pob_ui_core::controls::scroll_bar::ScrollDir;
use pob_ui_core::controls::slider::INVERT_SLIDER_SCROLL_DIRECTION;
use pob_ui_core::controls::{
    ButtonControl, CheckBoxControl, DropDownControl, EditArgs, EditControl, LabelControl,
    PopupDialog, RectangleOutlineControl, ScrollBarControl, SliderControl,
};
use pob_ui_core::popups::{close_popup, open_popup, PopupOpts};
use pob_ui_core::prelude::*;

use crate::app::CxApp;
use crate::main_mode::settings::save_settings;
use crate::main_mode::update_popup::set_manifest_branch;

/// The `savedState` table (L870-897): the option values when the popup was first opened, which
/// Cancel restores.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedState {
    pub node_power_theme: String,
    pub color_positive: String,
    pub color_negative: String,
    pub color_highlight: String,
    pub show_thousands_separators: bool,
    pub use_compact_values: bool,
    pub thousands_separator: String,
    pub decimal_separator: String,
    pub show_titlebar_name: bool,
    pub beta_test: Option<bool>,
    pub edge_search_highlight: bool,
    pub default_gem_quality: f64,
    pub default_char_level: f64,
    pub default_item_affix_quality: f64,
    pub show_warnings: bool,
    pub slot_only_tooltips: bool,
    pub migrate_eldritch_implicits: bool,
    pub not_supported_mod_tooltips: bool,
    pub invert_slider_scroll_direction: Option<bool>,
    pub disable_dev_auto_save: Option<bool>,
    pub show_public_builds: bool,
    pub show_flavour_text: bool,
    pub show_animations: bool,
    pub show_all_item_affixes: bool,
    pub disable_scroll_control_interaction: bool,
    pub dpi_scale_override_percent: f64,
}

impl SavedState {
    /// L870-897 (`savedState or { ... }`).
    fn capture(cx: &Cx) -> SavedState {
        let m = &cx.app().main;
        let c = &cx.main;
        SavedState {
            // L871
            node_power_theme: m.node_power_theme.clone(),
            // L872
            color_positive: m.color_positive.clone(),
            // L873
            color_negative: m.color_negative.clone(),
            // L874
            color_highlight: m.color_highlight.clone(),
            // L875
            show_thousands_separators: c.show_thousands_separators,
            // L876
            use_compact_values: m.use_compact_values,
            // L877
            thousands_separator: c.thousands_separator.clone(),
            // L878
            decimal_separator: c.decimal_separator.clone(),
            // L879
            show_titlebar_name: m.show_titlebar_name,
            // L880
            beta_test: m.beta_test,
            // L881
            edge_search_highlight: m.edge_search_highlight,
            // L882
            default_gem_quality: m.default_gem_quality.unwrap_or(0.0),
            // L883
            default_char_level: m.default_char_level.unwrap_or(1.0),
            // L884
            default_item_affix_quality: m.default_item_affix_quality.unwrap_or(0.5),
            // L885
            show_warnings: m.show_warnings,
            // L886
            slot_only_tooltips: m.slot_only_tooltips,
            // L887
            migrate_eldritch_implicits: m.migrate_eldritch_implicits,
            // L888
            not_supported_mod_tooltips: c.not_supported_mod_tooltips,
            // L889
            invert_slider_scroll_direction: m.invert_slider_scroll_direction,
            // L890
            disable_dev_auto_save: m.disable_dev_auto_save,
            // L891
            show_public_builds: m.show_public_builds,
            // L892
            show_flavour_text: c.show_flavour_text,
            // L893
            show_animations: m.show_animations,
            // L894
            show_all_item_affixes: m.show_all_item_affixes,
            // L895
            disable_scroll_control_interaction: c.disable_scroll_control_interaction,
            // L896
            dpi_scale_override_percent: m.dpi_scale_override_percent,
        }
    }
}

/// `updateColorCode(code, color)` (Data/Global.lua:80-87), including the `rgbColor` global.
fn update_color_code(cx: &mut Cx, code: &str, color: &str) {
    if let Some(rgb) = cx.main.cc.update_color_code(code, color) {
        cx.app_mut().main.rgb_color = rgb;
    }
}

/// `main.invertSliderScrollDirection = v` (and SliderControl's stand-in, CCR-0069 A).
fn set_invert_slider_scroll_direction(cx: &mut Cx, v: Option<bool>) {
    cx.app_mut().main.invert_slider_scroll_direction = v;
    INVERT_SLIDER_SCROLL_DIRECTION.with(|c| c.set(v == Some(true))); // CCR-0069
}

/// `tostring(s:gsub('^(^)', '0'))`: the colour as `0x..` (one value: the string).
fn hex_display(s: &str) -> String {
    pob_lua::gsub(s, "^(^)", "0", None).0
}

/// `x .. ""` for the affix-quality label (`(q * 100) .. "%"`).
fn affix_quality_label(q: f64) -> String {
    format!("{}%", pob_lua::tostring_num(q * 100.0))
}

/// `controlIsInsideScrollClip(popupDialog, control)` (L1279-1290).
fn control_is_inside_scroll_clip(
    cx: &Cx,
    popup_dialog: CtrlId,
    control: CtrlId,
    statics: &[Option<CtrlId>; 3],
) -> bool {
    // L1280 control == controls.scrollBar or control == controls.save or control == controls.cancel
    if statics.contains(&Some(control)) {
        // L1281
        return true;
    }
    // L1283
    let (_, y) = cx.pos(popup_dialog);
    // L1284
    let (_, height) = cx.size(popup_dialog);
    // L1285 just below title
    let clip_y_top = y + 20.0;
    // L1286 just above buttons
    let clip_y_bottom = y + height - 40.0;
    // L1287
    let (_, cy) = cx.pos(control);
    // L1288
    let (_, ch) = cx.size(control);
    // L1289
    cy >= clip_y_top && (cy + ch) <= clip_y_bottom
}

/// The layout locals `currentY`/`currentX` and the local function `nextRow` (L904-905, L918-922).
struct Layout {
    current_y: f64,
    current_x: f64,
}

impl Layout {
    /// local func to make a new line with a heightModifier (L918-922)
    fn next_row(&mut self, height_modifier: Option<f64>) {
        // L919
        let px_per_line = 26.0;
        // L920
        let height_modifier = height_modifier.unwrap_or(1.0);
        // L921
        self.current_y += height_modifier * px_per_line;
    }
}

/// What the builder hands back for the popup's height and the instance overrides.
#[derive(Default)]
struct Built {
    popup_height: f64,
    scroll_bar: Option<CtrlId>,
    save: Option<CtrlId>,
    cancel: Option<CtrlId>,
}

/// `main:OpenOptionsPopup()` as the bottom-bar button calls it (Main.lua:229).
pub fn open_options_popup_hook(cx: &mut Cx) {
    open_options_popup(cx, None);
}

/// lua: Main.lua:865-1316 main:OpenOptionsPopup(savedState)
/// @pob Modules/Main.lua:865-1315 h=14ebd674
pub fn open_options_popup(cx: &mut Cx, saved_state: Option<Rc<SavedState>>) {
    // L866 local controls = { }
    // Check for `savedState` or assign initial values
    // NOTE: update both this and the `controls.cancel` section below, when adding new options
    // L870
    let saved_state = saved_state.unwrap_or_else(|| Rc::new(SavedState::capture(cx)));

    // NOTE: Height needs to be adjusted if more menu options are added
    // L900 Min height required to not split menu into two columns
    let one_column_height_req = 850.0;
    // L901
    let column_width = 600.0;

    // L903
    let starting_y = 20.0;
    // L904-905 (currentX: initialized at `0`, only used for two-column layouts)
    let mut lay = Layout {
        current_y: starting_y,
        current_x: 0.0,
    };

    // Determine layout limits and modes
    let (screen_w, screen_h) = (cx.main.screen_w, cx.main.screen_h);
    // L908
    let use_two_columns = screen_h < one_column_height_req && screen_w >= column_width * 2.0;
    // L909
    let use_scroll_bar = screen_h < one_column_height_req && !use_two_columns;
    // L910
    let scroll_bar_width = if use_scroll_bar { 18.0 } else { 0.0 };

    // L912
    let popup_width = if use_two_columns {
        column_width * 2.0
    } else {
        column_width
    };

    // L1269-1273 local function scrollBarFunc() (passed to OpenPopup only with the scroll bar;
    // it reads `controls.sectionAnchor` and `controls.scrollBar`, which the builder creates)
    let scroll_ids: Rc<Cell<Option<(CtrlId, CtrlId)>>> = Rc::default();
    let scroll_bar_func: Option<Rc<dyn Fn(&mut Cx)>> = if use_scroll_bar {
        let scroll_ids = scroll_ids.clone();
        Some(Rc::new(move |cx: &mut Cx| {
            // L1270
            if use_scroll_bar {
                if let Some((section_anchor, scroll_bar)) = scroll_ids.get() {
                    // L1271
                    let offset = cx.w::<ScrollBarControl>(scroll_bar).offset;
                    cx.base_mut(section_anchor).y = Prop::Val(-offset);
                }
            }
        }))
    } else {
        None
    };

    let mut built = Built::default();
    let dev_mode = cx.app().launch.dev_mode;
    // L1275 local popup = self:OpenPopup(popupWidth, popupHeight, "Options", controls, "save", nil, "cancel", useScrollBar and scrollBarFunc or nil)
    let popup = open_popup(
        cx,
        popup_width,
        0.0, // popupHeight: set below (module notes)
        "Options",
        |cx, p| {
            build_controls(
                cx,
                p,
                &mut lay,
                &mut built,
                &saved_state,
                Geometry {
                    column_width,
                    scroll_bar_width,
                    use_two_columns,
                    use_scroll_bar,
                    starting_y,
                    screen_h,
                    dev_mode,
                },
            );
            if let (Some(sa), Some(sb)) = (cx.child(p, "sectionAnchor"), built.scroll_bar) {
                scroll_ids.set(Some((sa, sb)));
            }
        },
        PopupOpts {
            enter: Some("save".into()),
            default: None,
            escape: Some("cancel".into()),
            scroll_bar_func,
            resize_func: None,
        },
    );
    // PopupDialog.lua:14 `self:Control(nil, {0, 0, width, height})` and :18-20 with `popupHeight`
    let popup_height = built.popup_height;
    {
        let b = cx.base_mut(popup);
        b.height = Prop::Val(popup_height);
        b.rect_start[3] = Prop::Val(popup_height);
        // PopupDialog.lua:18-20
        b.y = Prop::f(move |cx, _| m_floor((cx.main.screen_h - popup_height) / 2.0));
    }

    // L1277 local originalDrawControls = popup.DrawControls
    let statics = [built.scroll_bar, built.save, built.cancel];

    // Modify draw controls to "clip" elements in case the scrollbar is used
    // L1293
    cx.set_ov(popup, move |ov| {
        ov.draw_controls = Some(Rc::new(move |cx, me, vp, _sel_control| {
            // L1294
            if !use_scroll_bar {
                // L1295 return originalDrawControls(self, viewPort)
                return cx.draw_controls_default(me, vp, None);
            }

            // L1298
            for id in cx.children(me) {
                if !cx.is_child_of(me, id) {
                    continue; // removed earlier in this iteration (`next` skips nil values)
                }
                // always draw static UI elements
                // L1300
                if cx.is_shown(id)
                    && cx.has(id, Caps::DRAW)
                    && control_is_inside_scroll_clip(cx, me, id, &statics)
                {
                    // L1301
                    let no_tooltip = cx
                        .host(me)
                        .sel_control
                        .is_some_and(|s| cx.base(s).has_focus == Some(true) && s != id);
                    let traced = cx.trace_push(me, id); // control-path marker (oracle patch 7)
                    cx.draw(id, vp, no_tooltip);
                    if traced {
                        cx.trace_pop();
                    }
                }
            }
        }));
    });

    // L1306
    if use_scroll_bar {
        // L1307
        cx.set_ov(popup, move |ov| {
            ov.get_mouse_over_control = Some(Rc::new(move |cx, me| {
                // L1308
                for id in cx.children(me) {
                    if !cx.is_child_of(me, id) {
                        continue;
                    }
                    // L1309
                    if cx.has(id, Caps::IS_MOUSE_OVER)
                        && control_is_inside_scroll_clip(cx, me, id, &statics)
                        && cx.is_mouse_over(id)
                    {
                        // L1310
                        return Some(id);
                    }
                }
                None
            }));
        });
    }
}

/// The layout values the builder reads.
#[derive(Clone, Copy)]
struct Geometry {
    column_width: f64,
    scroll_bar_width: f64,
    use_two_columns: bool,
    use_scroll_bar: bool,
    starting_y: f64,
    screen_h: f64,
    dev_mode: bool,
}

/// `{ label = .., <key> = v }` dropdown entries.
fn items<T: Clone + 'static>(list: &[(&str, T)]) -> Vec<DropItem> {
    list.iter()
        .map(|(l, v)| DropItem::new(*l, Rc::new(v.clone())))
        .collect()
}

/// `listVal[key]` of one of [`items`]' entries.
#[track_caller]
fn item_val<T: Clone + 'static>(v: Option<&DropItem>) -> T {
    match v.and_then(|v| v.get::<T>()) {
        Some(x) => x.clone(),
        None => lua_error!("attempt to index local 'value' (a nil value)"),
    }
}

fn tooltip(s: impl Into<String>) -> Option<Prop<Option<String>>> {
    Some(Prop::Val(Some(s.into())))
}

/// L866-1266: the `controls` table, in the Lua's order. Returns through `built`.
#[allow(clippy::too_many_lines)] // one Lua function
fn build_controls(
    cx: &mut Cx,
    p: Ctrl<PopupDialog>,
    lay: &mut Layout,
    built: &mut Built,
    saved_state: &Rc<SavedState>,
    g: Geometry,
) {
    let Geometry {
        column_width,
        scroll_bar_width,
        use_two_columns,
        use_scroll_bar,
        starting_y,
        screen_h,
        dev_mode,
    } = g;
    let popup_width = if use_two_columns {
        column_width * 2.0
    } else {
        column_width
    };

    // Scrollbar anchor
    // L915
    let section_anchor = cx
        .add(
            p,
            "sectionAnchor",
            PlainControl::new(Anchor::nil(TopLeft, TopLeft), rect(0, 0, popup_width, 0)),
        )
        .id();
    let sa = move || Anchor::new(TopLeft, section_anchor, TopLeft);

    // local func to make a new section header (L925-931)
    let draw_section_header = |cx: &mut Cx, lay: &mut Layout, id: &str, title: &str| {
        // L926
        let header_bg_color = [0.6, 0.6, 0.6];
        // L927
        cx.add(
            p,
            format!("section-{id}-bg"),
            RectangleOutlineControl::new(
                sa(),
                rect(
                    lay.current_x + scroll_bar_width + 8.0,
                    lay.current_y,
                    column_width - (scroll_bar_width * 2.0) - 17.0,
                    26,
                ),
                Some(header_bg_color),
                Some(1.0),
            ),
        );
        // L928
        lay.next_row(Some(0.2));
        // L929
        cx.add(
            p,
            format!("section-{id}-label"),
            LabelControl::new(
                sa(),
                rect(
                    lay.current_x + column_width / 2.0 - 60.0,
                    lay.current_y,
                    0,
                    16,
                ),
                format!("^7{title}"),
            ),
        );
        // L930
        lay.next_row(Some(1.5));
    };

    // L933
    let default_label_spacing_px = -4.0;
    // L934
    let default_label_placement_x = column_width * 0.45;
    let label_right_of = |cx: &mut Cx, key: &'static str, ctrl: CtrlId, width: f64, text: &str| {
        cx.add(
            p,
            key,
            LabelControl::new(
                Anchor::new(Right, ctrl, Left),
                rect(default_label_spacing_px, 0, width, 16),
                text,
            ),
        );
    };
    let check_box = |cx: &mut Cx,
                     lay: &Layout,
                     key: &'static str,
                     label: &str,
                     f: fn(&mut Cx, bool),
                     tooltip_text: Option<&str>|
     -> Ctrl<CheckBoxControl> {
        cx.add(
            p,
            key,
            CheckBoxControl::new(
                sa(),
                rect(
                    lay.current_x + default_label_placement_x,
                    lay.current_y,
                    20,
                    0,
                ),
                label,
                Some(Rc::new(move |cx: &mut Cx, state: bool| f(cx, state))),
                tooltip_text.map(|t| Prop::Val(Some(t.to_owned()))),
                false,
            ),
        )
    };

    // L936
    draw_section_header(cx, lay, "app", "Application options");

    // L938
    let connection_protocol = cx.add(
        p,
        "connectionProtocol",
        DropDownControl::new(
            sa(),
            rect(
                lay.current_x + default_label_placement_x,
                lay.current_y,
                100,
                18,
            ),
            items(&[("Auto", 0.0), ("IPv4", 1.0), ("IPv6", 2.0)]),
            Some(Rc::new(
                |cx: &mut Cx, _index: usize, value: Option<&DropItem>| {
                    // L943
                    cx.app_mut().main.connection_protocol = item_val::<f64>(value);
                },
            )),
            None,
            false,
        ),
    );
    // L945
    label_right_of(
        cx,
        "connectionProtocolLabel",
        connection_protocol.id(),
        0.0,
        "^7Connection Protocol:",
    );
    // L946
    cx.get_mut(connection_protocol).tt.tooltip_text =
        tooltip("Changes which protocol is used when downloading updates and importing builds.");
    // L947
    let lcp = cx.app().launch.connection_protocol;
    DropDownControl::sel_by_value(cx, connection_protocol.id(), &|v| {
        lcp.is_some() && v.get::<f64>().copied() == lcp
    });

    // L949
    lay.next_row(None);
    // L950
    let proxy_type = cx.add(
        p,
        "proxyType",
        DropDownControl::new(
            sa(),
            rect(
                lay.current_x + default_label_placement_x,
                lay.current_y,
                80,
                18,
            ),
            items(&[
                ("HTTP", "http".to_owned()),
                ("SOCKS", "socks5".to_owned()),
                ("SOCKS5H", "socks5h".to_owned()),
            ]),
            None,
            None,
            false,
        ),
    );
    // L955
    label_right_of(cx, "proxyLabel", proxy_type.id(), 0.0, "^7Proxy server:");
    // L956
    let proxy_url = cx.add(
        p,
        "proxyURL",
        EditControl::new(
            Anchor::new(Left, proxy_type, Right),
            rect(4, 0, 206, 18),
            EditArgs::default(),
        ),
    );

    // L958
    if let Some(launch_proxy_url) = cx.app().launch.proxy_url.clone() {
        // L959
        let (scheme, url) = match pob_lua::lmatch(&launch_proxy_url, "(%w+)://(.+)", None) {
            Some(c) => (
                Some(c[0].to_str().into_owned()),
                Some(c[1].to_str().into_owned()),
            ),
            None => (None, None),
        };
        // L960 (a nil value matches no entry)
        DropDownControl::sel_by_value(cx, proxy_type.id(), &|v| {
            scheme.is_some() && v.get::<String>() == scheme.as_ref()
        });
        // L961 SetText(url) (tostring(nil) is "nil")
        EditControl::set_text(cx, proxy_url.id(), url.as_deref().unwrap_or("nil"), false);
    }

    // L964
    lay.next_row(None);
    let saved_for_dpi = saved_state.clone();
    // L965
    let dpi_scale_override = cx.add(
        p,
        "dpiScaleOverride",
        DropDownControl::new(
            sa(),
            rect(
                lay.current_x + default_label_placement_x,
                lay.current_y,
                150,
                18,
            ),
            items(&[
                ("Use system default", 0.0),
                ("100%", 100.0),
                ("125%", 125.0),
                ("150%", 150.0),
                ("175%", 175.0),
                ("200%", 200.0),
                ("225%", 225.0),
                ("250%", 250.0),
            ]),
            Some(Rc::new(
                move |cx: &mut Cx, _index: usize, value: Option<&DropItem>| {
                    let percent = item_val::<f64>(value);
                    // L975
                    cx.app_mut().main.dpi_scale_override_percent = percent;
                    // L976
                    cx.sg.set_dpi_scale_override_percent(percent as i32);
                    // L977 refresh screen size immediately
                    let (w, h) = cx.sg.get_virtual_screen_size();
                    cx.main.screen_w = w;
                    cx.main.screen_h = h;
                    // L978
                    close_popup(cx);
                    // L979
                    open_options_popup(cx, Some(saved_for_dpi.clone()));
                },
            )),
            None,
            false,
        ),
    );
    // L981
    label_right_of(
        cx,
        "dpiScaleOverrideLabel",
        dpi_scale_override.id(),
        0.0,
        "^7UI scaling override:",
    );
    // L982
    cx.get_mut(dpi_scale_override).tt.tooltip_text = tooltip(
        "Overrides Windows DPI scaling inside Path of Building.\nChoose a percentage between 100% and 250% or revert to the system default.",
    );
    // L983
    let dpi = cx.app().main.dpi_scale_override_percent;
    DropDownControl::sel_by_value(cx, dpi_scale_override.id(), &|v| {
        v.get::<f64>() == Some(&dpi)
    });

    // L985
    lay.next_row(None);
    // L986
    let build_path = cx.add(
        p,
        "buildPath",
        EditControl::new(
            sa(),
            rect(
                lay.current_x + default_label_placement_x,
                lay.current_y,
                290,
                18,
            ),
            EditArgs::default(),
        ),
    );
    // L987
    label_right_of(
        cx,
        "buildPathLabel",
        build_path.id(),
        0.0,
        "^7Build save path:",
    );
    let (bp, dbp) = {
        let m = &cx.app().main;
        (m.build_path.clone(), m.default_build_path.clone())
    };
    // L988
    if bp != dbp {
        // L989 (tostring(nil) is "nil")
        EditControl::set_text(cx, build_path.id(), bp.as_deref().unwrap_or("nil"), false);
    }
    // L991
    let dbp = dbp.unwrap_or_else(|| {
        lua_error!("attempt to concatenate field 'defaultBuildPath' (a nil value)")
    });
    cx.get_mut(build_path).tt.tooltip_text = tooltip(format!(
        "Overrides the default save location for builds.\nThe default location is: '{dbp}'"
    ));

    // L993
    lay.next_row(None);
    // L994
    let node_power_theme = cx.add(
        p,
        "nodePowerTheme",
        DropDownControl::new(
            sa(),
            rect(
                lay.current_x + default_label_placement_x,
                lay.current_y,
                100,
                18,
            ),
            items(&[
                ("Red & Blue", "RED/BLUE".to_owned()),
                ("Red & Green", "RED/GREEN".to_owned()),
                ("Green & Blue", "GREEN/BLUE".to_owned()),
            ]),
            Some(Rc::new(
                |cx: &mut Cx, _index: usize, value: Option<&DropItem>| {
                    // L999
                    cx.app_mut().main.node_power_theme = item_val::<String>(value);
                },
            )),
            None,
            false,
        ),
    );
    // L1001
    label_right_of(
        cx,
        "nodePowerThemeLabel",
        node_power_theme.id(),
        0.0,
        "^7Node Power colours:",
    );
    // L1002
    cx.get_mut(node_power_theme).tt.tooltip_text =
        tooltip("Changes the colour scheme used for the node power display on the passive tree.");
    // L1003
    let npt = cx.app().main.node_power_theme.clone();
    DropDownControl::sel_by_value(cx, node_power_theme.id(), &|v| {
        v.get::<String>() == Some(&npt)
    });

    let defaults = ColorCodes::default(); // defaultColorCodes
                                          // The three colour edits (L1006-1015, L1018-1027, L1031-1040).
    let colour_edit = |cx: &mut Cx,
                       lay: &Layout,
                       key: &'static str,
                       value: &str,
                       code: &'static str,
                       set: fn(&mut Cx, String)|
     -> Ctrl<EditControl> {
        cx.add(
            p,
            key,
            EditControl::new(
                sa(),
                rect(
                    lay.current_x + default_label_placement_x,
                    lay.current_y,
                    100,
                    18,
                ),
                EditArgs {
                    init: Some(hex_display(value)),
                    limit: Some(8),
                    change_func: Some(Rc::new(move |cx: &mut Cx, buf: &str| {
                        // L1007
                        let m = pob_ui_core::luastr::lmatch(buf, "0x%x+");
                        // L1008
                        if m.is_some_and(|m| m.len() == 8) {
                            // L1009
                            update_color_code(cx, code, buf);
                            // L1010
                            set(cx, buf.to_owned());
                        }
                    })),
                    ..EditArgs::default()
                },
            ),
        )
    };

    // L1005
    lay.next_row(None);
    // L1006
    let cp = cx.app().main.color_positive.clone();
    let color_positive = colour_edit(cx, lay, "colorPositive", &cp, "POSITIVE", |cx, b| {
        cx.app_mut().main.color_positive = b;
    });
    // L1013
    label_right_of(
        cx,
        "colorPositiveLabel",
        color_positive.id(),
        0.0,
        "^7Hex colour for positive values:",
    );
    // L1014-1015
    cx.get_mut(color_positive).tt.tooltip_text = tooltip(format!(
        "Overrides the default hex colour for positive values in breakdowns. \nExpected format is 0x000000. The default value is {}.\nIf updating while inside a build, please re-load the build after saving.",
        hex_display(&defaults.POSITIVE)
    ));

    // L1017
    lay.next_row(None);
    // L1018
    let cn = cx.app().main.color_negative.clone();
    let color_negative = colour_edit(cx, lay, "colorNegative", &cn, "NEGATIVE", |cx, b| {
        cx.app_mut().main.color_negative = b;
    });
    // L1025
    label_right_of(
        cx,
        "colorNegativeLabel",
        color_negative.id(),
        0.0,
        "^7Hex colour for negative values:",
    );
    // L1026-1027
    cx.get_mut(color_negative).tt.tooltip_text = tooltip(format!(
        "Overrides the default hex colour for negative values in breakdowns. \nExpected format is 0x000000. The default value is {}.\nIf updating while inside a build, please re-load the build after saving.",
        hex_display(&defaults.NEGATIVE)
    ));

    // L1029
    lay.next_row(None);

    // L1031
    let ch = cx.app().main.color_highlight.clone();
    let color_highlight = colour_edit(cx, lay, "colorHighlight", &ch, "HIGHLIGHT", |cx, b| {
        cx.app_mut().main.color_highlight = b;
    });
    // L1038
    label_right_of(
        cx,
        "colorHighlightLabel",
        color_highlight.id(),
        0.0,
        "^7Hex colour for highlight nodes:",
    );
    // L1039-1040
    cx.get_mut(color_highlight).tt.tooltip_text = tooltip(format!(
        "Overrides the default hex colour for highlighting nodes in passive tree search. \nExpected format is 0x000000. The default value is {}\nIf updating while inside a build, please re-load the build after saving.",
        hex_display(&defaults.HIGHLIGHT)
    ));

    // L1042
    lay.next_row(None);
    // L1043
    let beta_test = check_box(
        cx,
        lay,
        "betaTest",
        "^7Opt-in to weekly beta test builds:",
        |cx, state| {
            // L1044
            cx.app_mut().main.beta_test = Some(state);
        },
        None,
    );

    // L1047
    lay.next_row(None);
    // L1048
    let edge_search_highlight = check_box(
        cx,
        lay,
        "edgeSearchHighlight",
        "^7Show search circles at viewport edge",
        |cx, state| {
            // L1049
            cx.app_mut().main.edge_search_highlight = state;
        },
        None,
    );

    // L1052
    lay.next_row(None);
    // L1053
    let show_public_builds = check_box(
        cx,
        lay,
        "showPublicBuilds",
        "^7Show Latest/Trending builds:",
        |cx, state| {
            // L1054
            cx.app_mut().main.show_public_builds = state;
        },
        None,
    );

    // L1057
    lay.next_row(None);
    // L1058
    let show_flavour_text = check_box(
        cx,
        lay,
        "showFlavourText",
        "^7Styled Tooltips with Flavour Text:",
        |cx, state| {
            // L1059
            cx.main.show_flavour_text = state;
        },
        None,
    );
    // L1061
    cx.get_mut(show_flavour_text).tt.tooltip_text =
        tooltip("If updating while inside a build, please re-load the build after saving.");

    // L1063
    lay.next_row(None);
    // L1064
    let show_animations = check_box(
        cx,
        lay,
        "showAnimations",
        "^7Show Animations:",
        |cx, state| {
            // L1065
            cx.app_mut().main.show_animations = state;
        },
        None,
    );

    // L1068
    lay.next_row(None);
    // L1069
    let show_all_item_affixes = check_box(
        cx,
        lay,
        "showAllItemAffixes",
        "^7Show all item affixes sliders:",
        |cx, state| {
            // L1070
            cx.app_mut().main.show_all_item_affixes = state;
        },
        None,
    );
    // L1072
    cx.get_mut(show_all_item_affixes).tt.tooltip_text = tooltip(
        "Display all item affix slots as a stacked list instead of hiding them in dropdowns.",
    );

    // L1074
    lay.next_row(None);
    // L1075
    let disable_scroll_control_interaction = check_box(
        cx,
        lay,
        "disableScrollControlInteraction",
        "^7Disable control scroll interaction:",
        |cx, state| {
            // L1076
            cx.main.disable_scroll_control_interaction = state;
        },
        None,
    );
    // L1078
    cx.get_mut(disable_scroll_control_interaction).tt.tooltip_text = tooltip(
        "Disable changing the values in controls such as dropdowns or numeric inputs when using the scroll wheel.",
    );

    // L1080
    lay.next_row(None);

    // L1082 store left column height
    let left_column_max_y = lay.current_y;

    // Check for two column layout
    // L1085
    if use_two_columns {
        // L1086 reset height back to top
        lay.current_y = starting_y;
        // L1087
        lay.current_x = column_width;
    }

    // Build-related Option Section starts
    // L1091
    draw_section_header(cx, lay, "build", "Build-related options");

    // L1093
    let show_thousands_separators = check_box(
        cx,
        lay,
        "showThousandsSeparators",
        "^7Show thousands separators:",
        |cx, state| {
            // L1094
            cx.main.show_thousands_separators = state;
        },
        None,
    );
    // L1096
    let v = cx.main.show_thousands_separators;
    cx.get_mut(show_thousands_separators).state = v;

    // L1098
    lay.next_row(None);
    // L1099
    let use_compact_values = check_box(
        cx,
        lay,
        "useCompactValues",
        "^7Compact large numbers (e.g. 12.3K):",
        |cx, state| {
            // L1100
            cx.app_mut().main.use_compact_values = state;
        },
        None,
    );
    // L1102
    let v = cx.app().main.use_compact_values;
    cx.get_mut(use_compact_values).state = v;

    // L1104
    lay.next_row(None);
    // L1105
    let ts = cx.main.thousands_separator.clone();
    let thousands_separator = cx.add(
        p,
        "thousandsSeparator",
        EditControl::new(
            sa(),
            rect(
                lay.current_x + default_label_placement_x,
                lay.current_y,
                30,
                20,
            ),
            EditArgs {
                init: Some(ts),
                filter: Some("%w".to_owned()),
                limit: Some(1),
                change_func: Some(Rc::new(|cx: &mut Cx, buf: &str| {
                    // L1106
                    cx.main.thousands_separator = buf.to_owned();
                })),
                ..EditArgs::default()
            },
        ),
    );
    // L1108
    label_right_of(
        cx,
        "thousandsSeparatorLabel",
        thousands_separator.id(),
        92.0,
        "^7Thousands separator:",
    );

    // L1110
    lay.next_row(None);
    // L1111
    let ds = cx.main.decimal_separator.clone();
    let decimal_separator = cx.add(
        p,
        "decimalSeparator",
        EditControl::new(
            sa(),
            rect(
                lay.current_x + default_label_placement_x,
                lay.current_y,
                30,
                20,
            ),
            EditArgs {
                init: Some(ds),
                filter: Some("%w".to_owned()),
                limit: Some(1),
                change_func: Some(Rc::new(|cx: &mut Cx, buf: &str| {
                    // L1112
                    cx.main.decimal_separator = buf.to_owned();
                })),
                ..EditArgs::default()
            },
        ),
    );
    // L1114
    label_right_of(
        cx,
        "decimalSeparatorLabel",
        decimal_separator.id(),
        92.0,
        "^7Decimal separator:",
    );

    // L1116
    lay.next_row(None);
    // L1117
    let titlebar_name = check_box(
        cx,
        lay,
        "titlebarName",
        "^7Show build name in window title:",
        |cx, state| {
            // L1118
            cx.app_mut().main.show_titlebar_name = state;
        },
        None,
    );

    // L1121
    lay.next_row(None);
    // L1122 (init `self.defaultGemQuality`: nil gives "")
    let dgq = cx.app().main.default_gem_quality.map(pob_lua::tostring_num);
    let default_gem_quality = cx.add(
        p,
        "defaultGemQuality",
        EditControl::new(
            sa(),
            rect(
                lay.current_x + default_label_placement_x,
                lay.current_y,
                80,
                20,
            ),
            EditArgs {
                init: dgq,
                filter: Some("%D".to_owned()),
                limit: Some(2),
                change_func: Some(Rc::new(|cx: &mut Cx, gem_quality: &str| {
                    // L1123
                    let v = m_min(pob_lua::tonumber(gem_quality).unwrap_or(0.0), 23.0);
                    cx.app_mut().main.default_gem_quality = Some(v);
                })),
                ..EditArgs::default()
            },
        ),
    );
    // L1125
    cx.get_mut(default_gem_quality).tt.tooltip_text = tooltip(
        "Set the default quality that can be overwritten by build-related quality settings in the skill panel.",
    );
    // L1126
    label_right_of(
        cx,
        "defaultGemQualityLabel",
        default_gem_quality.id(),
        0.0,
        "^7Default gem quality:",
    );

    // L1128
    lay.next_row(None);
    // L1129
    let dcl = cx.app().main.default_char_level.map(pob_lua::tostring_num);
    let default_char_level = cx.add(
        p,
        "defaultCharLevel",
        EditControl::new(
            sa(),
            rect(
                lay.current_x + default_label_placement_x,
                lay.current_y,
                80,
                20,
            ),
            EditArgs {
                init: dcl,
                filter: Some("%D".to_owned()),
                limit: Some(3),
                change_func: Some(Rc::new(|cx: &mut Cx, char_level: &str| {
                    // L1130
                    let v = m_min(
                        m_max(pob_lua::tonumber(char_level).unwrap_or(1.0), 1.0),
                        100.0,
                    );
                    cx.app_mut().main.default_char_level = Some(v);
                })),
                ..EditArgs::default()
            },
        ),
    );
    // L1132
    cx.get_mut(default_char_level).tt.tooltip_text = tooltip(
        "Set the default level of your builds. If this is higher than 1, manual level mode will be enabled by default in new builds.",
    );
    // L1133
    label_right_of(
        cx,
        "defaultCharLevelLabel",
        default_char_level.id(),
        0.0,
        "^7Default character level:",
    );

    // L1135
    lay.next_row(None);
    // L1136
    let default_item_affix_quality_slider = cx.add(
        p,
        "defaultItemAffixQualitySlider",
        SliderControl::new(
            sa(),
            rect(
                lay.current_x + default_label_placement_x,
                lay.current_y,
                200,
                20,
            ),
            Some(Rc::new(move |cx: &mut Cx, value: f64| {
                // L1137
                let q = pob_lua::round(value, Some(2));
                cx.app_mut().main.default_item_affix_quality = Some(q);
                // L1138 (`controls.defaultItemAffixQualityValue` is created below)
                let label = cx.child_as::<LabelControl>(p, "defaultItemAffixQualityValue");
                cx.get_mut(label).label = Prop::Val(affix_quality_label(q));
            })),
            None,
        ),
    );
    // L1140
    label_right_of(
        cx,
        "defaultItemAffixQualityLabel",
        default_item_affix_quality_slider.id(),
        92.0,
        "^7Default item affix quality:",
    );
    // L1141
    let default_item_affix_quality_value = cx.add(
        p,
        "defaultItemAffixQualityValue",
        LabelControl::new(
            Anchor::new(Left, default_item_affix_quality_slider, Right),
            rect(-default_label_spacing_px, 0, 92, 16),
            "50%",
        ),
    );
    // L1142 (`self.defaultItemAffixQuality * 100` on the next line errors on nil)
    let q = cx.app().main.default_item_affix_quality.unwrap_or_else(|| {
        lua_error!("attempt to perform arithmetic on field 'defaultItemAffixQuality' (a nil value)")
    });
    cx.get_mut(default_item_affix_quality_slider).val = q;
    // L1143
    cx.get_mut(default_item_affix_quality_value).label = Prop::Val(affix_quality_label(q));

    // L1145
    lay.next_row(None);
    // L1146
    let show_warnings = check_box(
        cx,
        lay,
        "showWarnings",
        "^7Show build warnings:",
        |cx, state| {
            // L1147
            cx.app_mut().main.show_warnings = state;
        },
        None,
    );
    // L1149
    let v = cx.app().main.show_warnings;
    cx.get_mut(show_warnings).state = v;

    // L1151
    lay.next_row(None);
    // L1152
    let slot_only_tooltips = check_box(cx, lay, "slotOnlyTooltips", "^7Show tooltips only for affected slots:", |cx, state| {
        // L1153
        cx.app_mut().main.slot_only_tooltips = state;
    }, Some("Shows comparisons in tooltips only for the slot you are currently placing the item in, instead of all slots."));
    // L1155
    let v = cx.app().main.slot_only_tooltips;
    cx.get_mut(slot_only_tooltips).state = v;

    // L1157
    lay.next_row(None);
    // L1158
    let migrate_eldritch_implicits = check_box(
        cx,
        lay,
        "migrateEldritchImplicits",
        "^7Copy Eldritch Implicits onto Display Item:",
        |cx, state| {
            // L1159
            cx.app_mut().main.migrate_eldritch_implicits = state;
        },
        None,
    );
    // L1161
    cx.get_mut(migrate_eldritch_implicits).tt.tooltip_text = tooltip(
        "Apply Eldritch Implicits from current gear when comparing new gear, given the new item doesn't have any influence",
    );
    // L1162
    let v = cx.app().main.migrate_eldritch_implicits;
    cx.get_mut(migrate_eldritch_implicits).state = v;

    // L1164
    lay.next_row(None);
    // L1165
    let not_supported_mod_tooltips = check_box(
        cx,
        lay,
        "notSupportedModTooltips",
        "^7Show tooltip for unsupported mods :",
        |cx, state| {
            // L1166
            cx.main.not_supported_mod_tooltips = state;
        },
        None,
    );
    // L1168
    cx.get_mut(not_supported_mod_tooltips).tt.tooltip_text =
        tooltip("Show ^8(Not supported in PoB yet) ^7next to unsupported mods");
    // L1169
    let v = cx.main.not_supported_mod_tooltips;
    cx.get_mut(not_supported_mod_tooltips).state = v;

    // L1171
    lay.next_row(None);
    // L1172
    let invert_slider_scroll_direction = check_box(
        cx,
        lay,
        "invertSliderScrollDirection",
        "^7Invert slider scroll direction:",
        |cx, state| {
            // L1173
            set_invert_slider_scroll_direction(cx, Some(state));
        },
        None,
    );
    // L1175
    cx.get_mut(invert_slider_scroll_direction).tt.tooltip_text =
        tooltip("Default scroll direction is:\nScroll Up = Move right\nScroll Down = Move left");
    // L1176 (nil is unchecked)
    let v = cx.app().main.invert_slider_scroll_direction == Some(true);
    cx.get_mut(invert_slider_scroll_direction).state = v;

    // L1178
    if dev_mode {
        // L1179
        lay.next_row(None);
        // L1180
        let disable_dev_auto_save = check_box(
            cx,
            lay,
            "disableDevAutoSave",
            "^7Disable Dev AutoSave:",
            |cx, state| {
                // L1181
                cx.app_mut().main.disable_dev_auto_save = Some(state);
            },
            None,
        );
        // L1183
        cx.get_mut(disable_dev_auto_save).tt.tooltip_text =
            tooltip("Do not Autosave builds while on Dev branch");
        // L1184
        let v = cx.app().main.disable_dev_auto_save == Some(true);
        cx.get_mut(disable_dev_auto_save).state = v;
    }

    // L1187-1194
    let (bt, esh, stn, spb, aff) = {
        let m = &cx.app().main;
        (
            m.beta_test == Some(true),
            m.edge_search_highlight,
            m.show_titlebar_name,
            m.show_public_builds,
            (m.show_animations, m.show_all_item_affixes),
        )
    };
    let (sft, dsci) = (
        cx.main.show_flavour_text,
        cx.main.disable_scroll_control_interaction,
    );
    cx.get_mut(beta_test).state = bt;
    // L1188
    cx.get_mut(edge_search_highlight).state = esh;
    // L1189
    cx.get_mut(titlebar_name).state = stn;
    // L1190
    cx.get_mut(show_public_builds).state = spb;
    // L1191
    cx.get_mut(show_flavour_text).state = sft;
    // L1192
    cx.get_mut(show_animations).state = aff.0;
    // L1193
    cx.get_mut(show_all_item_affixes).state = aff.1;
    // L1194
    cx.get_mut(disable_scroll_control_interaction).state = dsci;

    // Adjust height in case of two-column layout
    // L1197
    lay.current_y = m_max(left_column_max_y, lay.current_y);

    // last line with buttons has more spacing
    // L1200
    lay.next_row(Some(1.5));

    // lock the Save/Cancel buttons to the bottom so they don't scroll away
    // L1203
    let save = cx.add(
        p,
        "save",
        ButtonControl::new(
            Anchor::nil(Bottom, Bottom),
            rect(-45, -10, 80, 20),
            "Save",
            move |cx| {
                // L1204
                let cp = cx.app().main.connection_protocol;
                cx.app_mut().launch.connection_protocol = Some(cp);
                // L1205
                let proxy_buf = cx.get(proxy_url).buf.clone();
                if pob_lua::lmatch(&proxy_buf, "%w", None).is_some() {
                    // L1206
                    let scheme = {
                        let w = cx.get(proxy_type);
                        match w.sel_index.checked_sub(1).and_then(|i| w.list.get(i)) {
                            Some(v) => v.get::<String>().cloned().unwrap_or_else(|| {
                                lua_error!("attempt to concatenate field 'scheme' (a nil value)")
                            }),
                            None => lua_error!("attempt to index field '?' (a nil value)"),
                        }
                    };
                    cx.app_mut().launch.proxy_url = Some(format!("{scheme}://{proxy_buf}"));
                } else {
                    // L1208
                    cx.app_mut().launch.proxy_url = None;
                }
                // L1210
                let build_buf = cx.get(build_path).buf.clone();
                if pob_lua::lmatch(&build_buf, "%S", None).is_some() {
                    // L1211
                    let mut bp = build_buf;
                    // L1212
                    if pob_lua::lmatch(&bp, "[\\/]$", None).is_none() {
                        // L1213
                        bp.push('/');
                    }
                    cx.app_mut().main.build_path = Some(bp);
                } else {
                    // L1216
                    let d = cx.app().main.default_build_path.clone();
                    cx.app_mut().main.build_path = d;
                }
                // L1218
                if cx.app().main.mode.as_deref() == Some("LIST") {
                    // L1219 self.modes.LIST:BuildList() (CCR-0099 B, CCR-0100 B)
                    crate::build_list::rebuild(cx);
                }
                // L1221
                if !cx.app().launch.dev_mode {
                    // L1222
                    let branch = if cx.app().main.beta_test == Some(true) {
                        "beta"
                    } else {
                        "master"
                    };
                    set_manifest_branch(cx, branch);
                }
                // L1224
                let dpi = cx.app().main.dpi_scale_override_percent;
                cx.sg.set_dpi_scale_override_percent(dpi as i32);
                // L1225
                close_popup(cx);
                // L1226
                save_settings(cx);
            },
        ),
    );
    let saved = saved_state.clone();
    // L1228
    let cancel = cx.add(
        p,
        "cancel",
        ButtonControl::new(
            Anchor::nil(Bottom, Bottom),
            rect(45, -10, 80, 20),
            "Cancel",
            move |cx| {
                let s = &*saved;
                // L1229
                cx.app_mut().main.node_power_theme = s.node_power_theme.clone();
                // L1230
                cx.app_mut().main.color_positive = s.color_positive.clone();
                // L1231
                update_color_code(cx, "POSITIVE", &s.color_positive);
                // L1232
                cx.app_mut().main.color_negative = s.color_negative.clone();
                // L1233
                update_color_code(cx, "NEGATIVE", &s.color_negative);
                // L1234
                cx.app_mut().main.color_highlight = s.color_highlight.clone();
                // L1235
                update_color_code(cx, "HIGHLIGHT", &s.color_highlight);
                // L1236
                cx.main.show_thousands_separators = s.show_thousands_separators;
                // L1237
                cx.app_mut().main.use_compact_values = s.use_compact_values;
                // L1238
                cx.main.thousands_separator = s.thousands_separator.clone();
                // L1239
                cx.main.decimal_separator = s.decimal_separator.clone();
                let m = &mut cx.app_mut().main;
                // L1240
                m.show_titlebar_name = s.show_titlebar_name;
                // L1241
                m.beta_test = s.beta_test;
                // L1242
                m.edge_search_highlight = s.edge_search_highlight;
                // L1243
                m.default_gem_quality = Some(s.default_gem_quality);
                // L1244
                m.default_char_level = Some(s.default_char_level);
                // L1245
                m.default_item_affix_quality = Some(s.default_item_affix_quality);
                // L1246
                m.show_warnings = s.show_warnings;
                // L1247
                m.slot_only_tooltips = s.slot_only_tooltips;
                // L1248
                m.migrate_eldritch_implicits = s.migrate_eldritch_implicits;
                // L1249
                cx.main.not_supported_mod_tooltips = s.not_supported_mod_tooltips;
                // L1250
                set_invert_slider_scroll_direction(cx, s.invert_slider_scroll_direction);
                let m = &mut cx.app_mut().main;
                // L1251
                m.disable_dev_auto_save = s.disable_dev_auto_save;
                // L1252
                m.show_public_builds = s.show_public_builds;
                // L1253
                cx.main.show_flavour_text = s.show_flavour_text;
                let m = &mut cx.app_mut().main;
                // L1254
                m.show_animations = s.show_animations;
                // L1255
                m.show_all_item_affixes = s.show_all_item_affixes;
                // L1256
                cx.main.disable_scroll_control_interaction = s.disable_scroll_control_interaction;
                // L1257
                cx.app_mut().main.dpi_scale_override_percent = s.dpi_scale_override_percent;
                // L1258
                cx.sg
                    .set_dpi_scale_override_percent(s.dpi_scale_override_percent as i32);
                // L1259
                close_popup(cx);
            },
        ),
    );
    built.save = Some(save.id());
    built.cancel = Some(cancel.id());

    // L1262
    let popup_height = if use_scroll_bar {
        screen_h - 20.0
    } else {
        lay.current_y + 30.0
    };
    built.popup_height = popup_height;

    // L1264
    if use_scroll_bar {
        // L1265
        let scroll_bar = cx.add(
            p,
            "scrollBar",
            ScrollBarControl::new(
                Anchor::nil(TopRight, TopRight),
                rect(-2, 25, scroll_bar_width, popup_height - 65.0),
                Some(50.0),
                Some(ScrollDir::Vertical),
                true,
            ),
        );
        // L1266
        ScrollBarControl::set_content_dimension(
            cx,
            scroll_bar.id(),
            lay.current_y,
            popup_height - 65.0,
        );
        built.scroll_bar = Some(scroll_bar.id());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_helpers() {
        // tostring(colorPositive:gsub('^(^)', '0'))
        assert_eq!(hex_display("^x33FF77"), "0x33FF77");
        assert_eq!(hex_display("0x33FF77"), "0x33FF77");
        assert_eq!(hex_display("x^"), "x^");
        // (q * 100) .. "%"
        assert_eq!(affix_quality_label(0.5), "50%");
        assert_eq!(affix_quality_label(pob_lua::round(0.57, Some(2))), "57%");
        assert_eq!(affix_quality_label(0.0), "0%");
    }

    #[test]
    fn next_row() {
        let mut l = Layout {
            current_y: 20.0,
            current_x: 0.0,
        };
        l.next_row(Some(0.2));
        l.next_row(Some(1.5));
        l.next_row(None);
        assert_eq!(l.current_y, 20.0 + 0.2 * 26.0 + 1.5 * 26.0 + 26.0);
    }
}
