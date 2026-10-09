//! The names every control/tab port uses (`use pob_ui_core::prelude::*;`). UI-U0.
//!
//! Anchor points are exported unqualified (`TopLeft`, `Right`, ...) because ports write them as
//! the Lua strings ("TOPLEFT", "RIGHT"), ui doc §9.

pub use pob_sg::Key;

pub use crate::cx::Cx;
pub use crate::ids::{Ctrl, CtrlId, CtrlKey, TabGroupId, ViewPort};
pub use crate::input::{EventKind, InputEvent, InputEvents};
pub use crate::layout::AnchorPoint::{self, *};
pub use crate::layout::{
    base_is_enabled, base_is_mouse_in_bounds, base_is_shown, base_pos, base_size,
    is_mouse_in_region, rect, Anchor, Base,
};
pub use crate::lua_error;
pub use crate::prop::{Prop, PropFn};
pub use crate::widget::{
    nil_method, vtable_of, Caps, DragValue, Duck, Focus, New, Overrides, PlainControl, Widget,
};
