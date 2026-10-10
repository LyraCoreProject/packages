function welcome(event: PlayerLoginEvent): void {
  send_chat(event.player, "Welcome from example-script!");
}

events.player.onLogin(welcome);
