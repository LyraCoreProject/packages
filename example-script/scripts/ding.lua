local function ding(event)
    send_chat(event.player, "Welcome to your new level from example-script!")
end

events.player.onLevelUp(ding)
