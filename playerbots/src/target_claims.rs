//! Solo Target Claims are reads of retained fight work, so cancellation needs no second write.

use super::decision::Reason;
use super::pkg_playerbots_bot;
use super::recovery::Work;
use super::runner::{pkg_playerbots_runner, Controller, PlayerbotsRunner, RunnerOutcome};
use crate::{game_group_member, game_world_entity};
use spacetimedb::ReducerContext;
use std::collections::BTreeSet;

const ENTITY_LIMIT: usize = 256;
const BOT_LIMIT: usize = 32;
const CLAIM_LIFETIME_MICROS: i64 = 30_000_000;

fn grouped(ctx: &ReducerContext, guid: u64) -> bool {
    ctx.db
        .game_group_member()
        .by_character()
        .filter(guid)
        .next()
        .is_some()
}

fn retained_target(state: &PlayerbotsRunner, now: i64) -> Option<u64> {
    if state.transfer_checkpoint.is_some()
        || !matches!(
            state.last_outcome,
            RunnerOutcome::Accepted
                | RunnerOutcome::Waiting
                | RunnerOutcome::Arrived
                | RunnerOutcome::CastFinished(_)
        )
    {
        return None;
    }
    let recovery = state.recovery.as_ref()?;
    let Work::Fight(target) = recovery.active? else {
        return None;
    };
    let attempt = recovery.attempts.iter().find(|attempt| {
        attempt.work == Work::Fight(target)
            && attempt.objective == state.objective_sequence
            && matches!(
                attempt.reason,
                Reason::Quest | Reason::Grind | Reason::Defense
            )
            && attempt.deferred_until_micros.is_none()
    })?;
    let elapsed = now.saturating_sub(attempt.last_observed_micros).max(0);
    (attempt.stalled_micros.saturating_add(elapsed) < CLAIM_LIFETIME_MICROS).then_some(target)
}

/// Read competing solo fights in this partition. Two bots selecting within `search_radius` of
/// themselves can approach the same creature from twice that distance apart. An incomplete scan
/// refuses selection; it must not report unobserved claims as available targets.
pub(super) fn nearby(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    search_radius: f32,
) -> Result<BTreeSet<u64>, ()> {
    let mut targets = BTreeSet::new();
    if grouped(ctx, me.guid) {
        return Ok(targets);
    }
    let radius = search_radius * 2.0;
    let (gx0, gx1, gy0, gy1) = lyracore_shared::spatial::covering_cell_box(me.x, me.y, radius);
    let entities = ctx.db.game_world_entity();
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    let mut scanned = 0;
    let mut bots = 0;
    for gx in gx0..=gx1 {
        for gy in gy0..=gy1 {
            let cell = lyracore_shared::spatial::grid_cell_id(gx, gy);
            for owner in entities.by_cell().filter((me.map_id, me.instance_id, cell)) {
                if scanned == ENTITY_LIMIT {
                    return Err(());
                }
                scanned += 1;
                if owner.guid == me.guid
                    || owner.type_mask & 0x10 == 0
                    || owner.dead
                    || owner.health == 0
                    || (owner.x - me.x).powi(2) + (owner.y - me.y).powi(2) > radius * radius
                {
                    continue;
                }
                let Some(bot) = ctx
                    .db
                    .pkg_playerbots_bot()
                    .by_character()
                    .filter(owner.guid)
                    .next()
                else {
                    continue;
                };
                if bots == BOT_LIMIT {
                    return Err(());
                }
                bots += 1;
                if bot.controller != Controller::Cohort || grouped(ctx, owner.guid) {
                    continue;
                }
                let Some(state) = ctx
                    .db
                    .pkg_playerbots_runner()
                    .character_guid()
                    .find(owner.guid)
                else {
                    continue;
                };
                let Some(target) = retained_target(&state, now) else {
                    continue;
                };
                let Some(target) = entities.guid().find(target) else {
                    continue;
                };
                if !target.dead
                    && target.health > 0
                    && (target.map_id, target.instance_id) == (me.map_id, me.instance_id)
                    && (owner.x - target.x).powi(2)
                        + (owner.y - target.y).powi(2)
                        + (owner.z - target.z).powi(2)
                        <= search_radius * search_radius
                {
                    targets.insert(target.guid);
                }
            }
        }
    }
    Ok(targets)
}
