//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/Control.lua (anchors, rect, GetPos,
//! GetSize, IsShown, IsEnabled, IsMouseInBounds) and Common.lua:413-416 (isMouseInRegion).
//! Ui doc §6.3. Owner: U4. UI-U0 stub: the §6.3 reference bodies; U4 verifies them with T0.
#![allow(clippy::many_single_char_names)]

use crate::cx::Cx;
use crate::ids::{CtrlId, CtrlKey, TabGroupId, ViewPort};
use crate::prop::Prop;

/// Control.lua:11-21 anchorPos keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorPoint {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
    Center,
}

impl AnchorPoint {
    /// Control.lua:11-21 anchorPos
    pub const fn factors(self) -> (f64, f64) {
        match self {
            Self::TopLeft => (0.0, 0.0),
            Self::Top => (0.5, 0.0),
            Self::TopRight => (1.0, 0.0),
            Self::Right => (1.0, 0.5),
            Self::BottomRight => (1.0, 1.0),
            Self::Bottom => (0.5, 1.0),
            Self::BottomLeft => (0.0, 1.0),
            Self::Left => (0.0, 0.5),
            Self::Center => (0.5, 0.5),
        }
    }
    /// The anchorPos key ("TOPLEFT", ...); anything else is not an anchor point.
    pub fn from_lua(s: &str) -> Option<AnchorPoint> {
        Some(match s {
            "TOPLEFT" => Self::TopLeft,
            "TOP" => Self::Top,
            "TOPRIGHT" => Self::TopRight,
            "RIGHT" => Self::Right,
            "BOTTOMRIGHT" => Self::BottomRight,
            "BOTTOM" => Self::Bottom,
            "BOTTOMLEFT" => Self::BottomLeft,
            "LEFT" => Self::Left,
            "CENTER" => Self::Center,
            _ => return None,
        })
    }
}

/// Lua `{"POINT", other, "OTHERPOINT", collapse}`.
#[derive(Clone, Debug, Default)]
pub struct Anchor {
    pub point: Option<AnchorPoint>,
    pub other: Option<CtrlId>,
    /// PopupDialog only: Lua `anchor.other` given as a sibling key string; resolved when the popup opens.
    pub other_key: Option<CtrlKey>,
    pub other_point: Option<AnchorPoint>,
    pub collapse: bool,
}

impl Anchor {
    pub const NONE: Anchor = Anchor {
        point: None,
        other: None,
        other_key: None,
        other_point: None,
        collapse: false,
    };
    /// Lua {"POINT", other, "OTHERPOINT"}
    pub fn new(point: AnchorPoint, other: impl Into<CtrlId>, other_point: AnchorPoint) -> Anchor {
        Anchor {
            point: Some(point),
            other: Some(other.into()),
            other_point: Some(other_point),
            ..Anchor::NONE
        }
    }
    /// Lua {"POINT", nil, "OTHERPOINT"} (inside a popup, `nil` means the popup)
    pub fn nil(point: AnchorPoint, other_point: AnchorPoint) -> Anchor {
        Anchor {
            point: Some(point),
            other_point: Some(other_point),
            ..Anchor::NONE
        }
    }
    /// Lua {"POINT", "siblingKey", "OTHERPOINT"} (popups only)
    pub fn key(point: AnchorPoint, key: &'static str, other_point: AnchorPoint) -> Anchor {
        Anchor {
            point: Some(point),
            other_key: Some(key.into()),
            other_point: Some(other_point),
            ..Anchor::NONE
        }
    }
    /// 4th element `true`
    pub fn collapse(mut self) -> Anchor {
        self.collapse = true;
        self
    }
}

/// The Control.lua fields every control has.
pub struct Base {
    /// self.rectStart
    pub rect_start: [Prop<f64>; 4],
    pub x: Prop<f64>,
    pub y: Prop<f64>,
    pub width: Prop<f64>,
    pub height: Prop<f64>,
    pub anchor: Anchor,
    /// `self.collapseX or 0`
    pub collapse_x: f64,
    pub collapse_y: f64,
    pub shown: Prop<bool>,
    pub enabled: Prop<bool>,
    /// nil until the first SetFocus. Control.lua:141 compares `focus ~= self.hasFocus`, so
    /// SetFocus(false) on a never-focused control DOES call OnFocusLost.
    pub has_focus: Option<bool>,
    pub tab_order: Option<TabGroupId>,
    pub mouse_clip_rect: Option<Prop<Option<[f64; 4]>>>,
}

impl Base {
    /// ControlClass:Control(anchor, rect) (Control.lua:55-66). Pass rect(0,0,0,0) for a nil rect.
    pub fn new(anchor: Anchor, rect: [Prop<f64>; 4]) -> Base {
        let [x, y, width, height] = rect.clone();
        Base {
            rect_start: rect,
            x,
            y,
            width,
            height,
            anchor,
            collapse_x: 0.0,
            collapse_y: 0.0,
            shown: Prop::Val(true),
            enabled: Prop::Val(true),
            has_focus: None,
            tab_order: None,
            mouse_clip_rect: None,
        }
    }
}

pub fn rect(
    x: impl Into<Prop<f64>>,
    y: impl Into<Prop<f64>>,
    w: impl Into<Prop<f64>>,
    h: impl Into<Prop<f64>>,
) -> [Prop<f64>; 4] {
    [x.into(), y.into(), w.into(), h.into()]
}

/// Control.lua:92-119. Exact, including floor and WHEN sizes are queried (dynamic dispatch via cx.pos/cx.size).
pub fn base_pos(cx: &Cx, id: CtrlId) -> (f64, f64) {
    let b = cx.base(id);
    if b.anchor.collapse {
        if let Some(o) = b.anchor.other {
            if !cx.base(o).shown.get(cx, o) {
                // other:GetProperty("shown"), NOT IsShown()
                let (x, y) = cx.pos(o);
                return (x + b.collapse_x, y + b.collapse_y);
            }
        }
    }
    let mut x = b.x.get(cx, id);
    let mut y = b.y.get(cx, id);
    if let Some(o) = b.anchor.other {
        let (ox, oy) = cx.pos(o);
        let op = b
            .anchor
            .other_point
            .unwrap_or_else(|| crate::lua_error!("invalid anchor position 'nil'"));
        let (ow, oh) = if op != AnchorPoint::TopLeft {
            cx.size(o)
        } else {
            (0.0, 0.0)
        };
        let p = b
            .anchor
            .point
            .unwrap_or_else(|| crate::lua_error!("invalid anchor position 'nil'"));
        let (w, h) = if p != AnchorPoint::TopLeft {
            cx.size(id)
        } else {
            (0.0, 0.0)
        };
        let ((ofx, ofy), (fx, fy)) = (op.factors(), p.factors());
        x = (ox + ow * ofx + x - w * fx).floor();
        y = (oy + oh * ofy + y - h * fy).floor();
    }
    (x, y)
}

/// Control.lua:121-123
pub fn base_size(cx: &Cx, id: CtrlId) -> (f64, f64) {
    let b = cx.base(id);
    (b.width.get(cx, id), b.height.get(cx, id))
}

/// Control.lua:125-127
pub fn base_is_shown(cx: &Cx, id: CtrlId) -> bool {
    let b = cx.base(id);
    (b.anchor.other.is_none() || b.anchor.collapse || cx.is_shown(b.anchor.other.unwrap()))
        && b.shown.get(cx, id)
}

/// Control.lua:129-131
pub fn base_is_enabled(cx: &Cx, id: CtrlId) -> bool {
    cx.base(id).enabled.get(cx, id)
}

/// Control.lua:133-138
pub fn base_is_mouse_in_bounds(cx: &Cx, id: CtrlId) -> bool {
    let (x, y) = cx.pos(id);
    let (w, h) = cx.size(id);
    let (cx_, cy) = cx.sys.cursor_pos();
    cx_ >= x && cy >= y && cx_ < x + w && cy < y + h
}

/// Common.lua:413-416 isMouseInRegion
pub fn is_mouse_in_region(cx: &Cx, r: &ViewPort) -> bool {
    let (x, y) = cx.sys.cursor_pos();
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}
