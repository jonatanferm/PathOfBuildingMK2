//! The app's calc backend: Path of Building itself, running headless in
//! `pob_calc`'s interpreter on a worker thread.
//!
//! PoB's build object is the source of truth for calculation. The UI still
//! edits MK2's [`Character`]; [`PobSession::sync`] diffs it against the last
//! state sent to PoB and replays the difference as PoB commands (the same
//! calls PoB's own controls make). Anything without a dedicated command falls
//! back to reloading the whole build from exported XML. Imported builds are
//! handed to PoB verbatim so nothing is lost in translation.

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

use pob_calc::worker::{Command, Event, PobHandle, Snapshot};
use pob_data::{Item, Slot};
use pob_engine::{Character, CharacterSnapshot, Output};

/// PoB's slot name for an MK2 slot.
#[must_use]
pub fn pob_slot_name(slot: Slot) -> &'static str {
    match slot {
        Slot::Helmet => "Helmet",
        Slot::BodyArmour => "Body Armour",
        Slot::Gloves => "Gloves",
        Slot::Boots => "Boots",
        Slot::Amulet => "Amulet",
        Slot::Ring1 => "Ring 1",
        Slot::Ring2 => "Ring 2",
        Slot::Belt => "Belt",
        Slot::Weapon1 => "Weapon 1",
        Slot::Weapon2 => "Weapon 2",
        Slot::Weapon1Swap => "Weapon 1 Swap",
        Slot::Weapon2Swap => "Weapon 2 Swap",
        Slot::Flask1 => "Flask 1",
        Slot::Flask2 => "Flask 2",
        Slot::Flask3 => "Flask 3",
        Slot::Flask4 => "Flask 4",
        Slot::Flask5 => "Flask 5",
    }
}

/// Quote a string as a Lua literal.
fn lua_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Where the worker thread currently stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionStatus {
    Booting,
    Ready,
    Busy,
    Failed(String),
}

pub struct PobSession {
    handle: PobHandle,
    pub status: SessionStatus,
    /// The character as last sent to PoB (what PoB's build now reflects).
    baseline: Option<Character>,
    /// Sequence number of the newest command sent.
    latest_seq: u64,
    /// Snapshot answering `latest_seq`, once it arrives.
    pub snapshot: Option<Snapshot>,
    /// Boot duration, for the status bar.
    pub boot_millis: Option<u128>,
    /// Last command error from PoB, if any.
    pub last_error: Option<String>,
}

impl PobSession {
    /// Boot PoB from its `src/` directory on a worker thread.
    #[must_use]
    pub fn spawn(pob_src: PathBuf) -> Self {
        Self {
            handle: PobHandle::spawn(pob_src),
            status: SessionStatus::Booting,
            baseline: None,
            latest_seq: 0,
            snapshot: None,
            boot_millis: None,
            last_error: None,
        }
    }

    /// Find PoB's sources: `data/pob/src` (packaged) or the dev checkout.
    #[must_use]
    pub fn locate_pob_src(data_root: &std::path::Path) -> Option<PathBuf> {
        let candidates = [
            data_root.join("pob/src"),
            data_root.join("../.PathOfBuilding/src"),
            PathBuf::from(".PathOfBuilding/src"),
        ];
        candidates
            .into_iter()
            .find(|p| p.join("HeadlessWrapper.lua").is_file())
    }

    /// Hand PoB a build verbatim (import / open), and treat `character` (the
    /// MK2 view of the same build) as the new baseline.
    pub fn load_xml(&mut self, xml: String, name: &str, character: &Character) {
        self.latest_seq = self.handle.send(Command::LoadXml {
            xml,
            name: name.to_owned(),
        });
        self.baseline = Some(character.clone());
        self.status = SessionStatus::Busy;
    }

    /// Bring PoB's build in line with `character`.
    pub fn sync(&mut self, character: &Character, tree_version: &str) {
        let Some(base) = self.baseline.clone() else {
            self.full_reload(character);
            return;
        };
        // Everything the targeted commands below can't express → full reload.
        if !Self::only_targeted_changes(&base, character) {
            self.full_reload(character);
            return;
        }
        let mut lua = String::new();
        if base.level != character.level {
            lua.push_str(&format!(
                "build.characterLevel = {lvl}\nbuild.characterLevelAutoMode = false\nbuild.controls.characterLevel:SetText({lvl})\nbuild.configTab:BuildModList()\n",
                lvl = character.level.clamp(1, 100)
            ));
        }
        if base.class != character.class
            || base.ascendancy != character.ascendancy
            || base.allocated != character.allocated
            || base.mastery_selections != character.mastery_selections
        {
            lua.push_str(&Self::tree_command(character, tree_version));
        }
        for slot in Slot::all() {
            let old = base.items.get(*slot).map(|i| i.raw.as_str());
            let new = character.items.get(*slot).map(|i| i.raw.as_str());
            if old != new {
                lua.push_str(&Self::equip_command(*slot, character.items.get(*slot)));
            }
        }
        if lua.is_empty() {
            return;
        }
        self.latest_seq = self.handle.send(Command::Lua { code: lua });
        self.baseline = Some(character.clone());
        self.status = SessionStatus::Busy;
    }

    fn full_reload(&mut self, character: &Character) {
        let xml = pob_engine::export_pob_xml(character);
        self.latest_seq = self.handle.send(Command::LoadXml {
            xml,
            name: "MK2 build".to_owned(),
        });
        self.baseline = Some(character.clone());
        self.status = SessionStatus::Busy;
    }

    /// True when `a` and `b` differ only in fields with targeted commands
    /// (level, class/ascendancy/tree/masteries, equipped items).
    fn only_targeted_changes(a: &Character, b: &Character) -> bool {
        let mut b2 = b.clone();
        b2.level = a.level;
        b2.class = a.class.clone();
        b2.ascendancy = a.ascendancy.clone();
        b2.allocated = a.allocated.clone();
        b2.mastery_selections = a.mastery_selections.clone();
        b2.items = a.items.clone();
        let sa = serde_json::to_string(&CharacterSnapshot::from_character(a)).unwrap_or_default();
        let sb = serde_json::to_string(&CharacterSnapshot::from_character(&b2)).unwrap_or_default();
        sa == sb
    }

    fn tree_command(c: &Character, tree_version: &str) -> String {
        let class = c
            .ascendancy
            .as_deref()
            .filter(|a| !a.is_empty())
            .unwrap_or(&c.class.0);
        let mut ids: Vec<u32> = c.allocated.iter().copied().collect();
        ids.sort_unstable();
        let ids = ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let mut masteries: Vec<(u32, u32)> =
            c.mastery_selections.iter().map(|(k, v)| (*k, *v)).collect();
        masteries.sort_unstable();
        let masteries = masteries
            .iter()
            .map(|(k, v)| format!("[{k}]={v}"))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "do\n  local spec = build.spec\n  spec:ImportFromNodeList({class}, nil, nil, spec.curSecondaryAscendClassId or 0, {{{ids}}}, spec.hashOverrides, {{{masteries}}}, {version})\n  spec:AddUndoState()\nend\n",
            class = lua_str(class),
            version = if tree_version.is_empty() { "nil".to_owned() } else { lua_str(tree_version) },
        )
    }

    fn equip_command(slot: Slot, item: Option<&Item>) -> String {
        let name = lua_str(pob_slot_name(slot));
        match item {
            Some(item) => format!(
                "do\n  local item = new(\"Item\", {raw})\n  if item.base then\n    build.itemsTab:AddItem(item, true)\n    build.itemsTab.slots[{name}]:SetSelItemId(item.id)\n    build.itemsTab:PopulateSlots()\n    build.itemsTab:AddUndoState()\n  end\nend\n",
                raw = lua_str(&item.raw),
            ),
            None => format!(
                "build.itemsTab.slots[{name}]:SetSelItemId(0)\nbuild.itemsTab:PopulateSlots()\nbuild.itemsTab:AddUndoState()\n"
            ),
        }
    }

    /// Drain worker events. Returns true when new outputs arrived.
    pub fn poll(&mut self) -> bool {
        let mut updated = false;
        while let Some(ev) = self.handle.try_recv() {
            match ev {
                Event::Booted { millis } => {
                    self.boot_millis = Some(millis);
                    if self.status == SessionStatus::Booting {
                        self.status = SessionStatus::Ready;
                    }
                }
                Event::BootFailed(e) => self.status = SessionStatus::Failed(e),
                Event::Snapshot(s) => {
                    self.last_error = s.error.clone();
                    if s.seq >= self.latest_seq {
                        self.status = SessionStatus::Ready;
                    }
                    self.snapshot = Some(s);
                    updated = true;
                }
            }
        }
        updated
    }

    /// PoB's outputs as an MK2 [`Output`], with MK2's legacy key aliases
    /// filled in so existing panels keep working.
    #[must_use]
    pub fn output(&self) -> Option<Output> {
        let s = self.snapshot.as_ref()?;
        let mut out = Output::default();
        for (k, v) in &s.outputs.numbers {
            out.set(k.clone(), *v);
        }
        for (k, v) in &s.outputs.flags {
            out.set(k.clone(), if *v { 1.0 } else { 0.0 });
        }
        let n = |k: &str| s.outputs.numbers.get(k).copied();
        for (alias, key) in [
            ("Strength", "Str"),
            ("Dexterity", "Dex"),
            ("Intelligence", "Int"),
            ("MainSkillDPS", "TotalDPS"),
            ("MainSkillAverageHit", "AverageHit"),
            ("MainSkillAverageHitWithCrit", "AverageDamage"),
            ("MainSkillSpeed", "Speed"),
            ("MainSkillCritChance", "CritChance"),
            ("MainSkillHitChance", "HitChance"),
            ("MainSkillManaCost", "ManaCost"),
            ("MinionDPS", "MinionTotalDPS"),
            ("MinionAttacksPerSecond", "MinionSpeed"),
            ("MinionAverageDamage", "MinionAverageDamage"),
            ("MinionCritFactor", "MinionCritEffect"),
        ] {
            if let Some(v) = n(key) {
                out.set(alias, v);
            }
        }
        let dot_only = n("TotalDPS").unwrap_or(0.0) == 0.0 && n("TotalDot").unwrap_or(0.0) > 0.0;
        out.set("MainSkillIsDotOnly", if dot_only { 1.0 } else { 0.0 });
        Some(out)
    }
}
