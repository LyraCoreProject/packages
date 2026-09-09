//! A declared scheduler boundary for retained movement across an owned process restart.

use crate::game_creature_move_schedule;
use spacetimedb::{reducer, ReducerContext, ScheduleAt, Table, TimeDuration};

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
    let schedules = ctx.db.game_creature_move_schedule(); // package-api: exempt private fixture declares the next ordinary Core tick before movement starts
    let mut rows: Vec<_> = schedules.iter().take(2).collect();
    if rows.len() != 1 || rows[0].instance_id != u64::MAX {
        return Err("movement restart fixture requires one fresh global tick".to_string());
    }
    let at = ctx
        .timestamp
        .checked_add(TimeDuration::from_micros(30_000_000))
        .ok_or("movement restart fixture timestamp exhausted")?;
    let mut schedule = rows.remove(0);
    schedule.scheduled_at = ScheduleAt::Time(at);
    schedules.scheduled_id().update(schedule);
    super::fixture::playerbots_fixture_runner_stage(ctx, character_guid, false)?;
    super::fixture::playerbots_fixture_runner_select_cohort(ctx, character_guid)?;
    super::fixture::playerbots_fixture_runner_pass_once(ctx, character_guid)
}
