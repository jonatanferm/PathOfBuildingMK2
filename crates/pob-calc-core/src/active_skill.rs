//! Active skills and skill effects (calc doc §5.4; CalcActiveSkill.lua).
#![allow(non_snake_case)]

use std::cell::Cell;
use std::rc::{Rc, Weak};

use pob_gamedata::{GemDataRef, GrantedEffectRef, ModRef, SkillLevelRef};
use pob_lua::{lua_error, Field, Name, PtrSet, Shared};

use crate::actor::{ActorRef, ActorWeak};
use crate::cfg::{Cfg, ModCfg, SkillTypes};
use crate::classes::{ModList, ModStore};
use crate::lua_table::{LStr, Table};
use crate::model::{GemInstance, GemInstanceRef, SocketGroupRef};

pub type SkillEffectRef = Rc<SkillEffect>;

/// The `activeEffect` / `supportEffect` / `skillEffect` tables.
pub struct SkillEffect {
    pub grantedEffect: Field<GrantedEffectRef>,
    pub level: Cell<f64>,
    pub quality: Cell<f64>,
    /// The build model's gem table (identity used by cacheSkillUUID).
    pub srcInstance: Field<Option<GemInstanceRef>>,
    pub gemData: Field<Option<GemDataRef>>,
    pub gemCfg: Field<Option<Cfg>>,
    pub grantedEffectLevel: Field<Option<SkillLevelRef>>,
    pub isSupporting: Field<Option<Shared<PtrSet<GemInstance>>>>,
    pub superseded: Cell<bool>,
    pub appliesToGrantedSkills: Cell<bool>,
    pub fromItem: Cell<bool>,
    pub actorLevel: Cell<Option<f64>>,
    pub globalQuality: Cell<Option<f64>>,
    pub itemQuality: Cell<Option<f64>>,
    pub supportQuality: Cell<Option<f64>>,
    /// `effect.req = (effect.req or 0) + value.value` (applyGemMods, GemProperty `req`,
    /// CalcSetup.lua 382-418; read as `gemInstance.reqOverride`, 1705/1776): a number
    /// (CCR-0074 §6).
    pub req: Cell<Option<f64>>,
    pub gemPropertyInfo: Field<Option<Table>>,
    /// applyGemMods: `effect[value.key] = effect[value.key] + value.value` (CalcSetup.lua 382-418).
    pub extra: Table,
}

impl SkillEffect {
    /// `{ grantedEffect = ge, level = level, quality = quality }`.
    pub fn new(grantedEffect: GrantedEffectRef, level: f64, quality: f64) -> SkillEffectRef {
        Rc::new(SkillEffect {
            grantedEffect: Field::new(grantedEffect),
            level: Cell::new(level),
            quality: Cell::new(quality),
            srcInstance: Field::new(None),
            gemData: Field::new(None),
            gemCfg: Field::new(None),
            grantedEffectLevel: Field::new(None),
            isSupporting: Field::new(None),
            superseded: Cell::new(false),
            appliesToGrantedSkills: Cell::new(false),
            fromItem: Cell::new(false),
            actorLevel: Cell::new(None),
            globalQuality: Cell::new(None),
            itemQuality: Cell::new(None),
            supportQuality: Cell::new(None),
            req: Cell::new(None),
            gemPropertyInfo: Field::new(None),
            extra: Table::new(),
        })
    }
}

pub type ActiveSkillRef = Rc<ActiveSkill>;

/// One active skill (CalcActiveSkill.lua:83 `createActiveSkill`).
pub struct ActiveSkill {
    pub activeEffect: Field<SkillEffectRef>,
    pub supportList: Field<Shared<Vec<SkillEffectRef>>>,
    pub effectList: Field<Shared<Vec<SkillEffectRef>>>,
    /// May point into another env (copyActiveSkill, D2).
    pub actor: Field<ActorWeak>,
    /// The summoning skill of a minion's skill (`createActiveSkill(…, summonSkill)`,
    /// CalcActiveSkill.lua:89). A back-pointer, so `Weak` (calc doc §3.3 rule 4, CCR-0135): the
    /// summoning skill owns the minion (`activeSkill.minion`), which owns this skill. Read it
    /// with [`ActiveSkill::summonSkill`].
    pub summonSkill: Field<Option<ActiveSkillWeak>>,
    pub socketGroup: Field<Option<SocketGroupRef>>,
    pub skillTypes: Field<SkillTypes>,
    pub minionSkillTypes: Field<Option<SkillTypes>>,
    pub skillFlags: Field<Table>,
    pub skillData: Field<Table>,
    pub skillModList: Field<ModList>,
    pub baseSkillModList: Field<ModList>,
    pub extraSkillModList: Field<Shared<Vec<ModRef>>>,
    pub skillCfg: Field<Cfg>,
    pub weapon1Cfg: Field<Option<Cfg>>,
    pub weapon2Cfg: Field<Option<Cfg>>,
    pub weapon1Flags: Cell<Option<u32>>,
    pub weapon2Flags: Cell<Option<u32>>,
    pub bleedCfg: Field<Option<Cfg>>,
    pub poisonCfg: Field<Option<Cfg>>,
    pub igniteCfg: Field<Option<Cfg>>,
    pub OHbleedCfg: Field<Option<Cfg>>,
    pub OHpoisonCfg: Field<Option<Cfg>>,
    pub OHigniteCfg: Field<Option<Cfg>>,
    pub dotCfg: Field<Option<Cfg>>,
    pub decayCfg: Field<Option<Cfg>>,
    /// Buff tables (type, name, modList, cond, …) stay dynamic.
    pub buffList: Field<Shared<Vec<Table>>>,
    pub minion: Field<Option<ActorRef>>,
    pub minionList: Field<Shared<Vec<Name>>>,
    pub mirage: Field<Option<Table>>,
    pub skillPart: Cell<Option<f64>>,
    pub skillPartName: Field<Option<LStr>>,
    pub disableReason: Field<Option<LStr>>,
    pub infoMessage: Field<Option<LStr>>,
    pub infoMessage2: Field<Option<LStr>>,
    pub infoTrigger: Field<Option<LStr>>,
    pub conversionTable: Field<Option<Table>>,
    /// The trigger support's skill effect (`activeSkill.triggeredBy = skillEffect`,
    /// CalcActiveSkill.lua:536; the same table is in `supportList`/`effectList`). CalcTriggers
    /// adds run-time keys to it (`mainSkill`, `activationFreqInc`, …: the effect's `extra`) and
    /// reassigns its `grantedEffect` (CalcTriggers.lua:1225). CCR-0030 #1.
    pub triggeredBy: Field<Option<SkillEffectRef>>,
    pub slotName: Cell<Option<Name>>,
    pub skillTotemId: Cell<Option<f64>>,
    pub buffSkill: Cell<bool>,
    pub debuffSkill: Cell<bool>,
    pub totemBuffSkill: Cell<bool>,
    pub minionBuffSkill: Cell<bool>,
    pub activeMineCount: Cell<Option<f64>>,
    pub activeStageCount: Cell<Option<f64>>,
    pub extra: Table,
}

impl ActiveSkill {
    /// The table `createActiveSkill` starts from (CalcActiveSkill.lua 84-90):
    /// `{ activeEffect = …, supportList = …, actor = …, summonSkill = …,
    /// socketGroup = …, skillData = { }, buffList = { } }`. Every other field
    /// is nil / a fresh handle until AS assigns it.
    pub fn new(
        activeEffect: SkillEffectRef,
        supportList: Shared<Vec<SkillEffectRef>>,
        actor: ActorWeak,
    ) -> ActiveSkillRef {
        Rc::new(ActiveSkill {
            activeEffect: Field::new(activeEffect),
            supportList: Field::new(supportList),
            effectList: Field::new(Shared::new(Vec::new())),
            actor: Field::new(actor),
            summonSkill: Field::new(None),
            socketGroup: Field::new(None),
            skillTypes: Field::new(Shared::new(Default::default())),
            minionSkillTypes: Field::new(None),
            skillFlags: Field::new(Table::new()),
            skillData: Field::new(Table::new()),
            skillModList: Field::new(Rc::new(ModStore::default())),
            baseSkillModList: Field::new(Rc::new(ModStore::default())),
            extraSkillModList: Field::new(Shared::new(Vec::new())),
            skillCfg: Field::new(ModCfg::new()),
            weapon1Cfg: Field::new(None),
            weapon2Cfg: Field::new(None),
            weapon1Flags: Cell::new(None),
            weapon2Flags: Cell::new(None),
            bleedCfg: Field::new(None),
            poisonCfg: Field::new(None),
            igniteCfg: Field::new(None),
            OHbleedCfg: Field::new(None),
            OHpoisonCfg: Field::new(None),
            OHigniteCfg: Field::new(None),
            dotCfg: Field::new(None),
            decayCfg: Field::new(None),
            buffList: Field::new(Shared::new(Vec::new())),
            minion: Field::new(None),
            minionList: Field::new(Shared::new(Vec::new())),
            mirage: Field::new(None),
            skillPart: Cell::new(None),
            skillPartName: Field::new(None),
            disableReason: Field::new(None),
            infoMessage: Field::new(None),
            infoMessage2: Field::new(None),
            infoTrigger: Field::new(None),
            conversionTable: Field::new(None),
            triggeredBy: Field::new(None),
            slotName: Cell::new(None),
            skillTotemId: Cell::new(None),
            buffSkill: Cell::new(false),
            debuffSkill: Cell::new(false),
            totemBuffSkill: Cell::new(false),
            minionBuffSkill: Cell::new(false),
            activeMineCount: Cell::new(None),
            activeStageCount: Cell::new(None),
            extra: Table::new(),
        })
    }

    /// `activeSkill.actor`.
    pub fn actor(&self) -> Option<ActorRef> {
        self.actor.get().upgrade()
    }

    /// `activeSkill.summonSkill`: `None` where Lua's field is nil (not a minion's skill).
    ///
    /// CCR-0135: in Lua the summoning skill is alive as long as this skill is reachable (it owns
    /// the minion that owns this skill, or `copyActiveSkill` keeps a foreign one alive in
    /// `extra`), so the upgrade cannot fail. A dead handle is a Rust ownership bug, not a Lua
    /// nil, and panics instead of turning into a silent nil.
    pub fn summonSkill(&self) -> Option<ActiveSkillRef> {
        self.summonSkill.get().map(|w| {
            w.upgrade().unwrap_or_else(|| {
                panic!("activeSkill.summonSkill: the summoning skill was dropped (ownership bug, CCR-0135)")
            })
        })
    }

    /// `activeSkill[pass.label ~= "Off Hand" and "igniteCfg" or "OHigniteCfg"]`:
    /// computed field names over a fixed set.
    pub fn cfg_field(&self, name: Name) -> &Field<Option<Cfg>> {
        match name.as_str() {
            "weapon1Cfg" => &self.weapon1Cfg,
            "weapon2Cfg" => &self.weapon2Cfg,
            "bleedCfg" => &self.bleedCfg,
            "poisonCfg" => &self.poisonCfg,
            "igniteCfg" => &self.igniteCfg,
            "OHbleedCfg" => &self.OHbleedCfg,
            "OHpoisonCfg" => &self.OHpoisonCfg,
            "OHigniteCfg" => &self.OHigniteCfg,
            "dotCfg" => &self.dotCfg,
            "decayCfg" => &self.decayCfg,
            other => lua_error!("activeSkill.{other} is not a cfg field"),
        }
    }
}

/// A weak handle (for `Obj` storage that must not keep a skill alive).
pub type ActiveSkillWeak = Weak<ActiveSkill>;
