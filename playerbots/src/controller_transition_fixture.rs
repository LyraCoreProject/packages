#![cfg(feature = "debug_reducers")]

use super::{pkg_playerbots_bot, pkg_playerbots_runner, Controller};
use crate::transfer::game_bot_transfer_intent;
use spacetimedb::{reducer, ReducerContext};

/// Model a populated Legacy roster row on the current schema without using the retired selector.
#[reducer]
pub fn playerbots_controller_transition_fixture_stage_legacy(
    ctx: &ReducerContext,
    character_guid: u64,
    stale_in_transit: bool,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let bot = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(character_guid)
        .next()
        .ok_or("controller transition fixture bot missing")?;
    if bot.controller != Controller::Cohort {
        return Err("controller transition fixture requires Cohort source control".to_string());
    }
    if ctx
        .db
        .pkg_playerbots_runner()
        .character_guid()
        .find(character_guid)
        .is_some_and(|state| state.transfer_checkpoint.is_some())
        || ctx
            .db
            .game_bot_transfer_intent()
            .by_bot()
            .filter(character_guid)
            .next()
            .is_some()
        || crate::helpers::character_by_guid(ctx, character_guid).is_none()
    {
        return Err("controller transition fixture refuses Transfer-owned state".to_string());
    }
    super::runner::transition_controller(ctx, character_guid, Controller::Legacy)?;
    if stale_in_transit {
        // The preceding event transfer removed the live body before leaving this compatibility
        // marker. Use the Core logout path to model that persisted boundary.
        let body = crate::helpers::live_entity(ctx, character_guid)
            .map_err(|_| "controller transition fixture requires a live source body")?;
        crate::world::remove_live_character(ctx, body);
        super::goals::record_legacy_transfer(
            ctx,
            character_guid,
            ctx.timestamp.to_micros_since_unix_epoch(),
        );
    }
    Ok(())
}
