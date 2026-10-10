//! @pob-file Modules/CalcBreakdown.lua @16de4b82d
//! The breakdown helpers (Modules/CalcBreakdown.lua 1-255) as methods on [`Breakdown`] (calc doc §5.5).
//! `modDB`, `output` and `actor` are the module function's arguments (the
//! closures' upvalues), kept on the [`Breakdown`] value. Owner: CT.
//!
//! Parameter corrections against U0's reading (CCR-0014): `area` takes optional
//! breakpoints, label and radius modifier (CalcOffence.lua:198-223 returns nil
//! breakpoints for a zero radius; 1317/1401/1409 pass no label);
//! `effMult` takes optional `invertChance` / `effectiveResist` (CalcOffence.lua
//! 4587, 4858, 5199, 5209, 5720, 5836 pass eight arguments); `leech`'s `rate`
//! is a mod name.
#![allow(non_snake_case)]
// literal Lua statement shapes (`x = x * y`)
#![allow(clippy::assign_op_pattern)]

use crate::modules::calc_tools;
use crate::prelude::*;
use pob_lua::format::{lua_format, FmtArg};

/// @pob Modules/CalcBreakdown.lua:6-16 h=7a4100d0
/// Module header and `return function(modDB, output, actor) local breakdown = {}`: [`newBreakdown`].
const _HEADER: () = ();

/// `s_format(unpack(t))`: `t[1]` is the format, `t[2..#t]` the arguments.
fn s_format_unpack(t: &Table) -> String {
    let vals: Vec<Val> = (1..=t.len()).map(|i| t.rawget(i)).collect();
    s_format_vals(&vals)
}

/// `s_format(v1, v2, …)` over Lua values.
fn s_format_vals(vals: &[Val]) -> String {
    let fmt: String = match vals.first() {
        Some(Val::Str(s)) => s.to_string(),
        Some(Val::Num(n)) => tostring_num(*n),
        Some(other) => lua_error!(
            "bad argument #1 to 'format' (string expected, got {})",
            other.lua_type()
        ),
        None => lua_error!("bad argument #1 to 'format' (string expected, got no value)"),
    };
    let args: Vec<FmtArg<'_>> = vals[1..]
        .iter()
        .enumerate()
        .map(|(i, v)| match v {
            Val::Num(n) => FmtArg::Num(*n),
            Val::Str(s) => FmtArg::Str(s),
            Val::Bool(b) => FmtArg::Bool(*b),
            Val::Nil => FmtArg::Nil,
            other => lua_error!(
                "bad argument #{} to 'format' (a {} value has no Rust text)",
                i + 2,
                other.lua_type()
            ),
        })
        .collect();
    lua_format(&fmt, &args)
}

/// The breakdown module's `actor` upvalue.
fn upvalue_actor(b: &Breakdown) -> ActorRef {
    b.actor
        .upgrade()
        .unwrap_or_else(|| lua_error!("attempt to index upvalue 'actor' (a nil value)"))
}

/// @pob Modules/CalcBreakdown.lua:17-42 h=9cbb1b15
impl Breakdown {
    pub fn multiChain(&self, out: &Table, chain: &Table) -> f64 {
        // L18
        let base: Val = {
            let b = chain.get(n!("base"));
            let v = if b.truthy() { b.get(2_i64) } else { b };
            if v.truthy() {
                v
            } else {
                Val::Nil
            }
        };
        // L19
        let mut multiplier = 1.0;
        // L20 lines is the total number of non 1 multipliers.
        let mut lines = 0.0;
        // L21
        if chain.truthy(n!("label")) {
            // L22
            out.push(chain.get(n!("label")));
        }
        // L24
        if !base.is_nil() {
            // L25
            let b = chain
                .table(n!("base"))
                .unwrap_or_else(|| lua_error!("bad argument #1 to 'unpack' (table expected)"));
            out.push(s_format_unpack(&b));
        }
        // L27
        if !base.raw_eq(&Val::Num(0.0)) {
            // L28
            for_ipairs!((_, mult) in chain => {
                let mult = mult
                    .table()
                    .unwrap_or_else(|| lua_error!("attempt to index local 'mult' (a {} value)", mult.lua_type()));
                let m2 = mult.get(2_i64);
                // L29
                if m2.truthy() && !m2.raw_eq(&Val::Num(1.0)) {
                    // L30
                    multiplier = multiplier * m2.num();
                    // L31
                    out.push(format!("x {}", s_format_unpack(&mult)));
                    // L32
                    lines = lines + 1.0;
                }
            });
        }
        // L36
        if chain.truthy(n!("total")) {
            // L37
            out.push(chain.get(n!("total")));
        } else if (!chain.truthy(n!("noTotal")) && (lines > 0.0 && !base.is_nil()))
            || (lines > 1.0 && base.is_nil())
        {
            // L38
            // L39
            let b = if base.truthy() { base.num() } else { 1.0 };
            out.push(lfmt!("= %.2f", multiplier * b));
        }
        // L41
        lines
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
        let modDB = &self.modDB;
        // L45
        let extraBase = extraBase.unwrap_or(0.0);
        // L46 `(...)`: the first name only
        let base = modDB.Sum(ModType::BASE, cfg, &names[..names.len().min(1)]);
        // L47
        if (base + extraBase) != 0.0 {
            // L48
            let inc = modDB.Sum(ModType::INC, cfg, names);
            // L49
            let more = modDB.More(cfg, names);
            // L50
            if inc != 0.0 || more != 1.0 || (base != 0.0 && extraBase != 0.0) {
                // L51
                let out = Table::new();
                // L52
                if base != 0.0 && extraBase != 0.0 {
                    // L53
                    out.set(1_i64, lfmt!("(%g + %g) ^8(base)", extraBase, base));
                } else {
                    // L55
                    out.set(1_i64, lfmt!("%g ^8(base)", base + extraBase));
                }
                // L57
                if inc != 0.0 {
                    // L58
                    out.push(lfmt!("x %.2f ^8(increased/reduced)", 1.0 + inc / 100.0));
                }
                // L60
                if more != 1.0 {
                    // L61
                    out.push(lfmt!("x %.2f ^8(more/less)", more));
                }
                // L63
                out.push(lfmt!("= %g", total));
                // L64
                return Some(out);
            }
        }
        None
    }
}

/// @pob Modules/CalcBreakdown.lua:69-79 h=87b808a3
impl Breakdown {
    pub fn r#mod(&self, modList: &ModStore, cfg: Option<&ModCfg>, names: &[Name]) -> Option<Table> {
        // L70
        let inc = modList.Sum(ModType::INC, cfg, names);
        // L71
        let more = modList.More(cfg, names);
        // L72
        if inc != 0.0 || more != 1.0 {
            // L73
            return Some(tbl![
                lfmt!("%.2f ^8(increased/reduced)", 1.0 + inc / 100.0),
                lfmt!("x %.2f ^8(more/less)", more),
                lfmt!("= %.2f", (1.0 + inc / 100.0) * more),
            ]);
        }
        None
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
        let modDB = &self.modDB;
        // L82
        let inc = modDB.Sum(ModType::INC, cfg, names);
        // L83
        let more = modDB.More(cfg, names);
        // L84 `breakdown[...]`: the first name
        let first = names.first().copied();
        let slots = first
            .map(|k| self.t.get(k))
            .unwrap_or(Val::Nil)
            .get(n!("slots"))
            .table()
            .unwrap_or_else(|| lua_error!("bad argument #1 to 'insert' (table expected, got nil)"));
        let row = Table::new();
        row.set(n!("base"), base);
        row.set(
            n!("inc"),
            if inc != 0.0 {
                Val::from(lfmt!(" x %.2f", 1.0 + inc / 100.0))
            } else {
                Val::Bool(false)
            },
        );
        row.set(
            n!("more"),
            if more != 1.0 {
                Val::from(lfmt!(" x %.2f", more))
            } else {
                Val::Bool(false)
            },
        );
        row.set(
            n!("total"),
            lfmt!(
                "%.2f",
                total.unwrap_or_else(|| base * (1.0 + inc / 100.0) * more)
            ),
        );
        row.set(n!("source"), source);
        row.set(n!("sourceName"), sourceName.map_or(Val::Nil, Val::from));
        let item = upvalue_actor(self)
            .itemList
            .with(|l| l.get(&Name::from(source)).cloned());
        row.set(
            n!("item"),
            item.map_or(Val::Nil, |i| Val::Obj(i as Rc<dyn std::any::Any>)),
        );
        slots.push(row);
    }
}

/// @pob Modules/CalcBreakdown.lua:95-112 h=a0a310ec
impl Breakdown {
    pub fn area(
        &self,
        base: f64,
        areaMod: f64,
        total: f64,
        incBreakpoint: Option<f64>,
        moreBreakpoint: Option<f64>,
        redBreakpoint: Option<f64>,
        lessBreakpoint: Option<f64>,
        label: Option<&str>,
        radiusMod: Option<f64>,
    ) -> Table {
        // L96
        let out = Table::new();
        // L97 (a nil label inserts nothing)
        out.push(label.map_or(Val::Nil, Val::from));
        // L98
        if base != total {
            // L99
            out.push(lfmt!("%.1fm ^8(base radius)", base / 10.0));
            // L100
            out.push(lfmt!(
                "x %.2f ^8(square root of area of effect modifier)",
                m_floor(100.0 * m_sqrt(areaMod)) / 100.0
            ));
            // L101
            if let Some(radiusMod) = radiusMod {
                if radiusMod != 1.0 {
                    // L102
                    out.push(lfmt!("x %.2f ^8(final radius modifier)", radiusMod));
                }
            }
            // L104
            out.push(lfmt!("= %.1fm", total / 10.0));
        }
        // L106
        if let (
            Some(incBreakpoint),
            Some(moreBreakpoint),
            Some(redBreakpoint),
            Some(lessBreakpoint),
        ) = (incBreakpoint, moreBreakpoint, redBreakpoint, lessBreakpoint)
        {
            // L107
            out.push(lfmt!(
                "^8Next 0.1m breakpoint: %d%% increased AoE / a %d%% more AoE multiplier",
                incBreakpoint,
                moreBreakpoint
            ));
            // L108
            out.push(lfmt!(
                "^8Previous 0.1m breakpoint: %d%% reduced AoE / a %d%% less AoE multiplier",
                redBreakpoint,
                lessBreakpoint
            ));
        }
        // L110
        out.set(n!("radius"), total);
        // L111
        out
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
        invertChance: Option<f64>,
        effectiveResist: Option<f64>,
    ) -> Table {
        // L115
        let out = Table::new();
        // L116
        let resistForm = if damageType.as_str() == "Physical" {
            "physical damage reduction"
        } else {
            "resistance"
        };
        // L117
        let resistLabel = resistForm;
        // L118
        let effectiveResist = effectiveResist.unwrap_or(resist - pen);

        // L120
        if sourceRes.is_some_and(|s| s != damageType) {
            // L121
            out.push(lfmt!(
                "Enemy %s: %d%% ^8(%s)",
                resistLabel,
                resist,
                sourceRes.expect("checked")
            ));
        } else if resist != 0.0 {
            // L122
            // L123
            out.push(lfmt!("Enemy %s: %d%%", resistLabel, resist));
        }
        // L125
        if invertChance.is_some_and(|c| c != 0.0) && useRes {
            let invertChance = invertChance.expect("checked");
            // L126
            let normalResist = resist - pen;
            // L127
            let invertedResist = -resist - pen;
            // L128
            out.push("Effective resistance:");
            // L129
            out.push(lfmt!(
                "%g%% ^8(non-inverted hit after penetration)",
                normalResist
            ));
            // L130
            out.push(lfmt!(
                "%g%% ^8(inverted hit after penetration)",
                invertedResist
            ));
            // L131
            out.push(lfmt!(
                "= %g%% ^8(weighted average from %.0f%% inversion chance)",
                effectiveResist,
                invertChance * 100.0
            ));
        } else if pen != 0.0 || !useRes {
            // L132
            // L133
            out.push("Effective resistance:");
            // L134
            out.push(lfmt!("%d%% ^8(resistance)", resist));
            // L135
            if pen < 0.0 {
                // L136
                out.push(lfmt!("+ %d%% ^8(penetration)", -pen));
            } else if pen > 0.0 {
                // L137
                // L138
                out.push(lfmt!("- %d%% ^8(penetration)", pen));
            }
            // L140
            if !useRes {
                // L141
                out.push(lfmt!("x %d%% ^8(resistance ignored)", 0.0));
                // L142
                out.push(lfmt!("= %d%%", 0.0));
            } else {
                // L144
                out.push(lfmt!("= %d%%", resist - pen));
            }
        }
        // L147
        if useRes {
            // L148
            let chain = Table::new();
            chain.set(n!("label"), "Effective DPS modifier:");
            chain.push(tbl![
                "%.2f ^8(%s)",
                1.0 - effectiveResist / 100.0,
                resistForm
            ]);
            chain.push(tbl![
                "%.2f ^8(increased/reduced damage taken)",
                1.0 + taken / 100.0
            ]);
            chain.push(tbl!["%.2f ^8(more/less damage taken)", takenMore]);
            chain.set(n!("total"), lfmt!("= %.3f", mult));
            self.multiChain(&out, &chain);
        } else {
            // L156
            out.push("Effective DPS modifier:");
            // L157
            out.push(lfmt!("= %.3f ^8(increased/reduced damage taken)", mult));
        }
        // L159
        out
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
        // L163
        let chain = Table::new();
        chain.set(n!("base"), tbl!["%.1f ^8(base damage per second)", baseVal]);
        chain.push(tbl!["%.2f ^8(increased/reduced)", 1.0 + inc / 100.0]);
        chain.push(tbl!["%.2f ^8(more/less)", more]);
        chain.push(tbl![
            "%.2f ^8(multiplier)",
            1.0 + mult.unwrap_or(0.0) / 100.0
        ]);
        chain.push(tbl!["%.2f ^8(rate modifier)", Val::from(rate)]);
        chain.push(tbl!["%.3f ^8(aura effect modifier)", Val::from(aura)]);
        chain.push(tbl!["%.3f ^8(effective DPS modifier)", Val::from(effMult)]);
        chain.set(n!("total"), lfmt!("= %.1f ^8per second", total));
        self.multiChain(out, &chain);
    }
}

/// @pob Modules/CalcBreakdown.lua:175-196 h=0bd1f3fe
impl Breakdown {
    pub fn critDot(&self, dotMulti: f64, critMulti: f64, dotChance: f64, critChance: f64) -> Table {
        // L176
        let combined = (dotMulti * dotChance) + (critMulti * critChance);
        // L177
        let out = Table::new();
        // L178
        if dotChance > 0.0 {
            // L179
            out.push(lfmt!("Contribution from Non-crits:"));
            // L180
            out.push(lfmt!("%.2f ^8(dot multiplier for non-crits)", dotMulti));
            // L181
            out.push(lfmt!(
                "x %.4f ^8(portion of instances created by non-crits)",
                dotChance
            ));
            // L182
            out.push(lfmt!("= %.2f", dotMulti * dotChance));
        }
        // L184
        if critChance > 0.0 {
            // L185
            out.push(lfmt!("Contribution from Crits:"));
            // L186
            out.push(lfmt!("%.2f ^8(dot multiplier for crits)", critMulti));
            // L187
            out.push(lfmt!(
                "x %.4f ^8(portion of instances created by crits)",
                critChance
            ));
            // L188
            out.push(lfmt!("= %.2f", critMulti * critChance));
        }
        // L190
        if (dotChance > 0.0 && critChance > 0.0) && (dotMulti != critMulti) {
            // L191
            out.push(lfmt!("Effective DoT Multiplier:"));
            // L192
            out.push(lfmt!(
                "%.2f + %.2f",
                dotMulti * dotChance,
                critMulti * critChance
            ));
            // L193
            out.push(lfmt!("= %.2f", combined));
        }
        // L195
        out
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
        let modDB = &self.modDB;
        let LeechRateBase = data().misc.LeechRateBase;
        // L199
        let out = Table::new();
        // L200
        let mainSkill = upvalue_actor(self)
            .mainSkill
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"));
        if mainSkill.skillData.get().truthy(n!("showAverage")) {
            // L201
            if instant > 0.0 {
                // L202
                if instantLeechProportion != 1.0 {
                    // L203
                    out.push(lfmt!(
                        "Instant Leech: %.1f ^8(%d%% x %.1f)",
                        instant,
                        instantLeechProportion * 100.0,
                        dur * pool * LeechRateBase / (1.0 - instantLeechProportion)
                    ));
                } else {
                    // L205
                    out.push(lfmt!("Instant Leech: %.1f", instant));
                }
            }
            // L208
            if instances > 0.0 {
                // L209
                out.push("Total leeched per instance:");
                // L210
                out.push(lfmt!("%d ^8(size of leech destination pool)", pool));
                // L211
                out.push(lfmt!(
                    "x %.2f ^8(base leech rate is %d%% per second)",
                    LeechRateBase,
                    100.0 * LeechRateBase
                ));
                // L212 LUA-GLOBAL-NIL: `skillCfg` is an undeclared global
                let rateMod = calc_tools::r#mod(modDB, None, &[rate]);
                // L213
                if rateMod != 1.0 {
                    // L214
                    out.push(lfmt!("x %.2f ^8(leech rate modifier)", rateMod));
                }
                // L216
                out.push(lfmt!("x %.2fs ^8(instance duration)", dur));
                // L217
                out.push(lfmt!("= %.1f", pool * LeechRateBase * rateMod * dur));
            }
        } else {
            // L220
            if instantRate > 0.0 {
                // L221
                if instantLeechProportion != 1.0 {
                    // L222
                    out.push(lfmt!(
                        "Instant Leech: %.1f ^8(%d%% x %.1f)",
                        instant,
                        instantLeechProportion * 100.0,
                        dur * pool * LeechRateBase / (1.0 - instantLeechProportion)
                    ));
                } else {
                    // L224
                    out.push(lfmt!("Instant Leech: %.1f", instant));
                }
                // L226
                out.push(lfmt!(
                    "Instant Leech per second: %.1f ^8(%.1f x %.2f)",
                    instantRate,
                    instant,
                    hitRate
                ));
            }
            // L228
            if instances > 0.0 {
                // L229
                out.push("Rate per instance:");
                // L230
                out.push(lfmt!("%d ^8(size of leech destination pool)", pool));
                // L231
                out.push(lfmt!(
                    "x %.2f ^8(base leech rate is %d%% per second)",
                    LeechRateBase,
                    100.0 * LeechRateBase
                ));
                // L232 LUA-GLOBAL-NIL: `skillCfg` is an undeclared global
                let rateMod = calc_tools::r#mod(modDB, None, &[rate]);
                // L233
                if rateMod != 1.0 {
                    // L234
                    out.push(lfmt!("x %.2f ^8(leech rate modifier)", rateMod));
                }
                // L236
                out.push(lfmt!("= %.1f ^8per second", pool * LeechRateBase * rateMod));
                // L237
                out.push("Maximum leech rate against one target:");
                // L238
                out.push(lfmt!("%.1f", pool * LeechRateBase * rateMod));
                // L239
                out.push(lfmt!("x %.2f ^8(average instances)", instances));
                // L240
                let total = pool * LeechRateBase * rateMod * instances;
                // L241
                out.push(lfmt!("= %.1f ^8per second", total));
                // L242
                if total <= max {
                    // L243
                    out.push(lfmt!("Time to reach max: %.1fs", dur));
                }
                // L245
                out.push(lfmt!("Leech rate cap: %.1f", max));
                // L246
                if total > max {
                    // L247
                    out.push(lfmt!("Time to reach cap: %.1fs", dur / total * max));
                }
            }
        }
        // L251
        out
    }
}

/// @pob Modules/CalcBreakdown.lua:254-255 h=756ccb16
/// `return breakdown end`.
const _FOOTER: () = ();
