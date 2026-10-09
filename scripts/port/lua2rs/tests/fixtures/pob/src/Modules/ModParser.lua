-- lua2rs golden fixture: a miniature ModParser.lua with the list shapes of the real one
-- (ModParser.lua @16de4b82d). Each entry exercises one translation rule.
local t_insert = table.insert
local bor = bit.bor
local function firstToUpper(str)
	return (str:gsub("^%l", string.upper))
end
-- List of modifier forms
local formList = {
	["^(%d+)%% increased"] = "INC",
	["^([%+%-][%d%.]+)%%?"] = "BASE", -- trailing comment
	["(%d+) to (%d+) added (%a+) damage"] = "DMG",
}

-- Map of modifier names
local modNameList = {
	-- Attributes
	["strength"] = "Str",
	["strength and dexterity"] = { "Str", "Dex", "StrDex" },
	["mana cost of attacks"] = { "ManaCost", tag = { type = "SkillType", skillType = SkillType.Attack } },
	["with \"quotes\" and \\ backslash"] = "Quoted",
	["global"] = { tag = { type = "Global" } },
}

-- List of modifier flags
local modFlagList = {
	["with axes"] = { flags = bor(ModFlag.Axe, ModFlag.Hit) },
	["with axes or swords"] = { flags = ModFlag.Hit, tag = { type = "ModFlagOr", modFlags = bor(ModFlag.Axe, ModFlag.Sword) } },
	["minion"] = { addToMinion = true, keywordFlags = KeywordFlag.Hit },
	["by enemies killed with a critical strike"] = { tagList = { { type = "Condition", var = "CritRecently" }, { type = "Condition", var = "KilledRecently" } } },
	["unknown key"] = { flags = 0, someNewKey = "x" },
}

-- List of modifier flags/tags that appear at the start of a line
local preFlagList = {
	["^minions "] = { addToMinion = true },
	["^enemies (%a+) by you take "] = function(cond) return { tag = { type = "Condition", var = cond:gsub("^%a", string.upper) }, applyToEnemy = true, modSuffix = "Taken" } end,
	["^while a unique enemy is in your presence, minions [hd][ae][va][el] "] = { addToMinion = true, playerTag = { type = "ActorCondition", actor = "enemy", var = "RareOrUnique" } },
}

-- List of modifier tags
local modTagList = {
	["on enemies"] = { },
	["per (%d+) rage"] = function(num) return { tag = { type = "Multiplier", var = "Rage", div = num } } end,
	["while holding a (%w+) or (%w+)"] = function (_, g1, g2) return { tag = { type = "Condition", varList = { "Using"..firstToUpper(g1), "Using"..firstToUpper(g2) } } } end,
	["if you have (%a+) charges"] = function(kind) return { tag = { type = "Condition", var = "Have"..kind } } end,
	["per (.+) jewel"] = function(type) return { tag = { type = "Multiplier", var = type } } end,
	["mod before its local (%d+)"] = function(num) return { other = mod("A", "BASE", num) } end, -- `mod` is still a global here
	["when you warcry"] = { tag = { type = "Condition", var = "UsedWarcryRecently" } },
	["when you warcry"] = { tag = { type = "Condition", var = "UsedWarcryInPast8Seconds" } },
}

local mod = modLib.createMod
local function flag(name, ...)
	return mod(name, "FLAG", true, ...)
end
local function triggerExtraSkill(name, level, options)
	return { }
end
local explodeFunc = function(chance, amount, type, ...)
	return { }
end
-- List of special modifiers
local specialModList = {
	-- statics
	["cannot be stunned"] = { flag("AvoidStun") },
	["removes all energy shield"] = { mod("EnergyShield", "MORE", -100) },
	["projectiles from spells cannot pierce"] = { flag("CannotPierce", nil, ModFlag.Spell) },
	["source then tag"] = { mod("Life", "BASE", 1, "Src", { type = "Condition", var = "X" }) },
	["number in source slot"] = { mod("Life", "BASE", 1, 0, KeywordFlag.Hit, { type = "Condition", var = "Y" }) },
	["nil hole before tag"] = { mod("ProjectileCount", "MORE", 5, nil, { type = "SkillName", skillName = "Caustic Arrow" }) },
	["nested mod value"] = { mod("ExtraSkillMod", "LIST", { mod = mod("ProjectileCount", "BASE", 1) }, { type = "SocketedIn", slotName = "{SlotName}" }) },
	["list value with nil field"] = { mod("ExtraSupport", "LIST", { skillId = "SupportX", level = 1, noSupports = nil }) },
	["unknown skilltype"] = { mod("CooldownRecovery", "OVERRIDE", 0, nil, 0, KeywordFlag.Warcry, { type = "SkillType", skillType = SkillType.Totem }) },
	["ramp"] = { mod("Damage", "MORE", 10, { type = "DistanceRamp", ramp = {{35,0},{70,1}} }) },
	["empty"] = { },
	-- one-line closures
	["(%d+)%% increased area"] = function(num) return { mod("AreaOfEffect", "INC", num) } end,
	["(%d+)%% reduced area"] = function(num) return { mod("AreaOfEffect", "INC", -num) } end,
	["(%d+)%% of (%a+) as extra"] = function(num, _, dmgType) return { mod("PhysicalDamageGainAs"..firstToUpper(dmgType), "BASE", num) } end,
	["(%d+)%% per (%d+) (%a+) up to (%d+)"] = function(num, _, div, stat, limit) return { mod("Damage", "INC", num, { type = "PerStat", stat = stat, div = div, limit = tonumber(limit) }) } end,
	["per (%a+) of (%a+)"] = function(_, div, stat) return { mod("Damage", "INC", 1, { type = "PerStat", stat = stat, div = div }) } end,
	["trigger level (%d+) (.+) on kill"] = function(num, _, skill) return triggerExtraSkill(skill, num, { onCrit = true, triggerChance = 50 }) end,
	["explode (%d+)%% as (.+)"] = function(amount, _, type) return explodeFunc(100, amount, type) end,
	["gsub flag (%a+)"] = function(_, name) return { flag(name:gsub(" ", "")) } end,
	["gsub source (%a+)"] = function(_, name) return { mod("Life", "BASE", 1, name:gsub(" ", "")) } end,
	["gsub upper (%a+)"] = function(_, name) return { flag("Condition:Have"..name:gsub("^%l", string.upper):gsub(" ", "")) } end,
	["data constant"] = function(num) return { mod("ShockBase", "BASE", data.nonDamagingAilment["Shock"].default) } end,
	-- multi-statement closure
	["minions convert (%d+)%% of (.+) damage per socketed (%a+) gem"] = function(num, _, source, colour)
		if colour == "red" or colour == "green" then
			local var = string.format("Socketed%sGemsIn{SlotName}", colour:gsub("^%l", string.upper))
			return {
				mod("MinionModifier", "LIST", { mod = mod(firstToUpper(source) .. "DamageConvertToFire", "BASE", num) }, { type = "Multiplier", var = var })
			}
		end
	end,
	-- untranslatable
	["vararg"] = function(...) return { mod("X", "BASE", select(1, ...)) } end,
}
