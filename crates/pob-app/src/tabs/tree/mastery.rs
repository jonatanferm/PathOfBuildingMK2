//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/PassiveMasteryControl.lua 1-59.
//! Owner: U25.
//!
//! @pob-file Classes/PassiveMasteryControl.lua @16de4b82d
//!
//! `newClass("PassiveMasteryControl", "ListControl")`: the mastery effect list of
//! `TreeTab:OpenMasteryPopup`. Rows are [`MasteryEffect`]s (`{ label, id }`). Hovering a row
//! shows the node tooltip with that effect applied (`AddValueTooltip` writes the effect into the
//! node, as the Lua does); clicking saves it (`TreeTab:SaveMasteryPopup`).
//!
//! `self.treeView` is `treeTab.viewer`, which the TreeTab owns: `AddValueTooltip` lends it
//! (ui doc §7.2) to U24's `AddNodeTooltip` (`tree_view::hooks().add_node_tooltip`).
//! `self.saveButton` is always nil (TreeTab.lua:1042 passes `controls.save`, which does not
//! exist) and is never read; it is not kept.
//!
//! DIVERGENCE: none. ORDER: no `pairs` loops. Props with side effects (ui doc §7.3): none.
#![allow(non_snake_case)]

use std::rc::Rc;

use pob_lua::math::m_max;
use pob_sg::Font;
use pob_ui_core::controls::list::{ListControl, ListData, ListHooks, ListValue};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::Tooltip;

use super::TreeTab;
use crate::app::CxApp;
use crate::model::{self, NodeRef};

// @pob Classes/PassiveMasteryControl.lua:1-24 h=95bb0fbc
// (header comment, local aliases, newClass and the annotations: the struct and Widget impl below)

/// `{ label = t_concat(effect.stats, " / "), id = effect.effect }` (TreeTab.lua:1030)
#[derive(Clone, Debug)]
pub struct MasteryEffect {
    pub label: String,
    pub id: f64,
}

/// `PassiveMasteryControl`
pub struct PassiveMasteryControl {
    pub list: ListControl,
    /// `self.treeTab`
    pub tree_tab: CtrlId,
    /// `self.node`
    pub node: NodeRef,
}

static HOOKS: ListHooks = ListHooks {
    get_row_value: PassiveMasteryControl::GetRowValue,
    add_value_tooltip: Some(PassiveMasteryControl::AddValueTooltip),
    on_sel_click: Some(PassiveMasteryControl::OnSelClick),
    ..ListHooks::BASE
};

fn effect_of(v: &ListValue) -> Rc<MasteryEffect> {
    v.rc::<MasteryEffect>()
        .unwrap_or_else(|| lua_error!("PassiveMasteryControl: a row that is not an effect"))
}

impl PassiveMasteryControl {
    /// The automagical width (L28-30): `rect[3]` grown to fit every label.
    pub fn auto_width(cx: &Cx, list: &[MasteryEffect], width: f64) -> f64 {
        let mut w = width;
        // L28
        for e in list {
            // L29
            w = m_max(w, cx.sg.draw_string_width(16.0, Font::Var, &e.label) + 5.0);
        }
        w
    }

    /// lua: PassiveMasteryControl.lua:25-38 `PassiveMasteryControlClass:PassiveMasteryControl(anchor,
    /// rect, list, treeTab, node, saveButton)` (`saveButton` is always nil: module docs)
    /// @pob Classes/PassiveMasteryControl.lua:25-38 h=c1dde821
    pub fn new(
        anchor: Anchor,
        r: [Prop<f64>; 4],
        list: Vec<MasteryEffect>,
        tree_tab: CtrlId,
        node: NodeRef,
    ) -> New<PassiveMasteryControl> {
        let [x, y, w, h] = r;
        // L26 self.list = list or { }
        let values: Vec<ListValue> = list.iter().cloned().map(|e| ListValue::of(Rc::new(e))).collect();
        New::new(
            Base::new(anchor, [x, y, w, h]),
            PassiveMasteryControl {
                // L31
                list: ListControl::state(&HOOKS, 16.0, false, false, ListData::Local(values), false),
                // L32
                tree_tab,
                // L34
                node,
            },
        )
        .host()
        .init(move |cx, me| {
            let me = me.0;
            // -- automagical width
            // L28-30 (needs the fonts: run with the node)
            let w0 = cx.base(me).width.raw();
            let w = Self::auto_width(cx, &list, w0);
            cx.base_mut(me).width = Prop::Val(w);
            // L31 (the scroll bars)
            ListControl::init(cx, me);
            // L33 self.treeView = treeTab.viewer (read through the TreeTab, module docs)
            // L35
            cx.w_mut::<ListControl>(me).sel_index = None;
            // L36 self.saveButton = saveButton (nil)
            // L37 return self
        })
    }

    /// lua: PassiveMasteryControl.lua:44-48 `PassiveMasteryControlClass:GetRowValue(column, index,
    /// effect)`
    /// @pob Classes/PassiveMasteryControl.lua:44-48 h=ecba977e
    fn GetRowValue(_cx: &Cx, _me: CtrlId, column: usize, _index: usize, v: &ListValue) -> String {
        // L45
        if column == 1 {
            // L46
            return effect_of(v).label.clone();
        }
        String::new()
    }

    /// lua: PassiveMasteryControl.lua:50-56 `PassiveMasteryControlClass:AddValueTooltip(tooltip,
    /// index, effect)`
    /// @pob Classes/PassiveMasteryControl.lua:50-56 h=7f624feb
    fn AddValueTooltip(cx: &mut Cx, me: CtrlId, tooltip: &mut Tooltip, _index: usize, v: &ListValue) {
        let effect = effect_of(v);
        let (tree_tab, node) = {
            let w = cx.w::<PassiveMasteryControl>(me);
            (w.tree_tab, w.node.clone())
        };
        let tree = cx.build().model.spec.get().tree.get();
        // L51
        tooltip.clear(false);
        // L52
        let me_effect = tree
            .masteryEffects
            .get(effect.id)
            .obj::<model::Node>()
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
        node.sd.set(me_effect.sd.get());
        // L53
        node.allMasteryOptions.set(false);
        // L54
        tree.ProcessStats(&node, None);
        // L55
        let f = crate::tree_view::hooks().add_node_tooltip;
        cx.lend::<TreeTab, _, _>(tree_tab, |w| &mut w.core.viewer, |cx, viewer| {
            f(cx, viewer, tooltip, &node);
        });
    }

    /// lua: PassiveMasteryControl.lua:58-60 `PassiveMasteryControlClass:OnSelClick(index, mastery,
    /// doubleClick)`
    /// @pob Classes/PassiveMasteryControl.lua:58-60 h=0d245030
    fn OnSelClick(cx: &mut Cx, me: CtrlId, _index: usize, _v: &ListValue, _double_click: bool) -> Option<bool> {
        let (tree_tab, node) = {
            let w = cx.w::<PassiveMasteryControl>(me);
            (w.tree_tab, w.node.clone())
        };
        // L59 (`listControl.selValue`)
        let sel = cx.w::<ListControl>(me).sel_value.as_ref().map(|v| effect_of(v).id);
        TreeTab::SaveMasteryPopup(cx, tree_tab, &node, sel);
        None
    }
}

impl Widget for PassiveMasteryControl {
    const NAME: &'static str = "PassiveMasteryControl";
    const CAPS: Caps = <ListControl as Widget>::CAPS;

    /// lua: PassiveMasteryControl.lua:40-42 `PassiveMasteryControlClass:Draw(viewPort)`
    /// @pob Classes/PassiveMasteryControl.lua:40-42 h=32e640c0
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        // L41
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

    pob_ui_core::impl_parts!(PassiveMasteryControl: list => ListControl);
}
