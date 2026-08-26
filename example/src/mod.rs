//! Reference Package proving build-time discovery and notify-hook registration.

crate::game_hook!(on_login, fn observe_login(ctx, payload) {
    if let Err(refusal) = crate::actor::system_message(
        ctx,
        payload.character_guid,
        "Example Package is active.".to_string(),
    ) {
        spacetimedb::log::error!(
            "Example Package on_login invariant failed for Character {}: {}",
            payload.character_guid,
            refusal,
        );
    }
});
