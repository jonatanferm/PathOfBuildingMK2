//! `pob-ui-core`: PoB's control framework (ui doc §6) and the generic controls.
//!
//! Cx, the Ui arena, Prop, Anchor/GetPos, Widget/VTable/Overrides, ControlHost dispatch,
//! InputEvents, focus/tab groups, Undo, coroutines, MainCore, colour codes, draw helpers,
//! WrapString, Tooltip/TooltipHost, PopupDialog and the main:Open*Popup family, and the generic
//! controls (Label…PathControl).
//!
//! Module declarations are UI-U0's (§12.1: later units never edit `mod`/`pub use` lines). File
//! owners: `ids`, `prop`, `widget` (complete in UI-U0, then U4); `cx`, `arena`, `layout`, `host`,
//! `input`, `focus`, `undo`, `coroutine`, `lua_error`, `main_core`, `color_codes` (U4); `luastr`
//! (U5); `draw`, controls label/button/check_box/rect_outline/section/scroll_bar/slider/dragger
//! (U9); `tooltip` (U10); edit/resizable_edit (U11); list/text_list/path (U12);
//! drop_down/search_host (U13); `popups`, `wrap`, popup_dialog (U14).
#![allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::new_ret_no_self,
    clippy::should_implement_trait,
    clippy::module_inception,
    clippy::wildcard_imports,
    clippy::unused_self,
    clippy::trivially_copy_pass_by_ref,
    clippy::fn_params_excessive_bools,
    clippy::ref_option,
    clippy::missing_const_for_fn,
    clippy::semicolon_if_nothing_returned,
    clippy::used_underscore_binding,
    clippy::needless_lifetimes,
    clippy::len_without_is_empty
)]

pub mod arena;
pub mod color_codes;
pub mod controls;
pub mod coroutine;
pub mod cx;
pub mod draw;
pub mod focus;
pub mod host;
pub mod ids;
pub mod input;
pub mod layout;
pub mod lua_error;
pub mod luastr;
pub mod main_core;
pub mod popups;
pub mod prelude;
pub mod prop;
pub mod tooltip;
pub mod undo;
pub mod widget;
pub mod wrap;

/// `crate::lua_error!` (ui doc §6.14): the calc plan's macro.
pub use pob_lua::lua_error;
