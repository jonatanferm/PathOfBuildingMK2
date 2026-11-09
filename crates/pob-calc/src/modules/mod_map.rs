//! @pob-file Data/ModMap.lua @16de4b82d
//! Data/ModMap.lua: map mod `apply` closures and the `MODMAP_APPLY` registry (U0 frame, calc doc §8.1).
//! The caller (ConfigOptions.lua 117-130 `mapAffixDropDownFunction`) picks one of three call shapes from
//! `affixData.type`; each body is ported against the shape it actually receives.
//!
//! Owner: CO-C. Signatures and `ROWS` are U0's (frozen); the bodies are CO-C's port.
//!
//! The frozen signatures name the Lua parameters `val` (the tier for `list`/`count`) and
//! `rollRange` (`count`) by their call-site meaning (`tier`, `count`); each body rebinds the Lua
//! names first so the statements read as in the Lua. `values[val]` indexes the caller's
//! `affixData.values` table: an absent tier (the tier dropdown at "None", or out of range) reads
//! nil and the arithmetic raises Lua's error, as in PoB.
#![allow(non_snake_case, unused_variables, clippy::all, clippy::pedantic)]

use crate::prelude::*;

use pob_gamedata::tag;
use ModType::*;

/// `t[k]` for a numeric key: Lua reads nil for a non-integral or NaN key (only indexing a
/// non-table raises).
fn at(values: &Table, k: f64) -> Val {
    if k.is_finite() && k == k.trunc() {
        values.get(k)
    } else {
        Val::Nil
    }
}

/// A value in arithmetic (`nil`/non-numbers raise "attempt to perform arithmetic on …").
fn v(x: &Val) -> f64 {
    x.num()
}

/// The three call shapes of map mod applies (calc doc §8.1).
#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
pub enum ModMapApply {
    check(fn(val: &Val, mapModEffect: f64, modList: &ModList, enemyModList: &ModList)),
    list(
        fn(tier: f64, mapModEffect: f64, values: &Table, modList: &ModList, enemyModList: &ModList),
    ),
    count(
        fn(
            tier: f64,
            count: f64,
            mapModEffect: f64,
            values: &Table,
            modList: &ModList,
            enemyModList: &ModList,
        ),
    ),
}

pub use crate::data_fns::FnRow;

/// `data.mapMods.AffixData[k].apply` by closure id.
pub static MODMAP_APPLY: std::sync::LazyLock<
    ahash::AHashMap<&'static str, &'static FnRow<ModMapApply>>,
> = std::sync::LazyLock::new(|| ROWS.iter().map(|r| (r.id, r)).collect());

/// Every closure of this file: id (closures.json), sha256 of its source lines, Rust body.
pub static ROWS: &[FnRow<ModMapApply>] = &[
    FnRow {
        id: "data.mapMods.AffixData.Armoured.apply",
        hash: "818c41fa87bd12cac6f6fbc1a75aa0669108cf78175096fccb6c67b0d7e61721",
        f: ModMapApply::list(Armoured_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData.Hexproof.apply",
        hash: "b2003d96e3ca0ad272d846086b483bb499c75c33bba2846e02286fa44dfcff69",
        f: ModMapApply::check(Hexproof_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData.Hexwarded.apply",
        hash: "f6d6f8ff58fbdca3b939d90a247b23b4fab890fdc3d5ef1ef8ba052ac1153b1a",
        f: ModMapApply::list(Hexwarded_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData.Resistant.apply",
        hash: "4c39240b74d57c4f0a12ca033b84f700c18d767c9e5f43f1109342aec387bf12",
        f: ModMapApply::list(Resistant_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData.Unwavering.apply",
        hash: "9a5edb6e09eaa1dbc585bc6bd852702cb01275c0fd5b5f15a8abb3ced04ebb57",
        f: ModMapApply::count(Unwavering_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData.Fecund.apply",
        hash: "2499e4e54cf353ab108a2a7bcc4e9356b1ec075ceaa10b5d5274d4bc7f1dee4f",
        f: ModMapApply::count(Fecund_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData.Unstoppable.apply",
        hash: "d6eeb086a1e7fe3c4d11f6cf1de739dc35f9406ccb5bff86794d9a304a7676ad",
        f: ModMapApply::check(Unstoppable_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData.Impervious.apply",
        hash: "0bdb7b6cf9b33614eee9071eb907c6cd3a74ca85e0aa1cded384fc985aca0c62",
        f: ModMapApply::list(Impervious_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData.Oppressive.apply",
        hash: "433c9530b060d7ea33de063182a0409dbcd6d0cbef92955cc49aab2fa44506f7",
        f: ModMapApply::list(Oppressive_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData.Buffered.apply",
        hash: "71fb5806f1c4799d7a688c8358c1d2893d2673939ac6d58084791c25fc95b7ef",
        f: ModMapApply::count(Buffered_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData[\"Titan's\"].apply",
        hash: "250a49a257b3d15d688677b4edb40cd63d6cc1e1a393c5953b62cceed2816ef6",
        f: ModMapApply::list(data_mapMods_AffixData_Titan_s_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData.Savage.apply",
        hash: "190b6698d0934ea46b9a81e8f7e84e046114a2f37f47b72d459a5bf3a5cc161e",
        f: ModMapApply::count(Savage_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData.Burning.apply",
        hash: "294f42d630dc64fa11726899643c8835cdea0e0e2ac88455195fbeb3551c45df",
        f: ModMapApply::count(Burning_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData.Freezing.apply",
        hash: "393cc0c7eb5315306ad4b64b2136ccfaac293d3d304df836077cd383cafcc73a",
        f: ModMapApply::count(Freezing_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData.Shocking.apply",
        hash: "96dc8e5d295dd132cfaec9f7f0d7e41662eff775bf021bf80fb522c69f2c0ab0",
        f: ModMapApply::count(Shocking_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData.Profane.apply",
        hash: "02a7c50bb8639489a3675622afa48d0c8cc8b74097e988619a6474efc639a392",
        f: ModMapApply::count(Profane_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData.Fleet.apply",
        hash: "a3ad68ab3a97e5bc946578cbf65bf62715f06dd70ae6d2f90dc34de6ddefc7b6",
        f: ModMapApply::count(Fleet_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData.Conflagrating.apply",
        hash: "3b142ef4b69f6bfdf15d3bdf52ab79ade50dfbf712af61931409387e674f3250",
        f: ModMapApply::check(Conflagrating_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData.Impaling.apply",
        hash: "b250fe9547c8848044725422d371cae6684e116af30f8d1474797be9d6449a20",
        f: ModMapApply::list(Impaling_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData.Empowered.apply",
        hash: "adb70f0b03c7bdf728153c0625b823d9d9b7e07b9965099adfa4208bc9741e3a",
        f: ModMapApply::list(Empowered_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData[\"Overlord's\"].apply",
        hash: "6c9f71a497af6b859ec862e80d02c91384e80c390da1042c99ff0b906de1f943",
        f: ModMapApply::list(data_mapMods_AffixData_Overlord_s_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData.Punishing.apply",
        hash: "db17ed70c654b159acfe392be812363f5ff82659b164962d346e7e7a5fb957e1",
        f: ModMapApply::list(Punishing_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData.Mirrored.apply",
        hash: "db17ed70c654b159acfe392be812363f5ff82659b164962d346e7e7a5fb957e1",
        f: ModMapApply::list(Mirrored_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData[\"of Balance\"].apply",
        hash: "9cf01e0006c0f257d9c72cb48db9e7be5494ce9652daeb54b5157626317d582d",
        f: ModMapApply::check(data_mapMods_AffixData_of_Balance_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData[\"of Congealment\"].apply",
        hash: "c604ff72ec4d448ec8a29e4163ff3699b3540b8a82f8ad347f756abb2b349c7e",
        f: ModMapApply::check(data_mapMods_AffixData_of_Congealment_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData[\"of Drought\"].apply",
        hash: "0197215368725e64c7f6b763de5e85ff87122baba6255fcad408cd25c7e2ab3e",
        f: ModMapApply::list(data_mapMods_AffixData_of_Drought_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData[\"of Exposure\"].apply",
        hash: "2e295018661bc9e6bd279a4e7499be33584cf6970143bbc83b5d38e32047f6ef",
        f: ModMapApply::count(data_mapMods_AffixData_of_Exposure_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData[\"of Impotence\"].apply",
        hash: "3702fdea037a02f9c7d65172696fdb96e0747408dbc1fb1b75475278feeef004",
        f: ModMapApply::list(data_mapMods_AffixData_of_Impotence_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData[\"of Insulation\"].apply",
        hash: "56b5abc314ae5fc025a3cbd2f8e0694f172b4a6205df2e879f794b48c567d95d",
        f: ModMapApply::list(data_mapMods_AffixData_of_Insulation_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData[\"of Miring\"].apply",
        hash: "d4562915baf81d3685db4a954441ee8f961052b17ca61617d8112f3639c25232",
        f: ModMapApply::list(data_mapMods_AffixData_of_Miring_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData[\"of Rust\"].apply",
        hash: "8381aacb8d2da2e1f3537007824a18626a333000d29fd0592e9fa801ce6253e6",
        f: ModMapApply::list(data_mapMods_AffixData_of_Rust_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData[\"of Smothering\"].apply",
        hash: "f47c05a2bcd2f4b133df5219028f4ee27876dc6e0167f278b0fccc0e1ee15feb",
        f: ModMapApply::list(data_mapMods_AffixData_of_Smothering_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData[\"of Stasis\"].apply",
        hash: "315d1c21b867e21ec1ad1b50d9c636ad74bb92203583727f8e0596376ef94d4e",
        f: ModMapApply::check(data_mapMods_AffixData_of_Stasis_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData[\"of Toughness\"].apply",
        hash: "6894cf71d5e7e68f642da5f1162d880b65aa815c91c9a0f37f5029769b6809b4",
        f: ModMapApply::count(data_mapMods_AffixData_of_Toughness_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData[\"of Fatigue\"].apply",
        hash: "ef8f4f623031da0d505600bcf238b65e9db244a030c30ff6ad38888730ee3d1c",
        f: ModMapApply::list(data_mapMods_AffixData_of_Fatigue_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData[\"of Transience\"].apply",
        hash: "db17ed70c654b159acfe392be812363f5ff82659b164962d346e7e7a5fb957e1",
        f: ModMapApply::list(data_mapMods_AffixData_of_Transience_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData[\"of Doubt\"].apply",
        hash: "6d593d666f8fe51b118df8d6cab42966eb8f5b434c0b9981e1576bf99943f99e",
        f: ModMapApply::list(data_mapMods_AffixData_of_Doubt_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData[\"of Imprecision\"].apply",
        hash: "e3dd033cef23f4731d402b06fe3903c55a12ce6f164facab3d9dd17bf7431c83",
        f: ModMapApply::list(data_mapMods_AffixData_of_Imprecision_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData[\"of Blinding\"].apply",
        hash: "37e6755b9c89f84b7461b4ddbfb74a52bfb2419f7122c049328e97de10875209",
        f: ModMapApply::check(data_mapMods_AffixData_of_Blinding_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData[\"of Venom\"].apply",
        hash: "473f89de9d1052169a7fb206b69b5c16a94c17a5d120de414c82a2f5942844c7",
        f: ModMapApply::check(data_mapMods_AffixData_of_Venom_apply),
    },
    FnRow {
        id: "data.mapMods.AffixData[\"of Deadliness\"].apply",
        hash: "492527040dd43a3b391db710b9f1b3bd68a9d0ccfcf921370bd27ad600531adb",
        f: ModMapApply::count(data_mapMods_AffixData_of_Deadliness_apply),
    },
];

// @pob-skip Data/ModMap.lua:4-11 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:12-14 h=42d5e04d
/// `data.mapMods.AffixData.Armoured.apply`
pub(crate) fn Armoured_apply(
    tier: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
                    // L13
    enemyModList.NewMod(
        n!("PhysicalDamageReduction"),
        BASE,
        v(&at(values, val)) * mapModEffect,
        "Map mod Armoured",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:15-19 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:20-22 h=a8e451fe
/// `data.mapMods.AffixData.Hexproof.apply`
pub(crate) fn Hexproof_apply(
    val: &Val,
    mapModEffect: f64,
    modList: &ModList,
    enemyModList: &ModList,
) {
    // L21
    enemyModList.NewMod(n!("Hexproof"), FLAG, true, "Map mod Hexproof", 0, 0, &[]);
}

// @pob-skip Data/ModMap.lua:23-28 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:29-31 h=c5f4e672
/// `data.mapMods.AffixData.Hexwarded.apply`
pub(crate) fn Hexwarded_apply(
    tier: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
                    // L30
    enemyModList.NewMod(
        n!("CurseEffectOnSelf"),
        MORE,
        -v(&at(values, val)) * mapModEffect,
        "Map mod Hexwarded",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:32-37 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:38-41 h=a2d70afd
/// `data.mapMods.AffixData.Resistant.apply`
pub(crate) fn Resistant_apply(
    tier: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
                    // L39
    enemyModList.NewMod(
        n!("ElementalResist"),
        BASE,
        v(&at(values, val).get(1.0)) * mapModEffect,
        "Map mod Resistant",
        0,
        0,
        &[],
    );
    // L40
    enemyModList.NewMod(
        n!("ChaosResist"),
        BASE,
        v(&at(values, val).get(2.0)) * mapModEffect,
        "Map mod Resistant",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:42-46 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:47-50 h=e59c0a15
/// `data.mapMods.AffixData.Unwavering.apply`
pub(crate) fn Unwavering_apply(
    tier: f64,
    count: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
    let rollRange = count; // Lua's second parameter is `rollRange`
                           // L48
    enemyModList.NewMod(
        n!("AvoidStun"),
        BASE,
        100.0,
        "Map mod Unwavering",
        0,
        0,
        &[],
    );
    // L49
    enemyModList.NewMod(
        n!("Life"),
        MORE,
        (v(&at(values, val).get(1.0).get(1.0))
            + (v(&at(values, val).get(1.0).get(2.0)) - v(&at(values, val).get(1.0).get(1.0)))
                * rollRange
                / 100.0)
            * mapModEffect,
        "Map mod Unwavering",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:51-55 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:56-58 h=b1bfe6e1
/// `data.mapMods.AffixData.Fecund.apply`
pub(crate) fn Fecund_apply(
    tier: f64,
    count: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
    let rollRange = count; // Lua's second parameter is `rollRange`
                           // L57
    enemyModList.NewMod(
        n!("Life"),
        MORE,
        (v(&at(values, val).get(1.0))
            + (v(&at(values, val).get(2.0)) - v(&at(values, val).get(1.0))) * rollRange / 100.0)
            * mapModEffect,
        "Map mod Fecund",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:59-63 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:64-67 h=049bee4b
/// `data.mapMods.AffixData.Unstoppable.apply`
pub(crate) fn Unstoppable_apply(
    val: &Val,
    mapModEffect: f64,
    modList: &ModList,
    enemyModList: &ModList,
) {
    // MISSING: Monsters cannot be Taunted
    // L66
    enemyModList.NewMod(
        n!("MinimumActionSpeed"),
        MAX,
        100.0,
        "Map mod Unstoppable",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:68-72 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:73-77 h=249e2011
/// `data.mapMods.AffixData.Impervious.apply`
pub(crate) fn Impervious_apply(
    tier: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
                    // L74
    enemyModList.NewMod(
        n!("AvoidPoison"),
        BASE,
        v(&at(values, val)) * mapModEffect,
        "Map mod Impervious",
        0,
        0,
        &[],
    );
    // L75
    enemyModList.NewMod(
        n!("AvoidImpale"),
        BASE,
        v(&at(values, val)) * mapModEffect,
        "Map mod Impervious",
        0,
        0,
        &[],
    );
    // L76
    enemyModList.NewMod(
        n!("AvoidBleed"),
        BASE,
        v(&at(values, val)) * mapModEffect,
        "Map mod Impervious",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:78-82 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:83-85 h=09008a52
/// `data.mapMods.AffixData.Oppressive.apply`
pub(crate) fn Oppressive_apply(
    tier: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
                    // L84
    enemyModList.NewMod(
        n!("SpellSuppressionChance"),
        BASE,
        v(&at(values, val)) * mapModEffect,
        "Map mod Oppressive",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:86-90 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:91-93 h=f89e9b24
/// `data.mapMods.AffixData.Buffered.apply`
pub(crate) fn Buffered_apply(
    tier: f64,
    count: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
    let rollRange = count; // Lua's second parameter is `rollRange`
                           // L92
    enemyModList.NewMod(
        n!("LifeGainAsEnergyShield"),
        BASE,
        (v(&at(values, val).get(1.0))
            + (v(&at(values, val).get(2.0)) - v(&at(values, val).get(1.0))) * rollRange / 100.0)
            * mapModEffect,
        "Map mod Buffered",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:94-98 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:99-102 h=d2a43e52
/// `data.mapMods.AffixData["Titan's"].apply`
pub(crate) fn data_mapMods_AffixData_Titan_s_apply(
    tier: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
                    // L100
    enemyModList.NewMod(
        n!("Life"),
        MORE,
        v(&at(values, val).get(1.0)) * mapModEffect,
        "Map mod Titan's",
        0,
        0,
        &[tag!({ type = Condition, var = "RareOrUnique" })],
    );
    // L101
    enemyModList.NewMod(
        n!("AreaOfEffect"),
        INC,
        v(&at(values, val).get(2.0)) * mapModEffect,
        "Map mod Titan's",
        0,
        0,
        &[tag!({ type = Condition, var = "RareOrUnique" })],
    );
}

// @pob-skip Data/ModMap.lua:103-108 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:109-111 h=e104d13f
/// `data.mapMods.AffixData.Savage.apply`
pub(crate) fn Savage_apply(
    tier: f64,
    count: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
    let rollRange = count; // Lua's second parameter is `rollRange`
                           // L110
    enemyModList.NewMod(
        n!("Damage"),
        INC,
        (v(&at(values, val).get(1.0))
            + (v(&at(values, val).get(2.0)) - v(&at(values, val).get(1.0))) * rollRange / 100.0)
            * mapModEffect,
        "Map mod Savage",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:112-116 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:117-119 h=6281427c
/// `data.mapMods.AffixData.Burning.apply`
pub(crate) fn Burning_apply(
    tier: f64,
    count: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
    let rollRange = count; // Lua's second parameter is `rollRange`
                           // L118
    enemyModList.NewMod(
        n!("PhysicalDamageGainAsFire"),
        BASE,
        (v(&at(values, val).get(1.0))
            + (v(&at(values, val).get(2.0)) - v(&at(values, val).get(1.0))) * rollRange / 100.0)
            * mapModEffect,
        "Map mod Burning",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:120-124 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:125-127 h=02ab9af9
/// `data.mapMods.AffixData.Freezing.apply`
pub(crate) fn Freezing_apply(
    tier: f64,
    count: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
    let rollRange = count; // Lua's second parameter is `rollRange`
                           // L126
    enemyModList.NewMod(
        n!("PhysicalDamageGainAsCold"),
        BASE,
        (v(&at(values, val).get(1.0))
            + (v(&at(values, val).get(2.0)) - v(&at(values, val).get(1.0))) * rollRange / 100.0)
            * mapModEffect,
        "Map mod Freezing",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:128-132 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:133-135 h=9dd1a78b
/// `data.mapMods.AffixData.Shocking.apply`
pub(crate) fn Shocking_apply(
    tier: f64,
    count: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
    let rollRange = count; // Lua's second parameter is `rollRange`
                           // L134
    enemyModList.NewMod(
        n!("PhysicalDamageGainAsLightning"),
        BASE,
        (v(&at(values, val).get(1.0))
            + (v(&at(values, val).get(2.0)) - v(&at(values, val).get(1.0))) * rollRange / 100.0)
            * mapModEffect,
        "Map mod Shocking",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:136-140 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:141-144 h=15b4ff07
/// `data.mapMods.AffixData.Profane.apply`
pub(crate) fn Profane_apply(
    tier: f64,
    count: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
    let rollRange = count; // Lua's second parameter is `rollRange`
                           // L142
    enemyModList.NewMod(
        n!("PhysicalDamageGainAsChaos"),
        BASE,
        (v(&at(values, val).get(1.0).get(1.0))
            + (v(&at(values, val).get(1.0).get(2.0)) - v(&at(values, val).get(1.0).get(1.0)))
                * rollRange
                / 100.0)
            * mapModEffect,
        "Map mod Profane",
        0,
        0,
        &[],
    );
    // L143
    modList.NewMod(
        n!("Condition:CanBeWithered"),
        FLAG,
        true,
        "Map mod Profane",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:145-149 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:150-153 h=955ead35
/// `data.mapMods.AffixData.Fleet.apply`
pub(crate) fn Fleet_apply(
    tier: f64,
    count: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
    let rollRange = count; // Lua's second parameter is `rollRange`
                           // attack and cast is the same so applying it once, movespeed does not matter
                           // L152
    enemyModList.NewMod(
        n!("Speed"),
        INC,
        (v(&at(values, val).get(2.0).get(1.0))
            + (v(&at(values, val).get(2.0).get(2.0)) - v(&at(values, val).get(2.0).get(1.0)))
                * rollRange
                / 100.0)
            * mapModEffect,
        "Map mod Fleet",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:154-157 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:158-161 h=45930cc1
/// `data.mapMods.AffixData.Conflagrating.apply`
pub(crate) fn Conflagrating_apply(
    val: &Val,
    mapModEffect: f64,
    modList: &ModList,
    enemyModList: &ModList,
) {
    // L159
    enemyModList.NewMod(
        n!("IgniteChance"),
        BASE,
        100.0,
        "Map mod Conflagrating",
        0,
        0,
        &[],
    );
    // L160
    enemyModList.NewMod(
        n!("AllDamageIgnites"),
        FLAG,
        true,
        "Map mod Conflagrating",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:162-166 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:167-169 h=8727d4c8
/// `data.mapMods.AffixData.Impaling.apply`
pub(crate) fn Impaling_apply(
    tier: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
                    // L168
    enemyModList.NewMod(
        n!("ImpaleChance"),
        BASE,
        v(&at(values, val)) * mapModEffect,
        "Map mod Impaling",
        ModFlag::Attack,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:170-174 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:175-177 h=2b6c2334
/// `data.mapMods.AffixData.Empowered.apply`
pub(crate) fn Empowered_apply(
    tier: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
                    // L176
    enemyModList.NewMod(
        n!("ElementalAilmentChance"),
        BASE,
        v(&at(values, val)) * mapModEffect,
        "Map mod Empowered",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:178-182 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:183-186 h=24df59ca
/// `data.mapMods.AffixData["Overlord's"].apply`
pub(crate) fn data_mapMods_AffixData_Overlord_s_apply(
    tier: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
                    // L184
    enemyModList.NewMod(
        n!("Damage"),
        INC,
        v(&at(values, val).get(1.0)) * mapModEffect,
        "Map mod Overlord's",
        0,
        0,
        &[tag!({ type = Condition, var = "RareOrUnique" })],
    );
    // L185
    enemyModList.NewMod(
        n!("Speed"),
        INC,
        v(&at(values, val).get(2.0)) * mapModEffect,
        "Map mod Overlord's",
        0,
        0,
        &[tag!({ type = Condition, var = "RareOrUnique" })],
    );
}

// @pob-skip Data/ModMap.lua:187-192 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:193-194 h=ea22a839
/// `data.mapMods.AffixData.Punishing.apply`
pub(crate) fn Punishing_apply(
    tier: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
                    // (empty body)
}

// @pob-skip Data/ModMap.lua:195-199 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:200-201 h=ea22a839
/// `data.mapMods.AffixData.Mirrored.apply`
pub(crate) fn Mirrored_apply(
    tier: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
                    // (empty body)
}

// @pob-skip Data/ModMap.lua:202-207 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:208-211 h=dba63669
/// `data.mapMods.AffixData["of Balance"].apply`
pub(crate) fn data_mapMods_AffixData_of_Balance_apply(
    val: &Val,
    mapModEffect: f64,
    modList: &ModList,
    enemyModList: &ModList,
) {
    // Players cannot inflict Exposure
    // modList:NewMod("Keystone", "LIST", "Elemental Equilibrium", "Map mod of Balance") -- OLD MOD
}

// @pob-skip Data/ModMap.lua:212-216 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:217-221 h=7d9d3e45
/// `data.mapMods.AffixData["of Congealment"].apply`
pub(crate) fn data_mapMods_AffixData_of_Congealment_apply(
    val: &Val,
    mapModEffect: f64,
    modList: &ModList,
    enemyModList: &ModList,
) {
    // L218
    enemyModList.NewMod(
        n!("CannotLeechLifeFromSelf"),
        FLAG,
        true,
        "Map mod of Congealment",
        0,
        0,
        &[],
    );
    // L219
    enemyModList.NewMod(
        n!("CannotLeechManaFromSelf"),
        FLAG,
        true,
        "Map mod of Congealment",
        0,
        0,
        &[],
    );
    // L220
    enemyModList.NewMod(
        n!("CannotLeechEnergyShieldFromSelf"),
        FLAG,
        true,
        "Map mod of Congealment",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:222-227 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:228-230 h=e37d2d14
/// `data.mapMods.AffixData["of Drought"].apply`
pub(crate) fn data_mapMods_AffixData_of_Drought_apply(
    tier: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
                    // L229
    modList.NewMod(
        n!("FlaskChargesGained"),
        INC,
        -v(&at(values, val)) * mapModEffect,
        "Map mod of Drought",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:231-237 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:238-246 h=b25ad577
/// `data.mapMods.AffixData["of Exposure"].apply`
pub(crate) fn data_mapMods_AffixData_of_Exposure_apply(
    tier: f64,
    count: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
    let rollRange = count; // Lua's second parameter is `rollRange`
                           // L239
    if !at(values, val).get(2.0).raw_eq(&Val::Num(0.0)) {
        // L240
        let roll = (v(&at(values, val).get(1.0))
            + (v(&at(values, val).get(2.0)) - v(&at(values, val).get(1.0))) * rollRange / 100.0)
            * mapModEffect;
        // L241
        modList.NewMod(
            n!("FireResistMax"),
            BASE,
            -roll,
            "Map mod of Exposure",
            0,
            0,
            &[],
        );
        // L242
        modList.NewMod(
            n!("ColdResistMax"),
            BASE,
            -roll,
            "Map mod of Exposure",
            0,
            0,
            &[],
        );
        // L243
        modList.NewMod(
            n!("LightningResistMax"),
            BASE,
            -roll,
            "Map mod of Exposure",
            0,
            0,
            &[],
        );
        // L244
        modList.NewMod(
            n!("ChaosResistMax"),
            BASE,
            -roll,
            "Map mod of Exposure",
            0,
            0,
            &[],
        );
    }
}

// @pob-skip Data/ModMap.lua:247-252 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:253-255 h=4756e664
/// `data.mapMods.AffixData["of Impotence"].apply`
pub(crate) fn data_mapMods_AffixData_of_Impotence_apply(
    tier: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
                    // L254
    modList.NewMod(
        n!("AreaOfEffect"),
        MORE,
        -v(&at(values, val)) * mapModEffect,
        "Map mod of Impotence",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:256-261 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:262-264 h=1a563e24
/// `data.mapMods.AffixData["of Insulation"].apply`
pub(crate) fn data_mapMods_AffixData_of_Insulation_apply(
    tier: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
                    // L263
    enemyModList.NewMod(
        n!("AvoidElementalAilments"),
        BASE,
        v(&at(values, val)) * mapModEffect,
        "Map mod of Insulation",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:265-270 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:271-275 h=e8f1c5a0
/// `data.mapMods.AffixData["of Miring"].apply`
pub(crate) fn data_mapMods_AffixData_of_Miring_apply(
    tier: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
                    // modList:NewMod("DodgeChanceIsUnlucky", "FLAG", true, "Map mod of Miring") -- OLD MOD
                    // L273
    modList.NewMod(
        n!("SpellSuppressionEffect"),
        BASE,
        -v(&at(values, val).get(1.0)) * mapModEffect,
        "Map mod of Miring",
        0,
        0,
        &[],
    );
    // L274
    enemyModList.NewMod(
        n!("Accuracy"),
        INC,
        v(&at(values, val).get(2.0)) * mapModEffect,
        "Map mod of Miring",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:276-281 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:282-285 h=5413f41c
/// `data.mapMods.AffixData["of Rust"].apply`
pub(crate) fn data_mapMods_AffixData_of_Rust_apply(
    tier: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
                    // L283
    modList.NewMod(
        n!("BlockChance"),
        INC,
        -v(&at(values, val).get(1.0)) * mapModEffect,
        "Map mod of Rust",
        0,
        0,
        &[],
    );
    // L284
    modList.NewMod(
        n!("Armour"),
        MORE,
        -v(&at(values, val).get(2.0)) * mapModEffect,
        "Map mod of Rust",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:286-291 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:292-295 h=8ca4798b
/// `data.mapMods.AffixData["of Smothering"].apply`
pub(crate) fn data_mapMods_AffixData_of_Smothering_apply(
    tier: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
                    // L293
    modList.NewMod(
        n!("LifeRecoveryRate"),
        MORE,
        -v(&at(values, val)) * mapModEffect,
        "Map mod of Smothering",
        0,
        0,
        &[],
    );
    // L294
    modList.NewMod(
        n!("EnergyShieldRecoveryRate"),
        MORE,
        -v(&at(values, val)) * mapModEffect,
        "Map mod of Smothering",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:296-300 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:301-305 h=ad2057c7
/// `data.mapMods.AffixData["of Stasis"].apply`
pub(crate) fn data_mapMods_AffixData_of_Stasis_apply(
    val: &Val,
    mapModEffect: f64,
    modList: &ModList,
    enemyModList: &ModList,
) {
    // L302
    modList.NewMod(
        n!("NoLifeRegen"),
        FLAG,
        true,
        "Map mod of Stasis",
        0,
        0,
        &[],
    );
    // L303
    modList.NewMod(
        n!("NoEnergyShieldRegen"),
        FLAG,
        true,
        "Map mod of Stasis",
        0,
        0,
        &[],
    );
    // L304
    modList.NewMod(
        n!("NoManaRegen"),
        FLAG,
        true,
        "Map mod of Stasis",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:306-310 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:311-313 h=e50e0ba8
/// `data.mapMods.AffixData["of Toughness"].apply`
pub(crate) fn data_mapMods_AffixData_of_Toughness_apply(
    tier: f64,
    count: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
    let rollRange = count; // Lua's second parameter is `rollRange`
                           // L312
    enemyModList.NewMod(
        n!("SelfCritMultiplier"),
        INC,
        -(v(&at(values, val).get(1.0))
            + (v(&at(values, val).get(2.0)) - v(&at(values, val).get(1.0))) * rollRange / 100.0)
            * mapModEffect,
        "Map mod of Toughness",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:314-318 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:319-321 h=5e62575a
/// `data.mapMods.AffixData["of Fatigue"].apply`
pub(crate) fn data_mapMods_AffixData_of_Fatigue_apply(
    tier: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
                    // L320
    modList.NewMod(
        n!("CooldownRecovery"),
        MORE,
        -v(&at(values, val)) * mapModEffect,
        "Map mod of Fatigue",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:322-326 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:327-328 h=ea22a839
/// `data.mapMods.AffixData["of Transience"].apply`
pub(crate) fn data_mapMods_AffixData_of_Transience_apply(
    tier: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
                    // (empty body)
}

// @pob-skip Data/ModMap.lua:329-333 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:334-336 h=8d1d9287
/// `data.mapMods.AffixData["of Doubt"].apply`
pub(crate) fn data_mapMods_AffixData_of_Doubt_apply(
    tier: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
                    // L335
    modList.NewMod(
        n!("AuraEffect"),
        INC,
        -v(&at(values, val)) * mapModEffect,
        "Map mod of Doubt",
        0,
        0,
        &[
            tag!({ type = SkillType, skillType = SkillType::Aura }),
            tag!({ type = SkillType, skillType = SkillType::AppliesCurse, neg = true }),
        ],
    );
}

// @pob-skip Data/ModMap.lua:337-341 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:342-344 h=ef8f52ab
/// `data.mapMods.AffixData["of Imprecision"].apply`
pub(crate) fn data_mapMods_AffixData_of_Imprecision_apply(
    tier: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
                    // L343
    modList.NewMod(
        n!("Accuracy"),
        MORE,
        -v(&at(values, val)) * mapModEffect,
        "Map mod of Imprecision",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:345-348 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:349-351 h=395215db
/// `data.mapMods.AffixData["of Blinding"].apply`
pub(crate) fn data_mapMods_AffixData_of_Blinding_apply(
    val: &Val,
    mapModEffect: f64,
    modList: &ModList,
    enemyModList: &ModList,
) {
    //SHOULD THIS BE SUPPORTED? (as a flag to make "are you blinded" show on config?)
}

// @pob-skip Data/ModMap.lua:352-355 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:356-358 h=4bd8c9e0
/// `data.mapMods.AffixData["of Venom"].apply`
pub(crate) fn data_mapMods_AffixData_of_Venom_apply(
    val: &Val,
    mapModEffect: f64,
    modList: &ModList,
    enemyModList: &ModList,
) {
    // L357
    enemyModList.NewMod(
        n!("PoisonChance"),
        BASE,
        100.0,
        "Map mod of Venom",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:359-363 reason="map mod definition data (dumped: data().mapMods)"

/// @pob Data/ModMap.lua:364-367 h=88159f54
/// `data.mapMods.AffixData["of Deadliness"].apply`
pub(crate) fn data_mapMods_AffixData_of_Deadliness_apply(
    tier: f64,
    count: f64,
    mapModEffect: f64,
    values: &Table,
    modList: &ModList,
    enemyModList: &ModList,
) {
    let val = tier; // Lua's first parameter is `val`
    let rollRange = count; // Lua's second parameter is `rollRange`
                           // L365
    enemyModList.NewMod(
        n!("CritChance"),
        INC,
        (v(&at(values, val).get(1.0).get(1.0))
            + (v(&at(values, val).get(1.0).get(2.0)) - v(&at(values, val).get(1.0).get(1.0)))
                * rollRange
                / 100.0)
            * mapModEffect,
        "Map mod of Deadliness",
        0,
        0,
        &[],
    );
    // L366
    enemyModList.NewMod(
        n!("CritMultiplier"),
        BASE,
        (v(&at(values, val).get(2.0).get(1.0))
            + (v(&at(values, val).get(2.0).get(2.0)) - v(&at(values, val).get(2.0).get(1.0)))
                * rollRange
                / 100.0)
            * mapModEffect,
        "Map mod of Deadliness",
        0,
        0,
        &[],
    );
}

// @pob-skip Data/ModMap.lua:368-441 reason="map mod definition data (dumped: data().mapMods)"
