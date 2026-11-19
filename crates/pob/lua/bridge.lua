-- Build Doctor bridge.
--
-- Starts Path of Building PoE2 through PoB's own HeadlessWrapper.lua (which
-- PoB's developers keep in step with each release) inside a sandbox, then
-- exposes a few functions to the Rust side. Nothing in this file computes a
-- game number: it loads builds, lets PoB calculate, and hands back what PoB
-- wrote.
--
-- Chunk arguments: absolute PoB src directory and runtime/lua directory,
-- both ending in a path separator.

local SRC, LIB = ...

---------------------------------------------------------------------------
-- Sandbox
---------------------------------------------------------------------------

-- Relative paths resolve against PoB's src directory, so the process working
-- directory never matters and is never changed.
local function resolve(path)
	if type(path) ~= "string" or path:match("^[/\\]") or path:match("^%a:[/\\]") then
		return path
	end
	return SRC .. path
end

-- PoB may read its own files. It may not write, delete, rename or execute
-- anything. Each blocked call fails the way the real call reports failure.
local rawOpen, rawLines, rawLoadfile = io.open, io.lines, loadfile
io.open = function(path, mode)
	mode = mode or "r"
	if not mode:match("^r") or mode:find("+", 1, true) then
		return nil, "build-doctor: writing files is disabled"
	end
	return rawOpen(resolve(path), mode)
end
io.lines = function(path, ...)
	return rawLines(resolve(path), ...)
end
io.read = function() return nil end
io.popen = function() return nil, "build-doctor: running programs is disabled" end
loadfile = function(path, ...)
	return rawLoadfile(resolve(path), ...)
end
dofile = function(path)
	return assert(loadfile(path))()
end
os.execute = function() return nil end
os.remove = function() return nil, "build-doctor: deleting files is disabled" end
os.rename = function() return nil, "build-doctor: renaming files is disabled" end
os.exit = function() error("build-doctor: PoB tried to exit", 2) end
local rawInput, rawOutput = io.input, io.output
io.input = function(file)
	if file ~= nil then error("build-doctor: io.input redirection is disabled", 2) end
	return rawInput()
end
io.output = function(file)
	if file ~= nil then error("build-doctor: io.output redirection is disabled", 2) end
	return rawOutput()
end
package.loadlib = nil
-- No environment leaks into calculations. This also keeps PoB's ModCache on
-- (HeadlessWrapper disables it when CI is set), matching the desktop app.
os.getenv = function() return nil end

package.path = SRC .. "?.lua;" .. LIB .. "?.lua;" .. LIB .. "?/init.lua"
package.cpath = ""
arg = { }

-- PoB prints progress and warnings; keep the most recent lines for errors.
local LOG_LIMIT = 500
local log = { }
print = function(...)
	local parts = { }
	for i = 1, select("#", ...) do
		parts[i] = tostring((select(i, ...)))
	end
	log[#log + 1] = table.concat(parts, "\t")
	if #log > LOG_LIMIT then
		table.remove(log, 1)
	end
end

---------------------------------------------------------------------------
-- Start PoB
---------------------------------------------------------------------------

dofile("HeadlessWrapper.lua")

local function takePrompt()
	local msg = launch and launch.promptMsg
	if msg then
		launch.promptMsg = nil
		return (msg:gsub("%^x%x%x%x%x%x%x", ""):gsub("%^%d", ""))
	end
end

if not build then
	error("PoB did not reach the build screen: " .. (takePrompt() or "no error message"), 0)
end

-- PoB enters developer mode when run from a repository copy (its manifest
-- has no branch/platform), which the installed desktop app never does. Dev
-- mode changes no calculation, but it auto-saves a temp build file whenever a
-- build is replaced. Match the app.
launch.devMode = false

-- PoB reports some load problems (for example an unknown passive tree
-- version) only as a popup. Record popups instead of opening them, so a build
-- PoB could not load properly is an error rather than a set of wrong numbers.
local popups = { }
local function recordPopup(_, title, msg)
	popups[#popups + 1] = tostring(title) .. ": " .. tostring(msg):gsub("\n+", " ")
end
main.OpenMessagePopup = recordPopup
main.OpenConfirmPopup = recordPopup

---------------------------------------------------------------------------
-- API used by the Rust side
---------------------------------------------------------------------------

local MAX_PASSES = 8

local function frame()
	runCallback("OnFrame")
	local prompt = takePrompt()
	if prompt then
		error(prompt, 0)
	end
end

-- Recalculates until PoB's saved output stops changing, then returns that
-- output and the number of passes it took. PoB can need more than one pass
-- after a load (for example when the character level is derived from the
-- tree), so a single recalculation is not trusted. If the output never
-- stabilises, returns nil plus the last two outputs so the caller can show
-- what kept changing.
function bd_settle()
	local previous
	for pass = 1, MAX_PASSES do
		build.buildFlag = true
		frame()
		local saved = build:SaveDB("build-doctor")
		if not saved then
			error(takePrompt() or "PoB could not serialise the build", 0)
		end
		if saved == previous then
			return saved, pass
		end
		if pass == MAX_PASSES then
			return nil, pass, previous, saved
		end
		previous = saved
	end
end

local loadedXml -- PoB's settled output for the loaded build, unmodified

-- What-ifs leave PoB's outputs describing the what-if. Anything that reads
-- PoB's state afterwards first recalculates the original build and checks
-- that PoB's output is byte-identical to the original, so a what-if can never
-- leak into later numbers.
local dirty = false

local function ensureOriginal()
	if not dirty then
		return
	end
	local restored = bd_settle()
	if restored ~= loadedXml then
		error("PoB did not return to the build's original state after a what-if", 0)
	end
	dirty = false
end


function bd_load(xml)
	popups = { }
	loadBuildFromXML(xml, "build-doctor")
	local prompt = takePrompt()
	if prompt then
		error(prompt, 0)
	end
	if #popups > 0 then
		error(table.concat(popups, "\n"), 0)
	end
	local saved, passes, before, after = bd_settle()
	loadedXml = saved
	dirty = false
	return saved, passes, before, after
end

-- Who the loaded build is, as PoB sees it. Read-only.
function bd_identity()
	ensureOriginal()
	local spec = build.spec
	local env = build.calcsTab.mainEnv
	local skill = env and env.player and env.player.mainSkill
	local effect = skill and skill.activeEffect and skill.activeEffect.grantedEffect
	return {
		pobVersion = launch.versionNumber,
		className = spec and spec.curClassName,
		ascendancy = spec and spec.curAscendClassName,
		level = build.characterLevel,
		treeVersion = spec and spec.treeVersion,
		latestTreeVersion = latestTreeVersion,
		mainSkill = effect and effect.name,
	}
end

-- The configuration the loaded build's numbers depend on: every option the
-- build sets that PoB considers relevant to it (PoB's own config-tab
-- visibility rules), plus the enemy type, which always matters. Read-only.
--
-- Each is either a "condition" (a state the player has to keep up in a fight,
-- such as "Is the enemy Shocked?" or a custom modifier) or "scenario" (what
-- fight is being simulated and how PoB calculates it: Enemy Stats, the
-- General section's calculation-mode lists such as the resistance penalty,
-- and campaign rewards).
local configOptions = LoadModule("Modules/ConfigOptions")
local ALWAYS_SHOWN = { enemyIsBoss = true }

local SCENARIO_SECTIONS = { ["Enemy Stats"] = true, ["Quest Rewards"] = true }

local function assumptionKind(section, varData)
	if SCENARIO_SECTIONS[section] or (section == "General" and varData.type == "list") then
		return "scenario"
	end
	return "condition"
end

local function displayValue(varData, value)
	if varData.type == "list" then
		for _, entry in ipairs(varData.list or { }) do
			if entry.val == value then
				return StripEscapes(tostring(entry.label))
			end
		end
	elseif varData.type == "check" then
		return value and "yes" or "no"
	end
	return tostring(value)
end

function bd_assumptions()
	ensureOriginal()
	local configTab = build.configTab
	local input = configTab.input
	local out, section = { }, ""
	for _, varData in ipairs(configOptions) do
		if varData.section then
			section = varData.section
		elseif varData.var then
			local value = input[varData.var]
			if value == nil and ALWAYS_SHOWN[varData.var] and varData.list then
				local default = varData.list[varData.defaultIndex or 1]
				value = default and default.val
			end
			local control = configTab.varControls[varData.var]
			local relevant = control and control:GetProperty("shown")
			if value ~= nil and value ~= false and value ~= "" and relevant then
				local label = StripEscapes(tostring(varData.label or "")):gsub(":%s*$", "")
				out[#out + 1] = {
					var = varData.var,
					kind = assumptionKind(section, varData),
					section = section,
					label = label ~= "" and label or section,
					value = displayValue(varData, value),
				}
			end
		end
	end
	return out
end

-- Recalculates the loaded build with the given config options unset (as if
-- unticked or cleared in PoB's config tab) and returns PoB's settled output.
-- The options are put back immediately; the recalculation of the original
-- happens lazily (see ensureOriginal).
function bd_without_config(vars)
	assert(loadedXml, "no build loaded")
	local configTab = build.configTab
	local input = configTab.input
	local saved = { }
	for _, var in ipairs(vars) do
		saved[var] = input[var]
		input[var] = nil
	end
	configTab:BuildModList()
	dirty = true
	local ok, xml, passes = pcall(bd_settle)
	for var, value in pairs(saved) do
		input[var] = value
	end
	configTab:BuildModList()
	if not ok then
		error(xml, 0)
	end
	return xml, passes
end

-- Recalculates the original build if a what-if ran since, and checks PoB's
-- output is byte-identical to the original load.
function bd_ensure_original()
	ensureOriginal()
end

function bd_loaded_xml()
	return loadedXml
end

-- Mod lines on equipped items and socketed jewels that PoB cannot parse
-- (red in PoB's item tooltips). No number PoB produces includes them.
function bd_unsupported()
	ensureOriginal()
	local out = { }
	local itemsTab = build.itemsTab
	local function scan(slotName, item)
		for _, list in ipairs({ item.enchantModLines, item.runeModLines, item.implicitModLines, item.explicitModLines, item.buffModLines }) do
			for _, modLine in ipairs(list or { }) do
				local active = not item.CheckModLineVariant or item:CheckModLineVariant(modLine)
				if modLine.extra and active then
					out[#out + 1] = { slot = slotName, item = tostring(item.name), line = StripEscapes(tostring(modLine.line)) }
				end
			end
		end
	end
	-- orderedSlots includes tree jewel sockets; only allocated ones count.
	for _, slot in ipairs(itemsTab.orderedSlots) do
		local item = slot.selItemId and itemsTab.items[slot.selItemId]
		local allocated = not slot.nodeId or build.spec.allocNodes[slot.nodeId]
		if item and not slot.inactive and allocated then
			scan(slot.slotName, item)
		end
	end
	table.sort(out, function(a, b) return a.slot < b.slot or (a.slot == b.slot and a.line < b.line) end)
	return out
end

function bd_take_log()
	local out = log
	log = { }
	return out
end
