-- Shared body of the U14 galleries (`u14_keys`, `u14_mouse`): popups. Host buttons open
-- main:OpenMessagePopup (three lines with colour codes), main:OpenConfirmPopup with two and with
-- three buttons, main:OpenNewFolderPopup (MakeDir under GetUserPath()), main:OpenCloudErrorPopup,
-- and two main:OpenPopup dialogs: "Custom" (a function title, a nil-anchored label, a
-- {"TOPLEFT", nil, "TOPLEFT"} edit, a {"LEFT", "edit", "RIGHT"} string-key anchor, enter "ok",
-- default "edit", escape "cancel", a button that stacks a message popup on top) and "Scroll"
-- (a scrollBarFunc and a `scrollBar` control, so KeyUp of the scroll keys scrolls it).
-- OnFrame is main:OnFrame's popup handling: Main.lua:402-407 (the top popup gets the input, then
-- the events are wiped) and Main.lua:437-443 (dim and draw at layer 10).
-- Rust counterpart: src/gallery/_u14.rs.
--
-- `opts.pinScreenH`: main.screenH is set to this after the launch sets it each frame, so the
-- popups (centred on main.screenW/screenH) sit at the same virtual coordinates at both acceptance
-- screens (1280x800 and 1920x1080@1.5 = 1280x720) and one script's mouse positions hit their
-- buttons at both.

local opts = ...

-- Launch.lua:9 (the gallery mini-launch does not run Launch.lua)
APP_NAME = APP_NAME or "Path of Building"
if not GetCloudProvider then
	-- _SimpleGraphic.def.lua:333-335; SimpleGraphic implements it on Windows only
	function GetCloudProvider(path)
		return nil, nil, nil
	end
end

local G = { }

function G:Init()
	self.host = new("ControlHost"):ControlHost()
	local c = self.host.controls
	self.log = "none"
	c.msg = new("ButtonControl"):ButtonControl(nil, {20, 20, 180, 20}, "Message", function()
		main:OpenMessagePopup("Info", "Hello there.\nThis is a ^1coloured^7 message\nwith three lines.")
	end)
	c.confirm = new("ButtonControl"):ButtonControl(nil, {20, 50, 180, 20}, "Confirm", function()
		main:OpenConfirmPopup("Delete Build", "Are you sure you want to delete this build?", "Delete", function()
			self.log = "deleted"
		end)
	end)
	c.confirm3 = new("ButtonControl"):ButtonControl(nil, {20, 80, 180, 20}, "Confirm 3", function()
		main:OpenConfirmPopup("Connect", "Continue without connecting?", "Continue", function()
			self.log = "continue"
		end, "Connect Path", function()
			self.log = "connect"
		end)
	end)
	c.custom = new("ButtonControl"):ButtonControl(nil, {20, 110, 180, 20}, "Custom", function()
		self:OpenCustom()
	end)
	c.folder = new("ButtonControl"):ButtonControl(nil, {20, 140, 180, 20}, "New Folder", function()
		main:OpenNewFolderPopup(GetUserPath() .. "/", function(name)
			self.log = "folder " .. tostring(name)
		end)
	end)
	c.cloud = new("ButtonControl"):ButtonControl(nil, {20, 170, 180, 20}, "Cloud Error", function()
		main:OpenCloudErrorPopup("Builds/Missing.xml")
	end)
	c.scroll = new("ButtonControl"):ButtonControl(nil, {20, 200, 180, 20}, "Scroll", function()
		self:OpenScroll()
	end)
	c.status = new("LabelControl"):LabelControl(nil, {20, 240, 0, 16}, function()
		return "^7log: " .. self.log .. "  popups: " .. #main.popups
	end)
end

function G:OpenCustom()
	local controls = { }
	controls.label = new("LabelControl"):LabelControl(nil, {0, 16, 0, 16}, "^7Name:")
	controls.edit = new("EditControl"):EditControl({"TOPLEFT", nil, "TOPLEFT"}, {20, 40, 220, 20}, "", nil, nil, 20)
	controls.stack = new("ButtonControl"):ButtonControl({"LEFT", "edit", "RIGHT"}, {8, 0, 80, 20}, "Stack", function()
		main:OpenMessagePopup("Stacked", "On top of the custom popup")
	end)
	controls.ok = new("ButtonControl"):ButtonControl(nil, {-45, 160, 80, 20}, "Ok", function()
		self.log = "ok:" .. controls.edit.buf
		main:ClosePopup()
	end)
	controls.cancel = new("ButtonControl"):ButtonControl(nil, {45, 160, 80, 20}, "Cancel", function()
		self.log = "cancel"
		main:ClosePopup()
	end)
	main:OpenPopup(360, 200, function()
		return "Custom ^7(" .. #main.popups .. ")"
	end, controls, "ok", "edit", "cancel")
end

function G:OpenScroll()
	local controls = { }
	controls.scrollBar = new("ScrollBarControl"):ScrollBarControl({"TOPRIGHT", nil, "TOPRIGHT"}, {-4, 30, 18, 150}, 16)
	controls.offset = new("LabelControl"):LabelControl(nil, {0, 60, 0, 16}, function()
		return "^7offset " .. controls.scrollBar.offset
	end)
	main:OpenPopup(300, 200, "Scroll", controls, nil, nil, nil, function()
		controls.scrollBar:SetContentDimension(600, 150)
	end)
end

function G:OnFrame(inputEvents, viewPort)
	if opts.pinScreenH then
		main.screenH = opts.pinScreenH
	end
	-- Main.lua:402-407
	if main.popups[1] then
		main.popups[1]:ProcessInput(inputEvents, viewPort)
		wipeTable(inputEvents)
	else
		self.host:ProcessControlsInput(inputEvents, viewPort)
	end
	SetDrawColor(0.1, 0.1, 0.1)
	DrawImage(nil, viewPort.x, viewPort.y, viewPort.width, viewPort.height)
	self.host:DrawControls(viewPort)
	-- Main.lua:437-443
	if main.popups[1] then
		SetDrawLayer(10)
		SetDrawColor(0, 0, 0, 0.5)
		DrawImage(nil, 0, 0, main.screenW, main.screenH)
		main.popups[1]:Draw(main.viewPort)
		SetDrawLayer(0)
	end
end

return G
