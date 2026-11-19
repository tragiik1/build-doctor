dofile("HeadlessWrapper.lua")
newBuild()
build.skillsTab:PasteSocketGroup("Fireball 20/0  1\nControlled Destruction 20/0 1")
runCallback("OnFrame")
local spec=build.spec; local n=0
for i=1,300 do local pick; for id,node in pairs(spec.nodes) do if not node.alloc and node.pathDist==1 and not node.ascendancyName and node.type~="Socket" then pick=node break end end
 if not pick then break end; spec:AllocNode(pick); n=n+1; if n>=60 then break end end
build.buildFlag=true; runCallback("OnFrame")
build.buildFlag=true; runCallback("OnFrame")
print("level before", build.characterLevel, build.characterLevelAutoMode)
local xml = build:SaveDB("rt")
print((xml:match("<Build [^>]+>")))
local stats = {}
for stat,val in xml:gmatch('<PlayerStat stat="([^"]+)" value="([^"]+)"') do stats[#stats+1]={stat,val} end
loadBuildFromXML(xml, "rt2")
print("level after", build.characterLevel, build.characterLevelAutoMode)
local out = build.calcsTab.mainOutput
local mism, total = 0, 0
for _,sv in ipairs(stats) do total=total+1
  local mine = out[sv[1]]
  if mine ~= nil and tostring(mine) ~= sv[2] then mism=mism+1; print("MISMATCH", sv[1], sv[2], tostring(mine)) end
end
print("stats in xml", total, "mismatches", mism, "TotalDPS", out.TotalDPS)
