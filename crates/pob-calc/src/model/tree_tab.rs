//! @pob-file Classes/TreeTab.lua @16de4b82d
//! TreeTab model (Classes/TreeTab.lua 500-585).
//!
//! Owner: BM-A. Loading and saving the `<Tree>` section (and the legacy `<Spec>` section),
//! `PostLoad` and spec switching. UI statements (dropdowns, window title, loadouts) are left to
//! the UI port (ui doc U25-U27) and marked `// UI:`. `self.showConvert` (CCR-0052 §9) is kept in
//! the build's side table (`build_xml::build_extra(build)["treeTab"]`).
#![allow(non_snake_case, clippy::all, clippy::pedantic)]

use crate::classes::passive_spec::new_PassiveSpec;
use crate::classes::passive_tree::setJewelRadiiGlobally;
use crate::model::build_xml::build_extra;
use crate::model::items_tab::build_spec;
use crate::model::{XmlChild, XmlNode};
use crate::prelude::*;

/// `main:OpenMessagePopup(title, msg)` (UI): the calc prints it.
fn OpenMessagePopup(title: &str, msg: &str) {
    eprintln!("{title}: {msg}");
}

/// @pob Classes/TreeTab.lua:26-38 h=3c65e524
impl TreeTab {
    /// The constructor's model half (idempotent): `specList = { new PassiveSpec(build,
    /// latestTreeVersion) }` and `SetActiveSpec(1)`. `SetCompareSpec(1)` and the controls are
    /// the UI's. Needs the items tab's constructor (Build.lua:612-613 runs it first).
    pub fn ctor_model(&self, build: &BuildRef) {
        let extra = build_extra(build).get_or_new_table(n!("treeTab"));
        if extra.truthy(n!("$ctor")) {
            return;
        }
        extra.set(n!("$ctor"), true);
        build.itemsTab.ctor_model(build);
        // L35-36
        let spec = new_PassiveSpec(build, data().gameVersions.latestTreeVersion, false);
        self.specList.with_mut(|l| {
            l.clear();
            l.push(spec);
        });
        // L37
        self.SetActiveSpec(build, 1.0);
        // L38 UI: self:SetCompareSpec(1)
    }

    /// `self.showConvert` (CCR-0052 §9).
    pub fn showConvert(&self, build: &BuildRef) -> bool {
        build_extra(build)
            .get_or_new_table(n!("treeTab"))
            .truthy(n!("showConvert"))
    }
}

/// @pob Classes/TreeTab.lua:500-527 h=17598e1e
impl TreeTab {
    /// Lua returns `true` on an unknown tree version (Build:Init then closes the build); the
    /// frozen signature returns `()`, so the error is reported through [`TreeTab::Load_ret`].
    pub fn Load(&self, build: &BuildRef, xml: &crate::model::XmlNode, dbFileName: &str) {
        let _ = self.Load_ret(build, xml, dbFileName);
    }

    /// `TreeTab:Load` with its return value (`true` = error; CCR-0076 §4).
    pub fn Load_ret(&self, build: &BuildRef, xml: &XmlNode, dbFileName: &str) -> bool {
        let gv = &data().gameVersions;
        // L501
        self.specList.with_mut(|l| l.clear());
        // L502
        if &*xml.elem == "Spec" {
            // Import single spec from old build
            // L504
            let spec = new_PassiveSpec(build, gv.defaultTreeVersion, false);
            self.specList.with_mut(|l| l.push(spec.clone()));
            // L505
            spec.Load(xml, dbFileName);
            // L506
            self.activeSpec.set(1.0);
            // L507
            build.spec.set(spec);
            // L508
            return false;
        }
        // L510 (`pairs` over the node visits its array part 1..n first, in order; `elem` and
        // `attrib` are not Spec nodes)
        for node in &xml.children {
            // L511
            let XmlChild::Node(node) = node else { continue };
            // L512
            if &*node.elem == "Spec" {
                let treeVersion = node.attrib.get("treeVersion");
                // L513
                if let Some(v) = treeVersion {
                    if !gv.treeVersions.contains_key(&Name::from(&**v)) {
                        // L514
                        OpenMessagePopup(
                            "Unknown Passive Tree Version",
                            "The build you are trying to load uses an unrecognised version of the passive skill tree.\nYou may need to update the program before loading this build.",
                        );
                        // L515
                        return true;
                    }
                }
                // L517
                let ver = treeVersion.map_or(gv.defaultTreeVersion, |v| Name::from(&**v));
                let newSpec = new_PassiveSpec(build, ver, false);
                // L518
                newSpec.Load(node, dbFileName);
                // L519
                self.specList.with_mut(|l| l.push(newSpec));
            }
        }
        // L523
        if self.specList.get(1).is_none() {
            // L524
            let spec = new_PassiveSpec(build, gv.latestTreeVersion, false);
            self.specList.with_mut(|l| l.push(spec));
        }
        // L526
        let activeSpec = node_attr_num(xml, "activeSpec").unwrap_or(1.0);
        self.SetActiveSpec(build, activeSpec);
        false
    }
}

/// `tonumber(xml.attrib[k])`.
fn node_attr_num(xml: &XmlNode, k: &str) -> Option<f64> {
    xml.attrib.get(k).and_then(|v| pob_lua::tonumber(v))
}

/// @pob Classes/TreeTab.lua:529-534 h=b425e715
impl TreeTab {
    pub fn PostLoad(&self, build: &BuildRef) {
        // L530
        for spec in self.specList.snapshot() {
            // L531
            spec.PostLoad();
        }
        // L533
        build.itemsTab.PopulateSlots(build);
    }
}

/// @pob Classes/TreeTab.lua:536-547 h=203f2bbd
impl TreeTab {
    pub fn Save(&self, build: &BuildRef, xml: &mut crate::model::XmlNode) {
        let _ = build;
        // L537
        xml.attrib = indexmap::IndexMap::new();
        xml.attrib.insert(
            LStr::from("activeSpec"),
            LStr::from(tostring_num(self.activeSpec.get())),
        );
        // L540
        for spec in self.specList.snapshot() {
            // L541
            let mut child = XmlNode {
                elem: LStr::from("Spec"),
                attrib: indexmap::IndexMap::new(),
                children: Vec::new(),
            };
            // L544
            spec.Save(&mut child);
            // L545
            xml.children.push(XmlChild::Node(child));
        }
    }
}

/// @pob Classes/TreeTab.lua:549-585 h=0a94fa7e
impl TreeTab {
    pub fn SetActiveSpec(&self, build: &BuildRef, specId: f64) {
        // L550 (`nil` while no spec is active: the placeholder of `Build::new`)
        let prevSpec = build_spec(build);
        // L551
        self.activeSpec
            .set(m_min(specId, self.specList.len() as f64));
        // L552
        let a = self.activeSpec.get();
        let curSpec = if a >= 1.0 && a.fract() == 0.0 {
            self.specList.get(a as usize)
        } else {
            None
        };
        let curSpec =
            curSpec.unwrap_or_else(|| lua_error!("attempt to index local 'curSpec' (a nil value)"));
        // L553
        setJewelRadiiGlobally(&curSpec.treeVersion.get());
        // L554
        build.spec.set(curSpec.clone());
        // L555
        build.buildFlag.set(true);
        // L556 UI: self.build.spec:SetWindowTitleWithBuildClass()
        // L557 UI: self.build:UpdateClassDropdowns(curSpec.treeVersion)
        // L558 (string keys: insertion order; PoB's `pairs` order is random per process)
        // LUA-ORDER
        let slots: Vec<Table> = build.itemsTab.slots.with(|m| m.values().cloned().collect());
        for slot in slots {
            // L559
            let Some(nodeId) = slot.get(n!("nodeId")).opt_num() else {
                continue;
            };
            // L560
            if let Some(prevSpec) = &prevSpec {
                // Update the previous spec's jewel for this slot
                // L562
                let sel = slot.get(n!("selItemId")).opt_num();
                prevSpec.jewels.with_mut(|m| m.set(nodeId as i64, sel));
            }
            // L564
            if let Some(j) = curSpec.jewels.with(|m| m.get(nodeId as i64)) {
                // Socket the jewel for the new spec
                // L566
                slot.set(n!("selItemId"), j);
            } else {
                // Unsocket the old jewel from the previous spec
                // L569
                slot.set(n!("selItemId"), 0.0);
            }
        }
        // L573
        let latest = data().gameVersions.latestTreeVersion;
        let showConvert = pob_lua::pattern::find(
            &curSpec.treeVersion.get(),
            &format!("^{latest}"),
            None,
            false,
        )
        .is_none();
        build_extra(build)
            .get_or_new_table(n!("treeTab"))
            .set(n!("showConvert"), showConvert);
        // L574
        if build.itemsTab.orderList.get(1).is_some() {
            // Update item slots if items have been loaded already
            // L576
            build.itemsTab.PopulateSlots(build);
        }
        // L578-584 UI: specSelect.selIndex, versionSelect, self.build:SyncLoadouts()
    }
}
