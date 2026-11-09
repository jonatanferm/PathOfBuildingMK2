//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/SearchHost.lua 1-153
//! (a mixin class: its state is a field of the host widget). Owner: U13.
//!
//! @pob-file Classes/SearchHost.lua @16de4b82d
//!
//! # Surface (CCR-0082 C)
//!
//! - `listAccessor()` and `valueAccessor(value)` (L10) are one Rust closure, [`ListAccessor`]:
//!   it returns the list already mapped through the value accessor (the strings `matchWords`
//!   searches). Both Lua functions are pure, so composing them before `matchTerm` instead of
//!   per entry inside `matchWords` is unobservable. `nil` (no accessor) is `None`; DropDownControl
//!   is the only SearchHost in PoB (grep `SearchHost` over `src/`) and always passes both.
//! - The methods that read the accessor need the context: [`SearchHost::on_search_char`],
//!   [`SearchHost::on_search_key_down`] and [`SearchHost::update_search`] take `(cx, me, get)`,
//!   where `get` projects the host widget's `SearchHost` field. `UpdateSearch` is public: ItemsTab
//!   and AddImplicitPopup call it on dropdowns (ItemsTab.lua:2906, :3287, :3567,
//!   AddImplicitPopup.lua:272, :284).
//! - `searchInfos` is a hole-free array (`matchTerm` fills every index), so it is a `Vec`;
//!   a lookup past its end is Lua's `nil` (`Vec::get`). Ranges are 1-based inclusive byte
//!   positions, as `string.find` returns them.
//! - `matchCount` is nil until the first `UpdateSearch`; it is only read while a search is
//!   active, which implies an `UpdateSearch` ran (only `OnSearchChar` makes the term non-empty),
//!   so it is a `usize` starting at 0.
//! - The search term only ever grows by `OnChar` key names (ASCII, `Key::name`), so the
//!   patterns built from it are ASCII; the matching itself is byte-exact (`pob_lua::pattern`).
//! - The stub's `last_search_time` has no Lua counterpart and is gone.

use std::rc::Rc;

use pob_lua::math::m_max;
use pob_lua::pattern::{find_bytes, gmatch_bytes, gsub_bytes_with, match_bytes, Repl};
use pob_sg::Key;

use crate::prelude::*;

/// `listAccessor()` composed with `valueAccessor(value)` (SearchHost.lua:10, :43).
pub type ListAccessor = Rc<dyn Fn(&Cx, CtrlId) -> Vec<String>>;

/// `{ from = from, to = to }` (SearchHost.lua:49): 1-based inclusive byte positions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchRange {
    pub from: usize,
    pub to: usize,
}

/// `{ ranges = {}, matches = true }` (SearchHost.lua:44).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchInfo {
    pub ranges: Vec<SearchRange>,
    pub matches: bool,
}

// @pob Classes/SearchHost.lua:1-8 h=da983594 (header and newClass: the struct below)
#[derive(Default)]
pub struct SearchHost {
    /// `self.searchListAccessor` (with `self.valueAccessor` applied, see the module docs)
    pub list_accessor: Option<ListAccessor>,
    /// `self.ignoreOrder`
    pub ignore_order: bool,
    /// `self.searchTerm`
    pub search_term: String,
    /// `self.searchInfos`
    pub search_infos: Vec<SearchInfo>,
    /// `self.matchCount`
    pub match_count: usize,
}

/// `local function splitWords(s)`
/// @pob Classes/SearchHost.lua:19-25 h=533d86d7
fn split_words(s: &[u8]) -> Vec<Vec<u8>> {
    // L20
    let mut words = Vec::new();
    // L21
    for word in gmatch_bytes(s, b"%S+") {
        // L22
        words.push(word[0].as_bytes().to_vec());
    }
    // L24
    words
}

/// `local function letterToCaselessPattern(c)`
/// @pob Classes/SearchHost.lua:27-29 h=8c19483e
fn letter_to_caseless_pattern(c: &[u8]) -> Vec<u8> {
    // L28 string.format("[%s%s]", string.lower(c), string.upper(c))
    let mut out = Vec::with_capacity(4);
    out.push(b'[');
    out.extend(c.iter().map(u8::to_ascii_lowercase));
    out.extend(c.iter().map(u8::to_ascii_uppercase));
    out.push(b']');
    out
}

/// `local function wordsToCaselessPatterns(words)`
/// @pob Classes/SearchHost.lua:31-40 h=01989a81
fn words_to_caseless_patterns(words: &[Vec<u8>]) -> Vec<Vec<u8>> {
    // L32
    let mut patterns = Vec::with_capacity(words.len());
    // L33
    for word in words {
        // L34 escape all non alphanumeric chars
        // L35
        let escaped = gsub_bytes_with(word, b"%W", Repl::Str(b"%%%1"), None).0;
        // L36 make pattern case insensitive
        // L37
        let mut f =
            |c: &pob_lua::pattern::Captures<'_>| Some(letter_to_caseless_pattern(c[0].as_bytes()));
        patterns.push(gsub_bytes_with(&escaped, b"%a", Repl::Func(&mut f), None).0);
    }
    // L39
    patterns
}

/// `local function matchWords(searchWords, entry, valueAccessor, ignoreOrder)`; `value` is
/// `valueAccessor(entry)` (L43, done by the accessor).
/// @pob Classes/SearchHost.lua:42-79 h=d19eaa2b
fn match_words(search_words: &[Vec<u8>], value: &str, ignore_order: bool) -> SearchInfo {
    let value = value.as_bytes();
    // L44
    let mut search_info = SearchInfo {
        ranges: Vec::new(),
        matches: true,
    };
    // L45
    let mut last_match_end = 0usize;
    // L46
    for word in search_words {
        // L47
        let found = find_bytes(value, word, Some(last_match_end as i64 + 1), false);
        // L48
        if let Some((from, to, _)) = found {
            // L49
            let range = SearchRange { from, to };
            // L50
            search_info.ranges.push(range);
            // L51
            if !ignore_order {
                // L52
                last_match_end = to;
            }
        } else {
            // L55 at least one search word did not match at least once (respecting order)
            // L56
            search_info.matches = false;
        }
    }
    // L59
    if ignore_order {
        // L60 sort to be in left to right order
        // L61
        pob_lua::sort(&mut search_info.ranges, |a, b| a.from < b.from);
        // L64 merge overlapping ranges
        // L65
        let mut i = 0usize;
        // L66
        while i < search_info.ranges.len() {
            // L67-L68
            let next = search_info.ranges.get(i + 1).copied();
            // L69
            if let Some(next) = next.filter(|n| n.from <= search_info.ranges[i].to) {
                // L70
                let this = &mut search_info.ranges[i];
                this.to = m_max(this.to as f64, next.to as f64) as usize;
                // L71
                search_info.ranges.remove(i + 1);
                // L72 Check this range again because another range may overlap it.
            } else {
                // L74
                i += 1;
            }
        }
    }
    // L78
    search_info
}

/// `local function matchTerm(searchTerm, list, valueAccessor, ignoreOrder)`
/// @pob Classes/SearchHost.lua:81-92 h=b402c6ce
fn match_term(search_term: &str, list: Option<&[String]>, ignore_order: bool) -> Vec<SearchInfo> {
    // L82
    let Some(list) = list.filter(|_| !search_term.is_empty()) else {
        // L83
        return Vec::new();
    };
    // L86 (filled at every index below)
    // L87
    let search_patterns = words_to_caseless_patterns(&split_words(search_term.as_bytes()));
    // L88-L90
    list.iter()
        .map(|entry| match_words(&search_patterns, entry, ignore_order))
        .collect()
    // L91
}

/// `s:match(p)` is not nil.
fn matches(s: &[u8], p: &[u8]) -> bool {
    match_bytes(s, p, None).is_some()
}

impl SearchHost {
    /// lua: SearchHost.lua:10-17 SearchHost(listAccessor, valueAccessor, ignoreOrder)
    /// @pob Classes/SearchHost.lua:10-17 h=e68cf139
    pub fn new(list_accessor: ListAccessor, ignore_order: bool) -> SearchHost {
        SearchHost {
            // L11-L12
            list_accessor: Some(list_accessor),
            // L13
            search_term: String::new(),
            // L14
            search_infos: Vec::new(),
            // L15 `ignoreOrder or false`
            ignore_order,
            match_count: 0,
        }
    }

    /// lua: SearchHost.lua:94-96
    /// @pob Classes/SearchHost.lua:94-96 h=ea1a1619
    pub fn is_search_active(&self) -> bool {
        // L95
        !self.search_term.is_empty()
    }

    /// lua: SearchHost.lua:98-110 OnSearchChar(char); returns `self` (always `true`).
    /// @pob Classes/SearchHost.lua:98-110 h=b20b3907
    pub fn on_search_char(
        cx: &mut Cx,
        me: CtrlId,
        get: fn(&mut Cx, CtrlId) -> &mut SearchHost,
        ch: Key,
    ) -> bool {
        let ch = ch.name();
        let ch = ch.as_bytes();
        // L99
        if matches(ch, b"%s") {
            // L100 don't allow space char if search is empty or last character is already a space char
            let term = get(cx, me).search_term.as_bytes();
            // L101
            if term.is_empty() || matches(&term[term.len() - 1..], b"%s") {
                // L102
                return true;
            }
        }
        // L105
        if !matches(ch, b"%c") {
            // L106
            get(cx, me)
                .search_term
                .push_str(std::str::from_utf8(ch).expect("Key::name is ASCII"));
            // L107
            Self::update_search(cx, me, get);
        }
        // L109
        true
    }

    /// lua: SearchHost.lua:112-121 OnSearchKeyDown(key); `true` = `return self`.
    /// @pob Classes/SearchHost.lua:112-121 h=e4983e83
    pub fn on_search_key_down(
        cx: &mut Cx,
        me: CtrlId,
        get: fn(&mut Cx, CtrlId) -> &mut SearchHost,
        key: Key,
    ) -> bool {
        let active = get(cx, me).is_search_active();
        // L113
        if active && key == Key::Escape {
            // L114
            get(cx, me).reset_search();
            // L115
            return true;
        } else if active && key == Key::Back {
            // L117 self.searchTerm:sub(1, -2)
            let h = get(cx, me);
            h.search_term.pop();
            // L118
            Self::update_search(cx, me, get);
            // L119
            return true;
        }
        false
    }

    /// lua: SearchHost.lua:123-131
    /// @pob Classes/SearchHost.lua:123-131 h=3143979a
    pub fn update_match_count(&mut self) {
        // L124
        let mut match_count = 0;
        // L125
        for info in &self.search_infos {
            // L126
            if info.matches {
                // L127
                match_count += 1;
            }
        }
        // L130
        self.match_count = match_count;
    }

    /// lua: SearchHost.lua:133-135
    /// @pob Classes/SearchHost.lua:133-135 h=5c2cb5a7
    pub fn get_match_count(&self) -> usize {
        // L134
        self.match_count
    }

    /// lua: SearchHost.lua:137-142
    /// @pob Classes/SearchHost.lua:137-142 h=d1045ae0
    pub fn update_search(cx: &mut Cx, me: CtrlId, get: fn(&mut Cx, CtrlId) -> &mut SearchHost) {
        // L138
        let Some(accessor) = get(cx, me).list_accessor.clone() else {
            return;
        };
        // L139
        let list = accessor(cx, me);
        let h = get(cx, me);
        h.search_infos = match_term(&h.search_term, Some(&list), h.ignore_order);
        // L140
        h.update_match_count();
    }

    /// lua: SearchHost.lua:144-148
    /// @pob Classes/SearchHost.lua:144-148 h=5c6f1696
    pub fn reset_search(&mut self) {
        // L145
        self.search_term.clear();
        // L146
        self.match_count = 0;
        // L147
        self.search_infos = Vec::new();
    }

    /// lua: SearchHost.lua:150-153
    /// @pob Classes/SearchHost.lua:150-153 h=cbcca269
    pub fn get_search_term_pretty(&self) -> String {
        // L151
        let color = if self.is_search_active() && self.match_count > 0 {
            "^xFFFFFF"
        } else {
            "^xFF0000"
        };
        // L152
        format!("{color}{}", self.search_term)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(from: usize, to: usize) -> SearchRange {
        SearchRange { from, to }
    }

    fn info(term: &str, value: &str, ignore_order: bool) -> SearchInfo {
        match_term(term, Some(&[value.to_owned()]), ignore_order).remove(0)
    }

    #[test]
    fn caseless_patterns_escape_non_alphanumerics() {
        let p = words_to_caseless_patterns(&split_words(b"  a.b  C-1 "));
        assert_eq!(p, vec![b"[aA]%.[bB]".to_vec(), b"[cC]%-1".to_vec()]);
    }

    #[test]
    fn ordered_matching_continues_after_the_previous_match() {
        // "fire" then "re": the second word is searched after the first match's end
        let i = info("fire re", "Fireball of Rebirth", false);
        assert!(i.matches);
        assert_eq!(i.ranges, vec![r(1, 4), r(13, 14)]);
        // the order matters: "re" first consumes "re" of "Fire", then "fire" has nothing left
        let i = info("re fire", "Fireball", false);
        assert!(!i.matches);
        assert_eq!(i.ranges, vec![r(3, 4)]);
    }

    #[test]
    fn ignore_order_sorts_and_merges_ranges() {
        let i = info("ball fire ireb", "Fireball", true);
        assert!(i.matches);
        // ranges (5,8) (1,4) (2,5) sorted then merged into one
        assert_eq!(i.ranges, vec![r(1, 8)]);
        let i = info("all x", "Fireball", true);
        assert!(!i.matches);
        assert_eq!(i.ranges, vec![r(6, 8)]);
    }

    #[test]
    fn empty_term_gives_no_infos() {
        assert!(match_term("", Some(&["a".to_owned()]), false).is_empty());
        assert!(match_term("a", None, false).is_empty());
        // only whitespace: no words, every entry matches with no ranges
        assert_eq!(
            info(" ", "abc", false),
            SearchInfo {
                ranges: vec![],
                matches: true
            }
        );
    }

    struct Host {
        sh: SearchHost,
        list: Vec<String>,
    }
    impl Widget for Host {
        const NAME: &'static str = "Host";
        const CAPS: Caps = Caps::empty();
    }
    fn get(cx: &mut Cx, me: CtrlId) -> &mut SearchHost {
        &mut cx.w_mut::<Host>(me).sh
    }

    #[test]
    fn typing_back_and_escape() {
        let mut cx = Cx::headless();
        let sh = SearchHost::new(Rc::new(|cx, me| cx.w::<Host>(me).list.clone()), false);
        let me = cx
            .add_free(
                None,
                New::new(
                    Base::new(Anchor::NONE, rect(0, 0, 0, 0)),
                    Host {
                        sh,
                        list: vec!["Alpha".into(), "Beta".into(), "Gamma".into()],
                    },
                ),
            )
            .id();
        // a leading space is refused
        assert!(SearchHost::on_search_char(
            &mut cx,
            me,
            get,
            Key::Char(b' ')
        ));
        assert_eq!(get(&mut cx, me).search_term, "");
        SearchHost::on_search_char(&mut cx, me, get, Key::Char(b'a'));
        assert_eq!(get(&mut cx, me).get_match_count(), 3);
        SearchHost::on_search_char(&mut cx, me, get, Key::Char(b' '));
        // a second space is refused
        SearchHost::on_search_char(&mut cx, me, get, Key::Char(b' '));
        // a control character is ignored
        SearchHost::on_search_char(&mut cx, me, get, Key::Char(b'\t'));
        SearchHost::on_search_char(&mut cx, me, get, Key::Char(b'm'));
        assert_eq!(get(&mut cx, me).search_term, "a m");
        assert_eq!(get(&mut cx, me).get_match_count(), 1);
        assert_eq!(get(&mut cx, me).get_search_term_pretty(), "^xFFFFFFa m");
        SearchHost::on_search_char(&mut cx, me, get, Key::Char(b'z'));
        assert_eq!(get(&mut cx, me).get_match_count(), 0);
        assert_eq!(get(&mut cx, me).get_search_term_pretty(), "^xFF0000a mz");
        assert!(SearchHost::on_search_key_down(&mut cx, me, get, Key::Back));
        assert_eq!(get(&mut cx, me).search_term, "a m");
        assert!(!SearchHost::on_search_key_down(&mut cx, me, get, Key::Up));
        assert!(SearchHost::on_search_key_down(
            &mut cx,
            me,
            get,
            Key::Escape
        ));
        assert!(!get(&mut cx, me).is_search_active());
        assert!(get(&mut cx, me).search_infos.is_empty());
        // inactive: ESCAPE and BACK are not consumed
        assert!(!SearchHost::on_search_key_down(
            &mut cx,
            me,
            get,
            Key::Escape
        ));
        assert!(!SearchHost::on_search_key_down(&mut cx, me, get, Key::Back));
    }
}
