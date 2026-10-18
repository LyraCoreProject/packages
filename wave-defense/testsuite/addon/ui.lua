local frames, sent, now = {}, {}, 0
local cinematic, stoppedCinematics = true, 0
local methods = {}
local function region(kind, name, parent)
    local result = { kind = kind, name = name, parent = parent, scripts = {}, shown = true, enabled = true, labels = {} }
    setmetatable(result, { __index = methods })
    table.insert(frames, result)
    if name then _G[name] = result end
    return result
end
function methods:SetWidth(width) self.width = width end
function methods:SetHeight(height) self.height = height end
function methods:GetWidth() return self.width end
function methods:SetPoint(anchor, relative, relativeAnchor, x, y) self.anchor = { anchor, relative, relativeAnchor, x, y } end
function methods:SetBackdrop(backdrop) self.backdrop = backdrop end
function methods:SetBackdropColor(red, green, blue, alpha) self.background = { red, green, blue, alpha } end
function methods:SetBackdropBorderColor(red, green, blue, alpha) self.border = { red, green, blue, alpha } end
function methods:SetFrameStrata(strata) self.strata = strata end
function methods:SetScale(scale) self.scale = scale end
function methods:Show() self.shown = true end
function methods:Hide() self.shown = false end
function methods:SetText(text) self.text = text end
function methods:SetJustifyH(align) self.alignH = align end
function methods:SetJustifyV(align) self.alignV = align end
function methods:Enable() self.enabled = true end
function methods:Disable() self.enabled = false end
function methods:SetScript(eventName, callback) self.scripts[eventName] = callback end
function methods:RegisterEvent(eventName) self[eventName] = true end
function methods:SetStatusBarTexture(texture) self.texture = texture end
function methods:SetStatusBarColor(red, green, blue) self.color = { red, green, blue } end
function methods:SetMinMaxValues(minimum, maximum) self.minimum, self.maximum = minimum, maximum end
function methods:SetValue(value) self.value = value end
function methods:CreateFontString(name, layer, font)
    local text = region("FontString", name, self)
    text.layer, text.font = layer, font
    table.insert(self.labels, text)
    return text
end
function CreateFrame(kind, name, parent, template)
    local frame = region(kind, name, parent)
    frame.template = template
    return frame
end
function UnitClass() return "Mage", "MAGE" end
function UnitName() return "Kara" end
function GetTime() return now end
function InCinematic() return cinematic end
function StopCinematic() cinematic, stoppedCinematics = false, stoppedCinematics + 1 end
function SendAddonMessage(prefix, message, lane)
    assert(prefix == "STC" and lane == "PARTY")
    assert(not string.find(string.gsub(message, "||", ""), "|", 1, true))
    table.insert(sent, message)
end
SlashCmdList = {}
UIParent = region("Frame", "UIParent")
UIParent.width = 360
dofile("client/addons/WaveDefense/Protocol.lua")
dofile("client/addons/WaveDefense/Client.lua")
dofile("client/addons/WaveDefense/WaveDefense.lua")
local events = frames[table.getn(frames)]
local function fire(name, prefix, message, lane, sender)
    event, arg1, arg2, arg3, arg4 = name, prefix, message, lane, sender
    events.scripts.OnEvent()
    event, arg1, arg2, arg3, arg4 = nil, nil, nil, nil, nil
end
local function update(time)
    now = time
    events.scripts.OnUpdate()
end
local function response(action, payload)
    fire("CHAT_MSG_ADDON", "STC", "v1|package.wave_defense." .. action .. ".result|0|1/1|" .. payload, "UNKNOWN", "Kara")
end
local function findButton(parent, text)
    for _, frame in ipairs(frames) do
        if frame.parent == parent and frame.kind == "Button" and frame.text == text then return frame end
    end
    error("Missing button " .. text)
end
local function click(parent, text)
    local control = findButton(parent, text)
    assert(control.enabled and control.shown)
    control.scripts.OnClick()
end
local function hasText(parent, text)
    for _, value in ipairs(parent.labels) do
        if value.text and string.find(value.text, text, 1, true) then return true end
    end
    return false
end
local function status(phase, pending, revision, request)
    return "v1|" .. phase .. "|" .. (phase == "lobby" and "0" or "123") .. "|5|20|350|400|8400|10000|12|24|12500|" .. pending .. "|3|0|8|42|" .. request .. "|" .. revision
end

fire("PLAYER_ENTERING_WORLD")
assert(WaveDefenseLobby.shown and not WaveDefenseDock.shown)
assert(not findButton(WaveDefenseLobby, "Join queue").enabled)
update(0)
assert(table.getn(sent) == 1)
fire("CHAT_MSG_ADDON", "OTHER", "unrelated", "UNKNOWN", "Kara")
assert(stoppedCinematics == 0)
response("status", status("lobby", "0", "0", "0"))
assert(stoppedCinematics == 1)
assert(hasText(WaveDefenseLobby, "Frost Mage"))
click(WaveDefenseLobby, "Select specialization")
assert(WaveDefenseRequest == "1")
response("select", status("lobby", "0", "0", "1"))
click(WaveDefenseLobby, "Join queue")
assert(WaveDefenseRequest == "2")
response("queue", status("queued", "0", "0", "2"))
assert(hasText(WaveDefenseLobby, "Queued 0:42"))
update(0.2)
response("roster", "v1|41|Brann Bot|tank|1|1|42|Kara|damage|0|0")
assert(hasText(WaveDefenseLobby, "Brann Bot   Bot"))
update(2)
response("status", status("preparation", "2", "9", "2"))
assert(WaveDefenseDock.shown and not WaveDefenseLobby.shown)
assert(hasText(WaveDefenseDock, "Wave 5 / 15 boss"))
assert(hasText(WaveDefenseDock, "1g 25s 0c"))
assert(hasText(WaveDefenseDock, "Next wave in 0:24"))
update(2.3)
response("offer", "v1|123|9|20|2|7|talent|Ice Shards|All five ranks|8|spell|Frost Nova|Ranks grow with level|9|spell|Blizzard|Area damage")
assert(WaveDefenseReward.shown and hasText(WaveDefenseReward, "Ice Shards"))
click(WaveDefenseReward, "Bank for later")
update(2.6)
assert(not WaveDefenseReward.shown)
SlashCmdList.WAVEDEFENSE()
assert(WaveDefenseReward.shown)
click(WaveDefenseReward, "Choose")
assert(WaveDefenseRequest == "3")
response("choose", status("preparation", "1", "10", "3"))
assert(not WaveDefenseReward.shown)
click(WaveDefenseDock, "Ready early")
response("ready", status("combat", "1", "10", "4"))
assert(not WaveDefenseReward.shown and hasText(WaveDefenseDock, "Defend the base"))
local count = table.getn(sent)
click(WaveDefenseControls, "Leave")
assert(table.getn(sent) == count)
update(2.9)
click(WaveDefenseControls, "Confirm leave")
assert(table.getn(sent) == count + 1 and WaveDefenseRequest == "5")
response("leave", status("lobby", "0", "0", "5"))
assert(WaveDefenseLobby.shown and not WaveDefenseDock.shown and not WaveDefenseReward.shown)
for _, panel in ipairs({ WaveDefenseLobby, WaveDefenseDock, WaveDefenseReward, WaveDefenseControls }) do
    assert(panel.width * panel.scale <= 336.01)
end
fire("PLAYER_LEAVING_WORLD")
assert(hasText(WaveDefenseControls, "Connection closed"))
assert(not findButton(WaveDefenseLobby, "Join queue").enabled)
print("PASS addon UI flow at 360px with explicit WoW API shim")
