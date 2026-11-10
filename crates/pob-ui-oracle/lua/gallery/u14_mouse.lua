-- Gallery "u14_mouse" (U14): the popups of _u14.lua driven by the mouse (close from the own
-- button through OnKeyUp, the three-button layout, stacked popups, tooltips in popups), with
-- main.screenH pinned to 720 so the script's positions hit the same buttons at both screens.
-- Rust counterpart: src/gallery/u14_mouse.rs. Interactions: u14_mouse.pobscript.
local dir = __galleryPath:match("^(.*[/\\])")
return LoadModule(dir .. "_u14", { pinScreenH = 720 })
