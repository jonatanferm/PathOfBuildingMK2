//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/TimelessJewelSocketControl.lua 1-57
//! (`newClass("TimelessJewelSocketControl", "DropDownControl")`). Owner: U28.
//!
//! @pob-file Classes/TimelessJewelSocketControl.lua @16de4b82d
//!
//! The socket picker of the timeless jewel search popup (TreeTab.lua:1638, U27): a dropdown of
//! jewel sockets whose hover draws a 300x300 tree view centred on the hovered socket.
//!
//! - DropDownControl hook table: none (CCR-0086); this class overrides `Draw` only. Its
//!   `self.DropDownControl:Draw(viewPort)` is the super call with `noTooltip` nil, then
//!   `self:DrawControls(viewPort)` (the scroll bar) runs a second time, as in the Lua.
//! - List entries are the tables TreeTab builds (`{ label, keystone, id }`, TreeTab.lua:1306-1340):
//!   a [`DropItem`] whose `value` is that [`Table`]. A string entry (or any other value) has no
//!   `id` (`("x").id` is nil), so its hover draws nothing.
//! - `self.build` is the [`TimelessBuild`] of the list control (CCR-0112): `spec.nodes[id]` and
//!   `spec.tree.size`. `self.socketViewer` is a PassiveTreeView (U23, not ported yet): the
//!   [`SocketViewer`] trait carries the three field writes and the `Draw` call it receives.
//! - Props with side effects (ui doc §7.3): none.

use std::rc::Rc;

use pob_lua::math::m_min;
use pob_ui_core::controls::drop_down::{DropDownControl, DropItem, SelFunc};
use pob_ui_core::prelude::*;

use super::timeless_list::TimelessBuild;
use crate::model::{Table, Val};

// @pob Classes/TimelessJewelSocketControl.lua:1-11 h=0681db5d (header, locals and newClass: the struct and Widget impl below)

/// `self.socketViewer` (a `PassiveTreeView`, Classes/PassiveTreeView.lua): what this class does
/// with it.
pub trait SocketViewer {
    /// `socketViewer.zoom = v`
    fn set_zoom(&self, cx: &mut Cx, v: f64);
    /// `socketViewer.zoomX = v`
    fn set_zoom_x(&self, cx: &mut Cx, v: f64);
    /// `socketViewer.zoomY = v`
    fn set_zoom_y(&self, cx: &mut Cx, v: f64);
    /// `socketViewer:Draw(build, viewPort, { })` (empty input events)
    fn draw(&self, cx: &mut Cx, vp: &ViewPort);
}

pub struct TimelessJewelSocketControl {
    pub dd: DropDownControl,
    /// `self.build`
    pub build: Rc<dyn TimelessBuild>,
    /// `self.socketViewer`
    pub socket_viewer: Rc<dyn SocketViewer>,
}

impl TimelessJewelSocketControl {
    /// lua: TimelessJewelSocketControl.lua:18-23
    /// TimelessJewelSocketControl(anchor, rect, list, selFunc, build, socketViewer)
    /// @pob Classes/TimelessJewelSocketControl.lua:12-23 h=7af97e23
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        list: Vec<DropItem>,
        sel_func: Option<SelFunc>,
        build: Rc<dyn TimelessBuild>,
        socket_viewer: Rc<dyn SocketViewer>,
    ) -> New<TimelessJewelSocketControl> {
        New::new(
            Base::new(anchor, rect),
            TimelessJewelSocketControl {
                // L19
                dd: DropDownControl::state(list, sel_func, None, false),
                // L20
                build,
                // L21
                socket_viewer,
            },
        )
        .host()
        // L19 (the scroll bar, SetList, the widths)
        .init(|cx, me| DropDownControl::init(cx, me.id()))
    }

    /// `listVal.id` of a dropdown entry (nil for a string entry or a table without `id`).
    fn entry_id(item: &DropItem) -> Val {
        item.get::<Table>().map_or(Val::Nil, |t| t.get("id"))
    }
}

impl Widget for TimelessJewelSocketControl {
    const NAME: &'static str = "TimelessJewelSocketControl";
    const CAPS: Caps = <DropDownControl as Widget>::CAPS;

    /// lua: TimelessJewelSocketControl.lua:25-58 Draw(viewPort, noTooltip)
    /// @pob Classes/TimelessJewelSocketControl.lua:25-58 h=2645a36f
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        // L26
        let (x, y) = cx.pos(me);
        // L27
        let (_width, _height) = cx.size(me);
        // L28 (super Draw; noTooltip nil)
        DropDownControl::draw(cx, me, vp, false);
        // L29
        cx.draw_controls(me, vp, None);
        // L30
        if cx.is_mouse_over(me) && !no_tooltip && cx.main.popups.get(1).is_none() {
            // L31
            let hover = DropDownControl::get_hover_index(cx, me);
            let hovered_socket_id = {
                let dd = cx.w::<DropDownControl>(me);
                match hover.checked_sub(1).and_then(|i| dd.list.get(i)) {
                    Some(item) => Self::entry_id(item),
                    None => lua_error!("attempt to index field '?' (a nil value)"),
                }
            };
            let (build, viewer) = {
                let w = cx.w::<Self>(me);
                (w.build.clone(), w.socket_viewer.clone())
            };
            // L32
            let node = build.spec_node(cx, &hovered_socket_id);
            // L33
            if let Some(node) = node {
                // L34
                cx.sg.set_draw_layer(None, Some(15));
                // L35
                let viewer_y = {
                    let dd = cx.w::<DropDownControl>(me);
                    // L36
                    if dd.drop_up && dd.dropped {
                        // L37
                        y + 20.0
                    } else {
                        // L39
                        m_min(y - 150.0 - 5.0, vp.y + vp.height - 154.0)
                    }
                };
                // L41
                let viewer_x = x + 215.0;
                // L42
                cx.sg.set_draw_color(1.0, 1.0, 1.0);
                // L43
                cx.sg.draw_image(None, viewer_x, viewer_y, 304.0, 304.0);
                // L44
                viewer.set_zoom(cx, 5.0);
                // L45
                let scale = build.spec_tree_size(cx).num() / 1500.0;
                // L46
                viewer.set_zoom_x(cx, -node.x.get() / scale);
                // L47
                viewer.set_zoom_y(cx, -node.y.get() / scale);
                // L48
                cx.sg
                    .set_viewport(viewer_x + 2.0, viewer_y + 2.0, 300.0, 300.0);
                // L49
                viewer.draw(
                    cx,
                    &ViewPort {
                        x: 0.0,
                        y: 0.0,
                        width: 300.0,
                        height: 300.0,
                    },
                );
                // L50
                cx.sg.set_draw_layer(None, Some(30));
                // L51
                cx.sg.set_draw_color_a(1.0, 1.0, 1.0, 0.2);
                // L52
                cx.sg.draw_image(None, 149.0, 0.0, 2.0, 300.0);
                // L53
                cx.sg.draw_image(None, 0.0, 149.0, 300.0, 2.0);
                // L54
                cx.sg.reset_viewport();
                // L55
                cx.sg.set_draw_layer(None, Some(0));
            }
        }
    }
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        DropDownControl::is_mouse_over_comp(cx, me).0
    }
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, _double_click: bool) -> Focus {
        DropDownControl::on_key_down(cx, me, key)
    }
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        DropDownControl::on_key_up(cx, me, key)
    }
    fn on_char(cx: &mut Cx, me: CtrlId, ch: Key) -> Focus {
        DropDownControl::on_char(cx, me, ch)
    }

    pob_ui_core::impl_parts!(TimelessJewelSocketControl: dd => DropDownControl);
}
