-- lua2rs golden fixture: a miniature ConfigOptions.lua (ConfigOptions.lua @16de4b82d shapes).
local m_min = math.min
local m_max = math.max

local function mapAffixDropDownFunction(val, modList, enemyModList, build)
end

return {
	{ section = "General", col = 1 },
	{ var = "conditionMoving", type = "check", label = "Moving?", apply = function(val, modList, enemyModList)
		modList:NewMod("Condition:Moving", "FLAG", true, "Config")
	end },
	{ var = "conditionStationary", type = "countAllowZero", label = "Stationary:", apply = function(val, modList, enemyModList)
		if type(val) == "boolean" then
			-- Backwards compatibility with older versions that set this condition as a boolean
			val = val and 1 or 0
		end
		local sanitizedValue = m_max(0, val)
		modList:NewMod("Multiplier:StationarySeconds", "BASE", sanitizedValue, "Config")
		if sanitizedValue > 0 then
			modList:NewMod("Condition:Stationary", "FLAG", true, "Config")
		end
	end },
	{ var = "lifeRegenMode", type = "list", list = {{val="MIN",label="Minimum"},{val="AVERAGE",label="Average"}}, apply = function(val, modList, enemyModList)
		if val == "AVERAGE" then
			modList:NewMod("Condition:LifeRegenBurstAvg", "FLAG", true, "Config")
		elseif val == "FULL" then
			modList:NewMod("Condition:LifeRegenBurstFull", "FLAG", true, "Config")
		end
	end },
	{ var = "minionsConditionFullLife", type = "check", apply = function(val, modList, enemyModList)
		modList:NewMod("MinionModifier", "LIST", { mod = modLib.createMod("Condition:FullLife", "FLAG", true, "Config") })
	end },
	{ var = "touchedDebuffsCount", type = "countAllowZero", apply = function(val, modList, enemyModList)
		val = m_min(val, 10)
		modList:NewMod("DamageTaken", "INC", val * 6, val.." Eroding Touch Stacks", { type = "GlobalEffect", effectType = "Debuff" })
	end },
	{ var = "multiplierMapModEffect", type = "count", apply = function(val, modList, enemyModList, build)
		modList:NewMod("Dummy", "BASE", build.configTab.input['multiplierMapModEffect'] or 0, "Config")
		enemyModList:NewMod("Condition:Effective", "FLAG", val >= 1, "Config", { type = "Condition", var = "Combat" })
	end },
	{ var = "enemyRadius", type = "integer", apply = function(val, modList, enemyModList, build)
		build.configTab.varControls['enemyRadius']:SetPlaceholder(2, false)
	end },
	{ var = "MapPrefix1", type = "list", apply = mapAffixDropDownFunction },
}
