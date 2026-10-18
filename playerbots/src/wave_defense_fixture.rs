#![cfg(feature = "debug_reducers")]

use super::pkg_playerbots_bot;
use super::wave_defense::*;
use spacetimedb::ReducerContext;

#[spacetimedb::reducer]
pub fn playerbots_fixture_wave_defense_stage(
    ctx: &ReducerContext,
    grouped: bool,
) -> Result<(), String> {
    crate::package::require_operator(ctx)?;
    crate::package::fixture::require_no_imported_content(ctx)?;
    let mut guids = vec![];
    for role in [super::ROLE_TANK, super::ROLE_HEALER, super::ROLE_DPS] {
        guids.push(spawn_wave_defense_bot(
            ctx,
            role,
            5,
            0,
            0,
            (1200.0, 1200.0, 50.0),
        )?);
    }
    if grouped {
        crate::package::party::form(ctx, super::PACKAGE, &guids)?;
    }
    for guid in guids {
        let mut bot = ctx
            .db
            .pkg_playerbots_bot()
            .by_character()
            .filter(guid)
            .next()
            .ok_or("bot missing")?;
        bot.next_think_micros = i64::MAX;
        ctx.db.pkg_playerbots_bot().id().update(bot);
    }
    Ok(())
}

#[spacetimedb::reducer]
pub fn playerbots_fixture_wave_defense_targets(
    ctx: &ReducerContext,
    guid: u64,
    targets: Vec<u64>,
) -> Result<(), String> {
    crate::package::require_operator(ctx)?;
    crate::package::fixture::require_no_imported_content(ctx)?;
    set_wave_defense_objective(ctx, guid, (1200.0, 1200.0, 50.0), &targets)
}

#[spacetimedb::reducer]
pub fn playerbots_fixture_wave_defense_level(
    ctx: &ReducerContext,
    guid: u64,
    level: u8,
) -> Result<(), String> {
    crate::package::require_operator(ctx)?;
    crate::package::fixture::require_no_imported_content(ctx)?;
    update_wave_defense_bot_level(ctx, guid, level)
}

#[spacetimedb::reducer]
pub fn playerbots_fixture_wave_defense_remove(
    ctx: &ReducerContext,
    guid: u64,
) -> Result<(), String> {
    crate::package::require_operator(ctx)?;
    crate::package::fixture::require_no_imported_content(ctx)?;
    remove_wave_defense_bot(ctx, guid)
}
