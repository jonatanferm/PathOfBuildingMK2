//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/TextListControl.lua 1-122
//! (`newClass("TextListControl", "Control", "ControlHost")`). Owner: U12.
//!
//! @pob-file Classes/TextListControl.lua @16de4b82d
//!
//! Owner's amendments to the UI-U0 stub (ui doc §13 item 12, CCR-0078 C):
//! - A row (`lineInfo`) carries the optional `x`, `align`, `font`, `underline` keys Draw reads,
//!   plus `extra` for the caller's own keys (Build.lua:1994-2004 stores `breakdown`, `modNames`,
//!   `actorName`, ... and reads them back through `hoveredLine.line`). Rows are `Rc` so
//!   `hoveredLine.line` is the row itself, as the Lua table reference is.
//! - `new` takes `columns: Option<..>` (`columns or { { x = 0, align = "LEFT" } }`).
//! - `on_click` (`self.onClick`, Build.lua:576) and `hovered_line` (`self.hoveredLine`).
//!
//! ORDER: `pairs(self.list)` (L35) sums heights: (c) a sum over the array part visits 1..n,
//! exactly the Vec order. `pairs(self.columns)` (L45) iterates an array part too: (c), 1..n.

use std::any::Any;
use std::rc::Rc;

use pob_sg::{Align, Font, Key};

use crate::controls::scroll_bar::ScrollBarControl;
use crate::prelude::*;

// @pob-skip Classes/TextListControl.lua:1-8 reason="header comment and newClass (the Widget impl below)"

/// One `columns[i]` entry (`{ x, align }`).
#[derive(Clone, Debug, PartialEq)]
pub struct TextListColumn {
    pub x: f64,
    pub align: pob_sg::Align,
}

/// One `list[i]` row (`{ height, [1] = text, ..., x, align, font, underline }`).
#[derive(Clone, Default)]
pub struct TextListRow {
    pub height: f64,
    /// `[colIndex]` (index 0 = Lua `[1]`; None and missing = nil)
    pub cells: Vec<Option<String>>,
    /// `lineInfo.x`
    pub x: Option<f64>,
    /// `lineInfo.align`
    pub align: Option<Align>,
    /// `lineInfo.font`
    pub font: Option<Font>,
    /// `lineInfo.underline` (`underline[colIndex]` truthy; index 0 = Lua `[1]`)
    pub underline: Option<Vec<bool>>,
    /// The caller's other keys.
    pub extra: Option<Rc<dyn Any>>,
}

impl TextListRow {
    /// `{ height = h, a, b, ... }`
    pub fn new(height: f64, cells: &[&str]) -> TextListRow {
        TextListRow {
            height,
            cells: cells.iter().map(|c| Some((*c).to_owned())).collect(),
            ..TextListRow::default()
        }
    }
    /// `lineInfo[colIndex]`
    pub fn cell(&self, col_index: usize) -> Option<&str> {
        self.cells
            .get(col_index.wrapping_sub(1))
            .and_then(|c| c.as_deref())
    }
}

impl std::fmt::Debug for TextListRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextListRow")
            .field("height", &self.height)
            .field("cells", &self.cells)
            .finish_non_exhaustive()
    }
}

/// `self.hoveredLine` (`{ line = lineInfo, x = x, y = rowY, width = width }`).
#[derive(Clone, Debug)]
pub struct HoveredLine {
    pub line: Rc<TextListRow>,
    pub x: f64,
    pub y: f64,
    pub width: f64,
}

pub struct TextListControl {
    pub columns: Vec<TextListColumn>,
    pub list: Vec<Rc<TextListRow>>,
    pub section_heights: Option<Vec<f64>>,
    /// `self.onClick(self.hoveredLine)`
    pub on_click: Option<Rc<dyn Fn(&mut Cx, Option<HoveredLine>)>>,
    /// `self.hoveredLine` (written by Draw)
    pub hovered_line: Option<HoveredLine>,
}

impl TextListControl {
    /// lua: TextListControl.lua:9-21 TextListControl(anchor, rect, columns, list, sectionHeights)
    /// @pob Classes/TextListControl.lua:9-21 h=9ca814ac
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        columns: Option<Vec<TextListColumn>>,
        list: Vec<Rc<TextListRow>>,
        section_heights: Option<Vec<f64>>,
    ) -> New<TextListControl> {
        // L10-L11
        New::new(
            Base::new(anchor, rect),
            TextListControl {
                // L17
                columns: columns.unwrap_or_else(|| {
                    vec![TextListColumn {
                        x: 0.0,
                        align: Align::Left,
                    }]
                }),
                // L18
                list,
                // L19
                section_heights,
                on_click: None,
                hovered_line: None,
            },
        )
        .host()
        .init(|cx, me| {
            let me = me.0;
            // L12
            let sb = cx.add(
                me,
                "scrollBar",
                ScrollBarControl::new(
                    Anchor::new(Right, me, Right),
                    crate::layout::rect(-1, 0, 18, 0),
                    Some(40.0),
                    None,
                    false,
                ),
            );
            // L13
            cx.base_mut(sb).height = Prop::f(move |cx, _| {
                // L14
                let (_width, height) = cx.size(me);
                // L15
                height - 2.0
            });
        })
    }

    fn scroll_bar(cx: &Cx, me: CtrlId) -> CtrlId {
        cx.child_as::<ScrollBarControl>(me, "scrollBar").id()
    }
}

impl Widget for TextListControl {
    const NAME: &'static str = "TextListControl";
    const CAPS: Caps = Caps::DRAW
        .union(Caps::IS_MOUSE_OVER)
        .union(Caps::KEY_DOWN)
        .union(Caps::KEY_UP);

    /// lua: TextListControl.lua:23-28
    /// @pob Classes/TextListControl.lua:23-28 h=e6551226
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        // L24
        if !cx.is_shown(me) {
            // L25
            return false;
        }
        // L27
        cx.is_mouse_in_bounds(me) || cx.get_mouse_over_control(me).is_some()
    }

    /// lua: TextListControl.lua:30-81
    /// @pob Classes/TextListControl.lua:30-81 h=f88f40c5
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, _no_tooltip: bool) {
        // L31
        let (x, y) = cx.pos(me);
        // L32
        let (width, height) = cx.size(me);
        // L33
        let scroll_bar = Self::scroll_bar(cx, me);
        // L34
        let mut content_height = 0.0;
        // L35
        for line_info in &cx.w::<Self>(me).list {
            // L36
            content_height += line_info.height;
        }
        // L38
        ScrollBarControl::set_content_dimension(cx, scroll_bar, content_height, height - 4.0);
        // L39
        cx.sg.set_draw_color(0.66, 0.66, 0.66);
        // L40
        cx.sg.draw_image(None, x, y, width, height);
        // L41
        cx.sg.set_draw_color(0.05, 0.05, 0.05);
        // L42
        cx.sg
            .draw_image(None, x + 1.0, y + 1.0, width - 2.0, height - 2.0);
        // L43
        cx.draw_controls(me, vp, None);
        // L44
        cx.sg
            .set_viewport(x + 2.0, y + 2.0, width - 20.0, height - 4.0);
        let (columns, list) = {
            let w = cx.w::<Self>(me);
            (w.columns.clone(), w.list.clone())
        };
        // L45
        for (col_index, col_info) in columns.iter().enumerate() {
            let col_index = col_index + 1;
            // L46
            let mut line_y = -cx.w::<ScrollBarControl>(scroll_bar).offset;
            // L47
            for line_info in &list {
                // L48
                if let Some(text) = line_info.cell(col_index) {
                    // L49
                    let mut text_x = line_info.x.unwrap_or(col_info.x);
                    // L50
                    let align = line_info.align.unwrap_or(col_info.align);
                    // L51
                    cx.sg.draw_string(
                        text_x,
                        line_y,
                        align,
                        line_info.height,
                        line_info.font.unwrap_or(Font::Var),
                        text,
                    );
                    // L52
                    let underlined = line_info
                        .underline
                        .as_ref()
                        .is_some_and(|u| u.get(col_index - 1).copied().unwrap_or(false));
                    if underlined {
                        // L53
                        let width = cx.sg.draw_string_width(
                            line_info.height,
                            Font::Var,
                            &pob_sg::strip_escapes(text),
                        );
                        // L54-L55 note: not fully handled. this is currently only used for
                        // the side bar stats
                        // L56
                        if align == Align::RightX {
                            // L57
                            text_x -= width;
                        }
                        // L59
                        cx.sg.set_draw_color(0.5, 0.5, 0.5);
                        // L60
                        cx.sg
                            .draw_image(None, text_x, line_y + line_info.height, width, 1.0);
                    }
                }
                // L63
                line_y += line_info.height;
            }
        }
        // L66 determine which line the user is hovering over
        // L67
        cx.w_mut::<Self>(me).hovered_line = None;
        // L68
        let (cursor_x, cursor_y) = cx.sys.cursor_pos();
        // L69
        if cursor_x >= x + 2.0
            && cursor_x < x + width - 18.0
            && cursor_y >= y + 2.0
            && cursor_y < y + height - 2.0
        {
            // L70
            let mut row_y = y - cx.w::<ScrollBarControl>(scroll_bar).offset + 2.0;
            // L71 suboptimal. should do binary search if this causes performance problems
            // L72
            for line_info in &list {
                // L73
                if cursor_y >= row_y && cursor_y < row_y + line_info.height {
                    // L74
                    cx.w_mut::<Self>(me).hovered_line = Some(HoveredLine {
                        line: line_info.clone(),
                        x,
                        y: row_y,
                        width,
                    });
                    // L75
                    break;
                }
                // L77
                row_y += line_info.height;
            }
        }
        // L80
        cx.sg.reset_viewport();
    }

    /// lua: TextListControl.lua:83-94
    /// @pob Classes/TextListControl.lua:83-94 h=8e18ace9
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, _double_click: bool) -> Focus {
        // L84
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L85
            return None;
        }
        // L87
        if key == Key::LeftButton {
            if let Some(f) = cx.w::<Self>(me).on_click.clone() {
                // L88
                let hovered = cx.w::<Self>(me).hovered_line.clone();
                f(cx, hovered);
            }
        }
        // L90
        let m_over_control = cx.get_mouse_over_control(me);
        // L91
        if let Some(m) = m_over_control {
            if cx.has(m, Caps::KEY_DOWN) {
                // L92 (OnKeyDown(key): doubleClick nil)
                return cx.on_key_down(m, key, false);
            }
        }
        None
    }

    /// lua: TextListControl.lua:96-122
    /// @pob Classes/TextListControl.lua:96-122 h=545deec5
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        // L97
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L98
            return None;
        }
        // L100
        let scroll_bar = Self::scroll_bar(cx, me);
        // L101
        if cx.sys.is_key_down(Key::Shift) {
            if let Some(section_heights) = cx.w::<Self>(me).section_heights.clone() {
                let n = section_heights.len();
                // L102
                for (i, &height) in section_heights.iter().enumerate() {
                    let i = i + 1;
                    // L103
                    if height >= cx.w::<ScrollBarControl>(scroll_bar).offset {
                        // L104
                        if key == Key::WheelDown {
                            // L105
                            let to = if i == n {
                                section_heights[i - 1]
                            } else {
                                section_heights[i]
                            };
                            ScrollBarControl::set_offset(cx, scroll_bar, to);
                            // L106
                            return Some(me);
                        } else if key == Key::WheelUp {
                            // L108
                            let to = if i == 1 {
                                section_heights[i - 1]
                            } else {
                                section_heights[i - 2]
                            };
                            ScrollBarControl::set_offset(cx, scroll_bar, to);
                            // L109
                            return Some(me);
                        }
                    }
                }
                // L113 (`offsetMax` is nil before the first Draw: the next `-offset` would be
                // the Lua error; the port raises it here)
                let offset_max = cx
                    .w::<ScrollBarControl>(scroll_bar)
                    .offset_max
                    .unwrap_or_else(|| {
                        crate::lua_error!(
                            "attempt to perform arithmetic on field 'offset' (a nil value)"
                        )
                    });
                cx.w_mut::<ScrollBarControl>(scroll_bar).offset = offset_max;
                // L114
                return Some(me);
            }
        }
        // L116
        if key == Key::WheelDown {
            // L117
            ScrollBarControl::scroll(cx, scroll_bar, 1.0);
            // L118
            return Some(me);
        } else if key == Key::WheelUp {
            // L120
            ScrollBarControl::scroll(cx, scroll_bar, -1.0);
            // L121
            return Some(me);
        }
        None
    }
}
