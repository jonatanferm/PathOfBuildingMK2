//! The L4 stage dispatcher of the oracle (feature `probe`; calc doc §9.2), moved here from
//! `probe.rs` (owner ORC-X) because it calls the segments of every calc crate (CCR-0131;
//! docs/port/calc-split-plan.md B7). `pob_calc::probe::run_segment` re-exports it, so the
//! oracle tests' path is unchanged.
#![allow(non_snake_case)]

use crate::prelude::*;
use crate::probe::{ctx_fill, ctx_out, ProbeField};

/// Run the Rust segment that starts at line stage `stage` (a
/// docs/port/stages.toml id, `#n` ignored) on a ctx built from injected
/// `locals`; returns the segment's flow and the ctx locals afterwards
/// (pass-loop locals included for the pass-body stages).
pub fn run_segment(
    stage: &str,
    locals: &[(String, Val)],
) -> Result<(crate::flow::Flow, Vec<(String, Val)>), String> {
    use crate::modules::{
        calc_defence as cd, calc_offence as co, calc_perform as cp, calc_setup as cs,
    };
    let id = stage.split_once('#').map_or(stage, |(a, _)| a);
    let get = |k: &str| {
        locals
            .iter()
            .find(|(n, _)| n == k)
            .map_or(Val::Nil, |(_, v)| v.clone())
    };
    let env = || get("env").obj::<Env>().ok_or("local env is not an env");
    let actor = || get("actor").obj::<Actor>().ok_or("local actor is not an actor");
    let mut out = Vec::new();
    let flow = match id {
        "initEnv:488" | "initEnv:800" | "initEnv:1451" => {
            let build = get("build")
                .obj::<Build>()
                .ok_or("local build is not the build")?;
            let mode = Mode::from_val(&get("mode")).ok_or("local mode")?;
            let spec = SpecEnv::from_val(&get("specEnv"));
            let mut c = cs::InitEnvCtx::init(
                &build,
                mode,
                get("override").obj::<Override>(),
                spec.as_ref(),
            );
            ctx_fill(&mut c, locals)?;
            let f = match id {
                "initEnv:488" => cs::init_env_base::seg(&mut c),
                "initEnv:800" => cs::init_env_items::seg(&mut c),
                _ => cs::init_env_skills::seg(&mut c),
            };
            ctx_out(&c, &mut out);
            f
        }
        "perform:1289" | "perform:1943" | "perform:2308" | "perform:2997" | "perform:3497" => {
            let skip = matches!(get("skipEHP"), Val::Bool(true));
            let mut c = cp::PerformCtx::init(&env()?, skip);
            ctx_fill(&mut c, locals)?;
            let f = match id {
                "perform:1289" => cp::setup_flasks::seg(&mut c),
                "perform:1943" => cp::pools_charges::seg(&mut c),
                "perform:2308" => cp::buffs::seg(&mut c),
                "perform:2997" => cp::party_curses::seg(&mut c),
                _ => cp::ailments_dispatch::seg(&mut c),
            };
            ctx_out(&c, &mut out);
            f
        }
        "defence:659" | "defence:1211" => {
            let mut c = cd::DefenceCtx::init(&env()?, &actor()?);
            ctx_fill(&mut c, locals)?;
            let f = if id == "defence:659" {
                cd::primary::seg(&mut c)
            } else {
                cd::recovery::seg(&mut c)
            };
            ctx_out(&c, &mut out);
            f
        }
        "buildDefenceEstimations:1681"
        | "buildDefenceEstimations:2153"
        | "buildDefenceEstimations:2612"
        | "buildDefenceEstimations:3165" => {
            let mut c = cd::DefEstCtx::init(&env()?, &actor()?);
            ctx_fill(&mut c, locals)?;
            let f = match id {
                "buildDefenceEstimations:1681" => cd::est_inputs::seg(&mut c),
                "buildDefenceEstimations:2153" => cd::est_pools::seg(&mut c),
                "buildDefenceEstimations:2612" => cd::est_ehp::seg(&mut c),
                _ => cd::est_maxhit::seg(&mut c),
            };
            ctx_out(&c, &mut out);
            f
        }
        _ if id.starts_with("offence:") => {
            let skill = get("activeSkill")
                .obj::<ActiveSkill>()
                .ok_or("local activeSkill is not an active skill")?;
            let actor = actor()?;
            let mut c = co::OffenceCtx::init(&env()?, &actor, &skill);
            ctx_fill(&mut c, locals)?;
            if matches!(id, "offence:3346" | "offence:4990") {
                // In the pass bodies `output`/`breakdown` are the loop locals (CalcOffence.lua
                // 2610/2613, 4168/4171), which fill the pass locals below; the function-level
                // ones are `actor.output`/`actor.breakdown` (354/356), never reassigned. CCR-0049 §2.
                c.output = actor.output.get();
                c.breakdown = actor.breakdown.get();
            }
            if !matches!(id, "offence:362" | "offence:1037" | "offence:1692") {
                // The module upvalues `globalOutput, globalBreakdown` (65/67) are not dumped.
                // From 2174 on every segment runs after (or, for 2174/2607/4165, begins with)
                // `globalOutput, globalBreakdown = output, breakdown` (2176/2607/4165), which
                // copies the function-level locals. CCR-0075.
                crate::module_state::OFFENCE_STATE.with(|s| {
                    s.globalOutput.set(Some(actor.output.get()));
                    s.globalBreakdown.set(actor.breakdown.get());
                });
            }
            let pass = || get("pass").table().ok_or("local pass is not a table");
            let mut pass_out = Vec::new();
            let f = match id {
                "offence:362" => co::helpers_setup::seg_setup(&mut c),
                "offence:1037" => co::skill_types::seg_skillTypeStats(&mut c),
                "offence:1692" => co::costs_passes::seg_costsAndPasses(&mut c),
                "offence:2174" => co::speed::seg_speed(&mut c),
                "offence:2607" | "offence:3346" => {
                    let mut hp = co::HitPassLocals::new(pass()?);
                    ctx_fill(&mut hp, locals)?;
                    let f = if id == "offence:2607" {
                        co::crit::seg_passCrit(&mut c, &mut hp)
                    } else {
                        co::hit_dps::seg_passHit(&mut c, &mut hp)
                    };
                    ctx_out(&hp, &mut pass_out);
                    f
                }
                "offence:3957" => co::hit_dps::seg_combinePasses(&mut c),
                "offence:4155" => co::ailments_a::seg_ailmentPrelude(&mut c),
                "offence:4165" | "offence:4990" => {
                    let mut al = co::AilmentPassLocals::new(pass()?);
                    ctx_fill(&mut al, locals)?;
                    let f = if id == "offence:4165" {
                        co::ailments_a::seg_bleedPoison(&mut c, &mut al)
                    } else {
                        co::ailments_b::seg_igniteNonDmgImpale(&mut c, &mut al)
                    };
                    ctx_out(&al, &mut pass_out);
                    f
                }
                "offence:5650" => co::dot_combined::seg_dotCombined(&mut c),
                other => return Err(format!("no segment starts at {other}")),
            };
            ctx_out(&c, &mut out);
            for (k, v) in pass_out {
                if let Some(slot) = out.iter_mut().find(|(n, _)| *n == k) {
                    slot.1 = v;
                } else {
                    out.push((k, v));
                }
            }
            f
        }
        other => return Err(format!("no segment starts at {other}")),
    };
    Ok((flow, out))
}
