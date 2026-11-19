-- run from pob2/src after HeadlessWrapper
dofile("HeadlessWrapper.lua")
newBuild()
build.configTab.input.customMods = ""
build.characterLevel = 90
build.skillsTab:PasteSocketGroup("Fireball 20/0  1\nControlled Destruction 20/0 1\nFire Mastery 1/0 1")
runCallback("OnFrame")
-- allocate a connected blob of nodes by BFS from class start
local spec = build.spec
local alloc = 0
for iter=1,200 do
  local best
  for id,node in pairs(spec.nodes) do
    if not node.alloc and node.pathDist == 1 and node.type ~= "ClassStart" and node.type ~= "AscendClassStart" and not node.ascendancyName and node.type ~= "Socket" then best = node break end
  end
  if not best then break end
  spec:AllocNode(best)
  alloc = alloc + 1
  if alloc >= 100 then break end
end
spec:AddUndoState(); build.buildFlag = true
runCallback("OnFrame")
local out = build.calcsTab.mainOutput
print("allocated", alloc, "TotalDPS", out.TotalDPS, "Life", out.Life, "ES", out.EnergyShield)
local t0=os.clock(); for i=1,20 do build.calcsTab:BuildOutput() end
print(string.format("full BuildOutput: %.1f ms", (os.clock()-t0)/20*1000))
local calcFunc, calcBase = build.calcsTab:GetMiscCalculator()
local cands = {}
for id,node in pairs(spec.nodes) do if not node.alloc and node.pathDist==1 and not node.ascendancyName then cands[#cands+1]=node end end
local t1=os.clock(); local n=0
for _,node in ipairs(cands) do local o = calcFunc({addNodes={[node]=true}}); n=n+1 end
print(string.format("what-if node eval: %d nodes, %.2f ms each", n, (os.clock()-t1)/n*1000))
