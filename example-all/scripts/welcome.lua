-- A positive Script Answer sends the welcome. Zero suppresses it.
local function welcome(event)
    return 1
end

events.package.on("welcome", welcome)
