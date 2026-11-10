# CCR-0095: U17 seams to the tree assets and to `listMode:BuildList`

Unit: **U17** (Main popups). Status: **pending** (code on `port/U17` follows it).

U17's range reads two things later units own and the frozen surface does not expose yet. U17
works around both locally (marked `// CCR-0095` in its own files) and proposes the seams below.

## A. `main.tree[latestTreeVersion].assets.Background2/Background1` (main:DrawBackground)

Evidence: `Main.lua:1486-1495` (`self.tree[latestTreeVersion].assets.Background2.handle`, else
`.Background1.handle`). The handles are made by `PassiveTree.lua:263-266`
(`self:LoadImage(name..".png", ..., data, "ASYNC")`, `:909-935`), which the calc port skips as UI
(`pob-calc/src/classes/passive_tree.rs:653`, `@pob-skip PassiveTree.lua:234-435`, "U23
tree_view/assets.rs"). `pob-app/src/tree_view/assets.rs` (U23) declares `TreeAssets {}` with no
fields, and `MainHooks::load_tree` is a no-op (CCR-0091 B), so `cx.app().trees` is empty when
BuildList.lua:179 and every tab's Draw call `main:DrawBackground`.

Work-around (`pob-app/src/main_mode/background.rs`, `tree_background`): load the image Lua loads
for the latest tree: `TreeData/Background2.png` with `ASYNC` (3_29 has no `assets` of its own and
copies `TreeData/3_19/Assets.lua`, which has `Background2`; `LoadImage` finds the file at the top
of `TreeData/`). Registration is deduplicated by `(path, flags)`, so the per-call load is a lookup
and the draw ops equal the Lua's (verified: `tests/u17_popups.rs`, every scenario draws it).

Proposed: U23 gives `TreeAssets` the asset table and an accessor, and replaces the body of
`background::tree_background` with the lookup:

```diff
 // pob-app/src/tree_view/assets.rs (U23)
-pub struct TreeAssets {}
+pub struct TreeAssets { /* ... */ pub assets: IndexMap<String, AssetImage> }
+pub struct AssetImage { pub handle: ImageHandle, pub width: f64, pub height: f64 }
+impl TreeAssets { pub fn asset_handle(&self, name: &str) -> Option<ImageHandle>; }

 // pob-app/src/main_mode/background.rs (U17 file, U23 may edit this one function)
 pub fn tree_background(cx: &mut Cx) -> (Option<ImageHandle>, Option<ImageHandle>) {
-    let handle = ImageHandle::default();
-    handle.load(&mut cx.sg, "TreeData/Background2.png", ImageFlags::ASYNC);
-    (Some(handle), None)
+    let latest = cx.data().gameVersions.latestTreeVersion.to_string();
+    let Some((_, assets)) = cx.app().trees.trees.get(&latest) else {
+        lua_error!("attempt to index field '?' (a nil value)")
+    };
+    (assets.asset_handle("Background2"), assets.asset_handle("Background1"))
 }
```

## B. `self.modes.LIST:BuildList()` (Options' Save, Main.lua:1218-1220)

Evidence: `Main.lua:1218-1220` (`if self.mode == "LIST" then self.modes.LIST:BuildList() end`);
`BuildList.lua` `listMode:BuildList()` (U18). The `Mode` trait of CCR-0091 A
(`pob-app/src/main_mode/mod.rs`, U16) has no such method, and a `Rc<dyn Mode>` cannot be
downcast to U18's object.

Work-around (`pob-app/src/main_mode/options_popup.rs`, Save): the call is skipped (the stand-in
`PendingMode` has no list to rebuild). The UI oracle test gives the Lua stand-in mode a no-op
`BuildList`, so both sides agree.

Proposed diff to `pob-app/src/main_mode/mod.rs` (U16's file; U18 implements it):

```diff
 pub trait Mode {
     ...
+    /// `listMode:BuildList()` (BuildList.lua; Main.lua:1219 calls it on `main.modes.LIST`).
+    fn build_list(&self, cx: &mut Cx) {
+        let _ = cx;
+        lua_error!("attempt to call method 'BuildList' (a nil value)")
+    }
 }
```

and in `options_popup.rs` (U17):

```diff
             if cx.app().main.mode.as_deref() == Some("LIST") {
-                // L1219 self.modes.LIST:BuildList(): CCR-0095 B
+                // L1219
+                let list = cx.app().main.modes.get("LIST").cloned()
+                    .unwrap_or_else(|| lua_error!("attempt to index field 'LIST' (a nil value)"));
+                list.build_list(cx);
             }
```

(`PendingMode` would override `build_list` with a no-op until U18 lands.)

## C. Notes (no change requested)

- `main.invertSliderScrollDirection`: the Options checkbox and Cancel write both
  `MainState::invert_slider_scroll_direction` and SliderControl's stand-in thread-local
  `INVERT_SLIDER_SCROLL_DIRECTION` (CCR-0069 A). U16's LoadSettings writes only the former, so a
  value loaded from Settings.xml does not reach SliderControl until something syncs it (U16 /
  CCR-0069 follow-up).
- `MainHooks` (CCR-0091 B): U17 installed its three entries in `init::default_hooks` (the one-line
  change CCR-0091 B foresees).
