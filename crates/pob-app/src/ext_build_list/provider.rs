//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ExtBuildListProvider.lua 1-63 (the
//! abstract base class of the external build list providers). Owner: U52.
//!
//! @pob-file Classes/ExtBuildListProvider.lua @16de4b82d
//!
//! Lua shape: `newClass("ExtBuildListProvider")` with derived classes (`PoBArchivesProvider`)
//! overriding `GetBuilds` / `GetPageUrl`. The port is the trait [`ExtBuildListProviderClass`]:
//! the base class's fields live in an [`ExtBuildListProvider`] behind a `RefCell` (the provider
//! object is aliased by the list control and by the download callbacks it registers, which
//! write `statusMsg` / `buildList` later, as the Lua closures over `self` do); the base methods
//! are the trait's default methods, so `self:GetBuilds()` inside `SetActiveList` dispatches to
//! the derived class as Lua's method lookup does. A provider is shared as [`ProviderRef`]
//! (`Rc<dyn ExtBuildListProviderClass>`); methods that may call `GetBuilds` take
//! `self: Rc<Self>` (the callbacks keep the provider alive, as the Lua upvalue does).
//!
//! Values decoded from JSON (dkjson) keep their Lua type in [`LVal`]: PoB copies them into the
//! build entries unchecked (`build.dps = value.fullDPS`), and the list control's draw code then
//! applies Lua's coercions (`string.format('%0.f', s)` accepts a numeric string, `..` a number,
//! `>` errors on mixed types). The helpers below are those coercions.
//!
//! DIVERGENCE(table-tostring): `string.format('%s', t)` of a table prints its address in Lua
//! (`table: 0x…`); [`LVal::fmt_s`] prints `table: 0x0`. PoB only formats JSON scalars there.
//! ORDER: no `pairs` loops (`ipairs(self.listTitles)` in `SetActiveList`).
//! Props with side effects (ui doc §7.3): none.
#![allow(non_snake_case)] // Lua fields verbatim (calc doc §3.1 rule 1)

use std::cell::RefCell;
use std::rc::Rc;

use pob_ui_core::prelude::*;

/// A non-nil Lua value copied out of a dkjson document (`nil` is `Option::None`).
#[derive(Clone, Debug, PartialEq)]
pub enum LVal {
    Bool(bool),
    Num(f64),
    Str(String),
    /// A JSON object or array (a Lua table).
    Table(Rc<serde_json::Value>),
}

impl LVal {
    /// The dkjson value of a JSON value: `null` is nil (dkjson's default `nullval`).
    pub fn from_json(v: &serde_json::Value) -> Option<LVal> {
        use serde_json::Value as J;
        match v {
            J::Null => None,
            J::Bool(b) => Some(LVal::Bool(*b)),
            J::Number(n) => Some(LVal::Num(n.as_f64().unwrap_or(f64::NAN))),
            J::String(s) => Some(LVal::Str(s.clone())),
            J::Array(_) | J::Object(_) => Some(LVal::Table(Rc::new(v.clone()))),
        }
    }

    /// The Lua type name (`type(v)`).
    pub fn type_name(v: Option<&LVal>) -> &'static str {
        match v {
            None => "nil",
            Some(LVal::Bool(_)) => "boolean",
            Some(LVal::Num(_)) => "number",
            Some(LVal::Str(_)) => "string",
            Some(LVal::Table(_)) => "table",
        }
    }

    /// Lua truthiness of a possibly-nil value.
    pub fn truthy(v: Option<&LVal>) -> bool {
        !matches!(v, None | Some(LVal::Bool(false)))
    }

    /// `a or b` on possibly-nil values.
    pub fn or(a: Option<&LVal>, b: Option<&LVal>) -> Option<LVal> {
        if LVal::truthy(a) {
            a.cloned()
        } else {
            b.cloned()
        }
    }

    /// The operand of `..` (`what` names it in the error, e.g. `"field 'short_uuid'"`): a
    /// string, or a number as `tostring` writes it; anything else is the Lua error.
    #[track_caller]
    pub fn concat(v: Option<&LVal>, what: &str) -> String {
        match v {
            Some(LVal::Str(s)) => s.clone(),
            Some(LVal::Num(n)) => pob_lua::tostring_num(*n),
            other => lua_error!(
                "attempt to concatenate {what} (a {} value)",
                LVal::type_name(other)
            ),
        }
    }

    /// A number argument of `string.format` / `math.max` (`luaL_checknumber`: a number or a
    /// string `tonumber` accepts).
    #[track_caller]
    pub fn check_number(v: Option<&LVal>, arg: usize, func: &str) -> f64 {
        match v {
            Some(LVal::Num(n)) => *n,
            Some(LVal::Str(s)) => pob_lua::tonumber(s).unwrap_or_else(|| {
                lua_error!("bad argument #{arg} to '{func}' (number expected, got string)")
            }),
            other => lua_error!(
                "bad argument #{arg} to '{func}' (number expected, got {})",
                if other.is_none() {
                    "no value"
                } else {
                    LVal::type_name(other)
                }
            ),
        }
    }

    /// A string argument of a C function that takes strings (`luaL_checkstring` /
    /// `lua_isstring`: numbers are converted).
    #[track_caller]
    pub fn check_string(v: Option<&LVal>, arg: usize, func: &str) -> String {
        match v {
            Some(LVal::Str(s)) => s.clone(),
            Some(LVal::Num(n)) => pob_lua::tostring_num(*n),
            other => lua_error!(
                "bad argument #{arg} to '{func}' (string expected, got {})",
                LVal::type_name(other)
            ),
        }
    }

    /// `string.format('%s', v)` (LuaJIT 2.1 `lj_strfmt_obj`: `tostring` semantics).
    pub fn fmt_s(v: Option<&LVal>) -> String {
        match v {
            None => "nil".to_owned(),
            Some(LVal::Bool(b)) => b.to_string(),
            Some(LVal::Num(n)) => pob_lua::tostring_num(*n),
            Some(LVal::Str(s)) => s.clone(),
            // DIVERGENCE(table-tostring), module notes
            Some(LVal::Table(_)) => "table: 0x0".to_owned(),
        }
    }

    /// Lua `a < b` (numbers numerically, strings bytewise; anything else is the Lua error).
    #[track_caller]
    pub fn lt(a: &LVal, b: &LVal) -> bool {
        match (a, b) {
            (LVal::Num(x), LVal::Num(y)) => x < y,
            (LVal::Str(x), LVal::Str(y)) => x.as_bytes() < y.as_bytes(),
            _ => {
                let (ta, tb) = (LVal::type_name(Some(a)), LVal::type_name(Some(b)));
                if ta == tb {
                    lua_error!("attempt to compare two {ta} values")
                } else {
                    lua_error!("attempt to compare {ta} with {tb}")
                }
            }
        }
    }

    /// The string a method call on `v` needs (`str:gmatch(..)`, `ascendancy:gsub(..)`): only
    /// strings have methods; indexing anything else is the Lua error (`what` names it).
    #[track_caller]
    pub fn method_string<'a>(v: Option<&'a LVal>, what: &str) -> &'a str {
        match v {
            Some(LVal::Str(s)) => s,
            other => lua_error!(
                "attempt to index {what} (a {} value)",
                LVal::type_name(other)
            ),
        }
    }
}

/// One entry of `buildList` (the Lua table `build` that `ParseBuilds` fills; "buildName and
/// buildLink fields are required").
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExtBuild {
    pub buildName: Option<LVal>,
    pub author: Option<LVal>,
    pub mainSkill: Option<LVal>,
    pub ascendancy: Option<LVal>,
    pub class: Option<LVal>,
    pub previewLink: Option<String>,
    pub buildLink: Option<LVal>,
    pub ehp: Option<LVal>,
    pub life: Option<LVal>,
    pub es: Option<LVal>,
    pub dps: Option<LVal>,
    pub version: Option<LVal>,
    /// `build.metadata = { { key = .., value = .. } }`
    pub metadata: Option<Vec<(String, Option<LVal>)>>,
}

/// The fields of an `ExtBuildListProvider` object (the base class; module notes).
#[derive(Clone, Debug, Default)]
pub struct ExtBuildListProvider {
    /// `self.listTitles` (titles of the API endpoints, e.g. Latest, Popular)
    pub listTitles: Option<Vec<String>>,
    /// `self.buildList` (filled by `GetBuilds`)
    pub buildList: Vec<Rc<ExtBuild>>,
    /// `self.activeList`
    pub activeList: Option<String>,
    /// `self.statusMsg` (builds are not listed while it is set)
    pub statusMsg: Option<String>,
    /// `self.importCode`
    pub importCode: Option<String>,
    /// `self.buildListTitles` (ExtBuildListControl.lua:69 writes it; nothing reads it)
    pub buildListTitles: Option<Vec<String>>,
    /// `self.contentHeight` (PoBArchivesProvider.lua:132 clears it; nothing reads it)
    pub contentHeight: Option<f64>,
}

impl ExtBuildListProvider {
    /// lua: ExtBuildListProvider.lua:16-22 ExtBuildListProvider(listTitles)
    /// @pob Classes/ExtBuildListProvider.lua:16-22 h=156bedf7
    pub fn new(listTitles: Option<Vec<String>>) -> ExtBuildListProvider {
        ExtBuildListProvider {
            // L17
            listTitles,
            // L18
            buildList: Vec::new(),
            // L19
            activeList: None,
            // L20
            statusMsg: None,
            importCode: None,
            buildListTitles: None,
            contentHeight: None,
            // L21 return self
        }
    }
}

/// A provider object as the list control holds it (`provider.impl`).
pub type ProviderRef = Rc<dyn ExtBuildListProviderClass>;

/// The `ExtBuildListProvider` class: the base methods are the default methods; a derived class
/// implements [`Self::base`] and overrides what it overrides (module notes).
/// @pob Classes/ExtBuildListProvider.lua:13-14 h=9f018b3c
pub trait ExtBuildListProviderClass {
    /// The object's fields.
    fn base(&self) -> &RefCell<ExtBuildListProvider>;

    /// lua: ExtBuildListProvider.lua:24-26 GetPageUrl()
    /// @pob Classes/ExtBuildListProvider.lua:24-26 h=dc0c76c0
    fn get_page_url(&self) -> Option<String> {
        // L25
        None
    }

    /// lua: ExtBuildListProvider.lua:28-32 Activate()
    /// @pob Classes/ExtBuildListProvider.lua:28-32 h=248f971a
    fn activate(self: Rc<Self>, cx: &mut Cx) {
        // L29 if self.listTitles and next(self.listTitles) then
        let first = {
            let b = self.base().borrow();
            b.listTitles.as_ref().and_then(|t| t.first().cloned())
        };
        if let Some(first) = first {
            // L30
            self.set_active_list(cx, Some(&first));
        }
    }

    /// lua: ExtBuildListProvider.lua:34-43 SetActiveList(activeList)
    /// @pob Classes/ExtBuildListProvider.lua:34-43 h=c22cf610
    fn set_active_list(self: Rc<Self>, cx: &mut Cx, activeList: Option<&str>) {
        // L35
        if self.base().borrow().listTitles.is_none() {
            return;
        }
        // L36 for _, value in ipairs(self.listTitles) do (read live each step)
        let mut i = 0;
        loop {
            let value = {
                let b = self.base().borrow();
                b.listTitles.as_ref().and_then(|t| t.get(i).cloned())
            };
            let Some(value) = value else { break };
            i += 1;
            // L37
            if Some(value.as_str()) == activeList {
                // L38
                self.base().borrow_mut().activeList = activeList.map(str::to_owned);
                // L39
                self.clone().get_builds(cx);
            }
        }
    }

    /// lua: ExtBuildListProvider.lua:45-47 GetActiveList()
    /// @pob Classes/ExtBuildListProvider.lua:45-47 h=0ecb7113
    fn get_active_list(&self) -> Option<String> {
        // L46
        self.base().borrow().activeList.clone()
    }

    /// lua: ExtBuildListProvider.lua:49-51 GetListTitles()
    /// @pob Classes/ExtBuildListProvider.lua:49-51 h=b93e2e0e
    fn get_list_titles(&self) -> Option<Vec<String>> {
        // L50
        self.base().borrow().listTitles.clone()
    }

    /// lua: ExtBuildListProvider.lua:53-55 GetActivePageUrl()
    /// @pob Classes/ExtBuildListProvider.lua:53-55 h=a0d0a239
    fn get_active_page_url(&self) -> Option<String> {
        // L54
        None
    }

    /// The base returns an empty table; no caller reads the result.
    /// lua: ExtBuildListProvider.lua:57-59 GetBuilds()
    /// @pob Classes/ExtBuildListProvider.lua:57-59 h=e21e58f2
    fn get_builds(self: Rc<Self>, cx: &mut Cx) {
        // L58 return {}
        let _ = cx;
    }

    /// lua: ExtBuildListProvider.lua:61-63 SetImportCode(importCode)
    /// @pob Classes/ExtBuildListProvider.lua:61-63 h=3648309c
    fn set_import_code(&self, importCode: Option<String>) {
        // L62
        self.base().borrow_mut().importCode = importCode;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A provider that counts its `GetBuilds` calls (the base class has no fetch).
    struct Counting {
        base: RefCell<ExtBuildListProvider>,
        calls: RefCell<Vec<Option<String>>>,
    }
    impl ExtBuildListProviderClass for Counting {
        fn base(&self) -> &RefCell<ExtBuildListProvider> {
            &self.base
        }
        fn get_builds(self: Rc<Self>, _cx: &mut Cx) {
            let active = self.base.borrow().activeList.clone();
            self.calls.borrow_mut().push(active);
        }
    }

    #[test]
    fn activate_selects_the_first_title_and_fetches() {
        let mut cx = Cx::headless();
        let p = Rc::new(Counting {
            base: RefCell::new(ExtBuildListProvider::new(Some(vec![
                "Trending".into(),
                "Latest".into(),
            ]))),
            calls: RefCell::new(Vec::new()),
        });
        p.clone().activate(&mut cx);
        assert_eq!(p.get_active_list().as_deref(), Some("Trending"));
        // an unknown title changes nothing; a known one fetches again
        p.clone().set_active_list(&mut cx, Some("Popular"));
        p.clone().set_active_list(&mut cx, Some("Latest"));
        assert_eq!(
            *p.calls.borrow(),
            vec![Some("Trending".to_owned()), Some("Latest".to_owned())]
        );
        // `next(self.listTitles)` of an empty list: no activation
        let q = Rc::new(Counting {
            base: RefCell::new(ExtBuildListProvider::new(Some(Vec::new()))),
            calls: RefCell::new(Vec::new()),
        });
        q.clone().activate(&mut cx);
        assert!(q.calls.borrow().is_empty());
        assert_eq!(q.get_page_url(), None);
        assert_eq!(q.get_active_page_url(), None);
    }

    #[test]
    fn lua_coercions() {
        let s = |v: &str| Some(LVal::Str(v.to_owned()));
        assert_eq!(LVal::concat(Some(&LVal::Num(3.0)), "x"), "3");
        assert_eq!(LVal::check_number(s("12.5").as_ref(), 2, "format"), 12.5);
        assert_eq!(LVal::fmt_s(Some(&LVal::Bool(true))), "true");
        assert!(LVal::lt(&LVal::Num(1.0), &LVal::Num(2.0)));
        assert!(LVal::lt(&LVal::Str("a".into()), &LVal::Str("b".into())));
        let e = pob_lua::error::catch(|| LVal::lt(&LVal::Num(1.0), &LVal::Str("b".into())))
            .expect_err("mixed compare");
        assert_eq!(e.0, "attempt to compare number with string");
    }
}
