//! Finite attempts at useful work. Dispatch and route planning do not establish progress.

use super::actions::{self, ActionKind, ActionOutcome};
use super::decision::{Action, Candidate, CastAction, MoveTarget, QuestInteraction, Reason};
use super::runner::{Destination, PlayerbotsRunner, Running};
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
        }
    }
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug)]
pub struct Attempt {
    pub work: Work,
    pub destination: Destination,
    pub objective: u64,
    pub last_observed_micros: i64,
    pub stalled_micros: i64,
    pub target_health: Option<u32>,
    pub position: Option<Approach>,
    pub route: Option<crate::nav::RouteStep>,
    pub deferred_until_micros: Option<i64>,
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
}

/// The root action names the purpose. Positioning prerequisites retain that root through selection.
fn work(purpose: Candidate) -> Option<Work> {
    if matches!(
        purpose.id.reason,
        Reason::Survival | Reason::Resurrection | Reason::Restricted | Reason::CrowdControl
    ) {
        return None;
    }
    match purpose.id.action {
        Action::Hold | Action::Resurrect => None,
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
) -> Option<(crate::nav::RouteStep, bool)> {
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
    let dx = route.endpoint.x - route.from.x;
    let dy = route.endpoint.y - route.from.y;
    let length = (dx * dx + dy * dy).sqrt();
    let moved_x = me.x - route.from.x;
    let moved_y = me.y - route.from.y;
    let advanced = if length > 0.05 {
        let along = (moved_x * dx + moved_y * dy) / length;
        let across = (moved_x * dy - moved_y * dx).abs() / length;
        along > 0.05 && along <= length + 0.25 && across <= 0.25
    } else {
        false
    };
    Some((route, advanced))
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

impl Recovery {
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
        let geometry = crate::nav::coverage_generation(ctx, me.map_id);
        self.attempts.retain(|attempt| {
            (attempt.destination.map_id, attempt.destination.instance_id)
                == (me.map_id, me.instance_id)
                && attempt.destination.geometry_revision == geometry
                && match attempt.deferred_until_micros {
                    Some(until) => until > now,
                    None => {
                        self.active == Some(attempt.work)
                            || now.saturating_sub(attempt.last_observed_micros) < DEFER_MICROS
                    }
                }
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
                if objective.identity == attempt.objective {
                    objective.last_verified_progress_micros = Some(now);
                    objective.deadline_micros = now.saturating_add(120_000_000);
                }
            }
            self.attempts.remove(index);
            self.active = None;
            return None;
        }
        let advanced = movement_progress(ctx, me, state).is_some_and(|(route, advanced)| {
            attempt.route = Some(route);
            advanced && attempt.position.is_none()
        });
        if advanced {
            attempt.stalled_micros = 0;
            attempt.position = None;
            if let Some(objective) = &mut state.objective {
                if objective.identity == attempt.objective && !matches!(attempt.work, Work::Buff(_))
                {
                    objective.last_verified_progress_micros = Some(now);
                    objective.deadline_micros = now.saturating_add(120_000_000);
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
                destination,
                objective: purpose.id.objective,
                last_observed_micros: now,
                stalled_micros: 0,
                target_health: work
                    .target()
                    .and_then(|guid| ctx.db.game_world_entity().guid().find(guid))
                    .map(|unit| unit.health),
                position: None,
                route: None,
                deferred_until_micros: None,
            });
            self.attempts.len() - 1
        };
        let attempt = &mut self.attempts[index];
        attempt.objective = purpose.id.objective;
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
            attempt.last_observed_micros = now;
        }
    }
}
