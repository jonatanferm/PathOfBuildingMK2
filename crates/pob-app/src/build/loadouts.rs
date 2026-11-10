//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Build.lua:771-1617 (SyncLoadouts, EstimatePlayerProgress).
//! Owner: U20.
//!
//! @pob-file Modules/Build.lua @16de4b82d
//!
//! `self.treeListSpecialLinks` & co. are U19's `init::State` fields (the buildLoadouts selFunc
//! reads them); only `setId` of the Lua `{ setId, setName, linkId }` entries is ever read.
//! The `self.Act` field (EstimatePlayerProgress) lives in `on_frame::State`.
//!
//! ORDER: the loops are `ipairs` over sequences and `string.gmatch`; the per-tab title sets
//! (`itemList[setTitle] = true`) are only looked up by key.

use std::collections::HashSet;

use pob_lua::math::{m_max, m_min};
use pob_lua::{lfmt, n, tostring_num};
use pob_ui_core::controls::{DropDownControl, DropItem, EditControl};
use pob_ui_core::prelude::*;

use super::init::{self, act_extra, insert_if_new, SpecialLinks, WarningsControl, ACTS};
use super::BuildMode;
use crate::app::CxApp;
use crate::model;
use crate::model::build_xml;

/// `self.Act` (`level < 90 and act <= 10 and act or "Endgame"`).
#[derive(Clone, Debug, PartialEq)]
pub enum ActValue {
    Act(f64),
    Endgame,
}

impl ActValue {
    /// `%s` of the value (`tostring`).
    pub fn to_lua_string(&self) -> String {
        match self {
            ActValue::Act(a) => tostring_num(*a),
            ActValue::Endgame => "Endgame".to_owned(),
        }
    }
}

/// SyncLoadouts' return values `treeList, itemList, skillList, configList`.
#[derive(Clone, Debug, Default)]
pub struct LoadoutLists {
    pub tree_list: Vec<String>,
    pub item_list: HashSet<String>,
    pub skill_list: HashSet<String>,
    pub config_list: HashSet<String>,
}

/// One entry of `sortedTreeListSpecialLinks` (`{ setId, setName, linkId }`).
struct TreeLink {
    set_name: String,
    link_id: String,
}

/// `string.match(title, "%{([%w,]+)%}")`
fn link_identifier(title: &str) -> Option<String> {
    let caps = pob_lua::lmatch(title, "%{([%w,]+)%}", None)?;
    match caps.first() {
        Some(pob_lua::pattern::Capture::Str(s)) => Some(String::from_utf8_lossy(s).into_owned()),
        _ => None,
    }
}

/// `title:gsub("%{" .. linkIdentifier .. "%}", ""):gsub("^%s*", ""):gsub("%s*$", "")`, then
/// `"Default"` for an empty name (the gsub result is never nil).
fn set_name_of(title: &str, link_identifier: &str) -> String {
    let (s, _) = pob_lua::gsub(title, &format!("%{{{link_identifier}%}}"), "", None);
    let (s, _) = pob_lua::gsub(&s, "^%s*", "", None);
    let (s, _) = pob_lua::gsub(&s, "%s*$", "", None);
    if s.is_empty() {
        "Default".to_owned()
    } else {
        s
    }
}

/// `string.gmatch(linkIdentifier, "[^%,]+")`
fn link_ids(link_identifier: &str) -> Vec<String> {
    pob_lua::gmatch(link_identifier, "[^%,]+")
        .map(|c| c[0].to_str().into_owned())
        .collect()
}

/// `tabSets[set].title or "Default"` (a missing set is the Lua index error).
#[track_caller]
fn set_title(sets: &indexmap::IndexMap<u32, model::Table>, id: u32) -> String {
    sets.get(&id)
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
        .str(n!("title"))
        .map_or_else(|| "Default".to_owned(), |s| s.to_string())
}

/// L817-840 `local function identifyLinks(setOrderList, tabSets, setList, specialLinks, treeLinks)`:
/// item, skill, and config sets have identical structure.
fn identify_links(
    set_order_list: &[u32],
    tab_sets: &indexmap::IndexMap<u32, model::Table>,
    set_list: &mut HashSet<String>,
    special_links: &mut SpecialLinks,
) {
    // L818
    for set in set_order_list {
        // L819
        let set_title = set_title(tab_sets, *set);
        // L820
        let link_identifier = link_identifier(&set_title);

        // this if/else prioritizes group identifier in case the user creates sets with same name AND same identifiers
        // result is only the group is recognized and one loadout is created rather than a duplicate from each condition met
        // L824
        if let Some(link_identifier) = link_identifier {
            // L825-828 (setName is computed, but only Tree reads it)
            let _set_name = set_name_of(&set_title, &link_identifier);

            // L830
            for link_id in link_ids(&link_identifier) {
                // L831-833
                special_links.links.insert(link_id, f64::from(*set));
            }
        } else {
            // L837
            set_list.insert(set_title);
        }
    }
}

/// lua: Build.lua:771-896 `buildMode:SyncLoadouts()`
/// @pob Modules/Build.lua:771-896 h=9cadfec4
pub fn sync_loadouts(cx: &mut Cx) -> LoadoutLists {
    let me = cx.build().node.id();
    let loadouts = cx
        .child(me, "buildLoadouts")
        .unwrap_or_else(|| lua_error!("attempt to index field 'buildLoadouts' (a nil value)"));
    // L772
    cx.w_mut::<DropDownControl>(loadouts).list = vec![DropItem::str("No Loadouts")];

    // L774
    let mut filtered_list: Vec<String> = vec!["^7^7Loadouts:".to_owned()];
    // L775-778
    let mut lists = LoadoutLists::default();
    // used when clicking on the dropdown to set the correct setId for each SetActiveSet()
    // L780
    let mut tree_links = SpecialLinks::default();
    let mut item_links = SpecialLinks::default();
    let mut skill_links = SpecialLinks::default();
    let mut config_links = SpecialLinks::default();

    let model = cx.build().model.clone();
    let data = model.data;
    // L782
    let one_skill = model.skillsTab.skillSetOrderList.len() == 1;
    // L783
    let one_item = model.itemsTab.itemSetOrderList.len() == 1;
    // L784
    let one_config = model.configTab.configSetOrderList.len() == 1;

    // L786 (the four tabs always exist once Init has run)
    {
        // L787-788
        let mut sorted_tree_list_special_links: Vec<TreeLink> = Vec::new();
        // L789
        for (id, spec) in model.treeTab.specList.with(Clone::clone).iter().enumerate() {
            let id = (id + 1) as f64;
            // L790
            let spec_title = spec
                .title
                .get()
                .map_or_else(|| "Default".to_owned(), |s| s.to_string());
            // only alphanumeric and comma are allowed in the braces { }
            // L792
            let link_identifier = link_identifier(&spec_title);

            // L794
            if let Some(link_identifier) = link_identifier {
                // L795-798
                let set_name = set_name_of(&spec_title, &link_identifier);

                // iterate over each identifier, delimited by comma, and set the index so we can grab it later
                // setId index is the id of the set in the global list needed for SetActiveSet
                // setName is only used for Tree currently and we strip the braces to get the plain name of the set, this is used as the name of the loadout
                // L803
                for link_id in link_ids(&link_identifier) {
                    // L804-807
                    tree_links.links.insert(link_id.clone(), id);
                    // L808
                    sorted_tree_list_special_links.push(TreeLink {
                        set_name: set_name.clone(),
                        link_id,
                    });
                    // L809 transferTable = {}
                }
            } else {
                // L812
                let tv = spec.treeVersion.get();
                let prefix = if tv != data.gameVersions.latestTreeVersion {
                    let display = data
                        .gameVersions
                        .treeVersions
                        .get(&tv)
                        .map(|v| v.display.to_string())
                        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
                    format!("[{display}] ")
                } else {
                    String::new()
                };
                lists.tree_list.push(format!("{prefix}{spec_title}"));
            }
        }

        // item, skill, and config sets have identical structure
        // L817-840 identifyLinks: see identify_links
        // L841
        let order = model.itemsTab.itemSetOrderList.with(Clone::clone);
        model.itemsTab.itemSets.with(|sets| {
            identify_links(&order, sets, &mut lists.item_list, &mut item_links);
        });
        // L842
        let order = model.skillsTab.skillSetOrderList.with(Clone::clone);
        model.skillsTab.skillSets.with(|sets| {
            identify_links(&order, sets, &mut lists.skill_list, &mut skill_links);
        });
        // L843
        let order = model.configTab.configSetOrderList.with(Clone::clone);
        model.configTab.configSets.with(|sets| {
            identify_links(&order, sets, &mut lists.config_list, &mut config_links);
        });

        // loop over all for exact match loadouts
        // L846
        for tree in &lists.tree_list {
            // L847
            if (one_item || lists.item_list.contains(tree))
                && (one_skill || lists.skill_list.contains(tree))
                && (one_config || lists.config_list.contains(tree))
            {
                // L848
                filtered_list.push(tree.clone());
            }
        }
        // loop over the identifiers found within braces and set the loadout name to the TreeSet
        // L852
        for tree in &sorted_tree_list_special_links {
            // L853
            let tree_link_id = &tree.link_id;
            // L854
            if (one_item || item_links.links.contains_key(tree_link_id))
                && (one_skill || skill_links.links.contains_key(tree_link_id))
                && (one_config || config_links.links.contains_key(tree_link_id))
            {
                // L855
                filtered_list.push(format!("{} {{{tree_link_id}}}", tree.set_name));
            }
        }
    }
    {
        let st = &mut cx.w_mut::<BuildMode>(me).init;
        st.tree_list_special_links = tree_links;
        st.item_list_special_links = item_links;
        st.skill_list_special_links = skill_links;
        st.config_list_special_links = config_links;
    }

    // giving the options unique formatting so it can not match with user-created sets
    // L861-864
    filtered_list.push("^7^7-----".to_owned());
    filtered_list.push("^7^7New Loadout".to_owned());
    filtered_list.push("^7^7Sync".to_owned());
    filtered_list.push("^7^7Help >>".to_owned());

    // L866
    if !filtered_list.is_empty() {
        // L867
        cx.w_mut::<DropDownControl>(loadouts).list =
            filtered_list.iter().map(DropItem::str).collect();
    }

    // Try to select loadout in dropdown based on currently selected tree
    // L871 (the tree tab always exists)
    {
        // L872
        let active = model.treeTab.activeSpec.get();
        let spec = if active.fract() == 0.0 && active >= 1.0 {
            model.treeTab.specList.get(active as usize)
        } else {
            None
        }
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
        let tree_name = spec
            .title
            .get()
            .map_or_else(|| "Default".to_owned(), |s| s.to_string());
        // L873
        for (i, loadout) in filtered_list.iter().enumerate() {
            // L874
            if *loadout == tree_name {
                // L875
                let link_match = {
                    let caps = pob_lua::lmatch(&tree_name, "%{(%w+)%}", None);
                    match caps.as_ref().and_then(|c| c.first()) {
                        Some(pob_lua::pattern::Capture::Str(s)) => {
                            String::from_utf8_lossy(s).into_owned()
                        }
                        _ => tree_name.clone(),
                    }
                };
                // L876 (linkMatch is never nil)
                {
                    let find = |title: Option<model::LStr>| {
                        let name = title.map_or_else(|| "Default".to_owned(), |s| s.to_string());
                        let found = pob_lua::find(&name, &link_match, Some(1), true).is_some();
                        found
                    };
                    // L877-878
                    let skill_match = one_skill
                        || find(set_field_title(
                            &model.skillsTab.skillSets,
                            model.skillsTab.activeSkillSetId.get(),
                        ));
                    // L879-880
                    let item_match = one_item
                        || find(set_field_title(
                            &model.itemsTab.itemSets,
                            model.itemsTab.activeItemSetId.get(),
                        ));
                    // L881-882
                    let config_match = one_config
                        || find(set_field_title(
                            &model.configTab.configSets,
                            model.configTab.activeConfigSetId.get(),
                        ));

                    // L884
                    if skill_match && item_match && config_match {
                        // L885
                        DropDownControl::set_sel(cx, loadouts, i + 1, false);
                        // L886
                        return lists;
                    }
                }
                // L889
                break;
            }
        }
    }

    // L894
    DropDownControl::set_sel(cx, loadouts, 1, false);
    // L895
    lists
}

/// `tabSets[activeId].title` (a missing set is the Lua index error).
#[track_caller]
fn set_field_title(
    sets: &model::Shared<indexmap::IndexMap<u32, model::Table>>,
    active_id: f64,
) -> Option<model::LStr> {
    sets.with(|s| {
        let set = if active_id.fract() == 0.0 && active_id >= 0.0 {
            s.get(&(active_id as u32))
        } else {
            None
        };
        set.unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
            .str(n!("title"))
    })
}

/// lua: Build.lua:898-941 `buildMode:EstimatePlayerProgress()`
/// @pob Modules/Build.lua:898-941 h=77f35e7b
pub fn estimate_player_progress(cx: &mut Cx) {
    let me = cx.build().node.id();
    let model = cx.build().model.clone();
    // L899 (the build always has a spec)
    {
        // L900
        let (points_used, asc_used, secondary_asc_used, ..) = model.spec.get().CountAllocNodes();
        // L901
        let extra =
            model
                .calcsTab
                .mainOutput
                .get()
                .map_or(0.0, |o| match o.get(n!("ExtraPoints")) {
                    v if v.truthy() => v.num(),
                    _ => 0.0,
                });
        // L902
        let used_max = 99.0 + 23.0 + extra;
        let asc_max = 8.0;
        let secondary_asc_max = 8.0;
        let mut level;
        let mut act = 0.0;

        // Find estimated act and level based on points used
        // L905-908
        loop {
            // L906
            act += 1.0;
            // L907
            let a = ACTS[act as usize - 1];
            level = m_min(
                m_max(
                    points_used + 1.0 - a.questPoints - act_extra(act, extra),
                    a.level,
                ),
                100.0,
            );
            // L908
            if act == 11.0 || level <= ACTS[act as usize].level {
                break;
            }
        }

        // L910
        if build_xml::characterLevelAutoMode(&model) && model.characterLevel.get() != level {
            // L911
            model.characterLevel.set(level);
            // L912
            let cl = cx.child(me, "characterLevel").unwrap_or_else(|| {
                lua_error!("attempt to index field 'characterLevel' (a nil value)")
            });
            EditControl::set_text(cx, cl, &tostring_num(model.characterLevel.get()), false);
            // L913
            model.configTab.BuildModList(&model);
        }

        // Ascendancy points for lab
        // this is a recommendation for beginners who are using Path of Building for the first time and trying to map out progress in PoB
        // L918-923
        let lab_suggest = if level < 33.0 {
            ""
        } else if level < 55.0 {
            "\nLabyrinth: Normal Lab"
        } else if level < 68.0 {
            "\nLabyrinth: Cruel Lab"
        } else if level < 75.0 {
            "\nLabyrinth: Merciless Lab"
        } else if level < 90.0 {
            "\nLabyrinth: Uber Lab"
        } else {
            ""
        };

        let warnings = cx
            .child(me, "warnings")
            .unwrap_or_else(|| lua_error!("attempt to index field 'warnings' (a nil value)"));
        // L925
        if points_used > used_max {
            insert_if_new(
                Some(&mut cx.w_mut::<WarningsControl>(warnings).lines),
                "You have too many passive points allocated".to_owned(),
            );
        }
        // L926
        if asc_used > asc_max {
            insert_if_new(
                Some(&mut cx.w_mut::<WarningsControl>(warnings).lines),
                "You have too many ascendancy points allocated".to_owned(),
            );
        }
        // L927
        if secondary_asc_used > secondary_asc_max {
            insert_if_new(
                Some(&mut cx.w_mut::<WarningsControl>(warnings).lines),
                "You have too many secondary ascendancy points allocated".to_owned(),
            );
        }
        // L928
        let act_value = if level < 90.0 && act <= 10.0 {
            ActValue::Act(act)
        } else {
            ActValue::Endgame
        };
        cx.w_mut::<BuildMode>(me).on_frame.act = Some(act_value.clone());

        let negative = cx.main.cc.NEGATIVE.clone();
        // L930-935
        let s = lfmt!(
            "%s%3d / %3d   %s%d / %d",
            if points_used > used_max {
                negative.as_str()
            } else {
                "^7"
            },
            points_used,
            used_max,
            if asc_used > asc_max {
                negative.as_str()
            } else {
                "^7"
            },
            asc_used,
            asc_max
        );
        // L936-939
        let a = ACTS[act as usize - 1];
        let req = lfmt!(
            "Required Level: %d\nEstimated Progress:\nAct: %s\nQuestpoints: %d\nExtra Skillpoints: %d%s",
            level,
            act_value.to_lua_string().as_str(),
            a.questPoints,
            act_extra(act, extra),
            lab_suggest
        );
        let pd = cx
            .child(me, "pointDisplay")
            .unwrap_or_else(|| lua_error!("attempt to index field 'pointDisplay' (a nil value)"));
        let w = cx.w_mut::<init::InfoControl>(pd);
        w.str_ = Some(s);
        w.req = Some(req);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_helpers() {
        // Build.lua:792-798 / 803
        assert_eq!(
            link_identifier("Leveling {a1,b2}").as_deref(),
            Some("a1,b2")
        );
        assert_eq!(link_identifier("Plain"), None);
        assert_eq!(link_identifier("bad {a-b}"), None);
        assert_eq!(set_name_of("  Leveling {a1,b2}  ", "a1,b2"), "Leveling");
        assert_eq!(set_name_of("{x}", "x"), "Default");
        assert_eq!(link_ids("a1,b2"), ["a1", "b2"]);
    }

    #[test]
    fn identify_links_splits_titles() {
        // Build.lua:817-840
        let mut sets = indexmap::IndexMap::new();
        let t1 = model::Table::new();
        t1.set(n!("title"), "Boss {b}");
        let t3 = model::Table::new();
        t3.set(n!("title"), "Mapping");
        sets.insert(1u32, t1);
        sets.insert(2u32, model::Table::new());
        sets.insert(5u32, t3);
        let mut list = HashSet::new();
        let mut links = SpecialLinks::default();
        identify_links(&[5, 1, 2], &sets, &mut list, &mut links);
        assert!(list.contains("Mapping") && list.contains("Default"));
        assert_eq!(list.len(), 2);
        assert_eq!(links.links.get("b"), Some(&1.0));
    }

    #[test]
    fn act_value_strings() {
        assert_eq!(ActValue::Act(5.0).to_lua_string(), "5");
        assert_eq!(ActValue::Endgame.to_lua_string(), "Endgame");
    }
}
