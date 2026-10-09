# CCR-0012 (U0): no all57 build meets the slice5 definition; slice5 is derived

**Status: applied 2026-10-09 by CCR1.** Accepted with one change (orchestrator decision): `slice5_allflame_exp_046` (no weapon) was replaced by `slice5_allflame_exp_048` (Flicker Strike, Claw + Shield: exactly one weapon), derived with `derive_slice5.py` and checked with `--check` against the oracle (jit.off): one active skill, no aura/curse/link/minion `Extra*` mod. Calc doc §9.3.

- **Raised by:** U0 (corpus `crates/pob-oracle/corpus/U0/slice5.txt`)
- **Affects:** calc-architecture.md §9.3 (`slice5`), §9.7 (G1b "slice5 from XML"), §11.1 row U0
- **Pin:** 16de4b82d

## Problem

§9.3 defines slice5 as "one weapon, no auras; a single attack or spell", and G1b (§9.7) runs it
after CS3 + PF-A..D + DF-A..C + OF-A..F, so a slice5 build must not need PF-E (buffs, auras,
curses, warcries), MI (mirages), TR (triggers) or minion code. The first U0 list broke that:
`upstream_Dual_Savior` dual-wields two Saviours (Reflection mirage skills) and `allflame_exp_028`
dual-wields, with item-granted Envy (aura), Raise Spiders (minions) and Enduring Cry (warcry).

## Evidence

Over every all57 build (`.parity/builds`, 58 files), using the oracle's own view of each build
(`.parity/oracle/<stem>/stages.json`: the active skill list `skillOutputs` and the player modDB at
`initEnv:out`) and the active item set's weapons:

- every build whose Weapon 2 is not a weapon enables at least one aura, herald, minion, curse,
  warcry, buff, trap/mine/totem, Vaal or triggered skill (often item-granted: `Item:<n>` sources),
  except `allflame_exp_021` (Frostblink + Shield Charge), `allflame_exp_024` (Winter Orb + Flame
  Dash) and `allflame_exp_035` (Shield Charge + Frostblink + item-granted Create Lesser Shrine, a
  `GlobalEffect` buff, plus an `ExtraSupport`);
- `allflame_exp_021` and `allflame_exp_035` also carry `ExtraAura` (Condition:Tailwind) from
  `Tree:24848`, which PF-E applies; Tailwind changes action speed and so the DPS headline keys;
- `allflame_exp_024` has two active skills (Winter Orb and Flame Dash), not "a single attack or
  spell".

So the corpus holds no build that meets the definition as written, and only one (024) that meets
it up to a movement skill.

## Resolution (implemented by U0)

slice5 is five **derived** builds, checked in under `crates/pob-oracle/corpus/U0/slice5/`
(`slice5_<source stem>.xml`), made by `corpus/U0/derive_slice5.py` from an all57 build that already
has one weapon, no aura/curse/link/minion `Extra*` mod from its tree or items, and a plain attack
or spell as main skill: every socket group of the active skill set except the main one gets
`enabled="false"`; items, tree, config and every other attribute are unchanged.

| entry | main skill | weapons | oracle TotalDPS |
|---|---|---|---|
| `slice5_allflame_exp_024` | Winter Orb (spell) | Wand + Shield | 2657.5 |
| `slice5_allflame_exp_007` | Spark of the Nova (spell) | Wand + Shield | 772801 |
| `slice5_allflame_exp_030` | Ethereal Knives of the Massacre (spell) | Two Handed Sword | 248853 |
| `slice5_allflame_exp_031` | Shield Crush (attack) | Sceptre + Shield | 2002861 |
| `slice5_allflame_exp_046` | Poisonous Concoction of Bouncing (attack) | none + Shield | 53092 |

Each was run through the oracle (`pob_oracle all <xml> --out-dir DIR`) and checked with
`derive_slice5.py --check DIR/stages.json`: exactly one active skill, and no `ExtraAura`,
`ExtraCurse`, `ExtraAuraDebuff`, `ExtraLinkEffect` or `ExtraMinion` mod in the player modDB. The
remaining `ExtraSkill` mods (item-granted skills such as Asenath's Gentle Touch's curse) only
create socket groups, which are disabled. `tests/u0_smoke.rs` reads `corpus/U0/slice5/<entry>.xml`
first (then `.parity/builds/<entry>.xml`), and the opt-in `stages_fire_on_slice5` (tests/U0.rs)
probes `slice5_allflame_exp_031` with every stage.

## Asks

1. §9.3: "`slice5`: one weapon, no auras; a single attack or spell" →
   "`slice5`: `crates/pob-oracle/corpus/U0/slice5.txt` — five all57 builds reduced to their main
   socket group (`derive_slice5.py`, CCR-0012): one weapon, a single attack or spell, no aura,
   curse, buff, warcry, mirage, trigger or minion skill or `Extra*` aura mod."
2. G1b keeps "slice5 from XML: headline keys strict" with these files. If the integrator prefers
   undoctored builds, P3's synthetic one-skill builds (§9.3 `synthetic`) are the alternative; none
   exist yet.
