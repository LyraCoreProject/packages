//! Finite attempts at useful work. Dispatch and route planning do not establish progress.

use super::decision::{Action, Candidate, CastAction, MoveTarget, Reason};
use super::runner::{Destination, PlayerbotsRunner};
use crate::{game_gameobject, game_world_entity};
use spacetimedb::ReducerContext;

const CHANGE_APPROACH_MICROS: i64 = 10_000_000;
const ATTEMPT_LIMIT_MICROS: i64 = 30_000_000;
const DEFER_MICROS: i64 = 30_000_000;
const MEMORY_LIMIT: usize = 4;

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Work {
    Destination(u64),
    Follow(u64),
    Fight(u64),
    Heal(u64),
    Buff(CastAction),
    QuestInteraction { target: u64, quest: u32 },
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug)]
pub struct Attempt {
    pub work: Work,
    pub map_id: u32,
    pub instance_id: u64,
    pub geometry_revision: Option<u64>,
    pub objective: u64,
    pub last_observed_micros: i64,
    pub stalled_micros: i64,
    pub target_health: Option<u32>,
    pub position: Option<Approach>,
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

pub(super) enum Selection {
    Candidate(Candidate),
    Deferred { until_micros: i64 },
}

fn work(candidate: Candidate, state: &PlayerbotsRunner) -> Option<Work> {
    let target = match candidate.id.action {
        Action::Attack(target)
        | Action::Move(MoveTarget::Entity(target))
        | Action::Move(MoveTarget::CastingPosition(target)) => Some(target),
        Action::Cast(cast) => Some(cast.target),
        _ => None,
    };
    match candidate.id.reason {
        Reason::Survival | Reason::Resurrection | Reason::Restricted | Reason::CrowdControl => None,
        Reason::Follow => state.companion_leader_guid.map(Work::Follow),
        Reason::Heal | Reason::Recovery => target.map(Work::Heal),
        Reason::CastingPosition if target == state.companion_heal_target_guid => {
            target.map(Work::Heal)
        }
        Reason::Buff => match candidate.id.action {
            Action::Cast(cast) => Some(Work::Buff(cast)),
            _ => None,
        },
        _ => match candidate.id.action {
            Action::Attack(target)
            | Action::Cast(CastAction { target, .. })
            | Action::Move(MoveTarget::Entity(target))
            | Action::Move(MoveTarget::CastingPosition(target)) => Some(Work::Fight(target)),
            Action::Move(MoveTarget::Home) => Some(Work::Destination(candidate.id.objective)),
            Action::AcceptQuest(step) | Action::TurnInQuest(step) | Action::UseGameObject(step) => {
                Some(Work::QuestInteraction {
                    target: step.target,
                    quest: step.quest,
                })
            }
            Action::LootCreature(step) | Action::LootGameObject(step) => {
                Some(Work::QuestInteraction {
                    target: step.target,
                    quest: step.quest,
                })
            }
            Action::Move(MoveTarget::GameObject(target)) => {
                Some(Work::QuestInteraction { target, quest: 0 })
            }
            Action::Hold | Action::Resurrect | Action::Move(MoveTarget::RecoveryPosition(_)) => {
                None
            }
        },
    }
}

fn target(work: Work) -> Option<u64> {
    match work {
        Work::Destination(_) => None,
        Work::Follow(guid) | Work::Fight(guid) | Work::Heal(guid) => Some(guid),
        Work::Buff(cast) => Some(cast.target),
        Work::QuestInteraction { target, .. } => Some(target),
    }
}

impl Recovery {
    pub(super) fn position(&self, identity: u64) -> Option<Destination> {
        self.attempts
            .iter()
            .filter_map(|attempt| attempt.position.as_ref())
            .find(|position| position.identity == identity)
            .map(|position| position.destination.clone())
    }

    pub(super) fn select(
        &mut self,
        ctx: &ReducerContext,
        me: &crate::WorldEntity,
        state: &PlayerbotsRunner,
        candidate: Candidate,
        now: i64,
    ) -> Selection {
        let Some(work) = work(candidate, state) else {
            self.active = None;
            return Selection::Candidate(candidate);
        };
        let geometry = crate::nav::coverage_generation(ctx, me.map_id);
        self.attempts.retain(|attempt| {
            (attempt.map_id, attempt.instance_id) == (me.map_id, me.instance_id)
                && attempt.geometry_revision == geometry
                && attempt
                    .deferred_until_micros
                    .is_none_or(|until| until > now)
        });
        let unit = target(work).and_then(|guid| ctx.db.game_world_entity().guid().find(guid));
        let health = unit.as_ref().map(|unit| unit.health);
        let index = if let Some(index) = self
            .attempts
            .iter()
            .position(|attempt| attempt.work == work)
        {
            index
        } else {
            if self.attempts.len() == MEMORY_LIMIT {
                if let Some(index) = self.attempts.iter().position(|attempt| {
                    attempt.deferred_until_micros.is_none() && self.active != Some(attempt.work)
                }) {
                    self.attempts.remove(index);
                } else {
                    return Selection::Deferred {
                        until_micros: self
                            .attempts
                            .iter()
                            .filter_map(|attempt| attempt.deferred_until_micros)
                            .min()
                            .unwrap_or(now.saturating_add(DEFER_MICROS)),
                    };
                }
            }
            self.attempts.push(Attempt {
                work,
                map_id: me.map_id,
                instance_id: me.instance_id,
                geometry_revision: geometry,
                objective: candidate.id.objective,
                last_observed_micros: now,
                stalled_micros: 0,
                target_health: health,
                position: None,
                deferred_until_micros: None,
            });
            self.attempts.len() - 1
        };
        let attempt = &mut self.attempts[index];
        if let Some(until_micros) = attempt.deferred_until_micros {
            self.active = None;
            return Selection::Deferred { until_micros };
        }
        let health_progress = match (work, attempt.target_health, health) {
            (Work::Fight(_), Some(before), Some(after)) => after < before,
            (Work::Heal(_), Some(before), Some(after)) => after > before,
            _ => false,
        };
        let movement_progress = attempt.position.is_none()
            && state.movement_progress.as_ref().is_some_and(|progress| {
                progress.observed_micros == now
                    && state
                        .chosen
                        .is_some_and(|chosen| matches!(chosen.id.action, Action::Move(_)))
            });
        if health_progress || movement_progress {
            attempt.stalled_micros = 0;
            attempt.position = None;
        } else if self.active == Some(work) {
            attempt.stalled_micros = attempt
                .stalled_micros
                .saturating_add(now.saturating_sub(attempt.last_observed_micros).max(0));
        }
        attempt.last_observed_micros = now;
        attempt.target_health = health;
        attempt.objective = candidate.id.objective;
        self.active = Some(work);
        if attempt.stalled_micros >= ATTEMPT_LIMIT_MICROS {
            let until_micros = now.saturating_add(DEFER_MICROS);
            attempt.deferred_until_micros = Some(until_micros);
            attempt.position = None;
            self.active = None;
            return Selection::Deferred { until_micros };
        }
        if attempt.stalled_micros < CHANGE_APPROACH_MICROS {
            return Selection::Candidate(candidate);
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
            let destination = unit
                .as_ref()
                .map(|unit| (unit.x, unit.y, unit.z))
                .or_else(|| {
                    target(work)
                        .and_then(|guid| ctx.db.game_gameobject().guid().find(guid))
                        .map(|object| (object.x, object.y, object.z))
                })
                .or_else(|| {
                    state.objective.as_ref().map(|objective| {
                        let destination = &objective.destination;
                        (destination.x, destination.y, destination.z)
                    })
                });
            let Some((x, y, _)) = destination else {
                return Selection::Deferred {
                    until_micros: now.saturating_add(DEFER_MICROS),
                };
            };
            let dx = x - me.x;
            let dy = y - me.y;
            let length = (dx * dx + dy * dy).sqrt();
            let (dx, dy) = if length > 0.05 {
                (dx / length, dy / length)
            } else {
                (1.0, 0.0)
            };
            let side = if number == 1 { 1.0 } else { -1.0 };
            self.position_sequence = self.position_sequence.saturating_add(1);
            attempt.position = Some(Approach {
                identity: self.position_sequence,
                number,
                destination: Destination {
                    map_id: me.map_id,
                    instance_id: me.instance_id,
                    x: me.x - dy * 6.0 * side,
                    y: me.y + dx * 6.0 * side,
                    z: me.z,
                    geometry_revision: geometry,
                },
            });
        }
        let position = attempt
            .position
            .as_ref()
            .expect("the selected approach has a position");
        if ((me.x - position.destination.x).powi(2) + (me.y - position.destination.y).powi(2))
            <= 0.25
        {
            return Selection::Candidate(candidate);
        }
        let mut adjusted = candidate;
        adjusted.id.action = Action::Move(MoveTarget::RecoveryPosition(position.identity));
        Selection::Candidate(adjusted)
    }
}
