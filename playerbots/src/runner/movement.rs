//! Continue the selected movement between decision turns. Never select another Candidate here.

use super::*;

pub(super) const INTERVAL: i64 = 500_000;
const BATCH_LIMIT: usize = 128;
const RENEW_BEFORE_MICROS: u64 = 100_000;

pub(super) fn stand_off(candidate: Candidate) -> f32 {
    match candidate.id.action {
        Action::Move(MoveTarget::RecoveryPosition(_) | MoveTarget::AreaTrigger(_)) => 0.25,
        Action::Move(MoveTarget::Home) => 2.0,
        _ if candidate.id.reason == Reason::FightPosition => 25.0,
        _ => 3.0,
    }
}

pub(super) fn pass(ctx: &ReducerContext, now: i64) {
    let rows = ctx.db.pkg_playerbots_runner();
    let due: Vec<_> = rows
        .by_movement_due()
        .filter(..=now)
        .take(BATCH_LIMIT)
        .collect();
    for mut state in due {
        state.movement_due_micros = i64::MAX;
        advance(ctx, &mut state, now);
        // Saving a decision would move its eligibility and observation clocks on every movement tick.
        rows.character_guid().update(state);
    }
}

fn advance(ctx: &ReducerContext, state: &mut PlayerbotsRunner, now: i64) {
    let guid = state.character_guid;
    let Some(foreground) = state.foreground.clone() else {
        return;
    };
    let Running::Movement(movement) = &foreground.running else {
        return;
    };
    let Some(me) = ctx.db.game_world_entity().guid().find(guid) else {
        stop(ctx, guid, state);
        return;
    };
    let controller = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(guid)
        .next()
        .map(|bot| bot.controller);
    if controller == Some(Controller::Legacy) {
        return;
    }
    let order = super::super::orders::active(ctx, guid);
    if controller != Some(Controller::Cohort)
        || foreground.generation != state.generation
        || state.chosen != Some(foreground.candidate)
        || (foreground.map_id, foreground.instance_id) != (me.map_id, me.instance_id)
        || (
            movement.destination.map_id,
            movement.destination.instance_id,
        ) != (me.map_id, me.instance_id)
        || state.transfer_checkpoint.is_some()
        || crate::actor::sessionless_movement_gate(ctx, guid).is_err()
        || state.companion_order_revision != order.as_ref().map_or(0, |order| order.revision)
        || movement
            .destination
            .geometry_revision
            .is_some_and(|generation| geometry_revision(ctx, me.map_id) != Some(generation))
    {
        stop_movement(ctx, guid);
        state.foreground = None;
        state.last_outcome = RunnerOutcome::Cancelled;
        return;
    }
    let Some(destination) = destination(ctx, &me, state, &foreground, movement, order.as_ref())
    else {
        stop(ctx, guid, state);
        state.last_outcome = RunnerOutcome::Cancelled;
        return;
    };
    let Some(observation) = actions::observation(ctx, guid, actions::ActionKind::Move) else {
        return;
    };
    let actions::ActionOutcome::Movement(previous) = observation.outcome else {
        return;
    };
    if let Some(spline) = ctx.db.game_creature_spline().guid().find(guid) {
        // A teleport, fear leg, or another owner replaces this identity. Do not overwrite it.
        if spline.start_micros != observation.observed_micros as u64 {
            state.foreground = None;
            state.last_outcome = RunnerOutcome::Cancelled;
            return;
        }
        if spline
            .start_micros
            .saturating_add(u64::from(spline.dur_ms) * 1000)
            > (now.max(0) as u64).saturating_add(RENEW_BEFORE_MICROS)
        {
            state.movement_due_micros = now.saturating_add(INTERVAL);
            return;
        }
    } else if (me.x - previous.route.endpoint.x).hypot(me.y - previous.route.endpoint.y) > 0.25 {
        state.foreground = None;
        state.last_outcome = RunnerOutcome::Cancelled;
        return;
    }
    let stand_off = stand_off(foreground.candidate);
    if (me.x - destination.x).hypot(me.y - destination.y) <= stand_off + 0.05 {
        // The next decision observes arrival and advances the objective's own progress clock.
        return;
    }
    super::super::goals::walk_toward(
        ctx,
        &me,
        (destination.x, destination.y, destination.z),
        stand_off,
        true,
    );
    if let Some(Foreground {
        running: Running::Movement(movement),
        ..
    }) = &mut state.foreground
    {
        movement.destination = destination;
    }
    state.movement_due_micros = now.saturating_add(INTERVAL);
}

fn destination(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    state: &PlayerbotsRunner,
    foreground: &Foreground,
    movement: &MovementRun,
    order: Option<&super::super::orders::CompanionOrderState>,
) -> Option<Destination> {
    let mut destination = movement.destination.clone();
    match foreground.candidate.id.action {
        Action::Move(MoveTarget::Home)
            if state
                .objective
                .as_ref()
                .is_some_and(|objective| objective.kind == ObjectiveKind::Companion) =>
        {
            let party = super::super::companion::human_led_party(ctx, me.guid).ok()??;
            if Some(party.leader_guid) != state.companion_leader_guid
                || order.is_some_and(|order| {
                    order.group_id != party.group_id || order.issuer_guid != party.leader_guid
                })
            {
                return None;
            }
            destination = companion_destination(me, &party, order)?;
        }
        Action::Move(MoveTarget::Entity(guid) | MoveTarget::CastingPosition(guid)) => {
            let target = ctx.db.game_world_entity().guid().find(guid)?;
            let hostile = matches!(
                foreground.candidate.id.reason,
                Reason::Defense | Reason::Grind | Reason::MeleePosition | Reason::FightPosition
            ) || state.companion_fight_target_guid == Some(guid)
                || (foreground.candidate.id.reason == Reason::Quest
                    && matches!(
                        foreground.candidate.id.action,
                        Action::Move(MoveTarget::CastingPosition(_))
                    ));
            if hostile && defense_target(ctx, me, guid).ok().flatten().is_none() {
                return None;
            }
            if target.dead
                && !matches!(
                    foreground.candidate.id.reason,
                    Reason::Quest | Reason::Resurrection
                )
            {
                return None;
            }
            if foreground.candidate.id.reason == Reason::Quest
                && crate::combat::validate_attack_target(ctx, me, guid).is_ok()
                && crate::spell::control_status(ctx, guid, 64).ok()?.is_some()
            {
                return None;
            }
            destination.map_id = target.map_id;
            destination.instance_id = target.instance_id;
            destination.x = target.x;
            destination.y = target.y;
            destination.z = target.z;
        }
        Action::Move(MoveTarget::GameObject(guid)) => {
            let target = ctx.db.game_gameobject().guid().find(guid)?;
            destination.map_id = target.map_id;
            destination.instance_id = target.instance_id;
            destination.x = target.x;
            destination.y = target.y;
            destination.z = target.z;
        }
        Action::Move(_) => {}
        _ => return None,
    }
    ((destination.map_id, destination.instance_id) == (me.map_id, me.instance_id))
        .then_some(destination)
}
