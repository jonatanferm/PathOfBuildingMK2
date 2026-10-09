//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/Tooltip.lua 1-698 (ui doc §6.9). Owner: U10.
//! UI-U0 stub: final signatures.
//!
//! Notes (§6.9): CheckForUpdate compares with Lua `~=` (tables by identity: `TtKey::Obj` holds the
//! `Rc` so an address cannot be reused while compared); Draw relies on GetDrawColor save/restore
//! (Tooltip.lua:613); a widget-owned tooltip is always drawn through `cx.lend`.

use std::rc::Rc;

use pob_sg::Font;

use crate::cx::Cx;
use crate::ids::ViewPort;

/// CheckForUpdate args
#[derive(Clone, Debug, PartialEq)]
pub enum TtKey {
    Nil,
    Bool(bool),
    Num(f64),
    Str(Rc<str>),
    Obj(TtObj),
}

/// A table argument: compared by Rc identity and kept alive (Lua's updateParams holds the reference too).
#[derive(Clone)]
pub struct TtObj(pub Rc<dyn std::any::Any>);
impl PartialEq for TtObj {
    fn eq(&self, o: &Self) -> bool {
        Rc::ptr_eq(&self.0, &o.0)
    }
}
impl std::fmt::Debug for TtObj {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TtObj({:p})", Rc::as_ptr(&self.0))
    }
}

/// self.color = {r,g,b} | "^x.."
#[derive(Clone)]
pub enum TtColor {
    Rgb([f64; 3]),
    Esc(String),
}

/// AddLine background arg
#[derive(Clone)]
pub enum TtBackground {
    Image(String),
    Color(String),
}

/// modLine (reads .disabled etc. via downcast)
pub type ModLineRef = Rc<dyn std::any::Any>;

/// tooltip.Draw = function(self, ...) (TreeTab.lua:1749 etc.)
pub type TooltipDrawFn =
    Rc<dyn Fn(&mut Cx, &mut Tooltip, f64, f64, Option<f64>, Option<f64>, &ViewPort)>;

/// One `self.lines` entry (`{ size, text, block, font, center, modLine, background }`, or a
/// separator line `{ size, separatorImage }`).
#[derive(Clone, Default)]
pub struct TooltipLine {
    pub size: f64,
    pub text: Option<String>,
    pub block: usize,
    pub font: Option<Font>,
    pub center: bool,
    pub mod_line: Option<ModLineRef>,
    pub background: Option<TtBackground>,
    pub separator_image: Option<String>,
}

/// One `self.blocks` entry (`{ height }`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TooltipBlock {
    pub height: f64,
}

#[derive(Default)]
pub struct Tooltip {
    pub lines: Vec<TooltipLine>,
    pub blocks: Vec<TooltipBlock>,
    pub update_params: Option<Vec<TtKey>>,
    /// updateParams.notSupportedModTooltips
    pub update_nsmt: Option<bool>,
    /// `self.tooltipHeader` (false or a header name)
    pub tooltip_header: Option<String>,
    pub title_y_offset: f64,
    pub recipe: Option<Vec<String>>,
    pub center: bool,
    pub max_width: Option<f64>,
    pub color: Option<TtColor>,
    /// tooltip.Draw = function(self, ...) (TreeTab.lua:1749 etc.)
    pub draw_override: Option<TooltipDrawFn>,
    /// `self.childTooltips` (U10 completes the remaining Tooltip.lua fields)
    pub child_tooltips: Option<Vec<Tooltip>>,
}

impl Tooltip {
    /// lua: Tooltip.lua:54-69
    pub fn clear(&mut self, clear_update_params: bool) {
        let _ = clear_update_params;
        todo!("U10: Tooltip:Clear")
    }
    /// lua: Tooltip.lua:71-89 (also main.notSupportedModTooltips)
    pub fn check_for_update(&mut self, cx: &Cx, params: &[TtKey]) -> bool {
        let _ = (cx, params);
        todo!("U10: Tooltip:CheckForUpdate")
    }
    /// lua: Tooltip.lua:91-114 (`AddLine(size, text)`)
    pub fn add_line(&mut self, cx: &Cx, size: f64, text: &str) {
        let _ = (cx, size, text);
        todo!("U10: Tooltip:AddLine")
    }
    /// lua: Tooltip.lua:91-114 (`AddLine(size, text, font, modLine, background)`)
    pub fn add_line_ex(
        &mut self,
        cx: &Cx,
        size: f64,
        text: &str,
        font: Option<Font>,
        mod_line: Option<ModLineRef>,
        background: Option<TtBackground>,
    ) {
        let _ = (cx, size, text, font, mod_line, background);
        todo!("U10: Tooltip:AddLine")
    }
    /// lua: Tooltip.lua:116-118
    pub fn set_recipe(&mut self, recipe: Option<Vec<String>>) {
        let _ = recipe;
        todo!("U10: Tooltip:SetRecipe")
    }
    /// lua: Tooltip.lua:120-160
    pub fn add_separator(&mut self, cx: &Cx, size: Option<f64>) {
        let _ = (cx, size);
        todo!("U10: Tooltip:AddSeparator")
    }
    /// lua: Tooltip.lua:163-192
    pub fn get_size(&self, cx: &Cx) -> (f64, f64) {
        let _ = cx;
        todo!("U10: Tooltip:GetSize")
    }
    /// lua: Tooltip.lua:194-203
    pub fn get_dynamic_size(&self, cx: &Cx, vp: &ViewPort) -> (f64, f64) {
        let _ = (cx, vp);
        todo!("U10: Tooltip:GetDynamicSize")
    }
    /// tooltip:Draw: calls draw_override if set, else real_draw.
    pub fn draw(
        &mut self,
        cx: &mut Cx,
        x: f64,
        y: f64,
        w: Option<f64>,
        h: Option<f64>,
        vp: &ViewPort,
    ) {
        if let Some(f) = self.draw_override.clone() {
            return f(cx, self, x, y, w, h, vp);
        }
        self.real_draw(cx, x, y, w, h, vp)
    }
    /// lua: Tooltip.lua:379-698
    pub fn real_draw(
        &mut self,
        cx: &mut Cx,
        x: f64,
        y: f64,
        w: Option<f64>,
        h: Option<f64>,
        vp: &ViewPort,
    ) {
        let _ = (cx, x, y, w, h, vp);
        todo!("U10: Tooltip:Draw")
    }
}
