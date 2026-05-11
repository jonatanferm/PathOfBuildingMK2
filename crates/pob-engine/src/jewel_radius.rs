//! Radius-jewel framework — issue #31.
//!
//! This module owns the generic mechanic that lets a jewel socketed in a passive
//! tree socket modify the passive nodes inside its radius. It does **not** implement
//! the timeless-jewel notable/keystone replacement (issue #30) or the cluster-jewel
//! sub-graph synthesis (issue #21); those will plug into the same handler-dispatch
//! surface in follow-ups.
//!
//! ## What's covered
//!
//! 1. Computing the Cartesian position of any tree node — mirrors PoB's
//!    `Classes/PassiveTree.lua:828-833` (group `(x, y)` + orbit-radius +
//!    orbit-index angle).
//! 2. Enumerating allocated passives within a jewel-socket's radius for a chosen
//!    radius bucket (Small/Medium/Large/Very Large/Massive plus PoB's "Variable"
//!    donut bands).
//! 3. Identifying a socketed item as a "vanilla" radius jewel by its mod text
//!    (lines that mention `Passives in Radius` and an explicit ring size like
//!    `in Small Ring`, `in Medium Ring`, `in Large Ring`).
//! 4. Applying a radius jewel's mods to the env: each in-radius allocated passive
//!    receives one copy of every mod line on the jewel, sourced as
//!    `Source::Passive(<that node's id>)` so the per-node breakdown attributes the
//!    bonus to the in-radius node and a `RadiusJewel:<base>` source label so the
//!    Calcs-tab can find it.
//!
//! ## What's covered (Issue #196 named uniques)
//!
//! - **Watcher's Eye** ([`HandlerKind::WatchersEye`]) — aura-conditional global
//!   buff; mods carry per-aura `AffectedBy<Aura>` Condition tags emitted by the
//!   parser, gated by the `detect_active_auras` pass in `perform.rs`.
//! - **Healthy Mind** ([`HandlerKind::LifeToManaTransform`]) — transforms each
//!   in-radius allocated node's `Inc Life` mod into an `Inc Mana` mod at 200%.
//! - **Fertile Mind** ([`HandlerKind::DexToIntTransform`]) — transforms each
//!   in-radius allocated node's `+N Dex` BASE mod into an `+N Int` BASE plus a
//!   counter `-N Dex` so the source attribute is moved, not duplicated.
//! - **Brawn** ([`HandlerKind::StrDoubleTransform`]) — emits an extra `+N
//!   Strength` BASE per in-radius `+N Strength` BASE, doubling the
//!   contribution. No counter (cf. [`HandlerKind::StrToLifeTransform`]).
//! - **Energy from Within** ([`HandlerKind::LifeToEnergyShieldTransform`])
//!   — transforms each in-radius `Inc Life` into `Inc EnergyShield` at 1×.
//!   Mirror of Healthy Mind's Life→Mana, with ES instead of Mana and
//!   without the 2× scale factor.
//! - **Fluid Motion** ([`HandlerKind::StrToDexTransform`]) — mirror of
//!   Inertia at 1× scale: Strength on in-radius allocated nodes is
//!   moved into Dexterity (counter `-N Str` cancels the original).
//! - **Inertia** ([`HandlerKind::DexToStrTransform`]) — transforms each
//!   in-radius `+N Dex` BASE into `+N Str` BASE. Mirror of Fertile
//!   Mind's Dex→Int, with Strength as the destination.
//! - **Brute Force Solution / Careful Planning / Efficient Training**
//!   ([`HandlerKind::StrToIntTransform`] / [`HandlerKind::IntToDexTransform`] /
//!   [`HandlerKind::IntToStrTransform`]) — the remaining three
//!   permutations of the BASE attribute transform pattern, all
//!   sharing one dispatch arm via `transform_radius_attribute`.
//! - **Energised Armour** ([`HandlerKind::EnergyShieldToArmourTransform`])
//!   — transforms each in-radius `Inc EnergyShield` into `Inc Armour`
//!   at 2× scale. Mirror of Healthy Mind's Life→Mana@2× pattern.
//! - **Fireborn** ([`HandlerKind::OtherDamageToFireTransform`]) — every
//!   non-Fire damage type's Inc on in-radius allocated nodes
//!   (Phys / Cold / Lightning / Chaos) is additively re-emitted as Inc
//!   FireDamage at 1× scale.
//! - **Cold Steel** ([`HandlerKind::PhysColdSwapTransform`]) — Phys ↔
//!   Cold Inc double-transform applied additively (each in-radius
//!   node contributes to both damage types).
//! - **Anatomical Knowledge** ([`HandlerKind::IntCountToLife`]) —
//!   first per-attribute-tally radius pattern: sums in-radius
//!   `+N Int` BASE mods, integer-divides by 3, emits `+(N/3) Life`
//!   sourced as the jewel.
//! - **Static Electricity** ([`HandlerKind::DexCountToMaxLightningAttack`]) —
//!   Dexterity summed across in-radius allocated nodes; emits a single
//!   `LightningDamage` BASE mod with `ModValue::Range { 0, dex }` and the
//!   ATTACK / LIGHTNING flags so it slots into `perform.rs`'s flat-damage
//!   adds loop alongside vanilla `Adds N to M Lightning Damage to Attacks`.
//! - **Eldritch Knowledge** ([`HandlerKind::IntCountToIncChaosDamage`]) —
//!   Intelligence summed across in-radius allocated nodes, integer-divided
//!   by 10, then multiplied by 5 to produce an Inc ChaosDamage mod sourced
//!   as the jewel.
//! - **Spire of Stone** ([`HandlerKind::StrCountToIncTotemLife`]) — Strength
//!   summed across in-radius allocated nodes, integer-divided by 10, then
//!   multiplied by 3 to produce an Inc TotemLife mod sourced as the jewel.
//! - **Tempered / Transcendent Flesh**
//!   ([`HandlerKind::StrCountToIncLifeRecovery`]) — Strength summed across
//!   in-radius allocated nodes, integer-divided by 10, then multiplied by
//!   the per-line rate (2 for Tempered Flesh / Transcendent Flesh pre-3.10,
//!   3 for current Transcendent Flesh) to produce an Inc LifeRecovery mod
//!   sourced as the jewel. Sister to Tempered/Transcendent Spirit but
//!   Str-sourced and emits Inc LifeRecovery instead of Inc MovementSpeed.
//! - **Might in All Forms** ([`HandlerKind::DexIntToStrMeleeBonus`]) —
//!   Dex + Int summed across in-radius allocated nodes, emitted straight
//!   into a single `DexIntToMeleeBonus` BASE mod. PoB folds the same stat
//!   back into the Strength → Melee Damage bonus formula.
//! - **Pugilist** ([`HandlerKind::DexCountToIncEvasion`]) — Dex
//!   variant of the per-attribute-tally pattern emitting Inc
//!   Evasion. The two per-claw / per-unarmed lines on the same
//!   jewel need weapon-condition tags and follow as a future slice.
//! - **Tempered / Transcendent Spirit**
//!   ([`HandlerKind::DexCountToIncMovementSpeed`]) — Dex summed across
//!   in-radius allocated nodes, integer-divided by 10, then multiplied
//!   by the per-line rate (2 for Tempered Spirit / Transcendent pre-3.10,
//!   3 for current Transcendent Spirit) to produce an Inc MovementSpeed
//!   mod sourced as the jewel.
//! - **Tempered / Transcendent Mind**
//!   ([`HandlerKind::IntCountToIncManaRecovery`]) — Intelligence summed
//!   across in-radius allocated nodes, integer-divided by 10, then
//!   multiplied by the per-line rate (2 for Tempered Mind, 3 for current
//!   Transcendent Mind) to produce an Inc ManaRecovery mod sourced as the
//!   jewel. Sister to Tempered/Transcendent Spirit / Flesh but
//!   Int-sourced and emits Inc ManaRecovery (the engine's name for the
//!   per-second mana-recovery-rate stat — see `mod_parser.rs`
//!   `"Mana Recovery rate" => "ManaRecovery"`).
//! - **Karui Heart** ([`HandlerKind::StrToLifeTransform`]) — transforms each
//!   in-radius allocated node's `+N Str` BASE mod into a `+5N Life` BASE plus
//!   a counter `-N Str` so the strength is moved (not duplicated). The 5×
//!   factor mirrors PoB's `data/uniques/jewel.lua` Karui Heart handler: each
//!   transformed Str converts to +5 Life directly, *replacing* the +0.5 Life
//!   the Str would otherwise have provided through the standard 1 Str =
//!   0.5 Life ratio.
//!
//! ## What's deferred
//!
//! - Timeless jewels (#30): keystone / notable substitution.
//! - Cluster jewel sub-graph synthesis (#21): nodes spawned by a Cluster jewel.
//! - Intuitive Leap (#196 follow-up): pathfind-side connectivity skip.
//! - Pure Talent (#196 follow-up): class-conditional notable buff.
//! - Conqueror's Efficiency (#196 deferred): not actually radius-scoped — the
//!   live unique grants flat global mods only.

use ahash::{AHashMap, AHashSet};
use pob_data::{
    radii_for_tree_version, radius_index_for_label, Item, JewelRadiusInfo, NodeId, NodeKind,
    PassiveTree,
};
use serde::{Deserialize, Serialize};

use crate::mod_parser::parse_mod_line;
use crate::modifier::{Mod, Source};

/// Compute the Cartesian position of a tree node, in the same coordinate space the
/// passive tree's `(min_x, min_y)..(max_x, max_y)` rect uses. Mirrors
/// `PassiveTree.lua:828-833`:
///
/// ```text
///   x = group.x + sin(angle) * orbit_radii[orbit]
///   y = group.y - cos(angle) * orbit_radii[orbit]
/// ```
///
/// Returns `None` for nodes that lack a group/orbit/orbit_index (cluster-jewel
/// notable templates that haven't been placed yet, the synthetic root, etc.).
pub fn node_position(tree: &PassiveTree, node_id: NodeId) -> Option<(f64, f64)> {
    let node = tree.nodes.get(&node_id)?;
    let group = tree.groups.get(&node.group?)?;
    let orbit = node.orbit.unwrap_or(0) as usize;
    let orbit_index = node.orbit_index.unwrap_or(0) as usize;
    let radius = *tree.constants.orbit_radii.get(orbit).unwrap_or(&0) as f64;
    let nodes_in_orbit = tree
        .constants
        .skills_per_orbit
        .get(orbit)
        .copied()
        .unwrap_or(1);
    let angle = orbit_angle_rad(nodes_in_orbit, orbit_index);
    let x = f64::from(group.x) + angle.sin() * radius;
    let y = f64::from(group.y) - angle.cos() * radius;
    Some((x, y))
}

/// Per-orbit angle table. Mirrors `PassiveTree.lua:CalcOrbitAngles`. The 16- and
/// 40-slot orbits use bespoke degree tables; everything else is evenly spaced.
fn orbit_angle_rad(nodes_in_orbit: u32, orbit_index: usize) -> f64 {
    const TABLE_16: [f64; 16] = [
        0.0, 30.0, 45.0, 60.0, 90.0, 120.0, 135.0, 150.0, 180.0, 210.0, 225.0, 240.0, 270.0, 300.0,
        315.0, 330.0,
    ];
    const TABLE_40: [f64; 40] = [
        0.0, 10.0, 20.0, 30.0, 40.0, 45.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0, 110.0, 120.0,
        130.0, 135.0, 140.0, 150.0, 160.0, 170.0, 180.0, 190.0, 200.0, 210.0, 220.0, 225.0, 230.0,
        240.0, 250.0, 260.0, 270.0, 280.0, 290.0, 300.0, 310.0, 315.0, 320.0, 330.0, 340.0, 350.0,
    ];
    let deg = match nodes_in_orbit {
        16 if orbit_index < 16 => TABLE_16[orbit_index],
        40 if orbit_index < 40 => TABLE_40[orbit_index],
        n if n > 0 => 360.0 * (orbit_index as f64) / f64::from(n),
        _ => 0.0,
    };
    deg.to_radians()
}

/// Result of an in-radius scan: every node id that falls inside the chosen radius
/// band, keyed by node id, with the squared distance from the socket. The squared
/// distance lets callers further bucketise when a jewel cares about narrower bands
/// (e.g. timeless jewels' inner/outer pair).
pub fn nodes_in_radius(
    tree: &PassiveTree,
    socket_id: NodeId,
    radius: &JewelRadiusInfo,
) -> Vec<(NodeId, f64)> {
    let Some((sx, sy)) = node_position(tree, socket_id) else {
        return Vec::new();
    };
    let outer_sq = radius.outer * radius.outer;
    let inner_sq = radius.inner * radius.inner;
    let mut out: Vec<(NodeId, f64)> = Vec::new();
    for (id, node) in &tree.nodes {
        // PoB skips the socket itself, mastery nodes, and proxy/blighted nodes when
        // building `nodesInRadius`. Mirror that — we don't want a jewel to inject
        // its own mods into itself, and mastery effects come from the player's
        // `mastery_selections` not the jewel.
        if *id == socket_id {
            continue;
        }
        if matches!(
            node.kind,
            NodeKind::Mastery | NodeKind::Root | NodeKind::ClassStart | NodeKind::AscendancyStart
        ) {
            continue;
        }
        let Some((x, y)) = node_position(tree, *id) else {
            continue;
        };
        let dx = x - sx;
        let dy = y - sy;
        let dist_sq = dx * dx + dy * dy;
        if dist_sq >= inner_sq && dist_sq <= outer_sq {
            out.push((*id, dist_sq));
        }
    }
    out
}

/// Filter [`nodes_in_radius`] down to nodes the character has actually allocated.
/// PoB's first-pass radius dispatch (the "Self" handler) only modifies allocated
/// nodes; nearby unallocated nodes feed the second-pass / extra-node list, which
/// vanilla node-modifying jewels don't drive.
pub fn allocated_nodes_in_radius(
    tree: &PassiveTree,
    socket_id: NodeId,
    radius: &JewelRadiusInfo,
    allocated: &AHashSet<NodeId>,
) -> Vec<(NodeId, f64)> {
    nodes_in_radius(tree, socket_id, radius)
        .into_iter()
        .filter(|(id, _)| allocated.contains(id))
        .collect()
}

/// Handler kind. Mirrors PoB's `radiusJewelList[i].type` field. The framework's
/// default is `SelfAllocated` (PoB's `"Self"`); named-unique handlers — Issue
/// #196 — extend this enum with bespoke per-jewel logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HandlerKind {
    /// Apply the jewel's mods to every allocated passive in radius, copying each
    /// mod once per in-radius node so per-node breakdowns sum up correctly.
    /// Mirrors PoB's `"Self"` handler.
    SelfAllocated,
    /// Apply to every node in radius regardless of allocation. Used by jewels
    /// like Lethal Pride (which conquers the whole radius, allocated or not).
    /// Reserved for the timeless follow-up.
    All,
    /// Apply once globally per allocated passive matching a threshold (e.g.
    /// "with at least 40 Strength in Radius, X"). The radius pass tallies the
    /// triggering stat and emits the conditional mod once. Reserved for the
    /// threshold-jewel follow-up.
    Threshold,
    /// Apply to nearby unallocated passives — used by conversion jewels that
    /// transform an *unallocated* node's mod set. Reserved for follow-up.
    SelfUnalloc,
    /// Cross-cutting "any node in radius can be reached / treated specially" —
    /// for Intuitive-Leap-like behaviour. Doesn't itself emit mods; flips a
    /// pathfinder bit. Reserved.
    Pathfinder,
    /// Issue #196: Watcher's Eye. Despite being labelled a radius jewel by
    /// base, in PoB it's actually an aura-conditional global buff — every mod
    /// on the jewel is gated on `AffectedByHatred` / `AffectedByDetermination`
    /// / etc., set when the player has the corresponding aura active. The
    /// radius scan is bypassed; mods land in the player's modDB once with
    /// their parsed condition tag.
    WatchersEye,
    /// Issue #196: Healthy Mind. `Increases and Reductions to Life in Radius
    /// are Transformed to apply to Mana at 200% of their value`. Reads each
    /// in-radius allocated node's `Inc Life` / `Reduce Life` mods and emits
    /// equivalent `Inc Mana` / `Reduce Mana` mods at 2× value. The jewel's own
    /// flat mods (e.g. `+15% increased maximum Mana`) still apply globally
    /// like vanilla.
    LifeToManaTransform,
    /// Issue #196: Fertile Mind. `Dexterity from Passives in Radius is
    /// Transformed to Intelligence`. Reads each in-radius allocated node's
    /// `+N Dex` BASE mods and emits an equivalent `+N Int` BASE mod sourced as
    /// the in-radius node, suppressing the original Dex contribution by
    /// emitting a counter `-N Dex` BASE.
    DexToIntTransform,
    /// Issue #196: Pure Talent / Replica Pure Talent. The jewel grants per-class
    /// bonuses gated on whether the player's tree connects to that class's
    /// starting location. Each mod_line on the jewel is prefixed with a class
    /// name (`Marauder: …`, `Witch: …`); the handler emits only those whose
    /// prefix matches a connected class — the player's own class always counts,
    /// and any other class's `ClassStart` node that's allocated is treated as
    /// connected. The radius scan is bypassed; mods land in the player's modDB
    /// once with no per-radius copying.
    PureTalent,
    /// Issue #196: Karui Heart. `Strength from Passives in Radius is
    /// Transformed to Life`. Reads each in-radius allocated node's `+N
    /// Strength` BASE mods and emits an equivalent `+5N Life` BASE sourced as
    /// the in-radius node, suppressing the original Str contribution by
    /// emitting a counter `-N Str` BASE. The 5× factor mirrors PoB's
    /// `data/uniques/jewel.lua` Karui Heart implementation, where each
    /// transformed Str becomes +5 Life directly (replacing the +0.5 Life it
    /// would have given through the standard 1 Str = 0.5 Life ratio).
    StrToLifeTransform,
    /// Issue #196 (slice 2): Brawn. `Strength from Passives in Radius is
    /// Doubled`. Reads each in-radius allocated node's `+N Strength` BASE
    /// mods and emits an extra `+N Strength` BASE sourced as the in-radius
    /// node. No counter mod — doubling preserves the source contribution
    /// rather than transforming it away (cf. [`Self::StrToLifeTransform`]).
    StrDoubleTransform,
    /// Issue #196 (slice 3): Energy from Within. `Increases and Reductions
    /// to Life in Radius are Transformed to apply to Energy Shield`. Same
    /// shape as [`Self::LifeToManaTransform`] but routes Inc Life mods to
    /// EnergyShield at 100% (PoB doesn't double for ES). The plain
    /// `(15-20)% increased Energy Shield` the jewel rolls still applies
    /// globally as a vanilla bonus.
    LifeToEnergyShieldTransform,
    /// Issue #196 (slice 4): Inertia. `Dexterity from Passives in Radius
    /// is Transformed to Strength`. Mirror of [`Self::DexToIntTransform`]
    /// (Fertile Mind) with Strength as the destination — same plumbing,
    /// different `to` parameter to `transform_radius_attribute`.
    DexToStrTransform,
    /// Issue #196 (slice 13): Fluid Motion. `Strength from Passives in
    /// Radius is Transformed to Dexterity`. Mirror of
    /// [`Self::DexToStrTransform`] (Inertia) at 1× scale; the jewel's
    /// own `+(16-24) to Dexterity` roll still applies globally.
    StrToDexTransform,
    /// Issue #196 (slice 5): Brute Force Solution. `Strength from Passives
    /// in Radius is Transformed to Intelligence`. Same plumbing as the
    /// other attribute-transform handlers; differs only in `from` /
    /// `to` parameters to `transform_radius_attribute`.
    StrToIntTransform,
    /// Issue #196 (slice 5): Careful Planning. `Intelligence from Passives
    /// in Radius is Transformed to Dexterity`.
    IntToDexTransform,
    /// Issue #196 (slice 5): Efficient Training. `Intelligence from
    /// Passives in Radius is Transformed to Strength`.
    IntToStrTransform,
    /// Issue #196 (slice 6): Energised Armour. `Increases and Reductions
    /// to Energy Shield in Radius are Transformed to apply to Armour
    /// at 200% of their value`. Mirror of [`Self::LifeToManaTransform`]
    /// (Healthy Mind) — Inc transform with the 2× scale on the
    /// destination side, just routed ES → Armour instead of Life → Mana.
    EnergyShieldToArmourTransform,
    /// Issue #196 (slice 7): Cold Steel. Phys ↔ Cold Inc
    /// double-transform — both directions applied additively (the
    /// originals stay), so each in-radius node ends up
    /// contributing to both damage types.
    PhysColdSwapTransform,
    /// Issue #196 (slice 15): Fireborn. `Increases and Reductions to other
    /// Damage Types in Radius are Transformed to apply to Fire Damage`. Same
    /// shape as [`Self::PhysColdSwapTransform`] but with four source
    /// directions (Phys / Cold / Lightning / Chaos) all rolling additively
    /// into Inc FireDamage at 1× scale.
    OtherDamageToFireTransform,
    /// Issue #196 (slice 8): Anatomical Knowledge. `Adds 1 to Maximum
    /// Life per 3 Intelligence Allocated in Radius`. Sums in-radius
    /// `+N Int` BASE mods, integer-divides by 3, emits a single
    /// `+(N/3) Life` BASE sourced as the jewel — the first per-
    /// attribute-tally radius pattern (distinct from the existing
    /// per-node transforms).
    IntCountToLife,
    /// Issue #196 (slice 9): Pugilist (Inc Evasion line). `1%
    /// increased Evasion Rating per 3 Dexterity Allocated in Radius`.
    /// Sister to [`Self::IntCountToLife`] but Dex-sourced and emits
    /// Inc Evasion. The two per-claw / per-unarmed mods on the same
    /// jewel need weapon-condition tags and follow as a future slice.
    DexCountToIncEvasion,
    /// Issue #196: Eldritch Knowledge. `5% increased Chaos Damage per 10
    /// Intelligence from Allocated Passives in Radius`. Sister handler to
    /// Spire of Stone but Int-sourced and emits an Inc `ChaosDamage` mod
    /// (floor(int_sum / 10) × 5).
    IntCountToIncChaosDamage,
    /// Issue #196: Static Electricity. `Adds 1 maximum Lightning Damage to
    /// Attacks per 1 Dexterity Allocated in Radius`. Sums in-radius
    /// allocated `+N Dexterity` BASE mods and emits a single
    /// `LightningDamage` BASE mod with `ModValue::Range { 0, dex }` and the
    /// ATTACK / LIGHTNING flags — a max-only adds, since the per-Dex
    /// scaling only contributes to the upper bound.
    DexCountToMaxLightningAttack,
    /// Issue #196: Spire of Stone. `3% increased Totem Life per 10 Strength
    /// Allocated in Radius`. Sister handler to Pugilist / Anatomical Knowledge:
    /// sums in-radius allocated `+N Strength` BASE mods, integer-divides by
    /// 10, multiplies by 3, and emits a single Inc `TotemLife` mod sourced as
    /// the jewel. The jewel's static `Totems cannot be Stunned` line is left
    /// to vanilla mod_parser handling.
    StrCountToIncTotemLife,
    /// Issue #196: Might in All Forms. `Dexterity and Intelligence from
    /// passives in Radius count towards Strength Melee Damage bonus`. Sums
    /// in-radius allocated `+N Dexterity` and `+N Intelligence` BASE mods
    /// and emits a single `DexIntToMeleeBonus` BASE mod whose value is the
    /// straight `Dex + Int` total — no per-N integer-divide. Mirrors PoB's
    /// `jewelSelfFuncs` line (`ModParser.lua` ~6160) which folds the same
    /// stat back into the Strength → Melee Damage bonus computation
    /// (`CalcPerform.lua` ~501).
    DexIntToStrMeleeBonus,
    /// Issue #196: Tempered Spirit / Transcendent Spirit. `(2|3)% increased
    /// Movement Speed per 10 Dexterity on Allocated Passives in Radius`.
    /// Sister handler to Spire of Stone / Eldritch Knowledge but Dex-sourced
    /// and emits Inc `MovementSpeed`. Sums in-radius allocated `+N Dexterity`
    /// BASE mods, integer-divides by 10, and multiplies by the per-line
    /// percentage parsed from the marker (2 for Tempered Spirit / Transcendent
    /// Spirit pre-3.10, 3 for current Transcendent Spirit). Mirrors PoB's
    /// `jewelSelfFuncs` lines (`ModParser.lua` ~6178-6179) where each rate
    /// is its own `getPerStat("MovementSpeed", "INC", 0, "Dex", N / 10)`
    /// entry — we collapse them under one handler so a future Tempered/
    /// Transcendent Spirit slice for the `-1 Dex per 1 Dex Allocated` and
    /// the unallocated-side mods can stack additively without re-routing.
    DexCountToIncMovementSpeed,
    /// Issue #196: Tempered Flesh / Transcendent Flesh.
    /// `(2|3)% increased Life Recovery Rate per 10 Strength on Allocated
    /// Passives in Radius`. Sister handler to Spire of Stone but emits Inc
    /// `LifeRecovery` (the engine's name for the per-second life-recovery-
    /// rate stat — see `mod_parser.rs` `"Life Recovery rate" =>
    /// "LifeRecovery"`). Sums in-radius allocated `+N Strength` BASE mods,
    /// integer-divides by 10, and multiplies by the per-line percentage
    /// parsed from the marker (2 for Tempered Flesh / Transcendent Flesh
    /// pre-3.10, 3 for current Transcendent Flesh). Mirrors PoB's
    /// `jewelSelfFuncs` lines (`ModParser.lua` ~6170-6171) where each rate
    /// is its own `getPerStat("LifeRecoveryRate", "INC", 0, "Str", N / 10)`
    /// entry — we collapse them under one handler so the jewel's other
    /// lines (`-1 Str per 1 Str Allocated`, the unallocated-side mods,
    /// `1% additional Physical Damage Reduction per 10 Str Allocated`) can
    /// stack additively with future slices without re-routing.
    StrCountToIncLifeRecovery,
    /// Issue #196: Tempered Mind / Transcendent Mind.
    /// `(2|3)% increased Mana Recovery Rate per 10 Intelligence on Allocated
    /// Passives in Radius`. Sister handler to Tempered/Transcendent Spirit
    /// and Tempered/Transcendent Flesh but Int-sourced and emits Inc
    /// `ManaRecovery` (the engine's name for the per-second mana-recovery-
    /// rate stat — see `mod_parser.rs` `"Mana Recovery rate" =>
    /// "ManaRecovery"`). Sums in-radius allocated `+N Intelligence` BASE
    /// mods, integer-divides by 10, and multiplies by the per-line
    /// percentage parsed from the marker (2 for Tempered Mind, 3 for
    /// current Transcendent Mind). Mirrors PoB's `jewelSelfFuncs` lines
    /// (`ModParser.lua` ~6175-6176) where each rate is its own
    /// `getPerStat("ManaRecoveryRate", "INC", 0, "Int", N / 10)` entry —
    /// we collapse them under one handler so the jewel's other lines
    /// (`-1 Int per 1 Int Allocated`, the unallocated-side mods, the
    /// Energy Shield regen line on pre-3.10 Transcendent Mind, the
    /// Accuracy Rating / DoT Multi unallocated-side mods) can stack
    /// additively with future slices without re-routing.
    IntCountToIncManaRecovery,
    /// Issue #196: The Light of Meaning (Life variant). `Passive Skills in
    /// Radius also grant +N to maximum Life`. PoB pattern: per-node grant
    /// emitted to every passive skill in the radius that isn't a Keystone,
    /// Jewel Socket, or Class Start (`ModParser.lua` ~6054-6060). The
    /// player benefits from the per-node contribution only on *allocated*
    /// nodes (PoB writes to each node's `out` modList; only allocated
    /// nodes contribute to player stats), so the dispatch counts
    /// allocated in-radius eligible nodes and emits a single
    /// `+(count × N) Life` BASE mod sourced as the jewel. The `+N` is
    /// parsed off the marker line so this handler covers any roll.
    /// Other variants of The Light of Meaning (Mana, Armour, …) follow
    /// the same per-node grant shape under sibling handlers in
    /// follow-up slices.
    PassiveAlsoGrantBaseLife,
}

/// One radius jewel ready to be applied. Owns the parsed mod list and the radius
/// band the jewel claims. Construct via [`identify_radius_jewel`].
#[derive(Debug, Clone)]
pub struct RadiusJewel {
    /// Tree-socket node id the jewel is socketed into.
    pub socket_id: NodeId,
    /// Chosen radius band — typically Small/Medium/Large depending on the jewel's
    /// `Only affects Passives in <Size> Ring` text or PoB's per-base default.
    pub radius: JewelRadiusInfo,
    /// PoB's radius-index for this band (0 = Small, 1 = Medium, …). Useful for the
    /// Calcs-tab breakdown and for follow-ups that need to round-trip with PoB.
    pub radius_index: usize,
    /// Mods parsed off the jewel item that should be replayed per in-radius node.
    /// Each mod is sourceless on this struct; callers re-tag with
    /// `Source::Passive(node_id)` when applying.
    pub mods: Vec<Mod>,
    /// Display label for breakdown attribution (`"RadiusJewel:Crimson Jewel"`).
    pub source_label: String,
    /// Handler kind. Currently always [`HandlerKind::SelfAllocated`] for
    /// framework-level dispatch; named uniques will swap this out.
    pub kind: HandlerKind,
}

/// Identify whether `item` is a node-modifying radius jewel and, if so, build the
/// [`RadiusJewel`] descriptor that drives application.
///
/// Heuristic for this slice:
///
/// * The item's base name contains `Jewel` (matches Crimson/Viridian/Cobalt/Prismatic
///   Jewels and similarly-named uniques like Searching Eye, Healthy Mind, …).
/// * At least one of the jewel's mod lines mentions `Passives in Radius`,
///   `nearby allocated passives`, or `nodes in Radius` — that's PoB's canonical
///   marker for a radius effect. The chosen radius defaults to Medium (the most
///   common vanilla bucket); explicit `Only affects Passives in <Size> Ring` text,
///   when present, overrides.
/// * Cluster jewels (`subType = Cluster`), Abyss jewels, and timeless jewels
///   (`subType = Timeless`) are intentionally **not** picked up here — they have
///   dedicated dispatch paths (#21, #30) that consume the same radius primitives.
///
/// Returns `None` for items the framework should ignore.
pub fn identify_radius_jewel(socket_id: NodeId, item: &Item) -> Option<RadiusJewel> {
    if !is_jewel_base(&item.base_name) {
        return None;
    }
    // Issue #196: named-unique handlers. Detected by item *name* (the unique
    // name, not the base) so we don't conflate the unique with vanilla rolls
    // on the same base. Each named-unique routes through a dedicated
    // [`HandlerKind`] in [`apply_radius_jewels`]; their parsed mods, radius,
    // and source label come from per-handler constructors below.
    if let Some(j) = identify_named_unique(socket_id, item) {
        return Some(j);
    }
    if is_special_jewel_subtype(item) {
        return None;
    }
    // Walk mod lines to find a radius marker. We look at all mod sections so a
    // crafted-only or implicit-only radius mod still triggers identification.
    let mut radius_label: Option<&'static str> = None;
    let mut has_radius_text = false;
    for ml in &item.mod_lines {
        let line = ml.line.as_str();
        if mentions_radius(line) {
            has_radius_text = true;
        }
        if let Some(label) = explicit_ring_label(line) {
            radius_label = Some(label);
        }
    }
    if !has_radius_text && radius_label.is_none() {
        return None;
    }
    // Default to Medium when the jewel's text doesn't pin a ring size — that's the
    // canonical vanilla node-modifying-jewel bucket (Viridian / Crimson / Cobalt
    // base default). PoB encodes per-base defaults inside the bases data; once we
    // surface that we'll prefer the per-base value here.
    let label = radius_label.unwrap_or("Medium");
    let idx = radius_index_for_label(label)?;
    let radii = radii_for_tree_version(&item_tree_version(item));
    let radius = *radii.get(idx)?;

    // Parse every line into mods. Strip the trailing "in Radius" / "to allocated
    // passives" / etc. metadata from each line first — those phrases tell the
    // framework *where* to apply the mod, not *what* the mod is. Without
    // stripping, `mod_parser` mints PoB-style suffixed keys
    // (`MaximumLifeToNearbyAllocatedPassives`) that no calc consumer reads.
    let mut mods = Vec::with_capacity(item.mod_lines.len());
    for ml in &item.mod_lines {
        let raw = ml.line.as_str();
        // Skip the metadata-only `Only affects Passives in <Size> Ring` line — it's
        // not a real bonus, just a radius selector. PoB models it as a `JewelData`
        // LIST mod that we don't use yet.
        if explicit_ring_label(raw).is_some() {
            continue;
        }
        // Skip `<n> Added Passive Skills are Jewels` (cluster jewels handled separately).
        if raw.contains("Added Passive Skills are Jewels") {
            continue;
        }
        let stripped = strip_radius_suffix(raw);
        // If stripping leaves nothing parseable (the line was *only* a metadata
        // marker), fall back to parsing the original — `mod_parser` is the source
        // of truth for "is this a real mod".
        let target = stripped.as_deref().unwrap_or(raw);
        if let Some(parsed) = parse_mod_line(target) {
            mods.push(parsed.mod_);
        } else if stripped.is_some() {
            // Stripping changed the text but we couldn't parse the result; try the
            // original line in case the parser handles the long form directly.
            if let Some(parsed) = parse_mod_line(raw) {
                mods.push(parsed.mod_);
            }
        }
    }
    if mods.is_empty() {
        return None;
    }

    let source_label = format!("RadiusJewel:{}", item.base_name);
    Some(RadiusJewel {
        socket_id,
        radius,
        radius_index: idx,
        mods,
        source_label,
        kind: HandlerKind::SelfAllocated,
    })
}

/// Issue #196: dispatch on the unique's display name to pick a bespoke
/// [`HandlerKind`]. PoB tracks these in `data.jewelData.funcList` per unique;
/// we model them inline as a small lookup. Returns `None` for non-named
/// jewels — the caller then falls through to the vanilla
/// `SelfAllocated`-on-radius-marker path.
fn identify_named_unique(socket_id: NodeId, item: &Item) -> Option<RadiusJewel> {
    // The PoB-canonical "is this a named unique" check is `item.title ~= ""`
    // plus a name match. Items synthesised in tests / from PoB import keep the
    // unique name in `item.name`; the base lives in `base_name`. We compare
    // against `name` so a rare `Cobalt Jewel` named "Healthy Mind" by accident
    // doesn't trip the dispatch.
    let n = item.name.as_str();
    match n {
        "Watcher's Eye" => Some(build_watchers_eye(socket_id, item)),
        "Healthy Mind" => Some(build_life_to_mana(socket_id, item)),
        "Fertile Mind" => Some(build_dex_to_int(socket_id, item)),
        "Karui Heart" => Some(build_str_to_life(socket_id, item)),
        "Brawn" => Some(build_str_double(socket_id, item)),
        "Energy From Within" | "Energy from Within" => {
            Some(build_life_to_energy_shield(socket_id, item))
        }
        "Inertia" => Some(build_dex_to_str(socket_id, item)),
        "Fluid Motion" => Some(build_str_to_dex(socket_id, item)),
        "Brute Force Solution" => Some(build_str_to_int(socket_id, item)),
        "Careful Planning" => Some(build_int_to_dex(socket_id, item)),
        "Efficient Training" => Some(build_int_to_str(socket_id, item)),
        "Energised Armour" => Some(build_energy_shield_to_armour(socket_id, item)),
        "Cold Steel" => Some(build_phys_cold_swap(socket_id, item)),
        "Fireborn" => Some(build_other_damage_to_fire(socket_id, item)),
        "Anatomical Knowledge" => Some(build_int_count_to_life(socket_id, item)),
        "Might in All Forms" => Some(build_dex_int_to_str_melee_bonus(socket_id, item)),
        "Pugilist" => Some(build_dex_count_to_inc_evasion(socket_id, item)),
        "Spire of Stone" => Some(build_str_count_to_inc_totem_life(socket_id, item)),
        "Eldritch Knowledge" => Some(build_int_count_to_inc_chaos_damage(socket_id, item)),
        "Static Electricity" => Some(build_dex_count_to_max_lightning_attack(socket_id, item)),
        "Pure Talent" | "Replica Pure Talent" => Some(build_pure_talent(socket_id, item)),
        "Tempered Spirit" | "Transcendent Spirit" => {
            Some(build_dex_count_to_inc_movement_speed(socket_id, item))
        }
        "Tempered Flesh" | "Transcendent Flesh" => {
            Some(build_str_count_to_inc_life_recovery(socket_id, item))
        }
        "Tempered Mind" | "Transcendent Mind" => {
            Some(build_int_count_to_inc_mana_recovery(socket_id, item))
        }
        "Intuitive Leap" => Some(build_intuitive_leap(socket_id, item)),
        "The Light of Meaning" => build_light_of_meaning(socket_id, item),
        _ => None,
    }
}

/// Watcher's Eye: build a `RadiusJewel` whose mods are the parsed jewel-text
/// lines, sized to whatever radius the base claims (irrelevant — the
/// `WatchersEye` handler ignores radius). Each parsed mod retains the
/// `AffectedBy<Aura>` Condition tag emitted by `mod_parser`'s
/// `match_while_var_dyn`. Lines that don't carry a "while affected by" clause
/// (like the base `+X% maximum Energy Shield/Life/Mana`) parse as plain
/// global mods and apply unconditionally — matching PoB's behaviour where
/// those base implicits are unguarded.
fn build_watchers_eye(socket_id: NodeId, item: &Item) -> RadiusJewel {
    let mut mods = Vec::with_capacity(item.mod_lines.len());
    for ml in &item.mod_lines {
        if let Some(parsed) = parse_mod_line(ml.line.as_str()) {
            mods.push(parsed.mod_);
        }
    }
    RadiusJewel {
        socket_id,
        radius: pob_data::JewelRadiusInfo::new(0.0, 0.0, "Watcher's Eye"),
        radius_index: 0,
        mods,
        source_label: format!("RadiusJewel:{}", item.base_name),
        kind: HandlerKind::WatchersEye,
    }
}

/// Healthy Mind: parse the jewel's *non-transform* mods (e.g.
/// `(15-20)% increased maximum Mana`) like a vanilla jewel, but drop the
/// `Increases and Reductions to Life in Radius are Transformed to apply to
/// Mana at 200% of their value` line — that's a metadata marker the
/// [`HandlerKind::LifeToManaTransform`] handler reads directly. The radius
/// defaults to Large (`Radius: Large` per upstream `Data/Uniques/jewel.lua`).
fn build_life_to_mana(socket_id: NodeId, item: &Item) -> RadiusJewel {
    build_transformer(
        socket_id,
        item,
        HandlerKind::LifeToManaTransform,
        is_life_mana_transform_marker,
    )
}

/// Fertile Mind: parse the `+(16-24) to Intelligence` flat mod normally and
/// drop the transform marker line. Default radius Large.
fn build_dex_to_int(socket_id: NodeId, item: &Item) -> RadiusJewel {
    build_transformer(
        socket_id,
        item,
        HandlerKind::DexToIntTransform,
        is_dex_int_transform_marker,
    )
}

/// Karui Heart: parse the jewel's plain bonus mods (e.g. `+(20-30) to
/// Strength`) like a vanilla jewel, but drop the transform marker line —
/// that metadata feeds the [`HandlerKind::StrToLifeTransform`] dispatch
/// directly. Default radius Large (matches upstream `Data/Uniques/jewel.lua`'s
/// `Radius: Large` for Karui Heart).
fn build_str_to_life(socket_id: NodeId, item: &Item) -> RadiusJewel {
    build_transformer(
        socket_id,
        item,
        HandlerKind::StrToLifeTransform,
        is_str_life_transform_marker,
    )
}

/// Issue #196 (slice 2): Brawn. Same shape as Karui Heart's builder —
/// the marker line (`Strength from Passives in Radius is Doubled`) is
/// dispatch metadata only; everything else on the jewel still applies
/// globally as a vanilla bonus mod. Default radius is Large to mirror
/// upstream `Data/Uniques/jewel.lua`'s `Radius: Large` for Brawn.
fn build_str_double(socket_id: NodeId, item: &Item) -> RadiusJewel {
    build_transformer(
        socket_id,
        item,
        HandlerKind::StrDoubleTransform,
        is_str_double_marker,
    )
}

/// Issue #196 (slice 3): Energy from Within. Same shape as Healthy
/// Mind's builder — the marker line (`Increases and Reductions to Life
/// in Radius are Transformed to apply to Energy Shield`) is dispatch
/// metadata; the jewel's plain `(15-20)% increased Energy Shield`
/// roll still applies globally. Default radius Large.
fn build_life_to_energy_shield(socket_id: NodeId, item: &Item) -> RadiusJewel {
    build_transformer(
        socket_id,
        item,
        HandlerKind::LifeToEnergyShieldTransform,
        is_life_es_transform_marker,
    )
}

/// Issue #196 (slice 13): Fluid Motion. Mirror of Inertia at 1× scale.
/// Marker line is `Strength from Passives in Radius is Transformed to
/// Dexterity`; the jewel's plain `+(16-24) to Dexterity` roll still
/// applies globally.
fn build_str_to_dex(socket_id: NodeId, item: &Item) -> RadiusJewel {
    build_transformer(
        socket_id,
        item,
        HandlerKind::StrToDexTransform,
        is_str_dex_transform_marker,
    )
}

/// Issue #196 (slice 4): Inertia. Same shape as Fertile Mind's
/// builder — the marker line (`Dexterity from Passives in Radius is
/// Transformed to Strength`) is dispatch metadata only; the jewel's
/// plain `+(16-24) to Strength` roll still applies globally. Default
/// radius Large per upstream `Data/Uniques/jewel.lua`.
fn build_dex_to_str(socket_id: NodeId, item: &Item) -> RadiusJewel {
    build_transformer(
        socket_id,
        item,
        HandlerKind::DexToStrTransform,
        is_dex_str_transform_marker,
    )
}

/// Issue #196 (slice 5): Brute Force Solution. Str → Int.
fn build_str_to_int(socket_id: NodeId, item: &Item) -> RadiusJewel {
    build_transformer(
        socket_id,
        item,
        HandlerKind::StrToIntTransform,
        is_str_int_transform_marker,
    )
}

/// Issue #196 (slice 5): Careful Planning. Int → Dex.
fn build_int_to_dex(socket_id: NodeId, item: &Item) -> RadiusJewel {
    build_transformer(
        socket_id,
        item,
        HandlerKind::IntToDexTransform,
        is_int_dex_transform_marker,
    )
}

/// Issue #196 (slice 5): Efficient Training. Int → Str.
fn build_int_to_str(socket_id: NodeId, item: &Item) -> RadiusJewel {
    build_transformer(
        socket_id,
        item,
        HandlerKind::IntToStrTransform,
        is_int_str_transform_marker,
    )
}

/// Issue #196 (slice 6): Energised Armour. Same shape as Healthy
/// Mind / Energy from Within — the marker line is dispatch metadata
/// only; the jewel's plain `(15-20)% increased Armour` roll still
/// applies globally. Default radius Large per upstream
/// `Data/Uniques/jewel.lua`.
fn build_energy_shield_to_armour(socket_id: NodeId, item: &Item) -> RadiusJewel {
    build_transformer(
        socket_id,
        item,
        HandlerKind::EnergyShieldToArmourTransform,
        is_es_armour_transform_marker,
    )
}

/// Issue #196 (slice 7): Cold Steel. Two marker lines (one per
/// transform direction) are dispatch metadata; the predicate matches
/// either so the jewel's non-transform mods (none currently rolled,
/// but defended-in-depth) still flow through the global emission.
/// Issue #196 (slice 15): Fireborn. The marker line is dispatch-only metadata
/// — the radius scan adds an Inc FireDamage emission per (in-radius node,
/// non-Fire damage type) pair.
fn build_other_damage_to_fire(socket_id: NodeId, item: &Item) -> RadiusJewel {
    build_transformer(
        socket_id,
        item,
        HandlerKind::OtherDamageToFireTransform,
        is_other_damage_to_fire_marker,
    )
}

fn build_phys_cold_swap(socket_id: NodeId, item: &Item) -> RadiusJewel {
    build_transformer(
        socket_id,
        item,
        HandlerKind::PhysColdSwapTransform,
        is_phys_cold_swap_marker,
    )
}

/// Issue #196 (slice 8): Anatomical Knowledge. The marker line
/// (`Adds 1 to Maximum Life per 3 Intelligence Allocated in
/// Radius`) is dispatch metadata; the jewel's plain
/// `(6-8)% increased maximum Life` roll still applies globally.
fn build_int_count_to_life(socket_id: NodeId, item: &Item) -> RadiusJewel {
    build_transformer(
        socket_id,
        item,
        HandlerKind::IntCountToLife,
        is_int_count_to_life_marker,
    )
}

/// Issue #196: Might in All Forms. `Dexterity and Intelligence from
/// passives in Radius count towards Strength Melee Damage bonus`. The
/// marker line is dispatch-only metadata; the in-radius Dex + Int sum is
/// computed in the dispatch arm. Radius is Medium (the jewel's only band)
/// — `build_transformer`'s Large default doesn't apply here, so we build
/// the descriptor inline with the right `radius_index_for_label("Medium")`.
fn build_dex_int_to_str_melee_bonus(socket_id: NodeId, item: &Item) -> RadiusJewel {
    let mods = parse_non_transform_mods(item, is_dex_int_to_str_melee_bonus_marker);
    let idx = radius_index_for_label("Medium").unwrap_or(1);
    let radii = radii_for_tree_version(&item_tree_version(item));
    let radius = radii
        .get(idx)
        .copied()
        .unwrap_or_else(|| pob_data::JewelRadiusInfo::new(0.0, 1440.0, "Medium"));
    RadiusJewel {
        socket_id,
        radius,
        radius_index: idx,
        mods,
        source_label: format!("RadiusJewel:{}", item.base_name),
        kind: HandlerKind::DexIntToStrMeleeBonus,
    }
}

/// Issue #196 (slice 9): Pugilist (Inc Evasion line). The marker
/// (`1% increased Evasion Rating per 3 Dexterity Allocated in
/// Radius`) is dispatch metadata. The two per-claw / per-unarmed
/// lines on the same jewel are not handled here; the marker
/// predicate matches only the Evasion line so they fall through
/// to vanilla globals (which over-applies them — TODO follow-up
/// slice).
fn build_dex_count_to_inc_evasion(socket_id: NodeId, item: &Item) -> RadiusJewel {
    build_transformer(
        socket_id,
        item,
        HandlerKind::DexCountToIncEvasion,
        is_dex_count_to_inc_evasion_marker,
    )
}

/// Issue #196 (slice 12): Static Electricity (`Adds 1 maximum Lightning
/// Damage to Attacks per 1 Dexterity Allocated in Radius`). Marker matches
/// only the per-Dex line; the jewel's static `Adds 1 to 2 Lightning Damage
/// to Attacks` line falls through to vanilla mod_parser.
fn build_dex_count_to_max_lightning_attack(socket_id: NodeId, item: &Item) -> RadiusJewel {
    build_transformer(
        socket_id,
        item,
        HandlerKind::DexCountToMaxLightningAttack,
        is_dex_count_to_max_lightning_attack_marker,
    )
}

/// Issue #196: Tempered Spirit / Transcendent Spirit. Marker recognises
/// the `(N)% increased Movement Speed per 10 Dexterity on Allocated
/// Passives in Radius` line; the dispatch arm reads the leading
/// percentage off the matching mod_line so 2% (Tempered / Transcendent
/// pre-3.10) and 3% (Transcendent current) share one handler. Other
/// jewel-text lines (`-1 Dexterity per 1 Dexterity Allocated`, the
/// unallocated-side mods) fall through to vanilla mod_parser pending
/// dedicated slices for the negative-stat / SelfUnalloc patterns.
/// Default radius Medium (matches upstream `Data/Uniques/jewel.lua`'s
/// `Radius: Medium` for both jewels — line 749 / 760).
fn build_dex_count_to_inc_movement_speed(socket_id: NodeId, item: &Item) -> RadiusJewel {
    let mods = parse_non_transform_mods(item, is_dex_count_to_inc_movement_speed_marker);
    let idx = radius_index_for_label("Medium").unwrap_or(1);
    let radii = radii_for_tree_version(&item_tree_version(item));
    let radius = radii
        .get(idx)
        .copied()
        .unwrap_or_else(|| pob_data::JewelRadiusInfo::new(0.0, 1440.0, "Medium"));
    RadiusJewel {
        socket_id,
        radius,
        radius_index: idx,
        mods,
        source_label: format!("RadiusJewel:{}", item.base_name),
        kind: HandlerKind::DexCountToIncMovementSpeed,
    }
}

/// Issue #196 (slice 11): Eldritch Knowledge (`5% increased Chaos Damage
/// per 10 Intelligence from Allocated Passives in Radius`). The line is
/// dispatch-only metadata — the in-radius Int sum × 5 / 10 is computed in
/// the dispatch arm.
fn build_int_count_to_inc_chaos_damage(socket_id: NodeId, item: &Item) -> RadiusJewel {
    build_transformer(
        socket_id,
        item,
        HandlerKind::IntCountToIncChaosDamage,
        is_int_count_to_inc_chaos_damage_marker,
    )
}

/// Issue #196 (slice 10): Spire of Stone (`3% increased Totem Life per 10
/// Strength Allocated in Radius`). The line itself is dispatch-only metadata
/// — the in-radius Strength sum × 3 / 10 is computed in the dispatch arm.
/// The marker predicate matches only the Totem Life line; the jewel's
/// `Totems cannot be Stunned` flag falls through to vanilla mod_parser.
fn build_str_count_to_inc_totem_life(socket_id: NodeId, item: &Item) -> RadiusJewel {
    build_transformer(
        socket_id,
        item,
        HandlerKind::StrCountToIncTotemLife,
        is_str_count_to_inc_totem_life_marker,
    )
}

/// Issue #196: Tempered Flesh / Transcendent Flesh. Marker recognises
/// the `(N)% increased Life Recovery Rate per 10 Strength on Allocated
/// Passives in Radius` line; the dispatch arm reads the leading
/// percentage off the matching mod_line so 2% (Tempered Flesh /
/// Transcendent Flesh pre-3.10) and 3% (Transcendent Flesh current)
/// share one handler. Other jewel-text lines (`-1 Strength per 1
/// Strength Allocated`, the unallocated-side mods, the `1% additional
/// Physical Damage Reduction per 10 Strength Allocated` line) fall
/// through to vanilla mod_parser pending dedicated slices for the
/// negative-stat / SelfUnalloc / Phys-mitigation patterns. Default
/// radius Medium (matches upstream `Data/Uniques/jewel.lua`'s
/// `Radius: Medium` for both jewels — line 690 / 703).
fn build_str_count_to_inc_life_recovery(socket_id: NodeId, item: &Item) -> RadiusJewel {
    let mods = parse_non_transform_mods(item, is_str_count_to_inc_life_recovery_marker);
    let idx = radius_index_for_label("Medium").unwrap_or(1);
    let radii = radii_for_tree_version(&item_tree_version(item));
    let radius = radii
        .get(idx)
        .copied()
        .unwrap_or_else(|| pob_data::JewelRadiusInfo::new(0.0, 1440.0, "Medium"));
    RadiusJewel {
        socket_id,
        radius,
        radius_index: idx,
        mods,
        source_label: format!("RadiusJewel:{}", item.base_name),
        kind: HandlerKind::StrCountToIncLifeRecovery,
    }
}

/// Issue #196: Tempered Mind / Transcendent Mind. Marker recognises
/// the `(N)% increased Mana Recovery Rate per 10 Intelligence on
/// Allocated Passives in Radius` line; the dispatch arm reads the
/// leading percentage off the matching mod_line so 2% (Tempered Mind)
/// and 3% (current Transcendent Mind) share one handler. Other
/// jewel-text lines (`-1 Intelligence per 1 Intelligence Allocated`,
/// the unallocated-side mods, the pre-3.10 Energy Shield regen line)
/// fall through to vanilla mod_parser pending dedicated slices for
/// the negative-stat / SelfUnalloc / ES-regen patterns. Default
/// radius Medium (matches upstream `Data/Uniques/jewel.lua`'s
/// `Radius: Medium` for both jewels — line 719 / 732).
fn build_int_count_to_inc_mana_recovery(socket_id: NodeId, item: &Item) -> RadiusJewel {
    let mods = parse_non_transform_mods(item, is_int_count_to_inc_mana_recovery_marker);
    let idx = radius_index_for_label("Medium").unwrap_or(1);
    let radii = radii_for_tree_version(&item_tree_version(item));
    let radius = radii
        .get(idx)
        .copied()
        .unwrap_or_else(|| pob_data::JewelRadiusInfo::new(0.0, 1440.0, "Medium"));
    RadiusJewel {
        socket_id,
        radius,
        radius_index: idx,
        mods,
        source_label: format!("RadiusJewel:{}", item.base_name),
        kind: HandlerKind::IntCountToIncManaRecovery,
    }
}

/// Issue #196: The Light of Meaning. Prismatic Jewel with 13 variants — this
/// slice handles the Life variant (`Passive Skills in Radius also grant +N
/// to maximum Life`). Marker drops the per-node-grant line out of the
/// vanilla parse path; the dispatch arm reads the `+N` off the line and
/// emits one `Life BASE` mod scaled by the count of eligible (non-Keystone,
/// non-JewelSocket, non-ClassStart) allocated in-radius nodes. Default
/// radius Large (matches `Data/Uniques/jewel.lua:372` — `Radius: Large`).
/// If the item carries no Life-grant line (e.g. it's a different variant
/// not yet handled), this builder returns `None` so the named-unique
/// dispatch falls through and other variant handlers can claim it in
/// future slices without conflicting.
fn build_light_of_meaning(socket_id: NodeId, item: &Item) -> Option<RadiusJewel> {
    if !item
        .mod_lines
        .iter()
        .any(|ml| is_passive_also_grant_base_life_marker(&ml.line))
    {
        return None;
    }
    let mods = parse_non_transform_mods(item, is_passive_also_grant_base_life_marker);
    let idx = radius_index_for_label("Large").unwrap_or(2);
    let radii = radii_for_tree_version(&item_tree_version(item));
    let radius = radii
        .get(idx)
        .copied()
        .unwrap_or_else(|| pob_data::JewelRadiusInfo::new(0.0, 1500.0, "Large"));
    Some(RadiusJewel {
        socket_id,
        radius,
        radius_index: idx,
        mods,
        source_label: format!("RadiusJewel:{}", item.name),
        kind: HandlerKind::PassiveAlsoGrantBaseLife,
    })
}

/// Pure Talent / Replica Pure Talent: build a [`RadiusJewel`] whose `mods` list
/// is empty — the actual class-conditional bonuses come from the dispatch
/// handler reading the item's raw `mod_lines` and filtering by class
/// connection. The radius is irrelevant (PoB ignores it for this jewel) but we
/// still pin it to a `(0.0, 0.0)` band so the dispatch's radius-scan branch
/// short-circuits with an empty in-radius set if it accidentally falls
/// through.
fn build_pure_talent(socket_id: NodeId, item: &Item) -> RadiusJewel {
    RadiusJewel {
        socket_id,
        radius: pob_data::JewelRadiusInfo::new(0.0, 0.0, "Pure Talent"),
        radius_index: 0,
        mods: Vec::new(),
        source_label: format!("RadiusJewel:{}", item.name),
        kind: HandlerKind::PureTalent,
    }
}

/// Intuitive Leap: a Viridian Jewel whose only effect is the pathfinder
/// bypass — `Passive Skills in Radius can be Allocated without being
/// connected to your tree`. The dispatch in [`apply_radius_jewels`] is a
/// no-op for [`HandlerKind::Pathfinder`]; the actual bypass logic lives in
/// [`intuitive_leap_reachable`] and is consumed by `Character::allocate_path`
/// / `Character::unallocate`. The radius is `Small`.
fn build_intuitive_leap(socket_id: NodeId, item: &Item) -> RadiusJewel {
    let idx = radius_index_for_label("Small").unwrap_or(0);
    let radii = radii_for_tree_version(&item_tree_version(item));
    let radius = radii
        .get(idx)
        .copied()
        .unwrap_or_else(|| pob_data::JewelRadiusInfo::new(0.0, 800.0, "Small"));
    RadiusJewel {
        socket_id,
        radius,
        radius_index: idx,
        mods: Vec::new(),
        source_label: format!("RadiusJewel:{}", item.name),
        kind: HandlerKind::Pathfinder,
    }
}

fn build_transformer(
    socket_id: NodeId,
    item: &Item,
    kind: HandlerKind,
    marker: fn(&str) -> bool,
) -> RadiusJewel {
    let mods = parse_non_transform_mods(item, marker);
    let idx = radius_index_for_label("Large").unwrap_or(2);
    let radii = radii_for_tree_version(&item_tree_version(item));
    let radius = radii
        .get(idx)
        .copied()
        .unwrap_or_else(|| pob_data::JewelRadiusInfo::new(0.0, 1500.0, "Large"));
    RadiusJewel {
        socket_id,
        radius,
        radius_index: idx,
        mods,
        source_label: format!("RadiusJewel:{}", item.base_name),
        kind,
    }
}

fn parse_non_transform_mods(item: &Item, is_marker: fn(&str) -> bool) -> Vec<Mod> {
    let mut mods = Vec::with_capacity(item.mod_lines.len());
    for ml in &item.mod_lines {
        let raw = ml.line.as_str();
        if is_marker(raw) || explicit_ring_label(raw).is_some() {
            continue;
        }
        let stripped = strip_radius_suffix(raw);
        let target = stripped.as_deref().unwrap_or(raw);
        if let Some(parsed) = parse_mod_line(target) {
            mods.push(parsed.mod_);
        } else if stripped.is_some() {
            if let Some(parsed) = parse_mod_line(raw) {
                mods.push(parsed.mod_);
            }
        }
    }
    mods
}

fn is_life_mana_transform_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("increases and reductions to life in radius") && l.contains("to apply to mana")
}

fn is_life_es_transform_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("increases and reductions to life in radius")
        && l.contains("to apply to energy shield")
}

fn is_dex_str_transform_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("dexterity from passives in radius") && l.contains("transformed to strength")
}

fn is_str_dex_transform_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("strength from passives in radius") && l.contains("transformed to dexterity")
}

fn is_str_int_transform_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("strength from passives in radius") && l.contains("transformed to intelligence")
}

fn is_int_dex_transform_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("intelligence from passives in radius") && l.contains("transformed to dexterity")
}

fn is_int_str_transform_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("intelligence from passives in radius") && l.contains("transformed to strength")
}

fn is_es_armour_transform_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("increases and reductions to energy shield in radius")
        && l.contains("to apply to armour")
}

fn is_phys_cold_swap_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    (l.contains("increases and reductions to physical damage in radius")
        && l.contains("to apply to cold damage"))
        || (l.contains("increases and reductions to cold damage in radius")
            && l.contains("to apply to physical damage"))
}

fn is_other_damage_to_fire_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("increases and reductions to other damage types in radius")
        && l.contains("to apply to fire damage")
}

fn is_int_count_to_life_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("adds 1 to maximum life")
        && l.contains("per 3 intelligence")
        && l.contains("in radius")
}

/// Issue #196 (slice 9 + slice 14 + unarmed): Pugilist marker
/// covering all three per-Dex lines — Evasion, per-claw Inc Physical
/// Damage, and per-unarmed-melee Inc Physical Damage. All three lines
/// need to be excluded from `parse_non_transform_mods` so the radius
/// dispatch owns the scaled emission, and the dispatch arm
/// ([`HandlerKind::DexCountToIncEvasion`]) emits each present line
/// independently from a single shared in-radius Dex sum.
fn is_dex_count_to_inc_evasion_marker(line: &str) -> bool {
    is_pugilist_evasion_line_marker(line)
        || is_pugilist_claw_phys_marker(line)
        || is_pugilist_unarmed_phys_marker(line)
}

fn is_pugilist_evasion_line_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("1% increased evasion rating")
        && l.contains("per 3 dexterity")
        && l.contains("in radius")
}

fn is_pugilist_claw_phys_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("1% increased claw physical damage")
        && l.contains("per 3 dexterity")
        && l.contains("in radius")
}

fn is_pugilist_unarmed_phys_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("1% increased melee physical damage with unarmed attacks")
        && l.contains("per 3 dexterity")
        && l.contains("in radius")
}

fn is_str_count_to_inc_totem_life_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("3% increased totem life")
        && l.contains("per 10 strength")
        && l.contains("in radius")
}

/// Issue #196: Tempered/Transcendent Flesh marker — the leading rate
/// (`2%` / `3%`) varies between variants but the rest of the line is
/// fixed. Returns `true` for any rate so a single dispatch arm can read
/// the percentage off the matching mod_line.
fn is_str_count_to_inc_life_recovery_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("increased life recovery rate")
        && l.contains("per 10 strength")
        && l.contains("on allocated passives in radius")
}

/// Issue #196: extract the leading percentage (`N` from `N% increased
/// Life Recovery Rate per 10 Strength on Allocated Passives in Radius`).
/// Used by the dispatch arm for [`HandlerKind::StrCountToIncLifeRecovery`]
/// to scale Tempered Flesh's 2% vs Transcendent Flesh (current)'s 3%
/// off the same handler.
fn parse_str_count_to_inc_life_recovery_rate(line: &str) -> Option<f64> {
    let l = line.to_ascii_lowercase();
    let pct_idx = l.find('%')?;
    let head = &l[..pct_idx];
    // Walk back from `%` to the first non-numeric char; the slice between
    // there and `%` is the rate. If the line *starts* with the digits (no
    // preceding non-digit) the rate is the entire `head` slice.
    let num_start = head
        .rfind(|c: char| !c.is_ascii_digit() && c != '.')
        .map_or(0, |i| i + 1);
    head[num_start..].trim().parse::<f64>().ok()
}

/// Issue #196: Tempered/Transcendent Mind marker — the leading rate
/// (`2%` / `3%`) varies between variants but the rest of the line is
/// fixed. Returns `true` for any rate so a single dispatch arm can
/// read the percentage off the matching mod_line.
fn is_int_count_to_inc_mana_recovery_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("increased mana recovery rate")
        && l.contains("per 10 intelligence")
        && l.contains("on allocated passives in radius")
}

/// Issue #196: extract the leading percentage (`N` from `N% increased
/// Mana Recovery Rate per 10 Intelligence on Allocated Passives in
/// Radius`). Used by the dispatch arm for
/// [`HandlerKind::IntCountToIncManaRecovery`] to scale Tempered Mind's
/// 2% vs Transcendent Mind (current)'s 3% off the same handler.
fn parse_int_count_to_inc_mana_recovery_rate(line: &str) -> Option<f64> {
    let l = line.to_ascii_lowercase();
    let pct_idx = l.find('%')?;
    let head = &l[..pct_idx];
    let num_start = head
        .rfind(|c: char| !c.is_ascii_digit() && c != '.')
        .map_or(0, |i| i + 1);
    head[num_start..].trim().parse::<f64>().ok()
}

/// Issue #196: The Light of Meaning (Life variant) marker. Recognises the
/// jewel's `Passive Skills in Radius also grant +N to maximum Life` line —
/// pulled out of the vanilla parse path so the per-node grant emits via
/// the [`HandlerKind::PassiveAlsoGrantBaseLife`] dispatch arm rather than
/// landing globally on the player. Mirrors PoB's `jewelOtherFuncs` entry
/// at `ModParser.lua:6054-6060` (pattern
/// `Passive Skills in Radius also grant +(N) to Maximum Life`).
fn is_passive_also_grant_base_life_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("passive skills in radius also grant")
        && l.contains("to maximum life")
        && !l.contains("mana")
}

/// Issue #196: parse the `+N` off The Light of Meaning's Life-variant
/// line (`Passive Skills in Radius also grant +5 to maximum Life` →
/// `5`). Returns `None` when the line doesn't carry an integer grant.
fn parse_passive_also_grant_base_life_value(line: &str) -> Option<f64> {
    let l = line.to_ascii_lowercase();
    let plus_idx = l.find('+')?;
    let tail = &l[plus_idx + 1..];
    let end = tail
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(tail.len());
    tail[..end].trim().parse::<f64>().ok()
}

fn is_int_count_to_inc_chaos_damage_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("5% increased chaos damage")
        && l.contains("per 10 intelligence")
        && l.contains("allocated passives in radius")
}

fn is_dex_count_to_max_lightning_attack_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("adds 1 maximum lightning damage to attacks")
        && l.contains("per 1 dexterity")
        && l.contains("in radius")
}

/// Issue #196: Tempered/Transcendent Spirit marker — the leading rate
/// (`2%` / `3%`) varies between variants but the rest of the line is
/// fixed. Returns `true` for any rate so a single dispatch arm can read
/// the percentage off the matching mod_line.
fn is_dex_count_to_inc_movement_speed_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("increased movement speed")
        && l.contains("per 10 dexterity")
        && l.contains("on allocated passives in radius")
}

/// Issue #196: extract the leading percentage (`N` from `N% increased
/// Movement Speed per 10 Dexterity on Allocated Passives in Radius`).
/// Used by the dispatch arm for [`HandlerKind::DexCountToIncMovementSpeed`]
/// to scale Tempered Spirit's 2% vs Transcendent Spirit (current)'s 3%
/// off the same handler.
fn parse_dex_count_to_inc_movement_speed_rate(line: &str) -> Option<f64> {
    let l = line.to_ascii_lowercase();
    let pct_idx = l.find('%')?;
    let head = &l[..pct_idx];
    // Walk back from `%` to the first non-numeric char; the slice between
    // there and `%` is the rate. If the line *starts* with the digits (no
    // preceding non-digit) the rate is the entire `head` slice.
    let num_start = head
        .rfind(|c: char| !c.is_ascii_digit() && c != '.')
        .map_or(0, |i| i + 1);
    head[num_start..].trim().parse::<f64>().ok()
}

/// Issue #196: Might in All Forms marker. The jewel's only radius line is
/// `Dexterity and Intelligence from passives in Radius count towards
/// Strength Melee Damage bonus`. PoB matches the lowercase form verbatim
/// in `jewelSelfFuncs`; we use a substring match against the load-bearing
/// fragments to tolerate trivial whitespace / casing variation from the
/// item-text parser.
fn is_dex_int_to_str_melee_bonus_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("dexterity and intelligence from passives in radius")
        && l.contains("strength melee damage bonus")
}

/// Issue #196 (Pure Talent): the seven base classes whose starting locations
/// the jewel checks for connection. Mirrors PoB's `PureTalent` handler list
/// and the upstream jewel text. Replica Pure Talent uses the same set.
const PURE_TALENT_CLASSES: &[&str] = &[
    "Marauder", "Duelist", "Ranger", "Shadow", "Witch", "Templar", "Scion",
];

/// Issue #196: build the set of classes the player's tree currently connects
/// to for Pure Talent purposes. The player's own class always counts (PoB
/// treats the class-start anchor as connected even when not in the allocated
/// set). Any other class's `ClassStart` node that's been allocated also
/// counts — that's how a path that crosses through an adjacent class start
/// picks up its bonus.
fn pure_talent_connected_classes(
    player_class: &str,
    tree: &PassiveTree,
    allocated: &AHashSet<NodeId>,
) -> std::collections::HashSet<String> {
    let mut connected: std::collections::HashSet<String> = std::collections::HashSet::new();
    if PURE_TALENT_CLASSES.contains(&player_class) {
        connected.insert(player_class.to_owned());
    }
    for &id in allocated {
        let Some(node) = tree.nodes.get(&id) else {
            continue;
        };
        if node.kind != NodeKind::ClassStart {
            continue;
        }
        let Some(idx) = node.class_start_index else {
            continue;
        };
        // `tree.classes` is indexed positionally — `class_start_index` is the
        // same index PoB uses for `Build.targetVersion` class lookups.
        let Some(class) = tree.classes.get(idx as usize) else {
            continue;
        };
        if PURE_TALENT_CLASSES.contains(&class.name.as_str()) {
            connected.insert(class.name.clone());
        }
    }
    connected
}

/// Issue #196: walk Pure Talent's `mod_lines`, strip the leading `<Class>: `
/// prefix from each, and emit the resulting mod globally only when the class
/// is in `connected`. Returns the number of mods successfully emitted so the
/// dispatch's `RadiusJewelReport.mod_emissions` stays accurate.
fn apply_pure_talent_lines(
    item: &Item,
    connected: &std::collections::HashSet<String>,
    source_label: &str,
    db: &mut crate::ModDB,
) -> usize {
    let mut emitted = 0usize;
    for ml in &item.mod_lines {
        let raw = ml.line.trim();
        let Some((prefix, body)) = raw.split_once(": ") else {
            continue;
        };
        if !PURE_TALENT_CLASSES.contains(&prefix) {
            // Non-class metadata like `Limited to: 1` or PoB's
            // `Variant: Current` lines are intentionally dropped here —
            // they don't contribute mods to the build.
            continue;
        }
        if !connected.contains(prefix) {
            continue;
        }
        let body = body.trim();
        if body.is_empty() {
            continue;
        }
        if let Some(parsed) = parse_mod_line(body) {
            let mut clone = parsed.mod_;
            clone.source = Some(Source::Other(format!("{source_label}:{prefix}")));
            db.add(clone);
            emitted += 1;
        }
    }
    emitted
}

fn is_dex_int_transform_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("dexterity from passives in radius") && l.contains("transformed to intelligence")
}

fn is_str_double_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("strength from passives in radius") && l.contains("doubled")
}

fn is_str_life_transform_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("strength from passives in radius") && l.contains("transformed to life")
}

fn is_jewel_base(base: &str) -> bool {
    // Exclude bases that *contain* "Jewel" but aren't actually jewels — defensive
    // against future base names. The current set of jewel bases all literally end
    // with "Jewel" or are eye jewels.
    if base.is_empty() {
        return false;
    }
    base.contains("Jewel")
        || base.ends_with("Eye Jewel")
        || base.contains("Eye Jewel")
        || base == "Crimson Jewel"
        || base == "Viridian Jewel"
        || base == "Cobalt Jewel"
        || base == "Prismatic Jewel"
}

/// Cluster / Abyss / Timeless / Charm jewels follow their own dispatch path. We bail
/// out of the generic framework when the base name flags one of those subtypes —
/// for now the heuristic uses the base-name suffix; a future slice will cross-check
/// against `bases.json`'s `sub_type` field.
fn is_special_jewel_subtype(item: &Item) -> bool {
    let n = &item.base_name;
    n.ends_with("Cluster Jewel")
        || n.contains("Abyss")
        || n.contains("Eye Jewel") // Abyss eye-jewel bases (Murderous/Searching/...)
        || matches!(
            n.as_str(),
            "Timeless Jewel"
                | "Lethal Pride"
                | "Brutal Restraint"
                | "Glorious Vanity"
                | "Elegant Hubris"
                | "Militant Faith"
        )
        || n.starts_with("Grand Spectrum")
            && n.contains("Charm")
        || n.contains("Charm")
}

fn mentions_radius(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("passives in radius")
        || l.contains("nearby allocated passives")
        || l.contains("nodes in radius")
        || l.contains("in radius are")
}

fn explicit_ring_label(line: &str) -> Option<&'static str> {
    let l = line.to_ascii_lowercase();
    // PoB canonical text: "Only affects Passives in Small/Medium/Large/Very Large/Massive Ring".
    if !l.contains("only affects passives in") {
        return None;
    }
    if l.contains("massive") {
        Some("Massive")
    } else if l.contains("very large") {
        Some("Very Large")
    } else if l.contains("large") {
        Some("Large")
    } else if l.contains("medium") {
        Some("Medium")
    } else if l.contains("small") {
        Some("Small")
    } else {
        None
    }
}

/// PoB tree version string the radii table should be picked from. For now this is
/// always the modern (3.16+) default — items don't carry their tree version. A
/// future cluster / timeless slice will plumb the active tree version through.
fn item_tree_version(_item: &Item) -> String {
    "3_16".to_string()
}

/// Trim the radius-jewel metadata trailer off a mod line. When a vanilla radius
/// jewel says e.g. `10% increased Maximum Life to nearby allocated passives`, the
/// `to nearby allocated passives` part is a marker for the framework that the mod
/// should be applied per in-radius allocated node — it shouldn't end up in the
/// canonical mod-name. Stripping the trailer lets [`parse_mod_line`] mint the
/// regular `Life` / `MovementSpeed` / etc. keys instead of long-form suffixed
/// aliases that no calc consumer reads.
///
/// Returns `None` when the input has no recognised trailer (i.e. it's already a
/// plain mod line, or the trailer pattern doesn't match).
fn strip_radius_suffix(line: &str) -> Option<String> {
    // Patterns are listed longest-first so a line that contains multiple matches
    // strips the most specific one. Lower-case comparison keeps the match
    // case-insensitive; we splice using the original byte offset so casing in
    // the surviving prefix is preserved.
    const PATTERNS: &[&str] = &[
        " to nearby allocated passives",
        " to all allocated passives in Radius",
        " to allocated Passives in Radius",
        " to Passives in Radius",
        " from Passives in Radius",
        " from allocated Passives in Radius",
        " from nearby allocated passives",
        " for each allocated passive in radius",
        " in Radius",
    ];
    let lower = line.to_ascii_lowercase();
    for pat in PATTERNS {
        let pat_lc = pat.to_ascii_lowercase();
        if let Some(pos) = lower.find(&pat_lc) {
            // Slice the original string at the matched byte offset.
            let head = &line[..pos];
            let tail = &line[pos + pat.len()..];
            // Recombine head + any remaining trailing text (typically nothing,
            // sometimes a trailing comma).
            let mut out = head.trim_end().to_string();
            let trail = tail.trim_start();
            if !trail.is_empty() {
                out.push(' ');
                out.push_str(trail);
            }
            return Some(out);
        }
    }
    None
}

/// Per-character socketed-jewel storage. Maps tree-socket node id → jewel item.
/// Lives next to `Character` rather than on `ItemSet` because the [`Slot`] enum is
/// fixed-arity (Helmet/BodyArmour/…) and the tree exposes 60 jewel sockets, plus
/// timeless / cluster / abyss sockets we'll synthesise later.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SocketedJewels {
    /// Wire-format-friendly Vec — stable insertion order for round-trip.
    #[serde(default)]
    pub entries: Vec<(NodeId, Item)>,
}

impl SocketedJewels {
    pub fn new() -> Self {
        Self::default()
    }

    /// Socket `item` into `node_id`, replacing any existing jewel.
    pub fn socket(&mut self, node_id: NodeId, item: Item) {
        if let Some(slot) = self.entries.iter_mut().find(|(id, _)| *id == node_id) {
            slot.1 = item;
        } else {
            self.entries.push((node_id, item));
        }
    }

    /// Remove the jewel at `node_id`. Returns the unsocketed item.
    pub fn unsocket(&mut self, node_id: NodeId) -> Option<Item> {
        if let Some(pos) = self.entries.iter().position(|(id, _)| *id == node_id) {
            Some(self.entries.remove(pos).1)
        } else {
            None
        }
    }

    pub fn get(&self, node_id: NodeId) -> Option<&Item> {
        self.entries
            .iter()
            .find(|(id, _)| *id == node_id)
            .map(|(_, it)| it)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&NodeId, &Item)> {
        self.entries.iter().map(|(id, it)| (id, it))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Apply every socketed radius jewel's mods to the env. Each in-radius allocated
/// node receives one copy of every parsed mod line on the jewel; copies are sourced
/// as `Source::Passive(<node_id>)` so per-node breakdown attribution treats the
/// bonus as if it lived on that passive itself, mirroring PoB's
/// `buildModListForNode` "Self" pass.
///
/// Returns the number of (jewel, node) emissions performed, suitable for tests and
/// for callers that want a quick diagnostic ("X mods applied across Y radius
/// jewels"). Skips invalid socket node ids and silently drops jewels that don't
/// identify as radius jewels via [`identify_radius_jewel`].
pub fn apply_radius_jewels(
    tree: &PassiveTree,
    allocated: &AHashSet<NodeId>,
    socketed: &SocketedJewels,
    player_class: &str,
    db: &mut crate::ModDB,
) -> RadiusJewelReport {
    let mut report = RadiusJewelReport::default();
    for (socket_id, item) in socketed.iter() {
        let Some(jewel) = identify_radius_jewel(*socket_id, item) else {
            report.skipped += 1;
            continue;
        };
        report.applied_jewels += 1;
        match jewel.kind {
            HandlerKind::WatchersEye => {
                // Aura-conditional global buff: emit each parsed mod once into
                // the player's modDB. The mod's `AffectedBy<Aura>` Condition
                // tag (set by the parser) gates application; the conditions
                // themselves are flipped on by the active-aura detection in
                // perform.rs. The base implicits (`X% increased maximum Life`
                // etc.) parse without a condition tag and apply globally.
                for m in &jewel.mods {
                    let mut clone = m.clone();
                    clone.source = Some(Source::Other(jewel.source_label.clone()));
                    db.add(clone);
                    report.mod_emissions += 1;
                }
            }
            HandlerKind::LifeToEnergyShieldTransform => {
                // Issue #196 (slice 3): Energy from Within. Same shape
                // as the Life→Mana arm but routes Inc Life to
                // EnergyShield at 1× scale (PoB doesn't double for ES).
                for m in &jewel.mods {
                    let mut clone = m.clone();
                    clone.source = Some(Source::Other(jewel.source_label.clone()));
                    db.add(clone);
                    report.mod_emissions += 1;
                }
                let in_radius =
                    allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
                let n = transform_radius_attribute(
                    tree,
                    db,
                    &in_radius,
                    "Life",
                    "EnergyShield",
                    crate::ModType::Inc,
                    1.0,
                    &jewel.source_label,
                );
                report.mod_emissions += n;
            }
            HandlerKind::DexCountToMaxLightningAttack => {
                // Issue #196 (slice 12): Static Electricity. Sum
                // allocated `+N Dexterity` BASE mods across in-radius
                // nodes; emit a single `LightningDamage` BASE mod
                // shaped as `Range { 0, dex }` with ATTACK + LIGHTNING
                // flags. Matches the parser's emission for
                // `Adds N to M Lightning Damage to Attacks` so
                // `perform.rs`'s flat-damage adds loop picks it up.
                for m in &jewel.mods {
                    let mut clone = m.clone();
                    clone.source = Some(Source::Other(jewel.source_label.clone()));
                    db.add(clone);
                    report.mod_emissions += 1;
                }
                let in_radius =
                    allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
                let dex_sum = sum_radius_attribute_base(tree, &in_radius, "Dexterity");
                if dex_sum > 0.0 {
                    let mut mod_ = crate::Mod::base(
                        "LightningDamage",
                        crate::ModValue::Range {
                            min: 0.0,
                            max: dex_sum,
                        },
                    );
                    mod_.flags |= pob_data::ModFlag::ATTACK;
                    mod_.keyword_flags |= pob_data::KeywordFlag::LIGHTNING;
                    mod_.source = Some(Source::Other(jewel.source_label.clone()));
                    db.add(mod_);
                    report.mod_emissions += 1;
                }
            }
            HandlerKind::IntCountToIncChaosDamage => {
                // Issue #196 (slice 11): Eldritch Knowledge. Sum
                // allocated `+N Intelligence` BASE mods across in-
                // radius nodes, integer-divide by 10, multiply by 5,
                // emit a single `Inc ChaosDamage` mod sourced as the
                // jewel.
                for m in &jewel.mods {
                    let mut clone = m.clone();
                    clone.source = Some(Source::Other(jewel.source_label.clone()));
                    db.add(clone);
                    report.mod_emissions += 1;
                }
                let in_radius =
                    allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
                let int_sum = sum_radius_attribute_base(tree, &in_radius, "Intelligence");
                let inc_pct = (int_sum / 10.0).floor() * 5.0;
                if inc_pct > 0.0 {
                    let mod_ = crate::Mod::inc("ChaosDamage", inc_pct)
                        .with_source(Source::Other(jewel.source_label.clone()));
                    db.add(mod_);
                    report.mod_emissions += 1;
                }
            }
            HandlerKind::StrCountToIncTotemLife => {
                // Issue #196 (slice 10): Spire of Stone. Sum allocated
                // `+N Strength` BASE mods across in-radius nodes,
                // integer-divide by 10, multiply by 3, emit a single
                // `Inc TotemLife` mod sourced as the jewel. The
                // jewel's `Totems cannot be Stunned` line is left to
                // vanilla mod_parser.
                for m in &jewel.mods {
                    let mut clone = m.clone();
                    clone.source = Some(Source::Other(jewel.source_label.clone()));
                    db.add(clone);
                    report.mod_emissions += 1;
                }
                let in_radius =
                    allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
                let str_sum = sum_radius_attribute_base(tree, &in_radius, "Strength");
                let inc_pct = (str_sum / 10.0).floor() * 3.0;
                if inc_pct > 0.0 {
                    let mod_ = crate::Mod::inc("TotemLife", inc_pct)
                        .with_source(Source::Other(jewel.source_label.clone()));
                    db.add(mod_);
                    report.mod_emissions += 1;
                }
            }
            HandlerKind::StrCountToIncLifeRecovery => {
                // Issue #196: Tempered Flesh / Transcendent Flesh. Sum
                // allocated `+N Strength` BASE mods across in-radius
                // nodes, integer-divide by 10, multiply by the per-line
                // rate (2 for Tempered Flesh / Transcendent Flesh
                // pre-3.10, 3 for current Transcendent Flesh) parsed
                // off the matching mod_line, and emit a single Inc
                // `LifeRecovery` mod sourced as the jewel.
                for m in &jewel.mods {
                    let mut clone = m.clone();
                    clone.source = Some(Source::Other(jewel.source_label.clone()));
                    db.add(clone);
                    report.mod_emissions += 1;
                }
                let in_radius =
                    allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
                let str_sum = sum_radius_attribute_base(tree, &in_radius, "Strength");
                let rate = item
                    .mod_lines
                    .iter()
                    .filter(|ml| is_str_count_to_inc_life_recovery_marker(&ml.line))
                    .find_map(|ml| parse_str_count_to_inc_life_recovery_rate(&ml.line))
                    .unwrap_or(0.0);
                let inc_pct = (str_sum / 10.0).floor() * rate;
                if inc_pct > 0.0 {
                    let mod_ = crate::Mod::inc("LifeRecovery", inc_pct)
                        .with_source(Source::Other(jewel.source_label.clone()));
                    db.add(mod_);
                    report.mod_emissions += 1;
                }
            }
            HandlerKind::DexCountToIncEvasion => {
                // Issue #196 (slices 9 + 14 + unarmed): Pugilist. Sums
                // in-radius allocated Dex once and routes the
                // integer-divided count into per-line emissions:
                //   - Evasion line  → Inc Evasion (vanilla)
                //   - Claw line     → Inc PhysicalDamage with the
                //                     CLAW modflag (only counts when
                //                     wielding a claw)
                //   - Unarmed line  → Inc PhysicalDamage with the
                //                     MELEE+UNARMED modflags (only
                //                     counts when attacking unarmed
                //                     in melee)
                // Mirrors PoB ModParser.lua jewelSelfFuncs at
                // line 6155 — `getPerStat("PhysicalDamage", "INC",
                // ModFlag.Unarmed, "Dex", 1 / 3)` — where
                // `ModFlag.Unarmed` (0x01000000) is OR'd with
                // `ModFlag.Melee` (0x00000100) by the underlying mod
                // record. The marker text differs from the jewel's
                // visible "with Unarmed Attacks" wording (PoB
                // normalizes to "while Unarmed") but ModCache.lua
                // line 2287 treats both forms as flags=16777476
                // (MELEE | UNARMED).
                for m in &jewel.mods {
                    let mut clone = m.clone();
                    clone.source = Some(Source::Other(jewel.source_label.clone()));
                    db.add(clone);
                    report.mod_emissions += 1;
                }
                let in_radius =
                    allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
                let dex_sum = sum_radius_attribute_base(tree, &in_radius, "Dexterity");
                let inc_pct = (dex_sum / 3.0).floor();
                if inc_pct > 0.0 {
                    if item
                        .mod_lines
                        .iter()
                        .any(|ml| is_pugilist_evasion_line_marker(&ml.line))
                    {
                        let mod_ = crate::Mod::inc("Evasion", inc_pct)
                            .with_source(Source::Other(jewel.source_label.clone()));
                        db.add(mod_);
                        report.mod_emissions += 1;
                    }
                    if item
                        .mod_lines
                        .iter()
                        .any(|ml| is_pugilist_claw_phys_marker(&ml.line))
                    {
                        let mut mod_ = crate::Mod::inc("PhysicalDamage", inc_pct);
                        mod_.flags |= pob_data::ModFlag::CLAW;
                        mod_.keyword_flags |= pob_data::KeywordFlag::PHYSICAL;
                        mod_.source = Some(Source::Other(jewel.source_label.clone()));
                        db.add(mod_);
                        report.mod_emissions += 1;
                    }
                    if item
                        .mod_lines
                        .iter()
                        .any(|ml| is_pugilist_unarmed_phys_marker(&ml.line))
                    {
                        let mut mod_ = crate::Mod::inc("PhysicalDamage", inc_pct);
                        mod_.flags |= pob_data::ModFlag::MELEE | pob_data::ModFlag::UNARMED;
                        mod_.keyword_flags |= pob_data::KeywordFlag::PHYSICAL;
                        mod_.source = Some(Source::Other(jewel.source_label.clone()));
                        db.add(mod_);
                        report.mod_emissions += 1;
                    }
                }
            }
            HandlerKind::IntCountToLife => {
                // Issue #196 (slice 8): Anatomical Knowledge. Sum
                // `+N Int` BASE mods across in-radius allocated nodes,
                // integer-divide by 3, emit a single `+(N/3) Life`
                // BASE sourced as the jewel.
                for m in &jewel.mods {
                    let mut clone = m.clone();
                    clone.source = Some(Source::Other(jewel.source_label.clone()));
                    db.add(clone);
                    report.mod_emissions += 1;
                }
                let in_radius =
                    allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
                let int_sum = sum_radius_attribute_base(tree, &in_radius, "Intelligence");
                let life = (int_sum / 3.0).floor();
                if life > 0.0 {
                    let mod_ = crate::Mod::base("Life", life)
                        .with_source(Source::Other(jewel.source_label.clone()));
                    db.add(mod_);
                    report.mod_emissions += 1;
                }
            }
            HandlerKind::DexIntToStrMeleeBonus => {
                // Issue #196: Might in All Forms. Sum `+N Dexterity`
                // and `+N Intelligence` BASE mods across in-radius
                // allocated nodes and emit a single
                // `DexIntToMeleeBonus` BASE mod whose value is
                // `Dex + Int`. PoB folds this stat back into the
                // Strength → Melee Damage bonus computation; we emit
                // the canonical stat name so future calc consumers
                // pick it up without further plumbing here.
                for m in &jewel.mods {
                    let mut clone = m.clone();
                    clone.source = Some(Source::Other(jewel.source_label.clone()));
                    db.add(clone);
                    report.mod_emissions += 1;
                }
                let in_radius =
                    allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
                let dex_sum = sum_radius_attribute_base(tree, &in_radius, "Dexterity");
                let int_sum = sum_radius_attribute_base(tree, &in_radius, "Intelligence");
                let total = dex_sum + int_sum;
                if total > 0.0 {
                    let mod_ = crate::Mod::base("DexIntToMeleeBonus", total)
                        .with_source(Source::Other(jewel.source_label.clone()));
                    db.add(mod_);
                    report.mod_emissions += 1;
                }
            }
            HandlerKind::DexCountToIncMovementSpeed => {
                // Issue #196: Tempered Spirit / Transcendent Spirit. Sum
                // allocated `+N Dexterity` BASE mods across in-radius
                // nodes, integer-divide by 10, multiply by the per-line
                // rate (2 for Tempered Spirit / Transcendent pre-3.10,
                // 3 for current Transcendent Spirit) parsed off the
                // matching mod_line, and emit a single Inc
                // `MovementSpeed` mod sourced as the jewel.
                for m in &jewel.mods {
                    let mut clone = m.clone();
                    clone.source = Some(Source::Other(jewel.source_label.clone()));
                    db.add(clone);
                    report.mod_emissions += 1;
                }
                let in_radius =
                    allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
                let dex_sum = sum_radius_attribute_base(tree, &in_radius, "Dexterity");
                let rate = item
                    .mod_lines
                    .iter()
                    .filter(|ml| is_dex_count_to_inc_movement_speed_marker(&ml.line))
                    .find_map(|ml| parse_dex_count_to_inc_movement_speed_rate(&ml.line))
                    .unwrap_or(0.0);
                let inc_pct = (dex_sum / 10.0).floor() * rate;
                if inc_pct > 0.0 {
                    let mod_ = crate::Mod::inc("MovementSpeed", inc_pct)
                        .with_source(Source::Other(jewel.source_label.clone()));
                    db.add(mod_);
                    report.mod_emissions += 1;
                }
            }
            HandlerKind::IntCountToIncManaRecovery => {
                // Issue #196: Tempered Mind / Transcendent Mind. Sum
                // allocated `+N Intelligence` BASE mods across in-radius
                // nodes, integer-divide by 10, multiply by the per-line
                // rate (2 for Tempered Mind, 3 for current Transcendent
                // Mind) parsed off the matching mod_line, and emit a
                // single Inc `ManaRecovery` mod sourced as the jewel.
                for m in &jewel.mods {
                    let mut clone = m.clone();
                    clone.source = Some(Source::Other(jewel.source_label.clone()));
                    db.add(clone);
                    report.mod_emissions += 1;
                }
                let in_radius =
                    allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
                let int_sum = sum_radius_attribute_base(tree, &in_radius, "Intelligence");
                let rate = item
                    .mod_lines
                    .iter()
                    .filter(|ml| is_int_count_to_inc_mana_recovery_marker(&ml.line))
                    .find_map(|ml| parse_int_count_to_inc_mana_recovery_rate(&ml.line))
                    .unwrap_or(0.0);
                let inc_pct = (int_sum / 10.0).floor() * rate;
                if inc_pct > 0.0 {
                    let mod_ = crate::Mod::inc("ManaRecovery", inc_pct)
                        .with_source(Source::Other(jewel.source_label.clone()));
                    db.add(mod_);
                    report.mod_emissions += 1;
                }
            }
            HandlerKind::OtherDamageToFireTransform => {
                // Issue #196 (slice 15): Fireborn. Same shape as
                // Cold Steel but four source directions all rolling
                // additively into Inc FireDamage. The original mods
                // stay (Inc transforms have no counter), so each
                // in-radius node ends up contributing to both its
                // original element and Fire.
                for m in &jewel.mods {
                    let mut clone = m.clone();
                    clone.source = Some(Source::Other(jewel.source_label.clone()));
                    db.add(clone);
                    report.mod_emissions += 1;
                }
                let in_radius =
                    allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
                for from in [
                    "PhysicalDamage",
                    "ColdDamage",
                    "LightningDamage",
                    "ChaosDamage",
                ] {
                    let n = transform_radius_attribute(
                        tree,
                        db,
                        &in_radius,
                        from,
                        "FireDamage",
                        crate::ModType::Inc,
                        1.0,
                        &jewel.source_label,
                    );
                    report.mod_emissions += n;
                }
            }
            HandlerKind::PhysColdSwapTransform => {
                // Issue #196 (slice 7): Cold Steel. Two passes — Phys
                // → Cold and Cold → Phys — both at 1× scale. Inc
                // mods don't get a counter from
                // `transform_radius_attribute`, so the originals stay
                // and each in-radius node ends up contributing to
                // both damage types.
                for m in &jewel.mods {
                    let mut clone = m.clone();
                    clone.source = Some(Source::Other(jewel.source_label.clone()));
                    db.add(clone);
                    report.mod_emissions += 1;
                }
                let in_radius =
                    allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
                let n1 = transform_radius_attribute(
                    tree,
                    db,
                    &in_radius,
                    "PhysicalDamage",
                    "ColdDamage",
                    crate::ModType::Inc,
                    1.0,
                    &jewel.source_label,
                );
                let n2 = transform_radius_attribute(
                    tree,
                    db,
                    &in_radius,
                    "ColdDamage",
                    "PhysicalDamage",
                    crate::ModType::Inc,
                    1.0,
                    &jewel.source_label,
                );
                report.mod_emissions += n1 + n2;
            }
            HandlerKind::EnergyShieldToArmourTransform => {
                // Issue #196 (slice 6): Energised Armour. Same shape as
                // the Life→Mana arm but routes Inc EnergyShield to
                // Armour at 2× scale (PoB doubles per the jewel text).
                for m in &jewel.mods {
                    let mut clone = m.clone();
                    clone.source = Some(Source::Other(jewel.source_label.clone()));
                    db.add(clone);
                    report.mod_emissions += 1;
                }
                let in_radius =
                    allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
                let n = transform_radius_attribute(
                    tree,
                    db,
                    &in_radius,
                    "EnergyShield",
                    "Armour",
                    crate::ModType::Inc,
                    2.0,
                    &jewel.source_label,
                );
                report.mod_emissions += n;
            }
            HandlerKind::LifeToManaTransform => {
                // First, emit the jewel's plain mods (e.g. `+15% increased
                // maximum Mana`) globally — these aren't transforms and apply
                // exactly like vanilla jewel bonuses.
                for m in &jewel.mods {
                    let mut clone = m.clone();
                    clone.source = Some(Source::Other(jewel.source_label.clone()));
                    db.add(clone);
                    report.mod_emissions += 1;
                }
                // Then walk the in-radius allocated nodes' stats, find any
                // Inc/Reduce Life mod, and emit an equivalent Inc/Reduce Mana
                // mod at 200% of the source value sourced as the in-radius
                // node so per-node breakdowns attribute the bonus correctly.
                let in_radius =
                    allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
                let n = transform_radius_attribute(
                    tree,
                    db,
                    &in_radius,
                    "Life",
                    "Mana",
                    crate::ModType::Inc,
                    2.0,
                    &jewel.source_label,
                );
                report.mod_emissions += n;
            }
            HandlerKind::DexToIntTransform => {
                // Emit base mods (`+(16-24) to Intelligence`) globally first.
                for m in &jewel.mods {
                    let mut clone = m.clone();
                    clone.source = Some(Source::Other(jewel.source_label.clone()));
                    db.add(clone);
                    report.mod_emissions += 1;
                }
                // For each in-radius allocated node, find `+N Dex` BASE mods
                // and emit an equivalent `+N Int` BASE; do not double-count
                // the Dex (PoB's transform fully replaces it). We model that
                // by emitting an offsetting `-N Dex` BASE so the original
                // node-side Dex contribution cancels out.
                let in_radius =
                    allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
                let n = transform_radius_attribute(
                    tree,
                    db,
                    &in_radius,
                    "Dexterity",
                    "Intelligence",
                    crate::ModType::Base,
                    1.0,
                    &jewel.source_label,
                );
                report.mod_emissions += n;
            }
            HandlerKind::DexToStrTransform => {
                // Issue #196 (slice 4): Inertia. Mirror of the Dex→Int
                // arm; emit jewel-level plain mods globally first, then
                // transform each in-radius `+N Dex` BASE into `+N Str`
                // BASE sourced as the in-radius node, with a counter
                // `-N Dex` BASE so the source attribute is moved
                // rather than duplicated.
                for m in &jewel.mods {
                    let mut clone = m.clone();
                    clone.source = Some(Source::Other(jewel.source_label.clone()));
                    db.add(clone);
                    report.mod_emissions += 1;
                }
                let in_radius =
                    allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
                let n = transform_radius_attribute(
                    tree,
                    db,
                    &in_radius,
                    "Dexterity",
                    "Strength",
                    crate::ModType::Base,
                    1.0,
                    &jewel.source_label,
                );
                report.mod_emissions += n;
            }
            // Issue #196 (slice 5): Brute Force Solution / Careful
            // Planning / Efficient Training. Each is a 1× BASE
            // attribute transform with the same shape as Inertia /
            // Fertile Mind — we differ only in the (from, to) pair.
            HandlerKind::StrToIntTransform
            | HandlerKind::IntToDexTransform
            | HandlerKind::IntToStrTransform
            | HandlerKind::StrToDexTransform => {
                let (from, to) = match jewel.kind {
                    HandlerKind::StrToIntTransform => ("Strength", "Intelligence"),
                    HandlerKind::IntToDexTransform => ("Intelligence", "Dexterity"),
                    HandlerKind::IntToStrTransform => ("Intelligence", "Strength"),
                    HandlerKind::StrToDexTransform => ("Strength", "Dexterity"),
                    _ => unreachable!(),
                };
                for m in &jewel.mods {
                    let mut clone = m.clone();
                    clone.source = Some(Source::Other(jewel.source_label.clone()));
                    db.add(clone);
                    report.mod_emissions += 1;
                }
                let in_radius =
                    allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
                let n = transform_radius_attribute(
                    tree,
                    db,
                    &in_radius,
                    from,
                    to,
                    crate::ModType::Base,
                    1.0,
                    &jewel.source_label,
                );
                report.mod_emissions += n;
            }
            HandlerKind::StrDoubleTransform => {
                // Issue #196 (slice 2): Brawn. Emit jewel-level plain
                // mods (`+12 to Strength`) globally first — those
                // aren't subject to the doubling.
                for m in &jewel.mods {
                    let mut clone = m.clone();
                    clone.source = Some(Source::Other(jewel.source_label.clone()));
                    db.add(clone);
                    report.mod_emissions += 1;
                }
                // For each in-radius allocated node's `+N Strength`
                // BASE, emit an extra `+N Strength` BASE sourced as
                // the in-radius node. No counter mod — doubling
                // preserves the source contribution.
                let in_radius =
                    allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
                let n =
                    double_radius_attribute(tree, db, &in_radius, "Strength", crate::ModType::Base);
                report.mod_emissions += n;
            }
            HandlerKind::StrToLifeTransform => {
                // Issue #196: Karui Heart. Emit jewel-level plain mods
                // (`+(20-30) to Strength`) globally first — those aren't
                // subject to the radius transform.
                for m in &jewel.mods {
                    let mut clone = m.clone();
                    clone.source = Some(Source::Other(jewel.source_label.clone()));
                    db.add(clone);
                    report.mod_emissions += 1;
                }
                // For each in-radius allocated node, find `+N Strength` BASE
                // mods and emit an equivalent `+5N Life` BASE sourced as the
                // in-radius node, plus a counter `-N Strength` BASE so the
                // original node-side Str contribution (and the implicit
                // 0.5 × Str → Life chain) is removed in line with PoB's
                // "Transformed to" semantic.
                let in_radius =
                    allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
                let n = transform_radius_attribute(
                    tree,
                    db,
                    &in_radius,
                    "Strength",
                    "Life",
                    crate::ModType::Base,
                    5.0,
                    &jewel.source_label,
                );
                report.mod_emissions += n;
            }
            HandlerKind::PureTalent => {
                // Pure Talent grants per-class bonuses gated on the player's
                // tree connecting to that class' starting location. The own
                // class is always considered connected; any other class's
                // `ClassStart` node that lives in the allocated set counts as
                // connected too. Each `<Class>: <mod>` line on the jewel only
                // emits when its prefix matches a connected class.
                //
                // We read the raw `mod_lines` rather than a pre-parsed list
                // because the class prefix isn't a stat the mod parser
                // understands — stripping it here keeps the parser's per-line
                // mod-text grammar untouched and avoids minting bogus
                // `Marauder` named mods.
                let connected = pure_talent_connected_classes(player_class, tree, allocated);
                let n = apply_pure_talent_lines(item, &connected, &jewel.source_label, db);
                report.mod_emissions += n;
            }
            HandlerKind::Pathfinder => {
                // Issue #196: Intuitive Leap. The jewel doesn't emit mods —
                // its effect is the path-finder bypass surfaced via
                // `intuitive_leap_reachable` and consumed by
                // `Character::allocate_path` / `Character::unallocate`.
                // Counted as `applied_jewels` upstream so the dispatch
                // report still reflects "we recognised this jewel"; the
                // mod-emission count stays at zero.
            }
            HandlerKind::PassiveAlsoGrantBaseLife => {
                // Issue #196: The Light of Meaning (Life variant).
                // Per-node grant — mirrors PoB's `jewelOtherFuncs`
                // entry (`ModParser.lua:6054-6060`) which writes
                // `+N to Maximum Life` to every passive skill in the
                // radius that isn't a Keystone, JewelSocket, or
                // ClassStart. Player benefits from those per-node
                // mods only on *allocated* nodes — so we count the
                // eligible allocated in-radius nodes and emit a
                // single `+(count × N) Life` BASE mod sourced as
                // the jewel.
                let per_node = item
                    .mod_lines
                    .iter()
                    .filter(|ml| is_passive_also_grant_base_life_marker(&ml.line))
                    .find_map(|ml| parse_passive_also_grant_base_life_value(&ml.line))
                    .unwrap_or(0.0);
                if per_node > 0.0 {
                    let in_radius =
                        allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
                    let count = in_radius
                        .iter()
                        .filter(|(id, _)| {
                            tree.nodes
                                .get(id)
                                .map(|n| {
                                    !matches!(n.kind, NodeKind::Keystone | NodeKind::JewelSocket)
                                })
                                .unwrap_or(false)
                        })
                        .count() as f64;
                    let total = per_node * count;
                    if total > 0.0 {
                        let mod_ = crate::Mod::base("Life", total)
                            .with_source(Source::Other(jewel.source_label.clone()));
                        db.add(mod_);
                        report.mod_emissions += 1;
                    }
                }
            }
            // SelfAllocated (default), All, Threshold, SelfUnalloc all
            // currently route through the vanilla per-allocated-node mod
            // copy. The non-Self variants will get their own dispatch arms
            // when the timeless / cluster follow-ups land.
            _ => {
                let in_radius =
                    allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
                for (target_node, _) in in_radius {
                    for m in &jewel.mods {
                        let mut clone = m.clone();
                        clone.source = Some(Source::Passive(target_node));
                        db.add(clone);
                        report.mod_emissions += 1;
                    }
                }
            }
        }
    }
    report
}

/// Issue #196: Intuitive Leap pathfinder bypass. Returns `true` when
/// `target` lies within the radius of any socketed Intuitive Leap whose
/// host socket is itself allocated — those nodes can be allocated without
/// being connected to the tree by a normal path. Used by
/// [`crate::Character::allocate_path`] to short-circuit path-finding for
/// in-radius targets and by [`crate::Character::unallocate`]'s orphan
/// detection so floaters remain allocated when the connection that brought
/// them within radius is removed.
///
/// Performance: linear in the number of socketed jewels × in-radius nodes.
/// Cheap enough to call per-allocation; the rare-jewel filter
/// (`item.name == "Intuitive Leap"`) skips most sockets without hitting
/// the radius scan.
pub fn intuitive_leap_reachable(
    tree: &PassiveTree,
    socketed: &SocketedJewels,
    allocated: &AHashSet<NodeId>,
    target: NodeId,
) -> bool {
    intuitive_leap_radius_set(tree, socketed, allocated).contains(&target)
}

/// Issue #196: every node in the radius of any allocated Intuitive Leap
/// socket. The set is the universe of "free-floating" allocatable nodes
/// (allocated or not). `Character::allocate_path` uses this to short-
/// circuit pathing for in-radius targets via [`intuitive_leap_reachable`].
pub fn intuitive_leap_radius_set(
    tree: &PassiveTree,
    socketed: &SocketedJewels,
    allocated: &AHashSet<NodeId>,
) -> AHashSet<NodeId> {
    let mut out: AHashSet<NodeId> = AHashSet::default();
    for (socket_id, item) in socketed.iter() {
        if item.name != "Intuitive Leap" {
            continue;
        }
        // The IL effect requires the host socket to itself be allocated.
        // PoB enforces this via `JewelData.applies = function(...) return
        // build.spec.allocSubgraphNodes[node.id] end`-style guards; a
        // jewel sitting in an unallocated socket is considered inactive.
        if !allocated.contains(socket_id) {
            continue;
        }
        let jewel = build_intuitive_leap(*socket_id, item);
        for (id, _) in nodes_in_radius(tree, *socket_id, &jewel.radius) {
            out.insert(id);
        }
    }
    out
}

/// Issue #196: orphan-detection extension for Intuitive Leap. Given the
/// classic anchored set (BFS from class-start through allocated edges),
/// iterate to add nodes that should also be treated as anchored because
/// they sit in the radius of an Intuitive Leap socket whose host node IS
/// already anchored. Iterates to a fixed point so a chained IL — IL `A`'s
/// host inside IL `B`'s radius — settles correctly. Returns the extended
/// anchored set (a superset of the input).
///
/// Pure helper; doesn't read or mutate the character's allocation. Cheap
/// enough to call inside `Character::unallocate`'s orphan pass.
pub fn extend_anchored_with_intuitive_leap(
    tree: &PassiveTree,
    socketed: &SocketedJewels,
    allocated: &std::collections::HashSet<NodeId>,
    mut anchored: std::collections::HashSet<NodeId>,
) -> std::collections::HashSet<NodeId> {
    loop {
        let mut added = false;
        for (socket_id, item) in socketed.iter() {
            if item.name != "Intuitive Leap" {
                continue;
            }
            if !anchored.contains(socket_id) {
                continue;
            }
            let jewel = build_intuitive_leap(*socket_id, item);
            for (id, _) in nodes_in_radius(tree, *socket_id, &jewel.radius) {
                if allocated.contains(&id) && anchored.insert(id) {
                    added = true;
                }
            }
        }
        if !added {
            break;
        }
    }
    anchored
}

/// Issue #196: walk an in-radius node list, parse each node's stat lines, and
/// emit a transformed mod for any line that produces a `<from>` mod of the
/// requested kind. The transformed mod targets `<to>` at `scale ×` the source
/// value, sourced as the same in-radius node so per-node breakdowns line up
/// with PoB's. Returns the number of mod copies emitted.
///
/// Used for Healthy Mind (`Inc Life` → `Inc Mana` × 2) and Fertile Mind
/// (`Base Dex` → `Base Int` × 1, plus a counter `-N Dex` so the original Dex
/// contribution from the in-radius node cancels out).
/// Issue #196 (slice 8): sum `+N <attr>` BASE values across in-radius
/// allocated nodes. Used by per-attribute-tally jewels (Anatomical
/// Knowledge: `+1 Life per 3 Int in Radius`) where the emission
/// depends on the *total* attribute count, not on per-node copies.
fn sum_radius_attribute_base(tree: &PassiveTree, in_radius: &[(NodeId, f64)], attr: &str) -> f64 {
    let mut total = 0.0_f64;
    for (node_id, _) in in_radius {
        let Some(node) = tree.nodes.get(node_id) else {
            continue;
        };
        for raw in &node.stats {
            for line in raw.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let Some(parsed) = parse_mod_line(line) else {
                    continue;
                };
                let m = &parsed.mod_;
                if m.kind != crate::ModType::Base || m.name != attr {
                    continue;
                }
                if let Some(value) = m.value.as_f64() {
                    total += value;
                }
            }
        }
    }
    total
}

/// Issue #196 (slice 2): emit one extra copy of each in-radius `+N attr`
/// mod sourced as the in-radius node. Used by Brawn's "doubled"
/// semantics — the extra copy stacks with the engine's existing read of
/// the source mod, so the final attribute value lands at 2× the base
/// reading. Distinct from [`transform_radius_attribute`] which moves the
/// stat (with a counter) rather than duplicating it.
fn double_radius_attribute(
    tree: &PassiveTree,
    db: &mut crate::ModDB,
    in_radius: &[(NodeId, f64)],
    attr: &str,
    kind: crate::ModType,
) -> usize {
    let mut emitted = 0usize;
    for (node_id, _) in in_radius {
        let Some(node) = tree.nodes.get(node_id) else {
            continue;
        };
        for raw in &node.stats {
            for line in raw.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let Some(parsed) = parse_mod_line(line) else {
                    continue;
                };
                let m = &parsed.mod_;
                if m.kind != kind || m.name != attr {
                    continue;
                }
                let Some(value) = m.value.as_f64() else {
                    continue;
                };
                let mut extra = m.clone();
                extra.value = crate::ModValue::Number(value);
                extra.source = Some(Source::Passive(*node_id));
                // Drop tags — we mirror `transform_radius_attribute`'s
                // simplification of ignoring conditional clauses on the
                // source mod.
                extra.tags.clear();
                db.add(extra);
                emitted += 1;
            }
        }
    }
    emitted
}

fn transform_radius_attribute(
    tree: &PassiveTree,
    db: &mut crate::ModDB,
    in_radius: &[(NodeId, f64)],
    from: &str,
    to: &str,
    kind: crate::ModType,
    scale: f64,
    source_label: &str,
) -> usize {
    let mut emitted = 0usize;
    for (node_id, _) in in_radius {
        let Some(node) = tree.nodes.get(node_id) else {
            continue;
        };
        for raw in &node.stats {
            for line in raw.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let Some(parsed) = parse_mod_line(line) else {
                    continue;
                };
                let m = &parsed.mod_;
                if m.kind != kind || m.name != from {
                    continue;
                }
                let Some(value) = m.value.as_f64() else {
                    continue;
                };
                // Emit the transformed mod (e.g. Inc Mana += value × scale)
                // sourced as the in-radius passive so the per-node breakdown
                // attributes the gain to that node.
                let mut to_mod = m.clone();
                to_mod.name = to.to_string();
                to_mod.value = crate::ModValue::Number(value * scale);
                to_mod.source = Some(Source::Passive(*node_id));
                // Drop tags — the transformer ignores conditional clauses on
                // the source mod (PoB's Healthy Mind transforms unconditional
                // Inc Life only, mirroring the simplification).
                to_mod.tags.clear();
                db.add(to_mod);
                emitted += 1;
                // For BASE attribute transforms (Fertile Mind), also emit a
                // counter mod so the original Dex contribution cancels out.
                // This matches PoB's "Transformed to" semantics where the
                // attribute is *moved*, not duplicated.
                if kind == crate::ModType::Base {
                    let mut counter = m.clone();
                    counter.value = crate::ModValue::Number(-value);
                    counter.source = Some(Source::Other(source_label.to_string()));
                    counter.tags.clear();
                    db.add(counter);
                    emitted += 1;
                }
            }
        }
    }
    emitted
}

/// Diagnostic summary returned by [`apply_radius_jewels`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RadiusJewelReport {
    /// Number of socketed jewels successfully identified as radius jewels.
    pub applied_jewels: usize,
    /// Number of socketed jewels skipped because they didn't identify as a radius
    /// jewel (cluster / abyss / timeless / non-radius rare jewel).
    pub skipped: usize,
    /// Total per-(jewel, node) mod copies emitted into the modDB.
    pub mod_emissions: usize,
}

/// Issue #196: apply mods from socketed jewels that don't identify as radius
/// jewels and aren't a special subtype (cluster / abyss / timeless / charm).
/// Mirrors PoB's behaviour where any socketed jewel still contributes its
/// item-level stats to the global modDB — only the *radius-conditional* piece
/// is per-allocated-node. Without this, modern unique jewels whose entire
/// effect is global (Conqueror's Efficiency / Conqueror's Potency / Conqueror's
/// Longevity, plus most rare-rolled Crimson / Viridian / Cobalt / Prismatic
/// jewels with no radius modifiers) silently drop everything they grant.
///
/// Subtype gates:
/// - Cluster jewels feed `cluster_synth` and shouldn't apply their item-level
///   "Adds N Passive Skills" lines globally — those are synthesis metadata.
/// - Abyss / Eye / Timeless / Charm jewels follow their own dispatch paths
///   (#30 timeless, future abyss / charm follow-ups). We bail here so this
///   helper doesn't double-apply mods that the dedicated path will own.
/// - Cluster jewels are also caught by [`is_special_jewel_subtype`] above.
///
/// Mods are sourced as `Source::Other("SocketedJewel:<base>:<socket_id>")` so
/// the Calcs-tab breakdown can attribute each mod back to the socketed item
/// that contributed it. Returns the number of mods successfully parsed and
/// added so callers can spot unparseable mod text in tests.
pub fn apply_non_radius_socketed_jewels(socketed: &SocketedJewels, db: &mut crate::ModDB) -> usize {
    let mut emitted = 0usize;
    for (socket_id, item) in socketed.iter() {
        if !is_jewel_base(&item.base_name) {
            continue;
        }
        if is_special_jewel_subtype(item) {
            continue;
        }
        // Identifiable as a radius jewel → already handled by
        // `apply_radius_jewels`. We only fill the gap for jewels whose
        // *entire* mod set is non-radius.
        if identify_radius_jewel(*socket_id, item).is_some() {
            continue;
        }
        let source = Source::Other(format!("SocketedJewel:{}:{}", item.base_name, socket_id));
        for ml in &item.mod_lines {
            // Defensive: skip the metadata-only ring-size selectors that some
            // hand-crafted jewels still ship (rare, but cheap to filter).
            if explicit_ring_label(&ml.line).is_some() {
                continue;
            }
            if let Some(parsed) = parse_mod_line(&ml.line) {
                let m = parsed.mod_.with_source(source.clone());
                db.add(m);
                emitted += 1;
            }
        }
    }
    emitted
}

/// Convenience wrapper: collect node positions for every node in `tree`. Useful
/// for UI / debug; the radius scan computes positions on demand.
pub fn all_node_positions(tree: &PassiveTree) -> AHashMap<NodeId, (f64, f64)> {
    let mut out: AHashMap<NodeId, (f64, f64)> = AHashMap::default();
    for id in tree.nodes.keys() {
        if let Some(p) = node_position(tree, *id) {
            out.insert(*id, p);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ahash::AHashSet;
    use pob_data::{
        item::{ModSection, Rarity},
        Group, ItemSet, ModLine, Node, NodeKind, PassiveTree, TreeConstants,
    };

    fn mk_tree() -> PassiveTree {
        // Two-node toy tree: a jewel socket at group (0, 0) orbit-0, and a normal
        // passive sitting 600 units to the right (orbit-2 of a 16-orbit group at
        // x=600, orbit_index=4 → angle = 90°, sin=1, cos=0 → x = group.x + 162).
        // Easier: place both nodes at orbit-0 of their own groups so the math is
        // group.x / group.y verbatim.
        let mut groups = ahash::HashMap::default();
        groups.insert(
            10,
            Group {
                x: 0.0,
                y: 0.0,
                orbits: smallvec::smallvec![0],
                background: None,
                nodes: vec![1],
                is_proxy: false,
            },
        );
        groups.insert(
            20,
            Group {
                x: 600.0,
                y: 0.0,
                orbits: smallvec::smallvec![0],
                background: None,
                nodes: vec![2],
                is_proxy: false,
            },
        );
        groups.insert(
            30,
            Group {
                x: 2000.0,
                y: 0.0,
                orbits: smallvec::smallvec![0],
                background: None,
                nodes: vec![3],
                is_proxy: false,
            },
        );
        let mut nodes = ahash::HashMap::default();
        nodes.insert(
            1,
            Node {
                id: 1,
                name: Some("Jewel Socket".into()),
                icon: None,
                ascendancy_name: None,
                stats: vec![],
                reminder_text: vec![],
                kind: NodeKind::JewelSocket,
                class_start_index: None,
                group: Some(10),
                orbit: Some(0),
                orbit_index: Some(0),
                out_edges: smallvec::smallvec![],
                in_edges: smallvec::smallvec![],
                mastery_effects: vec![],
                expansion_jewel_size: None,
                jewel_radius: None,
            },
        );
        nodes.insert(
            2,
            Node {
                id: 2,
                name: Some("Near Notable".into()),
                icon: None,
                ascendancy_name: None,
                stats: vec!["10% increased Life".into()],
                reminder_text: vec![],
                kind: NodeKind::Notable,
                class_start_index: None,
                group: Some(20),
                orbit: Some(0),
                orbit_index: Some(0),
                out_edges: smallvec::smallvec![],
                in_edges: smallvec::smallvec![],
                mastery_effects: vec![],
                expansion_jewel_size: None,
                jewel_radius: None,
            },
        );
        nodes.insert(
            3,
            Node {
                id: 3,
                name: Some("Far Notable".into()),
                icon: None,
                ascendancy_name: None,
                stats: vec!["10% increased Life".into()],
                reminder_text: vec![],
                kind: NodeKind::Notable,
                class_start_index: None,
                group: Some(30),
                orbit: Some(0),
                orbit_index: Some(0),
                out_edges: smallvec::smallvec![],
                in_edges: smallvec::smallvec![],
                mastery_effects: vec![],
                expansion_jewel_size: None,
                jewel_radius: None,
            },
        );
        PassiveTree {
            version: "3_25".into(),
            tree: "Default".into(),
            classes: vec![],
            groups,
            nodes,
            jewel_slots: vec![1],
            min_x: -1000,
            min_y: -1000,
            max_x: 3000,
            max_y: 1000,
            constants: TreeConstants {
                skills_per_orbit: vec![1, 6, 16, 16, 40, 72, 72],
                orbit_radii: vec![0, 82, 162, 335, 493, 662, 846],
                classes: ahash::HashMap::default(),
                character_attributes: ahash::HashMap::default(),
                pss_centre_inner_radius: None,
            },
            points: pob_data::TreePoints::default(),
        }
    }

    fn mk_item(base: &str, mod_lines: &[(&str, ModSection)]) -> Item {
        mk_item_named(base, base, mod_lines)
    }

    fn mk_item_named(name: &str, base: &str, mod_lines: &[(&str, ModSection)]) -> Item {
        Item {
            name: name.into(),
            base_name: base.into(),
            rarity: Rarity::Unique,
            item_level: 84,
            quality: 0,
            tags: ahash::HashSet::default(),
            mod_lines: mod_lines
                .iter()
                .map(|(l, s)| ModLine {
                    line: (*l).to_string(),
                    section: *s,
                    variant_list: None,
                })
                .collect(),
            sockets: String::new(),
            raw: String::new(),
            corrupted: false,
            mirrored: false,
            variants: Vec::new(),
            variant: None,
        }
    }

    #[test]
    fn node_positions_compute() {
        let tree = mk_tree();
        let p1 = node_position(&tree, 1).unwrap();
        let p2 = node_position(&tree, 2).unwrap();
        // Group orbit-0 → node sits at the group origin.
        assert!((p1.0 - 0.0).abs() < 1e-6 && (p1.1 - 0.0).abs() < 1e-6);
        assert!((p2.0 - 600.0).abs() < 1e-6 && (p2.1 - 0.0).abs() < 1e-6);
    }

    #[test]
    fn medium_radius_includes_close_node() {
        let tree = mk_tree();
        let radius = pob_data::RADII_3_16[1]; // Medium 0..1440
        let near = nodes_in_radius(&tree, 1, &radius);
        let ids: Vec<NodeId> = near.iter().map(|(id, _)| *id).collect();
        assert!(ids.contains(&2));
        assert!(!ids.contains(&3)); // Far at 2000 is outside Medium 1440.
    }

    #[test]
    fn allocated_filter() {
        let tree = mk_tree();
        let radius = pob_data::RADII_3_16[1];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let res = allocated_nodes_in_radius(&tree, 1, &radius, &alloc);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].0, 2);
    }

    #[test]
    fn identify_basic_radius_jewel() {
        let item = mk_item(
            "Crimson Jewel",
            &[(
                "10% increased Strength to all allocated Passives in Radius",
                ModSection::Explicit,
            )],
        );
        let jewel = identify_radius_jewel(1, &item).expect("identified");
        assert_eq!(jewel.radius_index, 1); // Medium default
        assert_eq!(jewel.kind, HandlerKind::SelfAllocated);
        assert_eq!(jewel.mods.len(), 1);
    }

    #[test]
    fn identify_explicit_large_ring() {
        let item = mk_item(
            "Cobalt Jewel",
            &[
                (
                    "10% increased Cold Damage to nearby allocated passives",
                    ModSection::Explicit,
                ),
                ("Only affects Passives in Large Ring", ModSection::Explicit),
            ],
        );
        let jewel = identify_radius_jewel(1, &item).expect("identified");
        assert_eq!(jewel.radius_index, 2);
        // Header line is excluded from the mod list.
        assert_eq!(jewel.mods.len(), 1);
    }

    #[test]
    fn cluster_jewel_skipped() {
        let item = mk_item(
            "Small Cluster Jewel",
            &[("10% increased Damage", ModSection::Explicit)],
        );
        assert!(identify_radius_jewel(1, &item).is_none());
    }

    #[test]
    fn timeless_jewel_skipped() {
        let item = mk_item(
            "Lethal Pride",
            &[("Passives in Radius gain something", ModSection::Explicit)],
        );
        assert!(identify_radius_jewel(1, &item).is_none());
    }

    #[test]
    fn non_jewel_item_ignored() {
        let item = mk_item(
            "Driftwood Wand",
            &[("10% increased Spell Damage", ModSection::Explicit)],
        );
        assert!(identify_radius_jewel(1, &item).is_none());
    }

    #[test]
    fn rare_jewel_without_radius_text_skipped() {
        let item = mk_item(
            "Cobalt Jewel",
            &[("+20 to Maximum Life", ModSection::Explicit)],
        );
        assert!(identify_radius_jewel(1, &item).is_none());
    }

    #[test]
    fn apply_emits_one_mod_per_in_radius_node() {
        let tree = mk_tree();
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        // Pretend node 3 is also allocated even though it's outside radius — must
        // still be filtered out.
        alloc.insert(3);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item(
                "Crimson Jewel",
                &[(
                    "10% increased Maximum Life to nearby allocated passives",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        assert_eq!(report.applied_jewels, 1);
        assert_eq!(report.skipped, 0);
        // One in-radius allocated node × one mod = one emission.
        assert_eq!(report.mod_emissions, 1);
    }

    #[test]
    fn socketed_jewels_round_trip_storage() {
        let mut s = SocketedJewels::new();
        s.socket(
            42,
            mk_item(
                "Crimson Jewel",
                &[("+1 to all Attributes", ModSection::Explicit)],
            ),
        );
        assert_eq!(s.len(), 1);
        let pulled = s.unsocket(42).expect("removed");
        assert_eq!(pulled.base_name, "Crimson Jewel");
        assert!(s.is_empty());
    }

    #[test]
    fn strip_suffix_removes_to_nearby_allocated_passives() {
        let stripped =
            strip_radius_suffix("10% increased Maximum Life to nearby allocated passives");
        assert_eq!(stripped.as_deref(), Some("10% increased Maximum Life"));
    }

    #[test]
    fn strip_suffix_removes_from_passives_in_radius() {
        let stripped = strip_radius_suffix("+5 to all Attributes from Passives in Radius");
        assert_eq!(stripped.as_deref(), Some("+5 to all Attributes"));
    }

    #[test]
    fn strip_suffix_returns_none_when_line_has_no_marker() {
        assert!(strip_radius_suffix("+20 to maximum Life").is_none());
        assert!(strip_radius_suffix("10% increased Damage").is_none());
    }

    #[test]
    fn parsed_mods_use_canonical_names() {
        let item = mk_item(
            "Crimson Jewel",
            &[(
                "10% increased Maximum Life to nearby allocated passives",
                ModSection::Explicit,
            )],
        );
        let jewel = identify_radius_jewel(1, &item).expect("identified");
        assert_eq!(jewel.mods.len(), 1);
        // Stripping the suffix lets the parser mint the canonical `Life` key,
        // not the long-form `MaximumLifeToNearbyAllocatedPassives`.
        assert_eq!(jewel.mods[0].name, "Life");
        assert_eq!(jewel.mods[0].kind, crate::ModType::Inc);
    }

    #[test]
    fn empty_socket_set_is_no_op() {
        let tree = mk_tree();
        let alloc: AHashSet<NodeId> = AHashSet::default();
        let socketed = SocketedJewels::new();
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        assert_eq!(report, RadiusJewelReport::default());
    }

    /// Issue #196: a non-radius unique jewel like Conqueror's Efficiency
    /// (Crimson Jewel base) carries plain global mods — no "to Passives in
    /// Radius" markers. `apply_radius_jewels` correctly skips it. The
    /// fallback `apply_non_radius_socketed_jewels` must pick it up so the
    /// global stats actually land in the modDB.
    #[test]
    fn non_radius_unique_jewel_applies_mods_globally() {
        use crate::mod_db::{EvalState, QueryCfg};
        use crate::{ModStore, ModType};
        // Conqueror's Efficiency mod text — three plain global mods, none
        // mention radius. The Crimson Jewel base is a vanilla jewel base so
        // it survives the subtype gate.
        let item = mk_item(
            "Crimson Jewel",
            &[
                ("4% increased Skill Effect Duration", ModSection::Explicit),
                (
                    "4% increased Mana Reservation Efficiency of Skills",
                    ModSection::Explicit,
                ),
            ],
        );
        let mut socketed = SocketedJewels::new();
        socketed.socket(42, item);
        let mut db = crate::ModDB::default();

        // The radius pass skips this item — applied_jewels stays at 0.
        let tree = mk_tree();
        let alloc: AHashSet<NodeId> = AHashSet::default();
        let radius_report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        assert_eq!(radius_report.applied_jewels, 0);
        assert_eq!(radius_report.skipped, 1);

        // The fallback picks it up and emits both mods globally.
        let emitted = apply_non_radius_socketed_jewels(&socketed, &mut db);
        assert_eq!(emitted, 2);
        let cfg = QueryCfg::default();
        let st = EvalState::default();
        assert_eq!(db.sum(ModType::Inc, &cfg, &st, "SkillEffectDuration"), 4.0);
        assert_eq!(
            db.sum(ModType::Inc, &cfg, &st, "ManaReservationEfficiency"),
            4.0
        );
    }

    /// Issue #196: special-subtype jewels (Cluster, Abyss, Eye, Timeless,
    /// Charm) follow their own dispatch paths. The fallback must NOT
    /// double-apply their item-level mods globally — that would be a
    /// regression for cluster sub-graph synthesis (#21) and the timeless
    /// override path (#30).
    #[test]
    fn non_radius_fallback_skips_special_subtypes() {
        // Cluster jewel with a stat line that, if applied globally, would
        // pollute Damage. Verify the fallback leaves it alone.
        let cluster = mk_item(
            "Large Cluster Jewel",
            &[("Adds 8 Passive Skills", ModSection::Implicit)],
        );
        let mut socketed = SocketedJewels::new();
        socketed.socket(1, cluster);
        let mut db = crate::ModDB::default();
        let emitted = apply_non_radius_socketed_jewels(&socketed, &mut db);
        // Cluster jewel skipped — zero mods emitted.
        assert_eq!(emitted, 0);

        // Same for an abyss eye-jewel base.
        let abyss = mk_item(
            "Searching Eye Jewel",
            &[("+50 to maximum Life", ModSection::Explicit)],
        );
        let mut socketed2 = SocketedJewels::new();
        socketed2.socket(2, abyss);
        let mut db2 = crate::ModDB::default();
        let emitted2 = apply_non_radius_socketed_jewels(&socketed2, &mut db2);
        assert_eq!(emitted2, 0);

        // And for a timeless jewel.
        let timeless = mk_item(
            "Lethal Pride",
            &[(
                "Commanded leadership over 10000 warriors",
                ModSection::Explicit,
            )],
        );
        let mut socketed3 = SocketedJewels::new();
        socketed3.socket(3, timeless);
        let mut db3 = crate::ModDB::default();
        let emitted3 = apply_non_radius_socketed_jewels(&socketed3, &mut db3);
        assert_eq!(emitted3, 0);
    }

    /// Issue #196: a vanilla radius jewel must also be skipped by the
    /// fallback — `apply_radius_jewels` already owns it. Otherwise the
    /// jewel's mods would land twice (once per allocated passive in
    /// radius and once globally), badly inflating the stat.
    #[test]
    fn non_radius_fallback_skips_radius_jewels() {
        use crate::mod_db::{EvalState, QueryCfg};
        use crate::{ModStore, ModType};
        let item = mk_item(
            "Crimson Jewel",
            &[(
                "10% increased Maximum Life to nearby allocated passives",
                ModSection::Explicit,
            )],
        );
        let mut socketed = SocketedJewels::new();
        socketed.socket(1, item);
        let mut db = crate::ModDB::default();
        let emitted = apply_non_radius_socketed_jewels(&socketed, &mut db);
        // identify_radius_jewel returns Some for this item → fallback skips.
        assert_eq!(emitted, 0);
        let cfg = QueryCfg::default();
        let st = EvalState::default();
        assert_eq!(db.sum(ModType::Inc, &cfg, &st, "Life"), 0.0);
    }

    // Suppress "unused-import" lint for the convenience re-export when this
    // module is consumed by callers via the lib.rs facade.
    #[test]
    fn item_set_alias_compiles() {
        let _: ItemSet = ItemSet::new();
    }

    // ---- Issue #196: named-unique handlers ---------------------------------

    /// Watcher's Eye is identified by `item.name`, not by the radius marker —
    /// the unique's mod text is "while affected by <Aura>", not "in Radius".
    #[test]
    fn identify_watchers_eye_routes_to_aura_handler() {
        let item = mk_item_named(
            "Watcher's Eye",
            "Prismatic Jewel",
            &[(
                "40% increased Cold Damage while affected by Hatred",
                ModSection::Explicit,
            )],
        );
        let jewel = identify_radius_jewel(1, &item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::WatchersEye);
        assert_eq!(jewel.mods.len(), 1);
        // The parser stamps `AffectedByHatred` as a Condition tag on the mod.
        assert!(
            jewel.mods[0]
                .tags
                .iter()
                .any(|t| matches!(&t.kind, crate::TagKind::Condition { var, .. } if var == "AffectedByHatred")),
            "expected AffectedByHatred Condition tag on Watcher's Eye mod, got {:?}",
            jewel.mods[0].tags,
        );
    }

    #[test]
    fn watchers_eye_mods_apply_globally_with_condition() {
        let tree = mk_tree();
        let alloc: AHashSet<NodeId> = AHashSet::default(); // no allocations needed
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Watcher's Eye",
                "Prismatic Jewel",
                &[(
                    "40% increased Cold Damage while affected by Hatred",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        assert_eq!(report.applied_jewels, 1);
        // One global emission — the radius scan is bypassed entirely.
        assert_eq!(report.mod_emissions, 1);
        // The mod is in the modDB with its condition tag intact.
        let cold = db.slice_named("ColdDamage");
        assert!(
            cold.iter().any(|m| matches!(m.kind, crate::ModType::Inc)
                && m.tags.iter().any(|t| matches!(&t.kind, crate::TagKind::Condition { var, .. } if var == "AffectedByHatred"))),
            "expected gated Inc ColdDamage mod, got {cold:#?}",
        );
    }

    /// Healthy Mind: in-radius Inc Life mods should produce Inc Mana mods at 2×
    /// scale, sourced as the in-radius node.
    #[test]
    fn healthy_mind_transforms_inc_life_to_inc_mana_double() {
        let tree = mk_tree();
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        // Node 2 is in radius (600 units away from socket at 0,0 with Medium ring 1440).
        // The mock test tree node 2 has stats `["10% increased Life"]`.
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Healthy Mind",
                "Cobalt Jewel",
                &[
                    ("15% increased maximum Mana", ModSection::Explicit),
                    (
                        "Increases and Reductions to Life in Radius are Transformed to apply to Mana at 200% of their value",
                        ModSection::Explicit,
                    ),
                ],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        assert_eq!(report.applied_jewels, 1);
        // Exactly one global Mana mod (the +15% line) plus one transformed
        // Inc Mana mod from node 2's `10% increased Life`.
        assert!(report.mod_emissions >= 2);
        let mana = db.slice_named("Mana");
        // The +15% global Mana mod.
        assert!(
            mana.iter().any(|m| matches!(m.kind, crate::ModType::Inc)
                && (m.value.as_f64().unwrap_or(0.0) - 15.0).abs() < 1e-6),
            "expected global +15% Inc Mana, got {mana:#?}",
        );
        // The transformed mod: 10% Inc Life × 200% = +20% Inc Mana sourced from node 2.
        assert!(
            mana.iter().any(|m| matches!(m.kind, crate::ModType::Inc)
                && matches!(&m.source, Some(Source::Passive(id)) if *id == 2)
                && (m.value.as_f64().unwrap_or(0.0) - 20.0).abs() < 1e-6),
            "expected transformed +20% Inc Mana sourced from Passive(2), got {mana:#?}",
        );
    }

    #[test]
    fn fertile_mind_transforms_dex_base_to_int() {
        // Custom tree where node 2 has a `+30 to Dexterity` BASE stat.
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+30 to Dexterity".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Fertile Mind",
                "Cobalt Jewel",
                &[
                    ("+20 to Intelligence", ModSection::Explicit),
                    (
                        "Dexterity from Passives in Radius is Transformed to Intelligence",
                        ModSection::Explicit,
                    ),
                ],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        assert_eq!(report.applied_jewels, 1);
        // Plain mod (+20 Int) is one emission. Transform of +30 Dex emits two
        // (Int + counter Dex) for at least three emissions total.
        assert!(report.mod_emissions >= 3);
        let int_mods = db.slice_named("Intelligence");
        // +20 global Int
        assert!(
            int_mods
                .iter()
                .any(|m| (m.value.as_f64().unwrap_or(0.0) - 20.0).abs() < 1e-6),
            "expected global +20 Int, got {int_mods:#?}",
        );
        // Transformed +30 Int sourced from node 2.
        assert!(
            int_mods.iter().any(
                |m| matches!(&m.source, Some(Source::Passive(id)) if *id == 2)
                    && (m.value.as_f64().unwrap_or(0.0) - 30.0).abs() < 1e-6
            ),
            "expected +30 Int sourced from Passive(2), got {int_mods:#?}",
        );
        // Counter -30 Dex offsetting the source contribution.
        let dex = db.slice_named("Dexterity");
        assert!(
            dex.iter()
                .any(|m| (m.value.as_f64().unwrap_or(0.0) + 30.0).abs() < 1e-6),
            "expected counter -30 Dex from Fertile Mind, got {dex:#?}",
        );
    }

    /// Issue #196: Pure Talent test scaffold. Build a tree that has both
    /// ClassStart nodes (for Marauder + Witch) and a regular jewel-socket
    /// node so the dispatch can identify the connected class set. The
    /// tree's positions don't matter for this handler — the radius is
    /// pinned to (0, 0) by `build_pure_talent`.
    fn mk_class_start_tree() -> PassiveTree {
        use pob_data::Class;
        let mut groups = ahash::HashMap::default();
        groups.insert(
            10,
            Group {
                x: 0.0,
                y: 0.0,
                orbits: smallvec::smallvec![0],
                background: None,
                nodes: vec![1, 100, 200],
                is_proxy: false,
            },
        );
        let mut nodes = ahash::HashMap::default();
        nodes.insert(
            1,
            Node {
                id: 1,
                name: Some("Jewel Socket".into()),
                icon: None,
                ascendancy_name: None,
                stats: vec![],
                reminder_text: vec![],
                kind: NodeKind::JewelSocket,
                class_start_index: None,
                group: Some(10),
                orbit: Some(0),
                orbit_index: Some(0),
                out_edges: smallvec::smallvec![],
                in_edges: smallvec::smallvec![],
                mastery_effects: vec![],
                expansion_jewel_size: None,
                jewel_radius: None,
            },
        );
        // Class start for Marauder (index 0).
        nodes.insert(
            100,
            Node {
                id: 100,
                name: Some("Marauder Start".into()),
                icon: None,
                ascendancy_name: None,
                stats: vec![],
                reminder_text: vec![],
                kind: NodeKind::ClassStart,
                class_start_index: Some(0),
                group: Some(10),
                orbit: Some(0),
                orbit_index: Some(0),
                out_edges: smallvec::smallvec![],
                in_edges: smallvec::smallvec![],
                mastery_effects: vec![],
                expansion_jewel_size: None,
                jewel_radius: None,
            },
        );
        // Class start for Witch (index 1).
        nodes.insert(
            200,
            Node {
                id: 200,
                name: Some("Witch Start".into()),
                icon: None,
                ascendancy_name: None,
                stats: vec![],
                reminder_text: vec![],
                kind: NodeKind::ClassStart,
                class_start_index: Some(1),
                group: Some(10),
                orbit: Some(0),
                orbit_index: Some(0),
                out_edges: smallvec::smallvec![],
                in_edges: smallvec::smallvec![],
                mastery_effects: vec![],
                expansion_jewel_size: None,
                jewel_radius: None,
            },
        );
        PassiveTree {
            version: "3_25".into(),
            tree: "Default".into(),
            classes: vec![
                Class {
                    name: "Marauder".into(),
                    base_str: 32,
                    base_dex: 14,
                    base_int: 14,
                    ascendancies: vec![],
                },
                Class {
                    name: "Witch".into(),
                    base_str: 14,
                    base_dex: 14,
                    base_int: 32,
                    ascendancies: vec![],
                },
            ],
            groups,
            nodes,
            jewel_slots: vec![1],
            min_x: -100,
            min_y: -100,
            max_x: 100,
            max_y: 100,
            constants: TreeConstants {
                skills_per_orbit: vec![1, 6, 16, 16, 40, 72, 72],
                orbit_radii: vec![0, 82, 162, 335, 493, 662, 846],
                classes: ahash::HashMap::default(),
                character_attributes: ahash::HashMap::default(),
                pss_centre_inner_radius: None,
            },
            points: pob_data::TreePoints::default(),
        }
    }

    /// Issue #196: a Pure Talent socketed by a Marauder (own class) only
    /// emits the `Marauder:` line — the other six class lines are gated.
    /// Verify the Marauder bonus lands as `AreaOfEffect Inc 25`.
    #[test]
    fn pure_talent_emits_only_player_class_line_by_default() {
        use crate::{ModStore as _, ModType};
        let tree = mk_class_start_tree();
        // Marauder allocation only — no other class start node allocated.
        let alloc: AHashSet<NodeId> = AHashSet::default();
        let item = mk_item_named(
            "Pure Talent",
            "Viridian Jewel",
            &[
                (
                    "Marauder: Melee Skills have 25% increased Area of Effect",
                    ModSection::Explicit,
                ),
                (
                    "Duelist: 1% of Attack Damage Leeched as Life",
                    ModSection::Explicit,
                ),
                ("Ranger: 7% increased Movement Speed", ModSection::Explicit),
                (
                    "Witch: 0.5% of Mana Regenerated per second",
                    ModSection::Explicit,
                ),
            ],
        );
        let mut socketed = SocketedJewels::new();
        socketed.socket(1, item);
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        assert_eq!(report.applied_jewels, 1);
        // Only the Marauder line emits.
        assert_eq!(report.mod_emissions, 1);
        // Walk the modDB looking for the emitted mod. The Marauder line
        // parses as `AreaOfEffect Inc 25` with a `melee` flag — both flag
        // and name are inspected by the assertion so future parser
        // changes that rename the stat surface here.
        let mut found = false;
        for m in db.iter_all() {
            if m.kind == ModType::Inc && m.name == "AreaOfEffect" {
                found = true;
                assert!(matches!(m.value, crate::ModValue::Number(v) if (v - 25.0).abs() < 0.001));
            }
        }
        assert!(
            found,
            "expected an AreaOfEffect Inc mod from the Marauder line"
        );
        // None of the gated classes' bonuses landed: Witch's "Mana
        // Regenerated per second" would target ManaRegen if it had landed.
        let mut witch_found = false;
        for m in db.iter_all() {
            if m.name == "ManaRegen" {
                witch_found = true;
            }
        }
        assert!(
            !witch_found,
            "Witch line should be gated when Marauder is the player class"
        );
    }

    /// Issue #196: when the player's tree allocates a non-own ClassStart
    /// (e.g. a Marauder pathing into the Witch start), Pure Talent grants
    /// that other class's bonus too. Verify Witch's `Mana Regenerated per
    /// second` bonus lands when node 200 (Witch start) is in `allocated`.
    #[test]
    fn pure_talent_emits_other_class_when_class_start_allocated() {
        let tree = mk_class_start_tree();
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(200); // Witch start allocated.
        let item = mk_item_named(
            "Pure Talent",
            "Viridian Jewel",
            &[
                (
                    "Marauder: Melee Skills have 25% increased Area of Effect",
                    ModSection::Explicit,
                ),
                (
                    "Witch: 0.5% of Mana Regenerated per second",
                    ModSection::Explicit,
                ),
            ],
        );
        let mut socketed = SocketedJewels::new();
        socketed.socket(1, item);
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        // Both Marauder (own) and Witch (path-connected) emit.
        assert_eq!(report.mod_emissions, 2);
    }

    /// Issue #196: Replica Pure Talent uses the same handler. A non-jewel
    /// item with the Pure Talent name should still be ignored — the
    /// identifier checks `is_jewel_base` first.
    #[test]
    fn replica_pure_talent_uses_same_handler() {
        let tree = mk_class_start_tree();
        let alloc: AHashSet<NodeId> = AHashSet::default();
        let item = mk_item_named(
            "Replica Pure Talent",
            "Viridian Jewel",
            &[(
                "Marauder: Melee Skills have 25% increased Area of Effect",
                ModSection::Explicit,
            )],
        );
        let mut socketed = SocketedJewels::new();
        socketed.socket(1, item);
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        assert_eq!(report.applied_jewels, 1);
        assert_eq!(report.mod_emissions, 1);
    }

    /// Issue #196: `Limited to: 1` and other non-class metadata lines on
    /// Pure Talent must be silently dropped — they're informational, not
    /// stat mods.
    #[test]
    fn pure_talent_drops_metadata_lines() {
        let tree = mk_class_start_tree();
        let alloc: AHashSet<NodeId> = AHashSet::default();
        let item = mk_item_named(
            "Pure Talent",
            "Viridian Jewel",
            &[
                ("Limited to: 1", ModSection::Explicit),
                (
                    "Marauder: Melee Skills have 25% increased Area of Effect",
                    ModSection::Explicit,
                ),
            ],
        );
        let mut socketed = SocketedJewels::new();
        socketed.socket(1, item);
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        // One mod (Marauder), no error from the `Limited to: 1` line.
        assert_eq!(report.mod_emissions, 1);
    }

    /// Issue #196: connected_classes computes the same set the dispatch
    /// uses. Sanity-check the helper directly so a future refactor that
    /// changes the call shape can't silently break the trigger logic.
    #[test]
    fn pure_talent_connected_classes_resolves_player_and_allocated_starts() {
        let tree = mk_class_start_tree();
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        // Player class only.
        let connected = pure_talent_connected_classes("Marauder", &tree, &alloc);
        assert!(connected.contains("Marauder"));
        assert!(!connected.contains("Witch"));

        // Plus an allocated Witch start.
        alloc.insert(200);
        let connected = pure_talent_connected_classes("Marauder", &tree, &alloc);
        assert!(connected.contains("Marauder"));
        assert!(connected.contains("Witch"));
        assert_eq!(connected.len(), 2);
    }

    /// Issue #196 (slice 15): Fireborn — `Increases and Reductions
    /// to other Damage Types in Radius are Transformed to apply to
    /// Fire Damage`. Same shape as Cold Steel but four source
    /// directions (Phys / Cold / Lightning / Chaos) all rolling into
    /// Fire at 1× scale, additively. Each in-radius node's Inc on
    /// any non-Fire damage type contributes to Inc Fire.
    #[test]
    fn fireborn_transforms_other_damage_inc_to_fire() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec![
            "20% increased Physical Damage".into(),
            "15% increased Cold Damage".into(),
            "10% increased Lightning Damage".into(),
            "5% increased Chaos Damage".into(),
        ];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Fireborn",
                "Crimson Jewel",
                &[(
                    "Increases and Reductions to other Damage Types in Radius are Transformed to apply to Fire Damage",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        assert_eq!(report.applied_jewels, 1);

        let item = socketed.get(1).unwrap();
        let jewel = identify_radius_jewel(1, item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::OtherDamageToFireTransform);

        let fire_mods = db.slice_named("FireDamage");
        for (src_value, src_label) in [
            (20.0, "Phys"),
            (15.0, "Cold"),
            (10.0, "Lightning"),
            (5.0, "Chaos"),
        ] {
            assert!(
                fire_mods.iter().any(|m| matches!(m.kind, crate::ModType::Inc)
                    && matches!(&m.source, Some(Source::Passive(id)) if *id == 2)
                    && (m.value.as_f64().unwrap_or(0.0) - src_value).abs() < 1e-6),
                "expected +{src_value}% Inc Fire sourced from Passive(2) (from {src_label}), got {fire_mods:#?}",
            );
        }
    }

    /// Issue #196 (slice 15): Fireborn out-of-radius node (3 in
    /// `mk_tree`) carrying Inc Phys must not be transformed.
    #[test]
    fn fireborn_does_not_transform_out_of_radius_nodes() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec![];
        tree.nodes.get_mut(&3).unwrap().stats = vec!["20% increased Physical Damage".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        alloc.insert(3);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Fireborn",
                "Crimson Jewel",
                &[(
                    "Increases and Reductions to other Damage Types in Radius are Transformed to apply to Fire Damage",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);

        let fire_mods = db.slice_named("FireDamage");
        assert!(
            !fire_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Inc)
                    && matches!(&m.source, Some(Source::Passive(id)) if *id == 3)),
            "out-of-radius Inc Phys must not be transformed, got {fire_mods:#?}",
        );
    }

    /// Issue #196 (slice 7): Cold Steel — Phys ↔ Cold Inc
    /// double-transform. The jewel carries two marker lines that
    /// transform Inc PhysicalDamage to Inc ColdDamage AND Inc
    /// ColdDamage to Inc PhysicalDamage at 1×, applied additively
    /// (the original mods stay) so each in-radius node ends up
    /// contributing to both damage types.
    #[test]
    fn cold_steel_transforms_phys_and_cold_inc_in_both_directions() {
        let mut tree = mk_tree();
        // Node 2 has both Inc Phys and Inc Cold for the test to pin
        // both transform directions.
        tree.nodes.get_mut(&2).unwrap().stats = vec![
            "20% increased Physical Damage".into(),
            "30% increased Cold Damage".into(),
        ];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Cold Steel",
                "Viridian Jewel",
                &[
                    (
                        "Increases and Reductions to Physical Damage in Radius are Transformed to apply to Cold Damage",
                        ModSection::Explicit,
                    ),
                    (
                        "Increases and Reductions to Cold Damage in Radius are Transformed to apply to Physical Damage",
                        ModSection::Explicit,
                    ),
                ],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        assert_eq!(report.applied_jewels, 1);

        let item = socketed.get(1).unwrap();
        let jewel = identify_radius_jewel(1, item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::PhysColdSwapTransform);

        // Phys → Cold: +30% Cold sourced from Phys, +20% Cold sourced
        // from the Cold Damage line is the source of the +20% Cold Inc
        // mod from passive 2 — wait, that's the Phys side.
        // Direction 1 (Phys → Cold): Inc Phys 20% on node 2 → Inc Cold 20% sourced from node 2.
        let cold_mods = db.slice_named("ColdDamage");
        assert!(
            cold_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Inc)
                    && matches!(&m.source, Some(Source::Passive(id)) if *id == 2)
                    && (m.value.as_f64().unwrap_or(0.0) - 20.0).abs() < 1e-6),
            "expected +20% Cold Inc sourced from Passive(2) (from Phys), got {cold_mods:#?}",
        );
        // Direction 2 (Cold → Phys): Inc Cold 30% on node 2 → Inc Phys 30% sourced from node 2.
        let phys_mods = db.slice_named("PhysicalDamage");
        assert!(
            phys_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Inc)
                    && matches!(&m.source, Some(Source::Passive(id)) if *id == 2)
                    && (m.value.as_f64().unwrap_or(0.0) - 30.0).abs() < 1e-6),
            "expected +30% Phys Inc sourced from Passive(2) (from Cold), got {phys_mods:#?}",
        );
    }

    /// Issue #196 (slice 7): Cold Steel out-of-radius node (3 in
    /// `mk_tree`) carrying Inc Phys / Inc Cold mods must not be
    /// transformed in either direction.
    #[test]
    fn cold_steel_does_not_transform_out_of_radius_nodes() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec![];
        tree.nodes.get_mut(&3).unwrap().stats = vec!["20% increased Physical Damage".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(3);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Cold Steel",
                "Viridian Jewel",
                &[
                    (
                        "Increases and Reductions to Physical Damage in Radius are Transformed to apply to Cold Damage",
                        ModSection::Explicit,
                    ),
                    (
                        "Increases and Reductions to Cold Damage in Radius are Transformed to apply to Physical Damage",
                        ModSection::Explicit,
                    ),
                ],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);

        let cold_mods = db.slice_named("ColdDamage");
        assert!(
            !cold_mods
                .iter()
                .any(|m| matches!(&m.source, Some(Source::Passive(id)) if *id == 3)),
            "Cold Steel should not transform out-of-radius nodes, got {cold_mods:#?}",
        );
    }

    /// Issue #196 (slice 8): Anatomical Knowledge — `Adds 1 to
    /// Issue #196 (slice 9): Pugilist — `1% increased Evasion Rating
    /// per 3 Dexterity Allocated in Radius`. Sister to Anatomical
    /// Knowledge but Dex-sourced and emits Inc Evasion. The two
    /// per-claw / per-unarmed mods on the same jewel need
    /// weapon-condition tags and follow as a future slice.
    #[test]
    fn pugilist_emits_inc_evasion_per_three_dex_in_radius() {
        let mut tree = mk_tree();
        // Sum = 30 Dex → +10% Inc Evasion.
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+30 to Dexterity".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Pugilist",
                "Viridian Jewel",
                &[(
                    "1% increased Evasion Rating per 3 Dexterity Allocated in Radius",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Ranger", &mut db);
        assert_eq!(report.applied_jewels, 1);

        let item = socketed.get(1).unwrap();
        let jewel = identify_radius_jewel(1, item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::DexCountToIncEvasion);

        let evasion_mods = db.slice_named("Evasion");
        assert!(
            evasion_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Inc)
                    && (m.value.as_f64().unwrap_or(0.0) - 10.0).abs() < 1e-6),
            "expected +10% Inc Evasion from Pugilist, got {evasion_mods:#?}",
        );
    }

    /// Issue #196 (slice 14): Pugilist's per-claw line — `1%
    /// increased Claw Physical Damage per 3 Dexterity Allocated in
    /// Radius`. Same dex-count source as the Evasion line; emits a
    /// CLAW-flagged Inc PhysicalDamage so it only applies when
    /// wielding a claw. Sum = 30 Dex → +10% Inc PhysicalDamage with
    /// the CLAW modflag.
    #[test]
    fn pugilist_emits_claw_phys_per_three_dex_in_radius() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+30 to Dexterity".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Pugilist",
                "Viridian Jewel",
                &[
                    (
                        "1% increased Evasion Rating per 3 Dexterity Allocated in Radius",
                        ModSection::Explicit,
                    ),
                    (
                        "1% increased Claw Physical Damage per 3 Dexterity Allocated in Radius",
                        ModSection::Explicit,
                    ),
                ],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Ranger", &mut db);

        let phys_mods = db.slice_named("PhysicalDamage");
        assert!(
            phys_mods.iter().any(|m| {
                matches!(m.kind, crate::ModType::Inc)
                    && (m.value.as_f64().unwrap_or(0.0) - 10.0).abs() < 1e-6
                    && m.flags.contains(pob_data::ModFlag::CLAW)
            }),
            "expected CLAW-flagged +10% Inc PhysicalDamage from Pugilist, got {phys_mods:#?}",
        );
    }

    /// Issue #196 (slice 14): the Evasion line still works when both
    /// lines are present on the jewel — wider marker must not regress
    /// the Inc Evasion emission.
    #[test]
    fn pugilist_emits_both_evasion_and_claw_phys_when_both_lines_present() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+30 to Dexterity".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Pugilist",
                "Viridian Jewel",
                &[
                    (
                        "1% increased Evasion Rating per 3 Dexterity Allocated in Radius",
                        ModSection::Explicit,
                    ),
                    (
                        "1% increased Claw Physical Damage per 3 Dexterity Allocated in Radius",
                        ModSection::Explicit,
                    ),
                ],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Ranger", &mut db);

        let evasion_mods = db.slice_named("Evasion");
        assert!(
            evasion_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Inc)
                    && (m.value.as_f64().unwrap_or(0.0) - 10.0).abs() < 1e-6),
            "evasion regression — expected +10% Inc Evasion from Pugilist, got {evasion_mods:#?}",
        );
    }

    /// Issue #196 (slice 14): the per-claw line's parsed-mod form
    /// must NOT also land globally as a flat 1% Inc Phys (the radius
    /// dispatch owns the scaled emission). Without the wider marker
    /// the line falls through to vanilla `mod_parser`, which would
    /// emit an unscaled +1% globally.
    #[test]
    fn pugilist_claw_line_is_not_double_applied() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+30 to Dexterity".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Pugilist",
                "Viridian Jewel",
                &[(
                    "1% increased Claw Physical Damage per 3 Dexterity Allocated in Radius",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Ranger", &mut db);

        // Exactly one Inc PhysicalDamage with CLAW flag (the radius
        // emission), and no spurious unscaled +1% globally.
        let phys_inc: Vec<_> = db
            .slice_named("PhysicalDamage")
            .iter()
            .filter(|m| matches!(m.kind, crate::ModType::Inc))
            .collect();
        assert_eq!(
            phys_inc.len(),
            1,
            "expected exactly one Inc PhysicalDamage from Pugilist's claw line, got {phys_inc:#?}",
        );
    }

    /// Issue #196: Pugilist's per-unarmed line — `1% increased Melee
    /// Physical Damage with Unarmed Attacks per 3 Dexterity Allocated
    /// in Radius`. Same dex-count source as the Evasion / Claw lines;
    /// emits a MELEE+UNARMED-flagged Inc PhysicalDamage so it only
    /// applies to unarmed melee attacks (mirrors PoB ModParser.lua
    /// line 6155). Sum = 30 Dex → +10% Inc PhysicalDamage with the
    /// MELEE+UNARMED modflags.
    #[test]
    fn pugilist_emits_unarmed_melee_phys_per_three_dex_in_radius() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+30 to Dexterity".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Pugilist",
                "Viridian Jewel",
                &[(
                    "1% increased Melee Physical Damage with Unarmed Attacks per 3 Dexterity Allocated in Radius",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Ranger", &mut db);

        let phys_mods = db.slice_named("PhysicalDamage");
        assert!(
            phys_mods.iter().any(|m| {
                matches!(m.kind, crate::ModType::Inc)
                    && (m.value.as_f64().unwrap_or(0.0) - 10.0).abs() < 1e-6
                    && m.flags.contains(pob_data::ModFlag::MELEE)
                    && m.flags.contains(pob_data::ModFlag::UNARMED)
            }),
            "expected MELEE+UNARMED-flagged +10% Inc PhysicalDamage from Pugilist, got {phys_mods:#?}",
        );
    }

    /// Issue #196: when all three Pugilist per-Dex lines are present,
    /// each emits the scaled value once (Evasion + Claw-flagged Phys
    /// + Unarmed/Melee-flagged Phys). 30 Dex → +10% on each.
    #[test]
    fn pugilist_emits_all_three_lines_when_all_present() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+30 to Dexterity".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Pugilist",
                "Viridian Jewel",
                &[
                    (
                        "1% increased Evasion Rating per 3 Dexterity Allocated in Radius",
                        ModSection::Explicit,
                    ),
                    (
                        "1% increased Claw Physical Damage per 3 Dexterity Allocated in Radius",
                        ModSection::Explicit,
                    ),
                    (
                        "1% increased Melee Physical Damage with Unarmed Attacks per 3 Dexterity Allocated in Radius",
                        ModSection::Explicit,
                    ),
                ],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Ranger", &mut db);

        let evasion_mods = db.slice_named("Evasion");
        assert!(
            evasion_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Inc)
                    && (m.value.as_f64().unwrap_or(0.0) - 10.0).abs() < 1e-6),
            "expected +10% Inc Evasion from Pugilist, got {evasion_mods:#?}",
        );

        let phys_inc: Vec<_> = db
            .slice_named("PhysicalDamage")
            .iter()
            .filter(|m| matches!(m.kind, crate::ModType::Inc))
            .cloned()
            .collect();
        // Exactly two Inc PhysicalDamage emissions: one CLAW-flagged,
        // one MELEE+UNARMED-flagged. Both at +10% from the 30-Dex sum.
        assert_eq!(
            phys_inc.len(),
            2,
            "expected exactly two Inc PhysicalDamage from Pugilist's two phys lines, got {phys_inc:#?}",
        );
        assert!(
            phys_inc.iter().any(|m| {
                m.flags.contains(pob_data::ModFlag::CLAW)
                    && (m.value.as_f64().unwrap_or(0.0) - 10.0).abs() < 1e-6
            }),
            "expected CLAW-flagged +10% Inc PhysicalDamage, got {phys_inc:#?}",
        );
        assert!(
            phys_inc.iter().any(|m| {
                m.flags.contains(pob_data::ModFlag::MELEE)
                    && m.flags.contains(pob_data::ModFlag::UNARMED)
                    && (m.value.as_f64().unwrap_or(0.0) - 10.0).abs() < 1e-6
            }),
            "expected MELEE+UNARMED-flagged +10% Inc PhysicalDamage, got {phys_inc:#?}",
        );
    }

    /// Issue #196 (slice 9): integer-divide. 8 Dex → 2% Inc Evasion
    /// (floor(8/3)).
    #[test]
    fn pugilist_integer_divides_dex_sum() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+8 to Dexterity".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Pugilist",
                "Viridian Jewel",
                &[(
                    "1% increased Evasion Rating per 3 Dexterity Allocated in Radius",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Ranger", &mut db);

        let evasion_mods = db.slice_named("Evasion");
        assert!(
            evasion_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Inc)
                    && (m.value.as_f64().unwrap_or(0.0) - 2.0).abs() < 1e-6),
            "expected +2% Inc Evasion (floor(8/3)), got {evasion_mods:#?}",
        );
    }

    /// Issue #196 (slice 12): Static Electricity. `Adds 1 maximum
    /// Lightning Damage to Attacks per 1 Dexterity Allocated in
    /// Radius`. Sums in-radius allocated Dex and emits a single
    /// `LightningDamage` BASE mod with `ModValue::Range { 0, dex }`,
    /// `ATTACK` flag, and `LIGHTNING` keyword — the same shape the
    /// `Adds N to M Lightning Damage to Attacks` parser emits, which
    /// `perform.rs`'s flat-damage loop already reads. The jewel's
    /// static `Adds 1 to 2 Lightning Damage to Attacks` line still
    /// applies through vanilla mod_parser.
    #[test]
    fn static_electricity_emits_max_lightning_per_dex_in_radius() {
        let mut tree = mk_tree();
        // Sum = 25 Dex → +0-25 max Lightning to Attacks.
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+25 to Dexterity".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Static Electricity",
                "Viridian Jewel",
                &[
                    ("Adds 1 to 2 Lightning Damage to Attacks", ModSection::Explicit),
                    (
                        "Adds 1 maximum Lightning Damage to Attacks per 1 Dexterity Allocated in Radius",
                        ModSection::Explicit,
                    ),
                ],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Ranger", &mut db);
        assert_eq!(report.applied_jewels, 1);

        let item = socketed.get(1).unwrap();
        let jewel = identify_radius_jewel(1, item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::DexCountToMaxLightningAttack);

        let lightning_mods = db.slice_named("LightningDamage");
        assert!(
            lightning_mods.iter().any(|m| {
                matches!(m.kind, crate::ModType::Base)
                    && match m.value.as_range() {
                        Some((min, max)) => (min - 0.0).abs() < 1e-6 && (max - 25.0).abs() < 1e-6,
                        None => false,
                    }
            }),
            "expected Base LightningDamage Range[0,25] from Static Electricity, got {lightning_mods:#?}",
        );
    }

    /// Issue #196 (slice 12): the emitted mod must carry the ATTACK
    /// modflag so `perform.rs` only adds it on attacks (not spells).
    #[test]
    fn static_electricity_emits_attack_flagged_mod() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+10 to Dexterity".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Static Electricity",
                "Viridian Jewel",
                &[(
                    "Adds 1 maximum Lightning Damage to Attacks per 1 Dexterity Allocated in Radius",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Ranger", &mut db);

        let lightning_mods = db.slice_named("LightningDamage");
        assert!(
            lightning_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Base)
                    && m.flags.contains(pob_data::ModFlag::ATTACK)),
            "expected ATTACK-flagged Base LightningDamage, got {lightning_mods:#?}",
        );
    }

    /// Issue #196 (slice 11): Eldritch Knowledge. `5% increased
    /// Chaos Damage per 10 Intelligence from Allocated Passives in
    /// Radius`. Sister to Spire of Stone but Int-sourced and emits
    /// Inc ChaosDamage. Sum = 50 Int → floor(50/10) × 5 = 25% Inc
    /// ChaosDamage.
    #[test]
    fn eldritch_knowledge_emits_inc_chaos_per_ten_int_in_radius() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+50 to Intelligence".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Eldritch Knowledge",
                "Cobalt Jewel",
                &[(
                    "5% increased Chaos Damage per 10 Intelligence from Allocated Passives in Radius",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Witch", &mut db);
        assert_eq!(report.applied_jewels, 1);

        let item = socketed.get(1).unwrap();
        let jewel = identify_radius_jewel(1, item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::IntCountToIncChaosDamage);

        let chaos_mods = db.slice_named("ChaosDamage");
        assert!(
            chaos_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Inc)
                    && (m.value.as_f64().unwrap_or(0.0) - 25.0).abs() < 1e-6),
            "expected +25% Inc ChaosDamage from Eldritch Knowledge, got {chaos_mods:#?}",
        );
    }

    /// Issue #196 (slice 11): integer-divide on Int sum, then × 5.
    /// 27 Int → floor(27/10) × 5 = 10% (not 13.5%).
    #[test]
    fn eldritch_knowledge_integer_divides_int_sum() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+27 to Intelligence".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Eldritch Knowledge",
                "Cobalt Jewel",
                &[(
                    "5% increased Chaos Damage per 10 Intelligence from Allocated Passives in Radius",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Witch", &mut db);

        let chaos_mods = db.slice_named("ChaosDamage");
        assert!(
            chaos_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Inc)
                    && (m.value.as_f64().unwrap_or(0.0) - 10.0).abs() < 1e-6),
            "expected +10% Inc ChaosDamage (floor(27/10) × 5), got {chaos_mods:#?}",
        );
    }

    /// Issue #196 (slice 10): Spire of Stone. `3% increased Totem
    /// Life per 10 Strength Allocated in Radius`. Sister handler to
    /// Pugilist / Anatomical Knowledge but Str-sourced, divides by
    /// 10 (not 3), and multiplies the integer count by 3 before
    /// emitting an Inc TotemLife mod. Sum = 50 Str → floor(50/10) ×
    /// 3 = 15% Inc TotemLife.
    #[test]
    fn spire_of_stone_emits_inc_totem_life_per_ten_str_in_radius() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+50 to Strength".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Spire of Stone",
                "Crimson Jewel",
                &[(
                    "3% increased Totem Life per 10 Strength Allocated in Radius",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        assert_eq!(report.applied_jewels, 1);

        let item = socketed.get(1).unwrap();
        let jewel = identify_radius_jewel(1, item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::StrCountToIncTotemLife);

        let totem_life_mods = db.slice_named("TotemLife");
        assert!(
            totem_life_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Inc)
                    && (m.value.as_f64().unwrap_or(0.0) - 15.0).abs() < 1e-6),
            "expected +15% Inc TotemLife from Spire of Stone, got {totem_life_mods:#?}",
        );
    }

    /// Issue #196 (slice 10): integer-divide on Str sum, then × 3.
    /// 27 Str → floor(27/10) × 3 = 6% (not 8.1%).
    #[test]
    fn spire_of_stone_integer_divides_str_sum() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+27 to Strength".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Spire of Stone",
                "Crimson Jewel",
                &[(
                    "3% increased Totem Life per 10 Strength Allocated in Radius",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);

        let totem_life_mods = db.slice_named("TotemLife");
        assert!(
            totem_life_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Inc)
                    && (m.value.as_f64().unwrap_or(0.0) - 6.0).abs() < 1e-6),
            "expected +6% Inc TotemLife (floor(27/10) × 3), got {totem_life_mods:#?}",
        );
    }

    /// Maximum Life per 3 Intelligence Allocated in Radius`. Sums
    /// `+N Int` BASE mods across in-radius allocated nodes,
    /// integer-divides by 3, and emits a single `+(N/3) Life` BASE
    /// sourced as the jewel.
    #[test]
    fn anatomical_knowledge_emits_life_per_three_int_in_radius() {
        let mut tree = mk_tree();
        // Two in-radius nodes contributing Int. Sum = 30 → 10 Life.
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+30 to Intelligence".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Anatomical Knowledge",
                "Cobalt Jewel",
                &[
                    ("(6-8)% increased maximum Life", ModSection::Explicit),
                    (
                        "Adds 1 to Maximum Life per 3 Intelligence Allocated in Radius",
                        ModSection::Explicit,
                    ),
                ],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Witch", &mut db);
        assert_eq!(report.applied_jewels, 1);

        let item = socketed.get(1).unwrap();
        let jewel = identify_radius_jewel(1, item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::IntCountToLife);

        // +10 Life BASE sourced as the jewel (30 / 3 = 10).
        let life_mods = db.slice_named("Life");
        assert!(
            life_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Base)
                    && (m.value.as_f64().unwrap_or(0.0) - 10.0).abs() < 1e-6),
            "expected +10 Life BASE from Anatomical Knowledge, got {life_mods:#?}",
        );
    }

    /// Issue #196 (slice 8): integer-divides — only multiples of 3
    /// produce Life. Sum = 8 Int → 2 Life (not 2.67), per PoB's
    /// "per 3" semantics.
    #[test]
    fn anatomical_knowledge_integer_divides_int_sum() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+8 to Intelligence".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Anatomical Knowledge",
                "Cobalt Jewel",
                &[(
                    "Adds 1 to Maximum Life per 3 Intelligence Allocated in Radius",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Witch", &mut db);

        let life_mods = db.slice_named("Life");
        assert!(
            life_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Base)
                    && (m.value.as_f64().unwrap_or(0.0) - 2.0).abs() < 1e-6),
            "expected +2 Life BASE (floor(8/3)), got {life_mods:#?}",
        );
    }

    /// Issue #196 (slice 6): Energised Armour — `Increases and
    /// Reductions to Energy Shield in Radius are Transformed to apply
    /// to Armour at 200% of their value`. Mirror of Healthy Mind's
    /// Life→Mana@2× pattern: Inc transform with the doubling factor
    /// PoB applies on the destination side.
    #[test]
    fn energised_armour_transforms_inc_es_to_inc_armour_at_2x() {
        let mut tree = mk_tree();
        // +30% Inc EnergyShield on the in-radius node should produce
        // +60% Inc Armour sourced from the same node (2× scale).
        tree.nodes.get_mut(&2).unwrap().stats = vec!["30% increased maximum Energy Shield".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Energised Armour",
                "Crimson Jewel",
                &[
                    ("(15-20)% increased Armour", ModSection::Explicit),
                    (
                        "Increases and Reductions to Energy Shield in Radius are Transformed to apply to Armour at 200% of their value",
                        ModSection::Explicit,
                    ),
                ],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        assert_eq!(report.applied_jewels, 1);

        let item = socketed.get(1).unwrap();
        let jewel = identify_radius_jewel(1, item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::EnergyShieldToArmourTransform);

        // Doubled +60% Inc Armour sourced from node 2.
        let armour_mods = db.slice_named("Armour");
        assert!(
            armour_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Inc)
                    && matches!(&m.source, Some(Source::Passive(id)) if *id == 2)
                    && (m.value.as_f64().unwrap_or(0.0) - 60.0).abs() < 1e-6),
            "expected +60% Armour Inc sourced from Passive(2), got {armour_mods:#?}",
        );
    }

    /// Issue #196 (slice 6): out-of-radius node (3 in `mk_tree`)
    /// carrying an Inc ES mod must not be transformed. Pin node 2 to
    /// no ES so any Inc Armour sourced from a passive can only have
    /// come from node 3.
    #[test]
    fn energised_armour_does_not_transform_out_of_radius_inc_es() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec![];
        tree.nodes.get_mut(&3).unwrap().stats = vec!["30% increased maximum Energy Shield".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(3);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Energised Armour",
                "Crimson Jewel",
                &[(
                    "Increases and Reductions to Energy Shield in Radius are Transformed to apply to Armour at 200% of their value",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);

        let armour_mods = db.slice_named("Armour");
        assert!(
            !armour_mods
                .iter()
                .any(|m| matches!(&m.source, Some(Source::Passive(id)) if *id == 3)),
            "Energised Armour should not transform out-of-radius nodes, got {armour_mods:#?}",
        );
    }

    /// Issue #196 (slice 3): Energy from Within — `Increases and
    /// Reductions to Life in Radius are Transformed to apply to
    /// Energy Shield`. Pattern parallels Healthy Mind (Life→Mana at
    /// 200%) but targets EnergyShield at 100% — PoB doesn't double
    /// the scale for ES. The plain `(15-20)% increased Energy Shield`
    /// the jewel rolls still applies globally as a vanilla bonus mod.
    #[test]
    fn energy_from_within_transforms_inc_life_to_inc_energy_shield_at_1x() {
        let mut tree = mk_tree();
        // Replace node 2's stats with a +20% increased Life mod so the
        // transform has something to chew on.
        tree.nodes.get_mut(&2).unwrap().stats = vec!["20% increased maximum Life".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Energy From Within",
                "Cobalt Jewel",
                &[
                    ("(15-20)% increased maximum Energy Shield", ModSection::Explicit),
                    (
                        "Increases and Reductions to Life in Radius are Transformed to apply to Energy Shield",
                        ModSection::Explicit,
                    ),
                ],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Witch", &mut db);
        assert_eq!(report.applied_jewels, 1);

        // Dispatch routed to the dedicated handler.
        let item = socketed.get(1).unwrap();
        let jewel = identify_radius_jewel(1, item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::LifeToEnergyShieldTransform);

        // The +20% Life Inc on node 2 should appear as +20% EnergyShield
        // Inc sourced from Passive(2). 1× scale (no doubling) — that's
        // the structural difference vs Healthy Mind.
        let es_mods = db.slice_named("EnergyShield");
        assert!(
            es_mods.iter().any(|m| matches!(m.kind, crate::ModType::Inc)
                && matches!(&m.source, Some(Source::Passive(id)) if *id == 2)
                && (m.value.as_f64().unwrap_or(0.0) - 20.0).abs() < 1e-6),
            "expected +20% EnergyShield Inc sourced from Passive(2), got {es_mods:#?}",
        );
    }

    /// Issue #196 (slice 3): the transform fires only on in-radius
    /// allocated nodes. Node 3 (out of radius in `mk_tree`) carrying a
    /// `+20% Life` Inc must not produce a corresponding EnergyShield Inc.
    #[test]
    fn energy_from_within_does_not_transform_out_of_radius_inc_life() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec![];
        tree.nodes.get_mut(&3).unwrap().stats = vec!["20% increased maximum Life".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(3);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Energy From Within",
                "Cobalt Jewel",
                &[(
                    "Increases and Reductions to Life in Radius are Transformed to apply to Energy Shield",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Witch", &mut db);

        let es_mods = db.slice_named("EnergyShield");
        assert!(
            !es_mods
                .iter()
                .any(|m| matches!(&m.source, Some(Source::Passive(id)) if *id == 3)),
            "Energy from Within should not transform out-of-radius nodes, got {es_mods:#?}",
        );
    }

    /// Issue #196: Karui Heart — `Strength from Passives in Radius is
    /// Transformed to Life`. Verifies the dispatch routes the unique to the
    /// dedicated [`HandlerKind::StrToLifeTransform`] handler and that the
    /// in-radius `+N Strength` BASE mod becomes `+5N Life` BASE sourced as
    /// Issue #196 (slice 5): Brute Force Solution —
    /// `Strength from Passives in Radius is Transformed to Intelligence`.
    /// Same shape as Fertile Mind / Inertia: BASE attribute transform
    /// at 1× scale with a counter mod cancelling the source attribute.
    #[test]
    fn brute_force_solution_transforms_str_base_to_int_base() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+30 to Strength".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Brute Force Solution",
                "Cobalt Jewel",
                &[
                    ("+20 to Intelligence", ModSection::Explicit),
                    (
                        "Strength from Passives in Radius is Transformed to Intelligence",
                        ModSection::Explicit,
                    ),
                ],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Witch", &mut db);
        assert_eq!(report.applied_jewels, 1);

        let item = socketed.get(1).unwrap();
        let jewel = identify_radius_jewel(1, item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::StrToIntTransform);

        let int_mods = db.slice_named("Intelligence");
        assert!(
            int_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Base)
                    && matches!(&m.source, Some(Source::Passive(id)) if *id == 2)
                    && (m.value.as_f64().unwrap_or(0.0) - 30.0).abs() < 1e-6),
            "expected +30 Int BASE sourced from Passive(2), got {int_mods:#?}",
        );
        let str_mods = db.slice_named("Strength");
        assert!(
            str_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Base)
                    && (m.value.as_f64().unwrap_or(0.0) + 30.0).abs() < 1e-6),
            "expected counter -30 Str from Brute Force Solution, got {str_mods:#?}",
        );
    }

    /// Issue #196 (slice 5): Careful Planning —
    /// `Intelligence from Passives in Radius is Transformed to Dexterity`.
    #[test]
    fn careful_planning_transforms_int_base_to_dex_base() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+30 to Intelligence".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Careful Planning",
                "Viridian Jewel",
                &[(
                    "Intelligence from Passives in Radius is Transformed to Dexterity",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Ranger", &mut db);
        let item = socketed.get(1).unwrap();
        let jewel = identify_radius_jewel(1, item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::IntToDexTransform);

        let dex_mods = db.slice_named("Dexterity");
        assert!(
            dex_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Base)
                    && matches!(&m.source, Some(Source::Passive(id)) if *id == 2)
                    && (m.value.as_f64().unwrap_or(0.0) - 30.0).abs() < 1e-6),
            "expected +30 Dex BASE sourced from Passive(2), got {dex_mods:#?}",
        );
    }

    /// Issue #196 (slice 5): Efficient Training —
    /// `Intelligence from Passives in Radius is Transformed to Strength`.
    #[test]
    fn efficient_training_transforms_int_base_to_str_base() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+30 to Intelligence".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Efficient Training",
                "Crimson Jewel",
                &[(
                    "Intelligence from Passives in Radius is Transformed to Strength",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        let item = socketed.get(1).unwrap();
        let jewel = identify_radius_jewel(1, item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::IntToStrTransform);

        let str_mods = db.slice_named("Strength");
        assert!(
            str_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Base)
                    && matches!(&m.source, Some(Source::Passive(id)) if *id == 2)
                    && (m.value.as_f64().unwrap_or(0.0) - 30.0).abs() < 1e-6),
            "expected +30 Str BASE sourced from Passive(2), got {str_mods:#?}",
        );
    }

    /// Issue #196 (slice 13): Fluid Motion — `Strength from Passives
    /// in Radius is Transformed to Dexterity`. Mirror of Inertia at
    /// 1× scale: in-radius `+N Str` BASE becomes `+N Dex` BASE
    /// sourced as the in-radius node, plus a counter `-N Str` so the
    /// source attribute is moved rather than duplicated. The jewel's
    /// plain `+(16-24) to Dexterity` line still applies globally.
    #[test]
    fn fluid_motion_transforms_str_base_to_dex_base() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+30 to Strength".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Fluid Motion",
                "Viridian Jewel",
                &[
                    ("+20 to Dexterity", ModSection::Explicit),
                    (
                        "Strength from Passives in Radius is Transformed to Dexterity",
                        ModSection::Explicit,
                    ),
                ],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Ranger", &mut db);
        assert_eq!(report.applied_jewels, 1);

        let item = socketed.get(1).unwrap();
        let jewel = identify_radius_jewel(1, item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::StrToDexTransform);

        // Transformed +30 Dex sourced from node 2.
        let dex_mods = db.slice_named("Dexterity");
        assert!(
            dex_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Base)
                    && matches!(&m.source, Some(Source::Passive(id)) if *id == 2)
                    && (m.value.as_f64().unwrap_or(0.0) - 30.0).abs() < 1e-6),
            "expected +30 Dex BASE sourced from Passive(2), got {dex_mods:#?}",
        );
        // Plain +20 Dex global mod still landed.
        assert!(
            dex_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Base)
                    && (m.value.as_f64().unwrap_or(0.0) - 20.0).abs() < 1e-6),
            "expected global +20 Dex from Fluid Motion, got {dex_mods:#?}",
        );
        // Counter -30 Str cancelling the source contribution.
        let str_mods = db.slice_named("Strength");
        assert!(
            str_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Base)
                    && (m.value.as_f64().unwrap_or(0.0) + 30.0).abs() < 1e-6),
            "expected counter -30 Str from Fluid Motion transform, got {str_mods:#?}",
        );
    }

    /// Issue #196 (slice 13): out-of-radius node must not be
    /// transformed. Pin node 2 to no Str so any emission sourced from
    /// a passive can only have come from out-of-radius node 3.
    #[test]
    fn fluid_motion_does_not_transform_out_of_radius_str() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec![];
        tree.nodes.get_mut(&3).unwrap().stats = vec!["+30 to Strength".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        alloc.insert(3);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Fluid Motion",
                "Viridian Jewel",
                &[(
                    "Strength from Passives in Radius is Transformed to Dexterity",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Ranger", &mut db);

        // Out-of-radius node 3's Str must not be re-emitted as Dex.
        let dex_mods = db.slice_named("Dexterity");
        assert!(
            !dex_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Base)
                    && matches!(&m.source, Some(Source::Passive(id)) if *id == 3)),
            "out-of-radius Str must not be transformed, got {dex_mods:#?}",
        );
    }

    /// Issue #196 (slice 4): Inertia — `Dexterity from Passives in
    /// Radius is Transformed to Strength`. Mirror of Fertile Mind
    /// (Dex → Int) with Strength as the destination at 1× scale.
    /// In-radius `+N Dex` BASE becomes `+N Str` BASE sourced as the
    /// in-radius node, plus a counter `-N Dex` so the source attribute
    /// is moved rather than duplicated. The jewel's plain
    /// `+(16-24) to Strength` line still applies globally as a
    /// vanilla bonus.
    #[test]
    fn inertia_transforms_dex_base_to_str_base() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+30 to Dexterity".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Inertia",
                "Crimson Jewel",
                &[
                    ("+20 to Strength", ModSection::Explicit),
                    (
                        "Dexterity from Passives in Radius is Transformed to Strength",
                        ModSection::Explicit,
                    ),
                ],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        assert_eq!(report.applied_jewels, 1);

        let item = socketed.get(1).unwrap();
        let jewel = identify_radius_jewel(1, item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::DexToStrTransform);

        // Transformed +30 Str sourced from node 2.
        let str_mods = db.slice_named("Strength");
        assert!(
            str_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Base)
                    && matches!(&m.source, Some(Source::Passive(id)) if *id == 2)
                    && (m.value.as_f64().unwrap_or(0.0) - 30.0).abs() < 1e-6),
            "expected +30 Str BASE sourced from Passive(2), got {str_mods:#?}",
        );
        // Plain +20 Str global mod still landed.
        assert!(
            str_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Base)
                    && (m.value.as_f64().unwrap_or(0.0) - 20.0).abs() < 1e-6),
            "expected global +20 Str from Inertia, got {str_mods:#?}",
        );
        // Counter -30 Dex cancelling the source contribution.
        let dex_mods = db.slice_named("Dexterity");
        assert!(
            dex_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Base)
                    && (m.value.as_f64().unwrap_or(0.0) + 30.0).abs() < 1e-6),
            "expected counter -30 Dex from Inertia transform, got {dex_mods:#?}",
        );
    }

    /// Issue #196 (slice 4): out-of-radius node (3 in `mk_tree`) with a
    /// Dex mod must not be transformed. Pin node 2 to no Dex so any
    /// emission sourced from a passive can only have come from node 3.
    #[test]
    fn inertia_does_not_transform_out_of_radius_dex() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec![];
        tree.nodes.get_mut(&3).unwrap().stats = vec!["+30 to Dexterity".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(3);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Inertia",
                "Crimson Jewel",
                &[(
                    "Dexterity from Passives in Radius is Transformed to Strength",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);

        let str_mods = db.slice_named("Strength");
        assert!(
            !str_mods
                .iter()
                .any(|m| matches!(&m.source, Some(Source::Passive(id)) if *id == 3)),
            "Inertia should not transform out-of-radius nodes, got {str_mods:#?}",
        );
    }

    #[test]
    fn karui_heart_transforms_str_base_to_life_at_5x() {
        // Custom tree where node 2 has a `+30 to Strength` BASE stat.
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+30 to Strength".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Karui Heart",
                "Crimson Jewel",
                &[
                    ("+25 to Strength", ModSection::Explicit),
                    (
                        "Strength from Passives in Radius is Transformed to Life",
                        ModSection::Explicit,
                    ),
                ],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        assert_eq!(report.applied_jewels, 1);
        // Plain mod (+25 Str) is one emission. Transform of +30 Str emits
        // two (Life + counter Str) for at least three emissions total.
        assert!(report.mod_emissions >= 3);

        // Verify the dispatch picked the dedicated handler kind.
        let item = socketed.get(1).unwrap();
        let jewel = identify_radius_jewel(1, item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::StrToLifeTransform);

        // Transformed +150 Life sourced from node 2 (30 Str × 5 = 150 Life).
        let life = db.slice_named("Life");
        assert!(
            life.iter().any(|m| matches!(m.kind, crate::ModType::Base)
                && matches!(&m.source, Some(Source::Passive(id)) if *id == 2)
                && (m.value.as_f64().unwrap_or(0.0) - 150.0).abs() < 1e-6),
            "expected +150 Life BASE sourced from Passive(2), got {life:#?}",
        );

        // Plain +25 Str global mod still landed.
        let str_mods = db.slice_named("Strength");
        assert!(
            str_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Base)
                    && (m.value.as_f64().unwrap_or(0.0) - 25.0).abs() < 1e-6),
            "expected global +25 Str from Karui Heart, got {str_mods:#?}",
        );
        // Counter -30 Str cancelling the source contribution.
        assert!(
            str_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Base)
                    && (m.value.as_f64().unwrap_or(0.0) + 30.0).abs() < 1e-6),
            "expected counter -30 Str from Karui Heart transform, got {str_mods:#?}",
        );
    }

    /// Issue #196 (slice 2): Brawn — `Strength from Passives in Radius is
    /// Doubled`. Verifies the dispatch routes to the dedicated handler
    /// kind ([`HandlerKind::StrDoubleTransform`]) and that the in-radius
    /// `+N Strength` BASE mod produces an extra `+N Strength` BASE
    /// sourced as the in-radius node — no counter, since doubling
    /// preserves the source contribution.
    #[test]
    fn brawn_doubles_in_radius_str_base() {
        // Custom tree where node 2 has a `+30 to Strength` BASE stat.
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+30 to Strength".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Brawn",
                "Crimson Jewel",
                &[
                    ("+12 to Strength", ModSection::Explicit),
                    (
                        "Strength from Passives in Radius is Doubled",
                        ModSection::Explicit,
                    ),
                ],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        assert_eq!(report.applied_jewels, 1);

        // Dispatch routed to the dedicated handler.
        let item = socketed.get(1).unwrap();
        let jewel = identify_radius_jewel(1, item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::StrDoubleTransform);

        // The doubled +30 Str should be sourced from Passive(2). The
        // node's *own* +30 Str isn't (re)added by the jewel handler —
        // the engine reads it from `node.stats` already — so this
        // single emission represents the *extra* copy that doubles it.
        let str_mods = db.slice_named("Strength");
        assert!(
            str_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Base)
                    && matches!(&m.source, Some(Source::Passive(id)) if *id == 2)
                    && (m.value.as_f64().unwrap_or(0.0) - 30.0).abs() < 1e-6),
            "expected extra +30 Str BASE sourced from Passive(2), got {str_mods:#?}",
        );

        // Plain +12 Str global mod still applied (the jewel's own non-
        // radius bonus isn't consumed by the doubling pass).
        assert!(
            str_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Base)
                    && (m.value.as_f64().unwrap_or(0.0) - 12.0).abs() < 1e-6),
            "expected global +12 Str from Brawn, got {str_mods:#?}",
        );

        // No counter -30 Str — doubling doesn't transform-away the source.
        assert!(
            !str_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Base)
                    && (m.value.as_f64().unwrap_or(0.0) + 30.0).abs() < 1e-6),
            "Brawn should not emit a counter -30 Str (that's transform semantics, \
             not double semantics); got {str_mods:#?}",
        );
    }

    /// Issue #196 (slice 2): Brawn doubling only fires on in-radius
    /// allocated nodes. `mk_tree` has node 3 at 2000 units from the
    /// socket (well outside Large radius); a +30 Str on node 3 must
    /// not get doubled. Pin node 2 (in radius) to no Str so any Str
    /// emission sourced from a passive can only have come from node 3.
    #[test]
    fn brawn_does_not_double_out_of_radius_str() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec![];
        tree.nodes.get_mut(&3).unwrap().stats = vec!["+30 to Strength".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(3);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Brawn",
                "Crimson Jewel",
                &[(
                    "Strength from Passives in Radius is Doubled",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);

        let str_mods = db.slice_named("Strength");
        // No emission sourced from Passive(3) — it was out of radius.
        assert!(
            !str_mods
                .iter()
                .any(|m| matches!(&m.source, Some(Source::Passive(id)) if *id == 3)),
            "Brawn should not double out-of-radius nodes, got {str_mods:#?}",
        );
    }

    /// Karui Heart on a node outside the radius emits zero transformed mods.
    /// The plain `+N to Strength` global line still lands (it's not gated on
    /// the radius), so the dispatch reports `mod_emissions = 1`.
    #[test]
    fn karui_heart_skips_out_of_radius_nodes() {
        let mut tree = mk_tree();
        // Node 3 sits at (2000, 0) — outside Large ring (~1800).
        tree.nodes.get_mut(&3).unwrap().stats = vec!["+30 to Strength".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(3);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Karui Heart",
                "Crimson Jewel",
                &[
                    ("+25 to Strength", ModSection::Explicit),
                    (
                        "Strength from Passives in Radius is Transformed to Life",
                        ModSection::Explicit,
                    ),
                ],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        assert_eq!(report.applied_jewels, 1);
        // Only the plain +25 Str global line — no transform fires.
        assert_eq!(report.mod_emissions, 1);
        // No transformed Life mod sourced from a passive.
        let life = db.slice_named("Life");
        assert!(
            !life
                .iter()
                .any(|m| matches!(&m.source, Some(Source::Passive(_)))),
            "no Life mod should be sourced from a passive when nothing is in radius, got {life:#?}",
        );
    }

    /// A radius jewel that transforms Inc Life out of radius shouldn't fire
    /// on a node *outside* the medium ring.
    #[test]
    fn healthy_mind_skips_out_of_radius_nodes() {
        let tree = mk_tree();
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        // Node 3 sits at (2000, 0) — outside Large ring (~1800).
        alloc.insert(3);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Healthy Mind",
                "Cobalt Jewel",
                &[(
                    "Increases and Reductions to Life in Radius are Transformed to apply to Mana at 200% of their value",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        // No transformed mods (node 3 is out of Large ring) and no plain
        // global mods (the only line was the metadata marker).
        assert_eq!(report.applied_jewels, 1);
        assert_eq!(report.mod_emissions, 0);
    }

    /// Issue #196: Might in All Forms. `Dexterity and Intelligence from
    /// passives in Radius count towards Strength Melee Damage bonus`.
    /// Sums in-radius allocated `+N Dexterity` and `+N Intelligence` BASE
    /// mods and emits a single `DexIntToMeleeBonus` BASE mod whose value
    /// is `Dex + Int`. PoB's `data/uniques/jewel.lua` definition is the
    /// `Dexterity and Intelligence from passives in Radius count towards
    /// Strength Melee Damage bonus` line in `ModParser.lua`'s
    /// `jewelSelfFuncs` (line ~6160 — sums Dex and Int via
    /// `node.modList:Sum("BASE", nil, ...)` and emits the same stat name
    /// at radius-finalize time). 20 Dex + 30 Int → 50 `DexIntToMeleeBonus`.
    #[test]
    fn might_in_all_forms_emits_dex_int_to_melee_bonus_from_radius() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats =
            vec!["+20 to Dexterity".into(), "+30 to Intelligence".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Might in All Forms",
                "Crimson Jewel",
                &[(
                    "Dexterity and Intelligence from passives in Radius count towards Strength Melee Damage bonus",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        assert_eq!(report.applied_jewels, 1);

        let item = socketed.get(1).unwrap();
        let jewel = identify_radius_jewel(1, item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::DexIntToStrMeleeBonus);

        // +50 DexIntToMeleeBonus BASE sourced as the jewel (20 + 30 = 50).
        let bonus_mods = db.slice_named("DexIntToMeleeBonus");
        assert!(
            bonus_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Base)
                    && (m.value.as_f64().unwrap_or(0.0) - 50.0).abs() < 1e-6),
            "expected +50 DexIntToMeleeBonus BASE from Might in All Forms, got {bonus_mods:#?}",
        );
    }

    /// Issue #196: Transcendent Flesh (current variant). `3% increased
    /// Life Recovery Rate per 10 Strength on Allocated Passives in Radius`.
    /// Sister to Spire of Stone / Tempered Spirit: sums in-radius
    /// allocated `+N Strength` BASE mods, integer-divides by 10, and
    /// multiplies by the per-line percentage (3 here) before emitting a
    /// single Inc `LifeRecovery` mod sourced as the jewel. Mirrors PoB's
    /// `jewelSelfFuncs` line `getPerStat("LifeRecoveryRate", "INC", 0,
    /// "Str", 3 / 10)` (`ModParser.lua` ~6171). Sum = 50 Str →
    /// floor(50/10) × 3 = 15% Inc LifeRecovery.
    #[test]
    fn transcendent_flesh_emits_inc_life_recovery_per_ten_str_in_radius() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+50 to Strength".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Transcendent Flesh",
                "Crimson Jewel",
                &[(
                    "3% increased Life Recovery Rate per 10 Strength on Allocated Passives in Radius",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        assert_eq!(report.applied_jewels, 1);

        let item = socketed.get(1).unwrap();
        let jewel = identify_radius_jewel(1, item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::StrCountToIncLifeRecovery);

        let lr_mods = db.slice_named("LifeRecovery");
        assert!(
            lr_mods.iter().any(|m| matches!(m.kind, crate::ModType::Inc)
                && (m.value.as_f64().unwrap_or(0.0) - 15.0).abs() < 1e-6),
            "expected +15% Inc LifeRecovery from Transcendent Flesh, got {lr_mods:#?}",
        );
    }

    /// Issue #196: integer-divide on Str sum, then × 3. 27 Str →
    /// floor(27/10) × 3 = 6% (not 8.1%).
    #[test]
    fn transcendent_flesh_integer_divides_str_sum() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+27 to Strength".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Transcendent Flesh",
                "Crimson Jewel",
                &[(
                    "3% increased Life Recovery Rate per 10 Strength on Allocated Passives in Radius",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);

        let lr_mods = db.slice_named("LifeRecovery");
        assert!(
            lr_mods.iter().any(|m| matches!(m.kind, crate::ModType::Inc)
                && (m.value.as_f64().unwrap_or(0.0) - 6.0).abs() < 1e-6),
            "expected +6% Inc LifeRecovery (floor(27/10) × 3), got {lr_mods:#?}",
        );
    }

    /// Issue #196: Tempered Flesh (current variant) carries the 2%
    /// rate. 50 Str → floor(50/10) × 2 = 10% Inc LifeRecovery. Same
    /// dispatch arm as Transcendent Flesh; the per-line rate is parsed
    /// off the marker text so both 2% and 3% jewels share one handler.
    #[test]
    fn tempered_flesh_emits_inc_life_recovery_at_two_percent_rate() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+50 to Strength".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Tempered Flesh",
                "Crimson Jewel",
                &[(
                    "2% increased Life Recovery Rate per 10 Strength on Allocated Passives in Radius",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        assert_eq!(report.applied_jewels, 1);

        let item = socketed.get(1).unwrap();
        let jewel = identify_radius_jewel(1, item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::StrCountToIncLifeRecovery);

        let lr_mods = db.slice_named("LifeRecovery");
        assert!(
            lr_mods.iter().any(|m| matches!(m.kind, crate::ModType::Inc)
                && (m.value.as_f64().unwrap_or(0.0) - 10.0).abs() < 1e-6),
            "expected +10% Inc LifeRecovery from Tempered Flesh (2% rate), got {lr_mods:#?}",
        );
    }

    /// Issue #196: out-of-radius / unallocated nodes contribute zero. A
    /// single far-radius node carrying +50 Str must produce no Life
    /// Recovery mod (default Medium radius for Tempered/Transcendent
    /// Flesh per `Data/Uniques/jewel.lua` line 690/703).
    #[test]
    fn transcendent_flesh_skips_out_of_radius_nodes() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&3).unwrap().stats = vec!["+50 to Strength".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(3);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Transcendent Flesh",
                "Crimson Jewel",
                &[(
                    "3% increased Life Recovery Rate per 10 Strength on Allocated Passives in Radius",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);

        let lr_mods = db.slice_named("LifeRecovery");
        assert!(
            !lr_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Inc)),
            "expected no Inc LifeRecovery when only out-of-radius nodes carry Strength, got {lr_mods:#?}",
        );
    }

    /// Issue #196: Might in All Forms only sums Dex+Int from in-radius
    /// allocated nodes — a node sitting outside Medium ring (node 3 at
    /// x=2000) should not contribute. Sum of 0 → no emitted mod.
    #[test]
    fn might_in_all_forms_skips_out_of_radius_nodes() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&3).unwrap().stats = vec!["+40 to Dexterity".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(3);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Might in All Forms",
                "Crimson Jewel",
                &[(
                    "Dexterity and Intelligence from passives in Radius count towards Strength Melee Damage bonus",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);

        let bonus_mods = db.slice_named("DexIntToMeleeBonus");
        assert!(
            bonus_mods.is_empty(),
            "expected no DexIntToMeleeBonus when only out-of-radius nodes carry the attribute, got {bonus_mods:#?}",
        );
    }

    /// Issue #196: Transcendent Spirit (current variant). `3% increased
    /// Movement Speed per 10 Dexterity on Allocated Passives in Radius`.
    /// Sister to Spire of Stone / Eldritch Knowledge: sums in-radius
    /// allocated `+N Dexterity` BASE mods, integer-divides by 10, and
    /// multiplies by the per-line percentage (3 here) before emitting a
    /// single Inc `MovementSpeed` mod sourced as the jewel. Mirrors PoB's
    /// `jewelSelfFuncs` line `getPerStat("MovementSpeed", "INC", 0, "Dex",
    /// 3 / 10)` (`ModParser.lua` ~6179).
    #[test]
    fn transcendent_spirit_emits_inc_movement_speed_per_ten_dex_in_radius() {
        let mut tree = mk_tree();
        // Sum = 50 Dex → floor(50/10) × 3 = 15% Inc MovementSpeed.
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+50 to Dexterity".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Transcendent Spirit",
                "Viridian Jewel",
                &[(
                    "3% increased Movement Speed per 10 Dexterity on Allocated Passives in Radius",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Ranger", &mut db);
        assert_eq!(report.applied_jewels, 1);

        let item = socketed.get(1).unwrap();
        let jewel = identify_radius_jewel(1, item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::DexCountToIncMovementSpeed);

        let ms_mods = db.slice_named("MovementSpeed");
        assert!(
            ms_mods.iter().any(|m| matches!(m.kind, crate::ModType::Inc)
                && (m.value.as_f64().unwrap_or(0.0) - 15.0).abs() < 1e-6),
            "expected +15% Inc MovementSpeed from Transcendent Spirit, got {ms_mods:#?}",
        );
    }

    /// Issue #196: integer-divide on Dex sum, then × 3. 27 Dex →
    /// floor(27/10) × 3 = 6% (not 8.1%).
    #[test]
    fn transcendent_spirit_integer_divides_dex_sum() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+27 to Dexterity".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Transcendent Spirit",
                "Viridian Jewel",
                &[(
                    "3% increased Movement Speed per 10 Dexterity on Allocated Passives in Radius",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Ranger", &mut db);

        let ms_mods = db.slice_named("MovementSpeed");
        assert!(
            ms_mods.iter().any(|m| matches!(m.kind, crate::ModType::Inc)
                && (m.value.as_f64().unwrap_or(0.0) - 6.0).abs() < 1e-6),
            "expected +6% Inc MovementSpeed (floor(27/10) × 3), got {ms_mods:#?}",
        );
    }

    /// Issue #196: out-of-radius / unallocated nodes contribute zero. A
    /// single far-radius node carrying +50 Dex must produce no Movement
    /// Speed mod (default Medium radius for Tempered/Transcendent Spirit
    /// per `Data/Uniques/jewel.lua` line 749/760).
    #[test]
    fn transcendent_spirit_skips_out_of_radius_nodes() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&3).unwrap().stats = vec!["+50 to Dexterity".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(3);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Transcendent Spirit",
                "Viridian Jewel",
                &[(
                    "3% increased Movement Speed per 10 Dexterity on Allocated Passives in Radius",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Ranger", &mut db);

        let ms_mods = db.slice_named("MovementSpeed");
        assert!(
            !ms_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Inc)),
            "expected no Inc MovementSpeed when only out-of-radius nodes carry Dexterity, got {ms_mods:#?}",
        );
    }

    /// Issue #196: Transcendent Mind (current variant). `3% increased
    /// Mana Recovery Rate per 10 Intelligence on Allocated Passives in
    /// Radius`. Sister to Tempered/Transcendent Spirit/Flesh: sums
    /// in-radius allocated `+N Intelligence` BASE mods, integer-divides
    /// by 10, and multiplies by the per-line percentage (3 here) before
    /// emitting a single Inc `ManaRecovery` mod sourced as the jewel.
    /// Mirrors PoB's `jewelSelfFuncs` line `getPerStat("ManaRecoveryRate",
    /// "INC", 0, "Int", 3 / 10)` (`ModParser.lua` ~6176). Sum = 50 Int →
    /// floor(50/10) × 3 = 15% Inc ManaRecovery.
    #[test]
    fn transcendent_mind_emits_inc_mana_recovery_per_ten_int_in_radius() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+50 to Intelligence".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Transcendent Mind",
                "Cobalt Jewel",
                &[(
                    "3% increased Mana Recovery Rate per 10 Intelligence on Allocated Passives in Radius",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Witch", &mut db);
        assert_eq!(report.applied_jewels, 1);

        let item = socketed.get(1).unwrap();
        let jewel = identify_radius_jewel(1, item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::IntCountToIncManaRecovery);

        let mr_mods = db.slice_named("ManaRecovery");
        assert!(
            mr_mods.iter().any(|m| matches!(m.kind, crate::ModType::Inc)
                && (m.value.as_f64().unwrap_or(0.0) - 15.0).abs() < 1e-6),
            "expected +15% Inc ManaRecovery from Transcendent Mind, got {mr_mods:#?}",
        );
    }

    /// Issue #196: integer-divide on Int sum, then × 3. 27 Int →
    /// floor(27/10) × 3 = 6% (not 8.1%).
    #[test]
    fn transcendent_mind_integer_divides_int_sum() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+27 to Intelligence".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Transcendent Mind",
                "Cobalt Jewel",
                &[(
                    "3% increased Mana Recovery Rate per 10 Intelligence on Allocated Passives in Radius",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Witch", &mut db);

        let mr_mods = db.slice_named("ManaRecovery");
        assert!(
            mr_mods.iter().any(|m| matches!(m.kind, crate::ModType::Inc)
                && (m.value.as_f64().unwrap_or(0.0) - 6.0).abs() < 1e-6),
            "expected +6% Inc ManaRecovery (floor(27/10) × 3), got {mr_mods:#?}",
        );
    }

    /// Issue #196: Tempered Mind carries the 2% rate. 50 Int →
    /// floor(50/10) × 2 = 10% Inc ManaRecovery. Same dispatch arm as
    /// Transcendent Mind; the per-line rate is parsed off the marker text
    /// so both 2% and 3% jewels share one handler. Mirrors PoB's
    /// `jewelSelfFuncs` line `getPerStat("ManaRecoveryRate", "INC", 0,
    /// "Int", 2 / 10)` (`ModParser.lua` ~6175).
    #[test]
    fn tempered_mind_emits_inc_mana_recovery_at_two_percent_rate() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+50 to Intelligence".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Tempered Mind",
                "Cobalt Jewel",
                &[(
                    "2% increased Mana Recovery Rate per 10 Intelligence on Allocated Passives in Radius",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Witch", &mut db);
        assert_eq!(report.applied_jewels, 1);

        let item = socketed.get(1).unwrap();
        let jewel = identify_radius_jewel(1, item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::IntCountToIncManaRecovery);

        let mr_mods = db.slice_named("ManaRecovery");
        assert!(
            mr_mods.iter().any(|m| matches!(m.kind, crate::ModType::Inc)
                && (m.value.as_f64().unwrap_or(0.0) - 10.0).abs() < 1e-6),
            "expected +10% Inc ManaRecovery from Tempered Mind (2% rate), got {mr_mods:#?}",
        );
    }

    /// Issue #196: out-of-radius / unallocated nodes contribute zero. A
    /// single far-radius node carrying +50 Int must produce no Mana
    /// Recovery mod (default Medium radius for Tempered/Transcendent
    /// Mind per `Data/Uniques/jewel.lua` line 719/732).
    #[test]
    fn transcendent_mind_skips_out_of_radius_nodes() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&3).unwrap().stats = vec!["+50 to Intelligence".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(3);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Transcendent Mind",
                "Cobalt Jewel",
                &[(
                    "3% increased Mana Recovery Rate per 10 Intelligence on Allocated Passives in Radius",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Witch", &mut db);

        let mr_mods = db.slice_named("ManaRecovery");
        assert!(
            !mr_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Inc)),
            "expected no Inc ManaRecovery when only out-of-radius nodes carry Intelligence, got {mr_mods:#?}",
        );
    }

    /// Issue #196: Tempered Spirit (current variant) carries the 2%
    /// rate. 50 Dex → floor(50/10) × 2 = 10% Inc MovementSpeed. Same
    /// dispatch arm as Transcendent Spirit; the per-line rate is parsed
    /// off the marker text so both 2% and 3% jewels share one handler.
    #[test]
    fn tempered_spirit_emits_inc_movement_speed_at_two_percent_rate() {
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+50 to Dexterity".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Tempered Spirit",
                "Viridian Jewel",
                &[(
                    "2% increased Movement Speed per 10 Dexterity on Allocated Passives in Radius",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Ranger", &mut db);
        assert_eq!(report.applied_jewels, 1);

        let item = socketed.get(1).unwrap();
        let jewel = identify_radius_jewel(1, item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::DexCountToIncMovementSpeed);

        let ms_mods = db.slice_named("MovementSpeed");
        assert!(
            ms_mods.iter().any(|m| matches!(m.kind, crate::ModType::Inc)
                && (m.value.as_f64().unwrap_or(0.0) - 10.0).abs() < 1e-6),
            "expected +10% Inc MovementSpeed from Tempered Spirit, got {ms_mods:#?}",
        );
    }

    /// Issue #196: The Light of Meaning (Life variant). `Passive Skills in
    /// Radius also grant +5 to maximum Life`. Per-node grant: with one
    /// allocated eligible (Notable) node in radius, the player gains
    /// `5 × 1 = +5 Life` BASE sourced as the jewel. Mirrors PoB's
    /// `jewelOtherFuncs` entry at `ModParser.lua:6054-6060`.
    #[test]
    fn light_of_meaning_life_grant_emits_per_allocated_in_radius_node() {
        let tree = mk_tree();
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "The Light of Meaning",
                "Prismatic Jewel",
                &[(
                    "Passive Skills in Radius also grant +5 to maximum Life",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Ranger", &mut db);
        assert_eq!(report.applied_jewels, 1);

        let item = socketed.get(1).unwrap();
        let jewel = identify_radius_jewel(1, item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::PassiveAlsoGrantBaseLife);

        let life_mods = db.slice_named("Life");
        assert!(
            life_mods
                .iter()
                .any(|m| matches!(m.kind, crate::ModType::Base)
                    && (m.value.as_f64().unwrap_or(0.0) - 5.0).abs() < 1e-6),
            "expected +5 Life BASE from The Light of Meaning, got {life_mods:#?}",
        );
    }

    /// Issue #196: The Light of Meaning per-node grant must scale with the
    /// number of *allocated* eligible nodes in radius. Two allocated
    /// notables in radius → `5 × 2 = +10 Life`. The far node (out of
    /// radius) must not contribute.
    #[test]
    fn light_of_meaning_life_grant_scales_with_allocated_count_and_skips_out_of_radius() {
        let mut tree = mk_tree();
        // Add a second in-radius eligible node by relocating Far Notable into
        // the Large radius. Group 30 currently sits at x=2000 (out of 1500
        // Large); move it to x=900 so it's inside radius and add it to alloc.
        tree.groups.get_mut(&30).unwrap().x = 900.0;
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        alloc.insert(3);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "The Light of Meaning",
                "Prismatic Jewel",
                &[(
                    "Passive Skills in Radius also grant +5 to maximum Life",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Ranger", &mut db);

        let life_mods = db.slice_named("Life");
        assert!(
            life_mods.iter().any(|m| matches!(m.kind, crate::ModType::Base)
                && (m.value.as_f64().unwrap_or(0.0) - 10.0).abs() < 1e-6),
            "expected +10 Life BASE (5 × 2 allocated nodes) from The Light of Meaning, got {life_mods:#?}",
        );
    }

    /// Issue #196: The Light of Meaning per-node grant must NOT count
    /// Keystone or JewelSocket nodes (mirrors PoB's
    /// `node.type ~= "Keystone" and node.type ~= "Socket" and node.type
    /// ~= "ClassStart"` guard at `ModParser.lua:6056`). One allocated
    /// Notable + one allocated Keystone in radius should still emit only
    /// `+5 Life` (Notable alone), not `+10`.
    #[test]
    fn light_of_meaning_life_grant_skips_keystone_and_socket_nodes() {
        let mut tree = mk_tree();
        // Reuse Far Notable's slot for an in-radius Keystone — set kind to
        // Keystone and pull the group inside the Large radius. The Notable at
        // node 2 stays as the only eligible counted node.
        tree.groups.get_mut(&30).unwrap().x = 900.0;
        tree.nodes.get_mut(&3).unwrap().kind = NodeKind::Keystone;
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        alloc.insert(3);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "The Light of Meaning",
                "Prismatic Jewel",
                &[(
                    "Passive Skills in Radius also grant +5 to maximum Life",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let _ = apply_radius_jewels(&tree, &alloc, &socketed, "Ranger", &mut db);

        let life_mods = db.slice_named("Life");
        let total: f64 = life_mods
            .iter()
            .filter(|m| matches!(m.kind, crate::ModType::Base))
            .filter(|m| {
                matches!(&m.source, Some(Source::Other(s)) if s.contains("The Light of Meaning"))
            })
            .map(|m| m.value.as_f64().unwrap_or(0.0))
            .sum();
        assert!(
            (total - 5.0).abs() < 1e-6,
            "expected +5 Life BASE (only the Notable counts; Keystone is excluded), got total {total} from {life_mods:#?}",
        );
    }
}
