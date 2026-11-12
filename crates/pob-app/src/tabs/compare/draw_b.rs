//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/CompareTab.lua:4049-5036 (DrawSkills, calcs tooltip/header, DrawCalcs, DrawConfig).
//! Owner: U50.
//!
//! @pob-file Classes/CompareTab.lua @16de4b82d
//!
//! The Skills, Calcs and Config sub-view passes of the Compare tab. U47's `Draw` reaches them
//! through `layout::CompareDrawHooks` (CCR-0137 §3); `default_draw_hooks` installs
//! [`CompareTab::DrawSkills`], [`CompareTab::DrawCalcs`] and [`CompareTab::DrawConfig`].
//!
//! # The fields this file reads and writes
//!
//! * U46's `init::State`: `scrollY`, `skillsScrollX` and `skillsTotalContentHeight` (DrawSkills
//!   writes the last two), `calcsShowOnlyDifferences`, `calcsTooltip` (lent to U48's
//!   `DrawCalcsTooltip` for the call).
//! * U47's `layout::State`: `calcsSkillHeaderHeight` and `configSectionLayout` (read).
//! * This file's [`State`]: `self.calcsSkillHeaderHover` (DrawCalcsSkillHeader writes it,
//!   DrawCalcs reads it; like the Lua field it keeps its value across frames and views until the
//!   next header pass resets it) and `self.calcSections` (CompareTab.lua:192 `LoadModule(
//!   "Modules/CalcSections")`, U39's `sections_data::load`: fresh tables per tab, loaded on the
//!   first DrawCalcs; the module is pure data, so when it is loaded is not observable).
//!
//! # The comparison build's `skillsTab.controls.groupList` (CCR-0144 §1)
//!
//! `getGroupSlotIcon` (L4330-4336) calls `skillsTab.controls.groupList:GetRowIcon(1, idx,
//! group)` for both builds. The primary build's is U30/U31's SkillListControl (its `GetRowIcon`
//! hook is called through the ListControl hook table). In PoB the CompareEntry has its own
//! SkillsTab with its own SkillListControl (CompareEntry.lua:115) whose `GetRowIcon`
//! (SkillListControl.lua:241-267) reads the entry's `itemsTab`; the port's entry has the model
//! half only (CCR-0137 §2), and U30's body and `slot_map` icons are private. Workaround (marked
//! `// CCR-0144`): [`entry_row_icon`] repeats SkillListControl.lua:241-267 on the entry's model
//! (no `@pob` marker, the range stays U30's) with this file's own handles for `slot_map`'s 17
//! paths (loaded on first use; PoB reloads the shared handles in every SkillListControl
//! constructor, so its ImageLoad effects come at the entry's construction instead).
//!
//! # Lua semantics
//!
//! * The synthesized imbued-support entries (L4060-4081) and the socket groups' gems are both
//!   "gems" of the display lists ([`DGem`]); `imbuedGemCache[group]`, `highlightSet[gem]` and
//!   `entry.group == hoveredEntry.group` are identity tests (`Rc::ptr_eq`).
//! * `gem.level and (" " .. gem.level)`: a nil level is NaN in the model (contracts.md "Nil in
//!   typed fields"); a number concatenates as `tostring`.
//! * `pcall(formatCalcStr, ...)`: a Lua error raised inside (a non-string `format`, a bad
//!   output path) gives `ok = false` ([`pcall`]).
//! * `tostring(row.value)` of a table value is its address in PoB; the port prints the table's
//!   address too (`table: 0x…`), so equal keys come from the same table, as in PoB.
//! * `control.shown == false or (type(control.shown) == "function" and not control:IsShown())`
//!   reads the raw field (as U47's `layoutRow`).
//! * `primaryEnv.player and primaryEnv.player.output`: an env always has its player and the
//!   player its output table in the model, so both are truthy.
//! * `rowData.label`, `subSec.label` concatenated: a nil one is Lua's concatenation error.
//!
//! ORDER: `pairs(setA)`/`pairs(setB)`/`pairs(allKeys)` (L4106-4109) only count the union and
//! the intersection, and `pairs(cSet)`/`pairs(pSet)` (L4274-4279) fill lists that are sorted
//! right after (distinct names; `table.sort` of strings): order-independent, (c).
//! `pairs(highlightSet)` (L4452) counts. `table.sort(scorePairs, ...)` (L4141) is Lua 5.1's
//! `auxsort` (`pob_lua::sort`, not stable): pairs tied on the score keep the order PoB gives
//! them (the nested `pi`/`ci` insertion order is the same).
//!
//! Props with side effects (ui doc §7.3): none.

#![allow(non_snake_case)]
// literal Lua statement shapes (`for i = 1, #list` index loops, `local x = cache or build()`)
#![allow(clippy::needless_range_loop, clippy::single_match_else)]

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexSet;
use pob_lua::math::{m_floor, m_max, m_min};
use pob_lua::{n, tostring_num, Name};
use pob_sg::{Align, Font, ImageFlags, ImageHandle};
use pob_ui_core::controls::list::{ListControl, ListValue};
use pob_ui_core::controls::ScrollBarControl;
use pob_ui_core::prelude::*;

use pob_calc::actor::ActorRef;
use pob_calc::modules::calc_format::formatCalcStr;
use crate::model::gamedata::{GemDataRef, GrantedEffectRef};

use super::calcs_helpers as calcsHelpers;
use super::init::{ctl, st, st_mut, wrapInfoLine, CompareEntryRef, LAYOUT};
use super::CompareTab;
use crate::app::CxApp;
use crate::model::{self, GemInstanceRef, SocketGroupRef, Table, Val};
use crate::tabs::calcs::core::check_flag;
use crate::tabs::calcs::sections_data::{self, SectionDef};

// ---- this unit's fields ------------------------------------------------------------------------

/// `self.calcsSkillHeaderHover` (L4581, L4609-4614, L4619-4624).
#[derive(Clone)]
pub struct SkillHeaderHover {
    pub breakdown: &'static str,
    pub label: &'static str,
    /// `self.primaryBuild` or the CompareEntry (their models; both carry `calcsTab`)
    pub build: model::BuildRef,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// This unit's part of the CompareTab struct (ui doc §12.1; module notes).
#[derive(Default)]
pub struct State {
    /// `self.calcsSkillHeaderHover`
    pub calcsSkillHeaderHover: Option<SkillHeaderHover>,
    /// `self.calcSections` (L192; module notes)
    pub calcSections: Option<Rc<Vec<SectionDef>>>,
}

fn bst(cx: &Cx, me: CtrlId) -> &State {
    &cx.w::<CompareTab>(me).draw_b
}

fn bst_mut(cx: &mut Cx, me: CtrlId) -> &mut State {
    &mut cx.w_mut::<CompareTab>(me).draw_b
}

// ---- helpers (Rust-only) ----------------------------------------------------------------------

/// `pcall(f)`: `Err(message)` for a Lua error raised inside (module notes).
fn pcall<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        pob_lua::error::catch(f).map_err(|e| e.0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        Ok(f())
    }
}

/// `a .. b` operand: a string, or a number via `tostring` (anything else raises).
#[track_caller]
fn concat(v: &Val, what: &str) -> String {
    match v {
        Val::Str(s) => s.to_string(),
        Val::Num(x) => tostring_num(*x),
        other => lua_error!("attempt to concatenate {what} (a {} value)", other.lua_type()),
    }
}

/// Lua `tostring(v)` (module notes: tables print their address).
fn tostring_val(v: &Val) -> String {
    match v {
        Val::Nil => "nil".to_owned(),
        Val::Bool(b) => b.to_string(),
        Val::Num(x) => tostring_num(*x),
        Val::Str(s) => s.to_string(),
        Val::Table(t) => format!("table: {:p}", t.as_ptr()),
        Val::Data(_) | Val::Mod(_) => "table".to_owned(),
        Val::Obj(o) => format!("userdata: {:p}", Rc::as_ptr(o)),
    }
}

/// `SetDrawColor(v)` for a colour escape string value (`rowData.bgCol or "^0"`, `sec.colour`).
fn set_color_val(cx: &mut Cx, v: &Val) {
    match v {
        Val::Str(s) => cx.sg.set_draw_color_esc(s),
        Val::Nil | Val::Bool(false) => cx.sg.set_draw_color_esc("^0"),
        other => lua_error!(
            "SetDrawColor() argument 1: expected string or number, got {}",
            other.lua_type()
        ),
    }
}

/// `pcall(formatCalcStr, str, actor, colData)`: `Some(text)` when it did not raise.
fn pformat(str: &Val, actor: &ActorRef, colData: &Val) -> Option<String> {
    pcall(|| formatCalcStr(&concat(str, "local 'str'"), Some(actor), colData)).ok()
}

/// `control.shown == false or (type(control.shown) == "function" and not control:IsShown())`
fn control_hidden(cx: &Cx, control: CtrlId) -> bool {
    match &cx.base(control).shown {
        Prop::Val(v) => !*v,
        Prop::Fn(_) => !cx.is_shown(control),
    }
}

/// `self:GetShortBuildName(self.primaryBuild.buildName)`
fn primary_short_name(cx: &Cx) -> String {
    CompareTab::GetShortBuildName(cx.build().build_name.as_deref())
}

// ---- DrawSkills helpers -------------------------------------------------------------------------

/// The synthesized imbued-support entry (L4071-4078).
struct ImbuedGem {
    grantedEffect: Option<GrantedEffectRef>,
    nameSpec: String,
    level: f64,
    quality: f64,
    color: Option<String>,
}

/// A gem of a display list: a socket group's gem or a synthesized imbued support.
#[derive(Clone)]
enum DGem {
    Real(GemInstanceRef),
    Imbued(Rc<ImbuedGem>),
}

impl DGem {
    fn ptr_eq(&self, o: &DGem) -> bool {
        match (self, o) {
            (DGem::Real(a), DGem::Real(b)) => Rc::ptr_eq(a, b),
            (DGem::Imbued(a), DGem::Imbued(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
    /// `gem.grantedEffect`
    fn grantedEffect(&self) -> Option<GrantedEffectRef> {
        match self {
            DGem::Real(g) => g.grantedEffect.get(),
            DGem::Imbued(g) => g.grantedEffect.clone(),
        }
    }
    /// `gem.nameSpec`
    fn nameSpec(&self) -> String {
        match self {
            DGem::Real(g) => g.nameSpec.get().to_string(),
            DGem::Imbued(g) => g.nameSpec.clone(),
        }
    }
    /// `gem.gemData` (nil on a synthesized entry)
    fn gemData(&self) -> Option<GemDataRef> {
        match self {
            DGem::Real(g) => g.gemData.get(),
            DGem::Imbued(_) => None,
        }
    }
    /// `gem.level` (nil: `None`)
    fn level(&self) -> Option<f64> {
        let v = match self {
            DGem::Real(g) => g.level.get(),
            DGem::Imbued(g) => g.level,
        };
        (!v.is_nan()).then_some(v)
    }
    /// `gem.quality` (nil: `None`)
    fn quality(&self) -> Option<f64> {
        let v = match self {
            DGem::Real(g) => g.quality.get(),
            DGem::Imbued(g) => g.quality,
        };
        (!v.is_nan()).then_some(v)
    }
    /// `gem.color` (nil: `None`)
    fn color(&self) -> Option<String> {
        match self {
            DGem::Real(g) => {
                let c = g.extra.get(n!("color"));
                c.truthy().then(|| concat(&c, "field 'color'"))
            }
            DGem::Imbued(g) => g.color.clone(),
        }
    }
    /// `gem.isImbuedSupport`
    fn isImbuedSupport(&self) -> bool {
        matches!(self, DGem::Imbued(_))
    }
}

/// `"common"` / `"additional"` / `"missing"`
#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Common,
    Additional,
    Missing,
}

/// One display list entry `{ gem = …, name = …, status = … }`.
struct DisplayEntry {
    gem: Option<DGem>,
    name: String,
    status: Status,
}

/// One `gemEntries` entry `{ gem, x, y, group }`.
struct GemEntry {
    gem: DGem,
    x: f64,
    y: f64,
    group: Option<SocketGroupRef>,
}

/// One `scorePairs` entry.
struct ScorePair {
    pIdx: usize,
    cIdx: usize,
    score: f64,
}

/// One `renderPairs` entry.
struct RenderPair {
    pIdx: Option<usize>,
    cIdx: Option<usize>,
}

/// `imbuedGemCache` (group -> entry, by identity)
type ImbuedCache = RefCell<Vec<(SocketGroupRef, Rc<ImbuedGem>)>>;

/// The local `getImbuedGem(group, skillsTab)` (L4061-4081).
fn getImbuedGem(cx: &Cx, cache: &ImbuedCache, group: &SocketGroupRef, skillsTab: &model::SkillsTab) -> Option<DGem> {
    // L4062
    let imbuedSupport = group.extra.get(n!("imbuedSupport"));
    if !imbuedSupport.truthy() {
        return None;
    }
    // L4063
    if let Some((_, e)) = cache.borrow().iter().find(|(g, _)| Rc::ptr_eq(g, group)) {
        return Some(DGem::Imbued(e.clone()));
    }
    // Prefer the grantedEffect cached on skillsTab; fall back to a direct data lookup
    // (mirrors SkillsTab:RebuildImbuedSupportBySlot) in case the tab hasn't been primed.
    // L4066
    let mut grantedEffect: Option<GrantedEffectRef> = group
        .slot
        .get()
        .and_then(|slot| skillsTab.imbuedSupportBySlot.obj::<GrantedEffectRef>(slot))
        .map(|g| (*g).clone());
    // L4067
    if grantedEffect.is_none() {
        // L4068
        let lower = pob_lua::string::lower(&concat(&imbuedSupport, "field 'imbuedSupport'"));
        let data = model::data();
        let gemId = data.gemForBaseName.get(format!("{lower} support").as_str()).copied();
        // L4069
        grantedEffect = gemId.and_then(|id| data.gems.get(&id).map(|g| g.grantedEffect.clone()));
    }
    // L4071-4078
    let color = grantedEffect
        .as_ref()
        .and_then(|ge| crate::tabs::skills::core::skill_color_map(cx, ge.color));
    let entry = Rc::new(ImbuedGem {
        grantedEffect,
        nameSpec: concat(&imbuedSupport, "field 'imbuedSupport'"),
        level: 1.0,
        quality: 0.0,
        color,
    });
    // L4079
    cache.borrow_mut().push((group.clone(), entry.clone()));
    // L4080
    Some(DGem::Imbued(entry))
}

/// The local `getGemName(gem)` (L4211-4213); also the name of `getGemNameSet` (L4087, L4094).
fn getGemName(gem: &DGem) -> String {
    gem.grantedEffect().map_or_else(|| gem.nameSpec(), |ge| ge.name.to_string())
}

/// The local `getGemNameSet(group, skillsTab)` (L4084-4100).
fn getGemNameSet(cx: &Cx, cache: &ImbuedCache, group: &SocketGroupRef, skillsTab: &model::SkillsTab) -> IndexSet<String> {
    // L4085
    let mut set = IndexSet::new();
    // L4086-4091
    for gem in group.gemList.get().snapshot() {
        set.insert(getGemName(&DGem::Real(gem)));
    }
    // L4092-4098
    if let Some(imbuedGem) = getImbuedGem(cx, cache, group, skillsTab) {
        set.insert(getGemName(&imbuedGem));
    }
    // L4099
    set
}

/// The local `groupSimilarity(setA, setB)`: Jaccard similarity (L4103-4117; ORDER: module notes).
fn groupSimilarity(setA: &IndexSet<String>, setB: &IndexSet<String>) -> f64 {
    // L4104-4105
    let mut intersection = 0.0;
    let mut union = 0.0;
    // L4106-4108
    let mut allKeys: IndexSet<&String> = IndexSet::new();
    allKeys.extend(setA.iter());
    allKeys.extend(setB.iter());
    // L4109-4114
    for k in allKeys {
        union += 1.0;
        if setA.contains(k) && setB.contains(k) {
            intersection += 1.0;
        }
    }
    // L4115
    if union == 0.0 {
        return 0.0;
    }
    // L4116
    intersection / union
}

/// The local `checkSupporting(gemA, gemB)` (mirrors GemSelectControl:CheckSupporting;
/// L4169-4186).
fn checkSupporting(gemA: &DGem, gemB: &DGem) -> bool {
    // Synthesized imbued-support entries lack gemData/supportEffect wiring, but by definition
    // they support any active (non-support) gem in the group.
    // L4172
    if gemA.isImbuedSupport() {
        // L4173
        let bEffect = gemB.grantedEffect().or_else(|| gemB.gemData().map(|g| g.grantedEffect.clone()));
        // L4174
        return bEffect.is_some_and(|e| !e.support);
    }
    // L4176
    let (Some(aData), Some(bData)) = (gemA.gemData(), gemB.gemData()) else {
        return false;
    };
    let DGem::Real(a) = gemA else { return false };
    // `gemA.supportEffect and gemA.supportEffect.isSupporting and gemA.supportEffect.isSupporting[gemB]`
    let supporting = || match (a.supportEffect.get(), gemB) {
        (Some(se), DGem::Real(b)) => se.isSupporting.get().is_some_and(|s| s.with(|s| s.contains(b))),
        _ => false,
    };
    // L4177-4185
    (aData.grantedEffect.support && !bData.grantedEffect.support && supporting())
        || (aData.secondaryGrantedEffect.as_ref().is_some_and(|s| s.support)
            && !bData.grantedEffect.support
            && supporting())
}

/// The local `buildGemDisplayString(entry)`: the exact string drawGemList renders (L4192-4207).
fn buildGemDisplayString(cx: &Cx, entry: &DisplayEntry) -> String {
    let cc = &cx.main.cc;
    // L4193
    if entry.status == Status::Missing {
        // L4194
        format!("{}- {}^7", cc.NEGATIVE, entry.name)
    } else if let Some(gem) = &entry.gem {
        // L4196
        let gemName = gem.grantedEffect().map_or_else(|| gem.nameSpec(), |ge| ge.name.to_string());
        // L4197
        let gemColor = gem.color().unwrap_or_else(|| cc.GEM.clone());
        // L4198
        let levelStr = gem.level().map_or_else(String::new, |l| format!(" {}", tostring_num(l)));
        // L4199
        let qualStr = gem.quality().map_or_else(String::new, |q| format!("/{}", tostring_num(q)));
        // L4200-4203
        let mut prefix = String::new();
        if entry.status == Status::Additional {
            prefix = format!("{}+ ", cc.POSITIVE);
        }
        // L4204
        format!("{prefix}{gemColor}{gemName}^7{levelStr}{qualStr}")
    } else {
        // L4206
        String::new()
    }
}

/// The local `getGemsWithImbued(group, skillsTab)` (L4216-4227).
fn getGemsWithImbued(cx: &Cx, cache: &ImbuedCache, group: Option<&SocketGroupRef>, skillsTab: &model::SkillsTab) -> Vec<DGem> {
    // L4217
    let Some(group) = group else { return Vec::new() };
    // L4218-4221
    let mut gems: Vec<DGem> = group.gemList.get().snapshot().into_iter().map(DGem::Real).collect();
    // L4222-4225
    if let Some(imbuedGem) = getImbuedGem(cx, cache, group, skillsTab) {
        gems.push(imbuedGem);
    }
    // L4226
    gems
}

/// `{ x, y, w, h }` passed to the per-pair helpers.
struct Pair<'a> {
    pGroup: Option<&'a SocketGroupRef>,
    cGroup: Option<&'a SocketGroupRef>,
    pSet: &'a IndexSet<String>,
    cSet: &'a IndexSet<String>,
}

/// The local `buildAlignedGemLists(pGroup, cGroup, pSet, cSet)`: common gems first, then
/// additional, then missing (L4229-4291).
fn buildAlignedGemLists(
    cx: &Cx,
    cache: &ImbuedCache,
    p: &Pair<'_>,
    pSkillsTab: &model::SkillsTab,
    cSkillsTab: &model::SkillsTab,
) -> (Vec<DisplayEntry>, Vec<DisplayEntry>) {
    let (pSet, cSet) = (p.pSet, p.cSet);
    // L4230-4231
    let mut pDisplay = Vec::new();
    let mut cDisplay = Vec::new();

    // L4233-4234
    let pGems = getGemsWithImbued(cx, cache, p.pGroup, pSkillsTab);
    let cGems = getGemsWithImbued(cx, cache, p.cGroup, cSkillsTab);

    // Build name->gem lookup for compare side (common gems only)
    // L4237-4243
    let mut cGemByName: Vec<(String, DGem)> = Vec::new();
    for gem in &cGems {
        let name = getGemName(gem);
        if pSet.contains(&name) && !cGemByName.iter().any(|(n, _)| *n == name) {
            cGemByName.push((name, gem.clone()));
        }
    }

    // Common gems in primary build's order
    // L4246-4254
    let mut emittedCommon: IndexSet<String> = IndexSet::new();
    for gem in &pGems {
        let name = getGemName(gem);
        if cSet.contains(&name) && !emittedCommon.contains(&name) {
            emittedCommon.insert(name.clone());
            pDisplay.push(DisplayEntry {
                gem: Some(gem.clone()),
                name: name.clone(),
                status: Status::Common,
            });
            let cg = cGemByName.iter().find(|(n, _)| *n == name).map(|(_, g)| g.clone());
            cDisplay.push(DisplayEntry {
                gem: cg,
                name,
                status: Status::Common,
            });
        }
    }

    // Additional gems (unique to each side), preserving original order
    // L4257-4268
    for gem in &pGems {
        let name = getGemName(gem);
        if !cSet.contains(&name) {
            pDisplay.push(DisplayEntry {
                gem: Some(gem.clone()),
                name,
                status: Status::Additional,
            });
        }
    }
    for gem in &cGems {
        let name = getGemName(gem);
        if !pSet.contains(&name) {
            cDisplay.push(DisplayEntry {
                gem: Some(gem.clone()),
                name,
                status: Status::Additional,
            });
        }
    }

    // Missing gems (sorted alphabetically)
    // L4271
    if p.pGroup.is_some() && p.cGroup.is_some() {
        // L4272-4279 (ORDER: module notes)
        let mut pMissing: Vec<String> = cSet.iter().filter(|n| !pSet.contains(*n)).cloned().collect();
        let mut cMissing: Vec<String> = pSet.iter().filter(|n| !cSet.contains(*n)).cloned().collect();
        // L4280-4281
        pob_lua::sort::sort(&mut pMissing, |a, b| a.as_bytes() < b.as_bytes());
        pob_lua::sort::sort(&mut cMissing, |a, b| a.as_bytes() < b.as_bytes());
        // L4282-4287
        for name in pMissing {
            pDisplay.push(DisplayEntry {
                gem: None,
                name,
                status: Status::Missing,
            });
        }
        for name in cMissing {
            cDisplay.push(DisplayEntry {
                gem: None,
                name,
                status: Status::Missing,
            });
        }
    }

    // L4290
    (pDisplay, cDisplay)
}

/// The local `collectGemEntries(gemEntries, displayList, xOffset, startY, group)`: gem positions
/// for hit-testing (L4294-4303).
fn collectGemEntries(
    gemEntries: &mut Vec<GemEntry>,
    displayList: &[DisplayEntry],
    xOffset: f64,
    startY: f64,
    group: Option<&SocketGroupRef>,
    gemLineHeight: f64,
) -> f64 {
    // L4295
    let mut y = startY;
    // L4296-4301
    for entry in displayList {
        if let Some(gem) = &entry.gem {
            gemEntries.push(GemEntry {
                gem: gem.clone(),
                x: xOffset,
                y,
                group: group.cloned(),
            });
        }
        y += gemLineHeight;
    }
    // L4302
    y
}

/// `highlightSet[gem]`
fn in_set(set: &[DGem], gem: &DGem) -> bool {
    set.iter().any(|g| g.ptr_eq(gem))
}

/// The local `drawGemList(displayList, xOffset, startY, highlightSet, gemTextWidth)`
/// (L4306-4320).
fn drawGemList(
    cx: &mut Cx,
    displayList: &[DisplayEntry],
    xOffset: f64,
    startY: f64,
    highlightSet: &[DGem],
    gemTextWidth: f64,
    gemFontSize: f64,
    gemLineHeight: f64,
) -> f64 {
    // L4307
    let mut y = startY;
    // L4308
    for entry in displayList {
        // L4309
        if entry.gem.as_ref().is_some_and(|g| in_set(highlightSet, g)) {
            // L4310-4311
            cx.sg.set_draw_color_a(0.33, 1.0, 0.33, 0.25);
            cx.sg.draw_image(None, xOffset, y, gemTextWidth, gemLineHeight);
        }
        // L4313
        let line = buildGemDisplayString(cx, entry);
        // L4314
        if !line.is_empty() {
            // L4315
            cx.sg.draw_string(xOffset, y, Align::Left, gemFontSize, Font::Var, &line);
        }
        // L4317
        y += gemLineHeight;
    }
    // L4319
    y
}

/// The local `getGroupLabel(group, idx)` (L4323-4329).
fn getGroupLabel(group: &SocketGroupRef, idx: usize) -> String {
    // L4324
    let mut groupLabel = match group.displayLabel.get() {
        Some(l) => l.to_string(),
        None => model::skills_tab::socketGroup_label(group).map_or_else(|| format!("Group {idx}"), |l| l.to_string()),
    };
    // L4325
    if let Some(slot) = group.slot.get() {
        // L4326
        groupLabel = format!("{groupLabel} ({slot})");
    }
    // L4328
    groupLabel
}

/// Which build a `skillsTab` argument is.
#[derive(Clone, Copy)]
enum Side<'a> {
    Primary,
    Compare(&'a model::BuildRef),
}

/// The local `getGroupSlotIcon(skillsTab, idx, group)` (L4330-4336).
fn getGroupSlotIcon(cx: &mut Cx, side: Side<'_>, idx: usize, group: &SocketGroupRef) -> Option<ImageHandle> {
    match side {
        Side::Primary => {
            // L4331
            let groupList = crate::tabs::skills::core::tab(cx).and_then(|t| cx.child(t, "groupList"))?;
            // L4332
            let w = cx.w::<ListControl>(groupList);
            let f: Rc<pob_ui_core::controls::list::GetRowIconFn> = match (&w.ov.get_row_icon, w.hooks.get_row_icon) {
                (Some(f), _) => f.clone(),
                (None, Some(f)) => Rc::new(f),
                (None, None) => return None,
            };
            // L4335
            f(cx, groupList, 1, idx, &ListValue::of(group.clone()))
        }
        // CCR-0144: the entry's SkillListControl (module notes)
        Side::Compare(m) => entry_row_icon(cx, m, group),
    }
}

/// `slot_map` (SkillListControl.lua:9-27): slot name -> icon path (CCR-0144, module notes).
const SLOT_MAP: [(&str, &str); 17] = [
    ("Weapon 1", "Assets/icon_weapon.png"),
    ("Weapon 2", "Assets/icon_weapon_2.png"),
    ("Weapon 1 Swap", "Assets/icon_weapon_swap.png"),
    ("Weapon 2 Swap", "Assets/icon_weapon_2_swap.png"),
    ("Bow", "Assets/icon_bow.png"),
    ("Quiver", "Assets/icon_quiver.png"),
    ("Shield", "Assets/icon_shield.png"),
    ("Shield Swap", "Assets/icon_shield_swap.png"),
    ("Helmet", "Assets/icon_helmet.png"),
    ("Body Armour", "Assets/icon_body_armour.png"),
    ("Gloves", "Assets/icon_gloves.png"),
    ("Boots", "Assets/icon_boots.png"),
    ("Amulet", "Assets/icon_amulet.png"),
    ("Ring 1", "Assets/icon_ring_left.png"),
    ("Ring 2", "Assets/icon_ring_right.png"),
    ("Ring 3", "Assets/icon_ring_right.png"),
    ("Belt", "Assets/icon_belt.png"),
];

thread_local! {
    /// This file's `slot_map[k].icon` handles (CCR-0144), indexed like [`SLOT_MAP`].
    static SLOT_ICONS: RefCell<Option<Vec<ImageHandle>>> = const { RefCell::new(None) };
}

/// `slot_map[slot] and slot_map[slot].icon` (CCR-0144; the handles are loaded on first use).
fn slot_icon(cx: &mut Cx, slot: &str) -> Option<ImageHandle> {
    let i = SLOT_MAP.iter().position(|(k, _)| *k == slot)?;
    let icons = match SLOT_ICONS.with(|s| s.borrow().clone()) {
        Some(v) => v,
        None => {
            let v: Vec<ImageHandle> = SLOT_MAP.iter().map(|_| cx.sg.new_image_handle()).collect();
            for (h, (_, path)) in v.iter().zip(SLOT_MAP.iter()) {
                h.load(&mut cx.sg, path, ImageFlags::empty());
            }
            SLOT_ICONS.with(|s| *s.borrow_mut() = Some(v.clone()));
            v
        }
    };
    Some(icons[i].clone())
}

/// CCR-0144: SkillListControl.lua:241-267 `GetRowIcon(1, index, socketGroup)` of the comparison
/// build's group list (its `self.skillsTab.build.itemsTab` is the entry's), statement for
/// statement as U30's port.
fn entry_row_icon(cx: &mut Cx, m: &model::BuildRef, socketGroup: &SocketGroupRef) -> Option<ImageHandle> {
    // L243
    let mut slot = socketGroup.slot.get().map(|s| s.to_string());
    // L244
    let set = m
        .itemsTab
        .activeItemSet
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'activeItemSet' (a nil value)"));
    let sel = |name: &str| -> Val {
        let v = set
            .table(name)
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
            .get(n!("selItemId"));
        if v.truthy() {
            v
        } else {
            Val::Num(0.0)
        }
    };
    let base_type = |id: &Val| -> String {
        let item = id
            .opt_num()
            .and_then(model::items_tab::id_key)
            .and_then(|k| m.itemsTab.items.with(|items| items.get(&k).cloned()));
        match item {
            Some(item) => item
                .base
                .get()
                .unwrap_or_else(|| lua_error!("attempt to index field 'base' (a nil value)"))
                .r#type
                .to_string(),
            None => "None".to_owned(),
        }
    };
    // L245-252
    let weapon1Type = base_type(&sel("Weapon 1"));
    let weapon1SwapType = base_type(&sel("Weapon 1 Swap"));
    let weapon2Type = base_type(&sel("Weapon 2"));
    let weapon2SwapType = base_type(&sel("Weapon 2 Swap"));
    // L253-264
    if slot.as_deref() == Some("Weapon 1") && weapon1Type == "Bow" {
        slot = Some(weapon1Type);
    }
    if slot.as_deref() == Some("Weapon 1 Swap") && weapon1SwapType == "Bow" {
        slot = Some(format!("{weapon1SwapType} Swap"));
    }
    if slot.as_deref() == Some("Weapon 2") && (weapon2Type == "Quiver" || weapon2Type == "Shield") {
        slot = Some(weapon2Type);
    }
    if slot.as_deref() == Some("Weapon 2 Swap") && (weapon2SwapType == "Quiver" || weapon2SwapType == "Shield") {
        slot = Some(format!("{weapon2SwapType} Swap"));
    }
    // L265
    slot.as_deref().and_then(|s| slot_icon(cx, s))
}

// ---- DrawCalcs helpers ---------------------------------------------------------------------------

/// lua: CompareTab.lua:4639-4675 `local function calcRowMatchesBetweenBuilds(self, colData,
/// primaryActor, compareActor, compareEntry)`
/// @pob Classes/CompareTab.lua:4639-4675 h=6fa43164
fn calcRowMatchesBetweenBuilds(
    primaryBuild: &model::BuildRef,
    colData: &Table,
    primaryActor: &ActorRef,
    compareActor: &ActorRef,
    compareEntry: &CompareEntryRef,
) -> bool {
    // L4640
    let format = colData.get(n!("format"));
    if !format.truthy() {
        return false;
    }
    let cd = Val::Table(colData.clone());
    // L4641-4642
    let primaryFormatted = pformat(&format, primaryActor, &cd);
    let compareFormatted = pformat(&format, compareActor, &cd);
    // L4643
    let (Some(p), Some(c)) = (primaryFormatted, compareFormatted) else {
        return false;
    };
    if p != c {
        // L4644
        return false;
    }
    // L4646
    for (_, sectionData) in colData.ipairs() {
        let Some(sectionData) = sectionData.table() else {
            lua_error!("attempt to index local 'sectionData' (a {} value)", sectionData.lua_type())
        };
        // L4647
        if sectionData.truthy(n!("modName")) {
            // Compare mod rows to see if they match
            // L4648-4649
            let primaryRows = calcsHelpers::TabulateMods(&sectionData, primaryActor);
            let compareRows = calcsHelpers::TabulateMods(&sectionData, compareActor);
            // L4650
            if primaryRows.len() != compareRows.len() {
                return false;
            }
            // L4651
            let mut counts: indexmap::IndexMap<String, usize> = indexmap::IndexMap::new();
            // L4652-4655
            for row in &primaryRows {
                let key = format!("{}|{}", calcsHelpers::ModRowKey(row), tostring_val(&row.get(n!("value"))));
                *counts.entry(key).or_insert(0) += 1;
            }
            // L4656-4661
            for row in &compareRows {
                let key = format!("{}|{}", calcsHelpers::ModRowKey(row), tostring_val(&row.get(n!("value"))));
                let Some(&count) = counts.get(&key) else {
                    return false;
                };
                if count > 1 {
                    counts.insert(key, count - 1);
                } else {
                    counts.shift_remove(&key);
                }
            }
        }
        // L4663
        if sectionData.truthy(n!("breakdown")) {
            // Compare breakdown to see if they are the same
            // L4664-4665
            let primaryBreakdownLines = calcsHelpers::GetBreakdownLines(&sectionData, primaryBuild);
            let compareBreakdownLines = calcsHelpers::GetBreakdownLines(&sectionData, &compareEntry.model);
            // L4666-4668
            let primaryLineCount = primaryBreakdownLines.as_ref().map_or(0, Vec::len);
            let compareLineCount = compareBreakdownLines.as_ref().map_or(0, Vec::len);
            if primaryLineCount != compareLineCount {
                return false;
            }
            // L4669-4671
            for i in 0..primaryLineCount {
                if primaryBreakdownLines.as_ref().map(|l| &l[i]) != compareBreakdownLines.as_ref().map(|l| &l[i]) {
                    return false;
                }
            }
        }
    }
    // L4674
    true
}

/// lua: CompareTab.lua:4677-4682 `local function subSectionExtraMatches(subSecData,
/// primaryActor, compareActor)`
/// @pob Classes/CompareTab.lua:4677-4682 h=f0e95480
fn subSectionExtraMatches(subSecData: Option<&Table>, primaryActor: &ActorRef, compareActor: &ActorRef) -> bool {
    // L4678
    let Some(subSecData) = subSecData else { return true };
    let extra = subSecData.get(n!("extra"));
    if !extra.truthy() {
        return true;
    }
    // L4679-4680
    let primaryExtra = pformat(&extra, primaryActor, &Val::Nil);
    let compareExtra = pformat(&extra, compareActor, &Val::Nil);
    // L4681
    matches!((primaryExtra, compareExtra), (Some(p), Some(c)) if p == c)
}

/// One kept subsection `{ label, rows, data }` (L4748).
struct SubSecInfo {
    label: Val,
    rows: Vec<Table>,
    data: Option<Table>,
}

/// One visible section `{ id, group, colour, subSecs, height }` plus `drawX`/`drawY` (L4761-4765,
/// L4785-4786).
struct CalcSec {
    colour: String,
    subSecs: Vec<SubSecInfo>,
    height: f64,
    drawX: f64,
    drawY: f64,
}

impl CompareTab {
    // ============================================================
    // SKILLS VIEW
    // ============================================================

    /// lua: CompareTab.lua:4049-4491 `CompareTabClass:DrawSkills(vp, compareEntry)`
    /// @pob Classes/CompareTab.lua:4049-4491 h=74fb8eb8
    #[allow(clippy::too_many_lines)]
    pub fn DrawSkills(cx: &mut Cx, me: CtrlId, vp: &ViewPort, compareEntry: &CompareEntryRef) {
        // L4050
        let lineHeight = 18.0;

        // Get socket groups from both builds
        // L4053-4056
        let primary = cx.build().model.clone();
        let pSkillsTab = &primary.skillsTab;
        let cSkillsTab = &compareEntry.model.skillsTab;
        let pGroups: Vec<SocketGroupRef> = pSkillsTab.socketGroupList.get().snapshot();
        let cGroups: Vec<SocketGroupRef> = cSkillsTab.socketGroupList.get().snapshot();

        // Imbued supports live on socketGroup.imbuedSupport (a name string) rather than in gemList,
        // so synthesize a minimal gem-like entry that the rendering can treat like any other gem
        // L4060
        let imbuedGemCache: ImbuedCache = RefCell::new(Vec::new());

        // Build gem name sets for all groups
        // L4120-4127
        let pSets: Vec<IndexSet<String>> =
            pGroups.iter().map(|g| getGemNameSet(cx, &imbuedGemCache, g, pSkillsTab)).collect();
        let cSets: Vec<IndexSet<String>> =
            cGroups.iter().map(|g| getGemNameSet(cx, &imbuedGemCache, g, cSkillsTab)).collect();

        // Compute all pairwise similarity scores
        // L4130-4138
        let mut scorePairs: Vec<ScorePair> = Vec::new();
        for pi in 1..=pGroups.len() {
            for ci in 1..=cGroups.len() {
                let score = groupSimilarity(&pSets[pi - 1], &cSets[ci - 1]);
                if score > 0.0 {
                    scorePairs.push(ScorePair { pIdx: pi, cIdx: ci, score });
                }
            }
        }

        // Sort by similarity descending (best matches first)
        // L4141 (ORDER: module notes)
        pob_lua::sort::sort(&mut scorePairs, |a, b| a.score > b.score);

        // Greedy matching: assign best pairs first, each group used at most once
        // L4144-4153
        let mut pMatched = vec![false; pGroups.len() + 1];
        let mut cMatched = vec![false; cGroups.len() + 1];
        let mut renderPairs: Vec<RenderPair> = Vec::new();
        for sp in &scorePairs {
            if !pMatched[sp.pIdx] && !cMatched[sp.cIdx] {
                renderPairs.push(RenderPair {
                    pIdx: Some(sp.pIdx),
                    cIdx: Some(sp.cIdx),
                });
                pMatched[sp.pIdx] = true;
                cMatched[sp.cIdx] = true;
            }
        }

        // Add unmatched primary groups
        // L4156-4160
        for i in 1..=pGroups.len() {
            if !pMatched[i] {
                renderPairs.push(RenderPair { pIdx: Some(i), cIdx: None });
            }
        }
        // Add unmatched compare groups
        // L4162-4166
        for i in 1..=cGroups.len() {
            if !cMatched[i] {
                renderPairs.push(RenderPair { pIdx: None, cIdx: Some(i) });
            }
        }

        // L4188-4189
        let gemFontSize = 16.0;
        let gemLineHeight = 18.0;

        // L4337-4341
        let groupHeaderX = 10.0;
        let groupHeaderIconSize = 16.0;
        let groupHeaderIconGap = 2.0;
        let groupHeaderTextIndent = groupHeaderIconSize + groupHeaderIconGap;
        let groupHeaderTextX = groupHeaderX + groupHeaderTextIndent;
        // L4342-4344 `getGroupHeaderWidth(group, idx)`
        let getGroupHeaderWidth = |cx: &Cx, group: &SocketGroupRef, idx: usize| -> f64 {
            groupHeaderTextX + cx.sg.draw_string_width(18.0, Font::Var, &format!("^7{}", getGroupLabel(group, idx)))
        };
        // L4345-4353 `drawGroupHeader(skillsTab, group, idx, x, y)`
        let drawGroupHeader = |cx: &mut Cx, side: Side<'_>, group: &SocketGroupRef, idx: usize, x: f64, y: f64| {
            let icon = getGroupSlotIcon(cx, side, idx, group);
            let textX = x + groupHeaderTextIndent;
            if let Some(icon) = icon {
                cx.sg.set_draw_color(1.0, 1.0, 1.0);
                cx.sg.draw_image(Some(&icon), x, y, groupHeaderIconSize, groupHeaderIconSize);
            }
            cx.sg.draw_string(textX, y, Align::Left, 18.0, Font::Var, &format!("^7{}", getGroupLabel(group, idx)));
        };

        // Build display lists once and measure widest primary-side content
        // L4355-4374
        let empty: IndexSet<String> = IndexSet::new();
        let mut displayListsByPair: Vec<(Vec<DisplayEntry>, Vec<DisplayEntry>)> = Vec::new();
        let mut maxPrimaryW: f64 = 0.0;
        for pair in &renderPairs {
            let pSet = pair.pIdx.map_or(&empty, |i| &pSets[i - 1]);
            let cSet = pair.cIdx.map_or(&empty, |i| &cSets[i - 1]);
            let pGroup = pair.pIdx.map(|i| &pGroups[i - 1]);
            let cGroup = pair.cIdx.map(|i| &cGroups[i - 1]);
            let p = Pair { pGroup, cGroup, pSet, cSet };
            let (pDisplay, cDisplay) = buildAlignedGemLists(cx, &imbuedGemCache, &p, pSkillsTab, cSkillsTab);

            if let Some(pGroup) = pGroup {
                let w = getGroupHeaderWidth(cx, pGroup, pair.pIdx.expect("pGroup"));
                if w > maxPrimaryW {
                    maxPrimaryW = w;
                }
            }
            for entry in &pDisplay {
                let line = buildGemDisplayString(cx, entry);
                let w = groupHeaderTextX + cx.sg.draw_string_width(gemFontSize, Font::Var, &line);
                if w > maxPrimaryW {
                    maxPrimaryW = w;
                }
            }
            displayListsByPair.push((pDisplay, cDisplay));
        }

        // Include primary header width so the compare header has room too
        // L4377-4378
        let primaryName = format!("{}{}", cx.main.cc.POSITIVE, primary_short_name(cx));
        let primaryHeaderW = 10.0 + cx.sg.draw_string_width(18.0, Font::Var, &primaryName);
        if primaryHeaderW > maxPrimaryW {
            maxPrimaryW = primaryHeaderW;
        }

        // L4380-4384
        let colWidth = maxPrimaryW + LAYOUT::compareColGap;
        let contentWidth = colWidth * 2.0;
        let needsHScroll = contentWidth > vp.width;
        let gemTextWidth = colWidth - (groupHeaderTextX + 10.0);
        let headerReserve = LAYOUT::skillsHeaderHeight;

        // Configure horizontal scrollbar
        // L4387-4392
        let hBar = ctl(cx, me, "skillsHScrollBar");
        {
            let b = cx.base_mut(hBar);
            b.x = Prop::Val(vp.x);
            b.y = Prop::Val(vp.y + vp.height - LAYOUT::skillsHScrollBarHeight);
            b.width = Prop::Val(vp.width);
        }
        ScrollBarControl::set_content_dimension(cx, hBar, contentWidth, vp.width);
        st_mut(cx, me).skillsScrollX = cx.w::<ScrollBarControl>(hBar).offset;

        // L4394-4396
        let bottomReserve = if needsHScroll { LAYOUT::skillsHScrollBarHeight } else { 0.0 };
        let scrollViewH = vp.height - headerReserve - bottomReserve;
        let scrollOffsetX = -st(cx, me).skillsScrollX;

        // L4398-4402
        cx.sg.set_draw_color(1.0, 1.0, 1.0);
        cx.sg
            .draw_string(vp.x + scrollOffsetX + 10.0, vp.y + 4.0, Align::Left, 18.0, Font::Var, &primaryName);
        let cmpName = format!("{}{}", cx.main.cc.WARNING, compareEntry.label);
        cx.sg.draw_string(
            vp.x + scrollOffsetX + colWidth + 10.0,
            vp.y + 4.0,
            Align::Left,
            18.0,
            Font::Var,
            &cmpName,
        );
        cx.sg.set_draw_color(0.5, 0.5, 0.5);
        cx.sg.draw_image(None, vp.x + 4.0, vp.y + headerReserve, vp.width - 8.0, 2.0);

        // L4404-4405
        cx.sg.set_viewport(vp.x, vp.y + headerReserve, vp.width, scrollViewH);
        let scrollY = st(cx, me).scrollY;
        let mut drawY = 4.0 - scrollY;

        // Position pre-pass: compute gem positions for hover hit-testing
        // L4408-4423
        let mut gemEntries: Vec<GemEntry> = Vec::new();
        let mut preY = 4.0 - scrollY;
        for (i, pair) in renderPairs.iter().enumerate() {
            if i + 1 > 1 {
                preY += 2.0;
            }
            let (pDisplayList, cDisplayList) = &displayListsByPair[i];
            let pGroup = pair.pIdx.map(|i| &pGroups[i - 1]);
            let cGroup = pair.cIdx.map(|i| &cGroups[i - 1]);

            let pGemY = collectGemEntries(
                &mut gemEntries,
                pDisplayList,
                scrollOffsetX + groupHeaderTextX,
                preY + lineHeight,
                pGroup,
                gemLineHeight,
            );
            let cGemY = collectGemEntries(
                &mut gemEntries,
                cDisplayList,
                scrollOffsetX + colWidth + groupHeaderTextX,
                preY + lineHeight,
                cGroup,
                gemLineHeight,
            );

            preY = preY + m_max(pGemY - preY, cGemY - preY) + 6.0;
        }

        // Hit-test: find hovered gem
        // L4426-4438
        let (cursorX, cursorY) = cx.sys.cursor_pos();
        let localCursorX = cursorX - vp.x;
        let localCursorY = cursorY - (vp.y + headerReserve);
        let mut hoveredEntry: Option<&GemEntry> = None;
        if localCursorX >= 0.0 && localCursorX < vp.width && localCursorY >= 0.0 && localCursorY < scrollViewH {
            for entry in &gemEntries {
                if localCursorX >= entry.x
                    && localCursorX < entry.x + gemTextWidth
                    && localCursorY >= entry.y
                    && localCursorY < entry.y + gemLineHeight
                {
                    hoveredEntry = Some(entry);
                    break;
                }
            }
        }

        // Build set of highlighted gems based on hover
        // L4441-4456
        let mut highlightSet: Vec<DGem> = Vec::new();
        if let Some(hovered) = hoveredEntry {
            highlightSet.push(hovered.gem.clone());
            for entry in &gemEntries {
                let same_group = match (&entry.group, &hovered.group) {
                    (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                    (None, None) => true,
                    _ => false,
                };
                if same_group
                    && !entry.gem.ptr_eq(&hovered.gem)
                    && (checkSupporting(&hovered.gem, &entry.gem) || checkSupporting(&entry.gem, &hovered.gem))
                    && !in_set(&highlightSet, &entry.gem)
                {
                    highlightSet.push(entry.gem.clone());
                }
            }
            // L4451-4455 (ORDER: module notes)
            let count = highlightSet.len();
            if count <= 1 {
                highlightSet = Vec::new();
            }
        }

        // Draw pass
        // L4459-4486
        let cmpModel = compareEntry.model.clone();
        for (i, pair) in renderPairs.iter().enumerate() {
            if i + 1 > 1 {
                cx.sg.set_draw_color(0.3, 0.3, 0.3);
                // Divider spans the viewport width, independent of horizontal scroll
                cx.sg.draw_image(None, 0.0, drawY, vp.width, 1.0);
                drawY += 2.0;
            }

            let (pDisplayList, cDisplayList) = &displayListsByPair[i];

            let pGroup = pair.pIdx.map(|i| &pGroups[i - 1]);
            let cGroup = pair.cIdx.map(|i| &cGroups[i - 1]);

            if let Some(pGroup) = pGroup {
                drawGroupHeader(cx, Side::Primary, pGroup, pair.pIdx.expect("pGroup"), scrollOffsetX + groupHeaderX, drawY);
            }
            if let Some(cGroup) = cGroup {
                drawGroupHeader(
                    cx,
                    Side::Compare(&cmpModel),
                    cGroup,
                    pair.cIdx.expect("cGroup"),
                    scrollOffsetX + colWidth + groupHeaderX,
                    drawY,
                );
            }

            let pFinalGemY = drawGemList(
                cx,
                pDisplayList,
                scrollOffsetX + groupHeaderTextX,
                drawY + lineHeight,
                &highlightSet,
                gemTextWidth,
                gemFontSize,
                gemLineHeight,
            );
            let cFinalGemY = drawGemList(
                cx,
                cDisplayList,
                scrollOffsetX + colWidth + groupHeaderTextX,
                drawY + lineHeight,
                &highlightSet,
                gemTextWidth,
                gemFontSize,
                gemLineHeight,
            );

            drawY = drawY + m_max(pFinalGemY - drawY, cFinalGemY - drawY) + 6.0;
        }

        // L4488
        st_mut(cx, me).skillsTotalContentHeight = drawY + st(cx, me).scrollY + 36.0;

        // L4490
        cx.sg.reset_viewport();
    }

    // ============================================================
    // CALCS TOOLTIP HELPERS (delegated to CompareCalcsHelpers)
    // ============================================================

    /// lua: CompareTab.lua:4496-4502 `CompareTabClass:DrawCalcsTooltip(colData, rowLabel, rowX,
    /// rowY, rowW, rowH, vp, compareEntry)`
    /// @pob Classes/CompareTab.lua:4496-4502 h=5380ade8
    #[allow(clippy::too_many_arguments)]
    pub fn DrawCalcsTooltip(
        cx: &mut Cx,
        me: CtrlId,
        colData: &Table,
        rowLabel: Option<&str>,
        rowX: f64,
        rowY: f64,
        rowW: f64,
        rowH: f64,
        vp: &ViewPort,
        compareEntry: &CompareEntryRef,
    ) {
        // L4497
        let primaryLabel = primary_short_name(cx);
        // L4498-4501 (`self.calcsTooltip` is lent to the helper for the call)
        let primaryBuild = cx.build().model.clone();
        let mut tooltip = std::mem::take(&mut st_mut(cx, me).calcsTooltip);
        calcsHelpers::DrawCalcsTooltip(
            cx,
            &mut tooltip,
            &primaryBuild,
            &primaryLabel,
            colData,
            rowLabel,
            rowX,
            rowY,
            rowW,
            rowH,
            vp,
            compareEntry,
        );
        st_mut(cx, me).calcsTooltip = tooltip;
    }

    // ============================================================
    // CALCS VIEW (card-based sections with comparison)
    // ============================================================

    /// Draw the skill detail header area with labels for controls and text info lines
    /// lua: CompareTab.lua:4509-4637 `CompareTabClass:DrawCalcsSkillHeader(vp, compareEntry,
    /// headerHeight, primaryEnv, compareEnv)`
    /// @pob Classes/CompareTab.lua:4509-4637 h=8113f65d
    #[allow(clippy::too_many_lines)]
    pub fn DrawCalcsSkillHeader(
        cx: &mut Cx,
        me: CtrlId,
        vp: &ViewPort,
        compareEntry: &CompareEntryRef,
        headerHeight: f64,
        primaryEnv: &model::EnvRef,
        compareEnv: &model::EnvRef,
    ) {
        // L4510-4515
        let colWidth = m_floor((vp.width - 20.0) / 2.0);
        let leftX = vp.x + 4.0;
        let rightX = leftX + colWidth + 12.0;
        let rowH = 22.0;
        let mut y = vp.y + 4.0;

        // Build name headers
        // L4518-4522
        cx.sg.set_draw_color(1.0, 1.0, 1.0);
        let primaryName = format!("{}{}", cx.main.cc.POSITIVE, primary_short_name(cx));
        cx.sg.draw_string(leftX, y + 2.0, Align::Left, 18.0, Font::Var, &primaryName);
        let cmpName = format!("{}{}", cx.main.cc.WARNING, compareEntry.label);
        cx.sg.draw_string(rightX, y + 2.0, Align::Left, 18.0, Font::Var, &cmpName);
        // L4523
        y += rowH;

        // Draw labels next to each control row
        // L4526-4532
        let drawLabel = |cx: &mut Cx, label: &str, x: f64, cy: f64, control: &'static str| -> bool {
            let control = ctl(cx, me, control);
            if control_hidden(cx, control) {
                return false;
            }
            cx.sg.draw_string(x, cy + 2.0, Align::Left, 14.0, Font::Var, &format!("^7{label}:"));
            true
        };

        // L4534-4535
        let mut leftY = y;
        let mut rightY = y;

        // Socket Group
        // L4538-4541
        drawLabel(cx, "Socket Group", leftX, leftY, "primCalcsSocketGroup");
        drawLabel(cx, "Socket Group", rightX, rightY, "cmpCalcsSocketGroup");
        leftY += rowH;
        rightY += rowH;

        // L4543-4568 (Active Skill, Skill Part, Stages, Mines, Show Minion Stats, Minion, Minion Skill)
        for (label, prim, cmp) in [
            ("Active Skill", "primCalcsMainSkill", "cmpCalcsMainSkill"),
            ("Skill Part", "primCalcsSkillPart", "cmpCalcsSkillPart"),
            ("Stages", "primCalcsStageCount", "cmpCalcsStageCount"),
            ("Mines", "primCalcsMineCount", "cmpCalcsMineCount"),
            ("Show Minion Stats", "primCalcsShowMinion", "cmpCalcsShowMinion"),
            ("Minion", "primCalcsMinion", "cmpCalcsMinion"),
            ("Minion Skill", "primCalcsMinionSkill", "cmpCalcsMinionSkill"),
        ] {
            if drawLabel(cx, label, leftX, leftY, prim) {
                leftY += rowH;
            }
            if drawLabel(cx, label, rightX, rightY, cmp) {
                rightY += rowH;
            }
        }

        // Calc Mode
        // L4571-4574
        drawLabel(cx, "Calc Mode", leftX, leftY, "primCalcsMode");
        drawLabel(cx, "Calc Mode", rightX, rightY, "cmpCalcsMode");
        leftY += rowH;
        rightY += rowH;

        // Text info lines (Aura/Buffs, Combat Buffs, Curses)
        // L4577-4580 (`env.player and env.player.output`: module notes)
        let mut textY = m_max(leftY, rightY) + 2.0;
        let primaryOutput = primaryEnv.player.output.get();
        let compareOutput = compareEnv.player.output.get();
        // L4581 Reset hover state
        bst_mut(cx, me).calcsSkillHeaderHover = None;
        // L4582
        {
            // L4583-4584
            let (cursorX, cursorY) = cx.sys.cursor_pos();
            let wrapWidth = colWidth - 8.0;
            // L4585-4589
            let infoLines: [(&'static str, &'static str, Option<&'static str>); 3] = [
                ("Aura/Buff Skills", "BuffList", Some("SkillBuffs")),
                ("Combat Buffs", "CombatList", None),
                ("Curses/Debuffs", "CurseList", Some("SkillDebuffs")),
            ];
            // L4590
            for (label, key, breakdown) in infoLines {
                // L4591-4592
                let primaryValue = primaryOutput.get(Name::from(key));
                let compareValue = compareOutput.get(Name::from(key));
                let nonEmpty = |v: &Val| v.truthy() && !v.raw_eq(&Val::from(""));
                // L4593
                if nonEmpty(&primaryValue) || nonEmpty(&compareValue) {
                    // L4594-4595
                    let primaryLines = if nonEmpty(&primaryValue) {
                        let s = format!("{label}: {}", concat(&primaryValue, "local 'primaryValue'"));
                        wrapInfoLine(cx, Some(&s), wrapWidth)
                    } else {
                        Vec::new()
                    };
                    let compareLines = if nonEmpty(&compareValue) {
                        let s = format!("{label}: {}", concat(&compareValue, "local 'compareValue'"));
                        wrapInfoLine(cx, Some(&s), wrapWidth)
                    } else {
                        Vec::new()
                    };
                    // L4596-4598
                    let primaryHeight = primaryLines.len() as f64 * 18.0;
                    let compareHeight = compareLines.len() as f64 * 18.0;
                    let rowH = m_max(m_max(primaryHeight, compareHeight), 18.0);
                    // Check hover per-side for lines that have breakdown data
                    // L4600
                    if let Some(breakdown) = breakdown.filter(|_| cursorY >= textY && cursorY < textY + rowH) {
                        // L4601-4602
                        let onLeft = cursorX >= leftX
                            && cursorX < rightX
                            && primaryHeight > 0.0
                            && cursorY < textY + primaryHeight;
                        let onRight = cursorX >= rightX
                            && cursorX < vp.x + vp.width
                            && compareHeight > 0.0
                            && cursorY < textY + compareHeight;
                        // L4603
                        if onLeft {
                            // L4604-4605
                            cx.sg.set_draw_color(0.15, 0.25, 0.15);
                            cx.sg.draw_image(None, leftX, textY, colWidth, primaryHeight);
                            // L4606-4611
                            let build = cx.build().model.clone();
                            bst_mut(cx, me).calcsSkillHeaderHover = Some(SkillHeaderHover {
                                breakdown,
                                label,
                                build,
                                x: leftX,
                                y: textY,
                                w: colWidth,
                                h: primaryHeight,
                            });
                        } else if onRight {
                            // L4613-4614
                            cx.sg.set_draw_color(0.15, 0.25, 0.15);
                            cx.sg.draw_image(None, rightX, textY, colWidth, compareHeight);
                            // L4615-4620
                            bst_mut(cx, me).calcsSkillHeaderHover = Some(SkillHeaderHover {
                                breakdown,
                                label,
                                build: compareEntry.model.clone(),
                                x: rightX,
                                y: textY,
                                w: colWidth,
                                h: compareHeight,
                            });
                        }
                    }
                    // L4623-4628
                    for (i, line) in primaryLines.iter().enumerate() {
                        cx.sg.draw_string(
                            leftX,
                            textY + 1.0 + i as f64 * 18.0,
                            Align::Left,
                            14.0,
                            Font::Var,
                            &format!("^7{line}"),
                        );
                    }
                    for (i, line) in compareLines.iter().enumerate() {
                        cx.sg.draw_string(
                            rightX,
                            textY + 1.0 + i as f64 * 18.0,
                            Align::Left,
                            14.0,
                            Font::Var,
                            &format!("^7{line}"),
                        );
                    }
                    // L4629
                    textY += rowH;
                }
            }
        }

        // Separator line
        // L4635-4636
        cx.sg.set_draw_color(0.4, 0.4, 0.4);
        cx.sg.draw_image(None, vp.x + 2.0, vp.y + headerHeight - 2.0, vp.width - 4.0, 2.0);
    }

    /// lua: CompareTab.lua:4684-4931 `CompareTabClass:DrawCalcs(vp, compareEntry)`
    /// @pob Classes/CompareTab.lua:4684-4931 h=b913b525
    #[allow(clippy::too_many_lines)]
    pub fn DrawCalcs(cx: &mut Cx, me: CtrlId, vp: &ViewPort, compareEntry: &CompareEntryRef) {
        // Use calcsEnv for both values and tooltips (has breakdown data + respects Calcs skill selection)
        // L4686-4688
        let primaryBuild = cx.build().model.clone();
        let primaryEnv = primaryBuild.calcsTab.calcsEnv.get();
        let compareEnv = compareEntry.model.calcsTab.calcsEnv.get();
        let (Some(primaryEnv), Some(compareEnv)) = (primaryEnv, compareEnv) else {
            return;
        };
        // L4689-4690
        let primaryActor = match primaryEnv.minion.get() {
            Some(m) if primaryBuild.calcsTab.input.truthy(n!("showMinion")) => m,
            _ => primaryEnv.player.clone(),
        };
        let compareActor = match compareEnv.minion.get() {
            Some(m) if compareEntry.model.calcsTab.input.truthy(n!("showMinion")) => m,
            _ => compareEnv.player.clone(),
        };
        // L4691 (`env.player` is always set: module notes)

        // Skill detail header height
        // L4694
        let skillHeaderHeight = cx.w::<CompareTab>(me).layout.calcsSkillHeaderHeight;

        // Draw skill detail header background and labels
        // L4697-4699
        if skillHeaderHeight > 0.0 {
            Self::DrawCalcsSkillHeader(cx, me, vp, compareEntry, skillHeaderHeight, &primaryEnv, &compareEnv);
        }

        // Reserve space on the right for the scrollbar
        // L4702-4703
        let scrollBarWidth = 20.0;
        let gridWidth = vp.width - scrollBarWidth;

        // Card dimensions
        // Layout: [2px border | 130px label | 2px gap | 2px sep | valW | 2px sep | valW | 2px border]
        // L4707-4712
        let cardWidth = m_min(LAYOUT::calcsMaxCardWidth, gridWidth - 16.0);
        let labelWidth = LAYOUT::calcsLabelWidth;
        let sepW = LAYOUT::calcsSepW;
        let valColWidth = m_floor((cardWidth - 140.0) / 2.0);
        let valCol1X = labelWidth + sepW * 2.0;
        let valCol2X = valCol1X + valColWidth + sepW;

        // Layout parameters
        // L4715-4719
        let maxCol = m_max(1.0, m_floor(gridWidth / (cardWidth + 8.0)));
        let baseX = 4.0;
        let headerBarHeight = LAYOUT::calcsHeaderBarHeight;
        let check = ctl(cx, me, "calcsShowOnlyDifferencesCheck");
        let filterRowOffset = if cx.is_shown(check) { 12.0 } else { 0.0 };
        let baseY = headerBarHeight + filterRowOffset;

        // Pre-compute section visibility and heights
        // L4722
        let calcSections = match bst(cx, me).calcSections.clone() {
            Some(s) => s,
            None => {
                let s = Rc::new(sections_data::load());
                bst_mut(cx, me).calcSections = Some(s.clone());
                s
            }
        };
        let showOnlyDifferences = st(cx, me).calcsShowOnlyDifferences;
        let mut sections: Vec<CalcSec> = Vec::new();
        // L4723
        for secDef in calcSections.iter() {
            // L4724
            let colour = secDef.colour.clone();
            let subSections = &secDef.sub_section;
            // L4725
            let secData = subSections
                .table(1.0)
                .and_then(|s| s.table(n!("data")))
                .unwrap_or_else(|| lua_error!("attempt to index field 'data' (a nil value)"));
            // Check section-level flags against primary actor
            // L4727
            if check_flag(cx, &secData, Some(&primaryActor), None) {
                // L4728-4729
                let mut subSecInfo: Vec<SubSecInfo> = Vec::new();
                let mut sectionHasRows = false;
                // L4730
                for (_, subSec) in subSections.ipairs() {
                    let subSec = subSec
                        .table()
                        .unwrap_or_else(|| lua_error!("attempt to index local 'subSec' (a {} value)", subSec.lua_type()));
                    let data = subSec.table(n!("data"));
                    // L4731
                    let mut rows: Vec<Table> = Vec::new();
                    // L4732
                    let dataRows = data
                        .as_ref()
                        .unwrap_or_else(|| lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)"))
                        .ipairs();
                    for (_, rowData) in dataRows {
                        let Some(rowData) = rowData.table() else {
                            lua_error!("attempt to index local 'rowData' (a {} value)", rowData.lua_type())
                        };
                        // Only include rows with a label and a first column with a format string
                        // L4734
                        let col1 = rowData.get(1.0);
                        if rowData.truthy(n!("label")) && col1.truthy() && col1.get(n!("format")).truthy() {
                            // L4735-4736
                            let primaryVisible = check_flag(cx, &rowData, Some(&primaryActor), None);
                            let compareVisible = check_flag(cx, &rowData, Some(&compareActor), None);
                            // L4737
                            if primaryVisible || compareVisible {
                                // L4738
                                let mut keepRow = true;
                                // L4739
                                if showOnlyDifferences && primaryVisible && compareVisible {
                                    // L4740
                                    let colData = col1.table().unwrap_or_else(|| {
                                        lua_error!("attempt to index field '?' (a {} value)", col1.lua_type())
                                    });
                                    keepRow = !calcRowMatchesBetweenBuilds(
                                        &primaryBuild,
                                        &colData,
                                        &primaryActor,
                                        &compareActor,
                                        compareEntry,
                                    );
                                }
                                // L4742-4744
                                if keepRow {
                                    rows.push(rowData.clone());
                                }
                            }
                        }
                    }
                    // L4748
                    let keepSubSection = !rows.is_empty()
                        || (showOnlyDifferences && !subSectionExtraMatches(data.as_ref(), &primaryActor, &compareActor));
                    // L4749
                    if keepSubSection {
                        // L4750-4751
                        subSecInfo.push(SubSecInfo {
                            label: subSec.get(n!("label")),
                            rows,
                            data,
                        });
                        sectionHasRows = true;
                    }
                }
                // L4754
                if sectionHasRows {
                    // Calculate card height
                    // L4756-4762
                    let mut height = 2.0;
                    for si in &subSecInfo {
                        height = height + 22.0 + si.rows.len() as f64 * 18.0;
                        if !si.rows.is_empty() {
                            height += 2.0;
                        }
                    }
                    // L4763-4767
                    sections.push(CalcSec {
                        colour,
                        subSecs: subSecInfo,
                        height,
                        drawX: 0.0,
                        drawY: 0.0,
                    });
                }
            }
        }

        // Layout: place sections into shortest column
        // L4773-4788
        let maxColN = maxCol as usize;
        let mut colY: Vec<Option<f64>> = vec![None; maxColN + 1];
        let mut maxY = baseY;
        for sec in &mut sections {
            let mut col = 1;
            let mut minY = colY[1].unwrap_or(baseY);
            for c in 2..=maxColN {
                if colY[c].unwrap_or(baseY) < minY {
                    col = c;
                    minY = colY[c].unwrap_or(baseY);
                }
            }
            sec.drawX = baseX + (cardWidth + 8.0) * (col as f64 - 1.0);
            sec.drawY = colY[col].unwrap_or(baseY);
            colY[col] = Some(sec.drawY + sec.height + 8.0);
            maxY = m_max(maxY, colY[col].expect("set"));
        }

        // Position scrollbar and set content dimensions based on laid-out content
        // L4791-4795
        let scrollBar = ctl(cx, me, "calcsScrollBar");
        {
            let b = cx.base_mut(scrollBar);
            b.x = Prop::Val(vp.x + vp.width - 18.0);
            b.y = Prop::Val(vp.y + skillHeaderHeight);
            b.height = Prop::Val(vp.height - skillHeaderHeight);
        }
        ScrollBarControl::set_content_dimension(cx, scrollBar, maxY + 26.0, vp.height - skillHeaderHeight);

        // Set viewport for scroll clipping, offset below skill header so cards can't bleed into it
        // L4798
        cx.sg.set_viewport(vp.x, vp.y + skillHeaderHeight, gridWidth, vp.height - skillHeaderHeight);

        // Cursor position relative to viewport (for hover detection)
        // L4801-4806
        let (cursorX, cursorY) = cx.sys.cursor_pos();
        let vpCursorX = cursorX - vp.x;
        let vpCursorY = cursorY - (vp.y + skillHeaderHeight);
        let mut hoverColData: Option<Table> = None;
        let mut hoverRowLabel: Option<String> = None;
        let (mut hoverRowX, mut hoverRowY, mut hoverRowW, mut hoverRowH) = (0.0, 0.0, 0.0, 0.0);

        // Draw section cards
        // L4809
        let offset = cx.w::<ScrollBarControl>(scrollBar).offset;
        for sec in &sections {
            // L4810-4811
            let x = sec.drawX;
            let y = sec.drawY - offset;

            // Skip if entirely off-screen
            // L4814
            if y + sec.height >= 0.0 && y < vp.height {
                // Draw border
                // L4816-4822
                cx.sg.set_draw_layer(None, Some(-10));
                cx.sg.set_draw_color_esc(&sec.colour);
                cx.sg.draw_image(None, x, y, cardWidth, sec.height);
                // Draw background
                cx.sg.set_draw_color(0.10, 0.10, 0.10);
                cx.sg.draw_image(None, x + 2.0, y + 2.0, cardWidth - 4.0, sec.height - 4.0);
                cx.sg.set_draw_layer(None, Some(0));

                // L4824
                let mut lineY = y;
                // L4825
                for subSec in &sec.subSecs {
                    // Separator above header
                    // L4827-4828
                    cx.sg.set_draw_color_esc(&sec.colour);
                    cx.sg.draw_image(None, x + 2.0, lineY, cardWidth - 4.0, 2.0);
                    // Header text
                    // L4830
                    let label = concat(&subSec.label, "field 'label'");
                    cx.sg
                        .draw_string(x + 3.0, lineY + 3.0, Align::Left, 16.0, Font::VarBold, &format!("^7{label}:"));
                    // Show extra info (e.g. "4521/5000 | 3800/4200")
                    // L4832
                    let extra = subSec.data.as_ref().map_or(Val::Nil, |d| d.get(n!("extra")));
                    if extra.truthy() {
                        // L4833-4834
                        let extraTextW = cx.sg.draw_string_width(16.0, Font::VarBold, &format!("{label}:"));
                        let extraX = x + 3.0 + extraTextW + 8.0;
                        // L4835-4836
                        let primaryExtra = pformat(&extra, &primaryActor, &Val::Nil);
                        let compareExtra = pformat(&extra, &compareActor, &Val::Nil);
                        // L4837
                        if let (Some(p), Some(c)) = (primaryExtra, compareExtra) {
                            // L4838-4839
                            let cc = &cx.main.cc;
                            let text = format!("{}{p}  ^8|  {}{c}", cc.POSITIVE, cc.WARNING);
                            cx.sg.draw_string(extraX, lineY + 3.0, Align::Left, 16.0, Font::Var, &text);
                        }
                    }
                    // Separator below header
                    // L4843-4845
                    cx.sg.set_draw_color_esc(&sec.colour);
                    cx.sg.draw_image(None, x + 2.0, lineY + 20.0, cardWidth - 4.0, 2.0);
                    lineY += 22.0;

                    // Draw rows
                    // L4848
                    for rowData in &subSec.rows {
                        // L4849-4850
                        let colData = rowData.get(1.0);
                        let textSize = rowData.get(n!("textSize"));
                        let textSize = if textSize.truthy() { textSize.num() } else { 14.0 };

                        // Hover highlight
                        // L4853-4856
                        let isHovered = vpCursorX >= x
                            && vpCursorX < x + cardWidth
                            && vpCursorY >= lineY
                            && vpCursorY < lineY + 18.0
                            && vpCursorY >= 0.0
                            && vpCursorY < vp.height;
                        let rowHovered = isHovered && colData.truthy();
                        // L4857
                        if rowHovered {
                            // Draw green border around hovered row (matching normal CalcsTab style)
                            // L4859-4862
                            cx.sg.set_draw_color(0.25, 1.0, 0.25);
                            cx.sg.draw_image(None, x + 2.0, lineY, cardWidth - 4.0, 18.0);
                            set_color_val(cx, &rowData.get(n!("bgCol")));
                            cx.sg.draw_image(None, x + 3.0, lineY + 1.0, cardWidth - 6.0, 16.0);
                            // L4863-4868
                            hoverColData = colData.table();
                            hoverRowLabel = Some(concat(&rowData.get(n!("label")), "field 'label'"));
                            hoverRowX = x;
                            hoverRowY = lineY;
                            hoverRowW = cardWidth;
                            hoverRowH = 18.0;
                        }

                        // Label background and text
                        // L4872
                        let bgCol = rowData.get(n!("bgCol"));
                        let bgCol = if bgCol.truthy() { bgCol } else { Val::from("^0") };
                        // L4873-4876
                        if !rowHovered {
                            set_color_val(cx, &bgCol);
                            cx.sg.draw_image(None, x + 2.0, lineY, labelWidth - 2.0, 18.0);
                        }
                        // L4877-4878
                        let textColor = rowData.get(n!("color"));
                        let textColor = if textColor.truthy() {
                            concat(&textColor, "local 'textColor'")
                        } else {
                            "^7".to_owned()
                        };
                        let label = concat(&rowData.get(n!("label")), "field 'label'");
                        cx.sg.draw_string(
                            x + labelWidth,
                            lineY + 1.0,
                            Align::RightX,
                            16.0,
                            Font::Var,
                            &format!("{textColor}{label}^7:"),
                        );

                        let format = colData.get(n!("format"));
                        // Primary value column
                        // L4881-4886
                        if !rowHovered {
                            cx.sg.set_draw_color_esc(&sec.colour);
                            cx.sg.draw_image(None, x + valCol1X - sepW, lineY, sepW, 18.0);
                            set_color_val(cx, &bgCol);
                            cx.sg.draw_image(None, x + valCol1X, lineY, valColWidth, 18.0);
                        }
                        // L4887-4892
                        if colData.truthy() && format.truthy() {
                            if let Some(str) = pformat(&format, &primaryActor, &colData) {
                                cx.sg.draw_string(
                                    x + valCol1X + 2.0,
                                    lineY + 9.0 - textSize / 2.0,
                                    Align::Left,
                                    textSize,
                                    Font::Var,
                                    &format!("^7{str}"),
                                );
                            }
                        }

                        // Compare value column
                        // L4895-4900
                        if !rowHovered {
                            cx.sg.set_draw_color_esc(&sec.colour);
                            cx.sg.draw_image(None, x + valCol2X - sepW, lineY, sepW, 18.0);
                            set_color_val(cx, &bgCol);
                            cx.sg.draw_image(None, x + valCol2X, lineY, valColWidth, 18.0);
                        }
                        // L4901-4906
                        if colData.truthy() && format.truthy() {
                            if let Some(str) = pformat(&format, &compareActor, &colData) {
                                cx.sg.draw_string(
                                    x + valCol2X + 2.0,
                                    lineY + 9.0 - textSize / 2.0,
                                    Align::Left,
                                    textSize,
                                    Font::Var,
                                    &format!("^7{str}"),
                                );
                            }
                        }

                        // L4908
                        lineY += 18.0;
                    }
                    // L4910-4912
                    if !subSec.rows.is_empty() {
                        lineY += 2.0;
                    }
                }
            }
        }

        // Draw hover tooltip for calcs breakdown (reset viewport first so tooltip can extend beyond)
        // L4918
        if let Some(hoverColData) = hoverColData {
            // L4919-4920
            cx.sg.reset_viewport();
            Self::DrawCalcsTooltip(
                cx,
                me,
                &hoverColData,
                hoverRowLabel.as_deref(),
                hoverRowX + vp.x,
                hoverRowY + vp.y + skillHeaderHeight,
                hoverRowW,
                hoverRowH,
                vp,
                compareEntry,
            );
        } else if let Some(hover) = bst(cx, me).calcsSkillHeaderHover.clone() {
            // L4922-4928
            cx.sg.reset_viewport();
            calcsHelpers::DrawSkillBreakdownPanel(
                cx,
                &hover.build,
                hover.breakdown,
                Some(hover.label),
                hover.x,
                hover.y,
                hover.w,
                hover.h,
                vp,
            );
        } else {
            // L4929
            cx.sg.reset_viewport();
        }
    }

    // ============================================================
    // CONFIG VIEW
    // ============================================================

    /// lua: CompareTab.lua:4936-5032 `CompareTabClass:DrawConfig(vp, compareEntry, headerOnly)`
    /// @pob Classes/CompareTab.lua:4936-5032 h=03d91a08
    #[allow(clippy::too_many_lines)]
    pub fn DrawConfig(cx: &mut Cx, me: CtrlId, vp: &ViewPort, compareEntry: &CompareEntryRef, headerOnly: bool) {
        // L4937-4942
        let rowHeight = LAYOUT::configRowHeight;
        let columnHeaderHeight = LAYOUT::configColumnHeaderHeight;
        let fixedHeaderHeight = LAYOUT::configFixedHeaderHeight;
        let sectionInnerPad = LAYOUT::configSectionInnerPad;
        let sectionWidth = LAYOUT::configSectionWidth;
        let labelOffset = LAYOUT::configLabelOffset;

        // Fixed header area: row 1 = buttons, row 2 = search/dropdowns, then column headers + separator
        // L4945-4947
        cx.sg.set_viewport(vp.x, vp.y, vp.width, fixedHeaderHeight);
        cx.sg.set_draw_color(0.05, 0.05, 0.05);
        cx.sg.draw_image(None, 0.0, 0.0, vp.width, fixedHeaderHeight);
        // Controls are drawn by ControlHost (positioned in LayoutConfigView)
        // L4949-4950
        let colHeaderY = 54.0;
        cx.sg.set_draw_color(1.0, 1.0, 1.0);
        // Column headers aligned with first column's control offsets
        // L4952-4956
        let headerBaseX = 10.0;
        let primaryName = format!("{}{}", cx.main.cc.POSITIVE, primary_short_name(cx));
        cx.sg.draw_string(
            headerBaseX + LAYOUT::configCol3 - 8.0,
            colHeaderY,
            Align::RightX,
            columnHeaderHeight,
            Font::Var,
            &primaryName,
        );
        let cmpName = format!("{}{}", cx.main.cc.WARNING, compareEntry.label);
        cx.sg.draw_string(
            headerBaseX + LAYOUT::configCol3,
            colHeaderY,
            Align::Left,
            columnHeaderHeight,
            Font::Var,
            &cmpName,
        );
        // L4957-4958
        cx.sg.set_draw_color(0.5, 0.5, 0.5);
        cx.sg.draw_image(None, 4.0, colHeaderY + columnHeaderHeight + 4.0, vp.width - 8.0, 2.0);
        // L4959-4962
        if headerOnly {
            cx.sg.reset_viewport();
            return;
        }

        // Scrollable content area (clipped below fixed header)
        // L4965-4970
        let scrollH = vp.height - fixedHeaderHeight;
        if scrollH <= 0.0 {
            cx.sg.reset_viewport();
            return;
        }
        cx.sg.set_viewport(vp.x, vp.y + fixedHeaderHeight, vp.width, scrollH);

        // Draw section boxes
        // L4973
        let scrollY = st(cx, me).scrollY;
        let layout = cx.w::<CompareTab>(me).layout.configSectionLayout.clone();
        for sec in &layout {
            // L4974-4976
            let boxX = sec.x;
            let boxY = sec.y - scrollY;
            let boxH = sec.height;

            // Skip entirely off-screen sections
            // L4979
            if boxY + boxH >= 0.0 && boxY < scrollH {
                // Draw section box
                // L4981-4986
                cx.sg.set_draw_layer(None, Some(-10));
                cx.sg.set_draw_color(0.66, 0.66, 0.66);
                cx.sg.draw_image(None, boxX, boxY, sectionWidth, boxH);
                cx.sg.set_draw_color(0.1, 0.1, 0.1);
                cx.sg.draw_image(None, boxX + 2.0, boxY + 2.0, sectionWidth - 4.0, boxH - 4.0);
                cx.sg.set_draw_layer(None, Some(0));

                // Draw section label badge
                // L4989-4992
                let mut labelText = sec.name.clone();
                if sec.diffCount > 0 {
                    labelText = format!("{labelText}  ({} diff)", sec.diffCount);
                }
                // L4993-4999
                let labelWidth = cx.sg.draw_string_width(14.0, Font::Var, &labelText);
                cx.sg.set_draw_color(0.66, 0.66, 0.66);
                cx.sg.draw_image(None, boxX + 6.0, boxY - 8.0, labelWidth + 6.0, 18.0);
                cx.sg.set_draw_color(0.0, 0.0, 0.0);
                cx.sg.draw_image(None, boxX + 7.0, boxY - 7.0, labelWidth + 4.0, 16.0);
                cx.sg.set_draw_color(1.0, 1.0, 1.0);
                cx.sg.draw_string(boxX + 9.0, boxY - 6.0, Align::Left, 14.0, Font::Var, &labelText);

                // Draw rows inside section
                // L5002
                let mut rowY = boxY + sectionInnerPad;
                // L5003
                for row in &sec.rows {
                    // L5004
                    if rowY + rowHeight >= 0.0 && rowY < scrollH {
                        // L5005
                        let varData = row.ctrlInfo.varData;
                        // Subtle highlight for diff rows
                        // L5007-5012
                        if row.isDiff {
                            cx.sg.set_draw_layer(None, Some(-5));
                            cx.sg.set_draw_color(0.18, 0.14, 0.08);
                            cx.sg.draw_image(None, boxX + 3.0, rowY, sectionWidth - 6.0, rowHeight);
                            cx.sg.set_draw_layer(None, Some(0));
                        }
                        // Label (reduce font size for long labels, matching ConfigTab behavior)
                        // L5014
                        let labelStr: String = match (&varData.label, &varData.var) {
                            (Some(l), _) => l.to_string(),
                            (None, Some(v)) => v.to_string(),
                            (None, None) => lua_error!("bad argument #3 to 'DrawStringWidth' (string expected, got nil)"),
                        };
                        // L5015
                        let labelSize = if cx.sg.draw_string_width(14.0, Font::Var, &labelStr) > 228.0 { 12.0 } else { 14.0 };
                        // L5016-5018
                        cx.sg.set_draw_color(1.0, 1.0, 1.0);
                        cx.sg.draw_string(
                            boxX + labelOffset,
                            rowY + 2.0,
                            Align::Left,
                            labelSize,
                            Font::Var,
                            &format!("^7{labelStr}"),
                        );
                        // Controls are drawn by ControlHost (positioned in LayoutConfigView)
                    }
                    // L5021
                    rowY += rowHeight;
                }
            }
        }

        // L5026-5029
        if layout.is_empty() {
            let text = format!("{}No configuration options to display.", cx.main.cc.POSITIVE);
            cx.sg.draw_string(10.0, -scrollY, Align::Left, 16.0, Font::Var, &text);
        }

        // L5031
        cx.sg.reset_viewport();
    }
}

// @pob-skip Classes/CompareTab.lua:5033-5036 reason="`return CompareTabClass`: the module (`tabs::compare`)"
