//! Stand-in for the `pob-calc` types ConfigOptions apply fns use (calc doc §5.1 Val/Table,
//! §5.3 ModStore::NewMod/AddMod, §5.8 Build.configTab.input, §8.1 FnRow/ConfigApply, §5.7 unported!).

use crate::gamedata::{createMod, ModRef, ModType, Tag, Value};
use pob_lua::Name;
use std::rc::Rc;

pub type LStr = Rc<str>;

#[derive(Clone, Default)]
pub enum Val {
    #[default]
    Nil,
    Bool(bool),
    Num(f64),
    Str(LStr),
    Table(Table),
    Data(Value),
    Mod(ModRef),
    Obj(Rc<dyn std::any::Any>),
}
impl Val {
    pub fn truthy(&self) -> bool { !matches!(self, Val::Nil | Val::Bool(false)) }
    pub fn num(&self) -> f64 { unimplemented!() }
    pub fn opt_num(&self) -> Option<f64> { unimplemented!() }
    pub fn num_or(&self, d: f64) -> f64 { unimplemented!() }
    pub fn str(&self) -> Option<LStr> { unimplemented!() }
}
impl From<f64> for Val { fn from(x: f64) -> Self { Val::Num(x) } }
impl From<bool> for Val { fn from(b: bool) -> Self { Val::Bool(b) } }
impl From<&str> for Val { fn from(s: &str) -> Self { Val::Str(LStr::from(s)) } }

#[derive(Clone, Default)]
pub struct Table(Rc<()>);
impl Table { pub fn get(&self, k: Name) -> Val { unimplemented!() } }

pub struct ModStore;
impl ModStore {
    pub fn NewMod(&self, name: Name, ty: ModType, value: impl Into<Value>, source: &str, flags: u32, keywordFlags: u32, tags: &[Tag]) {}
    pub fn AddMod(&self, m: ModRef) {}
}
pub type ModList = Rc<ModStore>;

pub struct ConfigTab { pub input: Table }
pub struct Build { pub configTab: ConfigTab }
pub type BuildRef = Rc<Build>;

pub struct FnRow<F> { pub id: &'static str, pub hash: &'static str, pub f: F }
pub type ConfigApply = fn(val: &Val, modList: &ModList, enemyModList: &ModList, build: &BuildRef);

#[macro_export]
macro_rules! unported { ($site:literal) => {{ let _ = $site; Default::default() }} }
