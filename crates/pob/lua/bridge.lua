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

function bd_load(xml)
	loadBuildFromXML(xml, "build-doctor")
	local prompt = takePrompt()
	if prompt then
		error(prompt, 0)
	end
	return bd_settle()
end

-- Who the loaded build is, as PoB sees it. Read-only.
function bd_identity()
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
		mainSkill = effect and effect.name,
	}
end

function bd_take_log()
	local out = log
	log = { }
	return out
end
