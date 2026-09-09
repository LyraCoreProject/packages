//! Imported portal selection and source-local runner preparation for party Transfer.

use super::decision::{Action, ActionNode, MoveTarget, Reason, TransferAction};
use super::quest_catalog::pkg_playerbots_quest_objective;
use super::runner::{
    CompanionTransferPurpose, Failure, PlayerbotsRunner, QuestTransferPurpose, TransferCheckpoint,
    TransferPurpose,
};
use spacetimedb::ReducerContext;

const SUPPORTED_AREA_TRIGGERS: [u32; 3] = [78, 119, 121];
const TRANSFER_PRIORITY: i32 = 700;
const ARRIVAL_WAIT_MICROS: i64 = 30_000_000;

pub(super) fn candidate(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    partition: crate::group::PartyPartitionFacts,
    objective: u64,
) -> ActionNode {
    let route = SUPPORTED_AREA_TRIGGERS
        .into_iter()
        .filter_map(|trigger| {
            crate::actor::area_trigger_route(ctx, trigger).map(|route| (trigger, route))
        })
        .filter(|(_, route)| route.source_map == me.map_id && route.target_map == partition.map_id)
        .min_by(|left, right| {
            distance_sq(me, &left.1)
                .total_cmp(&distance_sq(me, &right.1))
                .then(left.0.cmp(&right.0))
        });
    let Some((trigger, route)) = route else {
        return ActionNode::ready(Action::Hold, Reason::Transfer, TRANSFER_PRIORITY);
    };
    let transfer = TransferAction {
        trigger,
        destination_map: partition.map_id,
        destination_instance: partition.instance_id,
    };
    let mut node = ActionNode::ready(
        Action::Transfer(transfer),
        Reason::Transfer,
        TRANSFER_PRIORITY,
    );
    node.candidate.id.objective = objective;
    if !route.contains(me.x, me.y, me.z) {
        let mut position = ActionNode::ready(
            Action::Move(MoveTarget::AreaTrigger(trigger)),
            Reason::TransferPosition,
            TRANSFER_PRIORITY,
        );
        position.candidate.id.objective = objective;
        node.prerequisites.push(position);
    }
    node
}

fn distance_sq(me: &crate::WorldEntity, route: &crate::quest::AreaTriggerRoute) -> f32 {
    (me.x - route.source_x).powi(2)
        + (me.y - route.source_y).powi(2)
        + (me.z - route.source_z).powi(2)
}

pub(super) fn normalize(
    ctx: &ReducerContext,
    state: &mut PlayerbotsRunner,
    me: &crate::WorldEntity,
    action: TransferAction,
    intent_id: u64,
    generation: u64,
    now: i64,
) {
    let objective_identity = state
        .objective
        .as_ref()
        .map_or(0, |objective| objective.identity);
    let purpose = checkpoint_purpose(ctx, state, now);
    state.generation = generation;
    state.foreground = None;
    state.chosen = None;
    state.candidate_order.clear();
    state.movement_progress = None;
    state.combat_progress = None;
    state.cast_progress = None;
    state.quest_progress.clear();
    state.progress_age_micros = None;
    state.last_target_health = None;
    state.defense_target = None;
    state.companion_heal_target_guid = None;
    state.companion_fight_target_guid = None;
    state.companion_buff_target_guid = None;
    state.transitions = 0;
    state.route_expansions = 0;
    state.route_budget = 0;
    state.retry_candidate = None;
    state.deferred_destinations.clear();
    state.next_eligible_micros = now;
    state.recovery = None;
    state.transfer_checkpoint = Some(TransferCheckpoint {
        intent_id,
        controller_generation: generation,
        source_map: me.map_id,
        source_instance: me.instance_id,
        destination_map: action.destination_map,
        destination_instance: action.destination_instance,
        objective_identity,
        arrival_started_micros: 0,
        purpose,
    });
}

fn checkpoint_purpose(
    ctx: &ReducerContext,
    state: &PlayerbotsRunner,
    now: i64,
) -> Option<TransferPurpose> {
    if let Some(member_guid) = state.companion_leader_guid {
        let attempt = state.recovery.as_ref().and_then(|recovery| {
            recovery.attempts.iter().find(|attempt| {
                attempt.objective
                    == state
                        .objective
                        .as_ref()
                        .map_or(0, |objective| objective.identity)
                    && attempt.reason == Reason::Follow
                    && attempt.work == super::recovery::Work::Follow(member_guid)
            })
        });
        return Some(TransferPurpose::Companion(CompanionTransferPurpose {
            member_guid,
            stalled_micros: attempt.map_or(0, |attempt| attempt.stalled_micros),
            approach: attempt
                .and_then(|attempt| attempt.position.as_ref())
                .map_or(0, |position| position.number),
            deferred_micros: attempt
                .and_then(|attempt| attempt.deferred_until_micros)
                .map_or(0, |until| until.saturating_sub(now).max(0)),
        }));
    }
    let objective = state
        .objective
        .as_ref()
        .filter(|objective| objective.kind == super::runner::ObjectiveKind::Quest)?;
    let retained = ctx
        .db
        .pkg_playerbots_quest_objective()
        .character_guid()
        .find(state.character_guid)
        .filter(|retained| retained.runner_objective_identity == objective.identity)?;
    let attempt = state.recovery.as_ref().and_then(|recovery| {
        recovery.attempts.iter().find(|attempt| {
            attempt.objective == retained.runner_objective_identity
                && attempt.reason == Reason::Quest
        })
    });
    Some(TransferPurpose::Quest(QuestTransferPurpose {
        quest: retained.quest_entry,
        objective_index: retained.target.objective_index,
        executor: retained.target.executor,
        source_entry: retained
            .target
            .source
            .as_ref()
            .map_or(retained.target.target_entry, |source| source.entry),
        stalled_micros: attempt.map_or(0, |attempt| attempt.stalled_micros),
        approach: attempt
            .and_then(|attempt| attempt.position.as_ref())
            .map_or(0, |position| position.number),
        deferred_micros: attempt
            .and_then(|attempt| attempt.deferred_until_micros)
            .map_or(0, |until| until.saturating_sub(now).max(0)),
    }))
}

pub(super) enum Arrival {
    Ready,
    Waiting,
    Invalid(Failure),
}

pub(super) fn requires_recovery(checkpoint: TransferCheckpoint) -> bool {
    match checkpoint.purpose {
        Some(TransferPurpose::Companion(CompanionTransferPurpose {
            stalled_micros,
            approach,
            deferred_micros,
            ..
        }))
        | Some(TransferPurpose::Quest(QuestTransferPurpose {
            stalled_micros,
            approach,
            deferred_micros,
            ..
        })) => stalled_micros > 0 || approach > 0 || deferred_micros > 0,
        None => false,
    }
}

pub(super) fn retains_quest(
    checkpoint: TransferCheckpoint,
    admission: &super::quest_catalog::QuestAdmission,
) -> bool {
    matches!(
        checkpoint.purpose,
        Some(TransferPurpose::Quest(QuestTransferPurpose {
            quest,
            objective_index,
            executor,
            source_entry,
            ..
        })) if admission.quest_entry == quest
            && admission.target.objective_index == objective_index
            && admission.target.executor == executor
            && admission
                .target
                .source
                .as_ref()
                .map_or(admission.target.target_entry, |source| source.entry)
                == source_entry
    )
}

pub(super) fn begin_arrival(state: &mut PlayerbotsRunner, me: &crate::WorldEntity, now: i64) {
    let Some(checkpoint) = &mut state.transfer_checkpoint else {
        return;
    };
    if checkpoint.arrival_started_micros == 0
        && (checkpoint.destination_map, checkpoint.destination_instance)
            == (me.map_id, me.instance_id)
    {
        checkpoint.arrival_started_micros = now;
    }
}

pub(super) fn arrival_expired(state: &PlayerbotsRunner, now: i64) -> bool {
    state.transfer_checkpoint.is_some_and(|checkpoint| {
        checkpoint.arrival_started_micros > 0
            && now.saturating_sub(checkpoint.arrival_started_micros) >= ARRIVAL_WAIT_MICROS
    })
}

pub(super) fn arrival(
    ctx: &ReducerContext,
    state: &PlayerbotsRunner,
    me: &crate::WorldEntity,
    party: Option<&super::companion::Party>,
    now: i64,
) -> Arrival {
    let Some(checkpoint) = &state.transfer_checkpoint else {
        return Arrival::Ready;
    };
    if checkpoint.controller_generation != state.generation {
        return Arrival::Invalid(Failure::TransferPurposeChanged);
    }
    if (checkpoint.destination_map, checkpoint.destination_instance) != (me.map_id, me.instance_id)
    {
        return Arrival::Invalid(Failure::TransferDestinationChanged);
    }
    match checkpoint.purpose {
        Some(TransferPurpose::Companion(CompanionTransferPurpose { member_guid, .. })) => {
            let Some(party) = party else {
                return Arrival::Invalid(Failure::TransferPurposeChanged);
            };
            if party.leader_guid != member_guid {
                return Arrival::Invalid(Failure::TransferPurposeChanged);
            }
            if party.members.iter().any(|member| {
                member.character_guid == member_guid
                    && (member.unit.as_ref().is_some_and(|unit| {
                        (unit.map_id, unit.instance_id) == (me.map_id, me.instance_id)
                    }) || member.partition.is_some_and(|partition| {
                        (partition.map_id, partition.instance_id) == (me.map_id, me.instance_id)
                    }))
            }) {
                Arrival::Ready
            } else if arrival_expired(state, now) {
                Arrival::Invalid(Failure::TransferArrivalUnavailable)
            } else {
                Arrival::Waiting
            }
        }
        Some(TransferPurpose::Quest(QuestTransferPurpose {
            quest,
            objective_index,
            executor,
            source_entry,
            ..
        })) => {
            if ctx
                .db
                .pkg_playerbots_quest_objective()
                .character_guid()
                .find(state.character_guid)
                .is_some_and(|retained| {
                    retained.runner_objective_identity == checkpoint.objective_identity
                        && retained.quest_entry == quest
                        && retained.target.objective_index == objective_index
                        && retained.target.executor == executor
                        && retained
                            .target
                            .source
                            .as_ref()
                            .map_or(retained.target.target_entry, |source| source.entry)
                            == source_entry
                })
            {
                Arrival::Ready
            } else {
                Arrival::Invalid(Failure::TransferPurposeChanged)
            }
        }
        None => Arrival::Ready,
    }
}

pub(super) fn restore_recovery(
    recovery: &mut super::recovery::Recovery,
    checkpoint: TransferCheckpoint,
    purpose: super::decision::Candidate,
    now: i64,
) -> Option<super::recovery::RestoredTransferAttempt> {
    let (reason, member_guid, stalled_micros, approach, deferred_micros) = match checkpoint.purpose
    {
        Some(TransferPurpose::Companion(CompanionTransferPurpose {
            member_guid,
            stalled_micros,
            approach,
            deferred_micros,
        })) => (
            Reason::Follow,
            Some(member_guid),
            stalled_micros,
            approach,
            deferred_micros,
        ),
        Some(TransferPurpose::Quest(QuestTransferPurpose {
            stalled_micros,
            approach,
            deferred_micros,
            ..
        })) => (
            Reason::Quest,
            None,
            stalled_micros,
            approach,
            deferred_micros,
        ),
        None => return None,
    };
    if purpose.id.reason != reason {
        return None;
    }
    recovery.restore_transfer_attempt(
        checkpoint.objective_identity,
        reason,
        member_guid,
        stalled_micros,
        approach,
        deferred_micros,
        now,
    )
}
