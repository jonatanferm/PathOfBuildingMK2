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

/// Base monster damage by level (the canonical "white mob" table). Used by the minion
/// damage compute as the per-hit damage baseline. Stored as `f32` because PoB's source
/// values are non-integer (e.g. 9.8199996948242 at level 8).
pub const MONSTER_DAMAGE_TABLE: [f32; 100] = [
    4.99, 5.56, 6.16, 6.81, 7.5, 8.23, 9.0, 9.82, 10.7, 11.62, 12.6, 13.64, 14.74, 15.91, 17.14,
    18.45, 19.83, 21.29, 22.84, 24.47, 26.19, 28.01, 29.94, 31.96, 34.11, 36.36, 38.75, 41.26,
    43.91, 46.7, 49.65, 52.75, 56.01, 59.45, 63.08, 66.89, 70.91, 75.13, 79.58, 84.26, 89.18,
    94.35, 99.8, 105.52, 111.53, 117.86, 124.5, 131.49, 138.83, 146.53, 154.63, 163.14, 172.07,
    181.45, 191.3, 201.63, 212.48, 223.87, 235.83, 248.37, 261.53, 275.33, 289.82, 305.01, 320.94,
    337.65, 355.18, 373.55, 392.81, 413.01, 434.18, 456.37, 479.62, 504.0, 529.54, 556.3, 584.35,
    613.73, 644.5, 676.75, 710.52, 745.89, 782.94, 821.73, 862.36, 904.9, 949.44, 996.07, 1044.89,
    1096.0, 1149.5, 1205.5, 1264.11, 1325.45, 1389.64, 1456.82, 1527.12, 1600.68, 1677.64, 1758.17,
];

/// Player-ally damage variant. Used for most summoned minions / totems' damage.
pub const MONSTER_ALLY_DAMAGE_TABLE: [f32; 100] = [
    5.62, 6.03, 6.46, 6.92, 7.41, 7.93, 8.48, 9.06, 9.68, 10.33, 11.03, 11.76, 12.54, 13.37, 14.25,
    15.17, 16.16, 17.2, 18.3, 19.46, 20.7, 22.0, 23.39, 24.85, 26.39, 28.03, 29.76, 31.58, 33.52,
    35.56, 37.72, 40.0, 42.41, 44.96, 47.64, 50.49, 53.49, 56.66, 60.0, 63.53, 67.26, 71.2, 75.36,
    79.74, 84.37, 89.25, 94.4, 99.84, 105.57, 111.62, 118.0, 124.73, 131.83, 139.31, 147.2, 155.52,
    164.28, 173.53, 183.27, 193.54, 204.37, 215.78, 227.8, 240.46, 253.81, 267.87, 282.69, 298.29,
    314.73, 332.05, 350.29, 369.5, 389.73, 411.04, 433.47, 457.09, 481.97, 508.15, 535.72, 564.75,
    595.3, 627.46, 661.31, 696.95, 734.45, 773.91, 815.45, 859.16, 905.15, 953.54, 1004.47,
    1058.04, 1114.41, 1173.71, 1236.1, 1301.73, 1370.76, 1443.38, 1519.76, 1600.09,
];

/// Base monster armour by level. Used by the per-hit physical mitigation formula on
/// the minion side once the minion perform pass needs it.
pub const MONSTER_ARMOUR_TABLE: [u32; 100] = [
    12, 15, 19, 23, 27, 32, 37, 43, 50, 57, 65, 74, 83, 94, 105, 118, 132, 147, 164, 182, 202, 224,
    248, 275, 303, 334, 368, 405, 445, 489, 537, 589, 646, 707, 774, 846, 925, 1010, 1103, 1204,
    1313, 1432, 1560, 1700, 1850, 2014, 2191, 2383, 2591, 2815, 3059, 3322, 3607, 3915, 4248, 4608,
    4997, 5418, 5873, 6365, 6896, 7469, 8089, 8757, 9480, 10259, 11101, 12009, 12989, 14047, 15188,
    16419, 17747, 19178, 20722, 22387, 24182, 26117, 28203, 30451, 32873, 35483, 38296, 41326,
    44591, 48107, 51894, 55973, 60365, 65095, 70188, 75670, 81573, 87926, 94765, 102125, 110047,
    118571, 127744, 137613,
];

/// Base monster evasion by level.
pub const MONSTER_EVASION_TABLE: [u32; 100] = [
    67, 86, 104, 124, 144, 166, 188, 211, 234, 259, 285, 311, 339, 368, 397, 428, 460, 493, 527,
    563, 600, 638, 677, 718, 760, 804, 849, 896, 944, 994, 1046, 1100, 1155, 1212, 1271, 1332,
    1395, 1460, 1528, 1597, 1669, 1743, 1819, 1898, 1979, 2063, 2150, 2239, 2331, 2426, 2524, 2626,
    2730, 2837, 2948, 3063, 3180, 3302, 3427, 3556, 3689, 3826, 3967, 4112, 4262, 4416, 4575, 4739,
    4907, 5081, 5260, 5444, 5633, 5828, 6029, 6235, 6448, 6667, 6892, 7124, 7362, 7608, 7860, 8120,
    8388, 8663, 8946, 9237, 9536, 9844, 10160, 10486, 10821, 11165, 11519, 11883, 12258, 12643,
    13038, 13445,
];

/// Base monster accuracy by level.
pub const MONSTER_ACCURACY_TABLE: [u32; 100] = [
    14, 15, 15, 16, 17, 18, 19, 20, 21, 23, 24, 25, 26, 28, 29, 31, 32, 34, 35, 37, 39, 41, 43, 45,
    47, 49, 52, 54, 57, 59, 62, 65, 68, 71, 74, 77, 81, 84, 88, 92, 96, 100, 105, 109, 114, 119,
    124, 129, 135, 140, 146, 152, 159, 165, 172, 179, 187, 195, 203, 211, 220, 229, 238, 247, 257,
    268, 279, 290, 301, 314, 326, 339, 352, 366, 381, 396, 412, 428, 444, 462, 480, 499, 518, 538,
    559, 580, 603, 626, 650, 675, 701, 728, 755, 784, 814, 845, 877, 910, 945, 980,
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

/// Player-ally life lookup. Used for most summoned minions / totems.
#[must_use]
pub fn monster_damage_at_level(level: u32) -> f32 {
    let idx = clamp_level_index(level);
    MONSTER_DAMAGE_TABLE[idx]
}

/// Player-ally damage variant. Used for most summoned minions / totems' damage.
#[must_use]
pub fn monster_ally_damage_at_level(level: u32) -> f32 {
    let idx = clamp_level_index(level);
    MONSTER_ALLY_DAMAGE_TABLE[idx]
}

/// Per-level monster armour lookup.
#[must_use]
pub fn monster_armour_at_level(level: u32) -> u32 {
    let idx = clamp_level_index(level);
    MONSTER_ARMOUR_TABLE[idx]
}

/// Per-level monster evasion lookup.
#[must_use]
pub fn monster_evasion_at_level(level: u32) -> u32 {
    let idx = clamp_level_index(level);
    MONSTER_EVASION_TABLE[idx]
}

/// Per-level monster accuracy lookup.
#[must_use]
pub fn monster_accuracy_at_level(level: u32) -> u32 {
    let idx = clamp_level_index(level);
    MONSTER_ACCURACY_TABLE[idx]
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

    #[test]
    fn damage_armour_evasion_accuracy_tables_have_100_entries() {
        assert_eq!(MONSTER_DAMAGE_TABLE.len(), 100);
        assert_eq!(MONSTER_ALLY_DAMAGE_TABLE.len(), 100);
        assert_eq!(MONSTER_ARMOUR_TABLE.len(), 100);
        assert_eq!(MONSTER_EVASION_TABLE.len(), 100);
        assert_eq!(MONSTER_ACCURACY_TABLE.len(), 100);
    }

    #[test]
    fn integer_stat_tables_are_monotonic_non_decreasing() {
        for table in [
            &MONSTER_ARMOUR_TABLE,
            &MONSTER_EVASION_TABLE,
            &MONSTER_ACCURACY_TABLE,
        ] {
            for w in table.windows(2) {
                assert!(w[1] >= w[0], "table not monotonic: {} → {}", w[0], w[1]);
            }
        }
    }

    #[test]
    fn float_stat_tables_are_monotonic_non_decreasing() {
        for table in [&MONSTER_DAMAGE_TABLE, &MONSTER_ALLY_DAMAGE_TABLE] {
            for w in table.windows(2) {
                assert!(w[1] >= w[0], "table not monotonic: {} → {}", w[0], w[1]);
            }
        }
    }

    #[test]
    fn extended_lookups_match_canonical_pob_values() {
        // Spot-check pinned values from PoB's `Data/Misc.lua`. PoB rounded to a few
        // decimals so we accept ~0.05 tolerance on the f32 helpers.
        // 1-indexed PoB lookups: level L → MK2's `[L - 1]`.
        // monsterDamageTable[1] = 4.99; [70] = 413.01.
        assert!((monster_damage_at_level(1) - 4.99).abs() < 0.05);
        assert!((monster_damage_at_level(70) - 413.01).abs() < 0.5);
        // monsterAllyDamageTable[1] = 5.62; [90] = 953.54.
        assert!((monster_ally_damage_at_level(1) - 5.62).abs() < 0.05);
        assert!((monster_ally_damage_at_level(90) - 953.54).abs() < 1.0);
        // monsterArmourTable[1] = 12; [70] = 14047.
        assert_eq!(monster_armour_at_level(1), 12);
        assert_eq!(monster_armour_at_level(70), 14047);
        // monsterEvasionTable[1] = 67; [90] = 9844.
        assert_eq!(monster_evasion_at_level(1), 67);
        assert_eq!(monster_evasion_at_level(90), 9844);
        // monsterAccuracyTable[1] = 14; [90] = 675.
        assert_eq!(monster_accuracy_at_level(1), 14);
        assert_eq!(monster_accuracy_at_level(90), 675);
    }
}
