#![cfg(feature = "debug_reducers")]

//! A declared scheduler boundary for retained movement across an owned process restart.

use spacetimedb::{reducer, ReducerContext, Table, TimeDuration};

#[reducer]
pub fn playerbots_lifecycle_stage_movement(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    use super::pkg_playerbots_bot;
    if ctx.db.pkg_playerbots_bot().count() != 1 {
        return Err("movement restart fixture requires one staged bot".to_string());
    }
    crate::package_fixture::declare_next_movement_tick(ctx, TimeDuration::from_micros(30_000_000))?;
    super::fixture::playerbots_fixture_runner_stage(ctx, character_guid, false)?;
    super::fixture::playerbots_fixture_runner_select_cohort(ctx, character_guid)?;
    super::fixture::playerbots_fixture_runner_pass_once(ctx, character_guid)
}
