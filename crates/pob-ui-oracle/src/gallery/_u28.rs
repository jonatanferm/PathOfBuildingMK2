//! Support module of the U28 galleries (`u28_*`). Owner: U28.

use std::cell::RefCell;

use pob_lua::format::tostring_num;

/// The galleries' `event(s)`: the last four recorded calls.
pub fn push_event(events: &RefCell<Vec<String>>, s: String) {
    let mut e = events.borrow_mut();
    e.push(s);
    if e.len() > 4 {
        e.remove(0);
    }
}

/// Lua `tostring(index)` of an optional index (`"nil"` for nil).
pub fn tostring_opt(v: Option<usize>) -> String {
    v.map_or_else(|| "nil".to_owned(), |i| tostring_num(i as f64))
}

/// `data.powerStatList` for a gallery: the dump's `powerStatList.json` (the Lua side runs
/// Modules/Data.lua:129-235 itself). Loaded once per process and leaked (`&'static`, like
/// `pob_gamedata::data()`).
pub fn power_stat_list() -> &'static pob_app::tabs::tree::notable_db::PowerStatList {
    thread_local! {
        static LIST: std::cell::OnceCell<&'static pob_app::tabs::tree::notable_db::PowerStatList> =
            const { std::cell::OnceCell::new() };
    }
    LIST.with(|l| {
        *l.get_or_init(|| {
            let dir = pob_testpaths::dump_dir()
                .expect("data/dump (powerStatList.json) not found: set POB_DUMP_DIR");
            let path = dir.join("powerStatList.json");
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            Box::leak(Box::new(
                serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
            ))
        })
    })
}
