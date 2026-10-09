//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/NotesTab.lua 1-112. Owner: U22.
//! UI-U0 stub: the ui doc §9.2 surface; the §9.2 reference body compiles against the frozen
//! contracts in `pob-ui-core/tests/compile_shape.rs`.

use pob_ui_core::prelude::*;

use crate::model::xml::XmlNode;

pub struct NotesTab {
    pub build: CtrlId,
    pub last_content: String,
    pub show_color_codes: bool,
    pub mod_flag: bool,
}

/// drawn by BuildMode
impl Widget for NotesTab {
    const NAME: &'static str = "NotesTab";
    const CAPS: Caps = Caps::empty();
}

impl NotesTab {
    /// lua: NotesTab.lua:12-51
    pub fn new(build: CtrlId) -> New<NotesTab> {
        let _ = build;
        todo!("U22: NotesTab.lua:12-51")
    }
    /// lua: NotesTab.lua:53-62
    pub fn set_show_color_codes(cx: &mut Cx, me: CtrlId, setting: bool) {
        let _ = (cx, me, setting);
        todo!("U22: NotesTab:SetShowColorCodes")
    }
    /// lua: NotesTab.lua:64-73
    pub fn set_color(cx: &mut Cx, me: CtrlId, color: &str) {
        let _ = (cx, me, color);
        todo!("U22: NotesTab:SetColor")
    }
    /// lua: NotesTab.lua:75-82
    pub fn load(cx: &mut Cx, me: CtrlId, xml: &XmlNode, file_name: &str) {
        let _ = (cx, me, xml, file_name);
        todo!("U22: NotesTab:Load")
    }
    /// lua: NotesTab.lua:84-88
    pub fn save(cx: &mut Cx, me: CtrlId, xml: &mut XmlNode) {
        let _ = (cx, me, xml);
        todo!("U22: NotesTab:Save")
    }
    /// lua: NotesTab.lua:90-112
    pub fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, ev: &mut InputEvents) {
        let _ = (cx, me, vp, ev);
        todo!("U22: NotesTab:Draw")
    }
}
