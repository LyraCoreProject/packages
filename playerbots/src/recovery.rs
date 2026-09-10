//! Finite attempts at useful work. Dispatch and route planning do not establish progress.

use super::actions::{self, ActionKind, ActionOutcome};
use super::decision::{Action, Candidate, CastAction, MoveTarget, QuestInteraction, Reason};
use super::runner::{Destination, ObjectiveKind, PlayerbotsRunner, Running};
use crate::{game_aura, game_gameobject, game_world_entity};
use spacetimedb::ReducerContext;

const CHANGE_APPROACH_MICROS: i64 = 10_000_000;
const ATTEMPT_LIMIT_MICROS: i64 = 30_000_000;
const DEFER_MICROS: i64 = 30_000_000;
const MEMORY_LIMIT: usize = 4;

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuestOperation {
    Accept,
    TurnIn,
    LootCreature,
    UseGameObject,
    LootGameObject,
}

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuestWork {
    pub step: QuestInteraction,
    pub operation: QuestOperation,
}

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Work {
    Destination(u64),
    Follow(u64),
    Fight(u64),
    Heal(u64),
    Buff(CastAction),
    Quest(QuestWork),
    AreaTrigger(u32),
}

impl Work {
    fn support(self) -> bool {
        matches!(self, Self::Heal(_))
    }

    fn target(self) -> Option<u64> {
        match self {
            Self::Destination(_) => None,
            Self::Follow(guid) | Self::Fight(guid) | Self::Heal(guid) => Some(guid),
            Self::Buff(cast) => Some(cast.target),
            Self::Quest(work) => Some(work.step.target),
            Self::AreaTrigger(_) => None,
        }
    }
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug)]
pub struct Attempt {
    pub work: Work,
    pub reason: Reason,
    pub destination: Destination,
    pub geometry: crate::nav::NavigationInputs,
    pub objective: u64,
    pub last_observed_micros: i64,
    pub stalled_micros: i64,
    pub target_health: Option<u32>,
    pub position: Option<Approach>,
    pub route: Option<crate::nav::RouteStep>,
    pub last_movement: Option<ObservedMovement>,
    pub deferred_until_micros: Option<i64>,
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug)]
pub struct ObservedMovement {
    pub started_micros: i64,
    pub position: crate::nav::RoutePoint,
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug)]
pub struct Approach {
    pub identity: u64,
    pub number: u8,
    pub destination: Destination,
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug, Default)]
pub struct Recovery {
    pub attempts: Vec<Attempt>,
    pub active: Option<Work>,
    pub position_sequence: u64,
}

pub(super) struct Deferral {
    pub destination: Destination,
    pub until_micros: i64,
    pub missing_coverage: bool,
}

pub(super) struct RestoredTransferAttempt {
    pub deferred_until_micros: Option<i64>,
}

/// The root action names the purpose. Positioning prerequisites retain that root through selection.
pub(super) fn work(purpose: Candidate) -> Option<Work> {
    if matches!(
        purpose.id.reason,
        Reason::Survival | Reason::Resurrection | Reason::Restricted | Reason::CrowdControl
    ) {
        return None;
    }
    match purpose.id.action {
        Action::Hold | Action::Resurrect => None,
        Action::Transfer(transfer) => Some(Work::AreaTrigger(transfer.trigger)),
        Action::Attack(target) => Some(Work::Fight(target)),
        Action::Cast(cast) => Some(match purpose.id.reason {
            Reason::Heal | Reason::Recovery => Work::Heal(cast.target),
            Reason::Buff => Work::Buff(cast),
            _ => Work::Fight(cast.target),
        }),
        Action::Move(MoveTarget::Home) => Some(Work::Destination(purpose.id.objective)),
        Action::Move(MoveTarget::Entity(target)) if purpose.id.reason == Reason::Follow => {
            Some(Work::Follow(target))
        }
        Action::Move(_) => None,
        Action::AcceptQuest(step) => Some(Work::Quest(QuestWork {
            step,
            operation: QuestOperation::Accept,
        })),
        Action::TurnInQuest(step) => Some(Work::Quest(QuestWork {
            step,
            operation: QuestOperation::TurnIn,
        })),
        Action::UseGameObject(step) => Some(Work::Quest(QuestWork {
            step,
            operation: QuestOperation::UseGameObject,
        })),
        Action::LootCreature(step) => Some(Work::Quest(QuestWork {
            step: QuestInteraction {
                target: step.target,
                quest: step.quest,
            },
            operation: QuestOperation::LootCreature,
        })),
        Action::LootGameObject(step) => Some(Work::Quest(QuestWork {
            step: QuestInteraction {
                target: step.target,
                quest: step.quest,
            },
            operation: QuestOperation::LootGameObject,
        })),
    }
}

fn destination(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    state: &PlayerbotsRunner,
    work: Work,
) -> Option<Destination> {
    let target = work.target();
    let point = target
        .and_then(|guid| ctx.db.game_world_entity().guid().find(guid))
        .filter(|unit| (unit.map_id, unit.instance_id) == (me.map_id, me.instance_id))
        .map(|unit| (unit.x, unit.y, unit.z))
        .or_else(|| {
            target
                .and_then(|guid| ctx.db.game_gameobject().guid().find(guid))
                .filter(|object| (object.map_id, object.instance_id) == (me.map_id, me.instance_id))
                .map(|object| (object.x, object.y, object.z))
        });
    if let Some((x, y, z)) = point {
        Some(Destination {
            map_id: me.map_id,
            instance_id: me.instance_id,
            x,
            y,
            z,
            geometry_revision: crate::nav::coverage_generation(ctx, me.map_id),
        })
    } else if let Work::AreaTrigger(trigger) = work {
        crate::actor::area_trigger_route(ctx, trigger).map(|route| Destination {
            map_id: route.source_map,
            instance_id: me.instance_id,
            x: route.source_x,
            y: route.source_y,
            z: route.source_z,
            geometry_revision: crate::nav::coverage_generation(ctx, route.source_map),
        })
    } else if matches!(work, Work::Destination(_)) {
        state
            .objective
            .as_ref()
            .map(|objective| objective.destination.clone())
    } else {
        None
    }
}

/// Only observed advancement along the retained Core leg counts. An emitted leg has no progress yet.
fn movement_progress(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    state: &PlayerbotsRunner,
    previous: Option<&ObservedMovement>,
) -> Option<(crate::nav::RouteStep, ObservedMovement, bool)> {
    let foreground = state.foreground.as_ref()?;
    if foreground.generation != state.generation
        || (foreground.map_id, foreground.instance_id) != (me.map_id, me.instance_id)
        || !matches!(foreground.running, Running::Movement(_))
    {
        return None;
    }
    let observation = actions::observation(ctx, me.guid, ActionKind::Move)?;
    if observation.observed_micros != foreground.started_micros {
        return None;
    }
    let ActionOutcome::Movement(movement) = observation.outcome else {
        return None;
    };
    if (movement.map_id, movement.instance_id) != (me.map_id, me.instance_id) {
        return None;
    }
    let route = movement.route;
    let previous_position = previous
        .filter(|previous| previous.started_micros == foreground.started_micros)
        .map_or(route.from, |previous| previous.position);
    let advanced = advanced_on_leg(&route, previous_position, (me.x, me.y).into());
    Some((
        route,
        ObservedMovement {
            started_micros: foreground.started_micros,
            position: (me.x, me.y).into(),
        },
        advanced,
    ))
}

fn advanced_on_leg(
    route: &crate::nav::RouteStep,
    previous: crate::nav::RoutePoint,
    current: crate::nav::RoutePoint,
) -> bool {
    let dx = route.endpoint.x - route.from.x;
    let dy = route.endpoint.y - route.from.y;
    let length = (dx * dx + dy * dy).sqrt();
    let moved_x = current.x - route.from.x;
    let moved_y = current.y - route.from.y;
    if length > 0.05 {
        let along = (moved_x * dx + moved_y * dy) / length;
        let before = ((previous.x - route.from.x) * dx + (previous.y - route.from.y) * dy) / length;
        let across = (moved_x * dy - moved_y * dx).abs() / length;
        along > before.max(0.0) + 0.05 && along <= length + 0.25 && across <= 0.25
    } else {
        false
    }
}

fn quest_completed(ctx: &ReducerContext, guid: u64, work: QuestWork, after: i64) -> bool {
    let kind = match work.operation {
        QuestOperation::Accept => ActionKind::AcceptQuest,
        QuestOperation::TurnIn => ActionKind::TurnInQuest,
        QuestOperation::UseGameObject => ActionKind::UseGameObject,
        QuestOperation::LootCreature | QuestOperation::LootGameObject => ActionKind::TakeLoot,
    };
    actions::observation(ctx, guid, kind).is_some_and(|outcome| {
        outcome.observed_micros >= after
            && outcome.target_guid == work.step.target
            && outcome.quest_entry == work.step.quest
            && matches!(outcome.outcome, ActionOutcome::Completed)
    })
}

pub(super) struct TransferAttemptRestore {
    pub objective: u64,
    pub reason: Reason,
    pub member_guid: Option<u64>,
    pub stalled_micros: i64,
    pub approach: u8,
    pub deferred_micros: i64,
}

impl Recovery {
    pub(super) fn restore_transfer_attempt(
        &mut self,
        restore: TransferAttemptRestore,
        now: i64,
    ) -> Option<RestoredTransferAttempt> {
        let attempt = self.attempts.iter_mut().find(|attempt| {
            attempt.objective == restore.objective
                && attempt.reason == restore.reason
                && restore
                    .member_guid
                    .is_none_or(|guid| attempt.work == Work::Follow(guid))
        })?;
        let approach_floor =
            CHANGE_APPROACH_MICROS.saturating_mul(i64::from(restore.approach.min(2)));
        attempt.stalled_micros = attempt
            .stalled_micros
            .max(restore.stalled_micros.max(approach_floor));
        attempt.last_observed_micros = now;
        attempt.position = None;
        attempt.route = None;
        attempt.last_movement = None;
        if restore.deferred_micros > 0 {
            attempt.deferred_until_micros = Some(now.saturating_add(restore.deferred_micros));
        }
        Some(RestoredTransferAttempt {
            deferred_until_micros: attempt.deferred_until_micros,
        })
    }

    pub(super) fn capacity_refused(&self, purpose: Candidate) -> bool {
        work(purpose).is_some_and(|work| {
            !self.attempts.iter().any(|attempt| attempt.work == work) && !self.room(work)
        })
    }

    pub(super) fn position(&self, identity: u64) -> Option<Destination> {
        self.attempts
            .iter()
            .filter_map(|attempt| attempt.position.as_ref())
            .find(|position| position.identity == identity)
            .map(|position| position.destination.clone())
    }

    fn room(&self, work: Work) -> bool {
        // Three ordinary attempts leave one slot for a heal. Unexpired failed work is never evicted.
        let retained: Vec<_> = self
            .attempts
            .iter()
            .filter(|attempt| attempt.stalled_micros > 0 || attempt.deferred_until_micros.is_some())
            .collect();
        retained.len() < MEMORY_LIMIT
            && (work.support()
                || retained.iter().filter(|a| !a.work.support()).count() < MEMORY_LIMIT - 1)
    }

    pub(super) fn eligible(&self, purpose: Candidate) -> bool {
        let Some(work) = work(purpose) else {
            return true;
        };
        self.eligible_work(work)
    }

    pub(super) fn eligible_work(&self, work: Work) -> bool {
        self.attempts
            .iter()
            .find(|attempt| attempt.work == work)
            .map_or_else(
                || self.room(work),
                |attempt| attempt.deferred_until_micros.is_none(),
            )
    }

    /// Observe the previous action before objective reconciliation or the next proposal can replace it.
    pub(super) fn observe(
        &mut self,
        ctx: &ReducerContext,
        me: &crate::WorldEntity,
        state: &mut PlayerbotsRunner,
        now: i64,
    ) -> Option<Deferral> {
        let geometry = crate::nav::inputs(ctx, me.map_id);
        self.attempts.retain(|attempt| {
            let retain = (attempt.destination.map_id, attempt.destination.instance_id)
                == (me.map_id, me.instance_id)
                && attempt.geometry == geometry
                && match attempt.deferred_until_micros {
                    Some(until) => until > now,
                    None => {
                        self.active == Some(attempt.work)
                            || now.saturating_sub(attempt.last_observed_micros) < DEFER_MICROS
                    }
                };
            if !retain {
                state.deferred_destinations.retain(|deferred| {
                    deferred.destination != attempt.destination
                        || Some(deferred.until_micros) != attempt.deferred_until_micros
                });
            }
            retain
        });
        let index = self
            .attempts
            .iter()
            .position(|attempt| Some(attempt.work) == self.active)?;
        if me.dead {
            self.active = None;
            return None;
        }
        let attempt = &mut self.attempts[index];
        if let Work::Buff(cast) = attempt.work {
            if ctx
                .db
                .game_aura()
                .by_target()
                .filter(cast.target)
                .take(64)
                .any(|aura| {
                    aura.spell_id == cast.spell
                        && aura.expires_at.to_micros_since_unix_epoch() > now
                })
            {
                self.attempts.remove(index);
                self.active = None;
                return None;
            }
        }
        if let Work::Quest(work) = attempt.work {
            if quest_completed(ctx, me.guid, work, attempt.last_observed_micros) {
                self.attempts.remove(index);
                self.active = None;
                return None;
            }
        }
        let health = attempt
            .work
            .target()
            .and_then(|guid| ctx.db.game_world_entity().guid().find(guid))
            .filter(|target| (target.map_id, target.instance_id) == (me.map_id, me.instance_id))
            .map(|target| target.health);
        let health_progress = match (attempt.work, attempt.target_health, health) {
            (Work::Fight(_), Some(before), Some(after)) => after < before,
            (Work::Heal(_), Some(before), Some(after)) => after > before,
            _ => false,
        };
        if health_progress || matches!((attempt.work, health), (Work::Fight(_), Some(0))) {
            if let Some(objective) = &mut state.objective {
                if objective.identity == attempt.objective
                    && matches!(
                        attempt.reason,
                        Reason::ReturnHome | Reason::Follow | Reason::Quest | Reason::Transfer
                    )
                {
                    objective.last_verified_progress_micros = Some(now);
                    objective.deadline_micros = if objective.kind == ObjectiveKind::Companion {
                        i64::MAX
                    } else {
                        now.saturating_add(120_000_000)
                    };
                }
            }
            self.attempts.remove(index);
            self.active = None;
            return None;
        }
        let advanced = movement_progress(ctx, me, state, attempt.last_movement.as_ref())
            .is_some_and(|(route, observation, advanced)| {
                attempt.route = Some(route);
                attempt.last_movement = Some(observation);
                advanced
                    && state.foreground.as_ref().is_some_and(|foreground| {
                        !matches!(
                            foreground.candidate.id.action,
                            Action::Move(MoveTarget::RecoveryPosition(_))
                        )
                    })
            });
        if advanced {
            attempt.stalled_micros = 0;
            attempt.position = None;
            if let Some(objective) = &mut state.objective {
                if objective.identity == attempt.objective
                    && matches!(
                        attempt.reason,
                        Reason::ReturnHome | Reason::Follow | Reason::Quest | Reason::Transfer
                    )
                {
                    objective.last_verified_progress_micros = Some(now);
                    objective.deadline_micros = if objective.kind == ObjectiveKind::Companion {
                        i64::MAX
                    } else {
                        now.saturating_add(120_000_000)
                    };
                }
            }
        } else {
            attempt.stalled_micros = attempt
                .stalled_micros
                .saturating_add(now.saturating_sub(attempt.last_observed_micros).max(0));
        }
        attempt.last_observed_micros = now;
        attempt.target_health = health;
        if attempt.stalled_micros >= ATTEMPT_LIMIT_MICROS {
            let until_micros = now.saturating_add(DEFER_MICROS);
            attempt.deferred_until_micros = Some(until_micros);
            attempt.position = None;
            self.active = None;
            return Some(Deferral {
                destination: attempt.destination.clone(),
                until_micros,
                missing_coverage: attempt.route.as_ref().is_some_and(|route| {
                    matches!(route.coverage, crate::nav::CoverageEvidence::Unknown)
                }),
            });
        }
        None
    }

    pub(super) fn select(
        &mut self,
        ctx: &ReducerContext,
        me: &crate::WorldEntity,
        state: &PlayerbotsRunner,
        candidate: Candidate,
        purpose: Candidate,
        now: i64,
    ) -> Candidate {
        let Some(work) = work(purpose) else {
            return candidate;
        };
        let index = if let Some(index) = self
            .attempts
            .iter()
            .position(|attempt| attempt.work == work)
        {
            index
        } else {
            if !self.room(work) {
                return candidate;
            }
            let Some(destination) = destination(ctx, me, state, work) else {
                return candidate;
            };
            self.attempts.retain(|attempt| {
                attempt.stalled_micros > 0 || attempt.deferred_until_micros.is_some()
            });
            self.attempts.push(Attempt {
                work,
                reason: purpose.id.reason,
                destination,
                geometry: crate::nav::inputs(ctx, me.map_id),
                objective: purpose.id.objective,
                last_observed_micros: now,
                stalled_micros: 0,
                target_health: work
                    .target()
                    .and_then(|guid| ctx.db.game_world_entity().guid().find(guid))
                    .map(|unit| unit.health),
                position: None,
                route: None,
                last_movement: None,
                deferred_until_micros: None,
            });
            self.attempts.len() - 1
        };
        let attempt = &mut self.attempts[index];
        if let Some(current) = destination(ctx, me, state, work) {
            attempt.destination = current;
        }
        if attempt.stalled_micros < CHANGE_APPROACH_MICROS {
            return candidate;
        }
        let number = if attempt.stalled_micros < CHANGE_APPROACH_MICROS * 2 {
            1
        } else {
            2
        };
        if attempt
            .position
            .as_ref()
            .is_none_or(|position| position.number != number)
        {
            let dx = attempt.destination.x - me.x;
            let dy = attempt.destination.y - me.y;
            let length = (dx * dx + dy * dy).sqrt();
            let (dx, dy) = if length > 0.05 {
                (dx / length, dy / length)
            } else {
                (1.0, 0.0)
            };
            let side = if number == 1 { 1.0 } else { -1.0 };
            let Some(identity) = self.position_sequence.checked_add(1) else {
                return candidate;
            };
            self.position_sequence = identity;
            attempt.position = Some(Approach {
                identity,
                number,
                destination: Destination {
                    map_id: me.map_id,
                    instance_id: me.instance_id,
                    x: me.x - dy * 6.0 * side,
                    y: me.y + dx * 6.0 * side,
                    z: me.z,
                    geometry_revision: attempt.destination.geometry_revision,
                },
            });
        }
        let position = attempt
            .position
            .as_ref()
            .expect("the selected approach has a position");
        if (me.x - position.destination.x).powi(2) + (me.y - position.destination.y).powi(2) <= 0.25
        {
            return candidate;
        }
        let mut adjusted = candidate;
        adjusted.id.action = Action::Move(MoveTarget::RecoveryPosition(position.identity));
        adjusted
    }

    pub(super) fn activate(&mut self, purpose: Option<Candidate>, now: i64) {
        self.active = purpose.and_then(work).filter(|work| {
            self.attempts
                .iter()
                .any(|attempt| attempt.work == *work && attempt.deferred_until_micros.is_none())
        });
        if let Some(attempt) = self
            .attempts
            .iter_mut()
            .find(|attempt| Some(attempt.work) == self.active)
        {
            if let Some(purpose) = purpose {
                attempt.objective = purpose.id.objective;
                attempt.reason = purpose.id.reason;
            }
            attempt.last_observed_micros = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::decision::CandidateId;
    use super::*;

    fn retained(work: Work) -> Attempt {
        Attempt {
            work,
            reason: Reason::Defense,
            destination: Destination {
                map_id: 0,
                instance_id: 0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
                geometry_revision: None,
            },
            geometry: crate::nav::NavigationInputs {
                imported_revision: None,
                navigation_enabled: true,
                collision_enabled: false,
                coverage_enabled: false,
                static_generation: None,
                coverage_generation: None,
            },
            objective: 1,
            last_observed_micros: 0,
            stalled_micros: 1,
            target_health: None,
            position: None,
            route: None,
            last_movement: None,
            deferred_until_micros: None,
        }
    }

    fn candidate(action: Action, reason: Reason) -> Candidate {
        Candidate {
            id: CandidateId {
                action,
                reason,
                objective: 1,
            },
            priority: 1,
        }
    }

    #[test]
    fn three_failed_ordinary_attempts_reserve_the_last_slot_for_a_heal() {
        let mut recovery = Recovery {
            attempts: vec![
                retained(Work::Fight(1)),
                retained(Work::Fight(2)),
                retained(Work::Fight(3)),
            ],
            ..Recovery::default()
        };
        let ordinary = candidate(Action::Attack(4), Reason::Defense);
        let heal = candidate(
            Action::Cast(CastAction {
                target: 10,
                spell: 2050,
            }),
            Reason::Heal,
        );

        assert!(!recovery.eligible(ordinary));
        assert!(recovery.eligible(heal));

        recovery.attempts.push(retained(Work::Heal(10)));
        let second_heal = candidate(
            Action::Cast(CastAction {
                target: 11,
                spell: 2050,
            }),
            Reason::Heal,
        );
        assert!(!recovery.eligible(second_heal));
    }

    #[test]
    fn progress_along_a_partial_leg_counts_once_even_when_it_moves_away_from_the_goal() {
        let route = crate::nav::RouteStep {
            from: (10.0, 0.0).into(),
            endpoint: (5.0, 0.0).into(),
            first_waypoint: Some((0.0, 0.0).into()),
            status: crate::nav::RouteStatus::Partial,
            expansions: 4096,
            clipping: None,
            coverage: crate::nav::CoverageEvidence::Unknown,
        };
        assert!(advanced_on_leg(
            &route,
            (10.0, 0.0).into(),
            (7.0, 0.0).into()
        ));
        assert!(!advanced_on_leg(
            &route,
            (7.0, 0.0).into(),
            (7.0, 0.0).into()
        ));
        assert!(!advanced_on_leg(
            &route,
            (7.0, 0.0).into(),
            (8.0, 0.0).into()
        ));
        assert!(!advanced_on_leg(
            &route,
            (7.0, 0.0).into(),
            (6.0, 3.0).into()
        ));
    }
}
