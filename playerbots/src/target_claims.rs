//! Solo Target Claims index retained fight work; Recovery owns their lifetime.

use super::decision::Reason;
use super::pkg_playerbots_bot;
use super::recovery::Work;
use super::runner::{pkg_playerbots_runner, Controller, PlayerbotsRunner, RunnerOutcome};
use crate::{game_group_member, game_world_entity};
use spacetimedb::ReducerContext;

const OWNER_LIMIT: usize = 16;
const CLAIM_LIFETIME_MICROS: i64 = 30_000_000;

pub(super) enum Availability {
    Available,
    Claimed,
    ReadLimit,
}

fn grouped(ctx: &ReducerContext, guid: u64) -> bool {
    ctx.db
        .game_group_member()
        .by_character()
        .filter(guid)
        .next()
        .is_some()
}

/// Derive the indexed target from current authority and retained work. Stale index values never
/// grant a claim: selection calls this again before treating a matching row as an owner.
pub(super) fn selected(ctx: &ReducerContext, state: &PlayerbotsRunner) -> Option<u64> {
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
            && matches!(attempt.reason, Reason::Quest | Reason::Grind)
            && attempt.deferred_until_micros.is_none()
    })?;
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    let elapsed = now.saturating_sub(attempt.last_observed_micros).max(0);
    if attempt.stalled_micros.saturating_add(elapsed) >= CLAIM_LIFETIME_MICROS {
        return None;
    }
    let bot = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(state.character_guid)
        .next()?;
    if bot.controller != Controller::Cohort || grouped(ctx, state.character_guid) {
        return None;
    }
    let owner = ctx
        .db
        .game_world_entity()
        .guid()
        .find(state.character_guid)?;
    let creature = ctx.db.game_world_entity().guid().find(target)?;
    (!owner.dead
        && owner.health > 0
        && !creature.dead
        && creature.health > 0
        && (owner.map_id, owner.instance_id) == (creature.map_id, creature.instance_id)
        && (attempt.destination.map_id, attempt.destination.instance_id)
            == (owner.map_id, owner.instance_id)
        && attempt.geometry == crate::nav::inputs(ctx, owner.map_id))
    .then_some(target)
}

pub(super) fn availability(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    target: u64,
) -> Availability {
    if grouped(ctx, me.guid) {
        return Availability::Available;
    }
    let rows = ctx.db.pkg_playerbots_runner();
    // A retained approach does not yield to a later defensive engagement or stale competing row.
    if rows.character_guid().find(me.guid).is_some_and(|state| {
        state.solo_target_guid == target && selected(ctx, &state) == Some(target)
    }) {
        return Availability::Available;
    }
    for (index, state) in rows.by_solo_target().filter(target).enumerate() {
        if index == OWNER_LIMIT {
            return Availability::ReadLimit;
        }
        if state.character_guid != me.guid && selected(ctx, &state) == Some(target) {
            return Availability::Claimed;
        }
    }
    Availability::Available
}
