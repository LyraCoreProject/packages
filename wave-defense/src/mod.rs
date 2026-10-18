mod catalog;
mod commands;
mod content;
mod content_fixture;
#[cfg(feature = "debug_reducers")]
mod eligibility_fixture;
mod fixture;
mod matching;
mod progression;
mod queue;
mod reconnect;
mod reconnect_fixture;
mod round;
mod state;

pub(crate) use state::*;

pub(crate) const PACKAGE: &str = "wave-defense";

crate::game_addon_command!(wave_defense, handle_addon);

pub(crate) fn handle_addon(
    ctx: &spacetimedb::ReducerContext,
    character_guid: u64,
    action: &str,
    payload: &str,
) -> Result<String, String> {
    commands::handle(ctx, character_guid, action, payload)
}

crate::game_hook!(on_login, fn wave_defense_login(ctx, payload) {
    round::login(ctx, payload.character_guid);
});

crate::game_hook!(on_logout, fn wave_defense_logout(ctx, payload) {
    round::logout(ctx, payload.character_guid);
});

crate::game_hook!(on_creature_death, fn wave_defense_creature_death(ctx, payload) {
    round::enemy_death(ctx, payload.creature_guid);
});
