//! Continue the selected movement between decision turns. Never select another Candidate here.

use super::*;

pub(super) const INTERVAL: i64 = 500_000;
const BATCH_LIMIT: usize = 1_024;
const PLAN_LIMIT: usize = 32;
const EXPANSION_LIMIT: u32 = 4 * crate::nav::LEG_MAX_EXPANSIONS;
const RENEWAL_LEAD_MICROS: u64 = 2_000_000;

struct PlanningBudget {
    searches: usize,
    expansions: u32,
}

impl PlanningBudget {
    fn available(&self) -> bool {
        self.searches < PLAN_LIMIT
            && self.expansions <= EXPANSION_LIMIT - crate::nav::LEG_MAX_EXPANSIONS
    }

    fn record(&mut self, expansions: u32) {
        self.searches += 1;
        self.expansions += expansions;
    }
}

fn begin(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    destination: (f32, f32, f32),
    stand_off: f32,
) -> u32 {
    #[cfg(feature = "debug_reducers")]
    let _route_time = super::super::profiling::movement(ctx, me.guid);
    let route = crate::nav::route_path(
        ctx,
        me.map_id,
        me.instance_id,
        (me.x, me.y, me.z),
        destination,
        stand_off,
    );
    let expansions = route.step.expansions;
    if route.points.is_empty() {
        stop_movement(ctx, me.guid);
    }
    actions::movement(
        ctx,
        me.guid,
        me.map_id,
        me.instance_id,
        (destination.0, destination.1).into(),
        (me.x - destination.0).hypot(me.y - destination.1) <= stand_off + 0.05,
        route.step,
    );
    if let Ok(mover) = crate::helpers::live_entity(ctx, me.guid) {
        crate::creatures::tick::emit_creature_path(ctx, mover, route.points, true);
    }
    expansions
}

pub(super) fn retained(
    ctx: &ReducerContext,
    state: &PlayerbotsRunner,
    candidate: Candidate,
    destination: &Destination,
    now: i64,
) -> bool {
    let Some(foreground) = &state.foreground else {
        return false;
    };
    let Running::Movement(movement) = &foreground.running else {
        return false;
    };
    foreground.generation == state.generation
        && foreground.candidate.id == candidate.id
        && (
            destination.map_id,
            destination.instance_id,
            destination.geometry_revision,
        ) == (
            movement.destination.map_id,
            movement.destination.instance_id,
            movement.destination.geometry_revision,
        )
        && (destination.x - movement.destination.x).hypot(destination.y - movement.destination.y)
            <= 2.0
        && (destination.z - movement.destination.z).abs() <= 2.0
        && (state.path_pending
            || ctx
                .db
                .game_creature_spline()
                .guid()
                .find(state.character_guid)
                .is_some_and(|spline| {
                    spline.path.is_some_and(|path| {
                        path.navigation == crate::nav::inputs(ctx, destination.map_id)
                    }) && spline
                        .start_micros
                        .saturating_add(u64::from(spline.dur_ms) * 1000)
                        > now.max(0) as u64
                        && actions::observation(
                            ctx,
                            state.character_guid,
                            actions::ActionKind::Move,
                        )
                        .is_some_and(|observation| {
                            spline.start_micros == observation.observed_micros as u64
                        })
                }))
}

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
        .by_path_due()
        .filter((false, ..=now))
        .take(BATCH_LIMIT)
        .collect();
    for mut state in due {
        advance(ctx, &mut state, now, None);
        // Movement must not advance the decision's observation or eligibility clocks.
        rows.character_guid().update(state);
    }

    let mut continuing = std::collections::VecDeque::new();
    let mut starting = std::collections::VecDeque::new();
    for state in rows.by_path_due().filter((true, ..=now)).take(BATCH_LIMIT) {
        if owned_observation(ctx, &state).is_some() {
            continuing.push_back(state);
        } else {
            starting.push_back(state);
        }
    }
    let mut budget = PlanningBudget {
        searches: 0,
        expansions: 0,
    };
    while budget.available() && (!continuing.is_empty() || !starting.is_empty()) {
        // Alternation gives old paths and new requests a turn within the same bounded budget.
        for queue in [&mut continuing, &mut starting] {
            if !budget.available() {
                break;
            }
            let Some(mut state) = queue.pop_front() else {
                continue;
            };
            advance(ctx, &mut state, now, Some(&mut budget));
            rows.character_guid().update(state);
        }
    }
}

fn owned_observation(
    ctx: &ReducerContext,
    state: &PlayerbotsRunner,
) -> Option<actions::PlayerbotsAction> {
    let foreground = state.foreground.as_ref()?;
    actions::observation(ctx, state.character_guid, actions::ActionKind::Move)
        .filter(|observation| observation.observed_micros >= foreground.started_micros)
}

fn advance(
    ctx: &ReducerContext,
    state: &mut PlayerbotsRunner,
    now: i64,
    budget: Option<&mut PlanningBudget>,
) {
    state.movement_due_micros = i64::MAX;
    state.path_pending = false;
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
    let order = super::super::orders::active(ctx, guid);
    if !matches!(controller, Some(Controller::Cohort | Controller::Legacy))
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
    let Some(destination) = destination(
        ctx,
        &me,
        state,
        &foreground,
        movement,
        order.as_ref(),
        controller == Some(Controller::Cohort),
    ) else {
        stop(ctx, guid, state);
        state.last_outcome = RunnerOutcome::Cancelled;
        return;
    };
    let spline = ctx.db.game_creature_spline().guid().find(guid);
    let observation = actions::observation(ctx, guid, actions::ActionKind::Move);
    let initial = observation
        .as_ref()
        .is_none_or(|observation| observation.observed_micros < foreground.started_micros);
    if spline.as_ref().is_some_and(|spline| {
        let active = spline.dur_ms > 0
            && spline
                .start_micros
                .saturating_add(u64::from(spline.dur_ms) * 1000)
                > now.max(0) as u64;
        (!initial || active)
            && observation
                .as_ref()
                .is_none_or(|observation| spline.start_micros != observation.observed_micros as u64)
    }) {
        // A queued first search must respect motion installed by another owner while it waited.
        state.foreground = None;
        state.last_outcome = RunnerOutcome::Cancelled;
        return;
    }
    let observation =
        observation.filter(|observation| observation.observed_micros >= foreground.started_micros);
    if let Some(observation) = &observation {
        let actions::ActionOutcome::Movement(previous) = &observation.outcome else {
            return;
        };
        if let Some(spline) = spline {
            if spline
                .path
                .as_ref()
                .is_some_and(|path| path.navigation != crate::nav::inputs(ctx, me.map_id))
            {
                stop(ctx, guid, state);
                state.last_outcome = RunnerOutcome::Cancelled;
                return;
            }
            let more_path = (spline.dx - destination.x).hypot(spline.dy - destination.y)
                > stand_off(foreground.candidate) + 0.05;
            let renew_at =
                (now.max(0) as u64).saturating_add(if more_path { RENEWAL_LEAD_MICROS } else { 0 });
            let destination_changed = (destination.x - movement.destination.x)
                .hypot(destination.y - movement.destination.y)
                > 2.0
                || (destination.z - movement.destination.z).abs() > 2.0;
            if !destination_changed
                && spline
                    .start_micros
                    .saturating_add(u64::from(spline.dur_ms) * 1000)
                    > renew_at
            {
                state.movement_due_micros = now.saturating_add(INTERVAL);
                return;
            }
        } else if (me.x - previous.route.endpoint.x).hypot(me.y - previous.route.endpoint.y) > 0.25
        {
            state.foreground = None;
            state.last_outcome = RunnerOutcome::Cancelled;
            return;
        }
    }
    let stand_off = stand_off(foreground.candidate);
    if (me.x - destination.x).hypot(me.y - destination.y) <= stand_off + 0.05 {
        // The next decision observes arrival and advances the objective's own progress clock.
        return;
    }
    let Some(budget) = budget else {
        state.path_pending = true;
        state.movement_due_micros = now;
        return;
    };
    let expansions = begin(
        ctx,
        &me,
        (destination.x, destination.y, destination.z),
        stand_off,
    );
    budget.record(expansions);
    state.route_expansions = expansions;
    #[cfg(feature = "debug_reducers")]
    if super::super::config_parsed(ctx, "decision_profile", false) {
        spacetimedb::log::info!(
            "playerbots_path guid={guid} observed_micros={now} expansions={expansions} initial={initial}"
        );
    }
    if initial {
        state.last_stall_check_micros = now;
        if let Some(foreground) = &mut state.foreground {
            foreground.started_micros = now;
        }
    }
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
    human_leader_required: bool,
) -> Option<Destination> {
    let mut destination = movement.destination.clone();
    match foreground.candidate.id.action {
        Action::Move(MoveTarget::Home)
            if state
                .objective
                .as_ref()
                .is_some_and(|objective| objective.kind == ObjectiveKind::Companion) =>
        {
            let party =
                super::super::companion::party(ctx, me.guid, human_leader_required).ok()??;
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
