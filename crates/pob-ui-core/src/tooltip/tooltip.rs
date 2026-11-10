//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/Tooltip.lua 1-698 (ui doc §6.9). Owner: U10.
//!
//! @pob-file Classes/Tooltip.lua @16de4b82d
//! @pob-file Modules/Main.lua @16de4b82d
//!
//! Notes (§6.9): CheckForUpdate compares with Lua `~=` (tables by identity: `TtKey::Obj` holds the
//! `Rc` so an address cannot be reused while compared); Draw relies on GetDrawColor save/restore
//! (Tooltip.lua:613); a widget-owned tooltip is always drawn through `cx.lend`.
//!
//! Lua tables of the class as Rust data: a `self.lines` entry is a [`TooltipLine`] (the fields
//! `CalculateColumns` writes into it, `_handled` and `bounds`, are `Cell`s because `GetSize` /
//! `GetDynamicSize` take `&self`); the `drawStack` of `CalculateColumns` is a `Vec<TtDrawEntry>`,
//! whose `#line >= 6` test ("text entry") is the `Text` variant.
//!
//! DIVERGENCE / CCR notes (CCR-0073):
//! - `AddSeparator` takes `&Cx` (frozen §6.9) but Lua creates and loads the separator image
//!   there (Tooltip.lua:145-149). The handle is created at once (so line identity and the
//!   `separatorImagePath` cache are Lua's) and its `Load` is queued in one thread-local queue
//!   shared by all tooltips; the queue is flushed, oldest first, at the top of the next
//!   `real_draw` of any tooltip ([`flush_pending_loads`], also public). Loads therefore keep
//!   Lua's order among themselves (a parent built with its child tooltips, then drawn); only the
//!   position of the `ImageLoad` side effect relative to other side effects between the build
//!   and the next tooltip draw can differ. CCR-0073 A proposes `&mut Cx`.
//! - The module-level recipe preload (Tooltip.lua:36-41) runs when Lua first loads the class, which
//!   Rust cannot observe: the owner of that moment (the app shell, a gallery) calls
//!   [`module_load`] once. Draw resolves a recipe icon through the handle `module_load` stored,
//!   re-validated against the current `Sg`'s image registry (a fresh `Sg`, e.g. in tests, gets a
//!   silent re-load of the same `(path, CLAMP)` key, which SG dedups).
//! - `data.modLine.disabled` (Tooltip.lua:302): a mod line passed to `AddLine` is read through a
//!   downcast to [`TtModLine`]; any other `ModLineRef` reads as not disabled (CCR-0073 B).
//! - `TtBackground::{Image, Color}` are both Lua strings: Tooltip.lua treats every string
//!   background as an image path (Tooltip.lua:615-625), so both variants do (CCR-0073 C).
//! - `main:WrapString` (Main.lua:1795-1820) is U14's `crate::wrap::wrap_string` (CCR-0073 D).
//! - Image loads record the `ImageLoad` side effect unless `Sg` already did (CCR-0028 item 2), as
//!   the oracle's bridge does for the Lua side.
//!
//! ORDER: the only `pairs` loop (Tooltip.lua:38, `recipeNames`) runs over a pure array part, which
//! LuaJIT's `next` visits 1..n: resolution (c), array order.

// Lua-shaped arithmetic (`x = x + w`, `(titleSize - titleSize)/2` at Tooltip.lua:259) and Lua's
// local names are kept line for line.
#![allow(clippy::assign_op_pattern, clippy::eq_op, clippy::many_single_char_names)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use pob_lua::math::{lpow, m_max, m_min};
use pob_sg::trace::{ImgRef, SideEffect};
use pob_sg::{Align, Font, ImageFlags, ImageHandle};

use crate::cx::Cx;
use crate::ids::ViewPort;
use crate::lua_error;

// @pob-skip Classes/Tooltip.lua:6-10 reason="local aliases of library functions"

/// @pob Classes/Tooltip.lua:12-16 h=6c47eac5
// Constants
const BORDER_WIDTH: f64 = 1.0;
const H_PAD: f64 = 12.0;
const V_PAD: f64 = 10.0;

/// All possible values for notable recipes (oils)
/// @pob Classes/Tooltip.lua:18-34 h=500e9edb
pub const RECIPE_NAMES: [&str; 14] = [
    "AmberOil",
    "AzureOil",
    "BlackOil",
    "ClearOil",
    "CrimsonOil",
    "GoldenOil",
    "IndigoOil",
    "OpalescentOil",
    "PrismaticOil",
    "SepiaOil",
    "SilverOil",
    "TealOil",
    "VerdantOil",
    "VioletOil",
];

thread_local! {
    /// `recipeImages` (module local), indexed like [`RECIPE_NAMES`].
    static RECIPE_IMAGES: RefCell<Vec<Option<ImageHandle>>> = RefCell::new(vec![None; RECIPE_NAMES.len()]);
    /// The `Load`s AddSeparator could not run under `&Cx` (CCR-0073 A), oldest first. One queue
    /// for every tooltip, so the loads keep Lua's relative order across tooltips (a parent's and
    /// its child tooltips' separators, built before either is drawn).
    static PENDING_LOADS: RefCell<Vec<(ImageHandle, String)>> = const { RefCell::new(Vec::new()) };
}

/// Runs the image loads AddSeparator queued (CCR-0073 A), oldest first, for all tooltips.
/// Every `real_draw` calls it first; a caller with `&mut Cx` may call it earlier.
pub fn flush_pending_loads(cx: &mut Cx) {
    let pending = PENDING_LOADS.with(|q| std::mem::take(&mut *q.borrow_mut()));
    for (h, path) in pending {
        load_image(cx, &h, &path, ImageFlags::empty());
    }
}

fn recipe_path(name: &str) -> String {
    format!("TreeData/{name}.png")
}

/// `imgHandle:Load(path, flags...)` plus the `ImageLoad` side effect when `Sg` did not record one
/// (CCR-0028 item 2; the oracle's bridge does the same for the Lua side).
fn load_image(cx: &mut Cx, h: &ImageHandle, path: &str, flags: ImageFlags) {
    let before = cx.sg.trace.as_ref().map_or(0, |t| t.cur.effects.len());
    h.load(&mut cx.sg, path, flags);
    if let Some(t) = &mut cx.sg.trace {
        if t.cur.effects.len() == before {
            t.effect(SideEffect::ImageLoad(ImgRef {
                path: path.to_owned(),
                flags,
            }));
        }
    }
}

/// Preload all recipe images: the module-level code that runs when Lua first loads the class
/// (the first `new("Tooltip")`). See the module notes.
/// @pob Classes/Tooltip.lua:36-41 h=d7bf4688
pub fn module_load(cx: &mut Cx) {
    // L37 local recipeImages = { }
    // L38 ORDER: (c) `pairs` over the array part of recipeNames visits 1..14.
    for (i, recipe_name) in RECIPE_NAMES.iter().enumerate() {
        // L39
        let h = cx.sg.new_image_handle();
        // L40
        load_image(cx, &h, &recipe_path(recipe_name), ImageFlags::CLAMP);
        RECIPE_IMAGES.with(|r| r.borrow_mut()[i] = Some(h));
    }
}

/// `recipeImages[recipeName]`: `None` when the name is not one of [`RECIPE_NAMES`].
fn recipe_image(cx: &mut Cx, recipe_name: &str) -> Option<ImageHandle> {
    let i = RECIPE_NAMES.iter().position(|n| *n == recipe_name)?;
    let path = recipe_path(recipe_name);
    let cached = RECIPE_IMAGES.with(|r| r.borrow()[i].clone());
    if let Some(h) = cached {
        let valid = h
            .tex()
            .and_then(|t| cx.sg.images.source(t))
            .is_some_and(|(p, f)| p == path && f == ImageFlags::CLAMP);
        if valid {
            return Some(h);
        }
    }
    // a fresh Sg (module_load ran against another one, or not at all): SG dedups the key
    let h = cx.sg.new_image_handle();
    h.load(&mut cx.sg, &path, ImageFlags::CLAMP);
    RECIPE_IMAGES.with(|r| r.borrow_mut()[i] = Some(h.clone()));
    Some(h)
}

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

/// AddLine background arg. Both variants are Lua strings, which Tooltip.lua loads as an image path
/// (CCR-0073 C).
#[derive(Clone)]
pub enum TtBackground {
    Image(String),
    Color(String),
}

impl TtBackground {
    fn path(&self) -> &str {
        match self {
            TtBackground::Image(s) | TtBackground::Color(s) => s,
        }
    }
}

/// modLine (reads .disabled etc. via downcast to [`TtModLine`])
pub type ModLineRef = Rc<dyn std::any::Any>;

/// The concrete type behind a [`ModLineRef`] (CCR-0073 B): `disabled` reads the model's
/// `modLine.disabled` live (ItemsTab toggles it, ItemsTab.lua:2127); `line` is the model object the
/// owner gets back from `TooltipLine::mod_line` (ItemsTab.lua:1532-1541).
pub struct TtModLine {
    pub disabled: Rc<dyn Fn() -> bool>,
    pub line: Rc<dyn std::any::Any>,
}

/// `data.modLine and data.modLine.disabled`
fn mod_line_disabled(m: &Option<ModLineRef>) -> bool {
    m.as_ref()
        .and_then(|m| m.downcast_ref::<TtModLine>())
        .is_some_and(|l| (l.disabled)())
}

/// tooltip.Draw = function(self, ...) (TreeTab.lua:1749 etc.)
pub type TooltipDrawFn =
    Rc<dyn Fn(&mut Cx, &mut Tooltip, f64, f64, Option<f64>, Option<f64>, &ViewPort)>;

/// `data.bounds = { x, y, width, height }` (Tooltip.lua:307; read by ItemsTab.lua:1532-1534).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TtBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// One `self.lines` entry (`{ size, text, block, font, center, modLine, background }`, or a
/// separator line `{ separatorImage, size, block }`).
#[derive(Clone, Default)]
pub struct TooltipLine {
    pub size: f64,
    pub text: Option<String>,
    /// 1-based index into `blocks`
    pub block: usize,
    pub font: Option<Font>,
    pub center: bool,
    pub mod_line: Option<ModLineRef>,
    pub background: Option<TtBackground>,
    /// `separatorImage`: the tooltip's handle at AddSeparator time (Lua shares the handle object).
    pub separator_image: Option<ImageHandle>,
    /// `_handled` (set by CalculateColumns on a recipe title line)
    pub handled: Cell<bool>,
    /// `bounds` (set by CalculateColumns on each drawn text line)
    pub bounds: Cell<Option<TtBounds>>,
}

/// One `self.blocks` entry (`{ height }`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TooltipBlock {
    pub height: f64,
}

/// `line[1]` of an image entry of the draw stack.
#[derive(Clone)]
pub enum TtDrawImg {
    /// `nil` (a plain divider: `DrawImage(nil, ...)`)
    Nil,
    /// `{ handle = h, isSeparator = s }`
    Handle {
        handle: ImageHandle,
        is_separator: bool,
    },
    /// `{ handle = recipeImages[name] }` (resolved at draw time, see the module notes)
    Recipe(String),
}

/// One `drawStack` entry of CalculateColumns.
#[derive(Clone)]
pub enum TtDrawEntry {
    /// `{x, y, align, size, font, text, strikethrough = .., background = ..}` (`#line == 6`)
    Text {
        x: f64,
        y: f64,
        align: Align,
        size: f64,
        font: Font,
        text: String,
        strikethrough: bool,
        background: Option<TtBackground>,
    },
    /// `{img, x, y, w, h}` (`#line == 5`)
    Image {
        img: TtDrawImg,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
}

/// One `headerConfigs` row (Tooltip.lua:411-425).
struct HeaderConfig {
    left: &'static str,
    middle: &'static str,
    right: &'static str,
    height: f64,
    side_width: f64,
    middle_width: f64,
    text_y_offset: f64,
    allow_influence_icon: bool,
}

// spell-checker: disable
#[rustfmt::skip]
const HEADER_CONFIGS: [(&str, HeaderConfig); 13] = [
    ("RELIC", HeaderConfig { left: "Assets/itemsheaderfoilleft.png", middle: "Assets/itemsheaderfoilmiddle.png", right: "Assets/itemsheaderfoilright.png", height: 54.0, side_width: 47.0, middle_width: 52.0, text_y_offset: 3.0, allow_influence_icon: true }),
    ("UNIQUE", HeaderConfig { left: "Assets/itemsheaderuniqueleft.png", middle: "Assets/itemsheaderuniquemiddle.png", right: "Assets/itemsheaderuniqueright.png", height: 54.0, side_width: 47.0, middle_width: 52.0, text_y_offset: 3.0, allow_influence_icon: true }),
    ("RARE", HeaderConfig { left: "Assets/itemsheaderrareleft.png", middle: "Assets/itemsheaderraremiddle.png", right: "Assets/itemsheaderrareright.png", height: 54.0, side_width: 47.0, middle_width: 52.0, text_y_offset: 3.0, allow_influence_icon: true }),
    ("MAGIC", HeaderConfig { left: "Assets/itemsheadermagicleft.png", middle: "Assets/itemsheadermagicmiddle.png", right: "Assets/itemsheadermagicright.png", height: 38.0, side_width: 32.0, middle_width: 32.0, text_y_offset: 6.0, allow_influence_icon: true }),
    ("NORMAL", HeaderConfig { left: "Assets/itemsheaderwhiteleft.png", middle: "Assets/itemsheaderwhitemiddle.png", right: "Assets/itemsheaderwhiteright.png", height: 38.0, side_width: 32.0, middle_width: 32.0, text_y_offset: 6.0, allow_influence_icon: true }),
    ("GEM", HeaderConfig { left: "Assets/itemsheadergemleft.png", middle: "Assets/itemsheadergemmiddle.png", right: "Assets/itemsheadergemright.png", height: 38.0, side_width: 33.0, middle_width: 38.0, text_y_offset: 5.0, allow_influence_icon: false }),
    ("JEWEL", HeaderConfig { left: "Assets/jewelpassiveheaderleft.png", middle: "Assets/jewelpassiveheadermiddle.png", right: "Assets/jewelpassiveheaderright.png", height: 38.0, side_width: 33.0, middle_width: 38.0, text_y_offset: 6.0, allow_influence_icon: false }),
    ("NOTABLE", HeaderConfig { left: "Assets/notablepassiveheaderleft.png", middle: "Assets/notablepassiveheadermiddle.png", right: "Assets/notablepassiveheaderright.png", height: 38.0, side_width: 38.0, middle_width: 38.0, text_y_offset: 6.0, allow_influence_icon: false }),
    ("PASSIVE", HeaderConfig { left: "Assets/normalpassiveheaderleft.png", middle: "Assets/normalpassiveheadermiddle.png", right: "Assets/normalpassiveheaderright.png", height: 38.0, side_width: 33.0, middle_width: 38.0, text_y_offset: 6.0, allow_influence_icon: false }),
    ("KEYSTONE", HeaderConfig { left: "Assets/keystonepassiveheaderleft.png", middle: "Assets/keystonepassiveheadermiddle.png", right: "Assets/keystonepassiveheaderright.png", height: 38.0, side_width: 33.0, middle_width: 38.0, text_y_offset: 6.0, allow_influence_icon: false }),
    ("ASCENDANCY", HeaderConfig { left: "Assets/ascendancypassiveheaderleft.png", middle: "Assets/ascendancypassiveheadermiddle.png", right: "Assets/ascendancypassiveheaderright.png", height: 38.0, side_width: 33.0, middle_width: 38.0, text_y_offset: 6.0, allow_influence_icon: false }),
    ("MASTERY", HeaderConfig { left: "Assets/masteryheaderunallocatedleft.png", middle: "Assets/masteryheaderunallocatedmiddle.png", right: "Assets/masteryheaderunallocatedright.png", height: 38.0, side_width: 33.0, middle_width: 38.0, text_y_offset: 6.0, allow_influence_icon: false }),
    ("MASTERYALLOC", HeaderConfig { left: "Assets/masteryheaderallocatedleft.png", middle: "Assets/masteryheaderallocatedmiddle.png", right: "Assets/masteryheaderallocatedright.png", height: 38.0, side_width: 33.0, middle_width: 38.0, text_y_offset: 6.0, allow_influence_icon: false }),
];

/// `headerInfluence` (Tooltip.lua:394-410)
#[rustfmt::skip]
const HEADER_INFLUENCE: [(&str, &str); 15] = [
    ("Fractured", "Assets/fracturedicon.png"),
    ("Veiled", "Assets/veiledicon.png"),
    ("Shaper", "Assets/shapericon.png"),
    ("Elder", "Assets/eldericon.png"),
    ("Redeemer", "Assets/redeemericon.png"),
    ("Hunter", "Assets/huntericon.png"),
    ("Crusader", "Assets/crusadericon.png"),
    ("Warlord", "Assets/warlordicon.png"),
    ("Eater", "Assets/eatericon.png"),
    ("Exarch", "Assets/exarchicon.png"),
    ("Synthesis", "Assets/synthesisicon.png"),
    ("Experimented", "Assets/experimentedicon.png"),
    ("Foulborn", "Assets/breachicon.png"),
    ("Memory", "Assets/memoryicon.png"),
    ("Vestigial", "Assets/vestigialicon.png"),
];
// spell-checker: enable

/// `foilTypes` (Tooltip.lua:514-528)
#[rustfmt::skip]
const FOIL_TYPES: [(&str, [f64; 3]); 13] = [
    ("Rainbow", [0.6, 1.0, 0.5]),
    ("Amethyst", [0.9, 0.6, 1.0]),
    ("Verdant", [0.5, 1.0, 0.5]),
    ("Ruby", [1.0, 0.5, 0.6]),
    ("Cobalt", [0.6, 0.7, 1.0]),
    ("Sunset", [1.0, 1.0, 0.6]),
    ("Aureate", [1.0, 0.85, 0.2]),
    ("Celestial Quartz", [1.0, 0.7, 0.85]),
    ("Celestial Ruby", [0.8, 0.3, 0.2]),
    ("Celestial Emerald", [0.2, 0.6, 0.3]),
    ("Celestial Aureate", [0.8, 0.7, 0.2]),
    ("Celestial Pearl", [1.0, 0.85, 0.9]),
    ("Celestial Amethyst", [0.5, 0.6, 1.0]),
];

/// `headerConfigs[rarity] or headerConfigs.NORMAL`
fn header_config(rarity: &str) -> &'static HeaderConfig {
    let row = HEADER_CONFIGS
        .iter()
        .find(|(k, _)| *k == rarity)
        .or_else(|| HEADER_CONFIGS.iter().find(|(k, _)| *k == "NORMAL"))
        .expect("NORMAL row");
    &row.1
}

/// `headerInfluence[name]` (nil for an unknown name)
fn header_influence(name: &Option<String>) -> Option<&'static str> {
    let name = name.as_deref()?;
    HEADER_INFLUENCE
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| *v)
}

/// `SetDrawColor(self.color)` / `SetDrawColor(unpack(self.color))`
fn set_color(cx: &mut Cx, color: &Option<TtColor>) {
    match color {
        Some(TtColor::Esc(s)) => cx.sg.set_draw_color_esc(s),
        Some(TtColor::Rgb([r, g, b])) => cx.sg.set_draw_color(*r, *g, *b),
        None => lua_error!("bad argument #1 to 'unpack' (table expected, got nil)"),
    }
}

/// `rn:sub(-3) == "Oil"` with `#rn > 3`: `rn:sub(1, #rn - 3)` (byte semantics).
fn trim_oil(r: &str) -> String {
    let mut rn = r.to_owned();
    if rn.len() > 3 && pob_lua::string::sub(&rn, -3, None) == "Oil" {
        rn = pob_lua::string::sub(&rn, 1, Some(rn.len() as i64 - 3)).into_owned();
    }
    rn
}

/// `main:WrapString(str, height, width)` (U14; CCR-0073 D).
pub use crate::wrap::wrap_string;

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
    /// `self.childTooltips`
    pub child_tooltips: Option<Vec<Tooltip>>,
    /// `self.influenceHeader1/2` (ItemsTab.lua:3994-4061; not reset by Clear)
    pub influence_header1: Option<String>,
    pub influence_header2: Option<String>,
    /// `self.foilType` (ItemsTab.lua:4221)
    pub foil_type: Option<String>,
    /// `self.separatorImage`, `self.separatorImagePath` (AddSeparator's cache)
    pub separator_image: Option<ImageHandle>,
    pub separator_image_path: Option<String>,
    /// `self.headerLeft/Middle/Right` and their paths (Draw's cache)
    pub header_left: Option<ImageHandle>,
    pub header_left_path: Option<String>,
    pub header_middle: Option<ImageHandle>,
    pub header_middle_path: Option<String>,
    pub header_right: Option<ImageHandle>,
    pub header_right_path: Option<String>,
    /// `self.influenceIcon1/2` (re-created by every Draw with an influence header)
    pub influence_icon1: Option<ImageHandle>,
    pub influence_icon2: Option<ImageHandle>,
    /// `self._bgHandles`
    pub bg_handles: Option<HashMap<String, ImageHandle>>,
}

impl Default for Tooltip {
    /// `new("Tooltip"):Tooltip()`
    /// @pob Classes/Tooltip.lua:43-52 h=aaf1af93
    fn default() -> Self {
        let mut t = Tooltip {
            // L47
            lines: Vec::new(),
            // L48
            blocks: Vec::new(),
            update_params: None,
            update_nsmt: None,
            tooltip_header: None,
            title_y_offset: 0.0,
            recipe: None,
            center: false,
            max_width: None,
            color: None,
            draw_override: None,
            // L49
            child_tooltips: None,
            influence_header1: None,
            influence_header2: None,
            foil_type: None,
            separator_image: None,
            separator_image_path: None,
            header_left: None,
            header_left_path: None,
            header_middle: None,
            header_middle_path: None,
            header_right: None,
            header_right_path: None,
            influence_icon1: None,
            influence_icon2: None,
            bg_handles: None,
        };
        // L50
        t.clear(false);
        t
    }
}

impl Tooltip {
    /// lua: Tooltip.lua:54-69
    /// @pob Classes/Tooltip.lua:54-69 h=f19c7d20
    pub fn clear(&mut self, clear_update_params: bool) {
        // L55
        self.lines.clear();
        // L56
        self.blocks.clear();
        // L57
        if self.update_params.is_some() && clear_update_params {
            // L58 wipeTable also drops updateParams.notSupportedModTooltips
            self.update_params = Some(Vec::new());
            self.update_nsmt = None;
        }
        // L61
        self.tooltip_header = None;
        // L62
        self.title_y_offset = 0.0;
        // L63
        self.recipe = None;
        // L64
        self.center = false;
        // L65
        self.max_width = None;
        // L67
        self.color = Some(TtColor::Rgb([0.5, 0.3, 0.0]));
        // L68
        self.blocks.push(TooltipBlock { height: 0.0 });
    }

    /// lua: Tooltip.lua:71-89 (also main.notSupportedModTooltips). Lua returns `true` or nothing.
    /// @pob Classes/Tooltip.lua:71-89 h=a507b3fb
    pub fn check_for_update(&mut self, cx: &Cx, params: &[TtKey]) -> bool {
        // L72
        let mut do_update = false;
        // L73
        let up = self.update_params.get_or_insert_with(Vec::new);
        // L77
        for (i0, temp) in params.iter().enumerate() {
            // L79
            if up.get(i0).unwrap_or(&TtKey::Nil) != temp {
                // L80
                if up.len() <= i0 {
                    up.resize(i0 + 1, TtKey::Nil);
                }
                up[i0] = temp.clone();
                // L81
                do_update = true;
            }
        }
        // L84
        if do_update || self.update_nsmt != Some(cx.main.not_supported_mod_tooltips) {
            // L85
            self.update_nsmt = Some(cx.main.not_supported_mod_tooltips);
            // L86
            self.clear(false);
            // L87
            return true;
        }
        false
    }

    /// lua: Tooltip.lua:91-114 (`AddLine(size, text)`)
    pub fn add_line(&mut self, cx: &Cx, size: f64, text: &str) {
        self.add_line_ex(cx, size, text, None, None, None);
    }

    /// lua: Tooltip.lua:91-114 (`AddLine(size, text, font, modLine, background)`)
    /// @pob Classes/Tooltip.lua:91-114 h=fde1b8b9
    pub fn add_line_ex(
        &mut self,
        cx: &Cx,
        size: f64,
        text: &str,
        font: Option<Font>,
        mod_line: Option<ModLineRef>,
        background: Option<TtBackground>,
    ) {
        // L92 if text then (a nil text is not representable: callers skip the call)
        // L94
        let font_to_use = if cx.main.show_flavour_text {
            // L95
            font.unwrap_or(Font::Var)
        } else {
            // L97
            Font::Var
        };
        // L99 for line in s_gmatch(text .. "\n", "([^\n]*)\n")
        for line in text.split('\n') {
            // L100 `line:match("^.*(Equipping)") == "Equipping"` is a substring test
            if line.contains("Equipping") || line.contains("Removing") {
                // L101
                self.blocks.push(TooltipBlock { height: size + 2.0 });
            } else {
                // L103
                let Some(last) = self.blocks.last_mut() else {
                    lua_error!("attempt to index a nil value");
                };
                last.height = last.height + size + 2.0;
            }
            let block = self.blocks.len();
            // L105
            if let Some(max_width) = self.max_width {
                // L106
                for wrapped_line in wrap_string(cx, line, size, max_width - H_PAD) {
                    // CCR-0073
                    // L107
                    self.lines.push(TooltipLine {
                        size,
                        text: Some(wrapped_line),
                        block,
                        font: Some(font_to_use),
                        center: self.center,
                        mod_line: mod_line.clone(),
                        background: background.clone(),
                        ..TooltipLine::default()
                    });
                }
            } else {
                // L110
                self.lines.push(TooltipLine {
                    size,
                    text: Some(line.to_owned()),
                    block,
                    font: Some(font_to_use),
                    center: self.center,
                    mod_line: mod_line.clone(),
                    background: background.clone(),
                    ..TooltipLine::default()
                });
            }
        }
    }

    /// lua: Tooltip.lua:116-118
    /// @pob Classes/Tooltip.lua:116-118 h=ace24d71
    pub fn set_recipe(&mut self, recipe: Option<Vec<String>>) {
        // L117
        self.recipe = recipe;
    }

    /// lua: Tooltip.lua:120-160. The separator image's Load is queued until the next draw
    /// (CCR-0073 A).
    /// @pob Classes/Tooltip.lua:120-160 h=e413c67e
    pub fn add_separator(&mut self, cx: &Cx, size: Option<f64>) {
        let _ = cx;
        // L121
        let size = size.unwrap_or(10.0);
        // L123
        let last_line = self.lines.last();
        // L124
        if last_line.is_some_and(|l| l.separator_image.is_some()) {
            // L126 Prevent back-to-back separator lines
            return;
        }
        let last_block = last_line.map(|l| l.block);
        // L129
        let mut separator_image = None;
        // L131
        if let Some(header) = &self.tooltip_header {
            // L132
            let rarity = pob_lua::string::upper(header);
            // L134-141 separatorConfigs; L143
            let separator_path = match rarity.as_str() {
                "RELIC" => "Assets/itemsseparatorfoil.png",
                "UNIQUE" => "Assets/itemsseparatorunique.png",
                "RARE" => "Assets/itemsseparatorrare.png",
                "MAGIC" => "Assets/itemsseparatormagic.png",
                "GEM" => "Assets/itemsseparatorgem.png",
                _ => "Assets/itemsseparatorwhite.png", // NORMAL, and `or separatorConfigs.NORMAL`
            };
            // L145
            if self.separator_image.is_none()
                || self.separator_image_path.as_deref() != Some(separator_path)
            {
                // L146
                let h = ImageHandle::default(); // NewImageHandle()
                // L147 CCR-0073 A: loaded by the next draw
                PENDING_LOADS.with(|q| q.borrow_mut().push((h.clone(), separator_path.to_owned())));
                self.separator_image = Some(h);
                // L148
                self.separator_image_path = Some(separator_path.to_owned());
            }
            // L151
            separator_image = self.separator_image.clone();
        }
        // L154
        let last_block = last_block.unwrap_or(1);
        // L155
        self.lines.push(TooltipLine {
            separator_image,
            size,
            block: last_block,
            ..TooltipLine::default()
        });
    }

    /// The image loads AddSeparator queued (CCR-0073 A), in order: [`flush_pending_loads`] (the
    /// queue is shared by all tooltips). `real_draw` calls it first.
    pub fn flush_loads(&mut self, cx: &mut Cx) {
        flush_pending_loads(cx);
    }

    /// lua: Tooltip.lua:163-192
    /// @pob Classes/Tooltip.lua:163-192 h=4c72391e
    pub fn get_size(&self, cx: &Cx) -> (f64, f64) {
        // L164
        let mut tt_w = 0.0;
        let mut tt_h = 0.0;
        // L165
        for (i0, data) in self.lines.iter().enumerate() {
            let prev = i0.checked_sub(1).and_then(|p| self.lines.get(p));
            let next = self.lines.get(i0 + 1);
            // L166
            if data.text.is_some()
                || (prev.is_some() && next.is_some_and(|n| n.text.is_some()))
            {
                // L167
                tt_h = tt_h + data.size + 2.0;
            }
            // L169
            if let Some(text) = &data.text {
                // L170
                tt_w = m_max(tt_w, cx.sg.draw_string_width(data.size, line_font(data), text));
            }
        }

        // L175 Account for recipe display
        if let (Some(recipe), Some(title)) = (&self.recipe, self.lines.first()) {
            // L177
            let font = if cx.main.show_flavour_text {
                Font::FontinSc
            } else {
                Font::Var
            };
            // L178
            let mut image_x =
                cx.sg.draw_string_width(title.size, font, title_text(title, 3)) + title.size;
            // L179
            let recipe_text_size = (title.size * 3.0) / 4.0;
            // L180
            for recipe_name in recipe {
                // L181 Trim "Oil" from the recipe name, which normally looks like "GoldenOil"
                // L182-185
                let recipe_name_short = trim_oil(recipe_name);
                // L186
                image_x = image_x
                    + cx
                        .sg
                        .draw_string_width(recipe_text_size, font, &recipe_name_short)
                    + title.size * 1.25;
            }
            // L188
            tt_w = m_max(tt_w, image_x);
        }

        // L191
        (tt_w + H_PAD, tt_h + V_PAD)
    }

    /// lua: Tooltip.lua:194-203
    /// @pob Classes/Tooltip.lua:194-203 h=7d54e243
    pub fn get_dynamic_size(&self, cx: &Cx, vp: &ViewPort) -> (f64, f64) {
        // L195
        let (static_tt_w, static_tt_h) = self.get_size(cx);
        // L196
        let (columns, tt_h, _, extra_column_width) =
            self.calculate_columns(cx, 0.0, 0.0, static_tt_h, static_tt_w, vp);

        // L199 ensure extra column width has sensible value
        let extra_column_width = if columns > 1.0 && extra_column_width > 0.0 {
            extra_column_width
        } else {
            static_tt_w
        };
        // L200
        let tt_w = static_tt_w + (m_max(columns - 1.0, 0.0) * extra_column_width);

        // L202
        (tt_w + H_PAD, tt_h + V_PAD)
    }

    /// Calculates the column breaks, layout heights, and individual rendering instructions for
    /// tooltip lines. By default, items exceeding window height will wrap to a new column.
    /// Returns `(columns, maxColumnHeight, drawStack, extraColumnWidth)`.
    /// @pob Classes/Tooltip.lua:205-372 h=ee6fdea3
    pub fn calculate_columns(
        &self,
        cx: &Cx,
        tt_y: f64,
        tt_x: f64,
        tt_h: f64,
        tt_w: f64,
        vp: &ViewPort,
    ) -> (f64, f64, Vec<TtDrawEntry>, f64) {
        let flavour = cx.main.show_flavour_text;
        // L217
        let mut y = tt_y + 2.0 * BORDER_WIDTH;
        // L218 (titleYOffset is always a number)
        y = y + self.title_y_offset;
        // L221
        let mut x = tt_x;
        // L222 reset to count columns by block heights
        let mut columns = 1.0;
        // L223
        let mut current_block: usize = 1;
        // L224
        let mut extra_column_width = 0.0;
        // L225
        let mut max_column_height = 0.0;
        // L226
        let mut draw_stack: Vec<TtDrawEntry> = Vec::new();

        // L229
        for (i0, data) in self.lines.iter().enumerate() {
            // L230 Handle first line with recipe/oils
            // L231
            let font = if flavour {
                // L232
                data.font.unwrap_or(Font::Var)
            } else {
                // L234
                Font::Var
            };
            // L236
            if let (Some(recipe), 0, Some(title_text)) = (&self.recipe, i0, &data.text) {
                // L238
                let title_size = data.size;
                // L239
                let recipe_text_size = (title_size * 3.0 / 4.0).floor();
                // L240
                let padding = 4.0;

                // L243 Measure total width for centering
                let mut total_width = cx.sg.draw_string_width(title_size, font, title_text);
                // L244
                let mut oil_widths: Vec<(String, String, f64, f64)> = Vec::new();
                // L245
                for r in recipe {
                    // L246-249
                    let rn = trim_oil(r);
                    // L250
                    let text_w = cx.sg.draw_string_width(recipe_text_size, font, &rn);
                    // L251
                    let icon_w = title_size;
                    // L252
                    oil_widths.push((rn, r.clone(), text_w, icon_w));
                    // L253
                    total_width = total_width + text_w + icon_w + padding;
                }

                // L257 Center title + oils
                let mut cur_x = tt_x + tt_w / 2.0 - total_width / 2.0;
                // L259 Draw title
                draw_stack.push(TtDrawEntry::Text {
                    x: cur_x,
                    y: y + (title_size - title_size) / 2.0,
                    align: Align::Left,
                    size: title_size,
                    font,
                    text: title_text.clone(),
                    strikethrough: false,
                    background: None,
                });
                // L260
                cur_x = cur_x + cx.sg.draw_string_width(title_size, font, title_text) + (H_PAD / 2.0);

                // L263 Draw oils
                let max_oil_height = 0.0;
                // L264
                for (rn, recipe_name, text_w, icon_w) in oil_widths {
                    // L266
                    let rn = if flavour {
                        // L267
                        format!("^xF8E6CA{rn}")
                    } else {
                        rn
                    };
                    // L269
                    draw_stack.push(TtDrawEntry::Text {
                        x: cur_x,
                        y: y + (title_size - recipe_text_size) / 2.0,
                        align: Align::Left,
                        size: recipe_text_size,
                        font,
                        text: rn,
                        strikethrough: false,
                        background: None,
                    });
                    // L270
                    cur_x = cur_x + text_w;

                    // L272-273
                    draw_stack.push(TtDrawEntry::Image {
                        img: TtDrawImg::Recipe(recipe_name),
                        x: cur_x,
                        y,
                        w: icon_w,
                        h: icon_w,
                    });

                    // L275
                    cur_x = cur_x + icon_w + padding;
                }
                // L278 Advance y by max height
                y = y + m_max(title_size, max_oil_height) + 2.0;

                // L281 Mark line handled so it won't print again
                data.handled.set(true);
            }

            // L285 Normal text handling (skip if first line handled)
            if let (Some(text), false) = (&data.text, data.handled.get()) {
                // L287 Column break logic
                if current_block != data.block
                    && block_height(&self.blocks, data.block) + y > tt_y + m_min(tt_h, vp.height)
                {
                    // L288
                    y = tt_y + 2.0 * BORDER_WIDTH;
                    // L289
                    x = tt_x + tt_w * columns;
                    // L290
                    columns = columns + 1.0;
                }
                // L292
                current_block = data.block;

                // L294-297 (line.center is always a boolean)
                let line_centered = data.center;
                // L298
                let line_x = if line_centered {
                    x + tt_w / 2.0
                } else {
                    x + (H_PAD / 2.0)
                };
                // L299
                let line_align = if line_centered {
                    Align::CenterX
                } else {
                    Align::Left
                };

                // L301-306
                draw_stack.push(TtDrawEntry::Text {
                    x: line_x,
                    y,
                    align: line_align,
                    size: data.size,
                    font,
                    text: text.clone(),
                    strikethrough: mod_line_disabled(&data.mod_line),
                    background: data.background.clone(),
                });
                // L307
                data.bounds.set(Some(TtBounds {
                    x: x + (H_PAD / 2.0),
                    y,
                    width: tt_w - H_PAD,
                    height: data.size + 2.0,
                }));
                // L308
                y = y + data.size + 2.0;

                // L311 track max width for extra columns
                if columns > 1.0 {
                    // L312
                    extra_column_width = m_max(
                        extra_column_width,
                        cx.sg.draw_string_width(data.size, font, text) + H_PAD,
                    );
                }
            } else if let (Some(sep), true) = (&data.separator_image, flavour) {
                // L316 (`data.size or 10`: size is always set)
                let sep_size = data.size;
                // L317
                if current_block != data.block && y + sep_size > tt_y + m_min(tt_h, vp.height) {
                    // L318
                    y = tt_y + 2.0 * BORDER_WIDTH;
                    // L319
                    x = tt_x + tt_w * columns;
                    // L320
                    columns = columns + 1.0;
                }
                // L322
                current_block = data.block;
                // L323
                draw_stack.push(TtDrawEntry::Image {
                    img: TtDrawImg::Handle {
                        handle: sep.clone(),
                        is_separator: true,
                    },
                    x: x + (H_PAD / 2.0),
                    y,
                    w: tt_w - H_PAD,
                    h: sep_size,
                });
                // L324
                y = y + sep_size + 2.0;
            } else if i0 > 0
                && self
                    .lines
                    .get(i0 + 1)
                    .is_some_and(|n| n.text.is_some())
            {
                // L326 (self.lines[i - 1] exists exactly when i > 1)
                // L327
                draw_stack.push(TtDrawEntry::Image {
                    img: TtDrawImg::Nil,
                    x,
                    y: y - 1.0 + data.size / 2.0,
                    w: tt_w - BORDER_WIDTH,
                    h: 2.0,
                });
                // L328
                y = y + data.size + 2.0;
            }

            // L331
            max_column_height = m_max(y - tt_y + 2.0 * BORDER_WIDTH, max_column_height);
        }

        // L334 Resizing/Shrinking drawStack elements in extra columns
        // L336
        if columns > 1.0 && extra_column_width > 0.0 {
            // L337
            for line in &mut draw_stack {
                // L338-340
                let orig_x = match line {
                    TtDrawEntry::Text { x, .. } | TtDrawEntry::Image { x, .. } => *x,
                };

                // L343 calculate column index (origX is at least x * original widths from start)
                let col_index = ((orig_x - tt_x) / tt_w).floor() + 1.0;

                // L345
                if col_index > 1.0 {
                    // L346
                    let old_base_x = tt_x + tt_w * (col_index - 1.0);
                    // L347 `- 2` because first column is unchanged
                    let new_base_x = tt_x + tt_w + extra_column_width * (col_index - 2.0);

                    match line {
                        // L350 centered texts
                        TtDrawEntry::Text {
                            x,
                            align: Align::CenterX,
                            ..
                        } => {
                            // L352
                            *x = new_base_x + extra_column_width / 2.0;
                        }
                        // L354 "LEFT" aligned text (NOTE: "RIGHT" aligned does not seem to exist)
                        TtDrawEntry::Text { x, .. } => {
                            // L355
                            *x = orig_x - old_base_x + new_base_x;
                        }
                        TtDrawEntry::Image { img, x, w, .. } => {
                            // L355
                            *x = orig_x - old_base_x + new_base_x;
                            // L358-366 Resize separators/dividers
                            if matches!(
                                img,
                                TtDrawImg::Handle {
                                    is_separator: true,
                                    ..
                                }
                            ) {
                                // L362 "fancy" separators get extra padding
                                *w = extra_column_width - H_PAD;
                            } else {
                                // L364
                                *w = extra_column_width - BORDER_WIDTH;
                            }
                        }
                    }
                }
            }
        }

        // L371
        (columns, max_column_height, draw_stack, extra_column_width)
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

    /// Draws tooltip to screen. `w`/`h`: optional size of the UI element being hovered over; the
    /// tooltip positions itself outside this box (if possible). Lua returns `ttW, ttH`, which no
    /// caller reads (TreeTab's overrides only forward it to TooltipHost, which drops it).
    /// lua: Tooltip.lua:379-698
    /// @pob Classes/Tooltip.lua:373-698 h=dc10326c
    pub fn real_draw(
        &mut self,
        cx: &mut Cx,
        x: f64,
        y: f64,
        w: Option<f64>,
        h: Option<f64>,
        vp: &ViewPort,
    ) {
        // CCR-0073 A: AddSeparator's loads
        self.flush_loads(cx);
        let flavour = cx.main.show_flavour_text;
        // L380
        if self.lines.is_empty() {
            // L381
            return;
        }
        // L383
        let (mut tt_w, tt_h) = self.get_size(cx);

        // L386 ensure ttW is at least title width + 50 pixels, this fixes the header image for
        // Magic items and some Tree passives.
        if let (Some(_), Some(text)) = (&self.tooltip_header, &self.lines[0].text) {
            // L387
            let font = if flavour { Font::FontinSc } else { Font::Var };
            // L388
            let title_w = cx.sg.draw_string_width(self.lines[0].size, font, text);
            // L389
            if title_w + 50.0 > tt_w {
                // L390
                tt_w = title_w + 50.0;
            }
        }
        // L394-426 headerInfluence, headerConfigs: HEADER_INFLUENCE, HEADER_CONFIGS
        // L428
        if let (Some(header), true, Some(_)) =
            (&self.tooltip_header, flavour, &self.lines[0].text)
        {
            // L429
            let rarity = pob_lua::string::upper(header);
            // L430
            let config = header_config(&rarity);
            // L431
            self.title_y_offset = config.text_y_offset;
        }
        // L433
        let mut tt_x = x;
        // L434
        let mut tt_y = y;
        // L435 `w` and `h` typically only provided for hover tooltips
        let hover = w.zip(h);
        // L436
        if let Some((w, h)) = hover {
            // L437
            tt_x = tt_x + w + 5.0;
            // L438
            if tt_x + tt_w > vp.x + vp.width {
                // L439
                tt_x = m_max(vp.x, x - 5.0 - tt_w);
                // L440
                if tt_x + tt_w > x {
                    // L441
                    tt_y = tt_y + h;
                }
            }
            // L444
            if tt_y + tt_h > vp.y + vp.height {
                // L445
                tt_y = m_max(vp.y, y + h - tt_h);
            }
        }
        // L449 Initial column calculation
        let (columns, max_column_height, mut draw_stack, extra_column_width) =
            self.calculate_columns(cx, tt_y, tt_x, tt_h, tt_w, vp);

        // L452 ensure extraColumnWidth has sensible value and calculate new total width
        let extra_column_width = if columns > 1.0 && extra_column_width > 0.0 {
            extra_column_width
        } else {
            tt_w
        };
        // L453
        let total_draw_width = tt_w + (m_max(columns - 1.0, 0.0) * extra_column_width);

        // L456 If hover tooltip and extra columns don't fit, shift to left and adjust drawStack
        // (because hover tooltips can't scroll)
        if columns > 1.0 && hover.is_some() && total_draw_width + tt_x >= vp.x + vp.width {
            // L457
            let new_x = m_max(vp.x, vp.x + vp.width - total_draw_width);
            // L458
            let offset_x = new_x - tt_x;
            // L459
            tt_x = new_x;

            // L461
            for line in &mut draw_stack {
                // L462-468
                match line {
                    TtDrawEntry::Image { x, .. } | TtDrawEntry::Text { x, .. } => {
                        *x = *x + offset_x;
                    }
                }
            }
        }

        // L472 background shading currently must be drawn before text lines.  API change will
        // allow something like the commented lines below
        // L473
        cx.sg.set_draw_color_a(0.0, 0.0, 0.0, 0.85);
        // L474 --SetDrawLayer(nil, GetDrawLayer() - 5)
        // L475
        cx.sg.draw_image(
            None,
            tt_x,
            tt_y + BORDER_WIDTH,
            total_draw_width - BORDER_WIDTH,
            max_column_height - 2.0 * BORDER_WIDTH,
        );
        // L476 --SetDrawLayer(nil, GetDrawLayer())
        // L477
        cx.sg.set_draw_color(1.0, 1.0, 1.0);

        // L479 Item header (drawn within borders)
        // L480
        if let (Some(header), true, Some(_)) =
            (self.tooltip_header.clone(), flavour, &self.lines[0].text)
        {
            self.draw_header(cx, &header, tt_x, tt_y, tt_w);
        }

        // L576 Draw lines and images
        // L577
        let mut first_separator_skipped = false;
        // L578
        for line in &draw_stack {
            match line {
                // L579
                TtDrawEntry::Image { img, x, y, w, h } => {
                    // L580
                    let mut skip = false;
                    // L581
                    if let TtDrawImg::Handle {
                        is_separator: true,
                        ..
                    } = img
                    {
                        // L582 Only skip first separator for items and skill gems
                        // L583
                        let tooltip_type = self
                            .tooltip_header
                            .as_deref()
                            .map(pob_lua::string::upper)
                            .unwrap_or_default();
                        // L584
                        if flavour
                            && !first_separator_skipped
                            && matches!(
                                tooltip_type.as_str(),
                                "RELIC" | "UNIQUE" | "RARE" | "MAGIC" | "GEM"
                            )
                        {
                            // L586
                            first_separator_skipped = true;
                            // L587
                            skip = true;
                        } else {
                            // L589
                            cx.sg.set_draw_color(1.0, 1.0, 1.0);
                        }
                    } else {
                        // L591-595
                        set_color(cx, &self.color);
                    }
                    // L596
                    if !skip {
                        // L597
                        let handle = match img {
                            TtDrawImg::Handle { handle, .. } => Some(handle.clone()),
                            TtDrawImg::Recipe(name) => recipe_image(cx, name),
                            TtDrawImg::Nil => None,
                        };
                        if let Some(handle) = handle {
                            // L598-601 (line[1] has no array part: nothing is appended)
                            // L602
                            cx.sg.set_draw_color(1.0, 1.0, 1.0);
                            // L603
                            cx.sg.draw_image(Some(&handle), *x, *y, *w, *h);
                        } else if let TtDrawImg::Recipe(_) = img {
                            // L605 DrawImage(unpack(line)) with line[1] = { handle = nil }
                            lua_error!(
                                "DrawImage() argument 1: expected image handle or nil, got table"
                            );
                        } else {
                            // L605
                            cx.sg.draw_image(None, *x, *y, *w, *h);
                        }
                    }
                }
                TtDrawEntry::Text {
                    x,
                    y,
                    align,
                    size,
                    font,
                    text,
                    strikethrough,
                    background,
                } => {
                    // L609 Draw background if specified, used for gem mod lines and desecrated
                    // mods on items.
                    // L610-611
                    if let (Some(bg), true) = (background, flavour) {
                        // L612 Save current draw color BEFORE drawing background image, otherwise
                        // wrapped strings print white text for later lines.
                        // L613
                        let [prev_r, prev_g, prev_b, prev_a] = cx.sg.get_draw_color();

                        // L615 (every background is a string, CCR-0073 C)
                        let key = bg.path();
                        // L616
                        if self.bg_handles.is_none() {
                            // L617
                            self.bg_handles = Some(HashMap::new());
                        }
                        // L619
                        let cached = self.bg_handles.as_ref().and_then(|m| m.get(key)).cloned();
                        let bg = if let Some(h) = cached {
                            h
                        } else {
                            // L620
                            let h = cx.sg.new_image_handle();
                            // L621
                            load_image(cx, &h, key, ImageFlags::empty());
                            // L622
                            if let Some(m) = self.bg_handles.as_mut() {
                                m.insert(key.to_owned(), h.clone());
                            }
                            h
                        };

                        // L627
                        let x = tt_x;
                        // L628
                        let y = *y - 5.0;
                        // L629
                        let width = tt_w - 8.0;
                        // L630
                        let height = *size + 10.0;
                        // L631
                        cx.sg.set_draw_color_a(1.0, 1.0, 1.0, 1.0);
                        // L632
                        cx.sg.draw_image(Some(&bg), x + 4.0, y, width, height);

                        // L634 Restore color BEFORE DrawString
                        // L635
                        cx.sg.set_draw_color_a(
                            f64::from(prev_r),
                            f64::from(prev_g),
                            f64::from(prev_b),
                            f64::from(prev_a),
                        );
                    }

                    // L638 Draw text line
                    // L639
                    cx.sg.draw_string(*x, *y, *align, *size, *font, text);
                    // L640
                    if *strikethrough {
                        // L641-646
                        let (text_x, text_y) = (*x, *y);
                        // L647
                        let text_w = cx.sg.draw_string_width(*size, *font, text);
                        // L648
                        let strike_x = if *align == Align::CenterX {
                            text_x - text_w / 2.0
                        } else {
                            text_x
                        };
                        // L649
                        let strike_y = text_y + *size / 2.0;
                        // L650
                        cx.sg.set_draw_color_a(0.75, 0.75, 0.75, 0.35);
                        // L651
                        cx.sg.draw_image(None, strike_x, strike_y, text_w, 1.0);
                    }
                }
            }
        }

        // L656 Draw borders
        // L657-661
        set_color(cx, &self.color);

        // L663 draw vertical borders, accounting for separate extra column width
        // L664 (`for i = 0, columns`: columns is an integer-valued count)
        let mut i = 0.0;
        while i <= columns {
            // L665
            let extra_col_x_offset = if i > 0.0 {
                tt_w + ((i - 1.0) * extra_column_width)
            } else {
                0.0
            };
            // L666
            let current_x = tt_x + extra_col_x_offset;
            // L667
            cx.sg.draw_image(
                None,
                current_x - BORDER_WIDTH * (lpow(i, 2.0) / (lpow(i, 2.0) + 1.0)).ceil(),
                tt_y,
                BORDER_WIDTH,
                max_column_height,
            );
            i += 1.0;
        }
        // L669 draw horizontal borders
        // L670 top
        cx.sg
            .draw_image(None, tt_x, tt_y, total_draw_width, BORDER_WIDTH);
        // L671 bottom
        cx.sg.draw_image(
            None,
            tt_x,
            tt_y + max_column_height - BORDER_WIDTH,
            total_draw_width,
            BORDER_WIDTH,
        );

        // L673 draw child tooltips for item skills. these are placed directly to the right of the
        // main tooltip, growing downwards, unless they would go outside the viewport, in which
        // case they will draw over the main tooltip
        // L676
        if let Some(mut children) = self.child_tooltips.take() {
            // L677
            let mut total_h = 0.0;
            // L678 we will move the tooltips up as a group, so get the total height
            // L679
            for tt in &children {
                // L680
                let (_, child_h) = tt.get_dynamic_size(cx, vp);
                // L681
                total_h = total_h + child_h;
            }
            // L683 if the whole group would go over the bottom edge, we apply a negative offset
            // to keep them in
            // L685
            let y_offset = m_min(0.0, vp.height - total_h - tt_y);
            // L686 movement to the left happens individually. i.e. the right edges are aligned
            // L687
            let mut y_pos = m_max(tt_y + y_offset, vp.y);
            // L688
            for tt in &mut children {
                // L689
                let (child_w, child_h) = tt.get_size(cx);
                // L690
                let furthest_allowed_x = vp.width + vp.x - child_w;
                // L691
                tt.draw(
                    cx,
                    m_min(tt_x + tt_w + 4.0, furthest_allowed_x),
                    y_pos,
                    None,
                    None,
                    vp,
                );
                // L693 next tooltip goes below this one
                // L694
                y_pos = y_pos + child_h + 6.0;
            }
            self.child_tooltips = Some(children);
        }
        // L697 return ttW, ttH
    }

    /// The item header block of Draw (drawn within borders).
    /// lua: Tooltip.lua:480-574 (part of 373-698's marker)
    fn draw_header(&mut self, cx: &mut Cx, header: &str, tt_x: f64, tt_y: f64, tt_w: f64) {
        // L481
        let rarity = pob_lua::string::upper(header);
        // L482
        let config = header_config(&rarity);

        // L484
        self.title_y_offset = config.text_y_offset;

        // L486
        if self.header_left.is_none() || self.header_left_path.as_deref() != Some(config.left) {
            // L487
            let h = cx.sg.new_image_handle();
            self.header_left = Some(h.clone());
            // L488
            load_image(cx, &h, config.left, ImageFlags::empty());
            // L489
            self.header_left_path = Some(config.left.to_owned());
            // L490
            let h = cx.sg.new_image_handle();
            self.header_middle = Some(h.clone());
            // L491
            load_image(cx, &h, config.middle, ImageFlags::empty());
            // L492
            self.header_middle_path = Some(config.middle.to_owned());
            // L493
            let h = cx.sg.new_image_handle();
            self.header_right = Some(h.clone());
            // L494
            load_image(cx, &h, config.right, ImageFlags::empty());
            // L495
            self.header_right_path = Some(config.right.to_owned());
        }

        // L498
        let header_height = config.height;
        // L499
        let header_side_width = config.side_width;
        // L500
        let header_middle_width = config.middle_width;

        // L502
        let header_x = tt_x + BORDER_WIDTH;
        // L503
        let header_y = tt_y + BORDER_WIDTH;
        // L504
        let header_total_width = tt_w - 2.0 * BORDER_WIDTH;
        // L505
        let header_middle_area_width = m_max(0.0, header_total_width - 2.0 * header_side_width);

        // L507
        if self.influence_header1.is_some() {
            // L508
            let h = cx.sg.new_image_handle();
            self.influence_icon1 = Some(h.clone());
            // L509
            let Some(p) = header_influence(&self.influence_header1) else {
                lua_error!("imgHandle:Load() argument 1: expected string, got nil");
            };
            load_image(cx, &h, p, ImageFlags::empty());
            // L510
            let h = cx.sg.new_image_handle();
            self.influence_icon2 = Some(h.clone());
            // L511
            let Some(p) = header_influence(&self.influence_header2) else {
                lua_error!("imgHandle:Load() argument 1: expected string, got nil");
            };
            load_image(cx, &h, p, ImageFlags::empty());
        }

        // L514-528 foilTypes: FOIL_TYPES
        let foil_color = || {
            let name = self.foil_type.as_deref();
            FOIL_TYPES
                .iter()
                .find(|(k, _)| Some(*k) == name)
                .or_else(|| FOIL_TYPES.iter().find(|(k, _)| *k == "Rainbow"))
                .map(|(_, c)| *c)
        };
        // L529 (an exact compare: not upper-cased)
        if header == "RELIC" {
            // L530 --ConPrintf(self.foilType)
            // L531
            let color = foil_color();
            // L532
            if let Some(color) = color {
                // L533
                cx.sg.set_draw_color(color[0], color[1], color[2]);
            } else {
                // L535 fallback to green
                cx.sg.set_draw_color(0.6, 1.0, 0.5);
            }
        }
        // L538 Draw left cap first, then influence icon on top
        // L539
        cx.sg.draw_image(
            self.header_left.as_ref(),
            header_x,
            header_y,
            header_side_width,
            header_height,
        );
        // L540
        if self.influence_header1.is_some() && config.allow_influence_icon {
            // L541
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
            // L542
            cx.sg.draw_image(
                self.influence_icon1.as_ref(),
                header_x + 2.0,
                header_y + (header_height - (header_height / 2.0)) / 2.0,
                header_height / 2.0,
                header_height / 2.0,
            );
        }

        // L545
        if header == "RELIC" {
            // L546 --ConPrintf(self.foilType)
            // L547
            let color = foil_color();
            // L548
            if let Some(color) = color {
                // L549
                cx.sg.set_draw_color(color[0], color[1], color[2]);
            } else {
                // L551 fallback to green
                cx.sg.set_draw_color(0.6, 1.0, 0.5);
            }
        }
        // L554 Draw middle fill
        // L555
        if header_middle_area_width > 0.0 {
            // L556
            let mut draw_x = header_x + header_side_width;
            // L557
            let end_x = header_x + header_total_width - header_side_width;
            // L558
            while draw_x + header_middle_width <= end_x {
                // L559
                cx.sg.draw_image(
                    self.header_middle.as_ref(),
                    draw_x,
                    header_y,
                    header_middle_width,
                    header_height,
                );
                // L560
                draw_x = draw_x + header_middle_width;
            }
            // L562
            let remaining_width = end_x - draw_x;
            // L563
            if remaining_width > 0.0 {
                // L564
                cx.sg.draw_image(
                    self.header_middle.as_ref(),
                    draw_x,
                    header_y,
                    remaining_width,
                    header_height,
                );
            }
        }

        // L568 Draw right cap
        // L569
        cx.sg.draw_image(
            self.header_right.as_ref(),
            header_x + header_total_width - header_side_width,
            header_y,
            header_side_width,
            header_height,
        );
        // L570
        if self.influence_header2.is_some() && config.allow_influence_icon {
            // L571
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
            // L572
            cx.sg.draw_image(
                self.influence_icon2.as_ref(),
                header_x + header_total_width - (header_height / 2.0) - 2.0,
                header_y + (header_height - (header_height / 2.0)) / 2.0,
                header_height / 2.0,
                header_height / 2.0,
            );
        }
    }
}

/// `self.blocks[data.block].height` (a missing block is Lua's index-of-nil error)
fn block_height(blocks: &[TooltipBlock], block: usize) -> f64 {
    match block.checked_sub(1).and_then(|i| blocks.get(i)) {
        Some(b) => b.height,
        None => lua_error!("attempt to index a nil value"),
    }
}

/// `data.font` of a text line in GetSize (`DrawStringWidth(size, nil, ..)` is SG's argument error)
fn line_font(data: &TooltipLine) -> Font {
    data.font
        .unwrap_or_else(|| lua_error!("DrawStringWidth() argument 2: expected string, got nil"))
}

/// `title.text` passed to DrawStringWidth (a separator first line has none)
fn title_text(title: &TooltipLine, arg: usize) -> &str {
    match &title.text {
        Some(t) => t,
        None => lua_error!("DrawStringWidth() argument {arg}: expected string, got nil"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_tooltip_is_cleared() {
        let t = Tooltip::default();
        assert_eq!(t.blocks, vec![TooltipBlock { height: 0.0 }]);
        assert!(matches!(t.color, Some(TtColor::Rgb([0.5, 0.3, 0.0]))));
        assert!(t.lines.is_empty() && t.update_params.is_none());
    }

    #[test]
    fn check_for_update_memo() {
        let mut cx = Cx::headless();
        let mut t = Tooltip::default();
        let k = |n: f64| [TtKey::Num(n), TtKey::Nil, TtKey::Str("a".into())];
        assert!(t.check_for_update(&cx, &k(1.0)));
        t.add_line(&cx, 14.0, "x");
        assert!(!t.check_for_update(&cx, &k(1.0)));
        assert_eq!(t.lines.len(), 1);
        assert!(t.check_for_update(&cx, &k(2.0)));
        assert!(t.lines.is_empty());
        // main.notSupportedModTooltips also forces an update
        cx.main.not_supported_mod_tooltips = !cx.main.not_supported_mod_tooltips;
        assert!(t.check_for_update(&cx, &k(2.0)));
        assert!(!t.check_for_update(&cx, &k(2.0)));
        // NaN ~= NaN
        assert!(t.check_for_update(&cx, &[TtKey::Num(f64::NAN)]));
        assert!(t.check_for_update(&cx, &[TtKey::Num(f64::NAN)]));
        // Clear(true) wipes the params, including notSupportedModTooltips
        t.clear(true);
        assert_eq!(t.update_params.as_deref(), Some(&[][..]));
        assert!(t.update_nsmt.is_none());
        assert!(t.check_for_update(&cx, &[]));
    }

    #[test]
    fn add_line_blocks_and_separators() {
        let cx = Cx::headless();
        PENDING_LOADS.with(|q| q.borrow_mut().clear());
        let mut t = Tooltip::default();
        t.add_separator(&cx, None); // no header: no image
        t.add_line(&cx, 16.0, "a\nEquipping this\n\nb");
        assert_eq!(t.lines.len(), 5);
        assert_eq!(t.blocks.len(), 2);
        assert_eq!(t.blocks[0].height, 18.0);
        assert_eq!(t.blocks[1].height, 54.0);
        assert_eq!(
            t.lines.iter().map(|l| l.block).collect::<Vec<_>>(),
            [1, 1, 2, 2, 2]
        );
        t.tooltip_header = Some("unique".into());
        t.add_separator(&cx, Some(8.0));
        t.add_separator(&cx, Some(8.0)); // back-to-back: dropped
        assert_eq!(t.lines.len(), 6);
        assert_eq!(t.lines[5].block, 2);
        assert_eq!(
            t.separator_image_path.as_deref(),
            Some("Assets/itemsseparatorunique.png")
        );
        assert_eq!(PENDING_LOADS.with(|q| q.borrow().len()), 1);
        t.lines.pop();
        t.tooltip_header = Some("NOPE".into());
        t.add_separator(&cx, None);
        assert_eq!(
            t.separator_image_path.as_deref(),
            Some("Assets/itemsseparatorwhite.png")
        );
        assert_eq!(PENDING_LOADS.with(|q| q.borrow().len()), 2);
    }

    #[test]
    fn trims_oil_and_header_fallback() {
        assert_eq!(trim_oil("GoldenOil"), "Golden");
        assert_eq!(trim_oil("Oil"), "Oil");
        assert_eq!(trim_oil("Soil"), "Soil");
        assert_eq!(trim_oil("XOil"), "X");
        assert_eq!(
            header_config("FOO").left,
            "Assets/itemsheaderwhiteleft.png"
        );
        assert_eq!(header_config("GEM").side_width, 33.0);
    }
}
