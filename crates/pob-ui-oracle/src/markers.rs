//! Control-path markers (ui doc §11.2 patch 7): `__tracePush(<HostClass>.<key>)` before
//! `control:Draw` and `__tracePop()` after, inside the patched DrawControls loop; Rust emits the
//! same path from `Cx::draw_controls`. Owner: U8.
//!
//! Also the runtime half of patch 6 (creation order, D7): `__stampSeq` (called from the patched
//! `new()`), `__canonPairs` (the patched `pairs(self.controls)` sites) and the audit mode. The Lua
//! side is `lua/markers.lua`; the Rust side registers `__ui_ctl_push` / `__ui_ctl_pop`, which
//! forward to `Sg::trace_ctl_push` / `trace_ctl_pop`.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::Lua;

const MARKERS_LUA: &str = include_str!("../lua/markers.lua");

/// Register `__tracePush` / `__tracePop` / `__canonPairs` in `lua`. Needs the SG bridge
/// ([`crate::sg_bridge::install`]) for the trace recorder of `sg`.
pub fn install(lua: &Lua, sg: Rc<RefCell<pob_sg::Sg>>) -> mlua::Result<()> {
    // The functions reach `sg` through the bridge registry (they must be Send); `sg` is the one the
    // bridge holds.
    let b = crate::sg_bridge::bridge(lua)?;
    if !Rc::ptr_eq(&b.sg, &sg) {
        return Err(mlua::Error::runtime(
            "markers::install: sg is not the bridge's Sg",
        ));
    }
    let g = lua.globals();
    g.set(
        "__ui_ctl_push",
        lua.create_function(|lua, elem: mlua::String| {
            let b = crate::sg_bridge::bridge(lua)?;
            b.sg.borrow_mut().trace_ctl_push(&elem.to_string_lossy());
            Ok(())
        })?,
    )?;
    g.set(
        "__ui_ctl_pop",
        lua.create_function(|lua, ()| {
            let b = crate::sg_bridge::bridge(lua)?;
            b.sg.borrow_mut().trace_ctl_pop();
            Ok(())
        })?,
    )?;
    lua.load(MARKERS_LUA)
        .set_name("@pob-ui-oracle/lua/markers.lua")
        .exec()
}

/// Switch the audit mode on: patch 6's canonical order is disabled (`__canonPairs` yields the
/// native `pairs` order) and every drawn control is recorded for [`audit_end_frame`].
pub fn enable_audit(lua: &Lua) -> mlua::Result<()> {
    lua.globals().set("__oracleAudit", true)
}

/// Close one audited frame (compare native and canonical draw order per host).
pub fn audit_end_frame(lua: &Lua) -> mlua::Result<()> {
    let f: mlua::Function = lua.globals().get("__auditEndFrame")?;
    f.call::<()>(())
}

/// One host whose native draw order put two overlapping controls in the other order than the
/// canonical one.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AuditHit {
    /// `<parent control path>/<HostClass>`.
    pub host: String,
    /// Inverted overlapping pairs seen (over all frames).
    pub count: u64,
    /// One pair, `"<canonical first> < <native first>"`.
    pub example: String,
}

/// The hosts recorded so far.
pub fn audit_hits(lua: &Lua) -> mlua::Result<Vec<AuditHit>> {
    let t: mlua::Table = lua.globals().get("__auditHits")?;
    let mut out = Vec::new();
    for pair in t.pairs::<String, mlua::Table>() {
        let (host, hit) = pair?;
        out.push(AuditHit {
            host,
            count: hit.get::<f64>("count")? as u64,
            example: hit.get("example")?,
        });
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::MARKERS_LUA;

    fn vm() -> mlua::Lua {
        let lua = mlua::Lua::new();
        lua.load(
            "__pushed = {} function __ui_ctl_push(e) __pushed[#__pushed + 1] = e end function __ui_ctl_pop() end",
        )
        .exec()
        .unwrap();
        lua.load(MARKERS_LUA).exec().unwrap();
        lua
    }

    /// Patch 6: integer keys ascending, then the other keys by creation sequence; a key removed
    /// before it is reached is skipped.
    #[test]
    fn canon_pairs_order() {
        let lua = vm();
        let order: String = lua
            .load(
                r#"
                local host = { controls = { } }
                local function mk(name) local o = { name = name } __stampSeq(o) return o end
                local z, a, m = mk("z"), mk("a"), mk("m")
                host.controls.zeta = z
                host.controls.alpha = a
                host.controls[3] = mk("3")
                host.controls[1] = mk("1")
                host.controls.mid = m
                local out = { }
                for k, v in __canonPairs(host.controls) do
                    out[#out + 1] = v.name
                    if k == 1 then host.controls.alpha = nil end
                end
                return table.concat(out, ",")
            "#,
            )
            .eval()
            .unwrap();
        // 1, 3, then z (seq 1), [a removed], m (seq 3).
        assert_eq!(order, "1,3,z,m");
    }

    /// Patch 7 markers and the audit mode: overlapping controls drawn in a native order that is not
    /// the canonical one are reported for their host; controls that do not overlap are not.
    #[test]
    fn audit_reports_inverted_overlaps() {
        let lua = vm();
        let n: i64 = lua
            .load(
                r#"
                __oracleAudit = true
                local host = { _className = "TestHost", controls = { } }
                for i = 1, 8 do
                    local o = { }
                    function o:GetPos() return 10, 10 end
                    function o:GetSize() return 50, 20 end
                    host.controls["c" .. i] = o
                end
                -- Creation order = the reverse of the native order, so canonical != native.
                local nat = { }
                for k in pairs(host.controls) do nat[#nat + 1] = k end
                for i = #nat, 1, -1 do __stampSeq(host.controls[nat[i]]) end
                -- Draw in native order (what the audited DrawControls loop does).
                for k in __canonPairs(host.controls) do __tracePush(host, k) __tracePop() end
                __auditEndFrame()
                local hit = __auditHits["TestHost"]
                return hit and hit.count or 0
            "#,
            )
            .eval()
            .unwrap();
        // Every one of the 8 * 7 / 2 pairs is inverted and overlapping.
        assert_eq!(n, 28);
        let pushed: String = lua.load("return __pushed[1]").eval().unwrap();
        assert!(pushed.starts_with("TestHost.c"), "{pushed}");
        let n: i64 = lua
            .load(
                r#"
                local host = { _className = "Apart", controls = { } }
                for i = 1, 24 do
                    local o = { }
                    __stampSeq(o)
                    function o:GetPos() return i * 100, 0 end
                    function o:GetSize() return 50, 20 end
                    host.controls["c" .. i] = o
                end
                for k in __canonPairs(host.controls) do __tracePush(host, k) __tracePop() end
                __auditEndFrame()
                local hit = __auditHits["Apart"]
                return hit and hit.count or 0
            "#,
            )
            .eval()
            .unwrap();
        assert_eq!(n, 0);
    }
}
