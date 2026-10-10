// @event on_login
// @id 100300

function script(): void {
  if (event.actor && event.actor.is_player) {
    send_chat(event.actor, "Welcome from example-script!");
  }
}
