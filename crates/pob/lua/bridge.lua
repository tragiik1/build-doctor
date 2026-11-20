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

-- Start-up leaves a lot of PoB's data parsing behind as garbage. Collect it
-- now, so every engine starts small and peak memory stays down.
collectgarbage("collect")

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

-- PoB keeps allocated nodes in hash tables, so after a tree what-if is
-- undone the same nodes can be written in a different order (in every
-- "…nodes" list, e.g. nodes, intNodes, and in the tree URL that encodes them). The canonical form sorts node lists
-- and drops tree URLs; everything else, every stat included, must match
-- character for character.
local function canonical(xml)
	xml = xml:gsub('(%a*[Nn]odes)="([%d,]*)"', function(name, list)
		local ids = { }
		for id in list:gmatch("%d+") do ids[#ids + 1] = tonumber(id) end
		table.sort(ids)
		return name .. '="' .. table.concat(ids, ",") .. '"'
	end)
	return (xml:gsub("https://www%.pathofexile%.com/passive%-skill%-tree/[%w%-_=]*", "<tree url>"))
end

local function ensureOriginal()
	if not dirty then
		return
	end
	local restored = bd_settle()
	if restored ~= loadedXml and canonical(restored or "") ~= canonical(loadedXml or "") then
		local a, b = { }, { }
		for line in canonical(loadedXml or ""):gmatch("[^\n]+") do a[#a + 1] = line end
		for line in canonical(restored or ""):gmatch("[^\n]+") do b[#b + 1] = line end
		local diffs = { }
		for i = 1, math.max(#a, #b) do
			if a[i] ~= b[i] and #diffs < 3 then
				diffs[#diffs + 1] = (a[i] or ""):gsub("^%s+", ""):sub(1, 160) .. " -> " .. (b[i] or ""):gsub("^%s+", ""):sub(1, 160)
			end
		end
		error("PoB did not return to the build's original state after a what-if: " .. table.concat(diffs, " | "), 0)
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
	collectgarbage("collect")
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

-- PoB's own warnings for the current calculation (unmet attribute
-- requirements, negative Spirit, too many points, …), as its sidebar shows.
local function currentWarnings()
	local out = { }
	for _, line in ipairs(build.controls.warnings and build.controls.warnings.lines or { }) do
		out[#out + 1] = StripEscapes(tostring(line))
	end
	return out
end

-- Numbers from PoB's main output table (the player's calculation, main
-- skill included), including the ones PoB does not save, such as
-- MissingFireResist. Booleans read as 1 or 0; keys PoB did not set are left
-- out.
local function outputValues(keys)
	local output = build.calcsTab.mainOutput or { }
	local out = { }
	for _, key in ipairs(keys) do
		local value = output[key]
		if type(value) == "number" then
			out[key] = value
		elseif type(value) == "boolean" then
			out[key] = value and 1 or 0
		end
	end
	return out
end

-- The least Spirit any skill or support in PoB's data reserves (flat, at
-- any level), or nil if none does.
function bd_cheapest_spirit()
	local least
	for _, skill in pairs(data.skills) do
		for _, level in pairs(skill.levels or { }) do
			local cost = type(level) == "table" and level.spiritReservationFlat
			if cost and cost > 0 and (not least or cost < least) then
				least = cost
			end
		end
	end
	return least
end

function bd_outputs(keys)
	ensureOriginal()
	return outputValues(keys)
end

local function pack(...)
	return { n = select("#", ...), ... }
end

-- Runs `calculate` with extra modifier lines added to PoB's Custom Modifiers
-- box, exactly as if typed there, then puts the box back. Every line must be
-- one PoB reads completely; anything else is an error, never silently
-- ignored. The original is recalculated lazily (see ensureOriginal).
local function withCustomMods(lines, calculate)
	assert(loadedXml, "no build loaded")
	for _, line in ipairs(lines) do
		local mods, extra = modLib.parseMod(line)
		if not mods or extra then
			error('PoB cannot read the modifier "' .. line .. '"', 0)
		end
	end
	local configTab = build.configTab
	local before = configTab.input.customMods
	local added = table.concat(lines, "\n")
	configTab.input.customMods = (before and before ~= "") and (before .. "\n" .. added) or added
	configTab:BuildModList()
	dirty = true
	local results = pack(pcall(calculate))
	configTab.input.customMods = before
	configTab:BuildModList()
	if not results[1] then
		error(results[2], 0)
	end
	return unpack(results, 2, results.n)
end

-- The build with extra modifier lines: PoB's settled output, the requested
-- output values and PoB's warnings.
function bd_with_mods(lines, keys)
	return withCustomMods(lines, function()
		local xml, passes = bd_settle()
		return xml, passes, xml and outputValues(keys), xml and currentWarnings()
	end)
end

-- One recalculation with extra modifier lines, returning only the requested
-- output values: a quick probe for searches, whose answer is then confirmed
-- with bd_with_mods.
function bd_probe_mods(lines, keys)
	return withCustomMods(lines, function()
		build.buildFlag = true
		frame()
		return outputValues(keys)
	end)
end

---------------------------------------------------------------------------
-- Passive tree
---------------------------------------------------------------------------

function bd_warnings()
	ensureOriginal()
	return currentWarnings()
end

-- Normal passive points spent (what the point budget and auto-level use).
local function normalPoints()
	local used, _, _, _, ws1, ws2 = build.spec:CountAllocNodes()
	return used - math.min(ws1 or 0, ws2 or 0)
end

-- The numbers a ranking needs, from one calculator output table.
local function summarise(output)
	local minHit
	for _, t in ipairs({ "Physical", "Fire", "Cold", "Lightning", "Chaos" }) do
		local v = output[t .. "MaximumHitTaken"]
		if v and (not minHit or v < minHit) then
			minHit = v
		end
	end
	return {
		fullDps = output.FullDPS or 0,
		combinedDps = output.CombinedDPS or 0,
		minionDps = output.Minion and output.Minion.CombinedDPS or 0,
		ehp = output.TotalEHP or 0,
		minHit = minHit or 0,
	}
end

local function isTreeCandidate(node)
	return node.type ~= "ClassStart" and node.type ~= "AscendClassStart" and node.type ~= "Socket"
		and not node.ascendancyName and not node.isFreeAllocate and not node.isAttribute
		and node.modKey ~= nil and node.modKey ~= ""
end

-- A node as the Rust side sees it: id, name, type and its stat lines.
local function nodeInfo(node)
	local stats = { }
	for _, line in ipairs(node.sd or { }) do
		stats[#stats + 1] = StripEscapes(tostring(line))
	end
	return { id = node.id, name = node.dn, kind = node.type, stats = stats }
end

-- Weapon set passives (allocMode 1 or 2) have their own point budgets in
-- PoE2, so a swap only ever frees points from the shared main tree.
local function mainTreeOnly(nodes)
	for _, node in ipairs(nodes) do
		if (node.allocMode or 0) ~= 0 then
			return false
		end
	end
	return true
end

local function nodeIds(list)
	local ids = { }
	for _, node in ipairs(list) do
		ids[#ids + 1] = node.id
	end
	return ids
end

-- Screening with PoB's own node calculator (the one behind its power
-- report): every unallocated node within `maxDistance` points, allocated
-- along its path, and every allocated node removed with the nodes that
-- depend on it. Fast and approximate (PoB reuses cached skill results);
-- anything recommended is re-checked exactly with bd_try_tree.
--
-- `part` and `parts` split the work between engines: this call screens the
-- nodes whose position in id order is `part` modulo `parts`.
function bd_tree_screen(maxDistance, part, parts)
	ensureOriginal()
	local spec = build.spec
	local calcFunc, baseOutput = build.calcsTab:GetMiscCalculator()
	local granted = build.calcsTab.mainEnv.grantedPassives or { }
	local ids = { }
	for id in pairs(spec.nodes) do ids[#ids + 1] = id end
	table.sort(ids)
	local adds, removes = { }, { }
	for index, id in ipairs(ids) do
		local node = spec.nodes[id]
		if (index - 1) % parts == part and isTreeCandidate(node) and not granted[id] then
			if not node.alloc and node.path and node.pathDist and node.pathDist <= maxDistance then
				local path = { }
				for _, pathNode in ipairs(node.path) do
					path[pathNode] = true
				end
				adds[#adds + 1] = {
					node = nodeInfo(node),
					nodes = nodeIds(node.path), cost = #node.path,
					out = summarise(calcFunc({ addNodes = path }, true)),
				}
			elseif node.alloc and node.depends and #node.depends > 0 and mainTreeOnly(node.depends) then
				local remove, cost = { }, 0
				for _, dep in ipairs(node.depends) do
					remove[dep] = true
					if not dep.ascendancyName and dep.type ~= "ClassStart" then
						cost = cost + 1
					end
				end
				removes[#removes + 1] = {
					node = nodeInfo(node),
					nodes = nodeIds(node.depends), cost = cost,
					out = summarise(calcFunc({ removeNodes = remove }, true)),
				}
			end
		end
	end
	return { base = summarise(baseOutput), adds = adds, removes = removes }
end

-- Screens one combined change with PoB's node calculator.
function bd_tree_screen_change(removeIds, addIds)
	ensureOriginal()
	local spec = build.spec
	local calcFunc = build.calcsTab:GetMiscCalculator()
	local add, remove = { }, { }
	for _, id in ipairs(addIds) do add[spec.nodes[id]] = true end
	for _, id in ipairs(removeIds) do remove[spec.nodes[id]] = true end
	return summarise(calcFunc({ addNodes = add, removeNodes = remove }, true))
end

-- Applies a tree change exactly as clicking in PoB would (DeallocNode for
-- each removal, then AllocNode for each target along PoB's own path), lets
-- PoB recalculate fully, and returns its saved output together with what
-- actually changed. The tree is then restored from PoB's undo state; the
-- byte-identical check in ensureOriginal guards everything after.
function bd_try_tree(removeIds, addIds)
	ensureOriginal()
	local spec = build.spec
	local undo = spec:CreateUndoState()
	local before = { }
	for id in pairs(spec.allocNodes) do before[id] = true end
	local pointsBefore = normalPoints()
	local warningsBefore = { }
	for _, w in ipairs(currentWarnings()) do warningsBefore[w] = true end

	dirty = true
	local ok, result = pcall(function()
		for _, id in ipairs(removeIds) do
			local node = spec.nodes[id]
			if node and node.alloc then spec:DeallocNode(node) end
		end
		for _, id in ipairs(addIds) do
			local node = spec.nodes[id]
			if node and not node.alloc then spec:AllocNode(node) end
		end
		local added, removed = { }, { }
		for id, node in pairs(spec.allocNodes) do
			if not before[id] then added[#added + 1] = nodeInfo(node) end
		end
		for id in pairs(before) do
			if not spec.allocNodes[id] then
				removed[#removed + 1] = nodeInfo(spec.nodes[id])
			end
		end
		local pointsAfter = normalPoints()
		local xml, passes = bd_settle()
		local newWarnings = { }
		for _, w in ipairs(currentWarnings()) do
			if not warningsBefore[w] then newWarnings[#newWarnings + 1] = w end
		end
		return { xml = xml, passes = passes, added = added, removed = removed,
			pointsBefore = pointsBefore, pointsAfter = pointsAfter, newWarnings = newWarnings }
	end)
	spec:RestoreUndoState(undo)
	build.buildFlag = true
	if not ok then
		error(result, 0)
	end
	return result
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

---------------------------------------------------------------------------
-- Items
---------------------------------------------------------------------------

-- Errors meant for the user start with "[user] "; the engine shows them as
-- written instead of as a PoB failure.

-- An item from its text (as the game copies it with Ctrl+C, or as PoB
-- writes it), exactly as given: quality and rolls are not changed.
local function readItem(raw)
	local ok, item = pcall(new, "Item", raw)
	if not ok or not item.base then
		error("[user] PoB cannot read this item. Copy it in game with Ctrl+C and paste all of its text.", 0)
	end
	return item
end

-- The slots an item can go in, with a display label: the active item
-- set's shown equipment slots (not weapon swap) and the allocated passive
-- tree jewel sockets, numbered in tree order ("Jewel socket 2").
local function equipSlots(item)
	local itemsTab = build.itemsTab
	local out, sockets = { }, { }
	for _, slot in ipairs(itemsTab.orderedSlots) do
		if slot.nodeId then
			if build.spec.allocNodes[slot.nodeId] then
				sockets[#sockets + 1] = slot
			end
		elseif not slot.slotName:find("Swap") and slot:IsShown() and itemsTab:IsItemValidForSlot(item, slot.slotName) then
			out[#out + 1] = { slot = slot, label = slot.slotName }
		end
	end
	table.sort(sockets, function(a, b) return a.nodeId < b.nodeId end)
	for i, slot in ipairs(sockets) do
		if itemsTab:IsItemValidForSlot(item, slot.slotName) then
			out[#out + 1] = { slot = slot, label = "Jewel socket " .. i }
		end
	end
	return out
end

-- What PoB makes of an item: its name, the slots it fits (with what the
-- build has there now) and the lines PoB cannot read.
function bd_item_info(raw)
	ensureOriginal()
	local item = readItem(raw)
	local slots, unread = { }, { }
	for _, fit in ipairs(equipSlots(item)) do
		local current = build.itemsTab.items[fit.slot.selItemId]
		slots[#slots + 1] = { slot = fit.slot.slotName, label = fit.label, current = current and current.name or nil }
	end
	for _, list in ipairs({ item.enchantModLines, item.runeModLines, item.implicitModLines, item.explicitModLines }) do
		for _, modLine in ipairs(list or { }) do
			if modLine.extra then
				unread[#unread + 1] = modLine.line
			end
		end
	end
	return { name = item.name, rarity = item.rarity, slots = slots, unread = unread }
end

-- The build with the item equipped in `slotName` instead of what is there:
-- PoB's settled output and its warnings. The item is removed again
-- afterwards; the original is recalculated lazily (see ensureOriginal).
function bd_with_item(raw, slotName)
	assert(loadedXml, "no build loaded")
	local itemsTab = build.itemsTab
	local item = readItem(raw)
	local slot
	for _, fit in ipairs(equipSlots(item)) do
		if fit.slot.slotName == slotName then
			slot = fit.slot
		end
	end
	if not slot then
		error("[user] This item cannot go in " .. tostring(slotName), 0)
	end
	local previous = slot.selItemId
	itemsTab:AddItem(item, true)
	slot:SetSelItemId(item.id)
	dirty = true
	local ok, xml, passes = pcall(bd_settle)
	local warnings = ok and currentWarnings()
	slot:SetSelItemId(previous)
	for i, id in ipairs(itemsTab.itemOrderList) do
		if id == item.id then
			table.remove(itemsTab.itemOrderList, i)
			break
		end
	end
	itemsTab.items[item.id] = nil
	itemsTab:PopulateSlots()
	if not ok then
		error(xml, 0)
	end
	return xml, passes, warnings
end

---------------------------------------------------------------------------
-- Gear: what each stat is worth in a slot
---------------------------------------------------------------------------

local tradeHelpers = LoadModule("Classes/TradeHelpers")
-- PoB's table of trade-site stats with their roll ranges per item
-- category (Data/QueryMods.lua, generated from game data), loaded once.
local queryMods

local function gearSlot(slotName)
	local itemsTab = build.itemsTab
	local slot = itemsTab.slots[slotName]
	local item = slot and not slot.nodeId and itemsTab.items[slot.selItemId]
	if not item then
		return nil
	end
	local tradeCategory, category = tradeHelpers.getTradeCategory(slotName, item)
	if not category or slotName:find("Flask") or slotName:find("Charm") then
		return nil
	end
	return slot, item, tradeCategory, category
end

-- Equipment slots worn with an item that a trade search can replace: the
-- active item set's shown slots, not weapon swap, flasks or charms.
function bd_gear_slots()
	ensureOriginal()
	local out = { }
	for _, slot in ipairs(build.itemsTab.orderedSlots) do
		if not slot.slotName:find("Swap") and slot:IsShown() then
			local _, item, tradeCategory, category = gearSlot(slot.slotName)
			if item then
				out[#out + 1] = {
					slot = slot.slotName,
					item = item.name,
					base = item.baseName,
					rarity = item.rarity,
					category = category,
					tradeCategory = tradeCategory,
				}
			end
		end
	end
	return out
end

-- Trade stats that can roll on an item category, in a fixed order so
-- workers can split them.
local function statCandidates(category)
	queryMods = queryMods or LoadModule("Data/QueryMods.lua")
	local list = { }
	for _, group in ipairs({ "Explicit", "Implicit" }) do
		for _, entry in pairs(queryMods[group] or { }) do
			if entry[category] then
				list[#list + 1] = entry
			end
		end
	end
	table.sort(list, function(a, b) return a.tradeMod.id < b.tradeMod.id end)
	return list
end

-- The mod line for a trade stat at the middle of its roll range for the
-- category, written the way PoB's own trade search tests it; nil for stats
-- that only roll negative values.
local function midRollLine(entry, category)
	local range = entry[category]
	local value = math.ceil((range.max - range.min) * 0.5 + range.min)
	if value < 0 then
		return nil
	end
	local special = entry.specialCaseData or { }
	local line
	if value == 1 and special.overrideModLineSingular then
		line = special.overrideModLineSingular
	elseif value ~= 1 and special.overrideModLinePlural then
		line = special.overrideModLinePlural
	else
		line = special.overrideModLine or entry.tradeMod.text
	end
	if line:find("+#") then
		line = line:gsub("#", value)
	elseif entry.usePositiveSign then
		line = line:gsub("#", "+" .. value)
	else
		line = line:gsub("+?#", value)
	end
	return (line:gsub("%(Local%)", "")), value
end

-- What each trade stat that can roll in `slotName` is worth: a blank rare
-- of the worn item's base replaces the worn item, then gets the stat alone
-- at a mid roll, measured with PoB's calculator (the one behind its own
-- trade search; it does not change the build). Only every `parts`-th
-- candidate starting at `part` is measured, so workers can split them.
function bd_stat_values(slotName, part, parts)
	ensureOriginal()
	local slot, item, _, category = gearSlot(slotName)
	if not slot then
		error("[user] Nothing a trade search can replace is worn in " .. tostring(slotName), 0)
	end
	local blank = new("Item", "Rarity: RARE\nStat Tester\n" .. item.baseName)
	local calcFunc, baseOutput = build.calcsTab:GetMiscCalculator()
	local blankOutput = calcFunc({ repSlotName = slotName, repItem = blank }, true)
	local stats = { }
	for index, entry in ipairs(statCandidates(category)) do
		if (index - 1) % parts == part then
			local line, value = midRollLine(entry, category)
			if line then
				blank.explicitModLines = { { line = line, custom = true } }
				blank:BuildAndParseRaw()
				local unread = (blank.modList and #blank.modList == 0)
					or (blank.slotModList and #blank.slotModList[1] == 0 and #blank.slotModList[2] == 0)
				if not unread then
					stats[#stats + 1] = {
						id = entry.tradeMod.id,
						line = line,
						value = value,
						out = summarise(calcFunc({ repSlotName = slotName, repItem = blank }, true)),
					}
				end
			end
		end
	end
	return { current = summarise(baseOutput), blank = summarise(blankOutput), stats = stats }
end

---------------------------------------------------------------------------
-- Skills and roll data for the diagnosis
---------------------------------------------------------------------------

-- Every active skill in the build's enabled skill groups, by name: whether
-- it is in the main skill group, counted in Full DPS, or socketed by the
-- player (not only granted by an item or the tree).
function bd_skill_usage()
	ensureOriginal()
	local out = { }
	for index, group in ipairs(build.skillsTab.socketGroupList) do
		if group.enabled ~= false then
			for _, gem in ipairs(group.gemList or { }) do
				local effect = gem.grantedEffect or (gem.gemData and gem.gemData.grantedEffect)
				if effect and effect.name and not effect.support then
					local entry = out[effect.name] or { main = false, fullDps = false, socketed = false }
					entry.main = entry.main or index == build.mainSocketGroup
					entry.fullDps = entry.fullDps or group.includeInFullDPS == true
					entry.socketed = entry.socketed or not (group.source or gem.fromItem or gem.fromTree)
					out[effect.name] = entry
				end
			end
		end
	end
	return out
end

-- The highest and lowest explicit roll PoB's trade data has for a stat text
-- in any item category, with the category of the highest.
function bd_roll_range(text)
	queryMods = queryMods or LoadModule("Data/QueryMods.lua")
	local low, high, where
	for _, entry in pairs(queryMods.Explicit or { }) do
		if entry.tradeMod and entry.tradeMod.text == text then
			for category, range in pairs(entry) do
				if type(range) == "table" and range.min and range.max then
					if not high or range.max > high then
						high, where = range.max, category
					end
					low = math.min(low or range.min, range.min)
				end
			end
		end
	end
	return low, high, where
end

-- "+1 to Level of all … Skills" lines for every such mod in PoB's trade
-- data, sorted.
function bd_level_lines()
	queryMods = queryMods or LoadModule("Data/QueryMods.lua")
	local seen, out = { }, { }
	for _, group in pairs(queryMods) do
		for _, entry in pairs(group) do
			local text = type(entry) == "table" and entry.tradeMod and entry.tradeMod.text
			if text and text:find("^# to Level of all ") and (text:find("Skills$") or text:find("Skill Gems$")) then
				local line = text:gsub("^#", "+1")
				if not seen[line] then
					seen[line] = true
					out[#out + 1] = line
				end
			end
		end
	end
	table.sort(out)
	return out
end

function bd_take_log()
	local out = log
	log = { }
	return out
end
