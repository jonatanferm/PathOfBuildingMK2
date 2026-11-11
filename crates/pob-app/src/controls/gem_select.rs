//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/GemSelectControl.lua 1-854
//! (`newClass("GemSelectControl", "EditControl")`, the gem selection combobox). Owner: U29.
//!
//! @pob-file Classes/GemSelectControl.lua @16de4b82d
//!
//! # Surface
//!
//! [`GemSelectControl::new`]`(anchor, rect, skills_tab, index, change_func, force_tooltip,
//! imbued)` is `GemSelectControl(anchor, rect, skillsTab, index, changeFunc, forceTooltip,
//! imbued)` (L28). `skills_tab` is the SkillsTab node (`self.skillsTab`): its ControlHost part
//! (`selControl`, `GetMouseOverControl`) is read through that node, its model half
//! (`displayGroup`, `socketGroupList`, `defaultGemLevel`, `defaultGemQuality`, `ProcessGemLevel`,
//! and the UI settings `sortGemsByDPS`, `sortGemsByDPSField`, `showSupportGemTypes`,
//! `showLegacyGems`, which BM-B keeps in `skillsTab:ui_state()`, CCR-0019) through
//! `cx.build().model.skillsTab`, and `self.skillsTab.build` is the current build
//! (`cx.build()`). The Lua fields the SkillsTab sets on its gem selectors after construction
//! (`gemId` SkillsTab.lua:178/362/1288, `index`, `inactiveCol`, the edit's `buf` via `SetText`)
//! are `pub`.
//!
//! The calc is reached through the facade (ui doc §10.4): `calcsTab:GetMiscCalculator()` is
//! `cx.build_mut().calc.misc_calculator(&model)` and `calcFunc(nil, useFullDPS)` is
//! `MiscCalc::call(None, Some(useFullDPS))`. [`GemSelectControl::calc_output_with_this_gem`]
//! mutates the live model (the socket group's `gemList`) around that call exactly as the Lua
//! does. `Poll::Pending` (web only) is the placeholder "no new numbers": the DPS of the gem
//! stays `baseDPS` and the hover tooltip is not drawn that frame.
//!
//! # Hooks
//!
//! No subclass and no instance overrides a GemSelectControl method (grep at 16de4b82d:
//! SkillsTab.lua:322 and :786 are the only constructors and assign data fields only), so its
//! internal `self:X()` calls are static, except where Lua reaches a method another class may
//! override: `self:IsMouseOver()` in OnKeyDown/OnKeyUp (L762, L833; virtual through `cx`, which is
//! this class's), `mOverControl:OnKeyDown`, `self.selControl:OnKeyUp`. `self:IsMouseOver()` in
//! Draw (L526) is read with its second return value ([`GemSelectControl::is_mouse_over_comp`]).
//! Super calls (`self.EditControl:Draw/OnFocusGained/OnKeyDown/OnKeyUp`) are static calls of
//! EditControl's methods on the same node; OnChar is EditControl's.
//!
//! # Notes
//!
//! - `self.gems` (`"Default:" .. gemId` → gem data) keeps `data.gems`' dump order; PoB's is its
//!   per-process string-hash order. ORDER (c): every list built from it is fully sorted by
//!   `SortGemList` (gem names are unique: `u29_data_invariants`, so the comparator is a total
//!   order), `canSupport` / `dps` / `dpsColor` are sets and maps, and the DPS builder's order of
//!   `pendingGems` only decides which partial sorts happen between the 50 ms yields, i.e. the
//!   "Sorting N%" text; the final order and colours are the same.
//! - `self.gemId` is either a `self.gems` key (a string, set by UpdateGem) or a gem data table
//!   (SkillsTab.lua:1288 assigns `data.gems[...]` to the imbued selector): [`GemIdVal`].
//! - L194 `addThisGem`, L663-L664 `colorS`/`colorA` and L676/L685 `sx` are undeclared globals in
//!   the Lua (assigned before every read; no other file reads them): locals here.
//! - L669/L747 `cursorX < (cursorX + width - 20)` (sic) is kept.
//! - L58-L64 `self.costs = data.costs` and `self.reservationMap = {…}` are stored and never read
//!   (GemTooltip has its own copy): not ported as fields.
//! - `CalcOutputWithThisGem` with `self.index` past the end of `gemList` (`index > #gemList + 1`):
//!   Lua stores the new gem in the hash part, where the calc's `ipairs` cannot see it; the port
//!   keeps that gem out of the list (same output), as the frozen `Shared<Vec<_>>` has no holes.
//! - The DPS builder is a coroutine (ui doc §6.13): `coroutine.resume`'s error result is
//!   [`Coroutine::try_resume`]'s (re-raised in dev mode, L514-L516); on wasm32, where errors are
//!   not caught, `resume`.
//! - `itemLib.wiki` (ItemTools.lua:384-413; IT-B left it to the UI, CCR-0108 §2): `openGem`,
//!   `open` and `matchesKey` are ported below as [`wiki_open_gem`] and friends.
//! - Property closures that write fields (ui doc §7.3): none (the scrollbar's `y`, `height` and
//!   `shown` only read).

use std::cell::RefCell;
use std::rc::Rc;
use std::task::Poll;

use indexmap::IndexMap;
use pob_lua::math::{m_floor, m_max, m_min};
use pob_lua::{lfmt, Name};
use pob_sg::{Align, Font};
use pob_ui_core::controls::scroll_bar::ScrollBarControl;
use pob_ui_core::controls::{EditArgs, EditControl};
use pob_ui_core::coroutine::{co, Coroutine};
use pob_ui_core::draw::draw_check_mark;
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::Tooltip;

use super::gem_tooltip::{self, TooltipGemData, TooltipGemInstance};
use crate::app::CxApp;
use crate::build::stats;
use crate::calc::MiscCalc;
use crate::model::gamedata::GemDataRef;
use crate::model::{BuildRef, GemInstance, GemInstanceRef, LStr, Output, SocketGroupRef, Val};

// @pob-skip Classes/GemSelectControl.lua:1-6 reason="file header comment"
// @pob-skip Classes/GemSelectControl.lua:17-20 reason="class annotation and header (newClass): the struct below"
// @pob-skip Classes/GemSelectControl.lua:69-69 reason="blank line between methods"

/// lua: GemSelectControl.lua:7-16 (the `table`/`math` locals, `require("Classes.GemTooltip")`
/// (`super::gem_tooltip`) and the two tooltip texts)
/// @pob Classes/GemSelectControl.lua:7-16 h=44f6eb3a
const TOOL_TIP_TEXT: &str =
    "Prefix tag searches with a colon and exclude tags with a dash. e.g. :fire:lightning:-cold:area";
const IMBUED_TOOLTIP_TEXT: &str = "\"Socketed in\" item must be set in order to add an imbued support.\nOnly one imbued support is allowed per item.";

/// `self.gemChangeFunc(gemId, addUndo, focusLost, bufMatchesGem)` (the SkillsTab's callbacks,
/// SkillsTab.lua:322, :786, read the last three as conditions only).
pub type GemChangeFunc = Rc<dyn Fn(&mut Cx, Option<String>, bool, bool, bool)>;

/// `self.gemId`: a key of `self.gems`, or the gem data table SkillsTab.lua:1288 assigns.
#[derive(Clone)]
pub enum GemIdVal {
    Key(String),
    Data(GemDataRef),
}

/// `self.sortCache` (L289-302 plus L356-360). Shared: the DPS builder keeps the table it
/// started with (`local sortCache = self.sortCache`, L416).
#[derive(Default)]
pub struct SortCache {
    pub consider_gem_type: Option<LStr>,
    pub show_legacy_gems: bool,
    pub socket_group: Option<SocketGroupRef>,
    pub gem_instance: Option<GemInstanceRef>,
    pub output_revision: f64,
    pub default_level: Option<Name>,
    pub default_quality: Option<f64>,
    pub character_level: f64,
    /// `canSupport[gemId] = true`
    pub can_support: std::collections::HashSet<String>,
    pub dps: std::collections::HashMap<String, f64>,
    pub dps_color: std::collections::HashMap<String, String>,
    pub sort_type: Option<LStr>,
    pub calc_func: Option<Rc<dyn MiscCalc>>,
    pub use_full_dps: bool,
    pub base_dps: f64,
    pub dps_field: Option<LStr>,
    /// nil once the DPS builder finished (L451)
    pub pending_gems: Option<Vec<String>>,
    /// `calcBase` of L352 (Rust only: the output a `Poll::Pending` call stands in with, module
    /// notes)
    pub base: Option<Output>,
}

pub struct GemSelectControl {
    pub edit: EditControl,
    /// `self.skillsTab` (the SkillsTab node; the model half is `cx.build().model.skillsTab`)
    pub skills_tab: CtrlId,
    /// `self.gems` (`"Default:" .. gemId` → gem data; ORDER: module notes)
    pub gems: IndexMap<String, GemDataRef>,
    /// `self.index` (a Lua index into the display group's `gemList`)
    pub index: f64,
    /// `self.gemChangeFunc`
    pub gem_change_func: Option<GemChangeFunc>,
    pub force_tooltip: bool,
    /// `self.list`: gem ids, or `""` when nothing matched
    pub list: Vec<String>,
    pub mode: String,
    /// `self.imbuedSelect`
    pub imbued_select: bool,
    pub dps_build_flag: bool,
    pub sort_gems_by: Option<Name>,
    pub last_sort_gems_by: Option<Name>,
    pub sort_cache: Option<Rc<RefCell<SortCache>>>,
    pub dropped: bool,
    /// `self.selIndex` (nil until the first BuildList / SyncSelection)
    pub sel_index: Option<f64>,
    /// `self.searchStr` (nil until the first BuildList)
    pub search_str: Option<String>,
    pub no_matches: bool,
    pub gem_id: Option<GemIdVal>,
    pub gem_name: Option<String>,
    pub initial_buf: Option<String>,
    pub hover_sel: Option<f64>,
    pub last_hover_sel: Option<f64>,
    pub hover_frame_count: Option<f64>,
    pub sort_percentage: Option<String>,
    pub dps_builder: Option<Coroutine>,
    pub dps_builder_callback: Option<Rc<dyn Fn(&mut Cx, f64)>>,
}

/// `#gemList` and `gemList[i]` of a socket group.
fn gem_at(group: &SocketGroupRef, index: f64) -> Option<GemInstanceRef> {
    if index >= 1.0 && index.fract() == 0.0 {
        group.gemList.get().get(index as usize)
    } else {
        None
    }
}

/// `(field == "FullDPS" and out[field] ~= nil and out[field]) or (out.Minion and
/// out.Minion.CombinedDPS) or (out[field] ~= nil and out[field]) or 0` (L354, L430).
#[track_caller]
fn dps_of(out: &Output, field: Option<&LStr>) -> f64 {
    let at = |f: Option<&LStr>| f.map_or(Val::Nil, |f| out.get(Name::from(f.as_ref())));
    let v = at(field);
    // `x ~= nil and x`: nil → false
    let a = if field.is_some_and(|f| &**f == "FullDPS") && !v.is_nil() {
        v.clone()
    } else {
        Val::Bool(false)
    };
    let r = if a.truthy() {
        a
    } else {
        let minion = out.get(Name::from("Minion"));
        let b = if minion.truthy() {
            minion
                .table()
                .unwrap_or_else(|| {
                    lua_error!(
                        "attempt to index field 'Minion' (a {} value)",
                        minion.lua_type()
                    )
                })
                .get(Name::from("CombinedDPS"))
        } else {
            minion
        };
        if b.truthy() {
            b
        } else {
            let c = if v.is_nil() { Val::Bool(false) } else { v };
            if c.truthy() {
                c
            } else {
                Val::Num(0.0)
            }
        }
    };
    r.opt_num()
        .unwrap_or_else(|| lua_error!("attempt to compare {} with number", r.lua_type()))
}

impl GemSelectControl {
    fn sb(cx: &Cx, me: CtrlId) -> Ctrl<ScrollBarControl> {
        cx.child_as::<ScrollBarControl>(me, "scrollBar")
    }

    fn model(cx: &Cx) -> BuildRef {
        cx.build().model.clone()
    }

    /// `self.skillsTab.displayGroup` (nil is Lua's index error at the use)
    #[track_caller]
    fn display_group(cx: &Cx) -> SocketGroupRef {
        Self::model(cx)
            .skillsTab
            .displayGroup
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'displayGroup' (a nil value)"))
    }

    fn buf(cx: &Cx, me: CtrlId) -> String {
        cx.w::<EditControl>(me).buf.clone()
    }

    /// `self.skillsTab:ProcessGemLevel(gemData, self.imbuedSelect)`
    fn process_gem_level(cx: &Cx, me: CtrlId, gem_data: &GemDataRef) -> f64 {
        let model = Self::model(cx);
        let imbued = cx.w::<Self>(me).imbued_select;
        model.skillsTab.ProcessGemLevel(&model, gem_data, imbued)
    }

    /// lua: GemSelectControl.lua:21-68 GemSelectControl(anchor, rect, skillsTab, index,
    /// changeFunc, forceTooltip, imbued)
    /// @pob Classes/GemSelectControl.lua:21-68 h=7b0e3536
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        skills_tab: CtrlId,
        index: f64,
        change_func: Option<GemChangeFunc>,
        force_tooltip: bool,
        imbued: bool,
    ) -> New<GemSelectControl> {
        // L29 self:EditControl(anchor, rect, nil, nil, "^ %a':-")
        let edit = EditControl::state(EditArgs {
            filter: Some("^ %a':-".to_owned()),
            ..EditArgs::default()
        });
        New::new(
            Base::new(anchor, rect),
            GemSelectControl {
                edit,
                // L41
                skills_tab,
                gems: IndexMap::new(),
                index,
                gem_change_func: None,
                force_tooltip: false,
                list: Vec::new(),
                mode: String::new(),
                imbued_select: false,
                dps_build_flag: false,
                sort_gems_by: None,
                last_sort_gems_by: None,
                sort_cache: None,
                dropped: false,
                sel_index: None,
                search_str: None,
                no_matches: false,
                gem_id: None,
                gem_name: None,
                initial_buf: None,
                hover_sel: None,
                last_hover_sel: None,
                hover_frame_count: None,
                sort_percentage: None,
                dps_builder: None,
                dps_builder_callback: None,
            },
        )
        .host()
        .init(|cx, me| EditControl::init(cx, me.id()))
        .init(move |cx, me| {
            let me = me.id();
            // L30
            let step = (cx.base(me).height.raw() - 4.0) * 4.0;
            let sb = cx.add(
                me,
                "scrollBar",
                ScrollBarControl::new(
                    Anchor::new(TopRight, me, TopRight),
                    pob_ui_core::layout::rect(-1, 0, 18, 0),
                    Some(step),
                    None,
                    false,
                ),
            );
            // L31-L34
            cx.base_mut(sb).y = Prop::f(move |cx, _| {
                let (_width, height) = cx.size(me);
                height + 1.0
            });
            // L35-L37
            cx.base_mut(sb).height = Prop::f(move |cx, _| {
                let n = cx.w::<Self>(me).list.len() as f64;
                (cx.base(me).height.raw() - 4.0) * m_min(n, 15.0) + 2.0
            });
            // L38-L40
            cx.base_mut(sb).shown = Prop::f(move |cx, _| {
                cx.w::<Self>(me).dropped && ScrollBarControl::enabled_raw(cx, Self::sb(cx, me).id())
            });
            // L42
            cx.w_mut::<Self>(me).gems = IndexMap::new();
            // L43
            Self::populate_gem_list(cx, me);
            let w = cx.w_mut::<Self>(me);
            // L44
            w.index = index;
            // L45
            w.gem_change_func = change_func;
            // L46
            w.force_tooltip = force_tooltip;
            // L47
            w.list = Vec::new();
            // L48
            w.mode = String::new();
            // L49-L57
            cx.w_mut::<EditControl>(me).change_func =
                Some(Rc::new(move |cx: &mut Cx, _buf: &str| {
                    // L50
                    if !cx.w::<Self>(me).dropped {
                        // L51
                        cx.w_mut::<Self>(me).dropped = true;
                        // L52
                        Self::update_sort_cache(cx, me);
                    }
                    // L54
                    cx.w_mut::<Self>(me).sel_index = Some(0.0);
                    // L55
                    let buf = Self::buf(cx, me);
                    Self::build_list(cx, me, &buf);
                    // L56
                    Self::update_gem(cx, me, false, false, false);
                }));
            // L58-L64 self.costs, self.reservationMap: never read (module notes)
            let w = cx.w_mut::<Self>(me);
            // L65
            w.imbued_select = imbued;
            // L66
            w.dps_build_flag = false;
        })
    }

    /// lua: GemSelectControl.lua:70-114 CalcOutputWithThisGem(calcFunc, gemData, useFullDPS):
    /// the output and the gem instance (the hovered gem, put into the display group's gemList
    /// for the call and restored afterwards).
    /// @pob Classes/GemSelectControl.lua:70-114 h=b540d6eb
    pub fn calc_output_with_this_gem(
        cx: &mut Cx,
        me: CtrlId,
        calc_func: &Rc<dyn MiscCalc>,
        gem_data: &GemDataRef,
        use_full_dps: bool,
    ) -> (Output, GemInstanceRef) {
        let model = Self::model(cx);
        let display_group = Self::display_group(cx);
        // L71
        let gem_list = display_group.gemList.get();
        // L72
        let display_gem_list = display_group.extra.get(Name::from("displayGemList"));
        // L73
        let mut old_gem: Option<(
            Option<GemDataRef>,
            f64,
            Option<pob_calc::active_skill::SkillEffectRef>,
        )> = None;

        // L75 the imbuedSupport control actively switches to the latest index of the current displayGroup's gemList so we can use the canSupport filtering
        // L76
        if cx.w::<Self>(me).imbued_select {
            // L77
            cx.w_mut::<Self>(me).index = (gem_list.len() + 1) as f64;
        }
        let index = cx.w::<Self>(me).index;
        // detached: Lua stored it in the hash part (module notes)
        let mut detached: Option<GemInstanceRef> = None;
        // L79
        if let Some(g) = gem_at(&display_group, index) {
            // L80 copyTable(gemList[self.index], true): the fields L104-L106 restore
            old_gem = Some((g.gemData.get(), g.level.get(), g.displayEffect.get()));
        } else {
            // L82-L92
            let g = Rc::new(GemInstance::default());
            g.level.set(gem_data.naturalMaxLevel);
            g.quality
                .set(model.skillsTab.defaultGemQuality.get().unwrap_or(0.0));
            g.count.set(Some(1.0));
            g.enabled.set(true);
            g.enableGlobal1.set(true);
            g.enableGlobal2.set(true);
            g.gemId.set(Some(gem_data.id));
            g.nameSpec.set(LStr::from(gem_data.name.as_str()));
            g.skillId.set(Some(gem_data.grantedEffectId));
            if index == (gem_list.len() + 1) as f64 {
                gem_list.push(g);
            } else {
                detached = Some(g);
            }
        }

        // L95 Create gemInstance to represent the hovered gem
        // L96
        let gem_instance = gem_at(&display_group, index)
            .or_else(|| detached.clone())
            .expect("stored above");
        // L97
        let level = Self::process_gem_level(cx, me, gem_data);
        gem_instance.level.set(level);
        // L98
        gem_instance.gemData.set(Some(gem_data.clone()));
        // L99
        gem_instance.displayEffect.set(None);
        // L100 Calculate the impact of using this gem
        // L101 (Pending: the placeholder of the module notes)
        let output = match calc_func.call(None, Some(use_full_dps)) {
            Poll::Ready(o) => Some(o),
            Poll::Pending => None,
        };
        // L102 Put the original gem back into the list
        // L103
        if let Some((gd, lvl, de)) = old_gem {
            // L104
            gem_instance.gemData.set(gd);
            // L105
            gem_instance.level.set(lvl);
            // L106
            gem_instance.displayEffect.set(de);
        } else if detached.is_none() {
            // L108 gemList[self.index] = nil (the last entry)
            gem_list.remove(Some(index as usize));
        }

        // L111
        display_group
            .extra
            .set(Name::from("displayGemList"), display_gem_list);

        // L113
        let output = output.unwrap_or_else(|| {
            cx.w::<Self>(me)
                .sort_cache
                .as_ref()
                .map(|sc| sc.borrow().base_output())
                .unwrap_or_default()
        });
        (output, gem_instance)
    }

    /// lua: GemSelectControl.lua:116-146 PopulateGemList
    /// @pob Classes/GemSelectControl.lua:116-146 h=6d93f97b
    pub fn populate_gem_list(cx: &mut Cx, me: CtrlId) {
        let model = Self::model(cx);
        let st = &model.skillsTab;
        let ui = st.ui_state();
        // L117
        cx.w_mut::<Self>(me).gems.clear();
        let show = ui.str(Name::from("showSupportGemTypes"));
        let show_is = |s: &str| show.as_deref() == Some(s);
        // L118
        let show_all = show_is("ALL");
        // L119
        let show_exceptional = show_is("EXCEPTIONAL");
        // L120
        let show_normal = show_is("NORMAL");
        // L121
        let match_level = st.defaultGemLevel().as_str() == "characterLevel";
        // L122
        let character_level = model.characterLevel.get();
        let (sort_gems_by, imbued) = {
            let w = cx.w::<Self>(me);
            (w.sort_gems_by, w.imbued_select)
        };
        let show_legacy = ui.truthy(Name::from("showLegacyGems"));
        let mut gems = IndexMap::new();

        // L124 ORDER: (c), module notes
        for (gem_id, gem_data) in &model.data.gems {
            let ge = &gem_data.grantedEffect;
            // L125
            if !ge.hideFromGemList
                && (sort_gems_by.is_some_and(|t| gem_data.tags.contains(&t))
                    || sort_gems_by.is_none())
            {
                // L126
                let level_requirement = ge
                    .levels
                    .get(1.0)
                    .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
                    .levelRequirement;
                // L127
                if character_level >= level_requirement || !match_level {
                    // L128
                    let is_legacy_awakened = ge.legacy && ge.plusVersionOf.is_some();
                    let exceptional = gem_data.tagString.contains("Exceptional");
                    let key = format!("Default:{gem_id}");
                    // L129
                    if imbued {
                        // L130 Imbued dropdown only allows non-exceptional support gems and supports that don't grant active skills
                        // L131
                        if ge.support
                            && !exceptional
                            && !is_legacy_awakened
                            && gem_data
                                .secondaryGrantedEffect
                                .as_ref()
                                .map_or(true, |s| s.support)
                        {
                            // L132
                            gems.insert(key, gem_data.clone());
                        }
                    } else if (show_exceptional || show_all) && (is_legacy_awakened || exceptional)
                    {
                        // L135-L136
                        if show_legacy || !ge.legacy {
                            // L137
                            gems.insert(key, gem_data.clone());
                        }
                    } else if show_normal || show_all {
                        // L140
                        gems.insert(key, gem_data.clone());
                    }
                }
            }
        }
        cx.w_mut::<Self>(me).gems = gems;
    }

    /// lua: GemSelectControl.lua:148-163 FilterSupport(gemId, gemData)
    /// @pob Classes/GemSelectControl.lua:148-163 h=62e0f3e8
    fn filter_support(cx: &Cx, me: CtrlId, gem_id: &str, gem_data: &GemDataRef) -> bool {
        let model = Self::model(cx);
        let ui = model.skillsTab.ui_state();
        // L149
        let show_support_types = ui.str(Name::from("showSupportGemTypes"));
        let is = |s: &str| show_support_types.as_deref() == Some(s);
        let ge = &gem_data.grantedEffect;
        // L150
        let is_legacy_awakened = ge.legacy && ge.plusVersionOf.is_some();
        // L151
        if ge.legacy && !ui.truthy(Name::from("showLegacyGems")) {
            // L152
            return false;
        }

        // L155
        if cx.w::<Self>(me).imbued_select {
            // L156
            return Self::sort_cache_of(cx, me)
                .borrow()
                .can_support
                .contains(gem_id);
        }

        let exceptional = gem_data.tagString.contains("Exceptional");
        // L159-L162
        !ge.support
            || is("ALL")
            || (is("NORMAL") && !(is_legacy_awakened || exceptional))
            || (is("EXCEPTIONAL") && (is_legacy_awakened || exceptional))
    }

    /// `self.sortCache` where Lua indexes it (nil is the index error).
    #[track_caller]
    fn sort_cache_of(cx: &Cx, me: CtrlId) -> Rc<RefCell<SortCache>> {
        cx.w::<Self>(me)
            .sort_cache
            .clone()
            .unwrap_or_else(|| lua_error!("attempt to index field 'sortCache' (a nil value)"))
    }

    /// lua: GemSelectControl.lua:165-264 BuildList(buf)
    /// @pob Classes/GemSelectControl.lua:165-264 h=11fe4e85
    #[allow(clippy::too_many_lines)]
    pub fn build_list(cx: &mut Cx, me: CtrlId, buf: &str) {
        // L166
        let mut search_term: String;
        // L167
        let mut tags_list: Vec<String>;

        // L169
        let sb = Self::sb(cx, me);
        cx.get_mut(sb).offset = 0.0;
        // L170
        cx.w_mut::<Self>(me).list.clear();
        // L171
        let search_str = format!("{buf}{}", cx.w::<Self>(me).mode);
        cx.w_mut::<Self>(me).search_str = Some(search_str.clone());
        // L172
        cx.w_mut::<Self>(me).mode = String::new();
        let gems: Vec<(String, GemDataRef)> = cx
            .w::<Self>(me)
            .gems
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        // L173
        if !search_str.is_empty() {
            // L174
            let mut added: std::collections::HashSet<String> = std::collections::HashSet::new();

            // L176 split the buffer using :
            // L177 Remove the first entry as the name search term (can be blank)
            // L178
            tags_list = pob_lua::string::split(&search_str, ":");
            // L179 (`tagsList[1]` of an all-colon string is nil; `searchTerm:lower()` then errors)
            search_term = tags_list
                .first()
                .cloned()
                .unwrap_or_else(|| lua_error!("attempt to index local 'searchTerm' (a nil value)"));
            // L180
            if !tags_list.is_empty() {
                tags_list.remove(0);
            }

            // L182 Search for gem name using increasingly broad search patterns
            // L183
            let lower_search = pob_lua::string::lower(&search_term);
            // L184-L189
            let pattern_list = [
                format!("^ {lower_search}$"), // Exact match
                format!(
                    "^{}$",
                    pob_lua::gsub(&lower_search, "%a", " %0%%l+", None).0
                ), // Simple abbreviation ("CtF" -> "Cold to Fire")
                format!("^ {lower_search}"),  // Starts with
                lower_search.clone(),         // Contains
            ];
            // L190
            for pattern in &pattern_list {
                // L191
                let mut match_list: Vec<String> = Vec::new();
                // L192 ORDER: (c), module notes
                for (gem_id, gem_data) in &gems {
                    let name_lower = format!(" {}", pob_lua::string::lower(gem_data.name.as_str()));
                    // L193
                    if Self::filter_support(cx, me, gem_id, gem_data)
                        && !added.contains(gem_id)
                        && pob_lua::lmatch(&name_lower, pattern, None).is_some()
                    {
                        // L194 (an undeclared global in the Lua)
                        let mut add_this_gem = true;
                        // L195
                        if !tags_list.is_empty() {
                            // L196
                            for tag in &tags_list {
                                // L197
                                let mut tag_name =
                                    pob_lua::string::lower(&pob_lua::gsub(tag, "%s+", "", None).0);
                                // L198
                                let negate_tag = pob_lua::string::sub(&tag_name, 1, Some(1)) == "-";
                                // L199
                                if negate_tag {
                                    tag_name =
                                        pob_lua::string::sub(&tag_name, 2, None).into_owned();
                                }
                                // L200-L210
                                match tag_name.as_str() {
                                    "active" => tag_name = "grants_active_skill".to_owned(),
                                    "int" => tag_name = "intelligence".to_owned(),
                                    "str" => tag_name = "strength".to_owned(),
                                    "dex" => tag_name = "dexterity".to_owned(),
                                    "aoe" => tag_name = "area".to_owned(),
                                    _ => {}
                                }
                                // L211 for :melee we want to exclude gems that DON'T have this tag
                                // L212 for :-melee we want to exclude gems that DO have this tag
                                // L213 EG: :active:fire:-aura		<-- No Anger (Calming ?)
                                let has = gem_data.tags.contains(&Name::from(tag_name.as_str()));
                                // L214
                                if negate_tag {
                                    // L215
                                    if has {
                                        add_this_gem = false;
                                    }
                                } else if !has {
                                    // L217
                                    add_this_gem = false;
                                }
                            }
                        }
                        // L221
                        if add_this_gem {
                            // L222
                            match_list.push(gem_id.clone());
                            // L223
                            added.insert(gem_id.clone());
                        }
                    } else {
                        // L226 This stanza is to support the original tag search
                        // L227 Name matching above failed, so lets use searchTerm to look for the tagName
                        // L228 aura:cold is now illogical and can't work (:aura:cold is the way to do it)
                        // L229-L237 (the rewrite persists for the later gems and patterns, as in Lua)
                        match search_term.as_str() {
                            "active" => search_term = "grants_active_skill".to_owned(),
                            "int" => search_term = "intelligence".to_owned(),
                            "str" => search_term = "strength".to_owned(),
                            "dex" => search_term = "dexterity".to_owned(),
                            _ => {}
                        }
                        // L238
                        if Self::filter_support(cx, me, gem_id, gem_data)
                            && !added.contains(gem_id)
                            && gem_data.tags.contains(&Name::from(
                                pob_lua::string::lower(&search_term).as_str(),
                            ))
                        {
                            // L239
                            match_list.push(gem_id.clone());
                            // L240
                            added.insert(gem_id.clone());
                        }
                    }
                }
                // L244
                Self::sort_gem_list(cx, me, &mut match_list);
                // L245-L247
                cx.w_mut::<Self>(me).list.extend(match_list);
            }
        } else {
            // L250 nothing in buffer
            // L251
            let mut list = Vec::new();
            for (gem_id, gem_data) in &gems {
                // L252
                if Self::filter_support(cx, me, gem_id, gem_data) {
                    // L253
                    list.push(gem_id.clone());
                }
            }
            cx.w_mut::<Self>(me).list = list;
            // L256
            let mut list = std::mem::take(&mut cx.w_mut::<Self>(me).list);
            Self::sort_gem_list(cx, me, &mut list);
            cx.w_mut::<Self>(me).list = list;
        }
        let w = cx.w_mut::<Self>(me);
        // L258
        if w.list.is_empty() {
            // L259
            w.list.push(String::new());
            // L260
            w.no_matches = true;
        } else {
            // L262
            w.no_matches = false;
        }
    }

    /// lua: GemSelectControl.lua:266-373 UpdateSortCache
    /// @pob Classes/GemSelectControl.lua:266-373 h=de90905d
    #[allow(clippy::too_many_lines)]
    pub fn update_sort_cache(cx: &mut Cx, me: CtrlId) {
        let model = Self::model(cx);
        let st = &model.skillsTab;
        let ui = st.ui_state();
        let display_group = Self::display_group(cx);
        let index = cx.w::<Self>(me).index;
        let show_types = ui.str(Name::from("showSupportGemTypes"));
        let show_legacy = ui.truthy(Name::from("showLegacyGems"));
        let sort_field = ui.str(Name::from("sortGemsByDPSField"));
        let default_level = st.defaultGemLevel();
        let default_quality = st.defaultGemQuality.get();
        let character_level = model.characterLevel.get();
        // L267 --local start = GetTime()
        // L268
        let sort_cache = cx.w::<Self>(me).sort_cache.clone();
        // L269
        let same_sort_by = {
            let w = cx.w::<Self>(me);
            w.sort_gems_by == w.last_sort_gems_by
        };
        let gem_instance = gem_at(&display_group, index);
        // L270 Don't update the cache if no settings have changed that would impact the ordering
        // L271-L275
        if let Some(sc) = &sort_cache {
            let sc = sc.borrow();
            if same_sort_by
                && sc
                    .socket_group
                    .as_ref()
                    .is_some_and(|g| Rc::ptr_eq(g, &display_group))
                && opt_rc_eq(sc.gem_instance.as_ref(), gem_instance.as_ref())
                && sc.output_revision == model.outputRevision.get()
                && sc.default_level == Some(default_level)
                && (sc.character_level == character_level
                    || default_level.as_str() != "characterLevel")
                && sc.default_quality == default_quality
                && sc.sort_type == sort_field
                && sc.consider_gem_type == show_types
                && sc.show_legacy_gems == show_legacy
            {
                // L276
                return;
            }
        }

        // L279-L283
        let repopulate = match &sort_cache {
            None => true,
            Some(sc) => {
                let sc = sc.borrow();
                !same_sort_by
                    || sc.consider_gem_type != show_types
                    || sc.show_legacy_gems != show_legacy
                    || sc.default_quality != default_quality
                    || sc.default_level != Some(default_level)
                    || (sc.character_level != character_level
                        && default_level.as_str() == "characterLevel")
            }
        };
        if repopulate {
            // L284
            let w = cx.w_mut::<Self>(me);
            w.last_sort_gems_by = w.sort_gems_by;
            // L285
            Self::populate_gem_list(cx, me);
        }

        // L288 Initialize a new sort cache
        // L289-L302
        let sort_cache = Rc::new(RefCell::new(SortCache {
            consider_gem_type: show_types,
            show_legacy_gems: show_legacy,
            socket_group: Some(display_group.clone()),
            gem_instance,
            output_revision: model.outputRevision.get(),
            default_level: Some(default_level),
            default_quality,
            character_level,
            sort_type: sort_field.clone(),
            ..SortCache::default()
        }));
        // L303
        cx.w_mut::<Self>(me).sort_cache = Some(sort_cache.clone());
        let imbued = cx.w::<Self>(me).imbued_select;
        let imbued_val = Val::Bool(true);
        let imbued_arg = imbued.then_some(&imbued_val);
        let gems: Vec<(String, GemDataRef)> = cx
            .w::<Self>(me)
            .gems
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let can = |ge: &pob_gamedata::GrantedEffect,
                   skills: &[pob_calc::active_skill::ActiveSkillRef]| {
            skills.iter().any(|active_skill| {
                pob_calc::modules::calc_tools::canGrantedEffectSupportActiveSkill(
                    ge,
                    active_skill,
                    imbued_arg,
                    false,
                )
            })
        };
        let add_can_support = |skills: &[pob_calc::active_skill::ActiveSkillRef]| {
            // ORDER: (c) a set
            for (gem_id, gem_data) in &gems {
                if gem_data.grantedEffect.support && can(&gem_data.grantedEffect, skills) {
                    sort_cache.borrow_mut().can_support.insert(gem_id.clone());
                }
            }
        };

        // L305 Determine supports that affect the active skill
        let dsl = display_group.displaySkillList.get().map(|l| l.snapshot());
        // L306
        if let Some(dsl) = dsl.filter(|l| !l.is_empty()) {
            // L307-L316
            add_can_support(&dsl);
        // L317 No active gem exists in the main socket group so check for item provided skills in matching slots
        } else if let Some(display_slot) = display_group.slot.get() {
            // L319
            for group in model.skillsTab.socketGroupList.get().snapshot() {
                let gdsl = group
                    .displaySkillList
                    .get()
                    .map(|l| l.snapshot())
                    .filter(|l| !l.is_empty());
                // L320
                let matching_item_skill_slot = group.source.get().is_some()
                    && group.slot.get().is_some_and(|s| s == display_slot)
                    && gdsl.is_some();
                // L321
                if matching_item_skill_slot {
                    // L322-L331
                    add_can_support(gdsl.as_deref().unwrap_or(&[]));
                }
                // L333
                let cross: Vec<Val> = match (group.slot.get(), &gdsl) {
                    (Some(_), Some(_)) => {
                        let main_env = model.calcsTab.mainEnv.get().unwrap_or_else(|| {
                            lua_error!("attempt to index field 'mainEnv' (a nil value)")
                        });
                        let key = pob_lua::gsub(display_slot.as_str(), " Swap", "", None).0;
                        let t = main_env
                            .crossLinkedSupportGroups
                            .get()
                            .get(Name::from(key.as_str()));
                        if t.truthy() {
                            t.table()
                                .unwrap_or_else(|| {
                                    lua_error!("bad argument #1 to 'ipairs' (table expected)")
                                })
                                .ipairs()
                                .into_iter()
                                .map(|(_, v)| v)
                                .collect()
                        } else {
                            Vec::new()
                        }
                    }
                    _ => Vec::new(),
                };
                for cross_linked_slot in cross {
                    // L334
                    let group_slot = group.slot.get().unwrap_or_else(|| {
                        lua_error!("attempt to index field 'slot' (a nil value)")
                    });
                    let gs = pob_lua::gsub(group_slot.as_str(), " Swap", "", None).0;
                    if cross_linked_slot.str().is_some_and(|c| *c == *gs) {
                        // L335-L344
                        add_can_support(gdsl.as_deref().unwrap_or(&[]));
                    }
                }
            }
        }

        // L350
        let dps_field = sort_field;
        // L351
        let use_full_dps = dps_field.as_deref() == Some("FullDPS");
        // L352 (Pending: the placeholder of the module notes; a calculator that is not ready
        // leaves the DPS at 0 and no gem pending)
        let calc = cx.build_mut().calc.misc_calculator(&model);
        let (calc_func, calc_base) = match calc {
            Poll::Ready((f, b)) => (Some(f), b),
            Poll::Pending => (None, Output::default()),
        };
        // L353 Check for nil because some fields may not be populated, default to 0
        // L354
        let base_dps = dps_of(&calc_base, dps_field.as_ref());

        {
            let mut sc = sort_cache.borrow_mut();
            // L356
            sc.calc_func = calc_func.clone();
            // L357
            sc.use_full_dps = use_full_dps;
            // L358
            sc.base_dps = base_dps;
            // L359
            sc.dps_field = dps_field;
            // L360
            sc.pending_gems = Some(Vec::new());
            sc.base = Some(calc_base);
        }

        // L362 ORDER: (c), module notes
        for (gem_id, gem_data) in &gems {
            let mut sc = sort_cache.borrow_mut();
            // L363
            sc.dps.insert(gem_id.clone(), base_dps);
            // L364 Gems that support the active skill or have global effects need DPS calc
            // L365
            if calc_func.is_some()
                && (sc.can_support.contains(gem_id)
                    || (pob_gamedata::skill_stat_map::hasGlobalEffect(&gem_data.grantedEffect)
                        && !gem_data.grantedEffect.support))
            {
                // L366
                sc.pending_gems
                    .as_mut()
                    .expect("set above")
                    .push(gem_id.clone());
            }
            // L368 Neutral color until DPS is computed
            // L369
            sc.dps_color.insert(gem_id.clone(), String::new());
        }

        // L372
        cx.w_mut::<Self>(me).dps_build_flag = true;
    }

    /// lua: GemSelectControl.lua:375-395 SortGemList(gemList)
    /// @pob Classes/GemSelectControl.lua:375-395 h=4419e5ae
    pub fn sort_gem_list(cx: &Cx, me: CtrlId, gem_list: &mut [String]) {
        // L376
        let sort_cache = Self::sort_cache_of(cx, me);
        let sort_cache = sort_cache.borrow();
        // L377
        let gems = &cx.w::<Self>(me).gems;
        // L378 cache names to avoid repeated table lookups in comparator
        // L379-L383
        let names: std::collections::HashMap<&str, String> = gem_list
            .iter()
            .map(|gem_id| {
                let gem = gems.get(gem_id);
                (
                    gem_id.as_str(),
                    gem.map_or_else(|| gem_id.clone(), |g| g.name.to_string()),
                )
            })
            .collect();
        let names: std::collections::HashMap<String, String> =
            names.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
        let by_dps = Self::model(cx)
            .skillsTab
            .ui_state()
            .truthy(Name::from("sortGemsByDPS"));
        // L384
        pob_lua::sort(gem_list, |a, b| {
            let (ca, cb) = (
                sort_cache.can_support.contains(a),
                sort_cache.can_support.contains(b),
            );
            // L385
            if ca == cb {
                let (da, db) = (
                    sort_cache.dps.get(a).copied(),
                    sort_cache.dps.get(b).copied(),
                );
                // L386
                if by_dps && da != db {
                    // L387 (a nil DPS compared with a number is Lua's compare error)
                    let (Some(da), Some(db)) = (da, db) else {
                        lua_error!("attempt to compare nil with number")
                    };
                    da > db
                } else {
                    // L389
                    names[a] < names[b]
                }
            } else {
                // L392
                ca
            }
        });
    }

    /// lua: GemSelectControl.lua:397-406 SyncSelection
    /// @pob Classes/GemSelectControl.lua:397-406 h=66f9317e
    pub fn sync_selection(cx: &mut Cx, me: CtrlId) {
        // L398
        cx.w_mut::<Self>(me).sel_index = Some(0.0);
        let buf_lower = pob_lua::string::lower(&Self::buf(cx, me));
        let list = cx.w::<Self>(me).list.clone();
        // L399
        for (i0, gem_id) in list.iter().enumerate() {
            let hit = cx
                .w::<Self>(me)
                .gems
                .get(gem_id)
                .is_some_and(|g| pob_lua::string::lower(g.name.as_str()) == buf_lower);
            // L400
            if hit {
                // L401
                cx.w_mut::<Self>(me).sel_index = Some((i0 + 1) as f64);
                // L402
                Self::scroll_sel_into_view(cx, me);
                // L403
                break;
            }
        }
    }

    /// lua: GemSelectControl.lua:408-413 SortCurrentList
    /// @pob Classes/GemSelectControl.lua:408-413 h=f8da9c65
    pub fn sort_current_list(cx: &mut Cx, me: CtrlId) {
        // L409
        let empty = cx
            .w::<Self>(me)
            .search_str
            .as_ref()
            .unwrap_or_else(|| {
                lua_error!("attempt to get length of field 'searchStr' (a nil value)")
            })
            .is_empty();
        if empty {
            // L410
            let mut list = std::mem::take(&mut cx.w_mut::<Self>(me).list);
            Self::sort_gem_list(cx, me, &mut list);
            cx.w_mut::<Self>(me).list = list;
            // L411
            Self::sync_selection(cx, me);
        }
    }

    /// lua: GemSelectControl.lua:415-452 DPSBuilder (the coroutine body)
    /// @pob Classes/GemSelectControl.lua:415-452 h=2d41da1e
    async fn dps_builder(me: CtrlId) {
        // L416
        let sort_cache = co::with_cx(|cx| cx.w::<Self>(me).sort_cache.clone());
        // L417
        let Some(sort_cache) = sort_cache else { return };
        let Some(pending) = sort_cache.borrow().pending_gems.clone() else {
            return;
        };

        // L419-L423
        let (calc_func, use_full_dps, base_dps, dps_field) = {
            let sc = sort_cache.borrow();
            (
                sc.calc_func.clone(),
                sc.use_full_dps,
                sc.base_dps,
                sc.dps_field.clone(),
            )
        };
        // L424
        let mut start = co::with_cx(|cx| cx.sys.time_ms() as f64);

        let n = pending.len() as f64;
        // L426
        for (i0, gem_id) in pending.iter().enumerate() {
            let index = (i0 + 1) as f64;
            let yielded = co::with_cx(|cx| {
                // L427
                let gem_data = cx.w::<Self>(me).gems.get(gem_id).cloned();
                // L428
                if let Some(gem_data) = gem_data {
                    // L429
                    let calc_func = calc_func.clone().unwrap_or_else(|| {
                        lua_error!("attempt to call local 'calcFunc' (a nil value)")
                    });
                    let (output, _) = Self::calc_output_with_this_gem(
                        cx,
                        me,
                        &calc_func,
                        &gem_data,
                        use_full_dps,
                    );
                    // L430
                    let dps = dps_of(&output, dps_field.as_ref());
                    let mut sc = sort_cache.borrow_mut();
                    sc.dps.insert(gem_id.clone(), dps);
                    // L431
                    let color = if dps > base_dps {
                        // L432
                        "^x228866"
                    } else if dps < base_dps {
                        // L434
                        "^xFF4422"
                    } else {
                        // L436
                        "^xFFFF66"
                    };
                    sc.dps_color.insert(gem_id.clone(), color.to_owned());
                }
                // L439
                let now = cx.sys.time_ms() as f64;
                // L440
                if now - start > 50.0 {
                    // L441
                    Self::sort_current_list(cx, me);
                    // L442
                    if let Some(cb) = cx.w::<Self>(me).dps_builder_callback.clone() {
                        // L443
                        cb(cx, m_floor(index / n * 100.0));
                    }
                    Some(now)
                } else {
                    None
                }
            });
            if let Some(now) = yielded {
                // L445
                co::yield_now().await;
                // L446
                start = now;
            }
        }

        // L450
        co::with_cx(|cx| Self::sort_current_list(cx, me));
        // L451
        sort_cache.borrow_mut().pending_gems = None;
    }

    /// lua: GemSelectControl.lua:454-470 UpdateGem(setText, addUndo, focusLost)
    /// @pob Classes/GemSelectControl.lua:454-470 h=e4964cba
    pub fn update_gem(cx: &mut Cx, me: CtrlId, set_text: bool, add_undo: bool, focus_lost: bool) {
        let buf = Self::buf(cx, me);
        // L455
        let gem_id = {
            let w = cx.w::<Self>(me);
            let i = m_max(
                w.sel_index.unwrap_or_else(|| {
                    lua_error!("bad argument #1 to 'max' (number expected, got nil)")
                }),
                1.0,
            );
            w.list.get(i as usize - 1).cloned()
        };
        let gem = gem_id
            .as_ref()
            .and_then(|g| cx.w::<Self>(me).gems.get(g).cloned());
        // L456 don't process unless the buffer equals an actual gem, whether typed, clicked, or navigated with arrows
        // L457 we don't nil the gemId here if it doesn't match because the imbuedGemSelect and slotGemSelect have different paths
        // L458
        let buf_matches_gem = gem.as_ref().is_some_and(|g| {
            pob_lua::string::lower(&buf) == pob_lua::string::lower(g.name.as_str())
        });

        // L460
        let new_gem_id = if pob_lua::lmatch(&buf, "%S", None).is_some() && gem.is_some() {
            // L461
            gem_id.clone().map(GemIdVal::Key)
        } else {
            // L463
            None
        };
        cx.w_mut::<Self>(me).gem_id = new_gem_id.clone();
        // L465
        let gem_name = if buf_matches_gem {
            match &new_gem_id {
                Some(GemIdVal::Key(k)) => cx.w::<Self>(me).gems.get(k).map(|g| g.name.to_string()),
                _ => None,
            }
            .unwrap_or_default()
        } else {
            String::new()
        };
        cx.w_mut::<Self>(me).gem_name = Some(gem_name.clone());
        // L466
        if set_text {
            // L467
            EditControl::set_text(cx, me, &gem_name, false);
        }
        // L469
        let arg = match &new_gem_id {
            Some(GemIdVal::Key(k)) => Some(pob_lua::gsub(k, "%w+:", "", None).0),
            Some(GemIdVal::Data(_)) | None => None,
        };
        let add = add_undo && Some(&gem_name) != cx.w::<Self>(me).initial_buf.as_ref();
        let f =
            cx.w::<Self>(me).gem_change_func.clone().unwrap_or_else(|| {
                lua_error!("attempt to call field 'gemChangeFunc' (a nil value)")
            });
        f(cx, arg, add, focus_lost, buf_matches_gem);
    }

    /// lua: GemSelectControl.lua:472-478 ScrollSelIntoView
    /// @pob Classes/GemSelectControl.lua:472-478 h=bf6cbe82
    pub fn scroll_sel_into_view(cx: &mut Cx, me: CtrlId) {
        // L473
        let (_width, height) = cx.size(me);
        // L474
        let scroll_bar = Self::sb(cx, me);
        let n = cx.w::<Self>(me).list.len() as f64;
        // L475
        let drop_height = (height - 4.0) * m_min(n, 15.0);
        // L476
        ScrollBarControl::set_content_dimension(
            cx,
            scroll_bar.id(),
            (height - 4.0) * n,
            drop_height,
        );
        // L477
        let sel = cx.w::<Self>(me).sel_index.unwrap_or_else(|| {
            lua_error!("attempt to perform arithmetic on field 'selIndex' (a nil value)")
        });
        ScrollBarControl::scroll_into_view(
            cx,
            scroll_bar.id(),
            (sel - 2.0) * (height - 4.0),
            3.0 * (height - 4.0),
        );
    }

    /// lua: GemSelectControl.lua:480-501 IsMouseOver: `mOver, mOverComp` ("BODY" / "DROP").
    /// @pob Classes/GemSelectControl.lua:480-501 h=7f17964f
    pub fn is_mouse_over_comp(cx: &Cx, me: CtrlId) -> (bool, Option<&'static str>) {
        // L481
        if !cx.is_shown(me) {
            // L482
            return (false, None);
        }
        // L484
        if cx.get_mouse_over_control(me).is_some() {
            // L485
            return (true, None);
        }
        // L487
        let (x, y) = cx.pos(me);
        // L488
        let (width, height) = cx.size(me);
        // L489
        let (cursor_x, cursor_y) = cx.sys.cursor_pos();
        let w = cx.w::<Self>(me);
        // L490
        let drop_extra = if w.dropped {
            (height - 4.0) * m_min(w.list.len() as f64, 15.0) + 2.0
        } else {
            0.0
        };
        // L491
        let m_over = cursor_x >= x
            && cursor_y >= y
            && cursor_x < x + width
            && cursor_y < y + height + drop_extra;
        // L492
        let mut m_over_comp = None;
        // L493
        if m_over {
            // L494
            if cursor_y < y + height {
                // L495
                m_over_comp = Some("BODY");
            } else {
                // L497
                m_over_comp = Some("DROP");
            }
        }
        // L500
        (m_over, m_over_comp)
    }

    /// lua: GemSelectControl.lua:701-704 CheckSupporting(gemA, gemB)
    /// @pob Classes/GemSelectControl.lua:701-704 h=8290c16a
    pub fn check_supporting(gem_a: &GemInstanceRef, gem_b: &GemInstanceRef) -> bool {
        let (Some(a_data), Some(b_data)) = (gem_a.gemData.get(), gem_b.gemData.get()) else {
            lua_error!("attempt to index field 'gemData' (a nil value)")
        };
        let supporting = || {
            gem_a
                .supportEffect
                .get()
                .and_then(|e| e.isSupporting.get())
                .is_some_and(|s| s.with(|s| s.contains(gem_b)))
        };
        // L702
        (a_data.grantedEffect.support && !b_data.grantedEffect.support && supporting())
            // L703
            || (a_data.secondaryGrantedEffect.as_ref().is_some_and(|s| s.support)
                && !b_data.grantedEffect.support
                && supporting())
    }

    /// lua: GemSelectControl.lua:706-708 AddGemTooltip(gemInstance)
    /// @pob Classes/GemSelectControl.lua:706-708 h=05bac6bb
    fn add_gem_tooltip(cx: &mut Cx, tooltip: &mut Tooltip, gem_instance: &TooltipGemInstance) {
        // L707
        let model = Self::model(cx);
        gem_tooltip::add_gem_tooltip(cx, tooltip, &model, gem_instance, None);
    }

    /// lua: GemSelectControl.lua:719-725 CancelSelection
    /// @pob Classes/GemSelectControl.lua:719-725 h=4504e51d
    pub fn cancel_selection(cx: &mut Cx, me: CtrlId) {
        // L720
        cx.w_mut::<Self>(me).dropped = false;
        // L721 (a raw write: caret and selection stay)
        let initial = cx
            .w::<Self>(me)
            .initial_buf
            .clone()
            .unwrap_or_else(|| lua_error!("attempt to index field 'buf' (a nil value)"));
        cx.w_mut::<EditControl>(me).buf = initial;
        // L722
        Self::build_list(cx, me, "");
        // L723
        Self::sync_selection(cx, me);
        // L724
        Self::update_gem(cx, me, false, true, true);
    }

    /// `self.gems[self.list[i]]`
    fn gem_of_list(cx: &Cx, me: CtrlId, i: f64) -> Option<GemDataRef> {
        let w = cx.w::<Self>(me);
        if i >= 1.0 && i.fract() == 0.0 {
            w.list
                .get(i as usize - 1)
                .and_then(|id| w.gems.get(id).cloned())
        } else {
            None
        }
    }
}

impl SortCache {
    /// The base output the placeholder returns for a Pending call (module notes).
    fn base_output(&self) -> Output {
        self.base.clone().unwrap_or_default()
    }
}

/// `a == b` for two optional tables (identity).
fn opt_rc_eq<T>(a: Option<&Rc<T>>, b: Option<&Rc<T>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

// ---- itemLib.wiki (ItemTools.lua:384-413; the calc's item_tools.rs skips it for the UI) -------

/// lua: ItemTools.lua:386-398 `itemLib.wiki.openGem(gemData)` (CCR-0108 §2; the range is covered
/// by IT-B's `@pob-skip`, so this port carries no second marker).
pub fn wiki_open_gem(cx: &mut Cx, gem_data: &GemDataRef) {
    // L388-L392 (a gem of `data.gems` always has a name)
    let mut name = gem_data.name.to_string();
    if gem_data.tags.contains(&Name::from("support")) {
        name += " Support";
    }
    // L397
    wiki_open(cx, &name);
}

/// lua: ItemTools.lua:404-409 `itemLib.wiki.open(name)`
pub fn wiki_open(cx: &mut Cx, name: &str) {
    // L405
    let route = pob_lua::gsub(name, " ", "_", None).0;
    // L407
    cx.sys
        .platform
        .open_url(&format!("https://www.poewiki.net/wiki/{route}"));
    // L408
    cx.app_mut().main.item_lib_wiki_triggered = true;
}

/// lua: ItemTools.lua:410-412 `itemLib.wiki.matchesKey(key)` (`itemLib.wiki.key = "F1"`, L385)
pub fn wiki_matches_key(key: Key) -> bool {
    key == Key::F(1)
}

impl Widget for GemSelectControl {
    pob_ui_core::impl_parts!(GemSelectControl: edit => EditControl);
    const NAME: &'static str = "GemSelectControl";
    const CAPS: Caps = <EditControl as Widget>::CAPS.union(Caps::FOCUS_LOST);

    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        Self::is_mouse_over_comp(cx, me).0
    }

    /// lua: GemSelectControl.lua:503-699 Draw(viewPort, noTooltip)
    /// @pob Classes/GemSelectControl.lua:503-699 h=26e3ba45
    #[allow(clippy::too_many_lines)]
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        // L504
        if cx.w::<Self>(me).sort_percentage.is_none() {
            cx.w_mut::<Self>(me).sort_percentage = Some(String::new());
        }
        // L505
        if cx.w::<Self>(me).dps_build_flag {
            // L506
            cx.w_mut::<Self>(me).dps_build_flag = false;
            // L507
            cx.w_mut::<Self>(me).dps_builder = Some(Coroutine::create(Self::dps_builder(me)));
            // L508-L510
            cx.w_mut::<Self>(me).dps_builder_callback =
                Some(Rc::new(move |cx: &mut Cx, percentage: f64| {
                    // L509
                    cx.w_mut::<Self>(me).sort_percentage = Some(lfmt!("%d%%", percentage));
                }));
        }
        // L512
        if cx.w::<Self>(me).dps_builder.is_some() {
            // L513
            let (res, err_msg, dead) = cx.lend::<Self, _, _>(
                me,
                |w| &mut w.dps_builder,
                |cx, co| {
                    let co = co.as_mut().expect("checked above");
                    #[cfg(not(target_arch = "wasm32"))]
                    let (res, err) = co.try_resume(cx);
                    #[cfg(target_arch = "wasm32")]
                    let (res, err) = (co.resume(cx), None::<String>);
                    (res, err, co.is_dead())
                },
            );
            // L514
            if cx.app().launch.dev_mode && !res {
                // L515
                lua_error!("{}", err_msg.unwrap_or_default());
            }
            // L517
            if dead {
                // L518
                cx.w_mut::<Self>(me).dps_builder = None;
            }
        }

        // L522
        let force_tooltip = cx.w::<Self>(me).force_tooltip;
        EditControl::draw(cx, me, vp, no_tooltip && !force_tooltip);
        // L523
        let (x, y) = cx.pos(me);
        // L524
        let (width, height) = cx.size(me);
        // L525
        let _enabled = cx.is_enabled(me);
        // L526
        let (m_over, m_over_comp) = Self::is_mouse_over_comp(cx, me);
        let list_len = cx.w::<Self>(me).list.len() as f64;
        // L527
        let drop_height = (height - 4.0) * m_min(list_len, 15.0);
        // L528
        let scroll_bar = Self::sb(cx, me);
        // L529
        ScrollBarControl::set_content_dimension(
            cx,
            scroll_bar.id(),
            (height - 4.0) * list_len,
            drop_height,
        );
        let dropped = cx.w::<Self>(me).dropped;
        // L530
        if dropped {
            // L531
            cx.sg.set_draw_layer(None, Some(5));
            // L532
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
            // L533
            cx.sg
                .draw_image(None, x, y + height, width, drop_height + 4.0);
            // L534
            cx.sg.set_draw_color(0.0, 0.0, 0.0);
            // L535
            cx.sg.draw_image(
                None,
                x + 1.0,
                y + height + 1.0,
                width - 2.0,
                drop_height + 2.0,
            );
            // L536
            cx.sg.set_draw_layer(None, Some(0));
        }
        // L538
        if dropped {
            // L539
            cx.sg.set_draw_layer(None, Some(5));
            // L540
            if cx.w::<Self>(me).dps_builder.is_some() {
                // L541
                cx.sg.set_draw_color(0.75, 0.75, 0.75);
                // L542
                let s = format!(
                    "Sorting {}",
                    cx.w::<Self>(me).sort_percentage.clone().unwrap_or_default()
                );
                cx.sg.draw_string(
                    x + width - 4.0,
                    y,
                    Align::RightX,
                    height - 2.0,
                    Font::Var,
                    &s,
                );
            }
            // L544
            let (_cursor_x, cursor_y) = cx.sys.cursor_pos();
            let offset = cx.get(scroll_bar).offset;
            // L545
            let mut hover_sel = if m_over_comp == Some("DROP") {
                Some(m_floor((cursor_y - y - height + offset) / (height - 4.0)) + 1.0)
            } else {
                None
            };
            // L546
            if let Some(hs) = hover_sel {
                if Self::gem_of_list(cx, me, hs).is_none() {
                    // L547
                    hover_sel = None;
                }
            }
            cx.w_mut::<Self>(me).hover_sel = hover_sel;
            // L549
            cx.sg
                .set_viewport(x + 2.0, y + height + 2.0, width - 4.0, drop_height);
            // L550
            let min_index = m_floor(offset / 16.0 + 1.0);
            // L551
            let max_index = m_min(m_floor((offset + drop_height) / 16.0 + 1.0), list_len);
            let sel_index = cx.w::<Self>(me).sel_index;
            let sort_cache = Self::sort_cache_of(cx, me);
            let sb_enabled = ScrollBarControl::enabled_raw(cx, scroll_bar.id());
            // L552
            let mut index = min_index;
            while index <= max_index {
                // L553
                let y = (index - 1.0) * (height - 4.0) - offset;
                // L554
                if Some(index) == hover_sel
                    || Some(index) == sel_index
                    || (index == 1.0 && sel_index == Some(0.0))
                {
                    // L555
                    cx.sg.set_draw_color(0.2, 0.2, 0.2);
                    // L556
                    cx.sg.draw_image(None, 0.0, y, width - 4.0, height - 4.0);
                }
                // L558
                cx.sg.set_draw_color(1.0, 1.0, 1.0);
                // L559
                let gem_id = if index >= 1.0 && index.fract() == 0.0 {
                    cx.w::<Self>(me).list.get(index as usize - 1).cloned()
                } else {
                    None
                };
                // L560
                let gem_data = gem_id
                    .as_ref()
                    .and_then(|g| cx.w::<Self>(me).gems.get(g).cloned());
                // L561
                if let Some(gd) = &gem_data {
                    // L562-L568
                    match gd.grantedEffect.color {
                        Some(1) => {
                            let c = cx.main.cc.STRENGTH.clone();
                            cx.sg.set_draw_color_esc(&c);
                        }
                        Some(2) => {
                            let c = cx.main.cc.DEXTERITY.clone();
                            cx.sg.set_draw_color_esc(&c);
                        }
                        Some(3) => {
                            let c = cx.main.cc.INTELLIGENCE.clone();
                            cx.sg.set_draw_color_esc(&c);
                        }
                        _ => {}
                    }
                }
                // L570
                let gem_text = gem_data
                    .as_ref()
                    .map_or_else(|| "<No matches>".to_owned(), |g| g.name.to_string());
                // L571
                cx.sg
                    .draw_string(0.0, y, Align::Left, height - 4.0, Font::Var, &gem_text);
                // L572
                if let Some(gd) = &gem_data {
                    let gem_id = gem_id.as_deref().unwrap_or_default();
                    let (can_support, color) = {
                        let sc = sort_cache.borrow();
                        (
                            sc.can_support.contains(gem_id),
                            sc.dps_color.get(gem_id).cloned(),
                        )
                    };
                    let has_color = color.as_deref().map_or(true, |c| !c.is_empty());
                    let mark_x = width - 4.0 - height / 2.0 - if sb_enabled { 18.0 } else { 0.0 };
                    // L573
                    if gd.grantedEffect.support && can_support && has_color {
                        // L574
                        set_color_val(cx, color.as_deref());
                        // L575
                        draw_check_mark(cx, mark_x, y + (height - 4.0) / 2.0, (height - 4.0) * 0.8);
                    } else if pob_gamedata::skill_stat_map::hasGlobalEffect(&gd.grantedEffect)
                        && has_color
                    {
                        // L577
                        set_color_val(cx, color.as_deref());
                        // L578
                        cx.sg
                            .draw_string(mark_x, y - 2.0, Align::CenterX, height, Font::Var, "+");
                    }
                }
                index += 1.0;
            }
            // L582
            cx.sg.reset_viewport();
            // L583
            let sel = if no_tooltip && !force_tooltip {
                Some(me)
            } else {
                None
            };
            cx.draw_controls(me, vp, sel);
            // L584
            if let Some(hover_sel) = hover_sel {
                // L585 Debounce hover
                let w = cx.w_mut::<Self>(me);
                // L586
                if Some(hover_sel) == w.last_hover_sel {
                    // L587
                    w.hover_frame_count = Some(w.hover_frame_count.unwrap_or(0.0) + 1.0);
                } else {
                    // L589
                    w.last_hover_sel = Some(hover_sel);
                    // L590
                    w.hover_frame_count = Some(0.0);
                }
                // L592
                if w.hover_frame_count.unwrap_or(0.0) >= 2.0 {
                    // L593
                    let model = Self::model(cx);
                    let calc = cx.build_mut().calc.misc_calculator(&model);
                    // L594 (Pending: no tooltip this frame, module notes)
                    if let Poll::Ready((calc_func, calc_base)) = calc {
                        cx.lend::<EditControl, Tooltip, _>(
                            me,
                            |w| &mut w.tt.tooltip,
                            |cx, tooltip| {
                                // L595
                                tooltip.clear(false);
                                // L596
                                let gem_data =
                                    Self::gem_of_list(cx, me, hover_sel).unwrap_or_else(|| {
                                        lua_error!("attempt to index local 'gemData' (a nil value)")
                                    });
                                // L597
                                let full = model
                                    .skillsTab
                                    .ui_state()
                                    .str(Name::from("sortGemsByDPSField"));
                                let (output, _) = Self::calc_output_with_this_gem(
                                    cx,
                                    me,
                                    &calc_func,
                                    &gem_data,
                                    full.as_deref() == Some("FullDPS"),
                                );
                                // L598-L611 (qualityId, count, enabled, enableGlobal1/2, gemId, nameSpec and
                                // skillId are not read by GemTooltip)
                                let gem_instance = TooltipGemInstance {
                                    level: Self::process_gem_level(cx, me, &gem_data),
                                    quality: model.skillsTab.defaultGemQuality.get().unwrap_or(0.0),
                                    display_effect: None,
                                    gem_data: TooltipGemData::Gem(gem_data),
                                };
                                // L612
                                Self::add_gem_tooltip(cx, tooltip, &gem_instance);
                                // L613
                                tooltip.add_separator(cx, Some(10.0));
                                // L614
                                stats::add_stat_compares_to_tooltip(
                                    cx,
                                    tooltip,
                                    &calc_base,
                                    &output,
                                    "^7Selecting this gem will give you:",
                                    None,
                                );
                                // L615
                                let offset = cx.get(scroll_bar).offset;
                                tooltip.draw(
                                    cx,
                                    x,
                                    y + height + 2.0 + (hover_sel - 1.0) * (height - 4.0) - offset,
                                    Some(width),
                                    Some(height - 4.0),
                                    vp,
                                );
                            },
                        );
                    }
                }
            } else {
                let w = cx.w_mut::<Self>(me);
                // L619
                w.last_hover_sel = None;
                // L620
                w.hover_frame_count = Some(0.0);
            }
            // L622
            cx.sg.set_draw_layer(None, Some(0));
        } else {
            // L624 not dropped
            let st = cx.w::<Self>(me).skills_tab;
            let is_gem_select = |cx: &Cx, c: CtrlId| cx.ui.class_name(c) == "GemSelectControl";
            // L625
            let hover_control: Option<CtrlId>;
            // L626
            let st_sel = cx.host(st).sel_control;
            if let Some(sel) = st_sel.filter(|&c| is_gem_select(cx, c)) {
                // L627
                hover_control = Some(sel);
            } else {
                // L629
                hover_control = cx.get_mouse_over_control(st);
            }
            // L631
            if let Some(hc) = hover_control.filter(|&c| is_gem_select(cx, c)) {
                let display_group = Self::display_group(cx);
                // L632
                let this_gem = gem_at(&display_group, cx.w::<Self>(me).index);
                // L633
                let hover_gem = gem_at(&display_group, cx.w::<Self>(hc).index);
                // L634-L635
                if let (Some(this_gem), Some(hover_gem)) = (this_gem, hover_gem) {
                    if this_gem.enabled.get()
                        && hover_gem.enabled.get()
                        && this_gem.gemData.get().is_some()
                        && hover_gem.gemData.get().is_some()
                        && (Self::check_supporting(&this_gem, &hover_gem)
                            || Self::check_supporting(&hover_gem, &this_gem))
                    {
                        // L636
                        cx.sg.set_draw_color_a(0.33, 1.0, 0.33, 0.25);
                        // L637
                        cx.sg.draw_image(None, x, y, width, height);
                    }
                }
            }
            // L640
            let st_sel = cx.host(st).sel_control;
            let sel_ok = match st_sel {
                None => true,
                Some(c) => !is_gem_select(cx, c) || !cx.w::<Self>(c).dropped,
            };
            if m_over && sel_ok && (!no_tooltip || force_tooltip) {
                let display_group = Self::display_group(cx);
                // L641
                let gem_instance = gem_at(&display_group, cx.w::<Self>(me).index);
                // L642
                let (cursor_x, _cursor_y) = cx.sys.cursor_pos();
                let imbued_select = cx.w::<Self>(me).imbued_select;
                cx.lend::<EditControl, Tooltip, _>(
                    me,
                    |w| &mut w.tt.tooltip,
                    |cx, tooltip| {
                        // L643
                        tooltip.clear(false);

                        // the instance GemTooltip receives (L641 or L646-L653)
                        let mut tip: Option<TooltipGemInstance> =
                            gem_instance.as_ref().and_then(|g| {
                                g.gemData.get().map(|gd| TooltipGemInstance::of_gem(g, gd))
                            });
                        // L645 tooltip for imbued
                        if let Some(hc) = hover_control
                            .filter(|&c| cx.is::<Self>(c) && cx.w::<Self>(c).imbued_select)
                        {
                            // L646-L651
                            let gem_data = match &cx.w::<Self>(hc).gem_id {
                                // L647-L648 on select
                                Some(GemIdVal::Key(k)) => cx.w::<Self>(hc).gems.get(k).cloned(),
                                // L649-L650 on load
                                Some(GemIdVal::Data(d)) => Some(d.clone()),
                                None => None,
                            };
                            // L652-L653
                            tip = gem_data.map(|gd| TooltipGemInstance {
                                gem_data: TooltipGemData::Gem(gd),
                                level: 1.0,
                                quality: 0.0,
                                display_effect: None,
                            });
                        }

                        // L656
                        if let Some(tip) = &tip {
                            // L657
                            Self::add_gem_tooltip(cx, tooltip, tip);
                        } else {
                            // L659
                            let text = if imbued_select {
                                IMBUED_TOOLTIP_TEXT
                            } else {
                                TOOL_TIP_TEXT
                            };
                            tooltip.add_line(cx, 16.0, text);
                        }

                        // L662
                        if !imbued_select {
                            // L663-L664 (globals in the Lua, module notes)
                            let mut color_s = 0.5;
                            let mut color_a = 0.5;
                            // L665
                            if cursor_x > (x + width - 18.0) {
                                // L666
                                color_s = 1.0;
                                // L667
                                tooltip.clear(false);
                                // L668
                                tooltip.add_line(cx, 16.0, "Only show Support gems");
                            } else if cursor_x > (x + width - 40.0)
                                && cursor_x < (cursor_x + width - 20.0)
                            {
                                // L670
                                color_a = 1.0;
                                // L671
                                tooltip.clear(false);
                                // L672
                                tooltip.add_line(cx, 16.0, "Only show Active gems");
                            }

                            // L675 support shortcut
                            // L676
                            let sx = x + width - 16.0 - 2.0;
                            // L677
                            cx.sg.set_draw_color(color_s, color_s, color_s);
                            // L678
                            cx.sg.draw_image(None, sx, y + 2.0, 16.0, height - 4.0);
                            // L679
                            cx.sg.set_draw_color(0.0, 0.0, 0.0);
                            // L680
                            cx.sg
                                .draw_image(None, sx + 1.0, y + 2.0, 16.0 - 2.0, height - 4.0);
                            // L681
                            cx.sg.set_draw_color(color_s, color_s, color_s);
                            // L682
                            cx.sg.draw_string(
                                sx + 8.0,
                                y,
                                Align::CenterX,
                                height - 2.0,
                                Font::Var,
                                "S",
                            );

                            // L684 active shortcut
                            // L685
                            let sx = x + width - (16.0 * 2.0) - (2.0 * 2.0);
                            // L686
                            cx.sg.set_draw_color(color_a, color_a, color_a);
                            // L687
                            cx.sg.draw_image(None, sx, y + 2.0, 16.0, height - 4.0);
                            // L688
                            cx.sg.set_draw_color(0.0, 0.0, 0.0);
                            // L689
                            cx.sg
                                .draw_image(None, sx + 1.0, y + 2.0, 16.0 - 2.0, height - 4.0);
                            // L690
                            cx.sg.set_draw_color(color_a, color_a, color_a);
                            // L691
                            cx.sg.draw_string(
                                sx + 8.0,
                                y,
                                Align::CenterX,
                                height - 2.0,
                                Font::Var,
                                "A",
                            );
                        }

                        // L694
                        cx.sg.set_draw_layer(None, Some(10));
                        // L695
                        tooltip.draw(cx, x, y, Some(width), Some(height), vp);
                        // L696
                        cx.sg.set_draw_layer(None, Some(0));
                    },
                );
            }
        }
    }

    /// lua: GemSelectControl.lua:710-717 OnFocusGained
    /// @pob Classes/GemSelectControl.lua:710-717 h=c48feed1
    fn on_focus_gained(cx: &mut Cx, me: CtrlId) {
        // L711
        <EditControl as Widget>::on_focus_gained(cx, me);
        // L712
        cx.w_mut::<Self>(me).dropped = true;
        // L713
        Self::update_sort_cache(cx, me);
        // L714
        Self::build_list(cx, me, "");
        // L715
        Self::sync_selection(cx, me);
        // L716
        let buf = Self::buf(cx, me);
        cx.w_mut::<Self>(me).initial_buf = Some(buf);
    }

    /// lua: GemSelectControl.lua:727-731 OnFocusLost
    /// @pob Classes/GemSelectControl.lua:727-731 h=209b4f31
    fn on_focus_lost(cx: &mut Cx, me: CtrlId) {
        // L728
        if cx.w::<Self>(me).dropped {
            // L729
            Self::cancel_selection(cx, me);
        }
    }

    /// lua: GemSelectControl.lua:733-819 OnKeyDown(key, doubleClick)
    /// @pob Classes/GemSelectControl.lua:733-819 h=6529c919
    #[allow(clippy::too_many_lines)]
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, double_click: bool) -> Focus {
        // L734
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L735
            return None;
        }

        // L738 for filter overlay buttons
        // L739
        let (x, y) = cx.pos(me);
        // L740
        let (width, height) = cx.size(me);
        // L741
        let (cursor_x, cursor_y) = cx.sys.cursor_pos();
        // L742 constrain cursor to the height of the control
        // L743
        if !cx.w::<Self>(me).imbued_select
            && key == Key::LeftButton
            && (cursor_y > y && cursor_y < (y + height))
        {
            // L744 no need to constrain right side of the S overlay as that's outside hover
            // L745
            let sort_by = if cursor_x > (x + width - 18.0) {
                // L746 only need to change sortBy, code will continue to UpdateSortCache
                Some(Name::from("support"))
            } else if cursor_x > (x + width - 40.0) && cursor_x < (cursor_x + width - 20.0) {
                // L748
                Some(Name::from("grants_active_skill"))
            } else {
                // L750
                None
            };
            cx.w_mut::<Self>(me).sort_gems_by = sort_by;
        }

        // L754
        let m_over_control = cx.get_mouse_over_control(me);
        // L755
        if let Some(m) = m_over_control.filter(|&m| cx.has(m, Caps::KEY_DOWN)) {
            // L756
            cx.host_mut(me).sel_control = Some(m);
            // L757
            return cx.on_key_down(m, key, false).map(|_| me);
        }
        // L759
        cx.host_mut(me).sel_control = None;
        // L761
        if cx.w::<Self>(me).dropped {
            // L762
            if key.name().contains("BUTTON") && !cx.is_mouse_over(me) {
                // L763
                Self::cancel_selection(cx, me);
                // L764
                return None;
            }
            // L766
            if key == Key::LeftButton {
                // L767
                let hover_sel = cx.w::<Self>(me).hover_sel;
                if let Some(hs) = hover_sel.filter(|&hs| Self::gem_of_list(cx, me, hs).is_some()) {
                    // L768
                    cx.w_mut::<Self>(me).dropped = false;
                    // L769
                    cx.w_mut::<Self>(me).sel_index = Some(hs);
                    // L770
                    let name = Self::gem_of_list(cx, me, hs)
                        .expect("checked")
                        .name
                        .to_string();
                    EditControl::set_text(cx, me, &name, false);
                    // L771
                    Self::update_gem(cx, me, false, true, false);
                    // L772
                    return None;
                }
            } else if key == Key::Return {
                // L775
                cx.w_mut::<Self>(me).dropped = false;
                // L776
                if cx.w::<Self>(me).no_matches {
                    // L777
                    EditControl::set_text(cx, me, "", false);
                }
                // L779
                let sel = m_max(
                    cx.w::<Self>(me).sel_index.unwrap_or_else(|| {
                        lua_error!("bad argument #1 to 'max' (number expected, got nil)")
                    }),
                    1.0,
                );
                cx.w_mut::<Self>(me).sel_index = Some(sel);
                // L780
                if let Some(gd) = Self::gem_of_list(cx, me, sel) {
                    // L781
                    EditControl::set_text(cx, me, &gd.name, false);
                }
                // L783
                Self::update_gem(cx, me, true, true, true);
                // L784
                return None;
            } else if key == Key::Escape {
                // L786
                Self::cancel_selection(cx, me);
                // L787
                return None;
            } else if ScrollBarControl::is_scroll_up_key(cx, Self::sb(cx, me).id(), key) {
                // L789
                ScrollBarControl::scroll(cx, Self::sb(cx, me).id(), -1.0);
            } else if ScrollBarControl::is_scroll_down_key(cx, Self::sb(cx, me).id(), key) {
                // L791
                ScrollBarControl::scroll(cx, Self::sb(cx, me).id(), 1.0);
            } else if key == Key::Down {
                let w = cx.w::<Self>(me);
                let sel = w
                    .sel_index
                    .unwrap_or_else(|| lua_error!("attempt to compare nil with number"));
                // L793
                if sel < w.list.len() as f64 && !w.no_matches {
                    // L794
                    cx.w_mut::<Self>(me).sel_index = Some(sel + 1.0);
                    // L795
                    let name = Self::gem_of_list(cx, me, sel + 1.0)
                        .unwrap_or_else(|| lua_error!("attempt to index a nil value"))
                        .name
                        .to_string();
                    EditControl::set_text(cx, me, &name, false);
                    // L796
                    Self::update_gem(cx, me, false, false, false);
                    // L797
                    Self::scroll_sel_into_view(cx, me);
                }
            } else if key == Key::Up {
                let w = cx.w::<Self>(me);
                let sel = w
                    .sel_index
                    .unwrap_or_else(|| lua_error!("attempt to compare number with nil"));
                // L800
                if sel > 0.0 && !w.no_matches {
                    // L801
                    let sel = sel - 1.0;
                    cx.w_mut::<Self>(me).sel_index = Some(sel);
                    // L802
                    if sel == 0.0 {
                        // L803
                        let s = cx.w::<Self>(me).search_str.clone().unwrap_or_default();
                        EditControl::set_text(cx, me, &s, false);
                    } else {
                        // L805
                        let name = Self::gem_of_list(cx, me, sel)
                            .unwrap_or_else(|| lua_error!("attempt to index a nil value"))
                            .name
                            .to_string();
                        EditControl::set_text(cx, me, &name, false);
                    }
                    // L807
                    Self::update_gem(cx, me, false, false, false);
                    // L808
                    Self::scroll_sel_into_view(cx, me);
                }
            }
        } else if key == Key::Return || key == Key::RightButton {
            // L812
            cx.w_mut::<Self>(me).dropped = true;
            // L813
            Self::update_sort_cache(cx, me);
            // L814
            let buf = Self::buf(cx, me);
            cx.w_mut::<Self>(me).initial_buf = Some(buf);
            // L815
            return Some(me);
        }
        // L817
        // L818 `newSel == self.EditControl and self or newSel`: the same node
        <EditControl as Widget>::on_key_down(cx, me, key, double_click)
    }

    /// lua: GemSelectControl.lua:821-854 OnKeyUp(key)
    /// @pob Classes/GemSelectControl.lua:821-854 h=e596f5ae
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        // L822
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L823
            return None;
        }
        // L825
        if let Some(sel) = cx.host(me).sel_control {
            // L826
            let new_sel = cx.on_key_up(sel, key);
            // L827
            if new_sel.is_some() {
                // L828
                return Some(me);
            }
            // L830
            cx.host_mut(me).sel_control = None;
        }
        // L833
        if wiki_matches_key(key) && cx.is_mouse_over(me) {
            // L834
            if cx.w::<Self>(me).dropped {
                let (hover_sel, sel_index, no_matches) = {
                    let w = cx.w::<Self>(me);
                    (w.hover_sel, w.sel_index, w.no_matches)
                };
                // L835
                if let Some(gd) = hover_sel.and_then(|hs| Self::gem_of_list(cx, me, hs)) {
                    // L836 mouse over
                    // L837
                    wiki_open_gem(cx, &gd);
                } else if let Some(si) = sel_index.filter(|&si| si > 0.0) {
                    // L838-L839 selected
                    // L840
                    let gd = Self::gem_of_list(cx, me, si).unwrap_or_else(|| {
                        lua_error!("attempt to index local 'gemData' (a nil value)")
                    });
                    wiki_open_gem(cx, &gd);
                } else if let Some(si) = sel_index.filter(|_| !no_matches) {
                    // L841-L842 search result
                    // L843
                    let gd = Self::gem_of_list(cx, me, m_max(si, 1.0)).unwrap_or_else(|| {
                        lua_error!("attempt to index local 'gemData' (a nil value)")
                    });
                    wiki_open_gem(cx, &gd);
                }
            } else {
                // L845 (`self.index` is always set)
                let display_group = Self::display_group(cx);
                // L846
                let gem = gem_at(&display_group, cx.w::<Self>(me).index);
                // L847
                if let Some(gd) = gem.and_then(|g| g.gemData.get()) {
                    // L848
                    wiki_open_gem(cx, &gd);
                }
            }
        }
        // L852
        // L853 `newSel == self.EditControl and self or newSel`: the same node
        <EditControl as Widget>::on_key_up(cx, me, key)
    }

    fn on_char(cx: &mut Cx, me: CtrlId, ch: Key) -> Focus {
        <EditControl as Widget>::on_char(cx, me, ch)
    }
}

/// `SetDrawColor(self.sortCache.dpsColor[gemId])`: a colour escape (a nil colour is Lua's
/// "bad argument" error).
fn set_color_val(cx: &mut Cx, color: Option<&str>) {
    let c = color.unwrap_or_else(|| {
        lua_error!("bad argument #1 to 'SetDrawColor' (number expected, got nil)")
    });
    cx.sg.set_draw_color_esc(c);
}
