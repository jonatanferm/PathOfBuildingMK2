-- Gallery "u14_keys" (U14): the popups of _u14.lua driven by the keyboard at the real screen
-- size (RETURN/ESCAPE routing, default focus, the edit eating RETURN/ESCAPE, scroll keys).
-- Rust counterpart: src/gallery/u14_keys.rs. Interactions: u14_keys.pobscript.
local dir = __galleryPath:match("^(.*[/\\])")
return LoadModule(dir .. "_u14", { pinScreenH = nil })
