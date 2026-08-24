//! Reference Package proving build-time discovery and notify-hook registration.

crate::game_hook!(on_login, fn observe_login(_ctx, payload) {
    let _character_guid = payload.character_guid;
});
