//! State export for the oracle (calc doc §9.2): `ctx_struct!`, `ProbeState`,
//! and (feature `probe`) `probe!(ctx, "<id>")`.
//!
//! Owner: ORC-X. Without the `probe` feature this module only defines the
//! ctx machinery (`ctx_struct!` generates each segment ctx struct together
//! with its [`ProbeState`] impl, whose `locals()` are the Lua local names).
//!
//! With `probe` (enabled by pob-oracle only) it also holds the Rust side of
//! the pob-state/1 format (crates/pob-oracle/README.md, `lua/state_dump.lua`):
//!
//! * a reflection layer over the calc's object graph ([`PObj`]: env, actors,
//!   active skills, skill effects, cfgs, mod stores, items, gem instances,
//!   socket groups, the `Shared` lists and maps, `GlobalCache`), which maps
//!   every typed field to its verbatim Lua key and back;
//! * [`export_root`]: the encoder, a faithful re-implementation of
//!   `state_dump.lua`'s two passes (breadth-first canonical paths and
//!   reference counts, then depth-first emission with `$id`/`$ref` numbered in
//!   emission order, `$keys` labels, store / item / skill headers), so a Rust
//!   graph exports to the same JSON PoB dumps;
//! * the injection [`Registry`] (thread-local): what a typed Rust struct
//!   cannot hold by itself but a faithful export needs — fields that were nil
//!   in Lua (`absent`), other envs' objects (`foreign` markers), the dumped
//!   `$uuid`s and mod `$at`s, object-keyed table entries and mod-store fields
//!   the frozen types have no slot for, the upvalue markers of
//!   function `__index` layers.
//!
//! `pob_oracle::inject` fills a Rust graph from a dump through the same
//! reflection ([`Reflect::r_set`], [`set_member`]), so injection and export
//! cannot drift apart.
#![allow(non_snake_case)]

/// A segment context whose fields are Lua locals, by name.
pub trait ProbeState {
    /// The field names as written (`r#override` for `override`); see [`ProbeState::locals`].
    const FIELDS: &'static [&'static str];

    /// The verbatim Lua local names, in declaration order.
    fn locals() -> Vec<&'static str> {
        Self::FIELDS
            .iter()
            .map(|f| f.strip_prefix("r#").unwrap_or(f))
            .collect()
    }

    /// Every field as a Lua value, by its Lua local name (feature `probe`).
    #[cfg(feature = "probe")]
    fn probe_locals(&self) -> Vec<(&'static str, crate::lua_table::Val)>;

    /// Assign the field whose Lua local name is `name` from an injected value.
    /// Returns false when the name is unknown or the value has the wrong type.
    #[cfg(feature = "probe")]
    fn probe_set(&mut self, name: &str, v: &crate::lua_table::Val) -> bool;
}

/// `ctx_struct! { pub struct X { a: A, b: B } }`: the struct with public
/// fields plus its [`ProbeState`] impl.
#[macro_export]
macro_rules! ctx_struct {
    ($(#[$m:meta])* pub struct $name:ident { $($f:ident : $t:ty),* $(,)? }) => {
        $(#[$m])*
        pub struct $name {
            $(pub $f: $t),*
        }
        impl $crate::probe::ProbeState for $name {
            const FIELDS: &'static [&'static str] = &[$(stringify!($f)),*];
            #[cfg(feature = "probe")]
            fn probe_locals(&self) -> Vec<(&'static str, $crate::lua_table::Val)> {
                vec![$((
                    $crate::probe::lua_local_name(stringify!($f)),
                    $crate::probe::ProbeField::to_val(&self.$f),
                )),*]
            }
            #[cfg(feature = "probe")]
            fn probe_set(&mut self, name: &str, v: &$crate::lua_table::Val) -> bool {
                $(
                    if name == $crate::probe::lua_local_name(stringify!($f)) {
                        return match <$t as $crate::probe::ProbeField>::from_val(v) {
                            Some(x) => {
                                self.$f = x;
                                true
                            }
                            None => false,
                        };
                    }
                )*
                false
            }
        }
    };
}

/// `probe!(ctx, "<id>")`: export the ctx at a stage boundary to the installed
/// sink ([`set_sink`]; feature `probe`). A no-op without the feature or a sink.
#[macro_export]
macro_rules! probe {
    ($ctx:expr, $id:literal) => {{
        #[cfg(feature = "probe")]
        {
            $crate::probe::emit_ctx($id, &$ctx);
        }
    }};
}

/// `r#override` → `override`.
pub fn lua_local_name(f: &'static str) -> &'static str {
    f.strip_prefix("r#").unwrap_or(f)
}

#[cfg(feature = "probe")]
pub use imp::*;

#[cfg(feature = "probe")]
mod imp {
    use std::any::Any;
    use std::cell::{Cell, RefCell};
    use std::collections::{HashMap, HashSet};
    use std::rc::{Rc, Weak};
    use std::sync::Arc;

    use pob_gamedata::{
        GemDataRef, GrantedEffectRef, ItemBaseRef, MinionDataRef, Mod, ModRef, NameSet,
        SkillLevelRef, SkillTypeSet, Tag, TagType, Value, ValueTable,
    };
    use pob_lua::{CheapClone, Field, LjIntMap, Name, PtrMap, PtrSet, Shared};
    use serde_json::{Map, Value as J};

    use super::ProbeState;
    use crate::active_skill::{ActiveSkill, ActiveSkillRef, SkillEffect, SkillEffectRef};
    use crate::actor::{Actor, ActorRef, ModPtrSet};
    use crate::breakdown::Breakdown;
    use crate::cfg::{Cfg, ModCfg, SkillTypes};
    use crate::classes::{
        Item, ItemRef, ModStore, ModStoreRef, Node, NodeRef, PassiveSpec, PassiveSpecRef,
        StoreKind, StoreMods,
    };
    use crate::env::{Accelerate, Env, EnvRef, ExplodeSource, Mode, Override, SpecEnv};
    use crate::modules::calc_perform::setup_flasks::{FlaskUpvalues, TinctureUpvalues};
    use crate::global_cache::{CachedData, GlobalCache};
    use crate::lua_table::{IndexMeta, Key, LStr, Table, Val};
    use crate::model::{Build, BuildRef, GemInstance, GemInstanceRef, SocketGroup, SocketGroupRef};

    // =====================================================================================
    // Kinds and leaf objects
    // =====================================================================================

    /// What a Lua table is on the Rust side. `Any` and `Table` are weak (a
    /// dynamic table); every other kind is a typed Rust object.
    #[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
    pub enum Kind {
        Any,
        Table,
        Env,
        Actor,
        Skill,
        Effect,
        Cfg,
        Store,
        Item,
        Gem,
        Group,
        Override,
        Breakdown,
        SkillTypes,
        NameSet,
        SkillList,
        EffectList,
        TableList,
        NameList,
        ModVec,
        GemList,
        NodeMap,
        BoolMap,
        ItemFlags,
        GemSet,
        ItemList,
        /// `env.explodeSources` / `socketGroup.explodeSources`: nodes and items (CCR-0030 #3).
        ExplodeList,
        /// `env.flaskSlotMap`: item → slot number (CCR-0030 #5).
        FlaskSlots,
        /// `actor.appliedEnemyModifiers`: mod → true (CCR-0030 #6).
        ModSet,
        /// `item.modList` / `item.slotModList[i]`: a plain list of mods in Lua
        /// (`{ unpack(modList) }`, Item.lua:2518), a ModList in Rust.
        ItemMods,
        /// `item.slotModList`: a plain list of those.
        SlotMods,
        /// Leaves: a passive tree node, a data table, the build, the spec.
        Node,
        Data,
        Build,
        Spec,
        Scalar,
    }

    impl Kind {
        /// A typed kind (not a dynamic table).
        pub fn is_strong(self) -> bool {
            !matches!(self, Kind::Any | Kind::Table | Kind::Scalar)
        }
        /// Kinds that are leaves (markers) in the dump.
        pub fn is_leaf(self) -> bool {
            matches!(
                self,
                Kind::Node | Kind::Data | Kind::Build | Kind::Spec | Kind::Scalar
            )
        }
    }

    /// `{"$data": "<path>"}`: a table owned by PoB's `data`.
    pub struct DataRef {
        pub path: String,
        pub target: DataTarget,
    }

    #[derive(Clone)]
    pub enum DataTarget {
        Skill(GrantedEffectRef),
        Gem(GemDataRef),
        Level(SkillLevelRef),
        ItemBase(ItemBaseRef),
        Minion(MinionDataRef),
        /// `data.skills.<id>.minionUses` (`minion.uses`, CCR-0030 #2).
        NameSet(NameSet),
        Root,
        Other,
    }

    /// An opaque marker kept verbatim (`$object`, `$class`, `$foreign`, `$env`,
    /// `$fn`, `$bytes`, `$cycle`, …).
    pub struct Marker(pub J);

    /// A `Shared` value stored in a dynamic table (`Val::Obj`).
    pub struct SBox<T>(pub Shared<T>);

    /// `cfg.baseFlags` stored in a dynamic table.
    pub struct NameSetBox(pub NameSet);

    // =====================================================================================
    // Registry
    // =====================================================================================

    /// What a faithful export needs beyond the typed fields (filled by
    /// `pob_oracle::inject`; thread-local like the calc).
    #[derive(Default)]
    pub struct Registry {
        /// Keeps every registered object alive (pointer keys stay valid).
        keep: Vec<Rc<dyn Any>>,
        keep_mods: Vec<ModRef>,
        keep_tables: Vec<Table>,
        /// (object ptr, typed Lua key) that was nil in Lua.
        absent: HashSet<(usize, &'static str)>,
        /// objects of other envs (or the build's): exported as this marker.
        foreign: HashMap<usize, J>,
        /// dumped `$uuid` of an active skill.
        uuid: HashMap<usize, String>,
        /// dumped `"$at"` of a mod, per store as state_dump.lua's `modAt[store][mod]`
        /// (store ptr, mod Arc ptr).
        mod_at: HashMap<(usize, usize), String>,
        /// fields of objects without an `extra` table (mod-store keys other than the typed ones).
        side_extra: HashMap<usize, Vec<(String, Val)>>,
        /// object-keyed entries of a dynamic table.
        obj_keys: HashMap<usize, Vec<(Val, Val)>>,
        /// the `$fn` marker of a table's function `__index` layer (upvalues are not dumped).
        meta_fn: HashMap<usize, J>,
        /// `$data` path of a data Arc as the dump spelled it.
        data_path: HashMap<usize, String>,
        /// counters of injection fallbacks, by "<kind>.<key>: reason".
        pub fallbacks: HashMap<String, u64>,
    }

    thread_local! {
        static REG: RefCell<Registry> = RefCell::new(Registry::default());
    }

    /// Run `f` on the registry.
    pub fn with_registry<R>(f: impl FnOnce(&mut Registry) -> R) -> R {
        REG.with(|r| f(&mut r.borrow_mut()))
    }

    /// Forget everything registered (start of an injection).
    pub fn registry_reset() {
        REG.with(|r| *r.borrow_mut() = Registry::default());
    }

    pub fn rc_ptr<T: ?Sized>(rc: &Rc<T>) -> usize {
        Rc::as_ptr(rc).cast::<()>() as usize
    }

    fn arc_ptr<T: ?Sized>(a: &Arc<T>) -> usize {
        Arc::as_ptr(a).cast::<()>() as usize
    }

    impl Registry {
        pub fn keep(&mut self, o: Rc<dyn Any>) {
            self.keep.push(o);
        }
        pub fn set_absent(&mut self, ptr: usize, key: &'static str) {
            self.absent.insert((ptr, key));
        }
        pub fn is_absent(&self, ptr: usize, key: &str) -> bool {
            self.absent.contains(&(ptr, leak_key(key)))
        }
        pub fn set_foreign(&mut self, o: Rc<dyn Any>, marker: J) {
            self.foreign.insert(rc_ptr(&o), marker);
            self.keep.push(o);
        }
        /// A dynamic table of another env (`{"$foreign": …}`): exported as `marker`.
        pub fn set_foreign_table(&mut self, t: &Table, marker: J) {
            self.foreign.insert(t.as_ptr() as usize, marker);
            self.keep_tables.push(t.clone());
        }
        pub fn set_uuid(&mut self, skill: &ActiveSkillRef, uuid: String) {
            self.uuid.insert(rc_ptr(skill), uuid);
        }
        pub fn set_mod_at(&mut self, store: &ModStoreRef, m: &ModRef, at: String) {
            self.mod_at.insert((rc_ptr(store), arc_ptr(m)), at);
            self.keep_mods.push(m.clone());
        }
        pub fn push_side_extra(&mut self, ptr: usize, key: String, v: Val) {
            self.side_extra.entry(ptr).or_default().push((key, v));
        }
        pub fn push_obj_key(&mut self, t: &Table, k: Val, v: Val) {
            self.obj_keys.entry(t.as_ptr() as usize).or_default().push((k, v));
        }
        pub fn set_meta_fn(&mut self, t: &Table, marker: J) {
            self.meta_fn.insert(t.as_ptr() as usize, marker);
        }
        pub fn set_data_path(&mut self, ptr: usize, path: String) {
            self.data_path.insert(ptr, path);
        }
        pub fn obj_keys(&self, t: &Table) -> Vec<(Val, Val)> {
            self.obj_keys
                .get(&(t.as_ptr() as usize))
                .cloned()
                .unwrap_or_default()
        }
        pub fn fallback(&mut self, what: String) {
            *self.fallbacks.entry(what).or_default() += 1;
        }
    }

    fn reg_absent(ptr: usize, key: &str) -> bool {
        REG.with(|r| r.borrow().absent.contains(&(ptr, leak_key(key))))
    }

    /// Interned `'static` copies of field keys (the absent set is keyed by them).
    fn leak_key(key: &str) -> &'static str {
        thread_local! {
            static KEYS: RefCell<HashMap<String, &'static str>> = RefCell::new(HashMap::new());
        }
        KEYS.with(|m| {
            let mut m = m.borrow_mut();
            if let Some(k) = m.get(key) {
                return *k;
            }
            let k: &'static str = Box::leak(key.to_owned().into_boxed_str());
            m.insert(key.to_owned(), k);
            k
        })
    }

    /// Mark `key` of the object at `ptr` as nil in Lua.
    pub fn mark_absent(ptr: usize, key: &str) {
        let k = leak_key(key);
        REG.with(|r| r.borrow_mut().absent.insert((ptr, k)));
    }

    /// Undo [`mark_absent`] (the key is set again).
    pub fn unmark_absent(ptr: usize, key: &str) {
        let k = leak_key(key);
        REG.with(|r| r.borrow_mut().absent.remove(&(ptr, k)));
    }

    fn reg_foreign(ptr: usize) -> Option<J> {
        REG.with(|r| r.borrow().foreign.get(&ptr).cloned())
    }

    // =====================================================================================
    // Data paths
    // =====================================================================================

    /// Lua-like path step (crates/pob-dump/FORMAT.md, state_dump.lua `pathStep`).
    pub fn path_step(base: &str, key: &str) -> String {
        let ident = key
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if ident {
            format!("{base}.{key}")
        } else {
            format!("{base}[{}]", serde_json::to_string(key).unwrap_or_default())
        }
    }

    struct DataIndex {
        by_ptr: HashMap<usize, String>,
    }

    fn data_index_path(ptr: usize) -> Option<String> {
        thread_local! {
            static IDX: RefCell<Option<Rc<DataIndex>>> = const { RefCell::new(None) };
        }
        if let Some(p) = REG.with(|r| r.borrow().data_path.get(&ptr).cloned()) {
            return Some(p);
        }
        if !pob_gamedata::is_initialised() {
            return None;
        }
        let idx = IDX.with(|i| {
            i.borrow_mut()
                .get_or_insert_with(|| {
                    let d = pob_gamedata::data();
                    let mut by_ptr = HashMap::new();
                    for (id, ge) in &d.skills {
                        let base = path_step("data.skills", id.as_str());
                        for (i, l) in ge.levels.0.iter().enumerate() {
                            if let Some(l) = l {
                                by_ptr
                                    .entry(arc_ptr(l))
                                    .or_insert_with(|| format!("{base}.levels[{}]", i + 1));
                            }
                        }
                        if let Some(u) = &ge.minionUses {
                            by_ptr
                                .entry(arc_ptr(u))
                                .or_insert_with(|| format!("{base}.minionUses"));
                        }
                        by_ptr.entry(arc_ptr(ge)).or_insert(base);
                    }
                    for (id, g) in &d.gems {
                        by_ptr
                            .entry(arc_ptr(g))
                            .or_insert_with(|| path_step("data.gems", id.as_str()));
                    }
                    for (id, b) in &d.itemBases {
                        by_ptr
                            .entry(arc_ptr(b))
                            .or_insert_with(|| path_step("data.itemBases", id));
                    }
                    for (id, m) in &d.minions {
                        by_ptr
                            .entry(arc_ptr(m))
                            .or_insert_with(|| path_step("data.minions", id.as_str()));
                    }
                    for (id, m) in &d.spectres {
                        by_ptr
                            .entry(arc_ptr(m))
                            .or_insert_with(|| path_step("data.spectres", id.as_str()));
                    }
                    Rc::new(DataIndex { by_ptr })
                })
                .clone()
        });
        idx.by_ptr.get(&ptr).cloned()
    }

    fn data_val(ptr: usize, target: DataTarget) -> Val {
        let path = data_index_path(ptr).unwrap_or_else(|| "data.?".to_owned());
        Val::Obj(Rc::new(DataRef { path, target }))
    }

    // =====================================================================================
    // ProbeField: field values <-> Lua values
    // =====================================================================================

    /// A ctx / struct field type that converts to and from a Lua value
    /// (typed objects as `Val::Obj`).
    pub trait ProbeField: Sized {
        fn to_val(&self) -> Val;
        fn from_val(v: &Val) -> Option<Self>;
        fn kind() -> Kind {
            Kind::Scalar
        }
    }

    impl ProbeField for f64 {
        fn to_val(&self) -> Val {
            Val::Num(*self)
        }
        fn from_val(v: &Val) -> Option<Self> {
            v.opt_num()
        }
    }
    impl ProbeField for bool {
        fn to_val(&self) -> Val {
            Val::Bool(*self)
        }
        fn from_val(v: &Val) -> Option<Self> {
            match v {
                Val::Bool(b) => Some(*b),
                _ => None,
            }
        }
    }
    impl ProbeField for u32 {
        fn to_val(&self) -> Val {
            Val::Num(f64::from(*self))
        }
        fn from_val(v: &Val) -> Option<Self> {
            v.opt_num().map(|n| n as u32)
        }
    }
    impl ProbeField for Name {
        fn to_val(&self) -> Val {
            Val::Str(LStr::from(self.as_str()))
        }
        fn from_val(v: &Val) -> Option<Self> {
            v.str().map(|s| Name::from(&s))
        }
    }
    impl ProbeField for LStr {
        fn to_val(&self) -> Val {
            Val::Str(self.clone())
        }
        fn from_val(v: &Val) -> Option<Self> {
            v.str()
        }
    }
    impl ProbeField for Mode {
        fn to_val(&self) -> Val {
            Val::Str(LStr::from(self.as_str()))
        }
        fn from_val(v: &Val) -> Option<Self> {
            match v.str().as_deref() {
                Some("MAIN") => Some(Mode::MAIN),
                Some("CALCS") => Some(Mode::CALCS),
                Some("CALCULATOR") => Some(Mode::CALCULATOR),
                _ => None,
            }
        }
    }
    impl ProbeField for Table {
        fn to_val(&self) -> Val {
            Val::Table(self.clone())
        }
        fn from_val(v: &Val) -> Option<Self> {
            v.table()
        }
        fn kind() -> Kind {
            Kind::Table
        }
    }
    impl ProbeField for ModRef {
        fn to_val(&self) -> Val {
            Val::Mod(self.clone())
        }
        fn from_val(v: &Val) -> Option<Self> {
            match v {
                Val::Mod(m) => Some(m.clone()),
                _ => None,
            }
        }
    }
    impl<T: ProbeField> ProbeField for Option<T> {
        fn to_val(&self) -> Val {
            self.as_ref().map_or(Val::Nil, ProbeField::to_val)
        }
        fn from_val(v: &Val) -> Option<Self> {
            if v.is_nil() {
                Some(None)
            } else {
                T::from_val(v).map(Some)
            }
        }
        fn kind() -> Kind {
            T::kind()
        }
    }
    impl ProbeField for Weak<Actor> {
        fn to_val(&self) -> Val {
            self.upgrade().map_or(Val::Nil, |a| Val::Obj(a))
        }
        fn from_val(v: &Val) -> Option<Self> {
            if v.is_nil() {
                return Some(Weak::new());
            }
            v.obj::<Actor>().map(|a| Rc::downgrade(&a))
        }
        fn kind() -> Kind {
            Kind::Actor
        }
    }
    impl ProbeField for Breakdown {
        fn to_val(&self) -> Val {
            Val::Obj(Rc::new(self.clone()))
        }
        fn from_val(v: &Val) -> Option<Self> {
            v.obj::<Breakdown>().map(|b| (*b).clone())
        }
        fn kind() -> Kind {
            Kind::Breakdown
        }
    }

    macro_rules! rc_field {
        ($($t:ty => $k:ident),* $(,)?) => {$(
            impl ProbeField for Rc<$t> {
                fn to_val(&self) -> Val {
                    Val::Obj(self.clone())
                }
                fn from_val(v: &Val) -> Option<Self> {
                    v.obj::<$t>()
                }
                fn kind() -> Kind {
                    Kind::$k
                }
            }
        )*};
    }
    rc_field!(
        Env => Env, Actor => Actor, ActiveSkill => Skill, SkillEffect => Effect, ModCfg => Cfg,
        ModStore => Store, Item => Item, Node => Node, GemInstance => Gem, SocketGroup => Group,
        Override => Override, Build => Build, PassiveSpec => Spec,
    );

    macro_rules! sbox_field {
        ($($t:ty => $k:ident),* $(,)?) => {$(
            impl ProbeField for Shared<$t> {
                fn to_val(&self) -> Val {
                    Val::Obj(Rc::new(SBox(self.clone())))
                }
                fn from_val(v: &Val) -> Option<Self> {
                    v.obj::<SBox<$t>>().map(|b| b.0.clone())
                }
                fn kind() -> Kind {
                    Kind::$k
                }
            }
        )*};
    }
    sbox_field!(
        Vec<ActiveSkillRef> => SkillList, Vec<SkillEffectRef> => EffectList, Vec<Table> => TableList,
        Vec<Name> => NameList, Vec<ModRef> => ModVec, Vec<GemInstanceRef> => GemList,
        LjIntMap<NodeRef> => NodeMap, LjIntMap<bool> => BoolMap, PtrMap<Item, bool> => ItemFlags,
        PtrSet<GemInstance> => GemSet, SkillTypeSet => SkillTypes,
        indexmap::IndexMap<Name, ItemRef> => ItemList, Vec<ExplodeSource> => ExplodeList,
        PtrMap<Item, f64> => FlaskSlots, ModPtrSet => ModSet,
    );

    /// An `env.explodeSources` entry: the node or item itself.
    impl ProbeField for ExplodeSource {
        fn to_val(&self) -> Val {
            ExplodeSource::to_val(self)
        }
        fn from_val(v: &Val) -> Option<Self> {
            ExplodeSource::from_val(v)
        }
    }

    /// A name set: the data's own set (`minion.uses = grantedEffect.minionUses`) exports as its
    /// `$data` path, any other (`cfg.baseFlags`) as the table of names.
    impl ProbeField for NameSet {
        fn to_val(&self) -> Val {
            if data_index_path(arc_ptr(self)).is_some() {
                return data_val(arc_ptr(self), DataTarget::NameSet(self.clone()));
            }
            Val::Obj(Rc::new(NameSetBox(self.clone())))
        }
        fn from_val(v: &Val) -> Option<Self> {
            if let Some(d) = v.obj::<DataRef>() {
                return match &d.target {
                    DataTarget::NameSet(s) => Some(s.clone()),
                    _ => None,
                };
            }
            v.obj::<NameSetBox>().map(|b| b.0.clone())
        }
        fn kind() -> Kind {
            Kind::NameSet
        }
    }

    macro_rules! data_field {
        ($($t:ty => $v:ident),* $(,)?) => {$(
            impl ProbeField for $t {
                fn to_val(&self) -> Val {
                    data_val(arc_ptr(self), DataTarget::$v(self.clone()))
                }
                fn from_val(v: &Val) -> Option<Self> {
                    match &v.obj::<DataRef>()?.target {
                        DataTarget::$v(x) => Some(x.clone()),
                        _ => None,
                    }
                }
                fn kind() -> Kind {
                    Kind::Data
                }
            }
        )*};
    }
    data_field!(
        GrantedEffectRef => Skill, GemDataRef => Gem, SkillLevelRef => Level,
        ItemBaseRef => ItemBase, MinionDataRef => Minion,
    );

    impl ProbeField for &'static indexmap::IndexMap<Name, pob_gamedata::AilmentData> {
        fn to_val(&self) -> Val {
            Val::Obj(Rc::new(DataRef {
                path: "data.nonDamagingAilment".to_owned(),
                target: DataTarget::Other,
            }))
        }
        fn from_val(v: &Val) -> Option<Self> {
            let r = v.obj::<DataRef>()?;
            (r.path == "data.nonDamagingAilment").then(|| &pob_gamedata::data().nonDamagingAilment)
        }
        fn kind() -> Kind {
            Kind::Data
        }
    }

    impl ProbeField for Accelerate {
        fn to_val(&self) -> Val {
            let t = Table::new();
            let set = |k: &str, b: bool| {
                if b {
                    t.set(Name::from(k), true);
                }
            };
            set("nodeAlloc", self.nodeAlloc);
            set("requirementsItems", self.requirementsItems);
            set("requirementsGems", self.requirementsGems);
            set("skills", self.skills);
            set("everything", self.everything);
            Val::Table(t)
        }
        fn from_val(v: &Val) -> Option<Self> {
            let t = match v {
                Val::Nil => return Some(Accelerate::default()),
                other => other.table()?,
            };
            let b = |k: &str| t.truthy(Name::from(k));
            Some(Accelerate {
                nodeAlloc: b("nodeAlloc"),
                requirementsItems: b("requirementsItems"),
                requirementsGems: b("requirementsGems"),
                skills: b("skills"),
                everything: b("everything"),
            })
        }
    }

    impl ProbeField for SpecEnv {
        fn to_val(&self) -> Val {
            let t = Table::new();
            if let Some(x) = &self.cachedPlayerDB {
                t.set(pob_lua::n!("cachedPlayerDB"), x.to_val());
            }
            if let Some(x) = &self.cachedEnemyDB {
                t.set(pob_lua::n!("cachedEnemyDB"), x.to_val());
            }
            if let Some(x) = &self.cachedMinionDB {
                t.set(pob_lua::n!("cachedMinionDB"), x.to_val());
            }
            if let Some(x) = &self.env {
                t.set(pob_lua::n!("env"), x.to_val());
            }
            t.set(pob_lua::n!("accelerate"), self.accelerate.to_val());
            Val::Table(t)
        }
        fn from_val(v: &Val) -> Option<Self> {
            let t = v.table()?;
            Some(SpecEnv {
                cachedPlayerDB: t.get(pob_lua::n!("cachedPlayerDB")).obj::<ModStore>(),
                cachedEnemyDB: t.get(pob_lua::n!("cachedEnemyDB")).obj::<ModStore>(),
                cachedMinionDB: t.get(pob_lua::n!("cachedMinionDB")).obj::<ModStore>(),
                env: t.get(pob_lua::n!("env")).obj::<Env>(),
                accelerate: Accelerate::from_val(&t.get(pob_lua::n!("accelerate")))
                    .unwrap_or_default(),
            })
        }
    }

    // =====================================================================================
    // Slots: typed struct fields
    // =====================================================================================

    /// One typed field of a reflected struct.
    pub trait Slot {
        fn get_v(&self) -> Val;
        /// Assign from a Lua value; false when the value does not fit the type.
        fn set_v(&self, v: &Val) -> bool;
        fn kind() -> Kind
        where
            Self: Sized;
    }

    impl Slot for Cell<f64> {
        fn get_v(&self) -> Val {
            Val::Num(self.get())
        }
        fn set_v(&self, v: &Val) -> bool {
            v.opt_num().map(|n| self.set(n)).is_some()
        }
        fn kind() -> Kind {
            Kind::Scalar
        }
    }
    impl Slot for Cell<bool> {
        fn get_v(&self) -> Val {
            Val::Bool(self.get())
        }
        fn set_v(&self, v: &Val) -> bool {
            match v {
                Val::Bool(b) => {
                    self.set(*b);
                    true
                }
                _ => false,
            }
        }
        fn kind() -> Kind {
            Kind::Scalar
        }
    }
    impl Slot for Cell<u32> {
        fn get_v(&self) -> Val {
            Val::Num(f64::from(self.get()))
        }
        fn set_v(&self, v: &Val) -> bool {
            match v.opt_num() {
                Some(n) if n >= 0.0 && n <= f64::from(u32::MAX) && n.fract() == 0.0 => {
                    self.set(n as u32);
                    true
                }
                _ => false,
            }
        }
        fn kind() -> Kind {
            Kind::Scalar
        }
    }
    impl Slot for Cell<Option<f64>> {
        fn get_v(&self) -> Val {
            self.get().map_or(Val::Nil, Val::Num)
        }
        fn set_v(&self, v: &Val) -> bool {
            match v {
                Val::Nil => {
                    self.set(None);
                    true
                }
                Val::Num(n) => {
                    self.set(Some(*n));
                    true
                }
                _ => false,
            }
        }
        fn kind() -> Kind {
            Kind::Scalar
        }
    }
    impl Slot for Cell<Option<u32>> {
        fn get_v(&self) -> Val {
            self.get().map_or(Val::Nil, |n| Val::Num(f64::from(n)))
        }
        fn set_v(&self, v: &Val) -> bool {
            match v {
                Val::Nil => {
                    self.set(None);
                    true
                }
                Val::Num(n) if *n >= 0.0 && n.fract() == 0.0 && *n <= f64::from(u32::MAX) => {
                    self.set(Some(*n as u32));
                    true
                }
                _ => false,
            }
        }
        fn kind() -> Kind {
            Kind::Scalar
        }
    }
    impl Slot for Cell<Option<Name>> {
        fn get_v(&self) -> Val {
            self.get().map_or(Val::Nil, |n| n.to_val())
        }
        fn set_v(&self, v: &Val) -> bool {
            match v {
                Val::Nil => {
                    self.set(None);
                    true
                }
                Val::Str(s) => {
                    self.set(Some(Name::from(s)));
                    true
                }
                _ => false,
            }
        }
        fn kind() -> Kind {
            Kind::Scalar
        }
    }
    impl<T: ProbeField + CheapClone> Slot for Field<T> {
        fn get_v(&self) -> Val {
            self.get().to_val()
        }
        fn set_v(&self, v: &Val) -> bool {
            match T::from_val(v) {
                Some(x) => {
                    self.set(x);
                    true
                }
                None => false,
            }
        }
        fn kind() -> Kind {
            T::kind()
        }
    }
    /// An immutable `Table` handle (`env.configInput`, `store.conditions`):
    /// injection pre-binds the dumped table to it.
    impl Slot for Table {
        fn get_v(&self) -> Val {
            Val::Table(self.clone())
        }
        fn set_v(&self, v: &Val) -> bool {
            matches!(v, Val::Table(t) if t.ptr_eq(self))
        }
        fn kind() -> Kind {
            Kind::Table
        }
    }
    /// Immutable `Rc` handles (`env.player`, `env.override`, `env.build`).
    impl<T: 'static> Slot for Rc<T>
    where
        Rc<T>: ProbeField,
    {
        fn get_v(&self) -> Val {
            self.to_val()
        }
        fn set_v(&self, v: &Val) -> bool {
            v.obj::<T>().is_some_and(|x| Rc::ptr_eq(&x, self))
                || <Rc<T> as ProbeField>::kind().is_leaf()
        }
        fn kind() -> Kind {
            <Rc<T> as ProbeField>::kind()
        }
    }
    /// `minion.lifeTable`: one of the data's monster life tables, by identity (CCR-0040 §1).
    impl Slot for Cell<Option<&'static [f64]>> {
        fn get_v(&self) -> Val {
            let Some(t) = self.get() else { return Val::Nil };
            let path = life_tables()
                .into_iter()
                .find(|(_, x)| std::ptr::eq(*x, t))
                .map_or_else(|| "data.?".to_owned(), |(p, _)| p.to_owned());
            Val::Obj(Rc::new(DataRef {
                path,
                target: DataTarget::Other,
            }))
        }
        fn set_v(&self, v: &Val) -> bool {
            if v.is_nil() {
                self.set(None);
                return true;
            }
            let Some(d) = v.obj::<DataRef>() else { return false };
            match life_tables().into_iter().find(|(p, _)| *p == d.path) {
                Some((_, t)) => {
                    self.set(Some(t));
                    true
                }
                None => false,
            }
        }
        fn kind() -> Kind {
            Kind::Data
        }
    }

    /// The `data.monster*LifeTable` sequences `minion.lifeTable` can be (CalcActiveSkill.lua
    /// 724-727), by their `$data` paths.
    fn life_tables() -> Vec<(&'static str, &'static [f64])> {
        if !pob_gamedata::is_initialised() {
            return Vec::new();
        }
        let d = pob_gamedata::data();
        vec![
            ("data.monsterLifeTable", &d.monsterLifeTable[..]),
            ("data.monsterLifeTable2", &d.monsterLifeTable2[..]),
            ("data.monsterLifeTable3", &d.monsterLifeTable3[..]),
            ("data.monsterAllyLifeTable", &d.monsterAllyLifeTable[..]),
        ]
    }

    /// `tinctureLimit` (a mutable upvalue of `mergeTinctures`).
    impl ProbeField for Cell<f64> {
        fn to_val(&self) -> Val {
            Val::Num(self.get())
        }
        fn from_val(v: &Val) -> Option<Self> {
            v.opt_num().map(Cell::new)
        }
    }

    /// The local functions `mergeFlasks` / `mergeTinctures` (CalcPerform.lua 1691 / 1862): the
    /// value is the closure's upvalue struct (CCR-0079 §1), which the exporter writes as the
    /// `{"$fn": …}` marker PoB dumps ([`closure_marker`]).
    macro_rules! closure_field {
        ($($t:ty),* $(,)?) => {$(
            impl ProbeField for Rc<$t> {
                fn to_val(&self) -> Val {
                    Val::Obj(self.clone())
                }
                fn from_val(v: &Val) -> Option<Self> {
                    v.obj::<$t>()
                }
            }
        )*};
    }
    closure_field!(FlaskUpvalues, TinctureUpvalues);

    /// The `$fn` marker of a closure-upvalue struct (`None` for any other object).
    pub fn closure_marker(o: &dyn Any) -> Option<J> {
        if o.downcast_ref::<FlaskUpvalues>().is_some() {
            return Some(serde_json::json!({ "$fn": "Modules/CalcPerform.lua:1691-1849" }));
        }
        if o.downcast_ref::<TinctureUpvalues>().is_some() {
            return Some(serde_json::json!({ "$fn": "Modules/CalcPerform.lua:1862-1932" }));
        }
        None
    }

    fn kind_of<T, S: Slot>(_: impl Fn(&T) -> &S) -> Kind {
        S::kind()
    }

    /// A struct whose typed fields map to verbatim Lua keys.
    pub trait Reflect {
        const KEYS: &'static [&'static str];
        fn r_get(&self, k: &str) -> Option<Val>;
        /// `None`: no typed field `k`; `Some(ok)`.
        fn r_set(&self, k: &str, v: &Val) -> Option<bool>;
        fn r_kind(k: &str) -> Option<Kind>;
        fn r_extra(&self) -> Option<&Table>;
    }

    macro_rules! reflect {
        ($T:ty, extra: $extra:tt, { $($key:literal => $f:ident),* $(,)? }) => {
            impl Reflect for $T {
                const KEYS: &'static [&'static str] = &[$($key),*];
                fn r_get(&self, k: &str) -> Option<Val> {
                    match k {
                        $($key => Some(Slot::get_v(&self.$f)),)*
                        _ => None,
                    }
                }
                fn r_set(&self, k: &str, v: &Val) -> Option<bool> {
                    match k {
                        $($key => Some(Slot::set_v(&self.$f, v)),)*
                        _ => None,
                    }
                }
                fn r_kind(k: &str) -> Option<Kind> {
                    match k {
                        $($key => Some(kind_of(|x: &$T| &x.$f)),)*
                        _ => None,
                    }
                }
                reflect!(@extra $extra);
            }
        };
        (@extra none) => {
            fn r_extra(&self) -> Option<&Table> { None }
        };
        (@extra extra) => {
            fn r_extra(&self) -> Option<&Table> { Some(&self.extra) }
        };
    }

    reflect!(Env, extra: extra, {
        "build" => build, "mode_buffs" => mode_buffs, "mode_combat" => mode_combat,
        "mode_effective" => mode_effective, "buildBreakdown" => buildBreakdown,
        "configInput" => configInput, "configPlaceholder" => configPlaceholder,
        "calcsInput" => calcsInput, "spec" => spec, "override" => r#override,
        "classId" => classId, "modDB" => modDB, "enemyDB" => enemyDB, "itemModDB" => itemModDB,
        "initialNodeModDB" => initialNodeModDB, "aegisModList" => aegisModList,
        "weaponModList1" => weaponModList1, "theIronMass" => theIronMass,
        "enemyLevel" => enemyLevel, "player" => player, "enemy" => enemy, "minion" => minion,
        "partyMembers" => partyMembers, "allocNodes" => allocNodes,
        "grantedPassives" => grantedPassives, "grantedSkills" => grantedSkills,
        "grantedSkillsNodes" => grantedSkillsNodes, "grantedSkillsItems" => grantedSkillsItems,
        "radiusJewelList" => radiusJewelList, "extraRadiusNodeList" => extraRadiusNodeList,
        "flasks" => flasks, "tinctures" => tinctures, "flaskSlotOccupied" => flaskSlotOccupied,
        "flaskSlotMap" => flaskSlotMap, "itemWarnings" => itemWarnings,
        "requirementsTable" => requirementsTable, "requirementsTableItems" => requirementsTableItems,
        "requirementsTableGems" => requirementsTableGems, "auxSkillList" => auxSkillList,
        "limitedSkills" => limitedSkills, "mainSocketGroup" => mainSocketGroup,
        "crossLinkedSupportGroups" => crossLinkedSupportGroups, "keystonesAdded" => keystonesAdded,
        "explodeSources" => explodeSources, "aegisItem" => aegisItem, "buffs" => buffs,
        "minionBuffs" => minionBuffs, "debuffs" => debuffs, "curseSlots" => curseSlots,
        "skillsUsed" => skillsUsed, "conditionsUsed" => conditionsUsed,
        "enemyConditionsUsed" => enemyConditionsUsed, "minionConditionsUsed" => minionConditionsUsed,
        "multipliersUsed" => multipliersUsed, "enemyMultipliersUsed" => enemyMultipliersUsed,
        "perStatsUsed" => perStatsUsed, "enemyPerStatsUsed" => enemyPerStatsUsed,
        "tagTypesUsed" => tagTypesUsed, "modsUsed" => modsUsed,
    });

    reflect!(Actor, extra: extra, {
        "modDB" => modDB, "level" => level, "output" => output, "breakdown" => breakdown,
        "itemList" => itemList, "weaponData1" => weaponData1, "weaponData2" => weaponData2,
        "weaponRange1" => weaponRange1, "weaponRange2" => weaponRange2, "mainSkill" => mainSkill,
        "activeSkillList" => activeSkillList, "enemy" => enemy, "parent" => parent,
        "player" => player, "minion" => minion, "partyMembers" => partyMembers,
        "minionData" => minionData, "damageShiftTable" => damageShiftTable,
        "damageOverTimeShiftTable" => damageOverTimeShiftTable, "allyLifeList" => allyLifeList,
        "strDmgBonus" => strDmgBonus, "hostile" => hostile,
        "appliedEnemyModifiers" => appliedEnemyModifiers, "infoTrigger" => infoTrigger,
        "itemSetId" => itemSetId, "uses" => uses, "type" => r#type, "lifeTable" => lifeTable,
        "itemSet" => itemSet,
    });

    reflect!(ActiveSkill, extra: extra, {
        "activeEffect" => activeEffect, "supportList" => supportList, "effectList" => effectList,
        "actor" => actor, "summonSkill" => summonSkill, "socketGroup" => socketGroup,
        "skillTypes" => skillTypes, "minionSkillTypes" => minionSkillTypes,
        "skillFlags" => skillFlags, "skillData" => skillData, "skillModList" => skillModList,
        "baseSkillModList" => baseSkillModList, "extraSkillModList" => extraSkillModList,
        "skillCfg" => skillCfg, "weapon1Cfg" => weapon1Cfg, "weapon2Cfg" => weapon2Cfg,
        "weapon1Flags" => weapon1Flags, "weapon2Flags" => weapon2Flags, "bleedCfg" => bleedCfg,
        "poisonCfg" => poisonCfg, "igniteCfg" => igniteCfg, "OHbleedCfg" => OHbleedCfg,
        "OHpoisonCfg" => OHpoisonCfg, "OHigniteCfg" => OHigniteCfg, "dotCfg" => dotCfg,
        "decayCfg" => decayCfg, "buffList" => buffList, "minion" => minion,
        "minionList" => minionList, "mirage" => mirage, "skillPart" => skillPart,
        "skillPartName" => skillPartName, "disableReason" => disableReason,
        "infoMessage" => infoMessage, "infoMessage2" => infoMessage2, "infoTrigger" => infoTrigger,
        "conversionTable" => conversionTable, "triggeredBy" => triggeredBy,
        "slotName" => slotName, "skillTotemId" => skillTotemId, "buffSkill" => buffSkill,
        "debuffSkill" => debuffSkill, "totemBuffSkill" => totemBuffSkill,
        "minionBuffSkill" => minionBuffSkill, "activeMineCount" => activeMineCount,
        "activeStageCount" => activeStageCount,
    });

    reflect!(SkillEffect, extra: extra, {
        "grantedEffect" => grantedEffect, "level" => level, "quality" => quality,
        "srcInstance" => srcInstance, "gemData" => gemData, "gemCfg" => gemCfg,
        "grantedEffectLevel" => grantedEffectLevel, "isSupporting" => isSupporting,
        "superseded" => superseded, "appliesToGrantedSkills" => appliesToGrantedSkills,
        "fromItem" => fromItem, "actorLevel" => actorLevel, "globalQuality" => globalQuality,
        "itemQuality" => itemQuality, "supportQuality" => supportQuality, "req" => req,
        "gemPropertyInfo" => gemPropertyInfo,
    });

    reflect!(ModCfg, extra: extra, {
        "flags" => flags, "keywordFlags" => keywordFlags, "source" => source, "actor" => actor,
        "skillName" => skillName, "summonSkillName" => summonSkillName, "skillGem" => skillGem,
        "skillGrantedEffect" => skillGrantedEffect, "skillPart" => skillPart,
        "skillTypes" => skillTypes, "skillCond" => skillCond, "skillDist" => skillDist,
        "skillStats" => skillStats, "slotName" => slotName, "socketColor" => socketColor,
        "socketNum" => socketNum, "strengthGems" => strengthGems,
        "dexterityGems" => dexterityGems, "intelligenceGems" => intelligenceGems,
        "item" => item, "conversionTable" => conversionTable, "baseFlags" => baseFlags,
    });

    reflect!(GemInstance, extra: extra, {
        "nameSpec" => nameSpec, "skillId" => skillId, "gemId" => gemId, "level" => level,
        "quality" => quality, "enabled" => enabled, "enableGlobal1" => enableGlobal1,
        "enableGlobal2" => enableGlobal2, "count" => count, "gemData" => gemData,
        "grantedEffect" => grantedEffect, "skillPart" => skillPart,
        "skillPartCalcs" => skillPartCalcs, "skillStageCount" => skillStageCount,
        "skillStageCountCalcs" => skillStageCountCalcs, "skillMineCount" => skillMineCount,
        "skillMineCountCalcs" => skillMineCountCalcs, "skillMinion" => skillMinion,
        "skillMinionCalcs" => skillMinionCalcs, "skillMinionItemSet" => skillMinionItemSet,
        "skillMinionItemSetCalcs" => skillMinionItemSetCalcs,
        "skillMinionSkill" => skillMinionSkill, "skillMinionSkillCalcs" => skillMinionSkillCalcs,
        "fromItem" => fromItem, "noSupports" => noSupports, "triggered" => triggered,
        "displayEffect" => displayEffect, "supportEffect" => supportEffect,
        "reqLevel" => reqLevel,
    });

    reflect!(SocketGroup, extra: extra, {
        "label" => label, "enabled" => enabled, "slotEnabled" => slotEnabled, "slot" => slot,
        "source" => source, "includeInFullDPS" => includeInFullDPS, "groupCount" => groupCount,
        "gemList" => gemList, "mainActiveSkill" => mainActiveSkill,
        "mainActiveSkillCalcs" => mainActiveSkillCalcs, "noSupports" => noSupports,
        "displaySkillList" => displaySkillList, "displaySkillListCalcs" => displaySkillListCalcs,
        "displayLabel" => displayLabel, "explodeSources" => explodeSources,
        "sourceItem" => sourceItem,
    });

    reflect!(Override, extra: extra, {
        "spec" => spec, "addNodes" => addNodes, "removeNodes" => removeNodes,
        "repSlotName" => repSlotName, "repItem" => repItem, "toggleFlask" => toggleFlask,
        "toggleTincture" => toggleTincture, "conditions" => conditions,
        "extraJewelFuncs" => extraJewelFuncs,
    });

    /// The item fields `state_dump.lua` writes (its `itemFields`).
    pub const ITEM_FIELDS: &[&str] = &[
        "armourData",
        "base",
        "baseName",
        "corrupted",
        "flaskData",
        "id",
        "jewelData",
        "jewelRadiusIndex",
        "limit",
        "modList",
        "name",
        "quality",
        "rarity",
        "requirements",
        "slotModList",
        "tinctureData",
        "title",
        "type",
        "variant",
        "weaponData",
    ];

    /// The breakdown module's closures (Modules/CalcBreakdown.lua at the pin),
    /// fields of every breakdown table.
    pub const BREAKDOWN_FNS: &[(&str, &str)] = &[
        ("area", "Modules/CalcBreakdown.lua:95-112"),
        ("critDot", "Modules/CalcBreakdown.lua:175-196"),
        ("dot", "Modules/CalcBreakdown.lua:162-173"),
        ("effMult", "Modules/CalcBreakdown.lua:114-160"),
        ("leech", "Modules/CalcBreakdown.lua:198-252"),
        ("mod", "Modules/CalcBreakdown.lua:69-79"),
        ("multiChain", "Modules/CalcBreakdown.lua:17-42"),
        ("simple", "Modules/CalcBreakdown.lua:44-67"),
        ("slot", "Modules/CalcBreakdown.lua:81-93"),
    ];

    /// The `skillCond` `__index` closures of the ailment cfgs (CalcOffence.lua 4422/4695/5000).
    fn orchain_site(cfg_key: &str) -> &'static str {
        match cfg_key {
            "poisonCfg" | "OHpoisonCfg" => "Modules/CalcOffence.lua:4695-4695",
            "igniteCfg" | "OHigniteCfg" => "Modules/CalcOffence.lua:5000-5000",
            _ => "Modules/CalcOffence.lua:4422-4422",
        }
    }

    /// The expected kind of member `key` of an object of kind `k`.
    pub fn member_kind(k: Kind, key: &str) -> Kind {
        let typed = match k {
            Kind::Env => Env::r_kind(key),
            Kind::Actor => Actor::r_kind(key),
            Kind::Skill => ActiveSkill::r_kind(key),
            Kind::Effect => SkillEffect::r_kind(key),
            Kind::Cfg => ModCfg::r_kind(key),
            Kind::Gem => GemInstance::r_kind(key),
            Kind::Group => SocketGroup::r_kind(key),
            Kind::Override => Override::r_kind(key),
            Kind::Store => match key {
                "actor" => Some(Kind::Actor),
                "parent" => Some(Kind::Store),
                "conditions" | "multipliers" => Some(Kind::Table),
                _ => None,
            },
            Kind::Item => match key {
                "modList" => Some(Kind::ItemMods),
                "slotModList" => Some(Kind::SlotMods),
                "base" => Some(Kind::Data),
                "requirements" | "weaponData" | "armourData" | "flaskData" | "tinctureData"
                | "jewelData" => Some(Kind::Table),
                _ => None,
            },
            _ => None,
        };
        typed.unwrap_or(Kind::Any)
    }

    /// The expected kind of the elements of a list / map kind.
    pub fn element_kind(k: Kind) -> Kind {
        match k {
            Kind::SkillList => Kind::Skill,
            Kind::EffectList => Kind::Effect,
            Kind::TableList => Kind::Table,
            Kind::GemList => Kind::Gem,
            Kind::NodeMap => Kind::Node,
            Kind::ItemList => Kind::Item,
            Kind::SlotMods => Kind::ItemMods,
            _ => Kind::Any,
        }
    }

    /// The expected kind of the keys of an object-keyed map kind.
    pub fn key_kind(k: Kind) -> Kind {
        match k {
            Kind::ItemFlags | Kind::FlaskSlots => Kind::Item,
            Kind::GemSet => Kind::Gem,
            _ => Kind::Any,
        }
    }

    // =====================================================================================
    // PObj: identity objects of the export graph
    // =====================================================================================

    /// The synthetic tables of a document: the root and `locals`.
    pub struct Synth {
        pub members: Vec<(String, Val)>,
    }

    /// An identity table of the export graph.
    #[derive(Clone)]
    pub enum PObj {
        Env(EnvRef),
        Actor(ActorRef),
        Skill(ActiveSkillRef),
        Effect(SkillEffectRef),
        Cfg(Cfg),
        Store(ModStoreRef),
        Item(ItemRef),
        Gem(GemInstanceRef),
        Group(SocketGroupRef),
        Override(Rc<Override>),
        Breakdown(Breakdown),
        Table(Table),
        /// A table's metatable `{ __index = … }`; the string is the owning cfg
        /// field (for the closure site of an OrChain layer).
        Meta(Table, IndexMeta, Option<&'static str>),
        SkillTypes(SkillTypes),
        NameSet(NameSet),
        SkillList(Shared<Vec<ActiveSkillRef>>),
        EffectList(Shared<Vec<SkillEffectRef>>),
        TableList(Shared<Vec<Table>>),
        NameList(Shared<Vec<Name>>),
        ModVec(Shared<Vec<ModRef>>),
        GemList(Shared<Vec<GemInstanceRef>>),
        NodeMap(Shared<LjIntMap<NodeRef>>),
        BoolMap(Shared<LjIntMap<bool>>),
        ItemFlags(Shared<PtrMap<Item, bool>>),
        GemSet(Shared<PtrSet<GemInstance>>),
        ItemList(Shared<indexmap::IndexMap<Name, ItemRef>>),
        ExplodeList(Shared<Vec<ExplodeSource>>),
        FlaskSlots(Shared<PtrMap<Item, f64>>),
        ModSet(Shared<ModPtrSet>),
        ItemMods(ModStoreRef),
        SlotMods(ItemRef),
        GlobalCache,
        CacheData,
        CacheMode(Mode),
        CacheEntry(Rc<CachedData>),
        Synth(Rc<Synth>),
    }

    /// The identity of a `Shared` (the address of its value inside the shared cell).
    fn sptr<T>(s: &Shared<T>) -> usize {
        s.with(|x| std::ptr::from_ref(x).cast::<()>() as usize)
    }

    impl PObj {
        /// The identity key (variant tag, address).
        pub fn key(&self) -> (u8, usize) {
            match self {
                PObj::Env(x) => (1, rc_ptr(x)),
                PObj::Actor(x) => (2, rc_ptr(x)),
                PObj::Skill(x) => (3, rc_ptr(x)),
                PObj::Effect(x) => (4, rc_ptr(x)),
                PObj::Cfg(x) => (5, rc_ptr(x)),
                PObj::Store(x) => (6, rc_ptr(x)),
                PObj::Item(x) => (7, rc_ptr(x)),
                PObj::Gem(x) => (8, rc_ptr(x)),
                PObj::Group(x) => (9, rc_ptr(x)),
                PObj::Override(x) => (10, rc_ptr(x)),
                PObj::Breakdown(b) => (11, b.t.as_ptr() as usize),
                PObj::Table(t) => (11, t.as_ptr() as usize),
                PObj::Meta(t, _, _) => (12, t.as_ptr() as usize),
                PObj::SkillTypes(s) => (13, sptr(s)),
                PObj::NameSet(s) => (14, arc_ptr(s)),
                PObj::SkillList(s) => (15, sptr(s)),
                PObj::EffectList(s) => (16, sptr(s)),
                PObj::TableList(s) => (17, sptr(s)),
                PObj::NameList(s) => (18, sptr(s)),
                PObj::ModVec(s) => (19, sptr(s)),
                PObj::GemList(s) => (20, sptr(s)),
                PObj::NodeMap(s) => (21, sptr(s)),
                PObj::BoolMap(s) => (22, sptr(s)),
                PObj::ItemFlags(s) => (23, sptr(s)),
                PObj::GemSet(s) => (24, sptr(s)),
                PObj::ItemList(s) => (25, sptr(s)),
                PObj::ItemMods(s) => (26, rc_ptr(s)),
                PObj::SlotMods(i) => (27, rc_ptr(i)),
                PObj::GlobalCache => (28, 0),
                PObj::CacheData => (29, 0),
                PObj::CacheMode(m) => (30, m.index()),
                PObj::CacheEntry(e) => (31, rc_ptr(e)),
                PObj::Synth(s) => (32, rc_ptr(s)),
                PObj::ExplodeList(s) => (33, sptr(s)),
                PObj::FlaskSlots(s) => (34, sptr(s)),
                PObj::ModSet(s) => (35, sptr(s)),
            }
        }

        /// The address the registry knows this object by.
        pub fn reg_ptr(&self) -> usize {
            self.key().1
        }

        /// A dynamic-table value of a typed object (`None` for leaves and scalars).
        pub fn from_val(v: &Val) -> Option<PObj> {
            match v {
                Val::Table(t) => Some(PObj::Table(t.clone())),
                Val::Obj(o) => {
                    macro_rules! try_rc {
                        ($($t:ty => $v:ident),*) => {$(
                            if let Ok(x) = o.clone().downcast::<$t>() {
                                return Some(PObj::$v(x));
                            }
                        )*};
                    }
                    try_rc!(Env => Env, Actor => Actor, ActiveSkill => Skill, SkillEffect => Effect,
                        ModCfg => Cfg, ModStore => Store, Item => Item, GemInstance => Gem,
                        SocketGroup => Group, Override => Override, CachedData => CacheEntry);
                    if let Ok(b) = o.clone().downcast::<Breakdown>() {
                        return Some(PObj::Breakdown((*b).clone()));
                    }
                    macro_rules! try_box {
                        ($($t:ty => $v:ident),*) => {$(
                            if let Some(b) = o.downcast_ref::<SBox<$t>>() {
                                return Some(PObj::$v(b.0.clone()));
                            }
                        )*};
                    }
                    try_box!(Vec<ActiveSkillRef> => SkillList, Vec<SkillEffectRef> => EffectList,
                        Vec<Table> => TableList, Vec<Name> => NameList, Vec<ModRef> => ModVec,
                        Vec<GemInstanceRef> => GemList, LjIntMap<NodeRef> => NodeMap,
                        LjIntMap<bool> => BoolMap, PtrMap<Item, bool> => ItemFlags,
                        PtrSet<GemInstance> => GemSet, SkillTypeSet => SkillTypes,
                        indexmap::IndexMap<Name, ItemRef> => ItemList,
                        Vec<ExplodeSource> => ExplodeList, PtrMap<Item, f64> => FlaskSlots,
                        ModPtrSet => ModSet);
                    if let Some(b) = o.downcast_ref::<NameSetBox>() {
                        return Some(PObj::NameSet(b.0.clone()));
                    }
                    None
                }
                _ => None,
            }
        }

        /// The object as a dynamic-table value.
        pub fn to_val(&self) -> Val {
            match self {
                PObj::Env(x) => Val::Obj(x.clone()),
                PObj::Actor(x) => Val::Obj(x.clone()),
                PObj::Skill(x) => Val::Obj(x.clone()),
                PObj::Effect(x) => Val::Obj(x.clone()),
                PObj::Cfg(x) => Val::Obj(x.clone()),
                PObj::Store(x) | PObj::ItemMods(x) => Val::Obj(x.clone()),
                PObj::Item(x) | PObj::SlotMods(x) => Val::Obj(x.clone()),
                PObj::Gem(x) => Val::Obj(x.clone()),
                PObj::Group(x) => Val::Obj(x.clone()),
                PObj::Override(x) => Val::Obj(x.clone()),
                PObj::Breakdown(b) => b.to_val(),
                PObj::Table(t) | PObj::Meta(t, _, _) => Val::Table(t.clone()),
                PObj::SkillTypes(s) => s.to_val(),
                PObj::NameSet(s) => s.to_val(),
                PObj::SkillList(s) => s.to_val(),
                PObj::EffectList(s) => s.to_val(),
                PObj::TableList(s) => s.to_val(),
                PObj::NameList(s) => s.to_val(),
                PObj::ModVec(s) => s.to_val(),
                PObj::GemList(s) => s.to_val(),
                PObj::NodeMap(s) => s.to_val(),
                PObj::BoolMap(s) => s.to_val(),
                PObj::ItemFlags(s) => s.to_val(),
                PObj::GemSet(s) => s.to_val(),
                PObj::ItemList(s) => s.to_val(),
                PObj::ExplodeList(s) => s.to_val(),
                PObj::FlaskSlots(s) => s.to_val(),
                PObj::ModSet(s) => s.to_val(),
                PObj::CacheEntry(e) => Val::Obj(e.clone()),
                PObj::GlobalCache | PObj::CacheData | PObj::CacheMode(_) | PObj::Synth(_) => {
                    Val::Nil
                }
            }
        }

        /// The Lua class of a class instance (`$keys` labels).
        fn class(&self) -> Option<&'static str> {
            match self {
                PObj::Store(s) => Some(match s.kind {
                    StoreKind::ModDB => "ModDB",
                    StoreKind::ModList => "ModList",
                }),
                PObj::Item(_) => Some("Item"),
                _ => None,
            }
        }
    }

    // =====================================================================================
    // Members
    // =====================================================================================

    /// A member key.
    #[derive(Clone)]
    pub enum PKey {
        S(String),
        N(f64),
        /// a boolean or table key (`$keys`)
        O(PV),
    }

    /// A member value: a Lua value or a synthetic identity table.
    #[derive(Clone)]
    pub enum PV {
        V(Val),
        O(PObj),
    }

    impl PV {
        fn is_nil(&self) -> bool {
            matches!(self, PV::V(Val::Nil))
        }
    }

    /// The members of a table, before ordering.
    #[derive(Default)]
    pub struct Members {
        pub entries: Vec<(PKey, PV)>,
        pub meta: Option<PV>,
    }

    impl Members {
        fn s(&mut self, k: &str, v: Val) {
            if !v.is_nil() {
                self.entries.push((PKey::S(k.to_owned()), PV::V(v)));
            }
        }
        fn so(&mut self, k: &str, o: PObj) {
            self.entries.push((PKey::S(k.to_owned()), PV::O(o)));
        }
        fn n(&mut self, i: f64, v: Val) {
            if !v.is_nil() {
                self.entries.push((PKey::N(i), PV::V(v)));
            }
        }
        fn has_s(&self, k: &str) -> bool {
            self.entries
                .iter()
                .any(|(x, _)| matches!(x, PKey::S(s) if s == k))
        }
        fn table(&mut self, t: &Table) {
            for (k, v) in t.pairs() {
                match k {
                    Key::S(n) => self.s(n.as_str(), v),
                    Key::I(i) => self.n(i as f64, v),
                    Key::B(b) => self.entries.push((PKey::O(PV::V(Val::Bool(b))), PV::V(v))),
                }
            }
            for (k, v) in REG.with(|r| r.borrow().obj_keys(t)) {
                if !v.is_nil() {
                    self.entries.push((PKey::O(PV::V(k)), PV::V(v)));
                }
            }
        }
    }

    /// Typed fields (minus the nil ones and those nil in Lua), then `extra`.
    fn struct_members<T: Reflect>(m: &mut Members, x: &T, ptr: usize) {
        for k in T::KEYS {
            if let Some(v) = x.r_get(k) {
                if v.is_nil() || (reg_absent(ptr, k) && val_is_default(&v)) {
                    continue;
                }
                m.s(k, v);
            }
        }
        if let Some(e) = x.r_extra() {
            let mut em = Members::default();
            em.table(e);
            for (k, v) in em.entries {
                if let PKey::S(s) = &k {
                    if m.has_s(s) {
                        continue;
                    }
                }
                m.entries.push((k, v));
            }
        }
        side_extra(m, ptr);
    }

    fn side_extra(m: &mut Members, ptr: usize) {
        let extra = REG.with(|r| r.borrow().side_extra.get(&ptr).cloned());
        for (k, v) in extra.unwrap_or_default() {
            if !m.has_s(&k) {
                m.s(&k, v);
            }
        }
    }

    /// A default value (0, false, empty table): what a typed field holds when
    /// Lua never set it.
    pub fn val_is_default(v: &Val) -> bool {
        match v {
            Val::Nil => true,
            Val::Bool(b) => !b,
            Val::Num(n) => *n == 0.0,
            Val::Str(s) => s.is_empty(),
            Val::Table(t) => t.is_empty() && t.meta().is_none(),
            Val::Obj(o) => {
                if let Some(p) = PObj::from_val(v) {
                    return match p {
                        PObj::SkillList(s) => s.is_empty(),
                        PObj::EffectList(s) => s.is_empty(),
                        PObj::TableList(s) => s.is_empty(),
                        PObj::NameList(s) => s.is_empty(),
                        PObj::ModVec(s) => s.is_empty(),
                        PObj::GemList(s) => s.is_empty(),
                        PObj::NodeMap(s) => s.with(|m| m.keys().is_empty()),
                        PObj::BoolMap(s) => s.with(|m| m.keys().is_empty()),
                        PObj::ItemFlags(s) => s.with(PtrMap::is_empty),
                        PObj::GemSet(s) => s.with(PtrSet::is_empty),
                        PObj::ItemList(s) => s.with(indexmap::IndexMap::is_empty),
                        PObj::ExplodeList(s) => s.is_empty(),
                        PObj::FlaskSlots(s) => s.with(PtrMap::is_empty),
                        PObj::ModSet(s) => s.with(ModPtrSet::is_empty),
                        PObj::SkillTypes(s) => s.with(|x| x.iter().next().is_none()),
                        PObj::Store(s) => s.all_mods().is_empty(),
                        // a `ModCfg::default()` an `ActiveSkill::new` put in a
                        // skill PoB had not given a cfg yet
                        PObj::Cfg(c) => {
                            let mut m = Members::default();
                            struct_members(&mut m, &*c, rc_ptr(&c));
                            m.entries.iter().all(|(_, v)| match v {
                                PV::V(x) => !matches!(x, Val::Obj(_) | Val::Table(_)) && val_is_default(x),
                                PV::O(_) => false,
                            })
                        }
                        _ => false,
                    };
                }
                let _ = o;
                false
            }
            _ => false,
        }
    }

    fn members_of(o: &PObj, owners: &HashMap<usize, &'static str>) -> Members {
        let mut m = Members::default();
        match o {
            PObj::Env(e) => {
                let p = rc_ptr(e);
                m.s("mode", Val::Str(LStr::from(e.mode.as_str())));
                m.s(
                    "data",
                    Val::Obj(Rc::new(DataRef {
                        path: "data".to_owned(),
                        target: DataTarget::Root,
                    })),
                );
                struct_members(&mut m, &**e, p);
            }
            PObj::Actor(a) => struct_members(&mut m, &**a, rc_ptr(a)),
            PObj::Skill(s) => struct_members(&mut m, &**s, rc_ptr(s)),
            PObj::Effect(s) => struct_members(&mut m, &**s, rc_ptr(s)),
            PObj::Cfg(c) => struct_members(&mut m, &**c, rc_ptr(c)),
            PObj::Gem(g) => struct_members(&mut m, &**g, rc_ptr(g)),
            PObj::Group(g) => struct_members(&mut m, &**g, rc_ptr(g)),
            PObj::Override(x) => struct_members(&mut m, &**x, rc_ptr(x)),
            PObj::Store(s) => {
                let p = rc_ptr(s);
                if !(reg_absent(p, "actor")) {
                    match s.actor.get().upgrade() {
                        Some(a) => m.s("actor", Val::Obj(a)),
                        // `self.actor = parent and parent.actor or { }` (ModStore.lua:42)
                        None => m.s("actor", Val::Table(Table::new())),
                    }
                }
                m.s("conditions", Val::Table(s.conditions.clone()));
                m.s("multipliers", Val::Table(s.multipliers.clone()));
                if !reg_absent(p, "parent") {
                    match s.parent.get() {
                        Some(x) => m.s("parent", Val::Obj(x)),
                        // `self.parent = parent or false`
                        None => m.s("parent", Val::Bool(false)),
                    }
                }
                if s.notBuff.get() {
                    m.s("notBuff", Val::Bool(true));
                }
                side_extra(&mut m, p);
            }
            PObj::Item(it) => {
                let p = rc_ptr(it);
                let opt_s = |x: Option<LStr>| x.map_or(Val::Nil, Val::Str);
                m.s("id", it.id.get().map_or(Val::Nil, Val::Num));
                m.s("name", Val::Str(it.name.get()));
                m.s("title", opt_s(it.title.get()));
                m.s("baseName", it.baseName.get().to_val());
                m.s("type", it.r#type.get().to_val());
                m.s("rarity", it.rarity.get().to_val());
                m.s("quality", it.quality.get().map_or(Val::Nil, Val::Num));
                if it.corrupted.get() || !reg_absent(p, "corrupted") {
                    m.s("corrupted", Val::Bool(it.corrupted.get()));
                }
                m.s("base", it.base.get().to_val());
                m.s("variant", it.variant.get().map_or(Val::Nil, Val::Num));
                m.s(
                    "jewelRadiusIndex",
                    it.jewelRadiusIndex.get().map_or(Val::Nil, Val::Num),
                );
                m.s("limit", it.limit.get().map_or(Val::Nil, Val::Num));
                if !(reg_absent(p, "requirements") && it.requirements.is_empty()) {
                    m.s("requirements", Val::Table(it.requirements.clone()));
                }
                if let Some(l) = it.modList.get() {
                    m.so("modList", PObj::ItemMods(l));
                }
                if it.slotModList.iter().any(|l| l.get().is_some()) {
                    m.so("slotModList", PObj::SlotMods(it.clone()));
                }
                m.s("weaponData", it.weaponData.get().to_val());
                m.s("armourData", it.armourData.get().to_val());
                m.s("flaskData", it.flaskData.get().to_val());
                m.s("tinctureData", it.tinctureData.get().to_val());
                m.s("jewelData", it.jewelData.get().to_val());
            }
            PObj::Breakdown(b) => {
                m.table(&b.t);
                for (k, src) in BREAKDOWN_FNS {
                    if !m.has_s(k) {
                        m.s(
                            k,
                            Val::Obj(Rc::new(Marker(serde_json::json!({ "$fn": src })))),
                        );
                    }
                }
            }
            PObj::Table(t) => {
                m.table(t);
                if let Some(meta) = t.meta() {
                    let owner = owners.get(&(t.as_ptr() as usize)).copied();
                    m.meta = Some(PV::O(PObj::Meta(t.clone(), meta, owner)));
                }
            }
            PObj::Meta(t, meta, owner) => {
                let v = match meta {
                    IndexMeta::Table(p) => Val::Table(p.clone()),
                    IndexMeta::OrChain(_) => {
                        let marker = REG
                            .with(|r| r.borrow().meta_fn.get(&(t.as_ptr() as usize)).cloned())
                            .unwrap_or_else(|| {
                                serde_json::json!({
                                    "$fn": orchain_site(owner.unwrap_or("")),
                                    "upnames": ["skillCfg", "cfg"],
                                })
                            });
                        Val::Obj(Rc::new(Marker(marker)))
                    }
                };
                m.s("__index", v);
            }
            PObj::SkillTypes(s) => {
                for t in s.with(|x| x.iter().collect::<Vec<_>>()) {
                    m.n(f64::from(t), Val::Bool(true));
                }
            }
            PObj::NameSet(s) => {
                let mut names: Vec<&Name> = s.iter().collect();
                names.sort_by(|a, b| a.as_str().cmp(b.as_str()));
                for n in names {
                    m.s(n.as_str(), Val::Bool(true));
                }
            }
            PObj::SkillList(s) => list_members(&mut m, &s.snapshot()),
            PObj::EffectList(s) => list_members(&mut m, &s.snapshot()),
            PObj::TableList(s) => list_members(&mut m, &s.snapshot()),
            PObj::NameList(s) => list_members(&mut m, &s.snapshot()),
            PObj::ModVec(s) => list_members(&mut m, &s.snapshot()),
            PObj::GemList(s) => list_members(&mut m, &s.snapshot()),
            PObj::NodeMap(s) => {
                for (k, v) in s.with(LjIntMap::pairs) {
                    m.n(k as f64, v.to_val());
                }
            }
            PObj::BoolMap(s) => {
                for (k, v) in s.with(LjIntMap::pairs) {
                    m.n(k as f64, Val::Bool(v));
                }
            }
            PObj::ItemFlags(s) => {
                for (k, v) in s.with(PtrMap::pairs) {
                    m.entries
                        .push((PKey::O(PV::V(k.to_val())), PV::V(Val::Bool(v))));
                }
            }
            PObj::GemSet(s) => {
                for k in s.with(PtrSet::keys) {
                    m.entries
                        .push((PKey::O(PV::V(k.to_val())), PV::V(Val::Bool(true))));
                }
            }
            PObj::ItemList(s) => {
                for (k, v) in s.with(|x| x.clone()) {
                    m.s(k.as_str(), v.to_val());
                }
            }
            PObj::ExplodeList(s) => list_members(&mut m, &s.snapshot()),
            PObj::FlaskSlots(s) => {
                for (k, v) in s.with(PtrMap::pairs) {
                    m.entries
                        .push((PKey::O(PV::V(k.to_val())), PV::V(Val::Num(v))));
                }
            }
            PObj::ModSet(s) => {
                for k in s.with(ModPtrSet::keys) {
                    m.entries
                        .push((PKey::O(PV::V(Val::Mod(k))), PV::V(Val::Bool(true))));
                }
            }
            PObj::ItemMods(s) => {
                for (i, x) in s.all_mods().into_iter().enumerate() {
                    m.n((i + 1) as f64, Val::Mod(x));
                }
            }
            PObj::SlotMods(it) => {
                for (i, slot) in it.slotModList.iter().enumerate() {
                    if let Some(l) = slot.get() {
                        m.entries
                            .push((PKey::N((i + 1) as f64), PV::O(PObj::ItemMods(l))));
                    }
                }
            }
            PObj::GlobalCache => m.so("cachedData", PObj::CacheData),
            PObj::CacheData => {
                for mode in [Mode::MAIN, Mode::CALCS, Mode::CALCULATOR] {
                    m.so(mode.as_str(), PObj::CacheMode(mode));
                }
            }
            PObj::CacheMode(mode) => {
                let entries =
                    GlobalCache.with(|g| g.borrow().cachedData[mode.index()].clone());
                for (k, e) in entries {
                    m.s(k.as_str(), Val::Obj(e));
                }
            }
            PObj::CacheEntry(e) => {
                m.s("Name", e.Name.to_val());
                for (k, v) in [
                    ("Speed", &e.Speed),
                    ("HitSpeed", &e.HitSpeed),
                    ("ManaCost", &e.ManaCost),
                    ("LifeCost", &e.LifeCost),
                    ("ESCost", &e.ESCost),
                    ("RageCost", &e.RageCost),
                    ("HitChance", &e.HitChance),
                    ("AccuracyHitChance", &e.AccuracyHitChance),
                    ("PreEffectiveCritChance", &e.PreEffectiveCritChance),
                    ("CritChance", &e.CritChance),
                    ("TotalDPS", &e.TotalDPS),
                ] {
                    m.s(k, v.clone());
                }
                m.s("ActiveSkill", Val::Obj(e.ActiveSkill.clone()));
                m.s("Env", Val::Obj(e.Env.clone()));
            }
            PObj::Synth(s) => {
                for (k, v) in &s.members {
                    if k == "globalCache" {
                        m.so(k, PObj::GlobalCache);
                    } else {
                        m.s(k, v.clone());
                    }
                }
            }
        }
        m
    }

    fn list_members<T: ProbeField>(m: &mut Members, items: &[T]) {
        for (i, x) in items.iter().enumerate() {
            m.n((i + 1) as f64, x.to_val());
        }
    }

    // =====================================================================================
    // Leaves
    // =====================================================================================

    /// A JSON number as state_dump.lua's `jnum` writes it.
    pub fn jnum(n: f64) -> J {
        if n.is_nan() {
            return serde_json::json!({"$float": "nan"});
        }
        if n.is_infinite() {
            return serde_json::json!({"$float": if n > 0.0 { "inf" } else { "-inf" }});
        }
        if n == 0.0 {
            if n.is_sign_negative() {
                return serde_json::json!({"$float": "-0"});
            }
            return J::from(0);
        }
        if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 {
            return J::from(n as i64);
        }
        serde_json::Number::from_f64(n).map_or(J::Null, J::Number)
    }

    /// A number key's canonical text (`keyText`).
    pub fn key_text(n: f64) -> String {
        if n.is_nan() {
            return "nan".into();
        }
        if n.is_infinite() {
            return if n > 0.0 { "inf" } else { "-inf" }.into();
        }
        if n == 0.0 {
            return "0".into();
        }
        if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 {
            return format!("{}", n as i64);
        }
        lua_g(n)
    }

    /// `%.15g` / `%.16g` / `%.17g`, the shortest that reads back.
    fn lua_g(n: f64) -> String {
        for p in [15usize, 16, 17] {
            let s = pob_lua::format::lua_format(
                &format!("%.{p}g"),
                &[pob_lua::format::FmtArg::from(n)],
            );
            if s.parse::<f64>().ok() == Some(n) {
                return s;
            }
        }
        format!("{n}")
    }

    /// Lua `tostring` of a number (`%.14g`).
    fn lua_tostring_num(n: f64) -> String {
        pob_lua::tostring_num(n)
    }

    fn leaf_of(v: &Val) -> Option<J> {
        match v {
            Val::Nil => Some(J::Null),
            Val::Bool(b) => Some(J::Bool(*b)),
            Val::Num(n) => Some(jnum(*n)),
            Val::Str(s) => Some(J::String(s.to_string())),
            Val::Mod(m) => Some(mod_json(m, None)),
            Val::Data(d) => Some(value_json(d)),
            Val::Table(t) => reg_foreign(t.as_ptr() as usize),
            Val::Obj(o) => {
                if let Some(m) = reg_foreign(rc_ptr(o)) {
                    return Some(m);
                }
                if let Some(n) = o.downcast_ref::<Node>() {
                    let tag = if n.tree.is_some() { "$node" } else { "$treeNode" };
                    let mut map = Map::new();
                    map.insert(tag.to_owned(), jnum(n.id));
                    return Some(J::Object(map));
                }
                if o.downcast_ref::<Build>().is_some() {
                    return Some(serde_json::json!({"$object": "Build"}));
                }
                if o.downcast_ref::<PassiveSpec>().is_some() {
                    return Some(serde_json::json!({"$object": "PassiveSpec"}));
                }
                if let Some(d) = o.downcast_ref::<DataRef>() {
                    return Some(serde_json::json!({"$data": d.path}));
                }
                if let Some(m) = o.downcast_ref::<Marker>() {
                    return Some(m.0.clone());
                }
                if let Some(m) = closure_marker(&**o) {
                    return Some(m);
                }
                if PObj::from_val(v).is_none() {
                    return Some(serde_json::json!({"$userdata": true}));
                }
                None
            }
        }
    }

    // =====================================================================================
    // Mods and values
    // =====================================================================================

    /// A mod in the pob-state shape (`state_dump.lua` `encodeMod`).
    pub fn mod_json(m: &Mod, at: Option<&str>) -> J {
        let mut o = Map::new();
        o.insert("name".into(), J::String(m.name.as_str().to_owned()));
        o.insert("type".into(), J::String(m.r#type.as_str().to_owned()));
        o.insert("value".into(), value_json(&m.value));
        let flags = m
            .extra_get(pob_lua::n!("flags"))
            .map_or_else(|| jnum(f64::from(m.flags)), value_json);
        o.insert("flags".into(), flags);
        let keyword_flags = m
            .extra_get(pob_lua::n!("keywordFlags"))
            .map_or_else(|| jnum(f64::from(m.keywordFlags)), value_json);
        o.insert("keywordFlags".into(), keyword_flags);
        if let Some(s) = &m.source {
            o.insert("source".into(), J::String(s.to_string()));
        }
        o.insert(
            "tags".into(),
            J::Array(m.tags.iter().map(tag_json).collect()),
        );
        if let Some(ex) = &m.extra {
            for (k, v) in &ex.fields {
                let k = k.as_str();
                if k == "flags" || k == "keywordFlags" || k.starts_with('#') {
                    continue;
                }
                o.insert(escape_key(k), value_json(v));
            }
        }
        if let Some(at) = at {
            o.insert("$at".into(), J::String(at.to_owned()));
        }
        J::Object(o)
    }

    fn escape_key(k: &str) -> String {
        if k.starts_with('$') {
            format!("${k}")
        } else {
            k.to_owned()
        }
    }

    /// A tag table (`encodePlain`).
    pub fn tag_json(t: &Tag) -> J {
        if t.r#type == TagType::LuaNonTable {
            return t
                .extra_get(pob_lua::n!("$value"))
                .map_or(J::Null, value_json);
        }
        let mut o = Map::new();
        pob_gamedata::modifier::for_each_tag_field(t, |k, v| {
            o.insert(escape_key(k), value_json(&v));
        });
        if o.is_empty() {
            return J::Object(o);
        }
        J::Object(o)
    }

    /// A mod value (`encodePlain`): tables inline, `#n` fields under `$num`.
    pub fn value_json(v: &Value) -> J {
        match v {
            Value::Nil => J::Null,
            Value::Bool(b) => J::Bool(*b),
            Value::Num(n) => jnum(*n),
            Value::Str(s) => J::String(s.to_string()),
            Value::Mod(m) => plain_mod_json(m),
            Value::Table(t) => vtable_json(t),
            Value::JewelFunc(j) => {
                let mut o = Map::new();
                let mut f = Map::new();
                f.insert("$fn".into(), J::String(j.key.as_str().to_owned()));
                o.insert("func".into(), J::Object(f));
                if let Some(r) = &j.radius {
                    o.insert("radius".into(), J::String(r.as_str().to_owned()));
                }
                if let Some(t) = &j.r#type {
                    o.insert("type".into(), J::String(t.as_str().to_owned()));
                }
                J::Object(o)
            }
        }
    }

    fn vtable_json(t: &ValueTable) -> J {
        let mut num = Map::new();
        let mut o = Map::new();
        for (k, v) in &t.fields {
            let k = k.as_str();
            if let Some(n) = k.strip_prefix('#') {
                num.insert(n.to_owned(), value_json(v));
            } else {
                // value tables keep `$`-members verbatim (markers inside mod values)
                o.insert(k.to_owned(), value_json(v));
            }
        }
        if o.is_empty() && num.is_empty() && !t.array.is_empty() {
            return J::Array(t.array.iter().map(value_json).collect());
        }
        if !t.array.is_empty() {
            o.insert(
                "$seq".into(),
                J::Array(t.array.iter().map(value_json).collect()),
            );
        }
        if !num.is_empty() {
            o.insert("$num".into(), J::Object(num));
        }
        J::Object(o)
    }

    /// A mod nested in a value (a plain table: tags under `$seq`).
    fn plain_mod_json(m: &Mod) -> J {
        let mut o = Map::new();
        o.insert("name".into(), J::String(m.name.as_str().to_owned()));
        if m.r#type != pob_gamedata::ModType::Nil {
            o.insert("type".into(), J::String(m.r#type.as_str().to_owned()));
        }
        if !m.value.is_nil() {
            o.insert("value".into(), value_json(&m.value));
        }
        let flags = m
            .extra_get(pob_lua::n!("flags"))
            .map_or_else(|| jnum(f64::from(m.flags)), value_json);
        o.insert("flags".into(), flags);
        let keyword_flags = m
            .extra_get(pob_lua::n!("keywordFlags"))
            .map_or_else(|| jnum(f64::from(m.keywordFlags)), value_json);
        o.insert("keywordFlags".into(), keyword_flags);
        if let Some(s) = &m.source {
            o.insert("source".into(), J::String(s.to_string()));
        }
        let mut num = Map::new();
        if let Some(ex) = &m.extra {
            for (k, v) in &ex.fields {
                let k = k.as_str();
                if k == "flags" {
                    continue;
                }
                if let Some(n) = k.strip_prefix('#') {
                    num.insert(n.to_owned(), value_json(v));
                } else {
                    o.insert(escape_key(k), value_json(v));
                }
            }
        }
        if !m.tags.is_empty() {
            o.insert("$seq".into(), J::Array(m.tags.iter().map(tag_json).collect()));
        }
        if !num.is_empty() {
            o.insert("$num".into(), J::Object(num));
        }
        J::Object(o)
    }

    // =====================================================================================
    // $keys labels (state_dump.lua keyLabel / shallowLabel)
    // =====================================================================================

    fn jstr(s: &str) -> String {
        serde_json::to_string(s).unwrap_or_default()
    }

    fn label_one(v: &PV, depth: i32, owners: &HashMap<usize, &'static str>) -> String {
        match v {
            PV::V(Val::Str(s)) => jstr(s),
            PV::V(Val::Num(n)) => jnum_text(*n),
            PV::V(Val::Bool(b)) => b.to_string(),
            PV::V(Val::Nil) => "nil".into(),
            PV::V(Val::Mod(m)) => {
                if depth > 0 {
                    format!("{{{}}}", mod_label(m, depth - 1))
                } else {
                    "{}".into()
                }
            }
            PV::V(Val::Data(d)) => {
                if depth > 0 {
                    format!("{{{}}}", value_label(d, depth - 1))
                } else {
                    "{}".into()
                }
            }
            PV::V(v @ Val::Obj(o)) => {
                if let Some(j) = leaf_of(v) {
                    if let Some(d) = o.downcast_ref::<DataRef>() {
                        return format!("@{}", d.path);
                    }
                    if o.downcast_ref::<Marker>().is_some_and(|m| m.0.get("$fn").is_some())
                        || closure_marker(&**o).is_some()
                    {
                        return "function".into();
                    }
                    return j.to_string();
                }
                obj_label(&PObj::from_val(v).expect("identity object"), depth, owners)
            }
            PV::V(v @ Val::Table(_)) => {
                if let Some(j) = leaf_of(v) {
                    return j.to_string();
                }
                obj_label(&PObj::from_val(v).expect("table"), depth, owners)
            }
            PV::O(o) => obj_label(o, depth, owners),
        }
    }

    fn obj_label(o: &PObj, depth: i32, owners: &HashMap<usize, &'static str>) -> String {
        if let Some(c) = o.class() {
            return format!("<{c}>");
        }
        if depth > 0 {
            format!("{{{}}}", shallow_label(o, depth - 1, owners))
        } else {
            "{}".into()
        }
    }

    /// `jnum` as text (for labels).
    fn jnum_text(n: f64) -> String {
        match jnum(n) {
            J::Number(x) => {
                if n.fract() == 0.0 {
                    x.to_string()
                } else {
                    lua_g(n)
                }
            }
            other => other.to_string(),
        }
    }

    fn shallow_label(o: &PObj, depth: i32, owners: &HashMap<usize, &'static str>) -> String {
        let m = members_of(o, owners);
        let mut s_parts: Vec<(String, String)> = Vec::new();
        let mut n_parts: Vec<(f64, String)> = Vec::new();
        for (k, v) in &m.entries {
            match k {
                PKey::S(s) => s_parts.push((s.clone(), label_one(v, depth, owners))),
                PKey::N(n) => n_parts.push((*n, label_one(v, depth, owners))),
                PKey::O(_) => {}
            }
        }
        s_parts.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        n_parts.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut parts: Vec<String> = s_parts.into_iter().map(|(k, v)| format!("{k}={v}")).collect();
        parts.extend(
            n_parts
                .into_iter()
                .map(|(k, v)| format!("[{}]={v}", key_text(k))),
        );
        parts.join(",")
    }

    fn mod_label(m: &Mod, depth: i32) -> String {
        // the mod as a Lua table: string fields, then tags [1..n]
        let j = mod_json(m, None);
        let mut s_parts: Vec<(String, String)> = Vec::new();
        if let J::Object(o) = &j {
            for (k, v) in o {
                if k == "tags" {
                    continue;
                }
                s_parts.push((k.clone(), json_label(v, depth)));
            }
        }
        s_parts.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        let mut parts: Vec<String> = s_parts.into_iter().map(|(k, v)| format!("{k}={v}")).collect();
        for (i, t) in m.tags.iter().enumerate() {
            parts.push(format!("[{}]={}", i + 1, json_label(&tag_json(t), depth)));
        }
        parts.join(",")
    }

    fn value_label(v: &Value, depth: i32) -> String {
        match value_json(v) {
            J::Object(o) => {
                let mut parts: Vec<String> = o
                    .iter()
                    .filter(|(k, _)| !k.starts_with('$'))
                    .map(|(k, x)| format!("{k}={}", json_label(x, depth)))
                    .collect();
                if let Some(J::Array(a)) = o.get("$seq") {
                    for (i, x) in a.iter().enumerate() {
                        parts.push(format!("[{}]={}", i + 1, json_label(x, depth)));
                    }
                }
                parts.join(",")
            }
            J::Array(a) => a
                .iter()
                .enumerate()
                .map(|(i, x)| format!("[{}]={}", i + 1, json_label(x, depth)))
                .collect::<Vec<_>>()
                .join(","),
            _ => String::new(),
        }
    }

    /// `one(v)` over an already-encoded plain value.
    fn json_label(v: &J, depth: i32) -> String {
        match v {
            J::String(s) => jstr(s),
            J::Number(n) => n
                .as_f64()
                .map_or_else(|| n.to_string(), jnum_text),
            J::Bool(b) => b.to_string(),
            J::Null => "nil".into(),
            J::Array(a) => {
                if depth > 0 {
                    let parts: Vec<String> = a
                        .iter()
                        .enumerate()
                        .map(|(i, x)| format!("[{}]={}", i + 1, json_label(x, depth - 1)))
                        .collect();
                    format!("{{{}}}", parts.join(","))
                } else {
                    "{}".into()
                }
            }
            J::Object(o) => {
                if let Some(f) = o.get("$float").and_then(J::as_str) {
                    return f.to_owned();
                }
                if o.contains_key("$fn") {
                    return "function".into();
                }
                if depth > 0 {
                    let mut parts: Vec<String> = o
                        .iter()
                        .filter(|(k, _)| !k.starts_with('$'))
                        .map(|(k, x)| format!("{k}={}", json_label(x, depth - 1)))
                        .collect();
                    if let Some(J::Array(a)) = o.get("$seq") {
                        for (i, x) in a.iter().enumerate() {
                            parts.push(format!("[{}]={}", i + 1, json_label(x, depth - 1)));
                        }
                    }
                    format!("{{{}}}", parts.join(","))
                } else {
                    "{}".into()
                }
            }
        }
    }

    fn key_label(k: &PV, owners: &HashMap<usize, &'static str>) -> String {
        match k {
            PV::V(Val::Bool(b)) => format!("0:{b}"),
            PV::V(Val::Str(s)) => format!("6:{s}"),
            PV::V(Val::Mod(m)) => format!("4:{}", mod_label(m, 2)),
            PV::V(v @ Val::Obj(o)) => {
                if let Some(j) = leaf_of(v) {
                    if let Some(d) = o.downcast_ref::<DataRef>() {
                        return format!("2:{}", d.path);
                    }
                    if let Some(m) = o.downcast_ref::<Marker>().map(|m| m.0.clone()).or_else(|| closure_marker(&**o)) {
                        if let Some(f) = m.get("$fn").and_then(J::as_str) {
                            return format!("5:{f}");
                        }
                    }
                    return format!("1:{j}");
                }
                match PObj::from_val(v) {
                    Some(PObj::Item(it)) => format!(
                        "3:{}:{}",
                        it.id.get().map_or_else(|| "nil".to_owned(), lua_tostring_num),
                        it.name.get()
                    ),
                    Some(o) => match o.class() {
                        Some(c) => format!("7:{c}:{}", shallow_label(&o, 0, owners)),
                        None => format!("9:{}", shallow_label(&o, 1, owners)),
                    },
                    None => "8:userdata".into(),
                }
            }
            PV::V(v @ Val::Table(_)) => match leaf_of(v) {
                Some(j) => format!("1:{j}"),
                None => format!(
                    "9:{}",
                    shallow_label(&PObj::from_val(v).expect("table"), 1, owners)
                ),
            },
            PV::V(Val::Data(d)) => format!("9:{}", value_label(d, 1)),
            PV::V(other) => format!("8:{}", other.lua_type()),
            PV::O(o) => match o.class() {
                Some(c) => format!("7:{c}:{}", shallow_label(o, 0, owners)),
                None => format!("9:{}", shallow_label(o, 1, owners)),
            },
        }
    }

    // =====================================================================================
    // The encoder
    // =====================================================================================

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Group {
        Str,
        Seq,
        Num,
        KKey,
        KVal,
        Meta,
    }

    /// One ordered member: group, JSON key (str / num), value.
    struct OM {
        g: Group,
        key: String,
        v: PV,
    }

    fn classify_pv(v: &PV) -> Option<PObj> {
        match v {
            PV::O(o) => Some(o.clone()),
            PV::V(val) => {
                if leaf_of(val).is_some() {
                    return None;
                }
                PObj::from_val(val)
            }
        }
    }

    struct Enc {
        order: HashMap<(u8, usize), Rc<Vec<OM>>>,
        canon: HashMap<(u8, usize), ((u8, usize), usize)>,
        count: HashMap<(u8, usize), u32>,
        ids: HashMap<(u8, usize), u64>,
        next_id: u64,
        emitted: HashSet<(u8, usize)>,
        owners: HashMap<usize, &'static str>,
        env: Option<EnvRef>,
    }

    impl Enc {
        fn ordered(&mut self, o: &PObj) -> Rc<Vec<OM>> {
            let key = o.key();
            if let Some(x) = self.order.get(&key) {
                return x.clone();
            }
            // remember which skill cfg field a skillCond table hangs off (OrChain sites)
            if let PObj::Skill(s) = o {
                for f in [
                    "weapon1Cfg",
                    "weapon2Cfg",
                    "bleedCfg",
                    "poisonCfg",
                    "igniteCfg",
                    "OHbleedCfg",
                    "OHpoisonCfg",
                    "OHigniteCfg",
                    "dotCfg",
                    "decayCfg",
                ] {
                    if let Some(c) = s.cfg_field(Name::from(f)).get() {
                        if let Some(t) = c.skillCond.get() {
                            self.owners.insert(t.as_ptr() as usize, f);
                        }
                    }
                }
            }
            let m = members_of(o, &self.owners);
            let v = Rc::new(order_members(o, m, &self.owners));
            self.order.insert(key, v.clone());
            v
        }

        fn walk(&mut self, root: &PObj) {
            let rk = root.key();
            self.count.insert(rk, 1);
            self.canon.insert(rk, ((0, 0), usize::MAX));
            let mut queue = std::collections::VecDeque::new();
            queue.push_back(root.clone());
            while let Some(o) = queue.pop_front() {
                let ok = o.key();
                let list = self.ordered(&o);
                for (i, m) in list.iter().enumerate() {
                    if let Some(c) = classify_pv(&m.v) {
                        let ck = c.key();
                        *self.count.entry(ck).or_insert(0) += 1;
                        if let std::collections::hash_map::Entry::Vacant(e) = self.canon.entry(ck) {
                            e.insert((ok, i));
                            queue.push_back(c);
                        }
                    }
                }
            }
        }

        fn id_for(&mut self, k: (u8, usize)) -> u64 {
            if let Some(i) = self.ids.get(&k) {
                return *i;
            }
            let i = self.next_id;
            self.next_id += 1;
            self.ids.insert(k, i);
            i
        }

        fn emit_value(&mut self, v: &PV, pos: ((u8, usize), usize)) -> J {
            match classify_pv(v) {
                None => match v {
                    PV::V(val) => leaf_of(val).unwrap_or(J::Null),
                    PV::O(_) => J::Null,
                },
                Some(o) => self.emit_obj(&o, pos),
            }
        }

        fn emit_obj(&mut self, o: &PObj, pos: ((u8, usize), usize)) -> J {
            let k = o.key();
            if self.canon.get(&k) != Some(&pos) || self.emitted.contains(&k) {
                let id = self.id_for(k);
                return serde_json::json!({ "$ref": id });
            }
            self.emitted.insert(k);
            let id = if self.count.get(&k).copied().unwrap_or(1) > 1 {
                Some(self.id_for(k))
            } else {
                None
            };
            let list = self.ordered(o);
            let is_skill = matches!(o, PObj::Skill(s) if !reg_absent(rc_ptr(s), "skillFlags"));
            let tableish = !matches!(o, PObj::Store(_) | PObj::Item(_));
            if tableish
                && id.is_none()
                && !is_skill
                && !list.is_empty()
                && list.iter().all(|m| m.g == Group::Seq)
            {
                let mut arr = Vec::with_capacity(list.len());
                for (i, m) in list.iter().enumerate() {
                    arr.push(self.emit_value(&m.v, (k, i)));
                }
                return J::Array(arr);
            }
            let mut out = Map::new();
            if let Some(id) = id {
                out.insert("$id".into(), J::from(id));
            }
            match o {
                PObj::Store(s) => {
                    out.insert(
                        "$store".into(),
                        J::String(o.class().unwrap_or("ModList").to_owned()),
                    );
                    out.insert("mods".into(), store_mods_json(s));
                }
                PObj::Item(it) => {
                    out.insert("$item".into(), J::String(it.name.get().to_string()));
                }
                PObj::Skill(s) if is_skill && self.env.is_some() => {
                    // state_dump.lua: cacheSkillUUID(skill, rootEnv); the dumped value
                    // when the skill was injected (the build model may not be loaded)
                    let uuid = REG.with(|r| r.borrow().uuid.get(&rc_ptr(s)).cloned());
                    let uuid = uuid.or_else(|| {
                        self.env.as_ref().map(|e| {
                            crate::modules::common::cacheSkillUUID(s, e).as_str().to_owned()
                        })
                    });
                    if let Some(u) = uuid {
                        out.insert("$uuid".into(), J::String(u));
                    }
                }
                _ => {}
            }
            let mut seq = Vec::new();
            let mut num = Map::new();
            let mut keys = Vec::new();
            let mut pending_key: Option<J> = None;
            for (i, m) in list.iter().enumerate() {
                let val = self.emit_value(&m.v, (k, i));
                match m.g {
                    Group::Str => {
                        out.insert(m.key.clone(), val);
                    }
                    Group::Seq => seq.push(val),
                    Group::Num => {
                        num.insert(m.key.clone(), val);
                    }
                    Group::KKey => pending_key = Some(val),
                    Group::KVal => {
                        keys.push(J::Array(vec![pending_key.take().unwrap_or(J::Null), val]));
                    }
                    Group::Meta => {
                        out.insert("$meta".into(), val);
                    }
                }
            }
            if !seq.is_empty() {
                out.insert("$seq".into(), J::Array(seq));
            }
            if !num.is_empty() {
                out.insert("$num".into(), J::Object(num));
            }
            if !keys.is_empty() {
                out.insert("$keys".into(), J::Array(keys));
            }
            J::Object(out)
        }
    }

    fn order_members(o: &PObj, m: Members, owners: &HashMap<usize, &'static str>) -> Vec<OM> {
        let mut strs: Vec<(String, PV)> = Vec::new();
        let mut nums: Vec<(f64, PV)> = Vec::new();
        let mut others: Vec<(PV, PV)> = Vec::new();
        for (k, v) in m.entries {
            if v.is_nil() {
                continue;
            }
            match k {
                PKey::S(s) => strs.push((s, v)),
                PKey::N(n) => nums.push((n, v)),
                PKey::O(k) => others.push((k, v)),
            }
        }
        strs.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        nums.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut out = Vec::new();
        for (k, v) in strs {
            out.push(OM {
                g: Group::Str,
                key: escape_key(&k),
                v,
            });
        }
        // the sequence part 1..n
        let mut n = 0usize;
        for (k, _) in &nums {
            if *k == (n + 1) as f64 {
                n += 1;
            } else if *k > (n + 1) as f64 {
                break;
            }
        }
        let _ = o;
        for (k, v) in &nums {
            if *k >= 1.0 && *k <= n as f64 && k.fract() == 0.0 {
                out.push(OM {
                    g: Group::Seq,
                    key: String::new(),
                    v: v.clone(),
                });
            }
        }
        for (k, v) in nums {
            if !(k >= 1.0 && k <= n as f64 && k.fract() == 0.0) {
                out.push(OM {
                    g: Group::Num,
                    key: key_text(k),
                    v,
                });
            }
        }
        let mut labelled: Vec<(String, PV, PV)> = others
            .into_iter()
            .map(|(k, v)| (key_label(&k, owners), k, v))
            .collect();
        labelled.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        for (_, k, v) in labelled {
            out.push(OM {
                g: Group::KKey,
                key: String::new(),
                v: k,
            });
            out.push(OM {
                g: Group::KVal,
                key: String::new(),
                v,
            });
        }
        if let Some(meta) = m.meta {
            out.push(OM {
                g: Group::Meta,
                key: String::new(),
                v: meta,
            });
        }
        out
    }

    fn store_mods_json(s: &ModStore) -> J {
        let sp = std::ptr::from_ref(s) as usize;
        let at = |m: &ModRef| REG.with(|r| r.borrow().mod_at.get(&(sp, arc_ptr(m))).cloned());
        match &*s.mods.borrow() {
            StoreMods::DB(map) => {
                let mut names: Vec<&Name> = map.keys().collect();
                names.sort_by(|a, b| a.as_str().as_bytes().cmp(b.as_str().as_bytes()));
                let mut o = Map::new();
                for n in names {
                    let list: Vec<J> = map[n]
                        .iter()
                        .map(|m| mod_json(m, at(m).as_deref()))
                        .collect();
                    o.insert(escape_key(n.as_str()), J::Array(list));
                }
                J::Object(o)
            }
            StoreMods::List { list, .. } => J::Array(
                list.iter()
                    .map(|m| mod_json(m, at(m).as_deref()))
                    .collect(),
            ),
        }
    }

    // =====================================================================================
    // Public export API
    // =====================================================================================

    /// The `root` of a pob-state/1 document for `env` and `locals` (plus the
    /// thread-local `GlobalCache`), encoded exactly as `state_dump.lua` does.
    pub fn export_root(env: Option<&EnvRef>, locals: &[(String, Val)]) -> J {
        let loc = Rc::new(Synth {
            members: locals.to_vec(),
        });
        let mut members = vec![("globalCache".to_owned(), Val::Nil)];
        if let Some(e) = env {
            members.push(("env".to_owned(), Val::Obj(e.clone())));
        }
        let root = PObj::Synth(Rc::new(Synth { members }));
        // `locals` as a member of the root
        let mut enc = Enc {
            order: HashMap::new(),
            canon: HashMap::new(),
            count: HashMap::new(),
            ids: HashMap::new(),
            next_id: 1,
            emitted: HashSet::new(),
            owners: HashMap::new(),
            env: env.cloned(),
        };
        // the root's member list includes locals as a synthetic object
        let mut rm = members_of(&root, &enc.owners);
        rm.entries
            .push((PKey::S("locals".to_owned()), PV::O(PObj::Synth(loc))));
        let list = Rc::new(order_members(&root, rm, &enc.owners));
        enc.order.insert(root.key(), list);
        enc.walk(&root);
        enc.emit_obj(&root, ((0, 0), usize::MAX))
    }

    /// A full pob-state/1 document around [`export_root`].
    pub fn export_doc(
        build: &str,
        hit: &str,
        kind: &str,
        env: Option<&EnvRef>,
        locals: &[(String, Val)],
    ) -> J {
        let (probe, n) = match hit.rsplit_once('#') {
            Some((p, n)) => (p.to_owned(), n.parse::<u64>().unwrap_or(1)),
            None => (hit.to_owned(), 1),
        };
        serde_json::json!({
            "$env": J::Null,
            "build": build,
            "format": "pob-state/1",
            "hit": hit,
            "kind": kind,
            "n": n,
            "probe": probe,
            "root": export_root(env, locals),
            "stack": [],
        })
    }

    // =====================================================================================
    // Injection support: member assignment through the same reflection
    // =====================================================================================

    /// `obj[key] = v` on a reflected object. Typed fields take the value when
    /// it fits; otherwise (or for a key without a typed field) it goes to the
    /// struct's `extra` table, or to the registry's side table for structs
    /// without one. Returns false when a typed field could not take the value
    /// (it is then marked absent) or no table could hold it.
    pub fn set_member(o: &PObj, key: &PKey, v: Val) -> bool {
        fn typed<T: Reflect>(x: &T, ptr: usize, key: &PKey, v: Val, what: &str) -> bool {
            if let PKey::S(k) = key {
                match x.r_set(k, &v) {
                    Some(true) => return true,
                    Some(false) => {
                        mark_absent(ptr, k);
                        with_registry(|r| r.fallback(format!("{what}.{k}: type")));
                        if let Some(e) = x.r_extra() {
                            table_set(e, key, v);
                        }
                        return false;
                    }
                    None => {}
                }
            }
            match x.r_extra() {
                Some(e) => table_set(e, key, v),
                None => {
                    if let PKey::S(k) = key {
                        with_registry(|r| r.push_side_extra(ptr, k.clone(), v));
                    }
                    false
                }
            }
        }
        match o {
            PObj::Env(x) => typed(&**x, rc_ptr(x), key, v, "env"),
            PObj::Actor(x) => typed(&**x, rc_ptr(x), key, v, "actor"),
            PObj::Skill(x) => typed(&**x, rc_ptr(x), key, v, "activeSkill"),
            PObj::Effect(x) => typed(&**x, rc_ptr(x), key, v, "skillEffect"),
            PObj::Cfg(x) => typed(&**x, rc_ptr(x), key, v, "cfg"),
            PObj::Gem(x) => typed(&**x, rc_ptr(x), key, v, "gemInstance"),
            PObj::Group(x) => typed(&**x, rc_ptr(x), key, v, "socketGroup"),
            PObj::Override(x) => typed(&**x, rc_ptr(x), key, v, "override"),
            PObj::Table(t) | PObj::Breakdown(Breakdown { t, .. }) => table_set(t, key, v),
            PObj::Store(s) => {
                let p = rc_ptr(s);
                match key {
                    PKey::S(k) if k == "actor" => match &v {
                        Val::Obj(_) => match v.obj::<Actor>() {
                            Some(a) => {
                                s.actor.set(Rc::downgrade(&a));
                                true
                            }
                            None => false,
                        },
                        _ => false,
                    },
                    PKey::S(k) if k == "parent" => match &v {
                        Val::Bool(false) => {
                            s.parent.set(None);
                            true
                        }
                        _ => match v.obj::<ModStore>() {
                            Some(x) => {
                                s.parent.set(Some(x));
                                true
                            }
                            None => false,
                        },
                    },
                    PKey::S(k) if k == "conditions" || k == "multipliers" => true,
                    // `modList.notBuff = true` (CCR-0030 #7, CCR-0076)
                    PKey::S(k) if k == "notBuff" => match &v {
                        Val::Bool(b) => {
                            s.notBuff.set(*b);
                            true
                        }
                        _ => false,
                    },
                    PKey::S(k) => {
                        with_registry(|r| {
                            r.push_side_extra(p, k.clone(), v);
                            r.fallback(format!("modStore.{k}: no field"));
                        });
                        false
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// `t[k] = v` on a dynamic table (object keys go to the registry).
    pub fn table_set(t: &Table, key: &PKey, v: Val) -> bool {
        match key {
            PKey::S(s) => {
                t.set(Name::from(s.as_str()), v);
                true
            }
            PKey::N(n) => {
                if n.fract() == 0.0 && n.is_finite() {
                    t.set(Key::I(*n as i64), v);
                    true
                } else {
                    with_registry(|r| {
                        r.push_obj_key(t, Val::Num(*n), v);
                        r.fallback("table: non-integer number key".to_owned());
                    });
                    false
                }
            }
            PKey::O(PV::V(Val::Bool(b))) => {
                t.set(Key::B(*b), v);
                true
            }
            PKey::O(PV::V(k)) => {
                with_registry(|r| {
                    r.push_obj_key(t, k.clone(), v);
                    r.fallback("table: object key".to_owned());
                });
                false
            }
            PKey::O(PV::O(o)) => {
                let k = o.to_val();
                with_registry(|r| r.push_obj_key(t, k, v));
                false
            }
        }
    }

    /// `self.mods[name] = mods` on a ModDB (injection: keeps PoB's per-name order
    /// and empty name lists); on a ModList the mods are appended.
    pub fn store_put_name(s: &ModStore, name: Name, mods: Vec<ModRef>) {
        if s.kind == StoreKind::ModList {
            for m in mods {
                s.AddMod(m);
            }
            return;
        }
        if let StoreMods::DB(map) = &mut *s.mods.borrow_mut() {
            map.entry(name).or_default().extend(mods);
        }
    }

    /// `o[key]` as the exporter sees it (`None` when nil).
    pub fn member_get(o: &PObj, key: &PKey) -> Option<PV> {
        let m = members_of(o, &HashMap::new());
        m.entries.into_iter().find_map(|(k, v)| {
            let hit = match (&k, key) {
                (PKey::S(a), PKey::S(b)) => a == b,
                (PKey::N(a), PKey::N(b)) => a == b,
                _ => false,
            };
            hit.then_some(v)
        })
    }

    /// The metatable of `o` as the exporter sees it.
    pub fn meta_get(o: &PObj) -> Option<PV> {
        members_of(o, &HashMap::new()).meta
    }

    /// A value as an identity object of the export graph (`None` for scalars and markers).
    pub fn pv_obj(v: &PV) -> Option<PObj> {
        classify_pv(v)
    }

    /// The kind of an identity object.
    pub fn pobj_kind(o: &PObj) -> Kind {
        match o {
            PObj::Env(_) => Kind::Env,
            PObj::Actor(_) => Kind::Actor,
            PObj::Skill(_) => Kind::Skill,
            PObj::Effect(_) => Kind::Effect,
            PObj::Cfg(_) => Kind::Cfg,
            PObj::Store(_) => Kind::Store,
            PObj::Item(_) => Kind::Item,
            PObj::Gem(_) => Kind::Gem,
            PObj::Group(_) => Kind::Group,
            PObj::Override(_) => Kind::Override,
            PObj::Breakdown(_) => Kind::Breakdown,
            PObj::SkillTypes(_) => Kind::SkillTypes,
            PObj::NameSet(_) => Kind::NameSet,
            PObj::SkillList(_) => Kind::SkillList,
            PObj::EffectList(_) => Kind::EffectList,
            PObj::TableList(_) => Kind::TableList,
            PObj::NameList(_) => Kind::NameList,
            PObj::ModVec(_) => Kind::ModVec,
            PObj::GemList(_) => Kind::GemList,
            PObj::NodeMap(_) => Kind::NodeMap,
            PObj::BoolMap(_) => Kind::BoolMap,
            PObj::ItemFlags(_) => Kind::ItemFlags,
            PObj::GemSet(_) => Kind::GemSet,
            PObj::ItemList(_) => Kind::ItemList,
            PObj::ExplodeList(_) => Kind::ExplodeList,
            PObj::FlaskSlots(_) => Kind::FlaskSlots,
            PObj::ModSet(_) => Kind::ModSet,
            PObj::ItemMods(_) => Kind::ItemMods,
            PObj::SlotMods(_) => Kind::SlotMods,
            PObj::Table(_)
            | PObj::Meta(..)
            | PObj::GlobalCache
            | PObj::CacheData
            | PObj::CacheMode(_)
            | PObj::CacheEntry(_)
            | PObj::Synth(_) => Kind::Table,
        }
    }

    /// Empty a live object before it is refilled from a dump (replay): tables,
    /// lists, maps and mod stores lose their content (keeping their identity),
    /// structs lose their `extra` and registry side entries.
    pub fn clear_for_refill(o: &PObj) {
        let clear_obj_keys = |t: &Table| {
            with_registry(|r| {
                r.obj_keys.remove(&(t.as_ptr() as usize));
            });
        };
        match o {
            PObj::Table(t) | PObj::Breakdown(Breakdown { t, .. }) => {
                t.wipe();
                t.set_meta(None);
                clear_obj_keys(t);
            }
            PObj::Store(s) | PObj::ItemMods(s) => {
                s.notBuff.set(false);
                match &mut *s.mods.borrow_mut() {
                    StoreMods::DB(map) => map.clear(),
                    StoreMods::List { list, index } => {
                        list.clear();
                        index.clear();
                    }
                }
                with_registry(|r| {
                    r.side_extra.remove(&rc_ptr(s));
                });
            }
            PObj::Env(x) => x.extra.wipe(),
            PObj::Actor(x) => x.extra.wipe(),
            PObj::Skill(x) => x.extra.wipe(),
            PObj::Effect(x) => x.extra.wipe(),
            PObj::Cfg(x) => x.extra.wipe(),
            PObj::Gem(x) => x.extra.wipe(),
            PObj::Group(x) => x.extra.wipe(),
            PObj::Override(x) => x.extra.wipe(),
            PObj::SkillTypes(s) => s.with_mut(|x| *x = SkillTypeSet::default()),
            PObj::SkillList(s) => s.with_mut(Vec::clear),
            PObj::EffectList(s) => s.with_mut(Vec::clear),
            PObj::TableList(s) => s.with_mut(Vec::clear),
            PObj::NameList(s) => s.with_mut(Vec::clear),
            PObj::ModVec(s) => s.with_mut(Vec::clear),
            PObj::GemList(s) => s.with_mut(Vec::clear),
            PObj::NodeMap(s) => s.with_mut(LjIntMap::wipe),
            PObj::BoolMap(s) => s.with_mut(LjIntMap::wipe),
            PObj::ItemFlags(s) => s.with_mut(PtrMap::wipe),
            PObj::GemSet(s) => s.with_mut(PtrSet::wipe),
            PObj::ItemList(s) => s.with_mut(indexmap::IndexMap::clear),
            PObj::ExplodeList(s) => s.with_mut(Vec::clear),
            PObj::FlaskSlots(s) => s.with_mut(PtrMap::wipe),
            PObj::ModSet(s) => s.with_mut(ModPtrSet::wipe),
            _ => {}
        }
    }

    /// Typed fields of a struct that a dump does not set: nil where the type
    /// allows it (replay), and marked absent.
    pub fn unset_typed(o: &PObj, key: &str) {
        let nil = Val::Nil;
        let ptr = o.reg_ptr();
        let _ = match o {
            PObj::Env(x) => x.r_set(key, &nil),
            PObj::Actor(x) => x.r_set(key, &nil),
            PObj::Skill(x) => x.r_set(key, &nil),
            PObj::Effect(x) => x.r_set(key, &nil),
            PObj::Cfg(x) => x.r_set(key, &nil),
            PObj::Gem(x) => x.r_set(key, &nil),
            PObj::Group(x) => x.r_set(key, &nil),
            PObj::Override(x) => x.r_set(key, &nil),
            _ => None,
        };
        mark_absent(ptr, key);
    }

    /// The typed Lua keys of a struct kind (for the absent registry).
    pub fn typed_keys(k: Kind) -> &'static [&'static str] {
        match k {
            Kind::Env => Env::KEYS,
            Kind::Actor => Actor::KEYS,
            Kind::Skill => ActiveSkill::KEYS,
            Kind::Effect => SkillEffect::KEYS,
            Kind::Cfg => ModCfg::KEYS,
            Kind::Gem => GemInstance::KEYS,
            Kind::Group => SocketGroup::KEYS,
            Kind::Override => Override::KEYS,
            _ => &[],
        }
    }

    // =====================================================================================
    // Sinks: probe!(ctx, id) at runtime
    // =====================================================================================

    type Sink = Box<dyn FnMut(&str, J)>;

    thread_local! {
        static SINK: RefCell<Option<Sink>> = RefCell::new(None);
        static HITS: RefCell<HashMap<String, u64>> = RefCell::new(HashMap::new());
    }

    /// Install (or with `None` remove) the receiver of `probe!` exports: it
    /// gets the hit id (`<id>#<n>`) and the pob-state/1 document.
    pub fn set_sink(f: Option<Sink>) {
        SINK.with(|s| *s.borrow_mut() = f);
        HITS.with(|h| h.borrow_mut().clear());
    }

    /// `probe!` body: export a ctx (its `env` local is the document's env).
    pub fn emit_ctx<C: ProbeState>(id: &'static str, ctx: &C) {
        let active = SINK.with(|s| s.borrow().is_some());
        if !active {
            return;
        }
        let locals: Vec<(String, Val)> = ctx
            .probe_locals()
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect();
        let env = locals
            .iter()
            .find(|(k, _)| k == "env")
            .and_then(|(_, v)| v.obj::<Env>());
        let n = HITS.with(|h| {
            let mut h = h.borrow_mut();
            let c = h.entry(id.to_owned()).or_insert(0);
            *c += 1;
            *c
        });
        let hit = format!("{id}#{n}");
        let doc = export_doc("", &hit, "line", env.as_ref(), &locals);
        SINK.with(|s| {
            if let Some(f) = s.borrow_mut().as_mut() {
                f(&hit, doc);
            }
        });
    }

    // =====================================================================================
    // Skill hooks: replay of not-yet-ported hooks during an L4 run (CCR-0041)
    // =====================================================================================

    /// Receives every `runSkillFunc(name)` call of the offence segments (CalcOffence.lua:509)
    /// before the Rust body runs: `true` means the call was handled (e.g. the hook's recorded
    /// `runSkillFunc:in → :out` delta was applied) and the Rust body is skipped.
    pub type SkillHookHandler = Box<dyn FnMut(&ActiveSkillRef, crate::data_fns::SkillHook) -> bool>;

    thread_local! {
        static SKILL_HOOK_HANDLER: RefCell<Option<SkillHookHandler>> = RefCell::new(None);
    }

    /// Install (or with `None` remove) the skill-hook handler; returns the previous one.
    pub fn set_skill_hook_handler(h: Option<SkillHookHandler>) -> Option<SkillHookHandler> {
        SKILL_HOOK_HANDLER.with(|s| std::mem::replace(&mut *s.borrow_mut(), h))
    }

    /// Offer a `runSkillFunc` call to the installed handler (false when none is installed).
    /// The handler is taken out while it runs, so it may call into the calc freely.
    pub fn skill_hook_handled(skill: &ActiveSkillRef, name: crate::data_fns::SkillHook) -> bool {
        let Some(mut h) = SKILL_HOOK_HANDLER.with(|s| s.borrow_mut().take()) else {
            return false;
        };
        let r = h(skill, name);
        SKILL_HOOK_HANDLER.with(|s| {
            let mut s = s.borrow_mut();
            if s.is_none() {
                *s = Some(h);
            }
        });
        r
    }

    // =====================================================================================
    // L4: one segment from injected locals
    // =====================================================================================

    /// Assign the injected locals the ctx has a field for. A local of the
    /// ctx whose value does not fit the field's type is an error (it would
    /// otherwise stay at the ctx default and the segment would run on it);
    /// function locals (`$fn` markers: Rust has the function itself) and
    /// locals of other ctxs are skipped.
    fn ctx_fill<C: ProbeState>(c: &mut C, locals: &[(String, Val)]) -> Result<(), String> {
        let names = C::locals();
        let mut bad = Vec::new();
        for (k, v) in locals {
            if !names.contains(&k.as_str()) {
                continue;
            }
            let is_fn = v
                .obj::<Marker>()
                .is_some_and(|m| m.0.get("$fn").is_some());
            if !c.probe_set(k, v) && !table_list_set(c, k, v) && !is_fn {
                bad.push(format!("{k} = {}", v.lua_type()));
            }
        }
        if bad.is_empty() {
            Ok(())
        } else {
            Err(format!("injected locals of the wrong type: {}", bad.join(", ")))
        }
    }

    /// A local of a `Shared<Vec<Table>>` field (`OffenceCtx.passList`) arrives as the plain Lua
    /// sequence of tables (`inject` decodes locals with `Kind::Any`): hand the field a fresh
    /// `Shared` of the same element tables (identity kept). A local is never aliased by another
    /// table, so this is the same state; typed fields of injected objects keep the strict
    /// conversion (their sequences may be aliased, e.g. `socketGroup.explodeSources`). CCR-0036.
    fn table_list_set<C: ProbeState>(c: &mut C, k: &str, v: &Val) -> bool {
        let Some(t) = v.table() else { return false };
        let mut list = Vec::new();
        for (_, e) in t.ipairs() {
            match e.table() {
                Some(e) => list.push(e),
                None => return false,
            }
        }
        c.probe_set(k, &Shared::new(list).to_val())
    }

    fn ctx_out<C: ProbeState>(c: &C, out: &mut Vec<(String, Val)>) {
        for (k, v) in c.probe_locals() {
            // a local function whose closure the Rust ctx does not hold (`mergeFlasks` /
            // `mergeTinctures` seeded from a `$fn` marker, CCR-0079 §1) is not exported: the
            // diff skips PoB's function locals the Rust side does not export
            if v.is_nil() && matches!(k, "mergeFlasks" | "mergeTinctures") {
                continue;
            }
            if let Some(slot) = out.iter_mut().find(|(n, _)| n == k) {
                slot.1 = v;
            } else {
                out.push((k.to_owned(), v));
            }
        }
    }

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

    #[allow(dead_code)]
    fn _assert_traits() {
        fn f<T: ProbeField>() {}
        f::<Option<Breakdown>>();
        f::<Option<f64>>();
        f::<Shared<Vec<Table>>>();
        f::<Rc<Override>>();
        f::<Option<SpecEnv>>();
        f::<Accelerate>();
        f::<BuildRef>();
        f::<PassiveSpecRef>();
        f::<Option<EnvRef>>();
    }
}
