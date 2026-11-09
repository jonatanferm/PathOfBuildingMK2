//! Determinism and order patches (ui doc §11.2). Owner: U8.
//!
//! Each patch is a text substitution applied when the module loads; it asserts that the original
//! text is present (so upstream drift fails loudly) and states its reason. Every substitution keeps
//! the file's line count, so the trace `src` locations (`"Classes/X.lua:54"`) stay the upstream
//! line numbers.
//!
//! The eight patches of §11.2 and where each lives:
//!
//! 1. Virtual clock (`GetTime`, `os.time`, `os.clock`, `os.date`): runtime, `GetTime` is bound to
//!    `Sys::time_ms` (sg_bridge) and `lua/determinism.lua` rebinds the `os` functions.
//! 2. `math.randomseed(0)`, wrapped `math.random`: runtime, `lua/determinism.lua`.
//! 3. `launch:CheckForUpdate` is a no-op: text, Launch.lua (below). With no update check running,
//!    `launch.updateAvailable` is never set and the update UI never shows.
//! 4. `GetUserPath` points at the fixture directory: runtime, `GetUserPath` is bound to the oracle
//!    platform (boot.rs, a fresh copy of the fixture user dir per run); text, Launch.lua (below):
//!    PoB runs as a release install (not dev mode), because dev mode would ignore `GetUserPath`.
//! 5. Image loads are synchronous: runtime, the oracle's `Sg` uses pob-host's `HeadlessLoader`.
//! 6. Creation order: text, Common.lua `new()` stamps the sequence (below) and the four
//!    `pairs(self.controls)` sites become `__canonPairs` (below); runtime in `lua/markers.lua`.
//! 7. Control markers around `control:Draw` in the patched DrawControls loops: text (below),
//!    runtime in markers.rs.
//! 8. `Net`/`LaunchSubScript`: runtime, scripted fixtures in sg_bridge (`LaunchSubScript`,
//!    `deliver_subscripts` at the start of the next frame).

/// One source substitution.
#[derive(Clone, Copy, Debug)]
pub struct Patch {
    /// Path relative to PoB's `src/`.
    pub file: &'static str,
    /// Text that must be present exactly once.
    pub find: &'static str,
    pub replace: &'static str,
    pub reason: &'static str,
}

/// Every registered patch (U8 fills it; `// ORDER: (a)` resolutions add theirs here).
pub const PATCHES: &[Patch] = &[
    Patch {
        file: "Launch.lua",
        find: "\tif localManXML and not self.versionBranch and not self.versionPlatform then\n",
        replace: "\tif false and localManXML and not self.versionBranch and not self.versionPlatform then -- pob-ui-oracle patch 4\n",
        reason: "patch 4: the checkout's manifest.xml has no branch/platform, which turns on dev mode; dev mode puts the user data in the script dir and ignores GetUserPath, so the fixture user dir would not be used. The oracle runs PoB as a release install.",
    },
    Patch {
        file: "Launch.lua",
        find: "function launch:CheckForUpdate(inBackground)\n\tif self.updateCheckRunning then\n",
        replace: "function launch:CheckForUpdate(inBackground) do return end -- pob-ui-oracle patch 3\n\tif self.updateCheckRunning then\n",
        reason: "patch 3: no update checks (network, timing); the update UI never shows.",
    },
    Patch {
        file: "Modules/Common.lua",
        find: "\tobject.Object = object\n\tif class._parents then\n",
        replace: "\tobject.Object = object __stampSeq(object) -- pob-ui-oracle patch 6\n\tif class._parents then\n",
        reason: "patch 6 (D7): stamp the creation sequence number of every object made by new(), for the canonical child order.",
    },
    Patch {
        file: "Classes/ControlHost.lua",
        find: "function ControlHostClass:GetMouseOverControl()\n\tfor _, control in pairs(self.controls) do\n",
        replace: "function ControlHostClass:GetMouseOverControl()\n\tfor _, control in __canonPairs(self.controls) do -- pob-ui-oracle patch 6\n",
        reason: "patch 6 (D7): hit-testing iterates the controls in canonical order (integer keys ascending, then creation order).",
    },
    Patch {
        file: "Classes/ControlHost.lua",
        find: "function ControlHostClass:DrawControls(viewPort, selControl)\n\tfor _, control in pairs(self.controls) do\n\t\tif control:IsShown() and control.Draw then\n\t\t\tcontrol:Draw(viewPort, (self.selControl and self.selControl.hasFocus and self.selControl ~= control) or (selControl and selControl.hasFocus and selControl ~= control))\n",
        replace: "function ControlHostClass:DrawControls(viewPort, selControl)\n\tfor __key, control in __canonPairs(self.controls) do -- pob-ui-oracle patches 6, 7\n\t\tif control:IsShown() and control.Draw then\n\t\t\t__tracePush(self, __key) control:Draw(viewPort, (self.selControl and self.selControl.hasFocus and self.selControl ~= control) or (selControl and selControl.hasFocus and selControl ~= control)) __tracePop()\n",
        reason: "patches 6 and 7 (D7): draw in canonical order, with the control-path marker <HostClass>.<key> around each Draw.",
    },
    Patch {
        file: "Modules/Main.lua",
        find: "\t\t\tfor id, control in pairs(self.controls) do\n\t\t\t\t-- always draw static UI elements\n\t\t\t\tif control:IsShown() and control.Draw and controlIsInsideScrollClip(self, control) then\n\t\t\t\t\tcontrol:Draw(viewPort, (self.selControl and self.selControl.hasFocus and self.selControl ~= control) or nil)\n",
        replace: "\t\t\tfor id, control in __canonPairs(self.controls) do -- pob-ui-oracle patches 6, 7\n\t\t\t\t-- always draw static UI elements\n\t\t\t\tif control:IsShown() and control.Draw and controlIsInsideScrollClip(self, control) then\n\t\t\t\t\t__tracePush(self, id) control:Draw(viewPort, (self.selControl and self.selControl.hasFocus and self.selControl ~= control) or nil) __tracePop()\n",
        reason: "patches 6 and 7 (D7): the scrolling popup's DrawControls override (Main.lua:1298) draws in canonical order, with markers.",
    },
    Patch {
        file: "Modules/Main.lua",
        find: "\t\tpopup.GetMouseOverControl = function(self)\n\t\t\tfor _, control in pairs(self.controls) do\n",
        replace: "\t\tpopup.GetMouseOverControl = function(self)\n\t\t\tfor _, control in __canonPairs(self.controls) do -- pob-ui-oracle patch 6\n",
        reason: "patch 6 (D7): the scrolling popup's GetMouseOverControl override (Main.lua:1308) hit-tests in canonical order.",
    },
];

/// The files that have patches (the loader hook only converts these sources).
pub fn patched_files() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = PATCHES.iter().map(|p| p.file).collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Apply the patches registered for `file` to its source; a missing anchor is an error.
pub fn apply(file: &str, src: &str) -> Result<String, String> {
    let file = file.replace('\\', "/");
    let mut out = src.to_owned();
    for p in PATCHES.iter().filter(|p| p.file == file) {
        let n = out.matches(p.find).count();
        if n != 1 {
            return Err(format!(
                "pob-ui-oracle: patch anchor for {} found {n} times (expected exactly 1; upstream drift?): {:?} [{}]",
                p.file, p.find, p.reason
            ));
        }
        out = out.replacen(p.find, p.replace, 1);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_patch_keeps_the_line_count() {
        for p in PATCHES {
            assert_eq!(
                p.find.matches('\n').count(),
                p.replace.matches('\n').count(),
                "{}: {}",
                p.file,
                p.reason
            );
            assert!(!p.reason.is_empty());
        }
    }

    /// Every patch's anchor is present exactly once in the pinned upstream file (v2.67.2), and the
    /// patched file still compiles as Lua with the same line count.
    #[test]
    fn every_patch_applies_to_upstream() {
        let root =
            crate::boot::locate_pob_root().expect("PoB checkout (.PathOfBuilding or POB_ROOT)");
        let lua = mlua::Lua::new();
        for file in patched_files() {
            let src = std::fs::read_to_string(root.join("src").join(file)).unwrap();
            let out = apply(file, &src).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(src.lines().count(), out.lines().count(), "{file}");
            for p in PATCHES.iter().filter(|p| p.file == file) {
                assert!(out.contains(p.replace), "{file}: {}", p.reason);
            }
            // Compound assignments (`+=`) are rewritten by the prelude's loader, not here.
            if !src.contains("+=") && !src.contains("-=") {
                lua.load(out.as_str())
                    .set_name(file)
                    .into_function()
                    .unwrap_or_else(|e| panic!("{file}: {e}"));
            }
        }
    }

    #[test]
    fn missing_anchor_is_an_error() {
        let e = apply("Launch.lua", "-- not Launch.lua\n").unwrap_err();
        assert!(e.contains("found 0 times"), "{e}");
        // An anchor present twice is ambiguous: also an error.
        let p = PATCHES[0];
        let twice = format!("{}{}", p.find, p.find);
        let e = apply(p.file, &twice).unwrap_err();
        assert!(e.contains("found 2 times"), "{e}");
        assert_eq!(apply("Classes/Other.lua", "x").unwrap(), "x");
    }
}
