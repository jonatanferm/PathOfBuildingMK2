//! @pob-file Modules/CalcBreakdown.lua @16de4b82d
//! The breakdown helpers (Modules/CalcBreakdown.lua 1-255) as methods on [`Breakdown`] (calc doc §5.5).
//! The parameter lists are U0's reading of the Lua; CT corrects any that differ (CCR, §5.5).
//!
//! Owner: CT. Stub created by U0: signatures are final (frozen; change through a CCR),
//! bodies are `unported!` until the owner ports them (calc doc D9, §10.2).
#![allow(
    non_snake_case,
    unused_variables,
    unused_imports,
    dead_code,
    clippy::all,
    clippy::pedantic
)]

use crate::prelude::*;

/// @pob Modules/CalcBreakdown.lua:6-16 h=7a4100d0
/// Module header and `return function(modDB, output, actor) local breakdown = {}`: [`newBreakdown`].
const _HEADER: () = ();

/// @pob Modules/CalcBreakdown.lua:17-42 h=9cbb1b15
impl Breakdown {
    pub fn multiChain(&self, out: &Table, chain: &Table) -> f64 {
        unported!("Modules/CalcBreakdown.lua:17-42")
    }
}

/// @pob Modules/CalcBreakdown.lua:44-67 h=6a6d5a88
impl Breakdown {
    pub fn simple(
        &self,
        extraBase: Option<f64>,
        cfg: Option<&ModCfg>,
        total: f64,
        names: &[Name],
    ) -> Option<Table> {
        unported!("Modules/CalcBreakdown.lua:44-67")
    }
}

/// @pob Modules/CalcBreakdown.lua:69-79 h=87b808a3
impl Breakdown {
    pub fn r#mod(&self, modList: &ModStore, cfg: Option<&ModCfg>, names: &[Name]) -> Option<Table> {
        unported!("Modules/CalcBreakdown.lua:69-79")
    }
}

/// @pob Modules/CalcBreakdown.lua:81-93 h=56c863d4
impl Breakdown {
    pub fn slot(
        &self,
        source: &str,
        sourceName: Option<&str>,
        cfg: Option<&ModCfg>,
        base: f64,
        total: Option<f64>,
        names: &[Name],
    ) {
        unported!("Modules/CalcBreakdown.lua:81-93")
    }
}

/// @pob Modules/CalcBreakdown.lua:95-112 h=a0a310ec
impl Breakdown {
    pub fn area(
        &self,
        base: f64,
        areaMod: f64,
        total: f64,
        incBreakpoint: f64,
        moreBreakpoint: f64,
        redBreakpoint: f64,
        lessBreakpoint: f64,
        label: &str,
        radiusMod: Option<f64>,
    ) -> Table {
        unported!("Modules/CalcBreakdown.lua:95-112")
    }
}

/// @pob Modules/CalcBreakdown.lua:114-160 h=4f67c15a
impl Breakdown {
    pub fn effMult(
        &self,
        damageType: Name,
        resist: f64,
        pen: f64,
        taken: f64,
        mult: f64,
        takenMore: f64,
        sourceRes: Option<Name>,
        useRes: bool,
        invertChance: f64,
        effectiveResist: f64,
    ) -> Table {
        unported!("Modules/CalcBreakdown.lua:114-160")
    }
}

/// @pob Modules/CalcBreakdown.lua:162-173 h=1aa14123
impl Breakdown {
    pub fn dot(
        &self,
        out: &Table,
        baseVal: f64,
        inc: f64,
        more: f64,
        mult: Option<f64>,
        rate: Option<f64>,
        aura: Option<f64>,
        effMult: Option<f64>,
        total: f64,
    ) {
        unported!("Modules/CalcBreakdown.lua:162-173")
    }
}

/// @pob Modules/CalcBreakdown.lua:175-196 h=0bd1f3fe
impl Breakdown {
    pub fn critDot(&self, dotMulti: f64, critMulti: f64, dotChance: f64, critChance: f64) -> Table {
        unported!("Modules/CalcBreakdown.lua:175-196")
    }
}

/// @pob Modules/CalcBreakdown.lua:198-252 h=6ce8d6aa
impl Breakdown {
    /// `skillCfg` at CalcBreakdown.lua:212/232 is an undeclared global (nil, CCR-0004): LUA-GLOBAL-NIL.
    pub fn leech(
        &self,
        instant: f64,
        instantRate: f64,
        instances: f64,
        pool: f64,
        rate: Name,
        max: f64,
        dur: f64,
        instantLeechProportion: f64,
        hitRate: f64,
    ) -> Table {
        unported!("Modules/CalcBreakdown.lua:198-252")
    }
}

/// @pob Modules/CalcBreakdown.lua:254-255 h=756ccb16
/// `return breakdown end`.
const _FOOTER: () = ();
