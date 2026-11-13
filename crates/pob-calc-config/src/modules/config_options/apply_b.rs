//! @pob-file Modules/ConfigOptions.lua @16de4b82d
//! ConfigOptions `apply` closures, Modules/ConfigOptions.lua 1189-2380 (calc doc §8).
//! The option definitions themselves are data (`data().configOptions`, `ConfigOptionDef`).
//!
//! Owner: CO-B. Bodies translated by scripts/port/lua2rs (unit T1:
//! `python3 -m lua2rs config --range 1189-2380`) and hand-fixed; the two closures T1 does not
//! translate (`enemyIsBoss`, `presetBossSkills`) are ported by hand, against the control-state
//! model of `build.configTab.varControls` below (CCR-0037, CCR-0038).
#![allow(
    non_snake_case,
    non_upper_case_globals,
    unused_variables,
    unused_mut,
    unused_parens,
    unused_imports,
    dead_code,
    clippy::all,
    clippy::pedantic
)]

use crate::prelude::*;

use super::{ConfigApply, FnRow};
use crate::classes::ModList;
use crate::lua_table::Val;
use crate::model::BuildRef;
use pob_gamedata::mod_tools::createMod;
use pob_gamedata::{
    data, tag, AStr, KeywordFlag, Mod, ModFlag, ModType, ModType::*, SkillType, Sockets, Tag,
    Value, ValueTable,
};
use pob_lua::bit::{band, bnot, bor};
use pob_lua::format::{tonumber, tostring_num};
use pob_lua::math::{lmod, lpow, m_abs, m_ceil, m_floor, m_huge, m_max, m_min, round};
use pob_lua::string::{firstToUpper, len, lower, sub, upper};
use pob_lua::{cat, lfmt, lua_error, n, nm, Name};
use std::sync::Arc;

/// Lua `<`/`<=` operand: numbers only (strings do not coerce in comparisons).
fn cmp_num(v: &Val) -> f64 {
    match v {
        Val::Num(x) => *x,
        _ => lua_error!("attempt to compare a non-number value with number"),
    }
}

/// Lua `val == <number>` (no coercion).
fn val_eq_num(v: &Val, n: f64) -> bool {
    matches!(v, Val::Num(x) if *x == n)
}

/// Lua `val == "..."` (no coercion).
fn val_eq_str(v: &Val, s: &str) -> bool {
    matches!(v, Val::Str(x) if &**x == s)
}

/// Lua `..` operand from a config value: numbers use tostring (%.14g), strings are kept.
fn val_str(v: &Val) -> String {
    match v {
        Val::Num(x) => tostring_num(*x),
        Val::Str(s) => s.to_string(),
        _ => lua_error!("attempt to concatenate a non-string value"),
    }
}

/// The config value `val` stored in a mod (Lua stores whatever value it is).
fn val_value(v: &Val) -> Value {
    match v {
        Val::Nil => Value::Nil,
        Val::Bool(b) => Value::Bool(*b),
        Val::Num(x) => Value::Num(*x),
        Val::Str(s) => Value::Str(AStr::from(&**s)),
        Val::Data(d) => d.clone(),
        Val::Mod(m) => Value::Mod(m.clone()),
        _ => lua_error!("lua2rs: a config value that is a mutable table cannot become a mod value"),
    }
}

// ---------------------------------------------------------------------------
// The UI controls `enemyIsBoss` / `presetBossSkills` drive (hand-port support).
//
// `build.configTab.varControls[var]` is the model's control-state table (CCR-0037: built by
// `ConfigTab::ctor_controls_model`); the control methods are model methods (CCR-0038:
// `ConfigTab::{varControl, SetPlaceholder, SelByValue}`). Indexing a missing control raises
// Lua's error, as in PoB.
// ---------------------------------------------------------------------------

/// `build.configTab.varControls[var]` (a missing control raises, as indexing nil does in Lua).
fn varControl(build: &BuildRef, var: Name) -> Table {
    build.configTab.varControl(var)
}

/// Lua `tostring(v)` of a data value.
fn tostring_value(v: &Value) -> String {
    match v {
        Value::Nil => "nil".to_owned(),
        Value::Bool(b) => {
            if *b {
                "true".to_owned()
            } else {
                "false".to_owned()
            }
        }
        Value::Num(x) => tostring_num(*x),
        Value::Str(s) => s.to_string(),
        _ => "table".to_owned(),
    }
}

/// `build.configTab.varControls[var]:SetPlaceholder(text, notify)` (`ConfigTab::SetPlaceholder`).
fn SetPlaceholder(build: &BuildRef, var: Name, text: Val, notify: bool) {
    build.configTab.SetPlaceholder(build, var, &text, notify);
}

/// `t[k]` for a dynamic key (a non-integral number or nil key reads nil).
fn index_val(t: &Val, k: &Val) -> Val {
    match k {
        Val::Num(x) if x.is_finite() && *x == x.trunc() => t.get(*x),
        Val::Str(s) => t.get(&**s),
        Val::Bool(b) => t.get(*b),
        _ => {
            // still raises when `t` cannot be indexed
            let _ = t.get(n!("selIndex"));
            Val::Nil
        }
    }
}

/// `build.configTab.varControls['enemyIsBoss'].list[build.configTab.varControls['enemyIsBoss'].selIndex].val`
fn enemyIsBoss_selVal(build: &BuildRef) -> Val {
    let list = varControl(build, n!("enemyIsBoss")).get(n!("list"));
    let selIndex = varControl(build, n!("enemyIsBoss")).get(n!("selIndex"));
    index_val(&list, &selIndex).get(n!("val"))
}

/// The argument check of `math.min` / `math.max` (numbers and numeric strings).
fn checknum(v: &Val, fname: &str) -> f64 {
    match v {
        Val::Num(x) => *x,
        Val::Str(s) => tonumber(s).unwrap_or_else(|| {
            lua_error!("bad argument #1 to '{fname}' (number expected, got string)")
        }),
        other => lua_error!(
            "bad argument #1 to '{fname}' (number expected, got {})",
            other.lua_type()
        ),
    }
}

/// `data.monster*Table[level]` (nil outside the array or for a non-integral level).
fn level_at(t: &[f64], level: f64) -> Val {
    if level >= 1.0 && level == level.trunc() && level <= t.len() as f64 {
        Val::Num(t[level as usize - 1])
    } else {
        Val::Nil
    }
}

/// `data.monster*Table[build.configTab.enemyLevel]` (`t[nil]` reads nil).
fn enemyLevel_at(t: &[f64], build: &BuildRef) -> Val {
    build
        .configTab
        .enemyLevel
        .get()
        .map_or(Val::Nil, |l| level_at(t, l))
}

/// `damageMult[i]` in arithmetic (nil raises).
fn vec_at(v: &[f64], i: usize) -> f64 {
    match v.get(i - 1) {
        Some(x) => *x,
        None => lua_error!("attempt to perform arithmetic on a nil value"),
    }
}

/// Every closure of this file: id (closures.json), sha256 of its source lines, Rust body.
pub static ROWS: &[FnRow<ConfigApply>] = &[
    FnRow {
        id: "ConfigOptions[393].apply",
        hash: "96c271abe205d1cd4d1fb5d2a1d8b7b15b6e77bd1c797a199ff5bb513d4fad50",
        f: opt_393_apply,
    },
    FnRow {
        id: "ConfigOptions[394].apply",
        hash: "81cfb38ec099f6f89d90ddafc976a3de125e92ed550ff0b3096bd380d3fe787c",
        f: opt_394_apply,
    },
    FnRow {
        id: "ConfigOptions[395].apply",
        hash: "9577e5372f9393acfc2e3669c079e979bc19aeeffdb1e8941fafdcb51dedd052",
        f: opt_395_apply,
    },
    FnRow {
        id: "ConfigOptions[396].apply",
        hash: "c89e3e6cb14e5e46753f0af0d1c5d7604be57b928bcc6047a5a5b206afa6bca5",
        f: opt_396_apply,
    },
    FnRow {
        id: "ConfigOptions[397].apply",
        hash: "6178e5019ea44e4890de873bd02473977bf14800ce0b987e9c7a8005a2d9774b",
        f: opt_397_apply,
    },
    FnRow {
        id: "ConfigOptions[398].apply",
        hash: "d6909e52d342aa85139106df3d6e0cdb954ce114cdf68bf902271072cc7fa7f2",
        f: opt_398_apply,
    },
    FnRow {
        id: "ConfigOptions[399].apply",
        hash: "e36b20df0eb4f7a32a21579c9cde05b04096d9e3c0c6e6c91407fb45f7b69ef3",
        f: opt_399_apply,
    },
    FnRow {
        id: "ConfigOptions[400].apply",
        hash: "bda75fb161b30af72c1fbf6efcc8bf5363d4b938e4d9987becb5e5b03e1094c3",
        f: opt_400_apply,
    },
    FnRow {
        id: "ConfigOptions[401].apply",
        hash: "922feec8e704c9d1fb67afc37d9d504f9877705b85c0fc41d959b01fc3071900",
        f: opt_401_apply,
    },
    FnRow {
        id: "ConfigOptions[402].apply",
        hash: "16e87f5edd59adf106128e67faf64903c3936d353b111c5a7f42f3902ae1f016",
        f: opt_402_apply,
    },
    FnRow {
        id: "ConfigOptions[403].apply",
        hash: "c4471396a3a3223675ed109785cee4a93dc9fa73164bdf057a924f8f8af97964",
        f: opt_403_apply,
    },
    FnRow {
        id: "ConfigOptions[404].apply",
        hash: "fd949f967332a10ac65231807f0f4ed0e5c1c9448776f38d4b5ed59f630ce751",
        f: opt_404_apply,
    },
    FnRow {
        id: "ConfigOptions[405].apply",
        hash: "e2011740746fba84af659ff383ecfb37a4bcb4e47c3a9cffbf15ac97163b9e90",
        f: opt_405_apply,
    },
    FnRow {
        id: "ConfigOptions[406].apply",
        hash: "3f291de1a92f86c5878eaab2ee6b7279953936948d6451c124f4f8b4637bbd7b",
        f: opt_406_apply,
    },
    FnRow {
        id: "ConfigOptions[407].apply",
        hash: "50d46c00b2fa06d5746b37751e2911cc692461d83bce46ced754a01eca8a7ff7",
        f: opt_407_apply,
    },
    FnRow {
        id: "ConfigOptions[408].apply",
        hash: "f1813139b17be8a71bdad003e411d6159f8b52a41fa6bd134fc410e13e80678a",
        f: opt_408_apply,
    },
    FnRow {
        id: "ConfigOptions[409].apply",
        hash: "fca45bcbac55a6c703c7e9bf3e63381d1f56e4565775e19b60ef850f53ef4b54",
        f: opt_409_apply,
    },
    FnRow {
        id: "ConfigOptions[410].apply",
        hash: "70e60b1d0292bf6c02d3092af20157c79dd9372830e39295e383a9bf82cf3cf3",
        f: opt_410_apply,
    },
    FnRow {
        id: "ConfigOptions[411].apply",
        hash: "6816d1df19e732880f2b655bb19e5054cde58888e9cde654bafce2caf59a808a",
        f: opt_411_apply,
    },
    FnRow {
        id: "ConfigOptions[412].apply",
        hash: "92ce5062a47e4219589d8d7fd5bae192a4d73c1e7fb433ed9987913762e1ad52",
        f: opt_412_apply,
    },
    FnRow {
        id: "ConfigOptions[413].apply",
        hash: "431b31e016e062fd3717d5786b26567a557d841bf261609fb418b8cfb355044e",
        f: opt_413_apply,
    },
    FnRow {
        id: "ConfigOptions[414].apply",
        hash: "75b346ed2021fd4fabbbdc2286a5548f1713b25eeca43baad02b0ccc08c2c2d0",
        f: opt_414_apply,
    },
    FnRow {
        id: "ConfigOptions[415].apply",
        hash: "d1f790d69735164c0ced2da614f76b2f8aac7f3c201b87649ee5f9a06fafcb1b",
        f: opt_415_apply,
    },
    FnRow {
        id: "ConfigOptions[416].apply",
        hash: "7a4df9f879394345486cd90ba7636036a87f269f2ce9cd27b258de2653e62aaa",
        f: opt_416_apply,
    },
    FnRow {
        id: "ConfigOptions[417].apply",
        hash: "b45ad92662d6c15f597a57c65bb2e2060a19532f18c2d6e6a9e803e67c2cf9a6",
        f: opt_417_apply,
    },
    FnRow {
        id: "ConfigOptions[418].apply",
        hash: "fc57196717c03a75b3127b1328ad3dbf1eebeb840e0da3e8fcd87cd2451b224e",
        f: opt_418_apply,
    },
    FnRow {
        id: "ConfigOptions[419].apply",
        hash: "41664d1e45edf663bce430e78d1415dcd78177a31df501ee552e0d0592025194",
        f: opt_419_apply,
    },
    FnRow {
        id: "ConfigOptions[420].apply",
        hash: "d00162c483ce2d9bacc568e41508de9d598b811a06f07ec1ddb194a3297985fb",
        f: opt_420_apply,
    },
    FnRow {
        id: "ConfigOptions[421].apply",
        hash: "726e47245af407bf257c9cfd0c3c54eede41870554d4b370b61e2158751b9294",
        f: opt_421_apply,
    },
    FnRow {
        id: "ConfigOptions[422].apply",
        hash: "4ee8420e86e41c6a6062810af56a69ae7f2b0c82fad4a4c1beb7184df8994c51",
        f: opt_422_apply,
    },
    FnRow {
        id: "ConfigOptions[423].apply",
        hash: "e0b3b3bddde5ba5f1066891bbcf820c453ac84c4e3119e4269801d45f688cece",
        f: opt_423_apply,
    },
    FnRow {
        id: "ConfigOptions[424].apply",
        hash: "1995c61beef947ba9fe71a559bc74a98d371a2346e17755d11794916fe40190b",
        f: opt_424_apply,
    },
    FnRow {
        id: "ConfigOptions[425].apply",
        hash: "83050bb2b029c79f459d9764d047364e036ca6566fa5088b17e0461c6f3f9e5a",
        f: opt_425_apply,
    },
    FnRow {
        id: "ConfigOptions[426].apply",
        hash: "fd9bad7e31b57d8e273e76bd4f4ff584f89294d3a4b1966f97a9e61440f199ad",
        f: opt_426_apply,
    },
    FnRow {
        id: "ConfigOptions[427].apply",
        hash: "4b7d584da97cc39698cf3e6b16cd785a81023811a36d22f77da83abd9894f7dc",
        f: opt_427_apply,
    },
    FnRow {
        id: "ConfigOptions[428].apply",
        hash: "9f19ea567fbc8babe5bb6cc386ac0ca73ba7cbef6b11fa9aeb80b0765c9143a5",
        f: opt_428_apply,
    },
    FnRow {
        id: "ConfigOptions[429].apply",
        hash: "52726ee074367d17c6c635821b361f999628338be5fc0ec53c066afde03b269b",
        f: opt_429_apply,
    },
    FnRow {
        id: "ConfigOptions[430].apply",
        hash: "c43c122247e8c1e9fd41a31287b5f8c585eeda49922697ef7d6b98872016d066",
        f: opt_430_apply,
    },
    FnRow {
        id: "ConfigOptions[431].apply",
        hash: "846741c39b583614929540a31f6953d4b6c79fef7b3ed9de35769bf6f9485701",
        f: opt_431_apply,
    },
    FnRow {
        id: "ConfigOptions[432].apply",
        hash: "c29e8e15fcf1986fecfbdf55beae9f81e69e958b2a5d51a2d6da0fffedd16a79",
        f: opt_432_apply,
    },
    FnRow {
        id: "ConfigOptions[433].apply",
        hash: "9b2557a9652a5f9271df0ce9de03610c0f521c855eb5135fb904ee25b6ca2e46",
        f: opt_433_apply,
    },
    FnRow {
        id: "ConfigOptions[434].apply",
        hash: "503783a1dd23fbf07201cdbe335264007df568106fe8c98feaaeb5f5fbd6c3f7",
        f: opt_434_apply,
    },
    FnRow {
        id: "ConfigOptions[435].apply",
        hash: "f6d30508d4ba9f3310bfd5eb100e01041e88326d51819e4ce7ace2a0c957c3f1",
        f: opt_435_apply,
    },
    FnRow {
        id: "ConfigOptions[436].apply",
        hash: "04dfaf304f250186a6ff587566c64139ecfe25406c173bbdc449808b7fdbb299",
        f: opt_436_apply,
    },
    FnRow {
        id: "ConfigOptions[437].apply",
        hash: "6e8a7cbfdae429680174d7f9cddc59fe0e3f56614eab3fd0e90bbca5f4ea3b08",
        f: opt_437_apply,
    },
    FnRow {
        id: "ConfigOptions[438].apply",
        hash: "fbc11e2b7e3807375d480c23257920d05725ded71d6a70a7e6597a9ad10f9393",
        f: opt_438_apply,
    },
    FnRow {
        id: "ConfigOptions[439].apply",
        hash: "2580f0bad02ef1cafa948c6ff3324787598cc10c4e441010d1cdbb1175fac9ad",
        f: opt_439_apply,
    },
    FnRow {
        id: "ConfigOptions[440].apply",
        hash: "0628c35f7771a000105542b045e50209e79a602956c5ec2747e971a97b89035c",
        f: opt_440_apply,
    },
    FnRow {
        id: "ConfigOptions[441].apply",
        hash: "470c2af1aace1d4de14d9af59d6240c26c8bde6d8aba243df025b5ed9b5e1af7",
        f: opt_441_apply,
    },
    FnRow {
        id: "ConfigOptions[442].apply",
        hash: "fdcd52b767c0d42eb5748267c6a1d4ff084cf7385f16134239d663ad34a8bf3a",
        f: opt_442_apply,
    },
    FnRow {
        id: "ConfigOptions[443].apply",
        hash: "fabe89e1ac103315e20e94c2a676725cb38e53c831b8149ae274fbc233680734",
        f: opt_443_apply,
    },
    FnRow {
        id: "ConfigOptions[444].apply",
        hash: "e645b78806c169d191473380cc0db3a75f59fa99f8f557a8e9d3b55315acb45d",
        f: opt_444_apply,
    },
    FnRow {
        id: "ConfigOptions[445].apply",
        hash: "6a395adb838ca1bb6a3b609029ecf03c5b03c8c6826b9c449e1df9c7b1fe7a98",
        f: opt_445_apply,
    },
    FnRow {
        id: "ConfigOptions[446].apply",
        hash: "5e6205a972a1b68cbcb84f78f1b62d6c61a976ebcc5cdc6bbd2c6364c502a6ff",
        f: opt_446_apply,
    },
    FnRow {
        id: "ConfigOptions[447].apply",
        hash: "aba9be84c09f66358e9b33b013cf183a14fdf5f71395dffd2f1b9b7e5e6f2294",
        f: opt_447_apply,
    },
    FnRow {
        id: "ConfigOptions[448].apply",
        hash: "f5a28300e18e0bb766034947ecb591799b9a43bfd99f338598b7bcfda0f39874",
        f: opt_448_apply,
    },
    FnRow {
        id: "ConfigOptions[449].apply",
        hash: "4973c34e193ea3e463de26f703320e963ad63f54167e3a330323839d2fdacab2",
        f: opt_449_apply,
    },
    FnRow {
        id: "ConfigOptions[450].apply",
        hash: "a35a5fda744aa723da3351e94badfe259343d2d46fc7998abb682e062d51f34d",
        f: opt_450_apply,
    },
    FnRow {
        id: "ConfigOptions[451].apply",
        hash: "6e20ba4606958e57521f574cd31e9dde09832fe463ae50f08259e455b5d9b943",
        f: opt_451_apply,
    },
    FnRow {
        id: "ConfigOptions[452].apply",
        hash: "a9dcf9147c312510c9954cde23c176a556d0ffddc003cc2e88c57abf0ce1fd3a",
        f: opt_452_apply,
    },
    FnRow {
        id: "ConfigOptions[453].apply",
        hash: "7288d5101b65e347613f9f51e86e9a275023067912832ba36fafecf624cd432a",
        f: opt_453_apply,
    },
    FnRow {
        id: "ConfigOptions[454].apply",
        hash: "604332b857a3e1ea7e7813de7a29e0f6f42c3842962a7e3dbb79d94c068a536e",
        f: opt_454_apply,
    },
    FnRow {
        id: "ConfigOptions[455].apply",
        hash: "4913f0dd6ef3c4c35da7b6c6fa0fe90fa819240e08cf92c7922748d388a3b5ae",
        f: opt_455_apply,
    },
    FnRow {
        id: "ConfigOptions[456].apply",
        hash: "14c0628dc14af8e7c8fc1632ebbcd1de528591dad87e9ebe147313cf9cb12311",
        f: opt_456_apply,
    },
    FnRow {
        id: "ConfigOptions[457].apply",
        hash: "d2b91e8afad73c7b56637c9a70a780a3959d1ecf0dc2eb571884fb264d9a307a",
        f: opt_457_apply,
    },
    FnRow {
        id: "ConfigOptions[458].apply",
        hash: "cb521930a9a1b228928725dd71255a2745b2084827117fba7d13c13d3205edc9",
        f: opt_458_apply,
    },
    FnRow {
        id: "ConfigOptions[459].apply",
        hash: "0206e0053fffa214b731acd70bd274b2929000cc1c499a847595679622d0c771",
        f: opt_459_apply,
    },
    FnRow {
        id: "ConfigOptions[460].apply",
        hash: "6f3b95ce9be98da50d9e90ca22556d4262ae954be3c3e0b87e501b08e7c4cd2d",
        f: opt_460_apply,
    },
    FnRow {
        id: "ConfigOptions[461].apply",
        hash: "1312c2c6f1abc48d823b2054589eb5f9e137255ae3f33fce49bb85682a08d66d",
        f: opt_461_apply,
    },
    FnRow {
        id: "ConfigOptions[462].apply",
        hash: "40bbc14164a13e17cc5f72d8d47b41c871a8d9f4703ad6a3f14641a29e928bf2",
        f: opt_462_apply,
    },
    FnRow {
        id: "ConfigOptions[463].apply",
        hash: "351d607e3fced0954991935d67b679f80a679ce06ec7d49d98e7ca48c1ad79aa",
        f: opt_463_apply,
    },
    FnRow {
        id: "ConfigOptions[464].apply",
        hash: "f5f9edc6769491db3c569f0f76a43abc695ba818f140dac4a8d3efccc49ee28a",
        f: opt_464_apply,
    },
    FnRow {
        id: "ConfigOptions[465].apply",
        hash: "019d241e3f783a0c69948c32887e058821c09759423b638ea61884aa057164fe",
        f: opt_465_apply,
    },
    FnRow {
        id: "ConfigOptions[466].apply",
        hash: "6626db82fa7e222bc2f9c4eade9ca2e902284f6058691420b7ce368452a7bba1",
        f: opt_466_apply,
    },
    FnRow {
        id: "ConfigOptions[467].apply",
        hash: "f5f17fa09d95aa2aace81162e291ae7d3427d0bb080d1e22820c8ce9107cf8bd",
        f: opt_467_apply,
    },
    FnRow {
        id: "ConfigOptions[468].apply",
        hash: "efb9d565b5eccf73b1d31c8110e4ea573ed7373073d1ddde6fa3a6a3045365d8",
        f: opt_468_apply,
    },
    FnRow {
        id: "ConfigOptions[469].apply",
        hash: "4379f696fb7b5cb989c0bb8e3ddd43ba21313ed45b653525e56fcd9ec062432a",
        f: opt_469_apply,
    },
    FnRow {
        id: "ConfigOptions[470].apply",
        hash: "e2652cf0a816fef670f32e4f346f6767d959e1523ca20bf21549cbb3e18f2d9d",
        f: opt_470_apply,
    },
    FnRow {
        id: "ConfigOptions[471].apply",
        hash: "a38bcfc838cae41a46d27f686ca781f70a5fc3c03bf66954013262d653fcd36a",
        f: opt_471_apply,
    },
    FnRow {
        id: "ConfigOptions[472].apply",
        hash: "3395cc187233aa721e8d6c3e603555e418659e282a352bad383b290910384667",
        f: opt_472_apply,
    },
    FnRow {
        id: "ConfigOptions[473].apply",
        hash: "62e5b0720b78a17d098684de594de9b41b0708e3cefd17caf9e05014675a9526",
        f: opt_473_apply,
    },
    FnRow {
        id: "ConfigOptions[474].apply",
        hash: "6c4eed0b82c62576e396c388771c3c137f09f0103b6268ee2fb2e5bf1269ee33",
        f: opt_474_apply,
    },
    FnRow {
        id: "ConfigOptions[475].apply",
        hash: "a18be71d30c465a46b07079c57e429bba220e67725b864b76e59b25d591b3b5d",
        f: opt_475_apply,
    },
    FnRow {
        id: "ConfigOptions[476].apply",
        hash: "cb65720c42984d3729992325de471d0579c4477dcffa9949e42787bf1193f7ea",
        f: opt_476_apply,
    },
    FnRow {
        id: "ConfigOptions[477].apply",
        hash: "0b650bd602a78aa2e06cf8c75b4c2baea5d7233502ad7674411c8db684d2d1f2",
        f: opt_477_apply,
    },
    FnRow {
        id: "ConfigOptions[478].apply",
        hash: "6749ab540fcdd971cf4e4e978dd26b2fb6a7e8e1b72d3f801534b3805aa57e5c",
        f: opt_478_apply,
    },
    FnRow {
        id: "ConfigOptions[479].apply",
        hash: "1f960d48b004cc9410e5badf41c49fccdf4944cb35f6a4fc3a03ba10e79358b2",
        f: opt_479_apply,
    },
    FnRow {
        id: "ConfigOptions[480].apply",
        hash: "5864ffd45ba9193c73e0b6ac9fc8f56fa24c83e02c2baeb64eec3654a2090cc1",
        f: opt_480_apply,
    },
    FnRow {
        id: "ConfigOptions[481].apply",
        hash: "67bda683b45798dad86adb9a15c6b8712e4bc87ef4db4eae188182e988d113a5",
        f: opt_481_apply,
    },
    FnRow {
        id: "ConfigOptions[482].apply",
        hash: "824f65e9670ee887ce22fce09d3f0055d99510e3143b50ca8b49ce0593b2905d",
        f: opt_482_apply,
    },
    FnRow {
        id: "ConfigOptions[483].apply",
        hash: "3d4d365c8333943d26a3d16171cfc8abbed6289757a58ddac79ffeeb04e61223",
        f: opt_483_apply,
    },
    FnRow {
        id: "ConfigOptions[484].apply",
        hash: "a9be86cda16c1a04dbe13ba613fa1dab15921528e5435ef23825ca97667dec87",
        f: opt_484_apply,
    },
    FnRow {
        id: "ConfigOptions[485].apply",
        hash: "0aa669ef4efeb768bcfa9c38021103cdacc4c4810613f264d9f8dedc0aaa0c72",
        f: opt_485_apply,
    },
    FnRow {
        id: "ConfigOptions[486].apply",
        hash: "bd29799020ad7c73ed6c8efb5f5e6959cb47a01fd3b5c8884183d36b23b6c882",
        f: opt_486_apply,
    },
    FnRow {
        id: "ConfigOptions[487].apply",
        hash: "c2c4f52af633db7b9d1d53fb28ad0f04f2f98f5bf993f451de8d1b64386a1a90",
        f: opt_487_apply,
    },
    FnRow {
        id: "ConfigOptions[488].apply",
        hash: "e7606fe40b2791b7966ab88a6ca65fca59d20d86c0d76cdecd4339ddc84e9238",
        f: opt_488_apply,
    },
    FnRow {
        id: "ConfigOptions[489].apply",
        hash: "afbf6186f3aa849b36e2e760e42225a03483a15aa417980a78631bea704a8d18",
        f: opt_489_apply,
    },
    FnRow {
        id: "ConfigOptions[490].apply",
        hash: "4f5a8d7b225057844a0825d01cfef6d28ffe1b8609ba057c3e5742f9c3e695ef",
        f: opt_490_apply,
    },
    FnRow {
        id: "ConfigOptions[491].apply",
        hash: "3fe019129d534734e11cbfeec4b51a59f994a713682ce57853a2e0523b131e9b",
        f: opt_491_apply,
    },
    FnRow {
        id: "ConfigOptions[492].apply",
        hash: "b88a3f10c23e0ce9769a353d0cd631c0f2dbe9e878ea723365a5f760c43e45bf",
        f: opt_492_apply,
    },
    FnRow {
        id: "ConfigOptions[493].apply",
        hash: "609ffcf83424add246792998681c0672bfa9176bade785ad9b5bb74fc3007cd5",
        f: opt_493_apply,
    },
    FnRow {
        id: "ConfigOptions[494].apply",
        hash: "900b1d75946256881edfdc44be13fbe33848ca9c56ca7787a47b7dad1b177b7d",
        f: opt_494_apply,
    },
    FnRow {
        id: "ConfigOptions[495].apply",
        hash: "b520cd478a20af64c526c20781f1eb19a9ab93ac9d5f17fc923270a1def5be33",
        f: opt_495_apply,
    },
    FnRow {
        id: "ConfigOptions[496].apply",
        hash: "bcde6a4ab8d87a709db885597d95393556d1dec199245db15b16226fc7765ad8",
        f: opt_496_apply,
    },
    FnRow {
        id: "ConfigOptions[497].apply",
        hash: "054f9ed5131ba70e7dbf4addc9d8bac2b3f40ced4f17aefa3a1504705f437965",
        f: opt_497_apply,
    },
    FnRow {
        id: "ConfigOptions[498].apply",
        hash: "450c92a9276ddc640a3169be94f3c351c65dc04b0c3af9eff6478646e1735eaa",
        f: opt_498_apply,
    },
    FnRow {
        id: "ConfigOptions[499].apply",
        hash: "8ce91e1b4f0413a2a1a2cb9b70e3f3dc45d8755f85f8804f7f14dc741f3e066c",
        f: opt_499_apply,
    },
    FnRow {
        id: "ConfigOptions[500].apply",
        hash: "125826b42b9f09d61e63986745d32dfe826d4d53727c06841159757bc038675c",
        f: opt_500_apply,
    },
    FnRow {
        id: "ConfigOptions[501].apply",
        hash: "287d9043f2266b3d02cdcded98d76cbe5ae67b605e8b0f483ca75afde9db81c8",
        f: opt_501_apply,
    },
    FnRow {
        id: "ConfigOptions[502].apply",
        hash: "962b281dfcfd2281eeeec4d0d40b35d8cfbaacda658fb5ff58b2a7ce1b8c9e30",
        f: opt_502_apply,
    },
    FnRow {
        id: "ConfigOptions[503].apply",
        hash: "2aa2c030ce4f79c878f69e3fe4a87d7ab0462d0ef570dbbd295ca564d18b30f3",
        f: opt_503_apply,
    },
    FnRow {
        id: "ConfigOptions[504].apply",
        hash: "4778b94cb2ad693a3baca24d9f9296c6cf4b813dc944212f8b716b7ed91f81a9",
        f: opt_504_apply,
    },
    FnRow {
        id: "ConfigOptions[505].apply",
        hash: "ddf8d72af89f2f702793f2a4e7acfeda380db75c51459608582ba29ee279c0f8",
        f: opt_505_apply,
    },
    FnRow {
        id: "ConfigOptions[506].apply",
        hash: "712ec2d8e7673d54b3a528e322f42f4d6af11db751018c155b54d94da607b827",
        f: opt_506_apply,
    },
    FnRow {
        id: "ConfigOptions[507].apply",
        hash: "12980adf13bc71669ef5691a68e3a5df38a9a8d8f08e7bf19262f66d3bb986dc",
        f: opt_507_apply,
    },
    FnRow {
        id: "ConfigOptions[508].apply",
        hash: "bba67680c2b80185298c52ce7a374d8e8071458ac850625267beb1f071822b0e",
        f: opt_508_apply,
    },
    FnRow {
        id: "ConfigOptions[509].apply",
        hash: "16a376c69cf3e1556ca7f4d9e59ca5ec5c74289fd8781ac7789cec413643bc5b",
        f: opt_509_apply,
    },
    FnRow {
        id: "ConfigOptions[510].apply",
        hash: "922a3552cf67dd6bc8cb53d6ba838f5ed0a03cb7c63732afbf759b3111808789",
        f: opt_510_apply,
    },
    FnRow {
        id: "ConfigOptions[511].apply",
        hash: "a5ab3cb413ac8f1c93f05bc1d12da01870b1a1347e40376e90992e7827da76f3",
        f: opt_511_apply,
    },
    FnRow {
        id: "ConfigOptions[512].apply",
        hash: "5b7c8360d541e77f6160ff6ce910245af3e645d20f43e32b28cd6e54ab374d7f",
        f: opt_512_apply,
    },
    FnRow {
        id: "ConfigOptions[513].apply",
        hash: "e828c96e8a594ef8ee36da7ab9965d52d32c125930bb13b71df62c909963950e",
        f: opt_513_apply,
    },
    FnRow {
        id: "ConfigOptions[514].apply",
        hash: "1d3cad5bec6276e63949d99d85b9bf5e40ae18d092c670e4a3fffb1c2e343a46",
        f: opt_514_apply,
    },
    FnRow {
        id: "ConfigOptions[515].apply",
        hash: "da0f9d32e629530df6be41f97c57b60bc6b76f91c18d8d4d4127298d3b532fa5",
        f: opt_515_apply,
    },
    FnRow {
        id: "ConfigOptions[516].apply",
        hash: "0207d529ee86c2857ada76599b910922bbd6f05c0660e5cb35bd0cc76cee1c29",
        f: opt_516_apply,
    },
    FnRow {
        id: "ConfigOptions[517].apply",
        hash: "0b582f4dedd31febe8421cfe8d36572d83368a0acf24336cdc4bc3356016b606",
        f: opt_517_apply,
    },
    FnRow {
        id: "ConfigOptions[518].apply",
        hash: "6dc6dd5e314889f3c8ba19ddd4c1076b6cd32fdc66b6d2a20944bbc3234ded16",
        f: opt_518_apply,
    },
    FnRow {
        id: "ConfigOptions[519].apply",
        hash: "69aff95817d2397d5661eaef5c64c90065266d84610214fb72f4e7e68350b7be",
        f: opt_519_apply,
    },
    FnRow {
        id: "ConfigOptions[520].apply",
        hash: "72d06af3f7ce53c03680b6a272c0db03701b113af50a4ddb08d2d8aad4da6f64",
        f: opt_520_apply,
    },
    FnRow {
        id: "ConfigOptions[521].apply",
        hash: "303ec06be89fe6af3c0455a12781f29f63219f62382f1a147a1119d3e4e3451a",
        f: opt_521_apply,
    },
    FnRow {
        id: "ConfigOptions[522].apply",
        hash: "3288eed8c71dd06312b6e675b2e02214e9713fb2557de5c091748859fcb02932",
        f: opt_522_apply,
    },
    FnRow {
        id: "ConfigOptions[523].apply",
        hash: "ba245bf819216d665ef9a574b559c74e8f173d185a1146bcee98e265c8cf3c0b",
        f: opt_523_apply,
    },
    FnRow {
        id: "ConfigOptions[524].apply",
        hash: "f82dbe4bf3b78ed20e0de809fe81fded72eeeb88557778d4a0351eb6ea2024ce",
        f: opt_524_apply,
    },
    FnRow {
        id: "ConfigOptions[525].apply",
        hash: "9d3f771a26d42f05cb80e322b9e991d46939d82c7a323ecc769beb57eda312ea",
        f: opt_525_apply,
    },
    FnRow {
        id: "ConfigOptions[526].apply",
        hash: "23487efe381d91f25a65d5b7d94e21aec63f6eb4085d82b040049bcfde372774",
        f: opt_526_apply,
    },
    FnRow {
        id: "ConfigOptions[527].apply",
        hash: "c2defb70dada91b898a902a7c400dd90810356652927a934da789615ad1c2daf",
        f: opt_527_apply,
    },
    FnRow {
        id: "ConfigOptions[528].apply",
        hash: "c1949fe4ea7c21c1d34f1aadc2c4bfca7088f5ecc0323bf8f2df60a4213c1d99",
        f: opt_528_apply,
    },
    FnRow {
        id: "ConfigOptions[529].apply",
        hash: "4014a52d9fa2938fd799b85148f36fc4640424c8894c3556b71c0f279720a6f1",
        f: opt_529_apply,
    },
    FnRow {
        id: "ConfigOptions[530].apply",
        hash: "fb5cd3b9a3273e668276bbc24610600d0a7e02d676729f38d0eafc3dc49f542c",
        f: opt_530_apply,
    },
    FnRow {
        id: "ConfigOptions[531].apply",
        hash: "7508f3eb3d95c93d486612bf719e0dba367250aad5fee6529b73f1e6cdb80e66",
        f: opt_531_apply,
    },
    FnRow {
        id: "ConfigOptions[532].apply",
        hash: "bba02c2e188224d1c27f69fba9fac48d21c4fd0558c4f08d36ef35ee4f4046eb",
        f: opt_532_apply,
    },
    FnRow {
        id: "ConfigOptions[533].apply",
        hash: "c9b281e5ac95918560bece6559d651eef913d0763fb4a92b08e02da3ec05c3ff",
        f: opt_533_apply,
    },
    FnRow {
        id: "ConfigOptions[534].apply",
        hash: "d09693450351982f7a4740708619099386ae03e8063ead776be81cbd645937b9",
        f: opt_534_apply,
    },
    FnRow {
        id: "ConfigOptions[535].apply",
        hash: "e5ff596f011c4e3fe690c4b5b2de7fce459e95cdda4762b675d16ba2f3221020",
        f: opt_535_apply,
    },
    FnRow {
        id: "ConfigOptions[536].apply",
        hash: "56a54c4f9827cab6d544db5cf5f149cfb4c2d86b7cae22a24714f24a5b2ccfad",
        f: opt_536_apply,
    },
    FnRow {
        id: "ConfigOptions[537].apply",
        hash: "e9e9171e1e54d12a628a5222ec0cc9524851ff59802e562ad5ae065b25463be1",
        f: opt_537_apply,
    },
    FnRow {
        id: "ConfigOptions[538].apply",
        hash: "1b8628ea7162260812d82f65551de577b091f5c68175055493b461e138e923d3",
        f: opt_538_apply,
    },
    FnRow {
        id: "ConfigOptions[539].apply",
        hash: "8a6fd7758221767ff168c564b18c3b8398d482c740c8c12afe210f68b331462a",
        f: opt_539_apply,
    },
    FnRow {
        id: "ConfigOptions[540].apply",
        hash: "e1028962cc3504110ab24fc417b11cf2483dce57e7bb3036f41ea2dc8ac0b9cf",
        f: opt_540_apply,
    },
    FnRow {
        id: "ConfigOptions[541].apply",
        hash: "6887762b6362d3dd9c6e1df3af8a96ff0ea0b45304eee969fc7e1d7e05d87a87",
        f: opt_541_apply,
    },
    FnRow {
        id: "ConfigOptions[542].apply",
        hash: "34766db0d4722f01ee424ad3cff4e53c9ffb6867b2ab931df88b766d4225be92",
        f: opt_542_apply,
    },
    FnRow {
        id: "ConfigOptions[543].apply",
        hash: "27a79ad658515d5b92633d3e7309fb7019c9fd9c2c35d3c90454b5368d7c6df7",
        f: opt_543_apply,
    },
    FnRow {
        id: "ConfigOptions[544].apply",
        hash: "d91ca804f6f860fd360f04115c2c7a11f35a0f15c2b84dcb5b58a8bfc154f0c5",
        f: opt_544_apply,
    },
    FnRow {
        id: "ConfigOptions[545].apply",
        hash: "14c23b1eb5884fdb08011dc13ef26b78359cf0b19a47f9890c62934ce62f5ae1",
        f: opt_545_apply,
    },
    FnRow {
        id: "ConfigOptions[546].apply",
        hash: "bf0f3205c47b3153b488a66971b5c664eba2840476aa849d50b09deddaeca459",
        f: opt_546_apply,
    },
    FnRow {
        id: "ConfigOptions[547].apply",
        hash: "9b5f2f165bf367622a2367ff050c4b4730b415a1733171ad235fcb3664daf7b7",
        f: opt_547_apply,
    },
    FnRow {
        id: "ConfigOptions[548].apply",
        hash: "1941791f07cb8456665d68aa8faf974f0ae5a784caeac97a88dfe33a68aa3c23",
        f: opt_548_apply,
    },
    FnRow {
        id: "ConfigOptions[549].apply",
        hash: "4990dbcbcaae299864ca9693963e66086c82f81124aa5ac1692b4818f91a2976",
        f: opt_549_apply,
    },
    FnRow {
        id: "ConfigOptions[550].apply",
        hash: "b39372a5a7c7703d61a4b08e38fef3ee443bf500e71c71631114ed2cde2ecaba",
        f: opt_550_apply,
    },
    FnRow {
        id: "ConfigOptions[551].apply",
        hash: "7ce1e813b40be20fcb11cab5a0b6c59b277b1032e14c9cad931b57bff4a88175",
        f: opt_551_apply,
    },
    FnRow {
        id: "ConfigOptions[552].apply",
        hash: "59ee682dee48eff7917876c2734c45ea878e378bb9c0371054a79524c6634853",
        f: opt_552_apply,
    },
    FnRow {
        id: "ConfigOptions[553].apply",
        hash: "fe6cca2499ad470a7a338daa66921537544feff953ce2d85ae0fa75dda8b7d12",
        f: opt_553_apply,
    },
    FnRow {
        id: "ConfigOptions[554].apply",
        hash: "4f100817633c8dc3a340ff66074c5dcb7d566205c49f1c647e29afb8fdfb71f7",
        f: opt_554_apply,
    },
    FnRow {
        id: "ConfigOptions[555].apply",
        hash: "1faab41ede0c9eaca6e5b391be73c81c0cf42549213524563608ebde1c204d6e",
        f: opt_555_apply,
    },
    FnRow {
        id: "ConfigOptions[556].apply",
        hash: "a9ce1529d4232b4a0ec1166672515dff8bf98dbab33daf416f348e367c41c642",
        f: opt_556_apply,
    },
    FnRow {
        id: "ConfigOptions[557].apply",
        hash: "09cc0332fba54717553663530dd2499ccdd73147f984fb3b9fc0a7180bab0bba",
        f: opt_557_apply,
    },
    FnRow {
        id: "ConfigOptions[558].apply",
        hash: "8953f54c62ae621dfa5f085da29049c968b02a44bc110017c3fd20abd0f3cbdf",
        f: opt_558_apply,
    },
    FnRow {
        id: "ConfigOptions[559].apply",
        hash: "dda1606219d777026d79d1980eb4cad586a6e8d46b6ecf8c4c79596a207ad08f",
        f: opt_559_apply,
    },
    FnRow {
        id: "ConfigOptions[560].apply",
        hash: "09adde4d1f05ffa50bfd4e762cf53bde13d8e2f5efab1981fcc825d2c3e2de65",
        f: opt_560_apply,
    },
    FnRow {
        id: "ConfigOptions[561].apply",
        hash: "db59860c6d2b3e196bd788e67d9e0e0f5e2bb8a35f38882ae3a2b74b27da0f53",
        f: opt_561_apply,
    },
    FnRow {
        id: "ConfigOptions[562].apply",
        hash: "00c0c9b9da22700a1fa5161a257b4126ef9db2e29631ff697f90cb45fe860ca9",
        f: opt_562_apply,
    },
    FnRow {
        id: "ConfigOptions[563].apply",
        hash: "d470e968ac066a5cedbb7230493281b3b79797873516e051de437f8bd3345066",
        f: opt_563_apply,
    },
    FnRow {
        id: "ConfigOptions[564].apply",
        hash: "0de26b4338b88b9ab8b2ab8ef435cb95dcbda085af032692f2d576072edde3ca",
        f: opt_564_apply,
    },
    FnRow {
        id: "ConfigOptions[566].apply",
        hash: "98285a521a506ec1aa5da4c55c595f949835f59892cab0d28b593c710d2db8fc",
        f: opt_566_apply,
    },
    FnRow {
        id: "ConfigOptions[567].apply",
        hash: "1b961e07fd3ec914564aabb6e8b333e26a8d2660086db266b42e7173f04d64d2",
        f: opt_567_apply,
    },
    FnRow {
        id: "ConfigOptions[568].apply",
        hash: "daccd350411094d070e759b101c7290db9a34c20a4d74422ab1bd29cbaac46c6",
        f: opt_568_apply,
    },
    FnRow {
        id: "ConfigOptions[571].apply",
        hash: "0defd61d27a6cd9e808c2579a5570d7a4459b1d448cd45d3aa35e5556e4c25d0",
        f: opt_571_apply,
    },
    FnRow {
        id: "ConfigOptions[572].apply",
        hash: "88290b8fa845f2061ff54d5c6c98d1abaff3f28fd4e25ebc88afc4f783f6d998",
        f: opt_572_apply,
    },
    FnRow {
        id: "ConfigOptions[573].apply",
        hash: "6055ef8bf3757f626d4ed624c9cf29a1f9601f8b7cfd37a68dc96b0a6eda3abc",
        f: opt_573_apply,
    },
    FnRow {
        id: "ConfigOptions[574].apply",
        hash: "189bbe27b4797fea65045e782acba6783f15065f89eabdbf29c71b1ed0dfd62e",
        f: opt_574_apply,
    },
    FnRow {
        id: "ConfigOptions[575].apply",
        hash: "c778bd29c64c80a92a9dfa56da42ebf724f1684019513dd4984e771bfafbb4d9",
        f: opt_575_apply,
    },
    FnRow {
        id: "ConfigOptions[576].apply",
        hash: "1a946e3193faafb6e5cfc952808a568e5fdbdb8e3723efd90d20efbe95abc896",
        f: opt_576_apply,
    },
    FnRow {
        id: "ConfigOptions[577].apply",
        hash: "c390a277cafdecf6e6e33be16c7dbcdd96b44672586ca7b7d4e6b90eb8769c79",
        f: opt_577_apply,
    },
    FnRow {
        id: "ConfigOptions[578].apply",
        hash: "09434b734b11c7236347cae4f88e694a0e281a0a95cc1a81ef2fed7339384abc",
        f: opt_578_apply,
    },
    FnRow {
        id: "ConfigOptions[579].apply",
        hash: "f781220e77c00341904ec406bc63c5faf4483e3f72b6150e23eafa6b7505b2c3",
        f: opt_579_apply,
    },
    FnRow {
        id: "ConfigOptions[580].apply",
        hash: "784e306e0eba7ed596528376627146752e90f210cde44d28dfd100e33031e46c",
        f: opt_580_apply,
    },
    FnRow {
        id: "ConfigOptions[581].apply",
        hash: "224cf38f91cdebab2948efeecc5a42660f695b2f0d93d6708f95a9704b074bb4",
        f: opt_581_apply,
    },
    FnRow {
        id: "ConfigOptions[582].apply",
        hash: "734b967b928ac884419a466c086f5cf16847eba20e51afc0659467c4de0e46e7",
        f: opt_582_apply,
    },
    FnRow {
        id: "ConfigOptions[583].apply",
        hash: "ac7fdcecbf10e125aff797d6dde6f2a1ab668a402558e38712e1aad82ba18915",
        f: opt_583_apply,
    },
    FnRow {
        id: "ConfigOptions[584].apply",
        hash: "c8b6f81110e5d718e4acf665876d339d525ee327f5dade1cbef9ab4589a66c45",
        f: opt_584_apply,
    },
    FnRow {
        id: "ConfigOptions[585].apply",
        hash: "fe65806fcac613d160a223df5ba66d2cfcc5acae5b42c82a053679a819508b70",
        f: opt_585_apply,
    },
    FnRow {
        id: "ConfigOptions[586].apply",
        hash: "07a71ac2f6df5df3d809ae238066339dc950f3862d93865b537f57cfd5fb7f74",
        f: opt_586_apply,
    },
    FnRow {
        id: "ConfigOptions[587].apply",
        hash: "ef7b69a03cabccb062db23c136e3fee65d3f6d42f413e9dc5c91729c71664562",
        f: opt_587_apply,
    },
    FnRow {
        id: "ConfigOptions[588].apply",
        hash: "4ce33ab6ba794a37426165518f2c34f80081e74d7db36389b1b778196cc16de5",
        f: opt_588_apply,
    },
    FnRow {
        id: "ConfigOptions[589].apply",
        hash: "45aec9db9765c5714a91b2c8ad6ea94fb84ea11683226f1f21858b68707ae80b",
        f: opt_589_apply,
    },
    FnRow {
        id: "ConfigOptions[590].apply",
        hash: "bf1fdd1424d4a257f686680a8b81bf5b2cc9fdb74df62c604605f132fb442e9d",
        f: opt_590_apply,
    },
    FnRow {
        id: "ConfigOptions[591].apply",
        hash: "f9b56aab7339126c71c19cecbbd614542c0820a9e4457b1fd792691594dd0d62",
        f: opt_591_apply,
    },
    FnRow {
        id: "ConfigOptions[592].apply",
        hash: "d2454c92bae136d07583ebff9d7eb57260edea67586f99b622a399ea794deb03",
        f: opt_592_apply,
    },
    FnRow {
        id: "ConfigOptions[593].apply",
        hash: "b1559c028b389ae3be0c062f47d474fb67d25b24e9a66855dc16bc4485b880a9",
        f: opt_593_apply,
    },
    FnRow {
        id: "ConfigOptions[594].apply",
        hash: "59f6ffd070b6b094bd3c0227fcb9047e8e08a5c9e9b019dbaa270463bac22583",
        f: opt_594_apply,
    },
    FnRow {
        id: "ConfigOptions[595].apply",
        hash: "c3df3c49dc51d1af972d067df58abaa6c26cee68a107d0d5c1e49c30dcddf6da",
        f: opt_595_apply,
    },
    FnRow {
        id: "ConfigOptions[596].apply",
        hash: "10e29703e8937993ddca9fe4182389bc5efe6d57e82a2b03a6f9752f26338b69",
        f: opt_596_apply,
    },
    FnRow {
        id: "ConfigOptions[597].apply",
        hash: "2ea072aad2a81191c7787605c7b2fcc3cbb63e307229c3064396aa1a242dc27b",
        f: opt_597_apply,
    },
    FnRow {
        id: "ConfigOptions[598].apply",
        hash: "823f63f333b34ac5049d4c5b0c997f382f206ccec81cd02bf57c4ac17dd384fa",
        f: opt_598_apply,
    },
    FnRow {
        id: "ConfigOptions[599].apply",
        hash: "f0ad3bd3d82144bd77049dfb7e4aa006537381b092165becbbb30e8c8b66fc4d",
        f: opt_599_apply,
    },
    FnRow {
        id: "ConfigOptions[600].apply",
        hash: "6d90ba738628acbc94b4a510925474cf00dd29fc64fc5be5aa8ad77e4c8ec0a4",
        f: opt_600_apply,
    },
    FnRow {
        id: "ConfigOptions[601].apply",
        hash: "cf6a465e2b0b761b45e370f1484af598801e036413192003c53e86c922f3f718",
        f: opt_601_apply,
    },
    FnRow {
        id: "ConfigOptions[602].apply",
        hash: "bbb099aca7ae70215625e87054380b226bae0173bdbdadcce97b6b5f7815bf6c",
        f: opt_602_apply,
    },
    FnRow {
        id: "ConfigOptions[603].apply",
        hash: "e2568e7b20393d0dd8dbf46e9bef82ac15382dd667fd974c041696bd0b02c1eb",
        f: opt_603_apply,
    },
    FnRow {
        id: "ConfigOptions[604].apply",
        hash: "f04840454689569495f95e78d6433462cd80445a1ce15ccdbed1ffacec5d8355",
        f: opt_604_apply,
    },
    FnRow {
        id: "ConfigOptions[605].apply",
        hash: "ee590f9bc502168f0ac261cf0afedffb90e4c630b2484e9a175d4a1f8a0c0d89",
        f: opt_605_apply,
    },
    FnRow {
        id: "ConfigOptions[606].apply",
        hash: "4a965a7a6c47a34cea11d1cada623540d3a652adc9820e8cbd1d82a57cc0aa2e",
        f: opt_606_apply,
    },
    FnRow {
        id: "ConfigOptions[607].apply",
        hash: "367da413f199d3aedebb4f1a7c397d52449390def7ef7e6ee14e8be040037a65",
        f: opt_607_apply,
    },
    FnRow {
        id: "ConfigOptions[608].apply",
        hash: "4e54624bdca3c592fd1d5f3d5d1963ff98f7932d3c310309b96c027ac87a1218",
        f: opt_608_apply,
    },
    FnRow {
        id: "ConfigOptions[609].apply",
        hash: "e5e52c42771e71a87f10d243831e6db6b7b0112f64a562164afbaed328043fec",
        f: opt_609_apply,
    },
    FnRow {
        id: "ConfigOptions[610].apply",
        hash: "ab1897316ce49300a30d9c59125e581635109fd8b82ccfc39770bedb13d584bb",
        f: opt_610_apply,
    },
    FnRow {
        id: "ConfigOptions[611].apply",
        hash: "ed8963b21718a1d023e2b20af2c9eafae76d9873338c87fab0012e6fee3b2edf",
        f: opt_611_apply,
    },
    FnRow {
        id: "ConfigOptions[612].apply",
        hash: "020e51ef4146fd062a2ba19bdd0fd22d088a4f3c8b6300eade1de9b5e3c00365",
        f: opt_612_apply,
    },
    FnRow {
        id: "ConfigOptions[613].apply",
        hash: "a1c7ce431e9ddf84a40b88db0afe9b7fc5053c36824cc1ea1a5d0669c48068f6",
        f: opt_613_apply,
    },
    FnRow {
        id: "ConfigOptions[614].apply",
        hash: "ca68ecb1befa1f8b2d6b257412eaa444e13f1cff6dec600f151f92a4d9569898",
        f: opt_614_apply,
    },
    FnRow {
        id: "ConfigOptions[615].apply",
        hash: "56c21c2c0e8ba7dc6d875502bf9ee2b4458e54aa295bb6add2a83fab29fd8e43",
        f: opt_615_apply,
    },
    FnRow {
        id: "ConfigOptions[616].apply",
        hash: "b7779da6a5a312d2e90e213c53ee59ff967e7acbc124ea546ac9867e96e64ffa",
        f: opt_616_apply,
    },
    FnRow {
        id: "ConfigOptions[617].apply",
        hash: "1d977f40ea6eea9c4f1fd47ec83ffb7fdfbe59cb2ace7339bccebb9a58049a98",
        f: opt_617_apply,
    },
    FnRow {
        id: "ConfigOptions[618].apply",
        hash: "fe1e720ead53e6a40ffb9adfb3561e9c4adb6c6afd2b97018da0dc82754689ee",
        f: opt_618_apply,
    },
    FnRow {
        id: "ConfigOptions[619].apply",
        hash: "c25756d58b87c7f1f01927a393f1e27da76b726f9a1a3f7800b548dba602e303",
        f: opt_619_apply,
    },
    FnRow {
        id: "ConfigOptions[620].apply",
        hash: "2fb2cc5abe217c7e888c795ea9c29f51aa75ad2d9c4c845fc1e436cdbc00170f",
        f: opt_620_apply,
    },
    FnRow {
        id: "ConfigOptions[621].apply",
        hash: "d0d9c781c6d71b1d29761e0ea78a93ee09413bb608586a2876378377a3195530",
        f: opt_621_apply,
    },
    FnRow {
        id: "ConfigOptions[622].apply",
        hash: "41773493b2f07d0b9e78e49b79005b355411095aa29618df1f640bfa4e1fb27f",
        f: opt_622_apply,
    },
    FnRow {
        id: "ConfigOptions[623].apply",
        hash: "4a1c647951cac7edba55388ecf804d29d6dde7fc6ac28b618b4ffbf750ae3581",
        f: opt_623_apply,
    },
    FnRow {
        id: "ConfigOptions[624].apply",
        hash: "d329bc0dd06ad1c885e3ae6248443de8ba5ffaa1ac6bab5a8a478560d25d16f1",
        f: opt_624_apply,
    },
    FnRow {
        id: "ConfigOptions[625].apply",
        hash: "c9def3edce5332d1331e5f910b78ac2f133c88e9a7afb2ea17bf4760d7cafc46",
        f: opt_625_apply,
    },
    FnRow {
        id: "ConfigOptions[626].apply",
        hash: "5eaf70ca42ba18c9949bd5476269f067a3619409fc93bfe306856e489b01726a",
        f: opt_626_apply,
    },
    FnRow {
        id: "ConfigOptions[627].apply",
        hash: "e5259654d8fd7e9bd1d1c73607893eead8c078a0a442f6584707acda19fc0708",
        f: opt_627_apply,
    },
    FnRow {
        id: "ConfigOptions[628].apply",
        hash: "8473a9541af601b5711976582052e4559c8e66b390d7d3218eeea3220fddd3fe",
        f: opt_628_apply,
    },
    FnRow {
        id: "ConfigOptions[629].apply",
        hash: "9cb6355b4dc4dc7f2018ef7a4d25f063f4222fc546059257f90c797613bcda6c",
        f: opt_629_apply,
    },
    FnRow {
        id: "ConfigOptions[630].apply",
        hash: "17acc07165771490984bb4d6da5323bf224b5c627440f991fa3c5948c4a72d91",
        f: opt_630_apply,
    },
    FnRow {
        id: "ConfigOptions[631].apply",
        hash: "73c61da47e3ebf5b614fcc76212c4d476ac0aa3ef29eca05e16d11cf9c8d009f",
        f: opt_631_apply,
    },
    FnRow {
        id: "ConfigOptions[632].apply",
        hash: "493f7f1871a529a643d62430d9acfa7c21ad3e4978b1b624d462bdb7e2f2213f",
        f: opt_632_apply,
    },
    FnRow {
        id: "ConfigOptions[633].apply",
        hash: "92480e57b85efb9403bb347d3b521a6ef227d94cb8f756475cd9bf4d52b3349e",
        f: opt_633_apply,
    },
    FnRow {
        id: "ConfigOptions[634].apply",
        hash: "8107201b77e08a42a676a4b9852a9efd91b0c9544746ff8ff39cb8132d73759f",
        f: opt_634_apply,
    },
    FnRow {
        id: "ConfigOptions[635].apply",
        hash: "8608d1d97f6df781a2cfc4122ec07ce666a88aa34492495a82be22e058a8ec57",
        f: opt_635_apply,
    },
    FnRow {
        id: "ConfigOptions[636].apply",
        hash: "3eeb0f9cb8b29c18dcc1eaec447ca63b499a290ef49a907a8d8892dd186d41fc",
        f: opt_636_apply,
    },
    FnRow {
        id: "ConfigOptions[637].apply",
        hash: "f347d42daea2c77680231fb0227164313de6d61e76716ef1491d0636bdf3993c",
        f: opt_637_apply,
    },
    FnRow {
        id: "ConfigOptions[638].apply",
        hash: "b5cd53199066b1fc27fa1fdd534f702eaea01be7631059e53b1ebfc2d9544e3c",
        f: opt_638_apply,
    },
    FnRow {
        id: "ConfigOptions[639].apply",
        hash: "232d8eeb3131080fb86114a6caef6a9b206c26cda2be344caa8aa0614fc7b75a",
        f: opt_639_apply,
    },
    FnRow {
        id: "ConfigOptions[640].apply",
        hash: "d1e573e80cdf59a0b7794db096b38d3237829ca2d65d7bd2cd6b880bd0a5bd37",
        f: opt_640_apply,
    },
    FnRow {
        id: "ConfigOptions[641].apply",
        hash: "7b8d5f7d2dbc3bb3599edd9b1c302b0b3d5f0b561d4e8fd9ee450270b93fdb61",
        f: opt_641_apply,
    },
    FnRow {
        id: "ConfigOptions[642].apply",
        hash: "5a9ec9d01817f8dc36c2eee918cfd11dbbd4725084387c566902e414a25146fd",
        f: opt_642_apply,
    },
    FnRow {
        id: "ConfigOptions[643].apply",
        hash: "892e1a6fcd1eb97182ef1ad873629482a923ad9a3462255bf584b045b35897a3",
        f: opt_643_apply,
    },
    FnRow {
        id: "ConfigOptions[644].apply",
        hash: "031497b37a4843584ae80622a60f2780387f26dfc111e4807ca3be9331e0a125",
        f: opt_644_apply,
    },
    FnRow {
        id: "ConfigOptions[645].apply",
        hash: "4b005f3b83e64eb4013465f8f18d609ff43e2f757ab99341b68f63f20ad4490f",
        f: opt_645_apply,
    },
    FnRow {
        id: "ConfigOptions[646].apply",
        hash: "c0cecfc24a59ddacada583adf426065a255444eb27139b7220443bd2196f1ea8",
        f: opt_646_apply,
    },
    FnRow {
        id: "ConfigOptions[647].apply",
        hash: "fa9e137314f91168109ee52e40df674bfe62423a6fd60e66d78a65ed5a067a60",
        f: opt_647_apply,
    },
    FnRow {
        id: "ConfigOptions[648].apply",
        hash: "2615840c7ec67f6c2c33ae3c510f0beef25a4c871e3ba58c0566e39b62585b7a",
        f: opt_648_apply,
    },
    FnRow {
        id: "ConfigOptions[649].apply",
        hash: "be605911fc7a8eae8580714d7ad958a00f7125d25257af03f3adee294f8392cb",
        f: opt_649_apply,
    },
    FnRow {
        id: "ConfigOptions[651].apply",
        hash: "2bfbde1763fd4ef6f9d101807820a7de9de85e53b21ad65f2b24bf4535e09df1",
        f: opt_651_apply,
    },
    FnRow {
        id: "ConfigOptions[652].apply",
        hash: "91dd92aff7806c7a4ddec3defde269e1c62d0d5840b4ef145ef9385c6030b809",
        f: opt_652_apply,
    },
    FnRow {
        id: "ConfigOptions[653].apply",
        hash: "95fe62616e7089015b1917c4f0b0acdf80116f0604063313e0c122b40a97e607",
        f: opt_653_apply,
    },
    FnRow {
        id: "ConfigOptions[654].apply",
        hash: "0e6dfa5163635e27b8d943ed27a34e6a2b49bb6d5a77f5b0ad5ed4b841bfd747",
        f: opt_654_apply,
    },
    FnRow {
        id: "ConfigOptions[655].apply",
        hash: "d3f3d3dec2eddc2beb592c290f36d41cc3a34851405c9bdd24580fce825c5d8c",
        f: opt_655_apply,
    },
    FnRow {
        id: "ConfigOptions[658].apply",
        hash: "bc6a496666e85b5019b67db8ae1554dc60b70311ddb715ad1a2f007e1d8d9a5e",
        f: opt_658_apply,
    },
    FnRow {
        id: "ConfigOptions[659].apply",
        hash: "0c63f14001520552cd4d5379f9f5636ba8e881470124bbf61cba1ddd6ee5d316",
        f: opt_659_apply,
    },
    FnRow {
        id: "ConfigOptions[660].apply",
        hash: "86434cd5153d9879a5ab0aaee11ef43e5dfe92a0003325043a08b459f2bf0192",
        f: opt_660_apply,
    },
    FnRow {
        id: "ConfigOptions[661].apply",
        hash: "634a29e9261ef68cb826fc5250d607b2e29f5be2e2b69739e4ba5c8ad927ecee",
        f: opt_661_apply,
    },
    FnRow {
        id: "ConfigOptions[662].apply",
        hash: "4dc0ff09bcdba38aeeab082649222c2ebcf86fe0c608136d2690bdecdeb2e924",
        f: opt_662_apply,
    },
    FnRow {
        id: "ConfigOptions[663].apply",
        hash: "b6ba75e088d68118f9285d656bf17bb4d4e5ce0c0a148996f073436248e29f6d",
        f: opt_663_apply,
    },
    FnRow {
        id: "ConfigOptions[664].apply",
        hash: "36cd29b3768a0c1e9d8b6195ddc889e801c891d8a68cb7830c5279d696f3e576",
        f: opt_664_apply,
    },
    FnRow {
        id: "ConfigOptions[665].apply",
        hash: "cd6772cf0cdabef4ca44bb22f9714e7843bc0912b7a138da00e3845ae66269d8",
        f: opt_665_apply,
    },
    FnRow {
        id: "ConfigOptions[666].apply",
        hash: "63673d557f60c61107cd5bd0b068845771e99bb12f9f41decf388313914e1b49",
        f: opt_666_apply,
    },
    FnRow {
        id: "ConfigOptions[667].apply",
        hash: "6a3b71977c72ac469a9c053d35977aa0bed66065cb9c306d83d0a7dec6e260bd",
        f: opt_667_apply,
    },
    FnRow {
        id: "ConfigOptions[668].apply",
        hash: "f77251e205681bd29d074a0779dce259ce2f5d2a778d9fd71fdc3ad56dcc78fa",
        f: opt_668_apply,
    },
    FnRow {
        id: "ConfigOptions[669].apply",
        hash: "e15192b77f669f4e98625a9d907d70bbf4eacb00093960263179aed865ae6bd0",
        f: opt_669_apply,
    },
    FnRow {
        id: "ConfigOptions[670].apply",
        hash: "5ee18b51ad867d8b1a16034a80ab73595ed76988455c03766b7301bf005e7203",
        f: opt_670_apply,
    },
    FnRow {
        id: "ConfigOptions[674].apply",
        hash: "4c62b2235a2d837991bbf68505d967403b7d3af66d5dbd35f438678d1b8a764d",
        f: opt_674_apply,
    },
];

/// @pob Modules/ConfigOptions.lua:1189-1191 h=362ecf72
/// `ConfigOptions[393].apply`
/// var = "conditionBurning"
pub(crate) fn opt_393_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1190
    modList.NewMod(
        n!("Condition:Burning"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1192-1194 h=0c7a7c68
/// `ConfigOptions[394].apply`
/// var = "conditionIgnited"
pub(crate) fn opt_394_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1193
    modList.NewMod(
        n!("Condition:Ignited"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1195-1197 h=564c62c8
/// `ConfigOptions[395].apply`
/// var = "conditionScorched"
pub(crate) fn opt_395_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1196
    modList.NewMod(
        n!("Condition:Scorched"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1198-1200 h=302643c1
/// `ConfigOptions[396].apply`
/// var = "conditionChilled"
pub(crate) fn opt_396_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1199
    modList.NewMod(
        n!("Condition:Chilled"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1201-1203 h=3619f89e
/// `ConfigOptions[397].apply`
/// var = "conditionChilledEffect"
pub(crate) fn opt_397_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1202
    modList.NewMod(
        n!("ChillVal"),
        OVERRIDE,
        val_value(&val),
        "Chill",
        0,
        0,
        &[tag!({ type = Condition, var = "Chilled" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1204-1206 h=58b22203
/// `ConfigOptions[398].apply`
/// var = "conditionFrozen"
pub(crate) fn opt_398_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1205
    modList.NewMod(
        n!("Condition:Frozen"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1207-1209 h=63a3806b
/// `ConfigOptions[399].apply`
/// var = "conditionBrittle"
pub(crate) fn opt_399_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1208
    modList.NewMod(
        n!("Condition:Brittle"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1210-1212 h=8188c34e
/// `ConfigOptions[400].apply`
/// var = "conditionShocked"
pub(crate) fn opt_400_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1211
    modList.NewMod(
        n!("Condition:Shocked"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1213-1215 h=20b88706
/// `ConfigOptions[401].apply`
/// var = "conditionPlayerShockEffect"
pub(crate) fn opt_401_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1214
    modList.NewMod(
        n!("ShockVal"),
        OVERRIDE,
        val_value(&val),
        "Shock",
        0,
        0,
        &[tag!({ type = Condition, var = "Shocked" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1216-1218 h=751be7e5
/// `ConfigOptions[402].apply`
/// var = "conditionSapped"
pub(crate) fn opt_402_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1217
    modList.NewMod(
        n!("Condition:Sapped"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1219-1221 h=fa126f91
/// `ConfigOptions[403].apply`
/// var = "conditionBleeding"
pub(crate) fn opt_403_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1220
    modList.NewMod(
        n!("Condition:Bleeding"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1222-1224 h=951aaf6b
/// `ConfigOptions[404].apply`
/// var = "conditionPoisoned"
pub(crate) fn opt_404_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1223
    modList.NewMod(
        n!("Condition:Poisoned"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1225-1227 h=4a2bcc76
/// `ConfigOptions[405].apply`
/// var = "conditionCanBeCurseImmune"
pub(crate) fn opt_405_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1226
    modList.NewMod(
        n!("AvoidCurse"),
        BASE,
        100.0,
        "Config",
        0,
        0,
        &[
            tag!({ type = Condition, var = "Combat" }),
            tag!({ type = GlobalEffect, effectType = "Global", unscalable = true }),
        ],
    );
}

/// @pob Modules/ConfigOptions.lua:1228-1230 h=286bba8d
/// `ConfigOptions[406].apply`
/// var = "multiplierPoisonOnSelf"
pub(crate) fn opt_406_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1229
    modList.NewMod(
        n!("Multiplier:PoisonStack"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1231-1233 h=c50e083b
/// `ConfigOptions[407].apply`
/// var = "multiplierWitheredStackCountSelf"
pub(crate) fn opt_407_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1232
    modList.NewMod(
        n!("Multiplier:WitheredStack"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1234-1237 h=3c9f16d5
/// `ConfigOptions[408].apply`
/// var = "multiplierNearbyEnemies"
pub(crate) fn opt_408_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1235
    modList.NewMod(
        n!("Multiplier:NearbyEnemies"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1236
    modList.NewMod(
        n!("Condition:OnlyOneNearbyEnemy"),
        FLAG,
        val_eq_num(&val, 1.0),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1238-1243 h=63a0e52a
/// `ConfigOptions[409].apply`
/// var = "multiplierNearbyRareOrUniqueEnemies"
pub(crate) fn opt_409_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1239
    modList.NewMod(
        n!("Multiplier:NearbyRareOrUniqueEnemies"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1240
    modList.NewMod(
        n!("Multiplier:NearbyEnemies"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1241
    modList.NewMod(
        n!("Condition:AtMostOneNearbyRareOrUniqueEnemy"),
        FLAG,
        (cmp_num(&val) <= 1.0),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1242
    enemyModList.NewMod(
        n!("Condition:NearbyRareOrUniqueEnemy"),
        FLAG,
        (cmp_num(&val) >= 1.0),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1244-1246 h=f8479945
/// `ConfigOptions[410].apply`
/// var = "conditionHitRecently"
pub(crate) fn opt_410_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1245
    modList.NewMod(
        n!("Condition:HitRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1247-1250 h=71a1b564
/// `ConfigOptions[411].apply`
/// var = "conditionHitSpellRecently"
pub(crate) fn opt_411_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1248
    modList.NewMod(
        n!("Condition:HitSpellRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1249
    modList.NewMod(
        n!("Condition:HitRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1251-1254 h=6283a2d0
/// `ConfigOptions[412].apply`
/// var = "conditionCritRecently"
pub(crate) fn opt_412_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1252
    modList.NewMod(
        n!("Condition:CritRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1253
    modList.NewMod(
        n!("Condition:SkillCritRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1255-1257 h=a99b8466
/// `ConfigOptions[413].apply`
/// var = "conditionSkillCritRecently"
pub(crate) fn opt_413_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1256
    modList.NewMod(
        n!("Condition:SkillCritRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1258-1260 h=8f9dcf84
/// `ConfigOptions[414].apply`
/// var = "conditionCritWithHeraldSkillRecently"
pub(crate) fn opt_414_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1259
    modList.NewMod(
        n!("Condition:CritWithHeraldSkillRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1261-1264 h=49b3a7ad
/// `ConfigOptions[415].apply`
/// var = "multiplierCritRecently"
pub(crate) fn opt_415_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1262
    modList.NewMod(
        n!("Multiplier:CritRecently"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1263
    modList.NewMod(
        n!("Condition:SkillCritRecently"),
        FLAG,
        (1.0 <= cmp_num(&val)),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1265-1267 h=75a7795c
/// `ConfigOptions[416].apply`
/// var = "LostNonVaalBuffRecently"
pub(crate) fn opt_416_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1266
    modList.NewMod(
        n!("Condition:LostNonVaalBuffRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1268-1270 h=4d91cec2
/// `ConfigOptions[417].apply`
/// var = "conditionNonCritRecently"
pub(crate) fn opt_417_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1269
    modList.NewMod(
        n!("Condition:NonCritRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1271-1273 h=e542c3e8
/// `ConfigOptions[418].apply`
/// var = "conditionChannelling"
pub(crate) fn opt_418_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1272
    modList.NewMod(
        n!("Condition:Channelling"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1274-1277 h=cd45a60b
/// `ConfigOptions[419].apply`
/// var = "multiplierChannelling"
pub(crate) fn opt_419_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1275
    modList.NewMod(
        n!("Multiplier:ChannellingTime"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1276
    modList.NewMod(
        n!("Condition:Channelling"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1278-1280 h=f4812c3f
/// `ConfigOptions[420].apply`
/// var = "conditionHitRecentlyWithWeapon"
pub(crate) fn opt_420_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1279
    modList.NewMod(
        n!("Condition:HitRecentlyWithWeapon"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1281-1283 h=30138bf1
/// `ConfigOptions[421].apply`
/// var = "conditionKilledRecently"
pub(crate) fn opt_421_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1282
    modList.NewMod(
        n!("Condition:KilledRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1284-1287 h=c3878f2f
/// `ConfigOptions[422].apply`
/// var = "multiplierKilledRecently"
pub(crate) fn opt_422_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1285
    modList.NewMod(
        n!("Multiplier:EnemyKilledRecently"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1286
    modList.NewMod(
        n!("Condition:KilledRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1288-1290 h=7ae0a49a
/// `ConfigOptions[423].apply`
/// var = "conditionKilledLast3Seconds"
pub(crate) fn opt_423_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1289
    modList.NewMod(
        n!("Condition:KilledLast3Seconds"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1291-1293 h=ed18c1e9
/// `ConfigOptions[424].apply`
/// var = "conditionKilledPoisonedLast2Seconds"
pub(crate) fn opt_424_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1292
    modList.NewMod(
        n!("Condition:KilledPoisonedLast2Seconds"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1294-1296 h=dbbeefbd
/// `ConfigOptions[425].apply`
/// var = "conditionKilledTauntedEnemyRecently"
pub(crate) fn opt_425_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1295
    modList.NewMod(
        n!("Condition:KilledTauntedEnemyRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1297-1299 h=49815ebb
/// `ConfigOptions[426].apply`
/// var = "conditionTotemsNotSummonedInPastTwoSeconds"
pub(crate) fn opt_426_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1298
    modList.NewMod(
        n!("Condition:NoSummonedTotemsInPastTwoSeconds"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1300-1302 h=3a7538a7
/// `ConfigOptions[427].apply`
/// var = "conditionTotemsKilledRecently"
pub(crate) fn opt_427_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1301
    modList.NewMod(
        n!("Condition:TotemsKilledRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1303-1305 h=56116e09
/// `ConfigOptions[428].apply`
/// var = "conditionTotemsHitRecently"
pub(crate) fn opt_428_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1304
    modList.NewMod(
        n!("Condition:TotemsHitRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1306-1309 h=12e9af0b
/// `ConfigOptions[429].apply`
/// var = "conditionTotemsHitSpellRecently"
pub(crate) fn opt_429_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1307
    modList.NewMod(
        n!("Condition:TotemsHitSpellRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1308
    modList.NewMod(
        n!("Condition:TotemsHitRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1310-1312 h=e993b32a
/// `ConfigOptions[430].apply`
/// var = "conditionUsedBrandRecently"
pub(crate) fn opt_430_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1311
    modList.NewMod(
        n!("Condition:UsedBrandRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1313-1316 h=83a23908
/// `ConfigOptions[431].apply`
/// var = "multiplierTotemsKilledRecently"
pub(crate) fn opt_431_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1314
    modList.NewMod(
        n!("Multiplier:EnemyKilledByTotemsRecently"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1315
    modList.NewMod(
        n!("Condition:TotemsKilledRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1317-1319 h=d293f66f
/// `ConfigOptions[432].apply`
/// var = "conditionMinionsKilledRecently"
pub(crate) fn opt_432_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1318
    modList.NewMod(
        n!("Condition:MinionsKilledRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1320-1322 h=7a663318
/// `ConfigOptions[433].apply`
/// var = "conditionMinionsDiedRecently"
pub(crate) fn opt_433_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1321
    modList.NewMod(
        n!("Condition:MinionsDiedRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1323-1326 h=a79d7a44
/// `ConfigOptions[434].apply`
/// var = "multiplierMinionsKilledRecently"
pub(crate) fn opt_434_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1324
    modList.NewMod(
        n!("Multiplier:EnemyKilledByMinionsRecently"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1325
    modList.NewMod(
        n!("Condition:MinionsKilledRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1327-1329 h=0aa4b1e9
/// `ConfigOptions[435].apply`
/// var = "conditionKilledAffectedByDoT"
pub(crate) fn opt_435_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1328
    modList.NewMod(
        n!("Condition:KilledAffectedByDotRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1330-1332 h=65fdc8b0
/// `ConfigOptions[436].apply`
/// var = "multiplierShockedEnemyKilledRecently"
pub(crate) fn opt_436_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1331
    modList.NewMod(
        n!("Multiplier:ShockedEnemyKilledRecently"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1333-1335 h=45b97a78
/// `ConfigOptions[437].apply`
/// var = "multiplierShockedNonShockedEnemyRecently"
pub(crate) fn opt_437_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1334
    modList.NewMod(
        n!("Multiplier:ShockedNonShockedEnemyRecently"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1336-1338 h=cead0075
/// `ConfigOptions[438].apply`
/// var = "conditionFrozenEnemyRecently"
pub(crate) fn opt_438_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1337
    modList.NewMod(
        n!("Condition:FrozenEnemyRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1339-1341 h=afdc3b26
/// `ConfigOptions[439].apply`
/// var = "conditionChilledEnemyRecently"
pub(crate) fn opt_439_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1340
    modList.NewMod(
        n!("Condition:ChilledEnemyRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1342-1344 h=be3a0753
/// `ConfigOptions[440].apply`
/// var = "conditionShatteredEnemyRecently"
pub(crate) fn opt_440_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1343
    modList.NewMod(
        n!("Condition:ShatteredEnemyRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1345-1347 h=ae9b7df1
/// `ConfigOptions[441].apply`
/// var = "conditionIgnitedEnemyRecently"
pub(crate) fn opt_441_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1346
    modList.NewMod(
        n!("Condition:IgnitedEnemyRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1348-1350 h=3083e6bc
/// `ConfigOptions[442].apply`
/// var = "multiplierIgniteAppliedRecently"
pub(crate) fn opt_442_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1349
    modList.NewMod(
        n!("Multiplier:IgniteAppliedRecently"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1351-1353 h=0abe0cf9
/// `ConfigOptions[443].apply`
/// var = "conditionShockedEnemyRecently"
pub(crate) fn opt_443_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1352
    modList.NewMod(
        n!("Condition:ShockedEnemyRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1354-1356 h=0a4a2c53
/// `ConfigOptions[444].apply`
/// var = "conditionStunnedEnemyRecently"
pub(crate) fn opt_444_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1355
    modList.NewMod(
        n!("Condition:StunnedEnemyRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1357-1359 h=3005c7bd
/// `ConfigOptions[445].apply`
/// var = "conditionStunnedRecently"
pub(crate) fn opt_445_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1358
    modList.NewMod(
        n!("Condition:StunnedRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1360-1362 h=e5886584
/// `ConfigOptions[446].apply`
/// var = "conditionPoisonedEnemyRecently"
pub(crate) fn opt_446_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1361
    modList.NewMod(
        n!("Condition:PoisonedEnemyRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1363-1365 h=68029086
/// `ConfigOptions[447].apply`
/// var = "multiplierPoisonAppliedRecently"
pub(crate) fn opt_447_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1364
    modList.NewMod(
        n!("Multiplier:PoisonAppliedRecently"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1366-1368 h=db41bab4
/// `ConfigOptions[448].apply`
/// var = "multiplierLifeSpentRecently"
pub(crate) fn opt_448_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1367
    modList.NewMod(
        n!("Multiplier:LifeSpentRecently"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1369-1371 h=a49064d0
/// `ConfigOptions[449].apply`
/// var = "multiplierManaSpentRecently"
pub(crate) fn opt_449_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1370
    modList.NewMod(
        n!("Multiplier:ManaSpentRecently"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1372-1374 h=c2d11696
/// `ConfigOptions[450].apply`
/// var = "conditionWardBrokenPast2Seconds"
pub(crate) fn opt_450_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1373
    modList.NewMod(
        n!("Condition:WardBrokenPast2Seconds"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1375-1377 h=277739f6
/// `ConfigOptions[451].apply`
/// var = "conditionBeenHitRecently"
pub(crate) fn opt_451_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1376
    modList.NewMod(
        n!("Condition:BeenHitRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1378-1381 h=67900d55
/// `ConfigOptions[452].apply`
/// var = "multiplierBeenHitRecently"
pub(crate) fn opt_452_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1379
    modList.NewMod(
        n!("Multiplier:BeenHitRecently"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1380
    modList.NewMod(
        n!("Condition:BeenHitRecently"),
        FLAG,
        (1.0 <= cmp_num(&val)),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1382-1384 h=19f84ec9
/// `ConfigOptions[453].apply`
/// var = "conditionBeenHitByAttackRecently"
pub(crate) fn opt_453_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1383
    modList.NewMod(
        n!("Condition:BeenHitByAttackRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1385-1387 h=06aa056e
/// `ConfigOptions[454].apply`
/// var = "conditionBeenCritRecently"
pub(crate) fn opt_454_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1386
    modList.NewMod(
        n!("Condition:BeenCritRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1388-1390 h=1c3a8dfe
/// `ConfigOptions[455].apply`
/// var = "conditionConsumed12SteelShardsRecently"
pub(crate) fn opt_455_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1389
    modList.NewMod(
        n!("Condition:Consumed12SteelShardsRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1391-1393 h=051dc383
/// `ConfigOptions[456].apply`
/// var = "conditionGainedPowerChargeRecently"
pub(crate) fn opt_456_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1392
    modList.NewMod(
        n!("Condition:GainedPowerChargeRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1394-1396 h=93bb40fb
/// `ConfigOptions[457].apply`
/// var = "conditionGainedFrenzyChargeRecently"
pub(crate) fn opt_457_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1395
    modList.NewMod(
        n!("Condition:GainedFrenzyChargeRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1397-1400 h=20d3383d
/// `ConfigOptions[458].apply`
/// var = "conditionBeenSavageHitRecently"
pub(crate) fn opt_458_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1398
    modList.NewMod(
        n!("Condition:BeenSavageHitRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1399
    modList.NewMod(
        n!("Condition:BeenHitRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1401-1404 h=3650880c
/// `ConfigOptions[459].apply`
/// var = "conditionHitByFireDamageRecently"
pub(crate) fn opt_459_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1402
    modList.NewMod(
        n!("Condition:HitByFireDamageRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1403
    modList.NewMod(
        n!("Condition:BeenHitRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1405-1408 h=90d826c9
/// `ConfigOptions[460].apply`
/// var = "conditionHitByColdDamageRecently"
pub(crate) fn opt_460_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1406
    modList.NewMod(
        n!("Condition:HitByColdDamageRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1407
    modList.NewMod(
        n!("Condition:BeenHitRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1409-1412 h=1a311f0d
/// `ConfigOptions[461].apply`
/// var = "conditionHitByLightningDamageRecently"
pub(crate) fn opt_461_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1410
    modList.NewMod(
        n!("Condition:HitByLightningDamageRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1411
    modList.NewMod(
        n!("Condition:BeenHitRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1413-1416 h=e0fee06d
/// `ConfigOptions[462].apply`
/// var = "conditionHitBySpellDamageRecently"
pub(crate) fn opt_462_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1414
    modList.NewMod(
        n!("Condition:HitBySpellDamageRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1415
    modList.NewMod(
        n!("Condition:BeenHitRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1417-1420 h=7f741e1c
/// `ConfigOptions[463].apply`
/// var = "conditionTakenFireDamageFromEnemyHitRecently"
pub(crate) fn opt_463_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1418
    modList.NewMod(
        n!("Condition:TakenFireDamageFromEnemyHitRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1419
    modList.NewMod(
        n!("Condition:BeenHitRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1421-1423 h=e64e2700
/// `ConfigOptions[464].apply`
/// var = "conditionBlockedRecently"
pub(crate) fn opt_464_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1422
    modList.NewMod(
        n!("Condition:BlockedRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1424-1427 h=e5491c6b
/// `ConfigOptions[465].apply`
/// var = "conditionBlockedAttackRecently"
pub(crate) fn opt_465_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1425
    modList.NewMod(
        n!("Condition:BlockedAttackRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1426
    modList.NewMod(
        n!("Condition:BlockedRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1428-1431 h=73b31f13
/// `ConfigOptions[466].apply`
/// var = "conditionBlockedSpellRecently"
pub(crate) fn opt_466_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1429
    modList.NewMod(
        n!("Condition:BlockedSpellRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1430
    modList.NewMod(
        n!("Condition:BlockedRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1432-1434 h=81a9ed5d
/// `ConfigOptions[467].apply`
/// var = "conditionEnergyShieldRechargeRecently"
pub(crate) fn opt_467_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1433
    modList.NewMod(
        n!("Condition:EnergyShieldRechargeRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1435-1437 h=681c98d9
/// `ConfigOptions[468].apply`
/// var = "conditionEnergyShieldRechargePastTwoSec"
pub(crate) fn opt_468_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1436
    modList.NewMod(
        n!("Condition:EnergyShieldRechargePastTwoSec"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1438-1440 h=886c5de6
/// `ConfigOptions[469].apply`
/// var = "conditionStoppedTakingDamageOverTimeRecently"
pub(crate) fn opt_469_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1439
    modList.NewMod(
        n!("Condition:StoppedTakingDamageOverTimeRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1441-1443 h=305e14b2
/// `ConfigOptions[470].apply`
/// var = "conditionConvergence"
pub(crate) fn opt_470_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1442
    modList.NewMod(
        n!("Condition:Convergence"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[
            tag!({ type = Condition, var = "Combat" }),
            tag!({ type = Condition, var = "CanGainConvergence" }),
        ],
    );
}

/// @pob Modules/ConfigOptions.lua:1444-1450 h=71a45fcf
/// `ConfigOptions[471].apply`
/// var = "buffPendulum"
pub(crate) fn opt_471_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1445
    if val_eq_str(&val, "AREA") {
        // L1446
        modList.NewMod(
            n!("Condition:PendulumOfDestructionAreaOfEffect"),
            FLAG,
            true,
            "Config",
            0,
            0,
            &[tag!({ type = Condition, var = "Combat" })],
        );
    } else if val_eq_str(&val, "DAMAGE") {
        // L1448
        modList.NewMod(
            n!("Condition:PendulumOfDestructionElementalDamage"),
            FLAG,
            true,
            "Config",
            0,
            0,
            &[tag!({ type = Condition, var = "Combat" })],
        );
    }
}

/// @pob Modules/ConfigOptions.lua:1451-1461 h=e8826bcc
/// `ConfigOptions[472].apply`
/// var = "buffConflux"
pub(crate) fn opt_472_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1452
    if (val_eq_str(&val, "CHILLING") || val_eq_str(&val, "ALL")) {
        // L1453
        modList.NewMod(
            n!("Condition:ChillingConflux"),
            FLAG,
            true,
            "Config",
            0,
            0,
            &[tag!({ type = Condition, var = "Combat" })],
        );
    }
    // L1455
    if (val_eq_str(&val, "SHOCKING") || val_eq_str(&val, "ALL")) {
        // L1456
        modList.NewMod(
            n!("Condition:ShockingConflux"),
            FLAG,
            true,
            "Config",
            0,
            0,
            &[tag!({ type = Condition, var = "Combat" })],
        );
    }
    // L1458
    if (val_eq_str(&val, "IGNITING") || val_eq_str(&val, "ALL")) {
        // L1459
        modList.NewMod(
            n!("Condition:IgnitingConflux"),
            FLAG,
            true,
            "Config",
            0,
            0,
            &[tag!({ type = Condition, var = "Combat" })],
        );
    }
}

/// @pob Modules/ConfigOptions.lua:1462-1467 h=a166ec38
/// `ConfigOptions[473].apply`
/// var = "highestDamageType"
pub(crate) fn opt_473_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1463
    if !val_eq_str(&val, "NONE") {
        // L1464
        modList.NewMod(
            cat!("Condition:", val_str(&val), "IsHighestDamageType"),
            FLAG,
            true,
            "Config",
            0,
            0,
            &[],
        );
        // L1465
        modList.NewMod(
            n!("IsHighestDamageTypeOVERRIDE"),
            FLAG,
            true,
            "Config",
            0,
            0,
            &[],
        );
    }
}

/// @pob Modules/ConfigOptions.lua:1468-1476 h=cbfe75dc
/// `ConfigOptions[474].apply`
/// var = "buffHeartstopper"
pub(crate) fn opt_474_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1469
    if val_eq_str(&val, "HIT") {
        // L1470
        modList.NewMod(
            n!("Condition:HeartstopperHIT"),
            FLAG,
            true,
            "Config",
            0,
            0,
            &[tag!({ type = Condition, var = "Combat" })],
        );
    } else if val_eq_str(&val, "DOT") {
        // L1472
        modList.NewMod(
            n!("Condition:HeartstopperDOT"),
            FLAG,
            true,
            "Config",
            0,
            0,
            &[tag!({ type = Condition, var = "Combat" })],
        );
    } else if val_eq_str(&val, "AVERAGE") {
        // L1474
        modList.NewMod(
            n!("Condition:HeartstopperAVERAGE"),
            FLAG,
            true,
            "Config",
            0,
            0,
            &[tag!({ type = Condition, var = "Combat" })],
        );
    }
}

/// @pob Modules/ConfigOptions.lua:1477-1479 h=9240b06b
/// `ConfigOptions[475].apply`
/// var = "buffBastionOfHope"
pub(crate) fn opt_475_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1478
    modList.NewMod(
        n!("Condition:BastionOfHopeActive"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1480-1482 h=5315658d
/// `ConfigOptions[476].apply`
/// var = "buffNgamahuFlamesAdvance"
pub(crate) fn opt_476_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1481
    modList.NewMod(
        n!("Condition:NgamahuFlamesAdvance"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1483-1485 h=6d4d34e3
/// `ConfigOptions[477].apply`
/// var = "buffHerEmbrace"
pub(crate) fn opt_477_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1484
    modList.NewMod(
        n!("HerEmbrace"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[
            tag!({ type = Condition, var = "Combat" }),
            tag!({ type = Condition, var = "CanGainHerEmbrace" }),
        ],
    );
}

/// @pob Modules/ConfigOptions.lua:1486-1488 h=3199eceb
/// `ConfigOptions[478].apply`
/// var = "conditionChampionIntimidate"
pub(crate) fn opt_478_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1487
    enemyModList.NewMod(
        n!("Condition:ChampionIntimidate"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1489-1491 h=84956d17
/// `ConfigOptions[479].apply`
/// var = "conditionUsedSkillRecently"
pub(crate) fn opt_479_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1490
    modList.NewMod(
        n!("Condition:UsedSkillRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1492-1495 h=bd8e0f92
/// `ConfigOptions[480].apply`
/// var = "multiplierSkillUsedRecently"
pub(crate) fn opt_480_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1493
    modList.NewMod(
        n!("Multiplier:SkillUsedRecently"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1494
    modList.NewMod(
        n!("Condition:UsedSkillRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1496-1499 h=88ec1149
/// `ConfigOptions[481].apply`
/// var = "conditionAttackedRecently"
pub(crate) fn opt_481_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1497
    modList.NewMod(
        n!("Condition:AttackedRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1498
    modList.NewMod(
        n!("Condition:UsedSkillRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1500-1503 h=2c35850d
/// `ConfigOptions[482].apply`
/// var = "conditionCastSpellRecently"
pub(crate) fn opt_482_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1501
    modList.NewMod(
        n!("Condition:CastSpellRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1502
    modList.NewMod(
        n!("Condition:UsedSkillRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1504-1506 h=ea6d3c1c
/// `ConfigOptions[483].apply`
/// var = "multiplierNonInstantSpellCastRecently"
pub(crate) fn opt_483_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1505
    modList.NewMod(
        n!("Multiplier:NonInstantSpellCastRecently"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1507-1509 h=fa45cf70
/// `ConfigOptions[484].apply`
/// var = "multiplierAppliedAilmentsRecently"
pub(crate) fn opt_484_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1508
    modList.NewMod(
        n!("Multiplier:AppliedAilmentsRecently"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1510-1512 h=eaca078f
/// `ConfigOptions[485].apply`
/// var = "conditionLinkedRecently"
pub(crate) fn opt_485_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1511
    modList.NewMod(
        n!("Condition:LinkedRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[],
    );
}

/// @pob Modules/ConfigOptions.lua:1513-1515 h=5930746d
/// `ConfigOptions[486].apply`
/// var = "conditionStunnedWhileCastingRecently"
pub(crate) fn opt_486_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1514
    modList.NewMod(
        n!("Condition:StunnedWhileCastingRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1516-1518 h=09ac2ee5
/// `ConfigOptions[487].apply`
/// var = "conditionCastLast1Seconds"
pub(crate) fn opt_487_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1517
    modList.NewMod(
        n!("Condition:CastLast1Seconds"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1519-1521 h=8a078565
/// `ConfigOptions[488].apply`
/// var = "multiplierCastLast8Seconds"
pub(crate) fn opt_488_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1520
    modList.NewMod(
        n!("Multiplier:CastLast8Seconds"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1522-1524 h=8f766b0c
/// `ConfigOptions[489].apply`
/// var = "conditionSuppressedRecently"
pub(crate) fn opt_489_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1523
    modList.NewMod(
        n!("Condition:SuppressedRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1525-1528 h=bfa2b514
/// `ConfigOptions[490].apply`
/// var = "multiplierHitsSuppressedRecently"
pub(crate) fn opt_490_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1526
    modList.NewMod(
        n!("Multiplier:HitsSuppressedRecently"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1527
    modList.NewMod(
        n!("Condition:SuppressedRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1529-1532 h=e2ffa8a2
/// `ConfigOptions[491].apply`
/// var = "conditionUsedFireSkillRecently"
pub(crate) fn opt_491_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1530
    modList.NewMod(
        n!("Condition:UsedFireSkillRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1531
    modList.NewMod(
        n!("Condition:UsedSkillRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1533-1536 h=e6a99ae9
/// `ConfigOptions[492].apply`
/// var = "conditionUsedColdSkillRecently"
pub(crate) fn opt_492_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1534
    modList.NewMod(
        n!("Condition:UsedColdSkillRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1535
    modList.NewMod(
        n!("Condition:UsedSkillRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1537-1540 h=4a3f8fc4
/// `ConfigOptions[493].apply`
/// var = "conditionUsedMinionSkillRecently"
pub(crate) fn opt_493_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1538
    modList.NewMod(
        n!("Condition:UsedMinionSkillRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1539
    modList.NewMod(
        n!("Condition:UsedSkillRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1541-1545 h=a6a12c1c
/// `ConfigOptions[494].apply`
/// var = "conditionUsedTravelSkillRecently"
pub(crate) fn opt_494_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1542
    modList.NewMod(
        n!("Condition:UsedTravelSkillRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1543
    modList.NewMod(
        n!("Condition:UsedMovementSkillRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1544
    modList.NewMod(
        n!("Condition:UsedSkillRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1546-1551 h=0867b88b
/// `ConfigOptions[495].apply`
/// var = "conditionUsedDashRecently"
pub(crate) fn opt_495_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1547
    modList.NewMod(
        n!("Condition:CastDashRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1548
    modList.NewMod(
        n!("Condition:UsedTravelSkillRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1549
    modList.NewMod(
        n!("Condition:UsedMovementSkillRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1550
    modList.NewMod(
        n!("Condition:UsedSkillRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1552-1555 h=35a5b70f
/// `ConfigOptions[496].apply`
/// var = "conditionUsedMovementSkillRecently"
pub(crate) fn opt_496_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1553
    modList.NewMod(
        n!("Condition:UsedMovementSkillRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1554
    modList.NewMod(
        n!("Condition:UsedSkillRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1556-1559 h=ad17463c
/// `ConfigOptions[497].apply`
/// var = "conditionUsedVaalSkillRecently"
pub(crate) fn opt_497_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1557
    modList.NewMod(
        n!("Condition:UsedVaalSkillRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1558
    modList.NewMod(
        n!("Condition:UsedSkillRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1560-1562 h=c60467a9
/// `ConfigOptions[498].apply`
/// var = "multiplierUsedVaalSkillInPast8Seconds"
pub(crate) fn opt_498_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1561
    modList.NewMod(
        n!("Multiplier:VaalSkillsUsedInPast8Seconds"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1563-1565 h=3f0a89e4
/// `ConfigOptions[499].apply`
/// var = "conditionSoulGainPrevention"
pub(crate) fn opt_499_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1564
    modList.NewMod(
        n!("Condition:SoulGainPrevention"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1566-1568 h=bcb8b12e
/// `ConfigOptions[500].apply`
/// var = "conditionSacrificeMinion"
pub(crate) fn opt_500_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1567
    modList.NewMod(
        n!("Condition:SacrificeMinionOnAttack"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1569-1573 h=97752a91
/// `ConfigOptions[501].apply`
/// var = "conditionUsedWarcryRecently"
pub(crate) fn opt_501_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1570
    modList.NewMod(
        n!("Condition:UsedWarcryRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1571
    modList.NewMod(
        n!("Condition:UsedWarcryInPast8Seconds"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1572
    modList.NewMod(
        n!("Condition:UsedSkillRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1574-1576 h=02bd56ba
/// `ConfigOptions[502].apply`
/// var = "conditionUsedWarcryInPast8Seconds"
pub(crate) fn opt_502_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1575
    modList.NewMod(
        n!("Condition:UsedWarcryInPast8Seconds"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1577-1579 h=35e008b5
/// `ConfigOptions[503].apply`
/// var = "multiplierAffectedByWarcryBuffDuration"
pub(crate) fn opt_503_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1578
    modList.NewMod(
        n!("Multiplier:AffectedByWarcryBuffDuration"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1580-1582 h=9e62661a
/// `ConfigOptions[504].apply`
/// var = "DetonatedMinesRecently"
pub(crate) fn opt_504_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1581
    modList.NewMod(
        n!("Condition:DetonatedMinesRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1583-1585 h=dad65c70
/// `ConfigOptions[505].apply`
/// var = "multiplierMineDetonatedRecently"
pub(crate) fn opt_505_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1584
    modList.NewMod(
        n!("Multiplier:MineDetonatedRecently"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1586-1588 h=885b3743
/// `ConfigOptions[506].apply`
/// var = "minesPerThrow"
pub(crate) fn opt_506_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1587
    modList.NewMod(
        n!("MineThrowCount"),
        OVERRIDE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1589-1591 h=6839c382
/// `ConfigOptions[507].apply`
/// var = "TriggeredTrapsRecently"
pub(crate) fn opt_507_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1590
    modList.NewMod(
        n!("Condition:TriggeredTrapsRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1592-1594 h=fb00d64f
/// `ConfigOptions[508].apply`
/// var = "multiplierTrapTriggeredRecently"
pub(crate) fn opt_508_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1593
    modList.NewMod(
        n!("Multiplier:TrapTriggeredRecently"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1595-1597 h=c542cb39
/// `ConfigOptions[509].apply`
/// var = "conditionThrownTrapOrMineRecently"
pub(crate) fn opt_509_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1596
    modList.NewMod(
        n!("Condition:TrapOrMineThrownRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1598-1600 h=c658b084
/// `ConfigOptions[510].apply`
/// var = "trapsPerThrow"
pub(crate) fn opt_510_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1599
    modList.NewMod(
        n!("TrapThrowCount"),
        OVERRIDE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1601-1603 h=d74a990b
/// `ConfigOptions[511].apply`
/// var = "conditionCursedEnemyRecently"
pub(crate) fn opt_511_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1602
    modList.NewMod(
        n!("Condition:CursedEnemyRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1604-1606 h=c194f29f
/// `ConfigOptions[512].apply`
/// var = "conditionCastMarkRecently"
pub(crate) fn opt_512_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1605
    modList.NewMod(
        n!("Condition:CastMarkRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1607-1609 h=623d88a1
/// `ConfigOptions[513].apply`
/// var = "conditionSpawnedCorpseRecently"
pub(crate) fn opt_513_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1608
    modList.NewMod(
        n!("Condition:SpawnedCorpseRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1610-1612 h=87f13d7b
/// `ConfigOptions[514].apply`
/// var = "conditionConsumedCorpseRecently"
pub(crate) fn opt_514_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1611
    modList.NewMod(
        n!("Condition:ConsumedCorpseRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1613-1615 h=88cb200f
/// `ConfigOptions[515].apply`
/// var = "conditionConsumedCorpseInPast2Sec"
pub(crate) fn opt_515_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1614
    modList.NewMod(
        n!("Condition:ConsumedCorpseInPast2Sec"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1616-1619 h=45400330
/// `ConfigOptions[516].apply`
/// var = "multiplierCorpseConsumedRecently"
pub(crate) fn opt_516_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1617
    modList.NewMod(
        n!("Multiplier:CorpseConsumedRecently"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1618
    modList.NewMod(
        n!("Condition:ConsumedCorpseRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1620-1622 h=493a903b
/// `ConfigOptions[517].apply`
/// var = "conditionRavenousCorpseConsumed"
pub(crate) fn opt_517_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1621
    modList.NewMod(
        n!("Condition:RavenousCorpseConsumed"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1623-1628 h=a73adfe7
/// `ConfigOptions[518].apply`
/// var = "multiplierWarcryUsedRecently"
pub(crate) fn opt_518_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1624
    modList.NewMod(
        n!("Multiplier:WarcryUsedRecently"),
        BASE,
        m_min(val.num(), 100.0),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1625
    modList.NewMod(
        n!("Condition:UsedWarcryRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1626
    modList.NewMod(
        n!("Condition:UsedWarcryInPast8Seconds"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1627
    modList.NewMod(
        n!("Condition:UsedSkillRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1629-1631 h=bf5d61b2
/// `ConfigOptions[519].apply`
/// var = "conditionTauntedEnemyRecently"
pub(crate) fn opt_519_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1630
    modList.NewMod(
        n!("Condition:TauntedEnemyRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1632-1634 h=31fb8542
/// `ConfigOptions[520].apply`
/// var = "conditionLostEnduranceChargeInPast8Sec"
pub(crate) fn opt_520_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1633
    modList.NewMod(
        n!("Condition:LostEnduranceChargeInPast8Sec"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1635-1638 h=cb809e7b
/// `ConfigOptions[521].apply`
/// var = "multiplierEnduranceChargesLostRecently"
pub(crate) fn opt_521_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1636
    modList.NewMod(
        n!("Multiplier:EnduranceChargesLostRecently"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1637
    modList.NewMod(
        n!("Condition:LostEnduranceChargeInPast8Sec"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1639-1641 h=bd0a4a8a
/// `ConfigOptions[522].apply`
/// var = "conditionBlockedHitFromUniqueEnemyInPast10Sec"
pub(crate) fn opt_522_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1640
    modList.NewMod(
        n!("Condition:BlockedHitFromUniqueEnemyInPast10Sec"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1642-1644 h=6bdc14fb
/// `ConfigOptions[523].apply`
/// var = "conditionKilledUniqueEnemy"
pub(crate) fn opt_523_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1643
    modList.NewMod(
        n!("Condition:KilledUniqueEnemy"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1645-1647 h=82727c3c
/// `ConfigOptions[524].apply`
/// var = "BlockedPast10Sec"
pub(crate) fn opt_524_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1646
    modList.NewMod(
        n!("Multiplier:BlockedPast10Sec"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1648-1650 h=275c89aa
/// `ConfigOptions[525].apply`
/// var = "conditionImpaledRecently"
pub(crate) fn opt_525_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1649
    modList.NewMod(
        n!("Condition:ImpaledRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1651-1653 h=300e46e5
/// `ConfigOptions[526].apply`
/// var = "multiplierImpalesOnEnemy"
pub(crate) fn opt_526_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1652
    enemyModList.NewMod(
        n!("Multiplier:ImpaleStacks"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1654-1656 h=eabb61e7
/// `ConfigOptions[527].apply`
/// var = "conditionCausedBleedingRecently"
pub(crate) fn opt_527_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1655
    modList.NewMod(
        n!("Condition:CausedBleedingRecently"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1657-1660 h=8a23da94
/// `ConfigOptions[528].apply`
/// var = "multiplierBleedsOnEnemy"
pub(crate) fn opt_528_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1658
    enemyModList.NewMod(
        n!("Multiplier:BleedStacks"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1659
    enemyModList.NewMod(
        n!("Condition:Bleeding"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1661-1663 h=ddacc919
/// `ConfigOptions[529].apply`
/// var = "multiplierFragileRegrowth"
pub(crate) fn opt_529_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1662
    modList.NewMod(
        n!("Multiplier:FragileRegrowthCount"),
        BASE,
        m_min(val.num(), 10.0),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1664-1667 h=83829cb4
/// `ConfigOptions[530].apply`
/// var = "conditionHaveArborix"
pub(crate) fn opt_530_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1665
    modList.NewMod(
        n!("Condition:HaveIronReflexes"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1666
    modList.NewMod(n!("Keystone"), LIST, "Iron Reflexes", "Config", 0, 0, &[]);
}

/// @pob Modules/ConfigOptions.lua:1668-1676 h=855cf271
/// `ConfigOptions[531].apply`
/// var = "conditionHaveAugyre"
pub(crate) fn opt_531_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1669
    if val_eq_str(&val, "EleOverload") {
        // L1670
        modList.NewMod(
            n!("Condition:HaveElementalOverload"),
            FLAG,
            true,
            "Config",
            0,
            0,
            &[tag!({ type = Condition, var = "Combat" })],
        );
        // L1671
        modList.NewMod(
            n!("Keystone"),
            LIST,
            "Elemental Overload",
            "Config",
            0,
            0,
            &[],
        );
    } else if val_eq_str(&val, "ResTechnique") {
        // L1673
        modList.NewMod(
            n!("Condition:HaveResoluteTechnique"),
            FLAG,
            true,
            "Config",
            0,
            0,
            &[tag!({ type = Condition, var = "Combat" })],
        );
        // L1674
        modList.NewMod(
            n!("Keystone"),
            LIST,
            "Resolute Technique",
            "Config",
            0,
            0,
            &[],
        );
    }
}

/// @pob Modules/ConfigOptions.lua:1677-1680 h=783d83c7
/// `ConfigOptions[532].apply`
/// var = "conditionHaveVulconus"
pub(crate) fn opt_532_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1678
    modList.NewMod(
        n!("Condition:HaveAvatarOfFire"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1679
    modList.NewMod(n!("Keystone"), LIST, "Avatar of Fire", "Config", 0, 0, &[]);
}

/// @pob Modules/ConfigOptions.lua:1681-1683 h=e8d07818
/// `ConfigOptions[533].apply`
/// var = "conditionHaveManaStorm"
pub(crate) fn opt_533_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1682
    modList.NewMod(
        n!("Condition:SacrificeManaForLightning"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1684-1686 h=857b7ae8
/// `ConfigOptions[534].apply`
/// var = "GamblesprintMovementSpeed"
pub(crate) fn opt_534_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1685
    modList.NewMod(
        n!("MovementSpeed"),
        INC,
        val_value(&val),
        "Config",
        0,
        0,
        &[
            tag!({ type = Condition, var = "Combat" }),
            tag!({ type = Condition, var = "HaveGamblesprint" }),
        ],
    );
}

/// @pob Modules/ConfigOptions.lua:1687-1690 h=a4643c98
/// `ConfigOptions[535].apply`
/// var = "EverlastingSacrifice"
pub(crate) fn opt_535_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1688
    modList.NewMod(
        n!("ElementalResistMax"),
        BASE,
        5.0,
        "Config",
        0,
        0,
        &[
            tag!({ type = Condition, var = "Combat" }),
            tag!({ type = Condition, var = "EverlastingSacrifice" }),
        ],
    );
    // L1689
    modList.NewMod(
        n!("ChaosResistMax"),
        BASE,
        5.0,
        "Config",
        0,
        0,
        &[
            tag!({ type = Condition, var = "Combat" }),
            tag!({ type = Condition, var = "EverlastingSacrifice" }),
        ],
    );
}

/// @pob Modules/ConfigOptions.lua:1691-1693 h=7da55813
/// `ConfigOptions[536].apply`
/// var = "buffFanaticism"
pub(crate) fn opt_536_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1692
    modList.NewMod(
        n!("Condition:Fanaticism"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[
            tag!({ type = Condition, var = "Combat" }),
            tag!({ type = Condition, var = "CanGainFanaticism" }),
        ],
    );
}

/// @pob Modules/ConfigOptions.lua:1694-1696 h=b14fd975
/// `ConfigOptions[537].apply`
/// var = "conditionHitsAlwaysStun"
pub(crate) fn opt_537_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1695
    modList.NewMod(
        n!("CullPercent"),
        MAX,
        10.0,
        "Config",
        0,
        0,
        &[
            tag!({ type = Condition, var = "Combat" }),
            tag!({ type = Condition, var = "maceMasteryStunCullSpecced" }),
        ],
    );
}

/// @pob Modules/ConfigOptions.lua:1697-1699 h=f4cd1bf4
/// `ConfigOptions[538].apply`
/// var = "multiplierPvpTvalueOverride"
pub(crate) fn opt_538_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1698
    modList.NewMod(
        n!("MultiplierPvpTvalueOverride"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1700-1702 h=c78f9a23
/// `ConfigOptions[539].apply`
/// var = "multiplierPvpDamage"
pub(crate) fn opt_539_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1701
    modList.NewMod(
        n!("PvpDamageMultiplier"),
        MORE,
        (val.num() - 100.0),
        "Config",
        0,
        0,
        &[],
    );
}

/// @pob Modules/ConfigOptions.lua:1703-1705 h=64bbc8c1
/// `ConfigOptions[540].apply`
/// var = "buffAccelerationShrine"
pub(crate) fn opt_540_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1704
    modList.NewMod(
        n!("Condition:AccelerationShrine"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1706-1708 h=9f41c3a0
/// `ConfigOptions[541].apply`
/// var = "buffBrutalShrine"
pub(crate) fn opt_541_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1707
    modList.NewMod(
        n!("Condition:BrutalShrine"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1709-1711 h=dd38066f
/// `ConfigOptions[542].apply`
/// var = "buffDiamondShrine"
pub(crate) fn opt_542_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1710
    modList.NewMod(
        n!("Condition:DiamondShrine"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1712-1714 h=31fcd1ca
/// `ConfigOptions[543].apply`
/// var = "buffDivineShrine"
pub(crate) fn opt_543_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1713
    modList.NewMod(
        n!("Condition:DivineShrine"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1715-1717 h=1105fe34
/// `ConfigOptions[544].apply`
/// var = "buffEchoingShrine"
pub(crate) fn opt_544_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1716
    modList.NewMod(
        n!("Condition:EchoingShrine"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1718-1720 h=aea51cf9
/// `ConfigOptions[545].apply`
/// var = "buffGloomShrine"
pub(crate) fn opt_545_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1719
    modList.NewMod(
        n!("Condition:GloomShrine"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1721-1723 h=cab89bf5
/// `ConfigOptions[546].apply`
/// var = "buffGreaterFreezingShrine"
pub(crate) fn opt_546_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1722
    modList.NewMod(
        n!("Condition:GreaterFreezingShrine"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1724-1726 h=421e0e77
/// `ConfigOptions[547].apply`
/// var = "buffGreaterShockingShrine"
pub(crate) fn opt_547_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1725
    modList.NewMod(
        n!("Condition:GreaterShockingShrine"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1727-1729 h=99bdc984
/// `ConfigOptions[548].apply`
/// var = "buffGreaterSkeletalShrine"
pub(crate) fn opt_548_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1728
    modList.NewMod(
        n!("Condition:GreaterSkeletalShrine"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1730-1732 h=6d5e6765
/// `ConfigOptions[549].apply`
/// var = "buffImpenetrableShrine"
pub(crate) fn opt_549_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1731
    modList.NewMod(
        n!("Condition:ImpenetrableShrine"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1733-1735 h=2a7fe4ae
/// `ConfigOptions[550].apply`
/// var = "buffMassiveShrine"
pub(crate) fn opt_550_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1734
    modList.NewMod(
        n!("Condition:MassiveShrine"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1736-1738 h=f076bf9f
/// `ConfigOptions[551].apply`
/// var = "buffReplenishingShrine"
pub(crate) fn opt_551_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1737
    modList.NewMod(
        n!("Condition:ReplenishingShrine"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1739-1741 h=256be61b
/// `ConfigOptions[552].apply`
/// var = "buffResistanceShrine"
pub(crate) fn opt_552_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1740
    modList.NewMod(
        n!("Condition:ResistanceShrine"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1742-1744 h=3891d913
/// `ConfigOptions[553].apply`
/// var = "buffResonatingShrine"
pub(crate) fn opt_553_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1743
    modList.NewMod(
        n!("Condition:ResonatingShrine"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1745-1747 h=0a7e1c90
/// `ConfigOptions[554].apply`
/// var = "buffLesserAccelerationShrine"
pub(crate) fn opt_554_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1746
    modList.NewMod(
        n!("Condition:LesserAccelerationShrine"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1748-1750 h=8606cce8
/// `ConfigOptions[555].apply`
/// var = "buffLesserBrutalShrine"
pub(crate) fn opt_555_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1749
    modList.NewMod(
        n!("Condition:LesserBrutalShrine"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1751-1753 h=cf702522
/// `ConfigOptions[556].apply`
/// var = "buffLesserImpenetrableShrine"
pub(crate) fn opt_556_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1752
    modList.NewMod(
        n!("Condition:LesserImpenetrableShrine"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1754-1756 h=e95bb7cd
/// `ConfigOptions[557].apply`
/// var = "buffLesserMassiveShrine"
pub(crate) fn opt_557_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1755
    modList.NewMod(
        n!("Condition:LesserMassiveShrine"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1757-1759 h=b4515349
/// `ConfigOptions[558].apply`
/// var = "buffLesserReplenishingShrine"
pub(crate) fn opt_558_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1758
    modList.NewMod(
        n!("Condition:LesserReplenishingShrine"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1760-1762 h=b99dde3d
/// `ConfigOptions[559].apply`
/// var = "buffLesserResistanceShrine"
pub(crate) fn opt_559_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1761
    modList.NewMod(
        n!("Condition:LesserResistanceShrine"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1763-1765 h=0e9a5bfa
/// `ConfigOptions[560].apply`
/// var = "buffBloodShrineOfRats"
pub(crate) fn opt_560_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1764
    modList.NewMod(
        n!("Condition:BloodShrineOfRats"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1766-1768 h=6a54f760
/// `ConfigOptions[561].apply`
/// var = "buffBloodShrineOfLocusts"
pub(crate) fn opt_561_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1767
    modList.NewMod(
        n!("Condition:BloodShrineOfLocusts"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1769-1771 h=85285b99
/// `ConfigOptions[562].apply`
/// var = "buffBloodShrineOfToads"
pub(crate) fn opt_562_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1770
    modList.NewMod(
        n!("Condition:BloodShrineOfToads"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1772-1774 h=8991a272
/// `ConfigOptions[563].apply`
/// var = "buffBloodShrineOfCrows"
pub(crate) fn opt_563_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1773
    modList.NewMod(
        n!("Condition:BloodShrineOfCrows"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1775-1777 h=476a751a
/// `ConfigOptions[564].apply`
/// var = "buffBloodShrineOfBats"
pub(crate) fn opt_564_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1776
    modList.NewMod(
        n!("Condition:BloodShrineOfBats"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
}

// @pob-skip Modules/ConfigOptions.lua:1779-1779 reason="option definition data (dumped: data().configOptions / ConfigOptionDef)"

/// @pob Modules/ConfigOptions.lua:1780-1782 h=e374ba7a
/// `ConfigOptions[566].apply`
/// var = "skillForkCount"
pub(crate) fn opt_566_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1781
    modList.NewMod(
        n!("ForkedCount"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1783-1785 h=1e33c385
/// `ConfigOptions[567].apply`
/// var = "skillChainCount"
pub(crate) fn opt_567_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1784
    modList.NewMod(
        n!("ChainCount"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1786-1788 h=f3f10fff
/// `ConfigOptions[568].apply`
/// var = "skillPierceCount"
pub(crate) fn opt_568_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1787
    modList.NewMod(
        n!("PiercedCount"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

// @pob-skip Modules/ConfigOptions.lua:1789-1790 reason="option definition data (dumped: data().configOptions / ConfigOptionDef)"

/// @pob Modules/ConfigOptions.lua:1791-1793 h=59fb9697
/// `ConfigOptions[571].apply`
/// var = "conditionAtCloseRange"
pub(crate) fn opt_571_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1792
    modList.NewMod(
        n!("Condition:AtCloseRange"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1794-1796 h=1599bbfd
/// `ConfigOptions[572].apply`
/// var = "enemyMultiplierEnemyPresenceSeconds"
pub(crate) fn opt_572_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1795
    enemyModList.NewMod(
        n!("Multiplier:EnemyPresenceSeconds"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1797-1799 h=77200449
/// `ConfigOptions[573].apply`
/// var = "conditionEnemyMoving"
pub(crate) fn opt_573_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1798
    enemyModList.NewMod(
        n!("Condition:Moving"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1800-1802 h=ab8bf54e
/// `ConfigOptions[574].apply`
/// var = "conditionEnemyFullLife"
pub(crate) fn opt_574_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1801
    enemyModList.NewMod(
        n!("Condition:FullLife"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1803-1805 h=8450ead4
/// `ConfigOptions[575].apply`
/// var = "conditionEnemyLowLife"
pub(crate) fn opt_575_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1804
    enemyModList.NewMod(
        n!("Condition:LowLife"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1806-1808 h=cf234dae
/// `ConfigOptions[576].apply`
/// var = "conditionEnemyCursed"
pub(crate) fn opt_576_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1807
    enemyModList.NewMod(
        n!("Condition:Cursed"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1809-1811 h=b8a30b09
/// `ConfigOptions[577].apply`
/// var = "conditionEnemyStunned"
pub(crate) fn opt_577_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1810
    enemyModList.NewMod(
        n!("Condition:Stunned"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1812-1814 h=959b56be
/// `ConfigOptions[578].apply`
/// var = "conditionEnemyBleeding"
pub(crate) fn opt_578_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1813
    enemyModList.NewMod(
        n!("Condition:Bleeding"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1815-1817 h=48e752ea
/// `ConfigOptions[579].apply`
/// var = "overrideBleedStackPotential"
pub(crate) fn opt_579_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1816
    modList.NewMod(
        n!("BleedStackPotentialOverride"),
        OVERRIDE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1818-1820 h=0416348a
/// `ConfigOptions[580].apply`
/// var = "conditionSingleBleed"
pub(crate) fn opt_580_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1819
    modList.NewMod(
        n!("Condition:SingleBleed"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1821-1823 h=fd83452e
/// `ConfigOptions[581].apply`
/// var = "multiplierRuptureStacks"
pub(crate) fn opt_581_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1822
    enemyModList.NewMod(
        n!("Multiplier:RuptureStack"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1824-1826 h=cb27749b
/// `ConfigOptions[582].apply`
/// var = "conditionEnemyPoisoned"
pub(crate) fn opt_582_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1825
    enemyModList.NewMod(
        n!("Condition:Poisoned"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1827-1829 h=1be406e2
/// `ConfigOptions[583].apply`
/// var = "multiplierPoisonOnEnemy"
pub(crate) fn opt_583_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1828
    enemyModList.NewMod(
        n!("Multiplier:PoisonStack"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1830-1832 h=8aa134d6
/// `ConfigOptions[584].apply`
/// var = "conditionNonPoisonedOnly"
pub(crate) fn opt_584_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1831
    modList.NewMod(
        n!("Condition:NonPoisonedOnly"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1833-1835 h=91c70e3c
/// `ConfigOptions[585].apply`
/// var = "multiplierCurseExpiredOnEnemy"
pub(crate) fn opt_585_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1834
    enemyModList.NewMod(
        n!("Multiplier:CurseExpired"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1836-1838 h=e7ddf397
/// `ConfigOptions[586].apply`
/// var = "multiplierCurseDurationExpiredOnEnemy"
pub(crate) fn opt_586_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1837
    enemyModList.NewMod(
        n!("Multiplier:CurseDurationExpired"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1839-1841 h=5da1440d
/// `ConfigOptions[587].apply`
/// var = "multiplierWitheredStackCount"
pub(crate) fn opt_587_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1840
    enemyModList.NewMod(
        n!("Multiplier:WitheredStack"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1842-1846 h=38c2b1a6
/// `ConfigOptions[588].apply`
/// var = "multiplierCorrosionStackCount"
pub(crate) fn opt_588_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1843
    enemyModList.NewMod(
        n!("Multiplier:CorrosionStack"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
    // L1844
    enemyModList.NewMod(
        n!("Armour"),
        BASE,
        -5000.0,
        "Corrosion",
        0,
        0,
        &[
            tag!({ type = Multiplier, var = "CorrosionStack" }),
            tag!({ type = ActorCondition, actor = "enemy", var = "CanCorrode" }),
        ],
    );
    // L1845
    enemyModList.NewMod(
        n!("Evasion"),
        BASE,
        -1000.0,
        "Corrosion",
        0,
        0,
        &[
            tag!({ type = Multiplier, var = "CorrosionStack" }),
            tag!({ type = ActorCondition, actor = "enemy", var = "CanCorrode" }),
        ],
    );
}

/// @pob Modules/ConfigOptions.lua:1847-1850 h=c5e9a8f5
/// `ConfigOptions[589].apply`
/// var = "multiplierEnsnaredStackCount"
pub(crate) fn opt_589_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1848
    modList.NewMod(
        n!("Multiplier:EnsnareStackCount"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
    // L1849
    enemyModList.NewMod(n!("Condition:Moving"), FLAG, true, "Config", 0, 0, &[tag!({ type = MultiplierThreshold, actor = "enemy", var = "EnsnareStackCount", threshold = 1.0 })]);
}

/// @pob Modules/ConfigOptions.lua:1851-1853 h=a1d4da43
/// `ConfigOptions[590].apply`
/// var = "conditionEnemyMaimed"
pub(crate) fn opt_590_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1852
    enemyModList.NewMod(
        n!("Condition:Maimed"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1854-1856 h=0dd3a890
/// `ConfigOptions[591].apply`
/// var = "conditionEnemyHindered"
pub(crate) fn opt_591_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1855
    enemyModList.NewMod(
        n!("Condition:Hindered"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1857-1859 h=899c7dea
/// `ConfigOptions[592].apply`
/// var = "conditionEnemyExcommunicated"
pub(crate) fn opt_592_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1858
    enemyModList.NewMod(
        n!("Condition:Excommunicated"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = ActorCondition, actor = "enemy", var = "CanExcommunicate" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1860-1862 h=9ca1f552
/// `ConfigOptions[593].apply`
/// var = "conditionEnemyBlinded"
pub(crate) fn opt_593_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1861
    enemyModList.NewMod(
        n!("Condition:Blinded"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1863-1865 h=f6bcdafd
/// `ConfigOptions[594].apply`
/// var = "overrideBuffBlinded"
pub(crate) fn opt_594_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1864
    enemyModList.NewMod(
        n!("BlindEffect"),
        OVERRIDE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = GlobalEffect, effectType = "Buff" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1866-1868 h=06c74f57
/// `ConfigOptions[595].apply`
/// var = "conditionEnemyTaunted"
pub(crate) fn opt_595_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1867
    enemyModList.NewMod(
        n!("Condition:Taunted"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1869-1871 h=0df3e19f
/// `ConfigOptions[596].apply`
/// var = "conditionEnemyDebilitated"
pub(crate) fn opt_596_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1870
    enemyModList.NewMod(
        n!("Condition:Debilitated"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1872-1874 h=a5f83dff
/// `ConfigOptions[597].apply`
/// var = "conditionEnemyPacified"
pub(crate) fn opt_597_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1873
    enemyModList.NewMod(
        n!("Condition:Pacified"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1875-1877 h=89fbb5ab
/// `ConfigOptions[598].apply`
/// var = "conditionEnemyBurning"
pub(crate) fn opt_598_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1876
    enemyModList.NewMod(
        n!("Condition:Burning"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1878-1880 h=42700a49
/// `ConfigOptions[599].apply`
/// var = "conditionEnemyIgnited"
pub(crate) fn opt_599_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1879
    enemyModList.NewMod(
        n!("Condition:Ignited"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1881-1883 h=d11b172d
/// `ConfigOptions[600].apply`
/// var = "overrideIgniteStackPotential"
pub(crate) fn opt_600_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1882
    modList.NewMod(
        n!("IgniteStackPotentialOverride"),
        OVERRIDE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1884-1887 h=ffdc48da
/// `ConfigOptions[601].apply`
/// var = "conditionEnemyScorched"
pub(crate) fn opt_601_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1885
    enemyModList.NewMod(
        n!("Condition:Scorched"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
    // L1886
    enemyModList.NewMod(
        n!("Condition:ScorchedConfig"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1888-1891 h=4fc6e7f2
/// `ConfigOptions[602].apply`
/// var = "conditionScorchedEffect"
pub(crate) fn opt_602_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1889
    enemyModList.NewMod(
        n!("ScorchVal"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "ScorchedConfig" })],
    );
    // L1890
    enemyModList.NewMod(
        n!("DesiredScorchVal"),
        BASE,
        val_value(&val),
        "Scorch",
        0,
        0,
        &[tag!({ type = Condition, var = "ScorchedConfig", neg = true })],
    );
}

/// @pob Modules/ConfigOptions.lua:1892-1894 h=7518aa62
/// `ConfigOptions[603].apply`
/// var = "ScorchStacks"
pub(crate) fn opt_603_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1893
    enemyModList.NewMod(
        n!("Multiplier:ScorchStacks"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1895-1898 h=091bf2f7
/// `ConfigOptions[604].apply`
/// var = "conditionEnemyOnScorchedGround"
pub(crate) fn opt_604_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1896
    enemyModList.NewMod(
        n!("Condition:Scorched"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
    // L1897
    enemyModList.NewMod(
        n!("Condition:OnScorchedGround"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1899-1902 h=31bc2266
/// `ConfigOptions[605].apply`
/// var = "conditionEnemyChilled"
pub(crate) fn opt_605_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1900
    enemyModList.NewMod(
        n!("Condition:Chilled"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
    // L1901
    enemyModList.NewMod(
        n!("Condition:ChilledConfig"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1903-1906 h=681e9e03
/// `ConfigOptions[606].apply`
/// var = "multiplierChilledByYouSeconds"
pub(crate) fn opt_606_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1904
    enemyModList.NewMod(
        n!("Multiplier:ChilledByYouSeconds"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1905
    enemyModList.NewMod(
        n!("Condition:ChilledByYou"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1907-1910 h=f9f1f446
/// `ConfigOptions[607].apply`
/// var = "conditionEnemyChilledEffect"
pub(crate) fn opt_607_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1908
    enemyModList.NewMod(
        n!("ChillVal"),
        BASE,
        val_value(&val),
        "Chill",
        0,
        0,
        &[tag!({ type = Condition, var = "ChilledConfig" })],
    );
    // L1909
    enemyModList.NewMod(
        n!("DesiredChillVal"),
        BASE,
        val_value(&val),
        "Chill",
        0,
        0,
        &[tag!({ type = Condition, var = "ChilledConfig", neg = true })],
    );
}

/// @pob Modules/ConfigOptions.lua:1911-1914 h=10bcff3e
/// `ConfigOptions[608].apply`
/// var = "conditionEnemyChilledByYourHits"
pub(crate) fn opt_608_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1912
    enemyModList.NewMod(
        n!("Condition:Chilled"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
    // L1913
    enemyModList.NewMod(
        n!("Condition:ChilledByYourHits"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1915-1917 h=b2ba1952
/// `ConfigOptions[609].apply`
/// var = "HoarfrostStacks"
pub(crate) fn opt_609_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1916
    enemyModList.NewMod(
        n!("HoarfrostFreezeDuration"),
        INC,
        (val.num() * 20.0),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1918-1923 h=9211665f
/// `ConfigOptions[610].apply`
/// var = "multiplierBarnacleStacks"
pub(crate) fn opt_610_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1919
    enemyModList.NewMod(
        n!("Multiplier:BarnacleStack"),
        BASE,
        m_min(val.num(), 10.0),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
    // L1920
    enemyModList.NewMod(
        n!("SelfFreezeDuration"),
        INC,
        5.0,
        "Config",
        0,
        0,
        &[
            tag!({ type = Multiplier, var = "BarnacleStack" }),
            tag!({ type = Condition, var = "Effective" }),
        ],
    );
    // L1921
    enemyModList.NewMod(
        n!("SelfChillDuration"),
        INC,
        5.0,
        "Config",
        0,
        0,
        &[
            tag!({ type = Multiplier, var = "BarnacleStack" }),
            tag!({ type = Condition, var = "Effective" }),
        ],
    );
    // L1922
    enemyModList.NewMod(
        n!("PhysicalDamageConvertToCold"),
        BASE,
        5.0,
        "Config",
        0,
        0,
        &[
            tag!({ type = Multiplier, var = "BarnacleStack" }),
            tag!({ type = Condition, var = "Effective" }),
        ],
    );
}

/// @pob Modules/ConfigOptions.lua:1924-1926 h=505dc7a3
/// `ConfigOptions[611].apply`
/// var = "conditionEnemyFrozen"
pub(crate) fn opt_611_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1925
    enemyModList.NewMod(
        n!("Condition:Frozen"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1927-1930 h=61e23927
/// `ConfigOptions[612].apply`
/// var = "multiplierFrozenByYouSeconds"
pub(crate) fn opt_612_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1928
    enemyModList.NewMod(
        n!("Multiplier:FrozenByYouSeconds"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Combat" })],
    );
    // L1929
    enemyModList.NewMod(
        n!("Condition:FrozenByYou"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1931-1934 h=32ea17df
/// `ConfigOptions[613].apply`
/// var = "conditionEnemyBrittle"
pub(crate) fn opt_613_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1932
    enemyModList.NewMod(
        n!("Condition:Brittle"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
    // L1933
    enemyModList.NewMod(
        n!("Condition:BrittleConfig"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1935-1938 h=ad345f81
/// `ConfigOptions[614].apply`
/// var = "conditionBrittleEffect"
pub(crate) fn opt_614_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1936
    enemyModList.NewMod(
        n!("BrittleVal"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "BrittleConfig" })],
    );
    // L1937
    enemyModList.NewMod(
        n!("DesiredBrittleVal"),
        BASE,
        val_value(&val),
        "Brittle",
        0,
        0,
        &[tag!({ type = Condition, var = "BrittleConfig", neg = true })],
    );
}

/// @pob Modules/ConfigOptions.lua:1939-1942 h=58deda51
/// `ConfigOptions[615].apply`
/// var = "conditionEnemyOnBrittleGround"
pub(crate) fn opt_615_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1940
    enemyModList.NewMod(
        n!("Condition:Brittle"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
    // L1941
    enemyModList.NewMod(
        n!("Condition:OnBrittleGround"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1943-1946 h=5d4c92dc
/// `ConfigOptions[616].apply`
/// var = "conditionEnemyShocked"
pub(crate) fn opt_616_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1944
    enemyModList.NewMod(
        n!("Condition:Shocked"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
    // L1945
    enemyModList.NewMod(
        n!("Condition:ShockedConfig"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1947-1950 h=cdf8fa01
/// `ConfigOptions[617].apply`
/// var = "conditionShockEffect"
pub(crate) fn opt_617_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1948
    enemyModList.NewMod(
        n!("ShockVal"),
        BASE,
        val_value(&val),
        "Shock",
        0,
        0,
        &[tag!({ type = Condition, var = "ShockedConfig" })],
    );
    // L1949
    enemyModList.NewMod(
        n!("DesiredShockVal"),
        BASE,
        val_value(&val),
        "Shock",
        0,
        0,
        &[tag!({ type = Condition, var = "ShockedConfig", neg = true })],
    );
}

/// @pob Modules/ConfigOptions.lua:1951-1953 h=09fd9ef8
/// `ConfigOptions[618].apply`
/// var = "ShockStacks"
pub(crate) fn opt_618_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1952
    enemyModList.NewMod(
        n!("Multiplier:ShockStacks"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1954-1957 h=75f04f1d
/// `ConfigOptions[619].apply`
/// var = "conditionEnemyOnShockedGround"
pub(crate) fn opt_619_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1955
    enemyModList.NewMod(
        n!("Condition:Shocked"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
    // L1956
    enemyModList.NewMod(
        n!("Condition:OnShockedGround"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1958-1961 h=ccfd0173
/// `ConfigOptions[620].apply`
/// var = "conditionEnemySapped"
pub(crate) fn opt_620_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1959
    enemyModList.NewMod(
        n!("Condition:Sapped"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
    // L1960
    enemyModList.NewMod(
        n!("Condition:SappedConfig"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1962-1965 h=baeed33e
/// `ConfigOptions[621].apply`
/// var = "conditionSapEffect"
pub(crate) fn opt_621_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1963
    enemyModList.NewMod(
        n!("SapVal"),
        BASE,
        val_value(&val),
        "Sap",
        0,
        0,
        &[tag!({ type = Condition, var = "SappedConfig" })],
    );
    // L1964
    enemyModList.NewMod(
        n!("DesiredSapVal"),
        BASE,
        val_value(&val),
        "Sap",
        0,
        0,
        &[tag!({ type = Condition, var = "SappedConfig", neg = true })],
    );
}

/// @pob Modules/ConfigOptions.lua:1966-1969 h=4f22ffaa
/// `ConfigOptions[622].apply`
/// var = "conditionEnemyOnSappedGround"
pub(crate) fn opt_622_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1967
    enemyModList.NewMod(
        n!("Condition:Sapped"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
    // L1968
    enemyModList.NewMod(
        n!("Condition:OnSappedGround"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1970-1972 h=6c751b57
/// `ConfigOptions[623].apply`
/// var = "multiplierFreezeShockIgniteOnEnemy"
pub(crate) fn opt_623_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1971
    modList.NewMod(
        n!("Multiplier:FreezeShockIgniteOnEnemy"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1973-1975 h=aa0dc4b1
/// `ConfigOptions[624].apply`
/// var = "conditionEnemyFireExposure"
pub(crate) fn opt_624_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1974
    enemyModList.NewMod(
        n!("FireExposure"),
        BASE,
        -10.0,
        "Config",
        0,
        0,
        &[
            tag!({ type = Condition, var = "Effective" }),
            tag!({ type = ActorCondition, actor = "enemy", var = "CanApplyFireExposure" }),
        ],
    );
}

/// @pob Modules/ConfigOptions.lua:1976-1978 h=28edeb51
/// `ConfigOptions[625].apply`
/// var = "conditionEnemyColdExposure"
pub(crate) fn opt_625_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1977
    enemyModList.NewMod(
        n!("ColdExposure"),
        BASE,
        -10.0,
        "Config",
        0,
        0,
        &[
            tag!({ type = Condition, var = "Effective" }),
            tag!({ type = ActorCondition, actor = "enemy", var = "CanApplyColdExposure" }),
        ],
    );
}

/// @pob Modules/ConfigOptions.lua:1979-1981 h=09cbccbe
/// `ConfigOptions[626].apply`
/// var = "conditionEnemyLightningExposure"
pub(crate) fn opt_626_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1980
    enemyModList.NewMod(
        n!("LightningExposure"),
        BASE,
        -10.0,
        "Config",
        0,
        0,
        &[
            tag!({ type = Condition, var = "Effective" }),
            tag!({ type = ActorCondition, actor = "enemy", var = "CanApplyLightningExposure" }),
        ],
    );
}

/// @pob Modules/ConfigOptions.lua:1982-1984 h=ab062bf5
/// `ConfigOptions[627].apply`
/// var = "conditionEnemyIntimidated"
pub(crate) fn opt_627_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1983
    enemyModList.NewMod(
        n!("Condition:Intimidated"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1985-1987 h=f93c8247
/// `ConfigOptions[628].apply`
/// var = "conditionEnemyCrushed"
pub(crate) fn opt_628_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1986
    enemyModList.NewMod(
        n!("Condition:Crushed"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1988-1990 h=df080d0d
/// `ConfigOptions[629].apply`
/// var = "conditionEnemyHallowingFlame"
pub(crate) fn opt_629_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1989
    enemyModList.NewMod(
        n!("Condition:HallowingFlame"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1991-1993 h=189ce896
/// `ConfigOptions[630].apply`
/// var = "multiplierEnemyHallowingFlame"
pub(crate) fn opt_630_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1992
    enemyModList.NewMod(
        n!("Multiplier:HallowingFlame"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:1994-1996 h=0d619e26
/// `ConfigOptions[631].apply`
/// var = "multiplierHallowingFlameStacksRemovedByAlly"
pub(crate) fn opt_631_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1995
    modList.NewMod(
        n!("Multiplier:HallowingFlameStacksRemovedByAlly"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[],
    );
}

/// @pob Modules/ConfigOptions.lua:1997-1999 h=79ca715d
/// `ConfigOptions[632].apply`
/// var = "conditionHallowingFlameMagnitude"
pub(crate) fn opt_632_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L1998
    modList.NewMod(
        n!("HallowingFlameMagnitude"),
        OVERRIDE,
        val_value(&val),
        "Config",
        0,
        0,
        &[],
    );
}

/// @pob Modules/ConfigOptions.lua:2000-2002 h=18cfae32
/// `ConfigOptions[633].apply`
/// var = "conditionNearLinkedTarget"
pub(crate) fn opt_633_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2001
    enemyModList.NewMod(
        n!("Condition:NearLinkedTarget"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:2003-2005 h=aaa890b1
/// `ConfigOptions[634].apply`
/// var = "conditionEnemyUnnerved"
pub(crate) fn opt_634_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2004
    enemyModList.NewMod(
        n!("Condition:Unnerved"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:2006-2008 h=45958877
/// `ConfigOptions[635].apply`
/// var = "conditionEnemyCoveredInAsh"
pub(crate) fn opt_635_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2007
    modList.NewMod(
        n!("CoveredInAshEffect"),
        BASE,
        20.0,
        "Covered in Ash",
        0,
        0,
        &[],
    );
}

/// @pob Modules/ConfigOptions.lua:2009-2011 h=0ef8849f
/// `ConfigOptions[636].apply`
/// var = "conditionEnemyCoveredInFrost"
pub(crate) fn opt_636_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2010
    modList.NewMod(
        n!("CoveredInFrostEffect"),
        BASE,
        20.0,
        "Covered in Frost",
        0,
        0,
        &[],
    );
}

/// @pob Modules/ConfigOptions.lua:2012-2014 h=9294ab7e
/// `ConfigOptions[637].apply`
/// var = "conditionEnemyOnConsecratedGround"
pub(crate) fn opt_637_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2013
    enemyModList.NewMod(
        n!("Condition:OnConsecratedGround"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:2015-2017 h=95f9493c
/// `ConfigOptions[638].apply`
/// var = "conditionEnemyHaveEnergyShield"
pub(crate) fn opt_638_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2016
    enemyModList.NewMod(
        n!("Condition:HaveEnergyShield"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:2018-2022 h=3587573b
/// `ConfigOptions[639].apply`
/// var = "conditionEnemyOnProfaneGround"
pub(crate) fn opt_639_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2019
    enemyModList.NewMod(
        n!("Condition:OnProfaneGround"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
    // L2020
    enemyModList.NewMod(
        n!("CurseEffectOnSelf"),
        INC,
        10.0,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "OnProfaneGround" })],
    );
    // L2021
    enemyModList.NewMod(
        n!("SelfCritChance"),
        INC,
        100.0,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "OnProfaneGround" })],
    );
}

/// @pob Modules/ConfigOptions.lua:2023-2025 h=518c2729
/// `ConfigOptions[640].apply`
/// var = "multiplierEnemyAffectedByGraspingVines"
pub(crate) fn opt_640_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2024
    modList.NewMod(
        n!("Multiplier:GraspingVinesAffectingEnemy"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:2026-2028 h=847e9d28
/// `ConfigOptions[641].apply`
/// var = "conditionEnemyOnFungalGround"
pub(crate) fn opt_641_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2027
    enemyModList.NewMod(
        n!("Condition:OnFungalGround"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:2029-2038 h=535bfe88
/// `ConfigOptions[642].apply`
/// var = "conditionEnemyOnBrineGround"
pub(crate) fn opt_642_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2030
    enemyModList.NewMod(
        n!("Condition:OnBrineGround"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
    // L2031
    enemyModList.NewMod(
        n!("SelfChillEffect"),
        INC,
        30.0,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "OnBrineGround" })],
    );
    // L2032
    enemyModList.NewMod(
        n!("SelfBrittleEffect"),
        INC,
        30.0,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "OnBrineGround" })],
    );
    // L2033
    enemyModList.NewMod(
        n!("SelfShockEffect"),
        INC,
        30.0,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "OnBrineGround" })],
    );
    // L2034
    enemyModList.NewMod(
        n!("SelfSapEffect"),
        INC,
        30.0,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "OnBrineGround" })],
    );
    // L2035
    enemyModList.NewMod(
        n!("Armour"),
        INC,
        -25.0,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "OnBrineGround" })],
    );
    // L2036
    enemyModList.NewMod(
        n!("Evasion"),
        INC,
        -25.0,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "OnBrineGround" })],
    );
}

/// @pob Modules/ConfigOptions.lua:2039-2041 h=d96fcccf
/// `ConfigOptions[643].apply`
/// var = "conditionEnemyInChillingArea"
pub(crate) fn opt_643_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2040
    enemyModList.NewMod(
        n!("Condition:InChillingArea"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:2042-2044 h=16fd6cb8
/// `ConfigOptions[644].apply`
/// var = "conditionEnemyInFrostGlobe"
pub(crate) fn opt_644_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2043
    enemyModList.NewMod(
        n!("Condition:EnemyInFrostGlobe"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:2045-2047 h=4d9dbc46
/// `ConfigOptions[645].apply`
/// var = "conditionEnemyLifeHigherThanPlayer"
pub(crate) fn opt_645_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2046
    enemyModList.NewMod(
        n!("Condition:HigherLifePercentThanPlayer"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:2048-2050 h=4b52db40
/// `ConfigOptions[646].apply`
/// var = "enemyConditionHitByFireDamage"
pub(crate) fn opt_646_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2049
    enemyModList.NewMod(
        n!("Condition:HitByFireDamage"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[],
    );
}

/// @pob Modules/ConfigOptions.lua:2051-2053 h=0756e002
/// `ConfigOptions[647].apply`
/// var = "enemyConditionHitByColdDamage"
pub(crate) fn opt_647_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2052
    enemyModList.NewMod(
        n!("Condition:HitByColdDamage"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[],
    );
}

/// @pob Modules/ConfigOptions.lua:2054-2056 h=08a243f3
/// `ConfigOptions[648].apply`
/// var = "enemyConditionHitByLightningDamage"
pub(crate) fn opt_648_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2055
    enemyModList.NewMod(
        n!("Condition:HitByLightningDamage"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[],
    );
}

/// @pob Modules/ConfigOptions.lua:2057-2059 h=c03f5174
/// `ConfigOptions[649].apply`
/// var = "enemyInRFOrScorchingRay"
pub(crate) fn opt_649_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2058
    modList.NewMod(
        n!("Condition:InRFOrScorchingRay"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[],
    );
}

// @pob-skip Modules/ConfigOptions.lua:2060-2060 reason="option definition data (dumped: data().configOptions / ConfigOptionDef)"

/// @pob Modules/ConfigOptions.lua:2061-2063 h=cc787606
/// `ConfigOptions[651].apply`
/// var = "conditionBetweenYouAndLinkedTarget"
pub(crate) fn opt_651_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2062
    enemyModList.NewMod(
        n!("Condition:BetweenYouAndLinkedTarget"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:2064-2066 h=df55e3d2
/// `ConfigOptions[652].apply`
/// var = "conditionEnemyFireResZero"
pub(crate) fn opt_652_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2065
    enemyModList.NewMod(
        n!("FireResist"),
        OVERRIDE,
        0.0,
        "Config",
        0,
        0,
        &[
            tag!({ type = Condition, var = "Effective" }),
            tag!({ type = ActorCondition, actor = "enemy", var = "HaveTrickstersSmile" }),
        ],
    );
}

/// @pob Modules/ConfigOptions.lua:2067-2069 h=54857775
/// `ConfigOptions[653].apply`
/// var = "conditionEnemyColdResZero"
pub(crate) fn opt_653_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2068
    enemyModList.NewMod(
        n!("ColdResist"),
        OVERRIDE,
        0.0,
        "Config",
        0,
        0,
        &[
            tag!({ type = Condition, var = "Effective" }),
            tag!({ type = ActorCondition, actor = "enemy", var = "HaveTrickstersSmile" }),
        ],
    );
}

/// @pob Modules/ConfigOptions.lua:2070-2072 h=0fbb7035
/// `ConfigOptions[654].apply`
/// var = "conditionEnemyLightningResZero"
pub(crate) fn opt_654_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2071
    enemyModList.NewMod(
        n!("LightningResist"),
        OVERRIDE,
        0.0,
        "Config",
        0,
        0,
        &[
            tag!({ type = Condition, var = "Effective" }),
            tag!({ type = ActorCondition, actor = "enemy", var = "HaveTrickstersSmile" }),
        ],
    );
}

/// @pob Modules/ConfigOptions.lua:2073-2079 h=0e7f1560
/// `ConfigOptions[655].apply`
/// var = "maniaDebuffsCount"
pub(crate) fn opt_655_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    let mut val: Val = val.clone(); // Lua reassigns the parameter
                                    // L2074
    val = Val::from(m_min(val.num(), 15.0));
    // L2075
    enemyModList.NewMod(
        n!("DamageTaken"),
        INC,
        (val.num() * 4.0),
        &*cat!(val_str(&val), " Mania Stacks"),
        0,
        0,
        &[
            tag!({ type = GlobalEffect, effectType = "Debuff" }),
            tag!({ type = Condition, var = "AfflictedByMania" }),
        ],
    );
    // L2076
    enemyModList.NewMod(
        n!("ActionSpeed"),
        INC,
        ((-val.num()) * 2.0),
        &*cat!(val_str(&val), " Mania Stacks"),
        0,
        0,
        &[
            tag!({ type = GlobalEffect, effectType = "Debuff" }),
            tag!({ type = Condition, var = "AfflictedByMania" }),
        ],
    );
    // L2077
    enemyModList.NewMod(
        n!("LifeRecoveryRate"),
        INC,
        ((-val.num()) * 10.0),
        &*cat!(val_str(&val), " Mania Stacks"),
        0,
        0,
        &[
            tag!({ type = GlobalEffect, effectType = "Debuff" }),
            tag!({ type = Condition, var = "AfflictedByMania" }),
        ],
    );
    // L2078
    enemyModList.NewMod(
        n!("EnergyShieldRecoveryRate"),
        INC,
        ((-val.num()) * 10.0),
        &*cat!(val_str(&val), " Mania Stacks"),
        0,
        0,
        &[
            tag!({ type = GlobalEffect, effectType = "Debuff" }),
            tag!({ type = Condition, var = "AfflictedByMania" }),
        ],
    );
}

// @pob-skip Modules/ConfigOptions.lua:2081-2082 reason="option definition data (dumped: data().configOptions / ConfigOptionDef)"

/// @pob Modules/ConfigOptions.lua:2083-2085 h=57eb2bcf
/// `ConfigOptions[658].apply`
/// var = "conditionEnemyRareOrUnique"
pub(crate) fn opt_658_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2084
    enemyModList.NewMod(
        n!("Condition:RareOrUnique"),
        FLAG,
        true,
        "Config",
        0,
        0,
        &[tag!({ type = Condition, var = "Effective" })],
    );
}

/// @pob Modules/ConfigOptions.lua:2086-2222 h=6f49a766
/// `ConfigOptions[659].apply`
/// var = "enemyIsBoss"
pub(crate) fn opt_659_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // Hand-ported (T1 does not translate method calls on UI controls). `SetPlaceholder` and
    // `UpdateLevel` act on the control-state model (helpers at the top of this file, CCR-0037).
    let data = data();
    // These defaults are here so that the placeholders get reset correctly
    // L2088
    SetPlaceholder(build, n!("enemySpeed"), Val::from(700.0), true);
    // L2089
    SetPlaceholder(build, n!("enemyCritChance"), Val::from(5.0), true);
    // L2090
    SetPlaceholder(
        build,
        n!("enemyCritDamage"),
        Val::from(data.monsterConstants.base_critical_strike_multiplier - 100.0),
        true,
    );
    // L2091
    if val_eq_str(val, "None") {
        // L2092
        let mut defaultResist = Val::from("");
        // L2093
        SetPlaceholder(
            build,
            n!("enemyLightningResist"),
            defaultResist.clone(),
            true,
        );
        // L2094
        SetPlaceholder(build, n!("enemyColdResist"), defaultResist.clone(), true);
        // L2095
        SetPlaceholder(build, n!("enemyFireResist"), defaultResist.clone(), true);
        // L2096
        SetPlaceholder(build, n!("enemyChaosResist"), defaultResist.clone(), true);

        // L2098
        let mut defaultLevel = 83.0;
        // L2099
        SetPlaceholder(build, n!("enemyLevel"), Val::from(""), true);
        // L2100
        build.configTab.UpdateLevel(build);
        // L2101
        if let Some(enemyLevel) = build.configTab.enemyLevel.get() {
            // L2102
            defaultLevel = enemyLevel;
        }

        // L2105
        let mut defaultDamage = round(
            level_at(&data.monsterDamageTable, defaultLevel).num() * 1.5,
            None,
        );
        // L2106
        SetPlaceholder(
            build,
            n!("enemyPhysicalDamage"),
            Val::from(defaultDamage),
            true,
        );
        // L2107
        SetPlaceholder(build, n!("enemyLightningDamage"), Val::from(""), true);
        // L2108
        SetPlaceholder(build, n!("enemyColdDamage"), Val::from(""), true);
        // L2109
        SetPlaceholder(build, n!("enemyFireDamage"), Val::from(""), true);
        // L2110
        SetPlaceholder(build, n!("enemyChaosDamage"), Val::from(""), true);

        // L2112
        let mut defaultPen = Val::from("");
        // L2113
        SetPlaceholder(
            build,
            n!("enemyPhysicalOverwhelm"),
            defaultPen.clone(),
            true,
        );
        // L2114
        SetPlaceholder(build, n!("enemyLightningPen"), defaultPen.clone(), true);
        // L2115
        SetPlaceholder(build, n!("enemyColdPen"), defaultPen.clone(), true);
        // L2116
        SetPlaceholder(build, n!("enemyFirePen"), defaultPen.clone(), true);

        // L2118
        SetPlaceholder(
            build,
            n!("enemyArmour"),
            level_at(&data.monsterArmourTable, defaultLevel),
            true,
        );
        // L2119
        SetPlaceholder(
            build,
            n!("enemyEvasion"),
            level_at(&data.monsterEvasionTable, defaultLevel),
            true,
        );
    } else if val_eq_str(val, "Boss") {
        // L2121
        enemyModList.NewMod(
            n!("Condition:RareOrUnique"),
            FLAG,
            true,
            "Config",
            0,
            0,
            &[tag!({ type = Condition, var = "Effective" })],
        );
        // L2122
        enemyModList.NewMod(n!("AilmentThreshold"), MORE, 488.0, "Boss", 0, 0, &[]);
        // L2123
        modList.NewMod(n!("WarcryPower"), BASE, 20.0, "Boss", 0, 0, &[]);
        // L2124
        modList.NewMod(n!("Multiplier:EnemyPower"), BASE, 20.0, "Boss", 0, 0, &[]);

        // L2126
        let mut defaultEleResist = 40.0;
        // L2127
        SetPlaceholder(
            build,
            n!("enemyLightningResist"),
            Val::from(defaultEleResist),
            true,
        );
        // L2128
        SetPlaceholder(
            build,
            n!("enemyColdResist"),
            Val::from(defaultEleResist),
            true,
        );
        // L2129
        SetPlaceholder(
            build,
            n!("enemyFireResist"),
            Val::from(defaultEleResist),
            true,
        );
        // L2130
        SetPlaceholder(build, n!("enemyChaosResist"), Val::from(25.0), true);

        // L2132
        let mut defaultLevel = 83.0;
        // L2133
        SetPlaceholder(build, n!("enemyLevel"), Val::from(""), true);
        // L2134
        build.configTab.UpdateLevel(build);
        // L2135
        if let Some(enemyLevel) = build.configTab.enemyLevel.get() {
            // L2136
            defaultLevel = enemyLevel;
        }

        // L2139
        let mut defaultDamage = round(
            level_at(&data.monsterDamageTable, defaultLevel).num() * 1.5 * data.misc.stdBossDPSMult,
            None,
        );
        // L2140
        SetPlaceholder(
            build,
            n!("enemyPhysicalDamage"),
            Val::from(defaultDamage),
            true,
        );
        // L2141
        SetPlaceholder(
            build,
            n!("enemyLightningDamage"),
            Val::from(defaultDamage),
            true,
        );
        // L2142
        SetPlaceholder(build, n!("enemyColdDamage"), Val::from(defaultDamage), true);
        // L2143
        SetPlaceholder(build, n!("enemyFireDamage"), Val::from(defaultDamage), true);
        // L2144
        SetPlaceholder(
            build,
            n!("enemyChaosDamage"),
            Val::from(round(defaultDamage / 2.5, None)),
            true,
        );

        // L2146
        let mut defaultPen = Val::from("");
        // L2147
        SetPlaceholder(
            build,
            n!("enemyPhysicalOverwhelm"),
            defaultPen.clone(),
            true,
        );
        // L2148
        SetPlaceholder(build, n!("enemyLightningPen"), defaultPen.clone(), true);
        // L2149
        SetPlaceholder(build, n!("enemyColdPen"), defaultPen.clone(), true);
        // L2150
        SetPlaceholder(build, n!("enemyFirePen"), defaultPen.clone(), true);

        // L2152
        SetPlaceholder(
            build,
            n!("enemyArmour"),
            level_at(&data.monsterArmourTable, defaultLevel),
            true,
        );
        // L2153
        SetPlaceholder(
            build,
            n!("enemyEvasion"),
            level_at(&data.monsterEvasionTable, defaultLevel),
            true,
        );
    } else if val_eq_str(val, "Pinnacle") {
        // L2155
        enemyModList.NewMod(
            n!("Condition:RareOrUnique"),
            FLAG,
            true,
            "Config",
            0,
            0,
            &[tag!({ type = Condition, var = "Effective" })],
        );
        // L2156
        enemyModList.NewMod(
            n!("Condition:PinnacleBoss"),
            FLAG,
            true,
            "Config",
            0,
            0,
            &[tag!({ type = Condition, var = "Effective" })],
        );
        // L2157
        enemyModList.NewMod(n!("AilmentThreshold"), MORE, 404.0, "Boss", 0, 0, &[]);
        // L2158
        modList.NewMod(n!("WarcryPower"), BASE, 20.0, "Boss", 0, 0, &[]);
        // L2159
        modList.NewMod(n!("Multiplier:EnemyPower"), BASE, 20.0, "Boss", 0, 0, &[]);

        // L2161
        let mut defaultEleResist = 50.0;
        // L2162
        SetPlaceholder(
            build,
            n!("enemyLightningResist"),
            Val::from(defaultEleResist),
            true,
        );
        // L2163
        SetPlaceholder(
            build,
            n!("enemyColdResist"),
            Val::from(defaultEleResist),
            true,
        );
        // L2164
        SetPlaceholder(
            build,
            n!("enemyFireResist"),
            Val::from(defaultEleResist),
            true,
        );
        // L2165
        SetPlaceholder(build, n!("enemyChaosResist"), Val::from(30.0), true);

        // L2167
        let mut defaultLevel = 84.0;
        // L2168
        SetPlaceholder(build, n!("enemyLevel"), Val::from(defaultLevel), true);
        // L2169
        build.configTab.UpdateLevel(build);
        // L2170
        if let Some(enemyLevel) = build.configTab.enemyLevel.get() {
            // L2171
            defaultLevel = m_max(enemyLevel, defaultLevel);
        }

        // L2174
        let mut defaultDamage = round(
            level_at(&data.monsterDamageTable, defaultLevel).num()
                * 1.5
                * data.misc.pinnacleBossDPSMult,
            None,
        );
        // L2175
        SetPlaceholder(
            build,
            n!("enemyPhysicalDamage"),
            Val::from(defaultDamage),
            true,
        );
        // L2176
        SetPlaceholder(
            build,
            n!("enemyLightningDamage"),
            Val::from(defaultDamage),
            true,
        );
        // L2177
        SetPlaceholder(build, n!("enemyColdDamage"), Val::from(defaultDamage), true);
        // L2178
        SetPlaceholder(build, n!("enemyFireDamage"), Val::from(defaultDamage), true);
        // L2179
        SetPlaceholder(
            build,
            n!("enemyChaosDamage"),
            Val::from(round(defaultDamage / 2.5, None)),
            true,
        );

        // L2181
        SetPlaceholder(
            build,
            n!("enemyLightningPen"),
            Val::from(data.misc.pinnacleBossPen),
            true,
        );
        // L2182
        SetPlaceholder(
            build,
            n!("enemyColdPen"),
            Val::from(data.misc.pinnacleBossPen),
            true,
        );
        // L2183
        SetPlaceholder(
            build,
            n!("enemyFirePen"),
            Val::from(data.misc.pinnacleBossPen),
            true,
        );

        // L2185
        SetPlaceholder(
            build,
            n!("enemyArmour"),
            Val::from(round(
                level_at(&data.monsterArmourTable, defaultLevel).num()
                    * (data.bossStats.PinnacleArmourMean / 100.0),
                None,
            )),
            true,
        );
        // L2186
        SetPlaceholder(
            build,
            n!("enemyEvasion"),
            Val::from(round(
                level_at(&data.monsterEvasionTable, defaultLevel).num()
                    * (data.bossStats.PinnacleEvasionMean / 100.0),
                None,
            )),
            true,
        );
    } else if val_eq_str(val, "Uber") {
        // L2188
        enemyModList.NewMod(
            n!("Condition:RareOrUnique"),
            FLAG,
            true,
            "Config",
            0,
            0,
            &[tag!({ type = Condition, var = "Effective" })],
        );
        // L2189
        enemyModList.NewMod(
            n!("Condition:PinnacleBoss"),
            FLAG,
            true,
            "Config",
            0,
            0,
            &[tag!({ type = Condition, var = "Effective" })],
        );
        // L2190
        enemyModList.NewMod(n!("DamageTaken"), MORE, -70.0, "Boss", 0, 0, &[]);
        // L2191
        enemyModList.NewMod(n!("AilmentThreshold"), MORE, 404.0, "Boss", 0, 0, &[]);
        // L2192
        modList.NewMod(n!("WarcryPower"), BASE, 20.0, "Boss", 0, 0, &[]);
        // L2193
        modList.NewMod(n!("Multiplier:EnemyPower"), BASE, 20.0, "Boss", 0, 0, &[]);

        // L2195
        let mut defaultEleResist = 50.0;
        // L2196
        SetPlaceholder(
            build,
            n!("enemyLightningResist"),
            Val::from(defaultEleResist),
            true,
        );
        // L2197
        SetPlaceholder(
            build,
            n!("enemyColdResist"),
            Val::from(defaultEleResist),
            true,
        );
        // L2198
        SetPlaceholder(
            build,
            n!("enemyFireResist"),
            Val::from(defaultEleResist),
            true,
        );
        // L2199
        SetPlaceholder(build, n!("enemyChaosResist"), Val::from(30.0), true);

        // L2201
        let mut defaultLevel = 85.0;
        // L2202
        SetPlaceholder(build, n!("enemyLevel"), Val::from(defaultLevel), true);
        // L2203
        build.configTab.UpdateLevel(build);
        // L2204
        if let Some(enemyLevel) = build.configTab.enemyLevel.get() {
            // L2205
            defaultLevel = m_max(enemyLevel, defaultLevel);
        }

        // L2208
        let mut defaultDamage = round(
            level_at(&data.monsterDamageTable, defaultLevel).num()
                * 1.5
                * data.misc.uberBossDPSMult,
            None,
        );
        // L2209
        SetPlaceholder(
            build,
            n!("enemyPhysicalDamage"),
            Val::from(defaultDamage),
            true,
        );
        // L2210
        SetPlaceholder(
            build,
            n!("enemyLightningDamage"),
            Val::from(defaultDamage),
            true,
        );
        // L2211
        SetPlaceholder(build, n!("enemyColdDamage"), Val::from(defaultDamage), true);
        // L2212
        SetPlaceholder(build, n!("enemyFireDamage"), Val::from(defaultDamage), true);
        // L2213
        SetPlaceholder(
            build,
            n!("enemyChaosDamage"),
            Val::from(round(defaultDamage / 4.0, None)),
            true,
        );

        // L2215
        SetPlaceholder(
            build,
            n!("enemyLightningPen"),
            Val::from(data.misc.uberBossPen),
            true,
        );
        // L2216
        SetPlaceholder(
            build,
            n!("enemyColdPen"),
            Val::from(data.misc.uberBossPen),
            true,
        );
        // L2217
        SetPlaceholder(
            build,
            n!("enemyFirePen"),
            Val::from(data.misc.uberBossPen),
            true,
        );

        // L2219
        SetPlaceholder(
            build,
            n!("enemyArmour"),
            Val::from(round(
                level_at(&data.monsterArmourTable, defaultLevel).num()
                    * (data.bossStats.UberArmourMean / 100.0),
                None,
            )),
            true,
        );
        // L2220
        SetPlaceholder(
            build,
            n!("enemyEvasion"),
            Val::from(round(
                level_at(&data.monsterEvasionTable, defaultLevel).num()
                    * (data.bossStats.UberEvasionMean / 100.0),
                None,
            )),
            true,
        );
    }
}

/// @pob Modules/ConfigOptions.lua:2223-2244 h=f62ab31b
/// `ConfigOptions[660].apply`
/// var = "deliriousPercentage"
pub(crate) fn opt_660_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2224
    if val_eq_str(&val, "20Percent") {
        // L2225
        enemyModList.NewMod(n!("DamageTaken"), MORE, -16.0, "20% Delirious", 0, 0, &[]);
        // L2226
        enemyModList.NewMod(n!("Damage"), INC, 6.0, "20% Delirious", 0, 0, &[]);
    }
    // L2228
    if val_eq_str(&val, "40Percent") {
        // L2229
        enemyModList.NewMod(n!("DamageTaken"), MORE, -32.0, "40% Delirious", 0, 0, &[]);
        // L2230
        enemyModList.NewMod(n!("Damage"), INC, 12.0, "40% Delirious", 0, 0, &[]);
    }
    // L2232
    if val_eq_str(&val, "60Percent") {
        // L2233
        enemyModList.NewMod(n!("DamageTaken"), MORE, -48.0, "60% Delirious", 0, 0, &[]);
        // L2234
        enemyModList.NewMod(n!("Damage"), INC, 18.0, "60% Delirious", 0, 0, &[]);
    }
    // L2236
    if val_eq_str(&val, "80Percent") {
        // L2237
        enemyModList.NewMod(n!("DamageTaken"), MORE, -64.0, "80% Delirious", 0, 0, &[]);
        // L2238
        enemyModList.NewMod(n!("Damage"), INC, 24.0, "80% Delirious", 0, 0, &[]);
    }
    // L2240
    if val_eq_str(&val, "100Percent") {
        // L2241
        enemyModList.NewMod(n!("DamageTaken"), MORE, -80.0, "100% Delirious", 0, 0, &[]);
        // L2242
        enemyModList.NewMod(n!("Damage"), INC, 30.0, "100% Delirious", 0, 0, &[]);
    }
}

/// @pob Modules/ConfigOptions.lua:2245-2247 h=74b22a46
/// `ConfigOptions[661].apply`
/// var = "enemyPhysicalReduction"
pub(crate) fn opt_661_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2246
    enemyModList.NewMod(
        n!("PhysicalDamageReduction"),
        BASE,
        val_value(&val),
        "EnemyConfig",
        0,
        0,
        &[],
    );
}

/// @pob Modules/ConfigOptions.lua:2248-2250 h=4cbad8e6
/// `ConfigOptions[662].apply`
/// var = "enemyLightningResist"
pub(crate) fn opt_662_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2249
    enemyModList.NewMod(
        n!("LightningResist"),
        BASE,
        val_value(&val),
        "EnemyConfig",
        0,
        0,
        &[],
    );
}

/// @pob Modules/ConfigOptions.lua:2251-2253 h=4a362c5d
/// `ConfigOptions[663].apply`
/// var = "enemyColdResist"
pub(crate) fn opt_663_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2252
    enemyModList.NewMod(
        n!("ColdResist"),
        BASE,
        val_value(&val),
        "EnemyConfig",
        0,
        0,
        &[],
    );
}

/// @pob Modules/ConfigOptions.lua:2254-2256 h=6d182222
/// `ConfigOptions[664].apply`
/// var = "enemyFireResist"
pub(crate) fn opt_664_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2255
    enemyModList.NewMod(
        n!("FireResist"),
        BASE,
        val_value(&val),
        "EnemyConfig",
        0,
        0,
        &[],
    );
}

/// @pob Modules/ConfigOptions.lua:2257-2259 h=95958d54
/// `ConfigOptions[665].apply`
/// var = "enemyChaosResist"
pub(crate) fn opt_665_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2258
    enemyModList.NewMod(
        n!("ChaosResist"),
        BASE,
        val_value(&val),
        "EnemyConfig",
        0,
        0,
        &[],
    );
}

/// @pob Modules/ConfigOptions.lua:2260-2262 h=6ec8145d
/// `ConfigOptions[666].apply`
/// var = "enemyMaxResist"
pub(crate) fn opt_666_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2261
    enemyModList.NewMod(
        n!("DoNotChangeMaxResFromConfig"),
        FLAG,
        true,
        "EnemyConfig",
        0,
        0,
        &[],
    );
}

/// @pob Modules/ConfigOptions.lua:2263-2265 h=c2e2a2f4
/// `ConfigOptions[667].apply`
/// var = "enemyBlockChance"
pub(crate) fn opt_667_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2264
    enemyModList.NewMod(
        n!("BlockChance"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[],
    );
}

/// @pob Modules/ConfigOptions.lua:2266-2268 h=bd246c76
/// `ConfigOptions[668].apply`
/// var = "enemyEvasion"
pub(crate) fn opt_668_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2267
    enemyModList.NewMod(n!("Evasion"), BASE, val_value(&val), "Config", 0, 0, &[]);
}

/// @pob Modules/ConfigOptions.lua:2269-2271 h=7f4a0aa4
/// `ConfigOptions[669].apply`
/// var = "enemyArmour"
pub(crate) fn opt_669_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2270
    enemyModList.NewMod(n!("Armour"), BASE, val_value(&val), "Config", 0, 0, &[]);
}

/// @pob Modules/ConfigOptions.lua:2272-2351 h=12f0c752
/// `ConfigOptions[670].apply`
/// var = "presetBossSkills"
pub(crate) fn opt_670_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // Hand-ported (T1 does not translate `data.bossSkills[val]` or method calls on UI controls).
    // The controls are the control-state model (helpers at the top of this file, CCR-0037).
    let data = data();
    // L2273
    if !val_eq_str(val, "None") {
        // L2274
        let mut bossData = match val {
            Val::Str(s) => data.bossSkills.get(&Name::from(&**s)),
            _ => None,
        };
        // L2275
        let mut isUber = enemyIsBoss_selVal(build).raw_eq(&Val::from("Uber"));
        // L2276
        let Some(bossData) = bossData else {
            lua_error!("attempt to index local 'bossData' (a nil value)")
        };
        if bossData.earlierUber && enemyIsBoss_selVal(build).raw_eq(&Val::from("Pinnacle")) {
            // L2277
            isUber = true;
        }
        // L2279
        let mut defaultDamage = Val::from("");
        // L2280
        SetPlaceholder(
            build,
            n!("enemyPhysicalDamage"),
            defaultDamage.clone(),
            true,
        );
        // L2281
        SetPlaceholder(
            build,
            n!("enemyLightningDamage"),
            defaultDamage.clone(),
            true,
        );
        // L2282
        SetPlaceholder(build, n!("enemyColdDamage"), defaultDamage.clone(), true);
        // L2283
        SetPlaceholder(build, n!("enemyFireDamage"), defaultDamage.clone(), true);
        // L2284
        SetPlaceholder(build, n!("enemyChaosDamage"), defaultDamage.clone(), true);

        // L2286
        let mut rollRangeMult = m_min(
            m_max(
                checknum(
                    &lor(
                        build.configTab.input.get(n!("enemyDamageRollRange")),
                        || varControl(build, n!("enemyDamageRollRange")).get(n!("placeholder")),
                    ),
                    "max",
                ),
                0.0,
            ),
            100.0,
        );
        // L2287 LUA-ORDER (PoB: pairs; each entry sets the placeholder of its own control)
        for (damageType, damageMult) in &bossData.DamageMultipliers {
            // L2288
            if isUber && bossData.UberDamageMultiplier.is_some() {
                let UberDamageMultiplier = bossData.UberDamageMultiplier.unwrap();
                // L2289
                SetPlaceholder(
                    build,
                    cat!("enemy", *damageType, "Damage"),
                    Val::from(round(
                        enemyLevel_at(&data.monsterDamageTable, build).num()
                            * (vec_at(damageMult, 1) + rollRangeMult * vec_at(damageMult, 2))
                            * UberDamageMultiplier,
                        None,
                    )),
                    true,
                );
            } else {
                // L2291
                SetPlaceholder(
                    build,
                    cat!("enemy", *damageType, "Damage"),
                    Val::from(round(
                        enemyLevel_at(&data.monsterDamageTable, build).num()
                            * (vec_at(damageMult, 1) + rollRangeMult * vec_at(damageMult, 2)),
                        None,
                    )),
                    true,
                );
            }
        }

        // L2295
        let mut defaultPen = Val::from("");
        // L2296
        SetPlaceholder(
            build,
            n!("enemyPhysicalOverwhelm"),
            defaultPen.clone(),
            true,
        );
        // L2297
        SetPlaceholder(build, n!("enemyLightningPen"), defaultPen.clone(), true);
        // L2298
        SetPlaceholder(build, n!("enemyColdPen"), defaultPen.clone(), true);
        // L2299
        SetPlaceholder(build, n!("enemyFirePen"), defaultPen.clone(), true);

        // L2301
        if let Some(DamagePenetrations) = &bossData.DamagePenetrations {
            // L2302 LUA-ORDER (PoB: pairs; each entry sets the placeholder of its own control)
            for (penType, pen) in DamagePenetrations {
                // L2303
                let uberPen = bossData
                    .UberDamagePenetrations
                    .as_ref()
                    .map_or(Val::Nil, |u| {
                        u.get(penType).map_or(Val::Nil, Val::from_value)
                    });
                if isUber && bossData.UberDamagePenetrations.is_some() && uberPen.truthy() {
                    // L2304
                    SetPlaceholder(build, cat!("enemy", *penType), uberPen, true);
                } else {
                    // L2306
                    SetPlaceholder(build, cat!("enemy", *penType), Val::from_value(pen), true);
                }
            }
        }

        // L2311 (`DamageType` is a required field of the schema: always truthy)
        {
            // L2312
            build.configTab.SelByValue(
                n!("enemyDamageType"),
                &Val::from(bossData.DamageType),
                n!("val"),
            );
            // L2313
            build
                .configTab
                .input
                .set(n!("enemyDamageType"), bossData.DamageType);
        }
        // L2315
        varControl(build, n!("enemyDamageType")).set(n!("enabled"), false);

        // L2317 (`speed` is a required field of the schema: the elseif is always taken)
        if isUber && bossData.UberSpeed.is_some() {
            // L2318
            SetPlaceholder(build, n!("enemySpeed"), Val::from(bossData.UberSpeed), true);
        } else {
            // L2320
            SetPlaceholder(build, n!("enemySpeed"), Val::from(bossData.speed), true);
        }
        // L2322
        if let Some(critChance) = bossData.critChance {
            // L2323
            SetPlaceholder(build, n!("enemyCritChance"), Val::from(critChance), true);
        }

        // L2326
        modList.NewMod(n!("BossSkillActive"), FLAG, true, "Config", 0, 0, &[]);

        // boss specific mods
        // L2329
        if val_eq_str(val, "Atziri Flameblast") && isUber {
            // L2330
            enemyModList.NewMod(n!("Damage"), INC, 60.0, "Alluring Abyss Map Mod", 0, 0, &[]);
        }
        // L2332
        if let Some(addl) = bossData.additionalStats.as_ref().filter(|v| v.truthy()) {
            // L2333
            let addl = Val::from_value(addl);
            let mut additionalStats = lor(land(Val::Bool(isUber), || addl.get(n!("uber"))), || {
                addl.get(n!("base"))
            });
            // L2334
            if additionalStats.truthy() {
                // L2335 LUA-ORDER (PoB: pairs, so the order of these enemy mods is random per
                // process; the data has string keys only)
                let Val::Data(Value::Table(stats)) = &additionalStats else {
                    lua_error!(
                        "bad argument #1 to 'pairs' (table expected, got {})",
                        additionalStats.lua_type()
                    )
                };
                for (k, v) in &stats.fields {
                    // L2336
                    if tostring_value(v) == "flag" {
                        // L2337
                        enemyModList.NewMod(*k, FLAG, true, "BossSkillAdditionalData", 0, 0, &[]);
                    } else {
                        // L2339
                        enemyModList.NewMod(
                            *k,
                            BASE,
                            v.clone(),
                            "BossSkillAdditionalData",
                            0,
                            0,
                            &[],
                        );
                    }
                }
            }
        }
    } else {
        // L2345
        if varControl(build, n!("enemyDamageType"))
            .get(n!("enabled"))
            .raw_eq(&Val::Bool(false))
        {
            // L2346
            build.configTab.input.set(n!("enemyDamageType"), "Average");
            // L2347
            build
                .configTab
                .SelByValue(n!("enemyDamageType"), &Val::from("Average"), n!("val"));
        }
        // L2349
        varControl(build, n!("enemyDamageType")).set(n!("enabled"), true);
    }
}

// @pob-skip Modules/ConfigOptions.lua:2352-2362 reason="option definition data (dumped: data().configOptions / ConfigOptionDef)"

/// @pob Modules/ConfigOptions.lua:2363-2365 h=26c0bbc1
/// `ConfigOptions[674].apply`
/// var = "enemyMultiplierPvpDamage"
pub(crate) fn opt_674_apply(
    val: &Val,
    modList: &ModList,
    enemyModList: &ModList,
    build: &BuildRef,
) {
    // L2364
    enemyModList.NewMod(
        n!("MultiplierPvpDamage"),
        BASE,
        val_value(&val),
        "Config",
        0,
        0,
        &[],
    );
}

// @pob-skip Modules/ConfigOptions.lua:2366-2380 reason="option definition data (dumped: data().configOptions / ConfigOptionDef)"
