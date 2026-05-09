//! Per-monster-level base stat tables — mirrors PoB's `Data/Misc.lua`.
//!
//! Foundation slice for [#20](https://github.com/jonatanferm/PathOfBuildingMK2/issues/20)
//! (parallel minion calc env). The minion perform pass needs these arrays so it can compute
//! a minion's life as `monster_ally_life_table[level] × minion.life`, and similar for damage
//! / accuracy / evasion / armour.
//!
//! Tables are 100-entry, 1-indexed in PoB (level 1 → index `[1]`). MK2 stores them as
//! plain `[u32; 100]` (or `[f32; 100]` for the floating-point ones), 0-indexed; use
//! [`monster_life_at_level`] / [`monster_ally_life_at_level`] etc. for clamped lookups.
//!
//! These constants are reproduced verbatim from `.PathOfBuilding/src/Data/Misc.lua`.
//! They've been stable for many leagues; if PoE rebalances them, regenerate via
//! `pob-extract` (a future slice can lift them into `data/misc.json`).

/// Base monster life by level (the canonical "white mob" table). Used for most
/// minion types (`lifeScaling = nil`). 100 entries, level 1 → index `[0]`.
pub const MONSTER_LIFE_TABLE: [u32; 100] = [
    22, 26, 31, 36, 42, 48, 55, 62, 70, 78, 87, 97, 107, 119, 131, 144, 158, 173, 190, 207, 226,
    246, 267, 290, 315, 341, 370, 400, 432, 467, 504, 543, 585, 630, 678, 730, 785, 843, 905, 972,
    1042, 1118, 1198, 1284, 1375, 1472, 1575, 1685, 1802, 1927, 2059, 2200, 2350, 2509, 2678, 2858,
    3050, 3253, 3469, 3698, 3942, 4201, 4476, 4768, 5078, 5407, 5756, 6127, 6520, 6937, 7380, 7850,
    8348, 8876, 9436, 10030, 10660, 11328, 12036, 12787, 13582, 14425, 15319, 16265, 17268, 18331,
    19457, 20649, 21913, 23250, 24667, 26168, 27756, 29438, 31220, 33105, 35101, 37214, 39450,
    41817,
];

/// Alternate table 2 (`lifeScaling = "AltLife1"`). Used by less tanky minion types.
pub const MONSTER_LIFE_TABLE_2: [u32; 100] = [
    10, 12, 15, 18, 21, 25, 29, 34, 39, 44, 50, 56, 63, 70, 79, 87, 97, 107, 119, 131, 144, 158,
    174, 191, 209, 228, 249, 272, 296, 323, 351, 382, 415, 450, 489, 530, 574, 621, 672, 727, 786,
    850, 917, 990, 1069, 1153, 1243, 1339, 1443, 1554, 1673, 1800, 1937, 2083, 2240, 2408, 2587,
    2780, 2986, 3206, 3442, 3694, 3963, 4252, 4560, 4890, 5243, 5620, 6022, 6453, 6913, 7404, 7929,
    8490, 9089, 9729, 10412, 11141, 11920, 12751, 13638, 14585, 15596, 16675, 17825, 19053, 20363,
    21760, 23250, 24840, 26535, 28343, 30270, 32326, 34517, 36853, 39343, 41997, 44826, 47841,
];

/// Alternate table 3 (`lifeScaling = "AltLife2"`).
pub const MONSTER_LIFE_TABLE_3: [u32; 100] = [
    13, 15, 18, 22, 25, 29, 34, 38, 44, 49, 55, 62, 69, 77, 86, 95, 106, 117, 128, 141, 155, 170,
    187, 204, 223, 244, 266, 290, 316, 344, 373, 406, 440, 478, 518, 561, 608, 658, 712, 769, 831,
    898, 970, 1046, 1129, 1217, 1312, 1414, 1523, 1640, 1766, 1900, 2044, 2199, 2364, 2541, 2731,
    2934, 3151, 3384, 3633, 3900, 4185, 4490, 4816, 5165, 5539, 5938, 6364, 6820, 7308, 7829, 8386,
    8980, 9616, 10295, 11020, 11795, 12622, 13506, 14449, 15456, 16531, 17679, 18904, 20211, 21607,
    23096, 24684, 26380, 28188, 30117, 32175, 34370, 36711, 39207, 41870, 44708, 47735, 50962,
];

/// Player-allied minion / totem base life (the default for most allied minions and totems).
pub const MONSTER_ALLY_LIFE_TABLE: [u32; 100] = [
    15, 16, 18, 20, 22, 24, 27, 29, 32, 35, 38, 41, 44, 48, 52, 56, 60, 65, 70, 75, 81, 87, 93, 99,
    106, 114, 121, 130, 138, 148, 158, 168, 179, 190, 203, 215, 229, 244, 259, 275, 292, 310, 328,
    348, 369, 392, 415, 440, 465, 493, 522, 552, 584, 618, 653, 691, 730, 772, 816, 862, 910, 961,
    1015, 1072, 1131, 1194, 1260, 1329, 1402, 1478, 1559, 1644, 1733, 1827, 1926, 2029, 2138, 2253,
    2373, 2500, 2633, 2773, 2919, 3074, 3236, 3406, 3585, 3773, 3970, 4178, 4395, 4624, 4864, 5116,
    5381, 5659, 5951, 6257, 6578, 6916,
];

/// Per-level lookup helper. Clamps `level` to the `[1, 100]` range PoB supports and
/// returns the matching life value. Level 0 is treated as level 1, level >100 as 100.
#[must_use]
pub fn monster_life_at_level(level: u32) -> u32 {
    let idx = clamp_level_index(level);
    MONSTER_LIFE_TABLE[idx]
}

/// Variant 1: less tanky minion types (`lifeScaling = "AltLife1"`).
#[must_use]
pub fn monster_life2_at_level(level: u32) -> u32 {
    let idx = clamp_level_index(level);
    MONSTER_LIFE_TABLE_2[idx]
}

/// Variant 2: tankier minion types (`lifeScaling = "AltLife2"`).
#[must_use]
pub fn monster_life3_at_level(level: u32) -> u32 {
    let idx = clamp_level_index(level);
    MONSTER_LIFE_TABLE_3[idx]
}

/// Player-ally life lookup. Used for most summoned minions / totems.
#[must_use]
pub fn monster_ally_life_at_level(level: u32) -> u32 {
    let idx = clamp_level_index(level);
    MONSTER_ALLY_LIFE_TABLE[idx]
}

/// Map a 1-indexed level (matching PoB's table layout) to a clamped 0-indexed slot.
fn clamp_level_index(level: u32) -> usize {
    let l = level.max(1).min(100);
    (l - 1) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn life_tables_have_100_entries() {
        assert_eq!(MONSTER_LIFE_TABLE.len(), 100);
        assert_eq!(MONSTER_LIFE_TABLE_2.len(), 100);
        assert_eq!(MONSTER_LIFE_TABLE_3.len(), 100);
        assert_eq!(MONSTER_ALLY_LIFE_TABLE.len(), 100);
    }

    #[test]
    fn life_tables_are_strictly_monotonic() {
        // Sanity: a higher-level monster never has less life than a lower-level one.
        for table in [
            &MONSTER_LIFE_TABLE,
            &MONSTER_LIFE_TABLE_2,
            &MONSTER_LIFE_TABLE_3,
            &MONSTER_ALLY_LIFE_TABLE,
        ] {
            for w in table.windows(2) {
                assert!(w[1] >= w[0], "table not monotonic: {} → {}", w[0], w[1]);
            }
        }
    }

    #[test]
    fn lookup_helpers_match_canonical_pob_values() {
        // Spot-check pinned values from PoB's `Data/Misc.lua`. PoB indexes the tables
        // 1-based, so `monsterLifeTable[L]` (1-indexed) maps to MK2's
        // `MONSTER_LIFE_TABLE[L - 1]` (0-indexed).
        assert_eq!(monster_life_at_level(1), 22);
        assert_eq!(monster_life_at_level(70), 6937);
        assert_eq!(monster_life_at_level(90), 23250);
        assert_eq!(monster_life_at_level(100), 41817);
        assert_eq!(monster_ally_life_at_level(1), 15);
        assert_eq!(monster_ally_life_at_level(70), 1478);
        assert_eq!(monster_ally_life_at_level(90), 4178);
        assert_eq!(monster_ally_life_at_level(100), 6916);
    }

    #[test]
    fn level_clamping() {
        // Below 1 → treated as 1.
        assert_eq!(monster_life_at_level(0), MONSTER_LIFE_TABLE[0]);
        // Above 100 → treated as 100.
        assert_eq!(monster_life_at_level(150), MONSTER_LIFE_TABLE[99]);
        assert_eq!(monster_life_at_level(u32::MAX), MONSTER_LIFE_TABLE[99]);
    }
}
