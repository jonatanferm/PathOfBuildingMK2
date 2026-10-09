# pob-ui-lua

SCOPE: PoB UI at /home/ferm/Work/projects/PathOfBuildingMK2/.PathOfBuilding/src. This is upstream PathOfBuildingCommunity v2.67.2, not a fork (git origin PathOfBuildingCommunity/PathOfBuilding, HEAD 16de4b82d). It uses the newer two-step constructor style, new("X"):X(args). Classes/*.lua is 46,850 lines in 74 files. About 30k of those are UI: base controls about 4.4k, specialised list controls about 2.5k, tabs 19.7k, PassiveTreeView 1,856. On top of that come Launch.lua (416), Modules/Main.lua (1,822), Modules/Build.lua (2,335), BuildList.lua (220) and ToastNotification.lua (254). Trade UI adds about 5k (TradeQuery 1415, TradeQueryGenerator 1609, TradeQueryRequests 515, RateLimiter 285, TradeHelpers 623, CompareBuySimilar 538).

TAB SIZES: TreeTab 2888, SkillsTab 1502, ItemsTab 5055, CalcsTab 773, ConfigTab 1362 (also defines CustomModBlockControl), ImportTab 1932, NotesTab 112, PartyTab 1042, CompareTab 5036 (plus CompareEntry 552 and CompareCalcsHelpers 476).

UI DATA MODULES: Modules/CalcSections.lua (2638) is pure data (only one function) and uses format strings like "{0:mod:1,2}" and "{output:X}", expanded by Modules/CalcFormat.lua (74) through Lua gsub. Modules/ConfigOptions.lua (2380) has 1043 vars and 531 `apply = function(val, modList, enemyModList)` closures, so it is calc logic, not UI. Modules/BuildDisplayStats.lua (293) has 99 functions.

CLASS SYSTEM (Modules/Common.lua:83 newClass, :158 new):
- Multiple inheritance via __index over the parents, copying methods onto the instance (rawset).
- `obj.Object = obj`; the constructor must return self.
- Mixin call syntax `new("X"):X(...) { shown = fn, width = fn }` (class __call copies fields).
- Duck-typed is-a checks, e.g. `selControl.ListControl` (ItemSlotControl.lua:64).

HIERARCHY (from newClass declarations)
- Base: Control (Control.lua 178); ControlHost (115); TooltipHost (27); UndoHandler (58); SearchHost (153); Tooltip (698, standalone).
- Leaf controls: LabelControl(Control) 24, RectangleOutlineControl(Control) 23, SectionControl(Control) 32, ScrollBarControl(Control) 327, ButtonControl(Control,TooltipHost) 130, CheckBoxControl(Control,TooltipHost) 117, SliderControl(Control,TooltipHost) 185, DraggerControl(Control,TooltipHost) 146.
- Containers:
  - EditControl(ControlHost,Control,UndoHandler,TooltipHost) 761: owns scrollBarH/V and +/- or x buttons. ResizableEditControl(EditControl) 50.
  - ListControl(Control,ControlHost) 493: abstract, with hooks GetRowValue, GetRowIcon, AddValueTooltip, GetDragValue, CanReceiveDrag, ReceiveDrag, OnSelect, OnSelClick, OnSelCopy, OnSelDelete, OnSelKeyDown, OnOrderChange, OverrideSelectIndex, OnHoverKeyUp.
  - TextListControl(Control,ControlHost) 122.
  - DropDownControl(Control,ControlHost,TooltipHost,SearchHost) 573: owns a scrollBar; the dropped list draws at sublayer 5.
  - PathControl(Control,ControlHost,UndoHandler) 110.
  - PopupDialog(ControlHost,Control) 96.
- ListControl subclasses: BuildListControl, FolderListControl, ItemListControl, ItemDBControl, ItemSetListControl, SharedItemListControl, SharedItemSetListControl, SkillListControl, SkillSetListControl, ConfigSetListControl, PassiveSpecListControl, PassiveMasteryControl, PowerReportListControl, ComparePowerReportListControl, TimelessJewelListControl, NotableDBControl, MinionListControl, MinionSearchListControl (which derives from MinionListControl), TradeStatWeightMultiplierListControl.
- DropDownControl subclasses: ItemSlotControl 185, TimelessJewelSocketControl 57.
- EditControl subclass: GemSelectControl 854.
- Calcs: CalcSectionControl(Control,ControlHost) 550; CalcBreakdownControl(Control,ControlHost) 789.
- Tabs: TreeTab(ControlHost); CalcsTab, ConfigTab, ItemsTab and SkillsTab are (UndoHandler,ControlHost,Control); ImportTab, NotesTab, PartyTab and CompareTab are (ControlHost,Control).
- PassiveTreeView is standalone and draws directly. GemTooltip.lua is a module, not a class.

RUNTIME FLOW
- SimpleGraphic calls launch:OnInit, OnFrame, OnKeyDown(key, doubleClick), OnKeyUp(key), OnChar(ch), CanExit and OnExit (Launch.lua:21-200).
- launch.main is the `main` ControlHost (Main.lua:57). main:OnKeyDown/OnKeyUp/OnChar only queue events into main.inputEvents ({type="KeyDown"|"KeyUp"|"Char", key, doubleClick}).
- main:OnFrame (Main.lua:379):
  1. screenW/H = GetVirtualScreenSize() (physical size / GetScreenScale, floored).
  2. Mode switch: modes LIST (Modules/BuildList) and BUILD (Modules/Build), called through CallMode Init/Shutdown/OnFrame.
  3. If popups[1] exists, it gets ProcessInput; otherwise main:ProcessControlsInput runs.
  4. mode:OnFrame(inputEvents, viewPort).
  5. Toasts.
  6. Bottom-left main bar (312x58).
  7. Popups drawn at layer 10 over a black 0.5 dim.
  8. Drag text at layer 20.
- Wheel events arrive as KeyUp "WHEELUP"/"WHEELDOWN".
- buildMode:OnFrame (Build.lua:1160):
  1. Global Ctrl hotkeys.
  2. Overlay panes (CalcSectionControl.isOverlay) take mouse clicks first.
  3. ProcessControlsInput.
  4. Recalc if buildFlag (calcsTab:BuildOutput, RefreshStatList).
  5. The current tab draws with tabViewPort = {x=312, y=32, w=screenW-312, h=screenH-32} via tab:Draw(viewPort, inputEvents).
  6. Overlay panes draw.
  7. SetDrawLayer(5) draws the top bar (0,0,screenW,28 at 0.2 grey; the separator is 0.85) and the sidebar (308 wide at 0.1 grey).
  8. Build's own controls draw (top bar plus sidebar stat list: TextListControl statBox with columns {x=170, RIGHT_X} and {x=174, LEFT}).
- Every tab:Draw follows the same pattern:
  1. self.x/y/width/height = viewPort.
  2. Handle hotkeys from inputEvents, setting consumed events to nil.
  3. self:ProcessControlsInput(inputEvents, viewPort).
  4. Imperative per-frame layout: set .x/.y, call SetAnchor.
  5. main:DrawBackground(viewPort) tiles the tree Background asset at sublayer -100 with tex coords viewPort/100 (REPEAT wrap).
  6. self:DrawControls(viewPort).

CONTROLHOST SEMANTICS (ControlHost.lua)
- selControl holds the focused control. OnKeyDown, OnKeyUp and OnChar return the next focused control: self to keep focus, nil to release, or another control.
- KeyDown goes to selControl first. If nothing is selected and it is a mouse button inside the viewport, it goes to GetMouseOverControl() (first in pairs() order where IsMouseOver; optional mouseClipRect).
- KeyUp goes to selControl and also to the hovered control's OnHoverKeyUp.
- Char goes only to selControl.
- DrawControls iterates pairs(self.controls). Key order is hash order for string keys, so Rust should use insertion order. It passes noTooltip=true to everything except the focused control.
- Tab order: AddToTabGroup/TabAdvance.
- Tooltips are drawn inside each control's Draw at SetDrawLayer(nil,100) via TooltipHost:DrawTooltip(x, y, w, h, viewPort). That uses tooltipFunc(tooltip, ...) when set (72 uses) or the tooltipText property.
- Tooltip placement: to the right of the control at x+w+5, flipping left or down when it would leave the viewport; supports multiple columns.
- Tooltip API: Clear, CheckForUpdate(...) (a memo key), AddLine(size, text, font, modLine, bg), AddSeparator, tooltipHeader with rarity frame images from Assets/*.png, and the main.showFlavourText option, which switches titles to "FONTIN SC".

UNDOHANDLER: undo and redo stacks of CreateUndoState() snapshots, capped at 101; setting modFlag marks the build unsaved.

STANDARD WIDGET LOOK (needed for pixel parity): a border rect, then an inner rect inset by 1.
- Border colour: 0.33 grey when disabled, 1.0 when hovered or locked, 0.5 otherwise.
- Inner colour: black, or 0.33 when hovered, or 0.5 when clicked.
- Labels: DrawString(x+w/2, y+2, "CENTER_X", h-4, "VAR", label).
- Arrows and check marks: DrawImageQuad with a nil image (main:DrawArrow, Main.lua:1497; main:DrawCheckMark, :1515).

CODE QUALITY: imperative and global-state heavy (main, launch, build, data, colorCodes are globals). Layout is recomputed every frame. Closures over self and upvalues are everywhere. Raw field reads (`ctrl.width`) are mixed with GetProperty, and some code calls `width()` without self. Behaviour is reproducible, but it needs a framework that tolerates dynamic props. The calc-adjacent code inside the UI (ConfigOptions apply closures, CalcsTab.PowerBuilder, GemSelectControl DPS sort, ItemsTab stat diffs) must call the Rust calc engine.

EXISTING RUST: crates/pob-ui is egui/eframe/wgpu (40k lines). Under this architecture it is mostly not reusable. Exceptions are the wgpu bootstrap and image loading.
- crates/pob-ui/src/color_codes.rs has the WRONG ^0-^9 palette (it uses colorCodes.* hex values). The real SimpleGraphic palette, confirmed from the float table at SimpleGraphic.dll offset 0x185380: ^0 (0,0,0), ^1 (1,0,0), ^2 (0,1,0), ^3 (0,0,1), ^4 (1,1,0), ^5 (1,0,1), ^6 (0,1,1), ^7 (1,1,1), ^8 (0.7,0.7,0.7), ^9 (0.4,0.4,0.4), all with alpha 1.
- crates/pob-calc/src/lua_pattern.rs is a Lua 5.1 pattern matcher (find/match/gmatch/gsub). Reuse it for EditControl filters (filterPattern = "[" .. filter .. "]", e.g. "%D", "^%w%p ", "%c"), for CalcFormat gsub, and for SearchHost.
- crates/pob-calc already runs PoB under LuaJIT with HeadlessWrapper stubs (headless.rs, jit.rs). That oracle can be extended to record SimpleGraphic draw calls as a UI parity oracle.

(1) RENDERER CONTRACT (what a Rust SimpleGraphic host must implement; source is _SimpleGraphic.def.lua, 516 lines)
- Frame model: a retained command list per frame.
  - Each draw command captures the state current at submission: (layer, subLayer, viewport rect, blend mode, RGBA colour).
  - At end of frame, sort stably by (layer, subLayer) and keep submission order inside a bucket. Then render with a scissor equal to the command's viewport.
- SetDrawLayer(layer, subLayer):
  - A nil layer keeps the current layer and only sets subLayer.
  - Layer alone resets subLayer to 0 (to verify).
  - Layers used: 0 default, 5 build top/side bars, 10 popups, 20 drag text, 1000 launch prompt.
  - SubLayers used: -100 background, -10 SectionControl frame, 5 dropdown lists, 15/20/25/30 tree layers, 99/100 tooltips.
  - GetDrawLayer is used only in a commented line.
- SetViewport(x, y, w, h) or SetViewport() to reset to the full screen. While set, all draw coordinates are relative to the viewport origin and clipped to it. GetCursorPos stays in screen coordinates.
- SetDrawColor:
  - (r, g, b[, a]) with floats 0-1; alpha defaults to 1.
  - Or an escape string "^7" / "^xRRGGBB"; PassiveTreeView passes a string as r.
  - GetDrawColor is used once (Tooltip.lua:613).
- DrawImage(handle|nil, x, y, w, h[, tcL, tcT, tcR, tcB][, stackIdx, mask]):
  - A nil handle draws a solid quad in the current colour.
  - The image is tinted by the colour.
  - Tex coords may exceed 1 (tiling, so REPEAT wrap unless the image was loaded with "CLAMP") or be flipped (0,1,1,0).
  - stackIdx/mask is unused by PoB src.
- DrawImageQuad(handle|nil, x1,y1..x4,y4[, s1,t1..s4,t4]): an arbitrary quad (TL, TR, BR, BL order). Used for tree connectors (arc sprites with per-vertex UV), rotated images and arrows. It needs a proper 2-triangle textured quad, not an axis-aligned blit.
- DrawString(x, y, align, height, font, text). Alignment:
  - "LEFT": at x.
  - "CENTER": floor((viewportW - textW)/2 + x).
  - "RIGHT": floor(viewportW - textW - x).
  - "CENTER_X": floor(x - textW/2).
  - "RIGHT_X": floor(x - textW).
  - CENTER is relative to the current viewport width: CompareTab.lua:1782 uses DrawString(0, 40, "CENTER", ...) inside SetViewport.
- DrawString escapes and newlines:
  - Handles "\n" for multi-line output with a line step equal to height (NotesTab label).
  - Inline escapes: ^[0-9] picks the palette entry; ^x followed by 6 hex digits sets RGB.
  - The colour starts as the current draw colour.
  - Whether alpha is kept across escapes is unverified.
- DrawStringWidth(height, font, text): the widest line, ignoring escapes.
- DrawStringCursorIndex(height, font, text, cx, cy): the byte index of the caret for a point; multi-line. Used by EditControl, ListControl ellipsis clipping and ItemsTab.
- StripEscapes(text): removes ^%d and ^x%x{6}.
- Images:
  - NewImageHandle(), then handle:Load(path, "ASYNC"|"CLAMP"|"MIPMAP"...). Paths are relative to src: Assets/*.png (86 files), TreeData/<ver>/*.{png,jpg,webp} (625 png, 112 jpg, 32 webp; 552 MB in total). The decoder needs png, jpg and webp.
  - handle:ImageSize() returns 0,0 until loaded; PassiveTreeView tolerates that. Also IsValid, Unload.
  - LoadArt*, NewArtHandle, Texture.*, SetBlendMode, SetClearColor, ShowCursor and SetCursorPos are unused by PoB src and can be skipped.
- Input:
  - Callbacks OnKeyDown(key, doubleClick), OnKeyUp(key), OnChar(char) (EditControl ignores '\b').
  - IsKeyDown(name) and GetCursorPos() (virtual pixels).
  - Key names used: LEFTBUTTON, RIGHTBUTTON, MIDDLEBUTTON, MOUSE4, MOUSE5, WHEELUP, WHEELDOWN (KeyUp), CTRL, SHIFT, ALT, RETURN, ESCAPE, BACK, DELETE, TAB, UP, DOWN, LEFT, RIGHT, HOME, END, PAGEUP, PAGEDOWN, F1-F6, PRINTSCREEN, PAUSE, plus single lowercase chars and digits for Ctrl combos.
- Screen and DPI: GetScreenSize, GetScreenScale and Get/SetDPIScaleOverridePercent. PoB works in virtual pixels = physical / scale.
- Misc: GetTime (ms; caret blink, tree animation), Copy/Paste (clipboard), OpenURL, SetWindowTitle, TakeScreenshot, Restart, Exit, Deflate/Inflate (zlib; build codes), NewFileSearch (build list folders), MakeDir/RemoveDir, GetUserPath/GetScriptPath/GetRuntimePath, LaunchSubScript (background HTTP and trade jobs; replace with threads), ConPrintf.

FONTS
- Font name to file mapping (the DLL string-table order matches the def.lua Font enum; FONTIN filenames inferred from that order):
  - "FIXED" → "Bitstream Vera Sans Mono"
  - "VAR" → "Liberation Sans"
  - "VAR BOLD" → "Liberation Sans Bold"
  - "FONTIN SC" → "Fontin SmallCaps"
  - "FONTIN SC ITALIC" → "Fontin SmallCaps Italic"
  - "FONTIN" → "Fontin"
  - "FONTIN ITALIC" → "Fontin Italic"
- Where each font is used:
  - "VAR" is almost everything.
  - "FIXED" is used for protected EditControl and the launch restart text.
  - "FONTIN SC" is used for tooltip titles when main.showFlavourText is set.
  - Tooltip lines can pass a font argument.
- Files: runtime/SimpleGraphic/Fonts/<Name>.tgf plus <Name>.<H>.tga for H in {10,12,14,16,18,20,22,24,26,28,32,36,40,48,56,64}. 7 faces, 120 files.
- .tgf format: ASCII text, 2064 lines. For each of the 16 heights there is one line `HEIGHT <h>;` followed by 128 lines `GLYPH <x> <y> <width> <spLeft> <spRight>;\t// <code> (<char>)`, one per byte 0-127. The DLL format string is "GLYPH %u %u %u %d %d;".
  - x and y are the top-left of the glyph cell in that height's TGA, in pixels with the origin at the top after flipping.
  - The cell is width × HEIGHT.
  - spLeft and spRight are signed advance adjustments before and after the glyph.
  - Codes 0-31 and 127 are placeholder 1-px glyphs. There are no glyphs above 127, so text must be ASCII (PoB sanitises; main.unicode is false in stock PoB).
- .tga format: image type 10 (RLE true-colour), 32 bpp BGRA, descriptor 0x00, so it is stored bottom-up and needs a vertical flip. RGB is always white and alpha is the coverage (verified by decoding Liberation Sans.16.tga glyph 'A'). Atlas sizes vary per height (10 → 128x128, 16 → 128x256).
- Rendering algorithm (SimpleGraphic r_font.cpp, from memory, to verify):
  - Pick the smallest atlas height >= requested (clamp to 64); scale = height / atlasH.
  - Per char: x += spLeft*scale; draw the quad width*scale × height with UV from (x, y, width, H) / atlasSize; x += width*scale; x += spRight*scale.
  - Width = the sum of the same advances, max over lines.

(3) ANCHORING/LAYOUT (Control.lua)
- Constructor: Control(anchor, rect). rect = {x, y, width, height}; each entry is a Prop, i.e. a number or fun(self) → number.
- anchor = {point, otherControl, otherPoint, collapse?}.
- Anchor points (fx, fy): TOPLEFT (0,0), TOP (.5,0), TOPRIGHT (1,0), RIGHT (1,.5), BOTTOMRIGHT (1,1), BOTTOM (.5,1), BOTTOMLEFT (0,1), LEFT (0,.5), CENTER (.5,.5).
- GetPos:
  - If anchor.collapse is set and the other control is not shown: return other:GetPos() + (collapseX, collapseY). The control takes the hidden sibling's slot, which gives vertical stacking that skips hidden rows (Build sidebar; ItemSlotControl sets anchor.collapse = true).
  - Otherwise, when other is set:
    - x = floor(otherX + otherW*ofx + x - w*fx)
    - y = floor(otherY + otherH*ofy + y - h*fy)
    - Sizes are only queried when the point is not TOPLEFT.
  - With no other: x and y are absolute screen coordinates.
- IsShown = (no other, or collapse, or other:IsShown()) and shown. Visibility inherits along the anchor chain unless collapse is set.
- Props:
  - Any of x, y, width, height, shown, enabled, label, locked and tooltipText can be a closure. Counts of closure assignments: shown 145, enabled 79, width 23, height 21, y 22, x 13, locked 14, label 13.
  - Rect entries can be closures (Build.lua:194).
  - SetAnchor(point, other, otherPoint, x, y) is called at runtime for responsive reflow (26 calls; e.g. TreeTab.lua:410-420).
  - Tabs and containers also assign .x/.y imperatively every frame: CalcsTab packs sections into columns (CalcsTab.lua:226); TreeTab sets anchorControls.
- PopupDialog:
  - The popup is centred on screen.
  - A child without an anchor point is anchored TOP to TOP of the popup.
  - A nil anchor.other means the popup.
  - A string anchor.other is resolved to the sibling with that key.
- Rust model (recommended):
  - One global arena (slotmap) of controls with a ControlId. Each host keeps an ordered Vec/IndexMap<name, ControlId>. Anchors cross hosts (child scrollbars anchor to the parent list; tab controls anchor to the tab), so per-host arenas do not work.
  - `struct ControlBase { rect: [Prop<f32>; 4], anchor: Option<Anchor{point, other: Option<ControlId>, other_point, collapse}>, collapse_xy, shown: Prop<bool>, enabled: Prop<bool>, has_focus, tab_group }`.
  - `enum Prop<T> { Val(T), Fn(Rc<dyn Fn(&UiCtx) -> T>) }`. UiCtx gives read-only access to app/build state plus the arena, for GetSize and GetPos of others.
  - `fn pos(id, ctx) -> (f32, f32)` is a recursive pure function implementing exactly the math above, including floor. It can be memoised per frame.
  - A behaviour trait `Widget { draw(&mut self, base, ctx, &mut Sg, viewport, no_tooltip); on_key_down(..) -> Focus; on_key_up; on_char; on_hover_key_up; is_mouse_over; on_focus_gained/lost; as_list() -> Option<&ListState> (replaces duck typing) }`.
  - `enum Focus { Keep, Release, To(ControlId) }`.
  - Callbacks (onClick, selFunc, changeFunc): Box<dyn FnMut(&mut AppCtx)>. Take the widget out of the arena during dispatch (Option::take) to avoid aliasing, or queue Action values to apply after input processing.
  - Prefer converting most shown/enabled closures into an explicit per-frame `sync(&State)` method on each tab. Keep Prop::Fn for geometry that depends on other controls' sizes.

(4) COLOURS
- SimpleGraphic escapes: `^0`-`^9` use the palette given under EXISTING RUST above (^7 white is the default text colour; ^8 0.7 grey; ^9 0.4 grey). `^xRRGGBB` gives arbitrary RGB.
- StripEscapes regexes: %^%d and %^x%x%x%x%x%x%x.
- colorCodes table (Data/Global.lua:7-77, a mutable global; `updateColorCode` lets Options override POSITIVE, NEGATIVE and HIGHLIGHT, and colour edits are stored with a 0x prefix converted to ^x):
  - NORMAL ^xC8C8C8, MAGIC ^x8888FF, RARE ^xFFFF77, UNIQUE ^xAF6025, RELIC ^x60C060, GEM ^x1AA29B, PROPHECY ^xB54BFF, CURRENCY ^xAA9E82, CRAFTED ^xB8DAF1, CUSTOM ^x5CF0BB, SOURCE ^x88FFFF
  - UNSUPPORTED ^xF05050, DISABLED ^x7F7F7F, WARNING ^xFF9922, TIP ^x80A080
  - FIRE ^xB97123, COLD ^x3F6DB3, LIGHTNING ^xADAA47, CHAOS ^xD02090
  - POSITIVE ^x33FF77, NEGATIVE ^xDD0022, HIGHLIGHT ^xFF0000, OFFENCE ^xE07030, DEFENCE ^x8080E0
  - SCION ^xFFF0F0, MARAUDER ^xE05030, RANGER ^x70FF70, WITCH ^x7070FF, DUELIST ^xE0E070, TEMPLAR ^xC040FF, SHADOW ^x30C0D0
  - MAINHAND ^x50FF50, MAINHANDBG ^x071907, OFFHAND ^xB7B7FF, OFFHANDBG ^x070719
  - SHAPER ^x55BBFF, ELDER ^xAA77CC, FRACTURED ^xA29160, MUTATED ^xCD2285, ADJUDICATOR ^xE9F831, BASILISK ^x00CB3A, CRUSADER ^x2946FC, EYRIE ^xAAB7B8, CLEANSING ^xF24141, TANGLE ^x038C8C
  - CHILLBG ^x151e26, FREEZEBG ^x0c262b, SHOCKBG ^x191732, SCORCHBG ^x270b00, BRITTLEBG ^x00122b, SAPBG ^x261500
  - SCOURGE ^xFF6E25, CRUCIBLE ^xFFA500, SPLITPERSONALITY ^xFFD62A, VESTIGIAL ^xCBA5F1, INTANGIBILITY ^x9BF4BD, MEMORY ^xBFE2FA
  - Aliases: STRENGTH = MARAUDER, DEXTERITY = RANGER, INTELLIGENCE = WITCH, LIFE = MARAUDER, MANA = WITCH, ES = SOURCE, WARD = RARE, ARMOUR = NORMAL, EVASION = POSITIVE, RAGE = WARNING, PHYS = NORMAL.
  - defaultColorCodes = copy of the above.
- rgbColor = hexToRGB(HIGHLIGHT) is used by the tree.

## reusable
- /home/ferm/Work/projects/PathOfBuildingMK2/.PathOfBuilding/src/_SimpleGraphic.def.lua — the authoritative renderer API signature list (516 lines); also the Font enum.
- /home/ferm/Work/projects/PathOfBuildingMK2/.PathOfBuilding/runtime/SimpleGraphic/Fonts/*.tgf + *.<H>.tga — bitmap font atlases (7 faces x 16 heights). Format: HEIGHT/GLYPH x y w spL spR; RLE 32bpp bottom-up TGA, white RGB, alpha = coverage.
- /home/ferm/Work/projects/PathOfBuildingMK2/.PathOfBuilding/src/Data/Global.lua:7-77 — colorCodes table (port verbatim as a const map plus mutable overrides).
- /home/ferm/Work/projects/PathOfBuildingMK2/.PathOfBuilding/src/Classes/Control.lua:11-21 (anchorPos table) and :84-110 (GetPos math) — port 1:1.
- /home/ferm/Work/projects/PathOfBuildingMK2/.PathOfBuilding/src/Classes/ControlHost.lua — input dispatch/focus algorithm (115 lines), port 1:1.
- /home/ferm/Work/projects/PathOfBuildingMK2/.PathOfBuilding/src/Modules/Main.lua:1486-1572 — DrawBackground, DrawArrow, DrawCheckMark, WorldToScreen, RenderCircle and RenderRing primitives; :1797 WrapString; :1660 OpenPopup/ClosePopup.
- /home/ferm/Work/projects/PathOfBuildingMK2/.PathOfBuilding/src/Modules/CalcSections.lua — pure data (one function in 2638 lines); can be converted to JSON/RON by the existing pob-extract crate rather than hand-ported.
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-calc/src/lua_pattern.rs — Lua 5.1 pattern engine (find/matches/gmatch/gsub). Reuse for EditControl filter patterns, CalcFormat gsub, SearchHost and WrapString %s+.
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-calc/src/headless.rs + jit.rs — LuaJIT oracle that already runs Launch.lua/HeadlessWrapper. It can be extended to override DrawImage/DrawString/SetDrawLayer/SetViewport/SetDrawColor and record a per-frame draw-command list, giving a golden-image-free UI parity oracle (feed identical input events, diff command streams).
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-extract — existing Lua-data-to-Rust artefact pipeline; usable for CalcSections, ConfigOptions metadata (not the apply closures) and TreeData sprites.lua.
- /home/ferm/Work/projects/PathOfBuildingMK2/crates/pob-ui (egui) — only the wgpu/eframe bootstrap and image decoding (image crate with jpeg+png; webp must be added) are worth salvaging. color_codes.rs PALETTE is wrong and must not be reused.
- SimpleGraphic ^0-^9 palette, verified from the float table in /home/ferm/Work/projects/PathOfBuildingMK2/.PathOfBuilding/runtime/SimpleGraphic.dll @0x185380: black, red, green, blue, yellow, magenta, cyan, white, 0.7 grey, 0.4 grey (alpha 1).

## discard


## workUnits
- **U0 sg-core: command-recording renderer API (no GPU)** [src/_SimpleGraphic.def.lua; Data/Global.lua:7-100 (colorCodes, hexToRGB, updateColorCode); Common.lua GetVirtualScreenSize] deps=none :: Crate `pob-sg`: struct Sg with set_draw_layer(Option<i32>, Option<i32>), set_viewport(Option<Rect>), set_draw_color(f32x4 | escape str), draw_image(Option<ImageId>, x, y, w, h, Option<[f32;4]> tc), draw_image_quad(Option<ImageId>, [f32;8], Option<[f32;8]>), draw_string, draw_string_width, draw_string_cursor_index, strip_escapes. Each command snapshots layer, sublayer, viewport and colour; end_frame() sorts stably and returns a Vec<DrawCmd>. Includes the escape parser (^0-9 palette from the DLL, ^xRRGGBB) and the colorCodes const table. Pure and testable; usable headless for parity diffs against the LuaJIT oracle.
- **U1 sg-font: TGF/TGA bitmap font engine** [runtime/SimpleGraphic/Fonts/*.tgf, *.tga (7 faces x 16 heights); font enum in _SimpleGraphic.def.lua] deps=U0 :: TGF text parser; RLE-32 TGA decoder with bottom-up flip (or the image crate's tga feature); FontId enum mapping VAR→Liberation Sans, VAR BOLD→Liberation Sans Bold, FIXED→Bitstream Vera Sans Mono, FONTIN[ ITALIC]→Fontin[ Italic], FONTIN SC[ ITALIC]→Fontin SmallCaps[ Italic]. Layout: choose the atlas height, scale, spLeft/width/spRight advances, newline handling, escape skipping, alignment modes (LEFT/CENTER/RIGHT relative to the viewport; CENTER_X/RIGHT_X relative to x, floored), width = max line, cursor index. First verify the metrics against PoB-SimpleGraphic r_font.cpp, ideally by golden widths captured from the LuaJIT oracle running real SimpleGraphic, which is not possible headless, so use the upstream source.
- **U2 sg-host: window, input and GPU backend** [Launch.lua (416) callback contract; Main.lua OnKeyDown/OnKeyUp/OnChar event queue] deps=U0, U1 :: winit + wgpu (reuse the pob-ui bootstrap). A batched textured-quad pipeline (quads as 2 triangles with arbitrary UV), scissor per viewport, REPEAT vs CLAMP samplers, mipmaps, async image loading (png, jpg, webp) with ImageSize 0 until ready. Maps winit input to PoB key names (LEFTBUTTON, WHEELUP as KeyUp, CTRL, F1-F12, single chars), doubleClick detection, OnChar text input, DPI scale and virtual pixels, clipboard (arboard), GetTime, OpenURL, window title. Defines a trait App { on_init, on_frame(&mut Sg, &Input), can_exit }.
- **U3 ui-core: Control/ControlHost framework plus simple widgets** [Classes/Control.lua 178, ControlHost.lua 115, TooltipHost.lua 27, UndoHandler.lua 58, LabelControl 24, ButtonControl 130, CheckBoxControl 117, RectangleOutlineControl 23, SectionControl 32, ScrollBarControl 327, SliderControl 185, DraggerControl 146; Main.lua:1497-1530 (DrawArrow, DrawCheckMark)] deps=U0, U1 (U2 only for running) :: Fixes the architecture every later unit uses: global slotmap arena, ControlId, Prop<T> {Val, Fn(Rc<dyn Fn(&UiCtx)->T>)}, Anchor plus collapse with exact GetPos/IsShown math, the Widget trait with Focus {Keep, Release, To(id)}, input-event dispatch identical to ProcessControlsInput, insertion-ordered DrawControls, tab groups, the callback/Action pattern, and a generic UndoStack<S> (cap 101). About 1.5k Lua lines; must ship with a design doc for the other agents.
- **U4 ui-text-and-lists: Edit, List, DropDown, Popup** [Classes/EditControl.lua 761, ResizableEditControl 50, ListControl 493, TextListControl 122, DropDownControl 573, SearchHost 153, PopupDialog 96, PathControl 110; Main.lua:1660-1760 (OpenPopup, ClosePopup, OpenMessagePopup, OpenConfirmPopup, OpenNewFolderPopup), Main.lua:1797 WrapString] deps=U3; pob-calc lua_pattern.rs for filters and search :: EditControl: caret/selection by byte index, multi-line with scrollbars, undo, Ctrl+C/V/X/Z/Y, numeric +/- buttons, filter patterns, protected mode in the FIXED font, zoom. ListControl: columns, sort labels, ellipsis clipping via DrawStringCursorIndex, drag-and-drop between lists (dragTargetList, main.showDragText). DropDown: dropped list at sublayer 5, type-to-search with highlights, droppedWidth auto-size. About 2.4k lines; may split Edit and List+DropDown across two agents.
- **U5 tooltip** [Classes/Tooltip.lua 698; Classes/GemTooltip.lua 261; Assets/items*header*.png, *icon.png, TreeData/*Oil.png] deps=U3; Item/gem model for GemTooltip :: Line/block model, separators, rarity header images (left/middle/right tiling), influence icons, recipe images, multi-column overflow, hover placement against the viewport, CheckForUpdate memoisation, maxWidth wrapping. GemTooltip needs the gem data model and calc output.
- **U6 app-shell: Launch, Main, BuildList mode, toasts, options** [Launch.lua 416; Modules/Main.lua 1822 (settings XML load/save, Options popup :865, About popup :1373, update UI); Modules/ToastNotification.lua 254; Modules/BuildList.lua 220; Modules/BuildListHelpers.lua 201; Classes/BuildListControl 287, FolderListControl 99; ExtBuildListControl 440 + ExtBuildListProvider 63 + PoBArchivesProvider 150 (network, optional)] deps=U3, U4, U2 :: Mode switching LIST/BUILD, popup stack, main bottom bar, Settings.xml compatibility, build folder browsing (NewFileSearch → std::fs), drag text overlay. Skip auto-update. About 2.5k lines core plus 650 network.
- **U7 build-shell: Build mode frame, sidebar and stats** [Modules/Build.lua 2335; Modules/BuildDisplayStats.lua 293 (99 functions)] deps=U4, U5, U6; Rust calc engine output API; all tab structs (as stubs initially) :: Top bar (back/save/name/points/level/class/ascendancy/loadouts), sidebar mode buttons with `locked`, main skill selectors with collapse anchors, statBox TextListControl with stat formatting and compare deltas, warnings, the per-tab viewport (x=312, y=32), the buildFlag → recalc loop, Save/Load build XML orchestration, save popups. RefreshSkillSelectControls and the stat lists are tightly coupled to calc output.
- **U8 passive-tree-view renderer** [Classes/PassiveTreeView.lua 1856; tree asset/sprite loading and connector geometry from Classes/PassiveTree.lua (LoadImage :909, BuildConnector :935, sprites.lua per version); Main.lua WorldToScreen/RenderCircle/RenderRing] deps=U0, U2, U3, U5; Rust PassiveTree/PassiveSpec model :: Pan/zoom (1.2^level, 0-12), node hit-testing (rsq), sublayers 15/20/25/30/99/100, connector arcs via DrawImageQuad with UVs, DrawImageRotated animations, group backgrounds with half mirroring (tc 0,1,1,0), jewel radius rings, search highlight, heat map colouring, compare overlay, node tooltips (AddNodeTooltip :1476 calls calc for stat diffs).
- **U9a TreeTab core** [Classes/TreeTab.lua :26-660, :818-1060 (controls, Draw, spec management, version convert, import/export popups, ModifyNodePopup, mastery popup); PassiveSpecListControl 132, PassiveMasteryControl 59] deps=U4, U8 :: Responsive two-line control layout using SetAnchor per frame (TreeTab.lua:395-420).
- **U9b TreeTab power report and timeless jewel search** [TreeTab.lua :1047-2888 (SetPowerCalc, BuildPowerReportList, FindTimelessJewel, ~1700 lines); PowerReportListControl 153, TimelessJewelListControl 352, TimelessJewelSocketControl 57, NotableDBControl 293; Modules/DataLegionLookUpTableHelper] deps=U9a; calc engine node-power API; legion LUT data :: Heavy calc coupling (CalcsTab.PowerBuilder). The trade query link uses TradeQueryRequests (network).
- **U10a GemSelectControl** [Classes/GemSelectControl.lua 854] deps=U4, U5; gem data; calc engine (CalcOutputWithThisGem for DPS sort) :: An EditControl subclass with a custom dropdown list, sort modes and a DPS preview.
- **U10b SkillsTab** [Classes/SkillsTab.lua 1502; SkillListControl 267, SkillSetListControl 127, MinionListControl 107, MinionSearchListControl 71] deps=U10a, U4; skills model :: Socket groups, gem slots (CreateGemSlot :743 builds rows of controls dynamically), copy/paste, skill sets, undo.
- **U11a Items: slots and lists** [ItemSlotControl 185, ItemListControl 387, ItemDBControl 400, ItemSetListControl 145, SharedItemListControl 80, SharedItemSetListControl 112] deps=U4, U5; Item model :: Drag-and-drop between DB/list/slots, anchor.collapse slot stacking, flask activate checkboxes.
- **U11b ItemsTab layout and display item** [Classes/ItemsTab.lua :93-2608 (constructor ~1100 lines of controls, Load/Save, Draw, sets, slots, display item, variants, affix sliders, cluster jewel controls)] deps=U11a :: The largest control constructor in PoB; many closures. Consider splitting the constructor from the rest.
- **U11c ItemsTab crafting popups** [ItemsTab.lua :2609-3992 (CraftItem, EditDisplayItemText, Enchant, Anoint, Corrupt, AddCustomModifier, Crucible); Modules/AddImplicitPopup.lua 414] deps=U11b; mod data :: Popup-heavy; mostly the U4 widgets.
- **U11d Item tooltips and stat diffs** [ItemsTab.lua :3993-5055 (SetTooltipHeaderInfluence, FormatItemSource, AddItemTooltip :4214, AddItemStatDifferences :4664, undo)] deps=U5, U11b; calc engine (compare output per slot) :: Accuracy-relevant display of DPS deltas.
- **U11e Trade query (optional, network)** [TradeQuery 1415, TradeQueryGenerator 1609, TradeQueryRequests 515, TradeQueryRateLimiter 285, TradeHelpers 623, TradeStatWeightMultiplierListControl 42, CompareBuySimilar 538] deps=U11b, HTTP client :: About 5k lines. Defer.
- **U12 Calcs tab** [Classes/CalcsTab.lua 773, CalcSectionControl 550, CalcBreakdownControl 789; Modules/CalcSections.lua 2638 (data), CalcFormat 74, CalcBreakdown 255] deps=U4, U5; calc engine output and breakdown structures :: CalcSections should be extracted as data via pob-extract. formatCalcStr gsub patterns map to lua_pattern.rs. Column packing layout (CalcsTab.lua:226), overlay panes (Build.overlayPanes), breakdown tables, DrawRadiusVisual. PowerBuilder (:496-760) is a calc-driven coroutine; it can be shared with U9b.
- **U13 Config tab** [Classes/ConfigTab.lua 1362 (including CustomModBlockControl), ConfigSetListControl 114; Modules/ConfigOptions.lua 2380 (1043 vars, 531 apply closures), ConfigVisibility 160, ConfigModBrowser 311] deps=U4; calc engine (the apply closures are calc logic and should live in the calc crate) :: Split: the UI metadata (label, type, list, tooltip, ifFlag/ifSkill visibility) is data-extractable; the apply functions need a hand port into the calc crate.
- **U14 Import tab** [Classes/ImportTab.lua 1932; Classes/PoEAPI.lua 238; Modules/BuildSiteTools.lua 129] deps=U4, U7; HTTP, OAuth (already exists natively in the app per commit fda2c40), Deflate/Inflate (flate2) :: Build code import/export is critical (base64 + zlib + XML). Character import is network-dependent.
- **U15 Notes and Party tabs** [Classes/NotesTab.lua 112; Classes/PartyTab.lua 1042] deps=U4; PartyTab needs ModDB/ModList and calc (ParseBuffs) :: Notes is trivial and a good first end-to-end tab. Party parses buff exports into modLists.
- **U16 Compare tab (last)** [Classes/CompareTab.lua 5036, CompareEntry 552, CompareCalcsHelpers 476, ComparePowerReportListControl 165] deps=U7-U13 all :: Split into: init/controls (:129-1650), layout (:1650-2470), power builder (:2516-3160), and draw summary/tree/items/skills/calcs/config (:3162-5036). Re-uses PassiveTreeView, CalcSections and ConfigOptions.

## risks
- Font metrics (atlas-height selection rule, fractional advance accumulation, floor points, CENTER/RIGHT relative to viewport width) are reconstructed from memory of PoB-SimpleGraphic r_font.cpp, not from source. Verify against github PathOfBuildingCommunity/PoB-SimpleGraphic (engine/render/r_font.cpp, r_main.cpp, ui_api.cpp) before claiming pixel parity. Text widths drive layout (LabelControl width = DrawStringWidth; CheckBox labelWidth; tooltip sizes), so any metric error shifts whole UIs.
- Layer semantics are unverified: whether SetDrawLayer(layer) without a subLayer resets subLayer to 0, and how viewport state interacts with layer switches. Recommended contract: each command snapshots (layer, sublayer, viewport, color) and the frame is sorted stably.
- Alpha handling of inline ^ escapes in DrawString (keep the SetDrawColor alpha or force 1) is unknown; this affects faded text.
- ControlHost iterates pairs(self.controls) (Lua hash order) for both draw order and mouse hit-testing. A Rust port using insertion order may differ where controls overlap. Rare, but visible in dropdown/list overlaps.
- Closures everywhere: shown (145), enabled (79), geometry (~90), tooltipFunc (72), plus onClick/selFunc/changeFunc closures that mutate build state. A naive Rust port fights the borrow checker. This needs an arena + context-passing design decided up front (unit U2) or every tab agent will invent its own.
- Duck typing and dynamic fields: `selControl.ListControl` is-a checks, raw `.width` reads vs GetProperty, `width()` called without self, controls tables holding mixed array/string keys, PopupDialog string anchors. These are quirks to preserve semantically, not literally.
- The UI is deeply entangled with the calc engine and model: ItemsTab stat diffs and tooltips, GemSelectControl DPS sort (CalcOutputWithThisGem), CalcsTab.PowerBuilder/TreeTab power report (re-running calcs per node), ConfigOptions (531 apply closures that build modLists, so calc logic, not UI), CalcBreakdownControl reading calc breakdown tables, BuildDisplayStats. UI units are blocked on a stable Rust calc API (env/output/breakdown/modDB queries).
- Model classes the UI needs are not UI: Item.lua 2736, PassiveSpec 2494, PassiveTree 1062 (tree layout, connectors, sprite atlas loading), ModStore/ModDB/ModList. They must exist in Rust before tabs can be ported.
- Network features embedded in the UI: ImportTab + PoEAPI (OAuth), TradeQuery* (~5k lines), ExtBuildListControl/PoBArchivesProvider, build-site import/export, all via LaunchSubScript+curl. These need a Rust HTTP/thread replacement and are large but low priority for DPS accuracy.
- Assets: TreeData is 552 MB over many tree versions; includes webp (32 files), large PNGs (7 MB ascendancy background) and MIPMAP/ASYNC loading. The renderer needs async decode, mipmaps for tree zoom-out quality, a REPEAT/CLAMP wrap per image, and a texture-memory budget.
- Fonts only cover bytes 0-127. Any UTF-8 in item names or notes renders as garbage or nothing in PoB, and the port must match (PoB sanitiseText in Common.lua:306).
- DPI: PoB works in virtual pixels (GetVirtualScreenSize floors physical/scale). Physical-vs-virtual atlas selection for crisp text at 150% scale is unverified; RenderInit('DPI_AWARE').
- CompareTab (5036) and ItemsTab (5055) are each larger than one agent's comfortable unit and must be split along the function boundaries listed in workUnits.

## commands
- wc -l /home/ferm/Work/projects/PathOfBuildingMK2/.PathOfBuilding/src/Classes/*.lua | sort -n
- grep -hn '= newClass(' /home/ferm/Work/projects/PathOfBuildingMK2/.PathOfBuilding/src/Classes/*.lua | sed 's/.*newClass(//' | sort   # full class hierarchy
- awk '/HEIGHT 16;/{p=1;next} /HEIGHT 18/{p=0} p' '/home/ferm/Work/projects/PathOfBuildingMK2/.PathOfBuilding/runtime/SimpleGraphic/Fonts/Liberation Sans.tgf'   # glyph table for one height
- strings -n 4 /home/ferm/Work/projects/PathOfBuildingMK2/.PathOfBuilding/runtime/SimpleGraphic.dll | grep -iE 'liberation|fontin|vera|GLYPH|CENTER_X|Usage:'   # font names / API usage strings
- python3 -I -c "import struct;d=open('/home/ferm/Work/projects/PathOfBuildingMK2/.PathOfBuilding/runtime/SimpleGraphic.dll','rb').read();i=d.find(struct.pack('<8f',0,0,0,1,1,0,0,1));print([round(x,3) for x in struct.unpack('<40f',d[i:i+160])])"   # ^0-^9 palette
- cd /home/ferm/Work/projects/PathOfBuildingMK2/.PathOfBuilding/src && for fn in 'DrawImage(' 'DrawImageQuad(' 'DrawString(' 'DrawStringWidth(' 'SetDrawLayer(' 'SetViewport(' 'SetDrawColor('; do printf '%s %s\n' "$fn" $(grep -rF "$fn" --include='*.lua' Classes Modules Launch.lua | wc -l); done   # 259/17/131/92/97/53/394
- grep -rhoE 'key == "[A-Za-z0-9_]+"|IsKeyDown\("[A-Za-z0-9_]+"\)' /home/ferm/Work/projects/PathOfBuildingMK2/.PathOfBuilding/src/Classes /home/ferm/Work/projects/PathOfBuildingMK2/.PathOfBuilding/src/Modules | sort | uniq -c | sort -rn   # key names used