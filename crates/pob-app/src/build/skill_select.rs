//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Build.lua:1618-2221 (RefreshSkillSelectControls).
//! Owner: U21.
//!
//! @pob-file Modules/Build.lua @16de4b82d
//!
//! `buildMode:RefreshSkillSelectControls(controls, mainGroup, suffix)` (Build.lua:1617-1716):
//! refresh the set of controls used to select main group/skill/minion. `controls` is the host
//! whose children are the selectors (the build node, Build.lua:1291; a CalcsTab section,
//! CalcsTab.lua:145); `suffix` is `""` or `"Calcs"` (the `…Calcs` fields of the socket groups
//! and gem instances).
//!
//! CCR-0103 §3: `grantedEffect.minionList` is typed `Vec<Name>` (nil and `{ }` both empty); the
//! port tells them apart with calc:AS's `minionList_is_set` (CCR-0040), reached directly because
//! `model.rs` does not re-export it.
//!
//! DIVERGENCE(node-name): an explode source that is a passive node reads `dn` for
//! `explodeSource.name or explodeSource.dn`: the `Node` type has no `name`, and PassiveTree.lua:534
//! sets `dn = name`.

#![allow(non_snake_case)]

use std::rc::Rc;

use pob_lua::{lua_error, tostring_num, Name};
use pob_ui_core::controls::{DropDownControl, DropItem, EditControl};
use pob_ui_core::prelude::*;

use super::init::{MinionListValue, WarningsControl};
use crate::app::CxApp;
use crate::model::{self, GemInstance, SocketGroup, Val};

/// `controls[key]` (a missing control is a Lua nil index).
#[track_caller]
fn ctl(cx: &Cx, host: CtrlId, key: &'static str) -> CtrlId {
    cx.child(host, key)
        .unwrap_or_else(|| lua_error!("attempt to index field '{key}' (a nil value)"))
}

fn set_shown(cx: &mut Cx, id: CtrlId, shown: bool) {
    cx.base_mut(id).shown = Prop::Val(shown);
}

fn set_enabled(cx: &mut Cx, id: CtrlId, enabled: bool) {
    cx.base_mut(id).enabled = Prop::Val(enabled);
}

/// A Lua number as a 1-based list index (`selIndex = n`).
fn sel(n: f64) -> usize {
    if n.is_finite() && n >= 0.0 {
        n as usize
    } else {
        0
    }
}

/// A nil `Cell<f64>` is NaN (contracts.md#class-census).
fn opt(n: f64) -> Option<f64> {
    (!n.is_nan()).then_some(n)
}

/// `srcInstance["<field>" .. suffix]` for the number fields.
fn src_num(g: &GemInstance, field: &str, calcs: bool) -> Option<f64> {
    match (field, calcs) {
        ("skillPart", false) => g.skillPart.get(),
        ("skillPart", true) => g.skillPartCalcs.get(),
        ("skillStageCount", false) => g.skillStageCount.get(),
        ("skillStageCount", true) => g.skillStageCountCalcs.get(),
        ("skillMineCount", false) => g.skillMineCount.get(),
        ("skillMineCount", true) => g.skillMineCountCalcs.get(),
        ("skillMinionItemSet", false) => g.skillMinionItemSet.get(),
        ("skillMinionItemSet", true) => g.skillMinionItemSetCalcs.get(),
        ("skillMinionSkill", false) => g.skillMinionSkill.get(),
        ("skillMinionSkill", true) => g.skillMinionSkillCalcs.get(),
        _ => unreachable!("srcInstance.{field}"),
    }
}

/// `x.k` of an explode source (an item, a passive node, or a table).
fn source_field(src: &Val, k: &str) -> Val {
    if let Some(item) = src.obj::<model::Item>() {
        return match k {
            "name" => Val::from(item.name.get()),
            "rarity" => Val::from(item.rarity.get()),
            _ => item.extra.get(Name::from(k)),
        };
    }
    if let Some(node) = src.obj::<model::Node>() {
        // DIVERGENCE(node-name): see the module doc
        return match k {
            "name" | "dn" => Val::from(node.dn.get()),
            _ => Val::Nil,
        };
    }
    src.get(Name::from(k))
}

/// The operand of `..` (string or number; anything else raises).
#[track_caller]
fn concat(v: &Val) -> String {
    match v {
        Val::Str(s) => s.to_string(),
        Val::Num(n) => tostring_num(*n),
        other => lua_error!("attempt to concatenate a {} value", other.lua_type()),
    }
}

/// `tostring(a or b or ...)` of the stage/mine count fallbacks.
fn tostring_val(v: &Val) -> String {
    match v {
        Val::Nil => "nil".to_owned(),
        Val::Bool(b) => b.to_string(),
        other => concat(other),
    }
}

/// `x or y` on values.
fn lor(a: Val, b: impl FnOnce() -> Val) -> Val {
    if a.truthy() {
        a
    } else {
        b()
    }
}

/// lua: Build.lua:1618-1716 `buildMode:RefreshSkillSelectControls(controls, mainGroup, suffix)`
/// @pob Modules/Build.lua:1618-1716 h=820ad6b6
#[allow(clippy::too_many_lines)]
pub fn refresh_skill_select_controls(cx: &mut Cx, controls: CtrlId, mainGroup: f64, suffix: &str) {
    let calcs = match suffix {
        "" => false,
        "Calcs" => true,
        other => lua_error!("RefreshSkillSelectControls: unknown suffix {other:?}"),
    };
    let model = cx.build().model.clone();
    let main_socket_group = ctl(cx, controls, "mainSocketGroup");
    // L1619
    cx.w_mut::<DropDownControl>(main_socket_group).sel_index = sel(mainGroup);
    // L1620
    cx.w_mut::<DropDownControl>(main_socket_group).list.clear();
    // L1621 (ORDER: `pairs` over the group list's array part visits 1..n, (c))
    let groups: Vec<model::SocketGroupRef> = model.skillsTab.socketGroupList.get().snapshot();
    for (i, socketGroup) in groups.iter().enumerate() {
        // L1622
        let label = socketGroup
            .displayLabel
            .get()
            .map(|s| s.to_string())
            .unwrap_or_default();
        cx.w_mut::<DropDownControl>(main_socket_group)
            .list
            .push(DropItem::new(label, Rc::new((i + 1) as f64)));
    }
    // L1624
    DropDownControl::check_dropped_width(cx, main_socket_group, true);
    // L1625
    if let Some(warnings) = cx.child(controls, "warnings") {
        let shown = !cx.w::<WarningsControl>(warnings).lines.is_empty();
        set_shown(cx, warnings, shown);
    }
    let main_skill = ctl(cx, controls, "mainSkill");
    let main_skill_part = ctl(cx, controls, "mainSkillPart");
    let main_skill_mine_count = ctl(cx, controls, "mainSkillMineCount");
    let main_skill_stage_count = ctl(cx, controls, "mainSkillStageCount");
    let main_skill_minion = ctl(cx, controls, "mainSkillMinion");
    let main_skill_minion_skill = ctl(cx, controls, "mainSkillMinionSkill");
    // L1626
    if cx.w::<DropDownControl>(main_socket_group).list.is_empty() {
        // L1627
        cx.w_mut::<DropDownControl>(main_socket_group)
            .list
            .push(DropItem::new("<No skills added yet>", Rc::new(1.0)));
        // L1628-1633
        set_shown(cx, main_skill, false);
        set_shown(cx, main_skill_part, false);
        set_shown(cx, main_skill_mine_count, false);
        set_shown(cx, main_skill_stage_count, false);
        set_shown(cx, main_skill_minion, false);
        set_shown(cx, main_skill_minion_skill, false);
        return;
    }
    // L1635
    let mainSocketGroup: model::SocketGroupRef = groups
        .get(sel(mainGroup).wrapping_sub(1))
        .filter(|_| mainGroup.fract() == 0.0)
        .cloned()
        .unwrap_or_else(|| lua_error!("attempt to index local 'mainSocketGroup' (a nil value)"));
    // L1636
    let displaySkillList = display_skill_list(&mainSocketGroup, calcs);
    // L1637
    let mainActiveSkill = opt(if calcs {
        mainSocketGroup.mainActiveSkillCalcs.get()
    } else {
        mainSocketGroup.mainActiveSkill.get()
    })
    .unwrap_or(1.0);
    // L1638
    cx.w_mut::<DropDownControl>(main_skill).list.clear();
    // L1639
    for (i, activeSkill) in displaySkillList.iter().enumerate() {
        let activeEffect = activeSkill.activeEffect.get();
        // L1640
        let srcInstance = activeEffect
            .srcInstance
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'srcInstance' (a nil value)"));
        let explodeSource = srcInstance.extra.get(Name::from("explodeSource"));
        // L1641
        let explodeSourceName = if explodeSource.truthy() {
            lor(source_field(&explodeSource, "name"), || {
                source_field(&explodeSource, "dn")
            })
        } else {
            explodeSource.clone()
        };
        // L1642
        let colourCoded = if explodeSourceName.truthy() {
            let rarity = lor(source_field(&explodeSource, "rarity"), || Val::from("NORMAL"));
            let code = match &rarity {
                Val::Str(r) => cx.main.cc.get(r).cloned(),
                _ => None,
            }
            .unwrap_or_else(|| lua_error!("attempt to concatenate a nil value"));
            Some(format!("From {code}{}", concat(&explodeSourceName)))
        } else {
            None
        };
        // L1643
        let label = colourCoded
            .unwrap_or_else(|| activeEffect.grantedEffect.get().name.to_string());
        cx.w_mut::<DropDownControl>(main_skill)
            .list
            .push(DropItem::new(label, Rc::new((i + 1) as f64)));
    }
    // L1645
    set_enabled(cx, main_skill, displaySkillList.len() > 1);
    // L1646
    cx.w_mut::<DropDownControl>(main_skill).sel_index = sel(mainActiveSkill);
    // L1647-1653
    set_shown(cx, main_skill, true);
    set_shown(cx, main_skill_part, false);
    set_shown(cx, main_skill_mine_count, false);
    set_shown(cx, main_skill_stage_count, false);
    set_shown(cx, main_skill_minion, false);
    let library = ctl(cx, controls, "mainSkillMinionLibrary");
    set_shown(cx, library, false);
    set_shown(cx, main_skill_minion_skill, false);
    // L1654
    if displaySkillList.is_empty() {
        return;
    }
    // L1655
    let activeSkill = displaySkillList
        .get(sel(mainActiveSkill).wrapping_sub(1))
        .filter(|_| mainActiveSkill.fract() == 0.0)
        .cloned()
        .unwrap_or_else(|| lua_error!("attempt to index local 'activeSkill' (a nil value)"));
    // L1656
    let activeEffect = activeSkill.activeEffect.get();
    // L1657 (`activeEffect` is always set)
    let grantedEffect = activeEffect.grantedEffect.get();
    let srcInstance = || {
        activeEffect
            .srcInstance
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'srcInstance' (a nil value)"))
    };
    let skillFlags = activeSkill.skillFlags.get();
    let skillData = activeSkill.skillData.get();
    let parts = &grantedEffect.parts;
    // L1658
    if parts.len() > 1 {
        // L1659
        set_shown(cx, main_skill_part, true);
        // L1660
        cx.w_mut::<DropDownControl>(main_skill_part).list.clear();
        // L1661
        for (i, part) in parts.iter().enumerate() {
            // L1662
            cx.w_mut::<DropDownControl>(main_skill_part)
                .list
                .push(DropItem::new(part.name.to_string(), Rc::new((i + 1) as f64)));
        }
        // L1664
        let part_sel = src_num(&srcInstance(), "skillPart", calcs).unwrap_or(1.0);
        cx.w_mut::<DropDownControl>(main_skill_part).sel_index = sel(part_sel);
        // L1665
        let part = parts
            .get(sel(part_sel).wrapping_sub(1))
            .filter(|_| part_sel.fract() == 0.0)
            .unwrap_or_else(|| lua_error!("attempt to index a nil value (field '?')"));
        if part.flags.iter().any(|(n, v)| n.as_str() == "stages" && *v) {
            // L1666
            set_shown(cx, main_skill_stage_count, true);
            // L1667 (`parts[selIndex].stagesMin` is never set in the data: SkillPart has no such field)
            let v = lor(
                src_num(&srcInstance(), "skillStageCount", calcs).map_or(Val::Nil, Val::Num),
                || lor(skillData.get(Name::from("stagesMax")), || Val::Num(1.0)),
            );
            cx.w_mut::<EditControl>(main_skill_stage_count).buf = tostring_val(&v);
        }
    }
    // L1670
    if skillFlags.truthy(Name::from("mine")) {
        // L1671
        set_shown(cx, main_skill_mine_count, true);
        // L1672
        let v = src_num(&srcInstance(), "skillMineCount", calcs).map_or(Val::from(""), Val::Num);
        cx.w_mut::<EditControl>(main_skill_mine_count).buf = tostring_val(&v);
    }
    // L1674
    if skillFlags.truthy(Name::from("multiStage")) && parts.len() <= 1 {
        // L1675
        set_shown(cx, main_skill_stage_count, true);
        // L1676
        let v = lor(
            src_num(&srcInstance(), "skillStageCount", calcs).map_or(Val::Nil, Val::Num),
            || {
                lor(skillData.get(Name::from("stagesMax")), || {
                    lor(skillData.get(Name::from("stagesMin")), || Val::Num(1.0))
                })
            },
        );
        cx.w_mut::<EditControl>(main_skill_stage_count).buf = tostring_val(&v);
    }
    // L1678 (CCR-0103 §3: `grantedEffect.minionList` nil vs `{ }`)
    let ge_minion_list_set = pob_calc::modules::calc_active_skill::minionList_is_set(&grantedEffect);
    let activeMinionList: Vec<Name> = activeSkill.minionList.get().snapshot();
    if !skillFlags.truthy(Name::from("disable"))
        && (ge_minion_list_set || !activeMinionList.is_empty())
    {
        // L1679
        cx.w_mut::<DropDownControl>(main_skill_minion).list.clear();
        // L1680
        if grantedEffect.minionHasItemSet {
            // L1681
            let order: Vec<u32> = model.itemsTab.itemSetOrderList.snapshot();
            for itemSetId in order {
                // L1682
                let itemSet = model
                    .itemsTab
                    .itemSets
                    .with(|s| s.get(&itemSetId).cloned())
                    .unwrap_or_else(|| lua_error!("attempt to index local 'itemSet' (a nil value)"));
                // L1683-1686
                let label = itemSet
                    .str(Name::from("title"))
                    .map_or_else(|| "Default Item Set".to_owned(), |s| s.to_string());
                cx.w_mut::<DropDownControl>(main_skill_minion).list.push(DropItem::new(
                    label,
                    Rc::new(MinionListValue {
                        item_set_id: Some(f64::from(itemSetId)),
                        minion_id: None,
                    }),
                ));
            }
            // L1688
            let value = src_num(&srcInstance(), "skillMinionItemSet", calcs).unwrap_or(1.0);
            DropDownControl::sel_by_value(cx, main_skill_minion, &|item| {
                item.get::<MinionListValue>()
                    .is_some_and(|v| v.item_set_id == Some(value))
            });
        } else {
            // L1690
            let library_shown = ge_minion_list_set && grantedEffect.minionList.is_empty();
            set_shown(cx, library, library_shown);
            // L1691
            for minionId in &activeMinionList {
                // L1692-1695
                let minion = model::data()
                    .minions
                    .get(minionId)
                    .unwrap_or_else(|| lua_error!("attempt to index a nil value (field '?')"));
                cx.w_mut::<DropDownControl>(main_skill_minion).list.push(DropItem::new(
                    minion.name.to_string(),
                    Rc::new(MinionListValue {
                        item_set_id: None,
                        minion_id: Some(*minionId),
                    }),
                ));
            }
            // L1697 (`srcInstance["skillMinion"..suffix] or list[1]`: with no such field the
            // value is the first entry, a table, which equals no `minionId`)
            let value = if calcs {
                srcInstance().skillMinionCalcs.get()
            } else {
                srcInstance().skillMinion.get()
            };
            DropDownControl::sel_by_value(cx, main_skill_minion, &|item| {
                value.as_ref().is_some_and(|v| {
                    item.get::<MinionListValue>()
                        .is_some_and(|m| m.minion_id.as_ref() == Some(v))
                })
            });
        }
        // L1699
        let n = cx.w::<DropDownControl>(main_skill_minion).list.len();
        set_enabled(cx, main_skill_minion, n > 1);
        // L1700
        set_shown(cx, main_skill_minion, true);
        // L1701
        cx.w_mut::<DropDownControl>(main_skill_minion_skill).list.clear();
        // L1702
        if let Some(minion) = activeSkill.minion.get() {
            // L1703
            for minionSkill in minion.activeSkillList.get().snapshot() {
                // L1704
                let name = minionSkill.activeEffect.get().grantedEffect.get().name.to_string();
                cx.w_mut::<DropDownControl>(main_skill_minion_skill)
                    .list
                    .push(DropItem::str(name));
            }
            // L1706
            let s = src_num(&srcInstance(), "skillMinionSkill", calcs).unwrap_or(1.0);
            cx.w_mut::<DropDownControl>(main_skill_minion_skill).sel_index = sel(s);
            // L1707
            set_shown(cx, main_skill_minion_skill, true);
            // L1708
            let n = cx.w::<DropDownControl>(main_skill_minion_skill).list.len();
            set_enabled(cx, main_skill_minion_skill, n > 1);
        } else {
            // L1710
            cx.w_mut::<DropDownControl>(main_skill_minion)
                .list
                .push(DropItem::str("<No spectres in build>"));
        }
    }
}

/// `mainSocketGroup["displaySkillList" .. suffix]` (nil raises in `ipairs`).
fn display_skill_list(g: &SocketGroup, calcs: bool) -> Vec<pob_calc::active_skill::ActiveSkillRef> {
    let f = if calcs {
        g.displaySkillListCalcs.get()
    } else {
        g.displaySkillList.get()
    };
    f.map(|l| l.snapshot())
        .unwrap_or_else(|| lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)"))
}
