-- @event on_levelup
-- @id 100301

if event.actor and event.actor.is_player then
    send_chat(event.actor, "Welcome to your new level from example-script!")
end
