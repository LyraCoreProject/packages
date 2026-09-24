//! Solo Target Claims index retained fight work; Recovery owns their lifetime.

use super::decision::Reason;
use super::pkg_playerbots_bot;
use super::recovery::Work;
use super::runner::{pkg_playerbots_runner, Controller, PlayerbotsRunner, RunnerOutcome};
use crate::{game_group_member, game_world_entity};
use spacetimedb::ReducerContext;

const OWNER_LIMIT: usize = 16;
const BACKFILL_LIMIT: usize = 16;
const UNINDEXED: u64 = u64::MAX;
const CLAIM_LIFETIME_MICROS: i64 = 30_000_000;

pub(super) enum Availability {
    Available,
    Claimed,
    ReadLimit,
}

/// Populate pre-publish rows before allowing new claims. The sentinel is indexed, so each pass
/// visits only unfinished rows and never resets their progress or action clocks.
pub(super) fn backfill(ctx: &ReducerContext) {
    let rows = ctx.db.pkg_playerbots_runner();
    let pending: Vec<_> = rows
        .by_solo_target()
        .filter(UNINDEXED)
        .take(BACKFILL_LIMIT)
        .collect();
    for mut state in pending {
        state.solo_target_guid = selected(ctx, &state).unwrap_or(0);
        rows.character_guid().update(state);
    }
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
            && (matches!(attempt.reason, Reason::Quest | Reason::Grind)
                || (attempt.reason == Reason::Defense && state.solo_target_guid == target))
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

/// Facts shared by all candidates in one target selection, within the current reducer transaction.
pub(super) struct TargetClaims {
    character_guid: u64,
    grouped: bool,
    own_target: Option<u64>,
    backfill_pending: bool,
}

impl TargetClaims {
    pub(super) fn read(ctx: &ReducerContext, character_guid: u64) -> Self {
        let grouped = grouped(ctx, character_guid);
        let rows = ctx.db.pkg_playerbots_runner();
        Self {
            character_guid,
            grouped,
            own_target: if grouped {
                None
            } else {
                rows.character_guid()
                    .find(character_guid)
                    .and_then(|state| selected(ctx, &state))
            },
            backfill_pending: !grouped && rows.by_solo_target().filter(UNINDEXED).next().is_some(),
        }
    }

    pub(super) fn availability(&self, ctx: &ReducerContext, target: u64) -> Availability {
        // A retained approach does not yield to a later defensive engagement or stale competing row.
        if self.grouped || self.own_target == Some(target) {
            return Availability::Available;
        }
        if self.backfill_pending {
            return Availability::ReadLimit;
        }
        for (index, state) in ctx
            .db
            .pkg_playerbots_runner()
            .by_solo_target()
            .filter(target)
            .enumerate()
        {
            if index == OWNER_LIMIT {
                return Availability::ReadLimit;
            }
            if state.character_guid != self.character_guid && selected(ctx, &state) == Some(target)
            {
                return Availability::Claimed;
            }
        }
        Availability::Available
    }
}
