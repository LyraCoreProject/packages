#![cfg(feature = "debug_reducers")]

use spacetimedb::{reducer, ReducerContext};

use crate::tables::{game_character, game_world_entity};

use super::state::*;

#[reducer]
pub fn wave_defense_fixture_disconnect(ctx: &ReducerContext, guid: u64) -> Result<(), String> {
    crate::package::require_operator(ctx)?;
    ctx.db
        .pkg_wave_defense_member()
        .character_guid()
        .find(guid)
        .filter(|member| !member.bot)
        .ok_or("Fixture needs a human match member")?;
    crate::package::fixture::remove_live_character(ctx, guid)
}

#[reducer]
pub fn wave_defense_fixture_relogin(ctx: &ReducerContext, guid: u64) -> Result<(), String> {
    crate::package::require_operator(ctx)?;
    ctx.db
        .pkg_wave_defense_member()
        .character_guid()
        .find(guid)
        .filter(|member| !member.bot)
        .ok_or("Fixture needs a human match member")?;
    let mut character =
        crate::actor::character_by_guid(ctx, guid).ok_or("Fixture Character is missing")?;
    if ctx.db.game_world_entity().guid().find(guid).is_some() || character.online {
        return Err("Fixture Character is already online".into());
    }
    character.online = true;
    let character = ctx.db.game_character().guid().update(character);
    crate::package::character::build_player_entity(ctx, &character, character.owner_identity)
        .insert(ctx);
    super::reconnect::login(ctx, guid);
    Ok(())
}

#[reducer]
pub fn wave_defense_fixture_reconnect_once(ctx: &ReducerContext) -> Result<(), String> {
    crate::package::require_operator(ctx)?;
    let config = ctx
        .db
        .pkg_wave_defense_config()
        .id()
        .find(0)
        .filter(|config| config.enabled)
        .ok_or("Fixture mode is disabled")?;
    super::reconnect::tick(ctx, &config)
}
