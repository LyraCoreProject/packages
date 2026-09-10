#![cfg(feature = "debug_reducers")]

//! Declared inputs for the five-Character companion acceptance route.
//!
//! The fixture creates content and a timed fault plan before the route begins. After `begin`, the
//! only mutation reducer consumes one already-declared fault at or after its deadline. Movement,
//! orders, combat, recovery, AreaTriggers, and Transfer remain on their ordinary owners.

use super::{pkg_playerbots_bot, pkg_playerbots_provisioning, Controller};
use crate::nav::game_nav_chunk;
use crate::threat::top_threat_target; // package-api: exempt private observers record authoritative Core threat ordering
use crate::{
    game_area_trigger, game_areatrigger_teleport, game_character, game_character_quest,
    game_character_shard, game_creature_move_schedule, game_creature_quest, game_creature_spawn,
    game_group, game_group_member, game_group_member_partition, game_group_roster_revision,
    game_item_instance, game_quest_objective, game_quest_template, game_spell_cast_event,
    game_spell_impact_event, game_world_entity,
};
use spacetimedb::{reducer, table, Identity, ReducerContext, ScheduleAt, Table};
use std::collections::BTreeSet;

const GROUP: u64 = 5_098_000;
const MEMBER_IDS: [u64; 5] = [1, 2, 3, 4, 5];
const EXIT_TRIGGER: u32 = 119;
const RETAINED_QUEST: u32 = 50_911;
const RETAINED_OBJECTIVE: u64 = 5_098_101;
const INTERACTION_GIVER_ENTRY: u32 = 5_090_101;
const CONTROL_SPELL: u32 = 50_020;
const FAULT_WOUND: u8 = 0;
const FAULT_CONTROL: u8 = 1;
const FAULT_DEATH: u8 = 2;
const FAULT_CLEAR_CONTROL: u8 = 3;
const COMBAT_RECEIPT_LIMIT: usize = 12;
// Three 1,000-health pulls can require 150 fixture spell impacts at 20 damage each. The remaining
// rows cover the declared one-time armor, Fortitude, heal, and terminal in-flight casts.
const CAST_RECEIPT_LIMIT: usize = 256;
const CAST_EVENT_READ_LIMIT: usize = 256;
const CAST_IMPACT_EVENT_READ_LIMIT: usize = 256;
const CAST_GO_KIND: u8 = 2;
const IMPACT_FAILURE_RECEIPT_OVERFLOW: u8 = 1;
const IMPACT_FAILURE_EVENT_OVERFLOW: u8 = 2;
const IMPACT_FAILURE_MISSING_RECEIPT: u8 = 3;
const IMPACT_FAILURE_AMBIGUOUS_RECEIPT: u8 = 4;
const IMPACT_FAILURE_DUPLICATE_EVENT: u8 = 5;
const EXPECTED_FAULTS: [(u8, i64); 4] = [
    (FAULT_WOUND, 20_000_000),
    (FAULT_CONTROL, 40_000_000),
    (FAULT_DEATH, 60_000_000),
    (FAULT_CLEAR_CONTROL, 75_000_000),
];
const EXIT_SOURCE: (f32, f32, f32) = (-14.3628, -393.38, 64.5605);
const EXIT_LANDING: (f32, f32, f32, f32) = (-11_208.7, 1_675.9, 24.5733, 4.71239);

#[table(accessor = pkg_playerbots_companion_acceptance, public)]
pub struct CompanionAcceptance {
    #[primary_key]
    pub id: u8,
    pub leader_guid: u64,
    pub warrior_guid: u64,
    pub priest_guid: u64,
    pub mage_one_guid: u64,
    pub mage_two_guid: u64,
    pub enemy_guids: Vec<u64>,
    pub reward_quest: u32,
    pub staged_micros: i64,
    pub begun_micros: i64,
}

#[table(accessor = pkg_playerbots_companion_fault, public)]
pub struct CompanionFault {
    #[primary_key]
    pub id: u8,
    pub kind: u8,
    pub subject_guid: u64,
    pub target_guid: u64,
    pub due_offset_micros: i64,
    pub applied_micros: i64,
    pub death_dead: bool,
    pub death_was_ghost: bool,
    pub death_health: u32,
    pub death_player_flags: u32,
    pub death_x: f32,
    pub death_y: f32,
    pub death_z: f32,
    pub ghost_observed_micros: i64,
    pub ghost_x: f32,
    pub ghost_y: f32,
    pub ghost_z: f32,
    pub alive_observed_micros: i64,
    pub alive_x: f32,
    pub alive_y: f32,
    pub alive_z: f32,
}

#[table(accessor = pkg_playerbots_companion_combat_receipt, public)]
pub struct CompanionCombatReceipt {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub attacker_guid: u64,
    pub target_guid: u64,
    pub tank_is_top_threat: bool,
    pub observed_micros: i64,
}

#[derive(Clone)]
#[table(accessor = pkg_playerbots_companion_cast_receipt, public)]
pub struct CompanionCastReceipt {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub caster_guid: u64,
    pub target_guid: u64,
    pub spell_id: u32,
    pub source_event_id: u64,
    pub scheduled_id: u64,
    pub damage: u32,
    pub healed: u32,
    pub target_health_after: u32,
    pub resolved_micros: i64,
    pub finished_micros: i64,
    // Deferred projectile evidence. A zero event id means this cast had no observed impact.
    #[default(0u64)]
    pub impact_event_id: u64,
    #[default(0i64)]
    pub impact_micros: i64,
    // 1 receipt overflow, 2 event overflow, 3 missing receipt, 4 ambiguity, 5 duplicate event.
    #[default(0u8)]
    pub impact_failure: u8,
    #[default(0u64)]
    pub impact_failure_event_id: u64,
}

// These rows belong to the one private acceptance run, not to any Character they name. Several
// participants share the plan, fault, and observation rows. They must remain on the source Shard
// after a participant transfers so the caller can retain the complete evidence. The private
// database teardown removes them after the run.
crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_companion_acceptance(_ctx, _character_guid) {});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_playerbots_companion_acceptance());
crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_companion_fault(_ctx, _character_guid) {});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_playerbots_companion_fault());
crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_companion_combat_receipt(_ctx, _character_guid) {});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_playerbots_companion_combat_receipt());
crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_companion_cast_receipt(_ctx, _character_guid) {});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_playerbots_companion_cast_receipt());

crate::game_hook!(on_damage_taken, fn playerbots_companion_acceptance_observe_damage(ctx, payload) {
    let Some(plan) = ctx.db.pkg_playerbots_companion_acceptance().id().find(0) else {
        return;
    };
    if plan.begun_micros == 0
        || ![
            plan.warrior_guid,
            plan.priest_guid,
            plan.mage_one_guid,
            plan.mage_two_guid,
        ]
        .contains(&payload.attacker_guid)
        || !plan.enemy_guids.contains(&payload.target_guid)
    {
        return;
    }
    let tank_is_top_threat = payload.attacker_guid == plan.warrior_guid
        && top_threat_target(ctx, payload.target_guid) == Some(plan.warrior_guid);
    let receipts = ctx.db.pkg_playerbots_companion_combat_receipt();
    let retained: Vec<_> = receipts.iter().take(COMBAT_RECEIPT_LIMIT + 1).collect();
    if retained.len() > COMBAT_RECEIPT_LIMIT {
        return;
    }
    let receipt_count = retained.len();
    if let Some(mut receipt) = retained
        .into_iter()
        .find(|receipt| {
            receipt.attacker_guid == payload.attacker_guid
                && receipt.target_guid == payload.target_guid
        })
    {
        receipt.tank_is_top_threat |= tank_is_top_threat;
        receipt.observed_micros = ctx.timestamp.to_micros_since_unix_epoch();
        receipts.id().update(receipt);
    } else if receipt_count < COMBAT_RECEIPT_LIMIT {
        receipts.insert(CompanionCombatReceipt {
            id: 0,
            attacker_guid: payload.attacker_guid,
            target_guid: payload.target_guid,
            tank_is_top_threat,
            observed_micros: ctx.timestamp.to_micros_since_unix_epoch(),
        });
    }
});

crate::game_hook!(on_cast_resolved, fn playerbots_companion_acceptance_observe_resolved_cast(ctx, payload) {
    let Some(plan) = ctx.db.pkg_playerbots_companion_acceptance().id().find(0) else {
        return;
    };
    if plan.begun_micros == 0
        || ![
            plan.warrior_guid,
            plan.priest_guid,
            plan.mage_one_guid,
            plan.mage_two_guid,
        ]
        .contains(&payload.caster_guid)
        || !plan
            .enemy_guids
            .iter()
            .copied()
            .chain([plan.leader_guid, plan.mage_one_guid])
            .any(|target| target == payload.target_guid)
    {
        return;
    }
    let receipts = ctx.db.pkg_playerbots_companion_cast_receipt();
    let retained: Vec<_> = receipts.iter().take(CAST_RECEIPT_LIMIT + 1).collect();
    if retained.len() > CAST_RECEIPT_LIMIT {
        return;
    }
    let cast_events: Vec<_> = ctx
        .db
        .game_spell_cast_event()
        .iter()
        .take(CAST_EVENT_READ_LIMIT + 1)
        .collect();
    if cast_events.len() > CAST_EVENT_READ_LIMIT {
        return;
    }
    let matching_events: Vec<_> = cast_events
        .into_iter()
        .filter(|event| {
            event.caster_guid == payload.caster_guid
                && event.target_guid == payload.target_guid
                && event.spell_id == payload.spell_id
                && event.created_at == ctx.timestamp
                && event.cast_time_ms == 0
                && event.kind == CAST_GO_KIND
                && !event.is_interrupted
        })
        .take(2)
        .collect();
    if retained.len() < CAST_RECEIPT_LIMIT && matching_events.len() == 1 {
        let event = &matching_events[0];
        receipts.insert(CompanionCastReceipt {
            id: 0,
            caster_guid: payload.caster_guid,
            target_guid: payload.target_guid,
            spell_id: payload.spell_id,
            source_event_id: event.id,
            scheduled_id: 0,
            damage: event.damage,
            healed: event.healed,
            target_health_after: ctx
                .db
                .game_world_entity()
                .guid()
                .find(payload.target_guid)
                .map_or(0, |target| target.health),
            resolved_micros: ctx.timestamp.to_micros_since_unix_epoch(),
            finished_micros: 0,
            impact_event_id: 0,
            impact_micros: 0,
            impact_failure: 0,
            impact_failure_event_id: 0,
        });
    }
});

crate::game_hook!(on_cast_finished, fn playerbots_companion_acceptance_observe_finished_cast(ctx, payload) {
    if !matches!(&payload.outcome, crate::spell::CastFinish::Resolved) {
        return;
    }
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    let receipts = ctx.db.pkg_playerbots_companion_cast_receipt();
    let retained: Vec<_> = receipts.iter().take(CAST_RECEIPT_LIMIT + 1).collect();
    if retained.len() > CAST_RECEIPT_LIMIT {
        return;
    }
    let matching_receipts: Vec<_> = retained
        .into_iter()
        .filter(|receipt| {
            receipt.caster_guid == payload.caster_guid
                && receipt.target_guid == payload.target_guid
                && receipt.resolved_micros == now
                && receipt.source_event_id != 0
                && receipt.scheduled_id == 0
        })
        .take(2)
        .collect();
    if matching_receipts.len() == 1 {
        let mut receipt = matching_receipts.into_iter().next().unwrap();
        receipt.scheduled_id = payload.scheduled_id;
        receipt.finished_micros = now;
        receipts.id().update(receipt);
    }
});

fn observed_cast_participant(plan: &CompanionAcceptance, character_guid: u64) -> bool {
    [
        plan.warrior_guid,
        plan.priest_guid,
        plan.mage_one_guid,
        plan.mage_two_guid,
    ]
    .contains(&character_guid)
}

fn observed_cast_target(plan: &CompanionAcceptance, target_guid: u64) -> bool {
    plan.enemy_guids
        .iter()
        .copied()
        .chain([plan.leader_guid, plan.mage_one_guid])
        .any(|candidate| candidate == target_guid)
}

fn record_impact_failure(
    ctx: &ReducerContext,
    retained: &mut [CompanionCastReceipt],
    receipt_id: Option<u64>,
    failure: u8,
    event: Option<&crate::SpellImpactEvent>,
) {
    let receipts = ctx.db.pkg_playerbots_companion_cast_receipt();
    let existing = receipt_id
        .and_then(|id| retained.iter().position(|receipt| receipt.id == id))
        .or_else(|| (!retained.is_empty()).then_some(0));
    if let Some(index) = existing {
        retained[index].impact_failure = failure;
        retained[index].impact_failure_event_id = event.map_or(0, |event| event.id);
        receipts.id().update(retained[index].clone());
        return;
    }
    let (caster_guid, target_guid, spell_id, impact_event_id, impact_micros) =
        event.map_or((0, 0, 0, 0, 0), |event| {
            (
                event.caster_guid,
                event.target_guid,
                event.spell_id,
                event.id,
                event.created_at.to_micros_since_unix_epoch(),
            )
        });
    receipts.insert(CompanionCastReceipt {
        id: 0,
        caster_guid,
        target_guid,
        spell_id,
        source_event_id: 0,
        scheduled_id: 0,
        damage: 0,
        healed: 0,
        target_health_after: 0,
        resolved_micros: 0,
        finished_micros: 0,
        impact_event_id,
        impact_micros,
        impact_failure: failure,
        impact_failure_event_id: impact_event_id,
    });
}

fn reconcile_cast_impacts(ctx: &ReducerContext, plan: &CompanionAcceptance) {
    let receipts = ctx.db.pkg_playerbots_companion_cast_receipt();
    let mut retained: Vec<_> = receipts.iter().take(CAST_RECEIPT_LIMIT + 1).collect();
    if retained.len() > CAST_RECEIPT_LIMIT {
        record_impact_failure(
            ctx,
            &mut retained,
            None,
            IMPACT_FAILURE_RECEIPT_OVERFLOW,
            None,
        );
        return;
    }
    if retained.iter().any(|receipt| receipt.impact_failure != 0) {
        return;
    }
    let mut retained_event_ids = BTreeSet::new();
    if let Some(duplicate) = retained.iter().find_map(|receipt| {
        (receipt.impact_event_id != 0 && !retained_event_ids.insert(receipt.impact_event_id))
            .then_some(receipt.id)
    }) {
        record_impact_failure(
            ctx,
            &mut retained,
            Some(duplicate),
            IMPACT_FAILURE_DUPLICATE_EVENT,
            None,
        );
        return;
    }
    let mut impacts: Vec<_> = ctx
        .db
        .game_spell_impact_event()
        .iter()
        .take(CAST_IMPACT_EVENT_READ_LIMIT + 1)
        .collect();
    if impacts.len() > CAST_IMPACT_EVENT_READ_LIMIT {
        record_impact_failure(
            ctx,
            &mut retained,
            None,
            IMPACT_FAILURE_EVENT_OVERFLOW,
            None,
        );
        return;
    }
    impacts.sort_by_key(|event| (event.created_at.to_micros_since_unix_epoch(), event.id));
    for impact in impacts.into_iter().filter(|event| {
        event.created_at.to_micros_since_unix_epoch() >= plan.begun_micros
            && observed_cast_participant(plan, event.caster_guid)
            && observed_cast_target(plan, event.target_guid)
    }) {
        if retained_event_ids.contains(&impact.id) {
            continue;
        }
        let impact_micros = impact.created_at.to_micros_since_unix_epoch();
        // The public impact row has no scheduled cast id. Associate it only while exactly one
        // unresolved receipt with the same semantic identity precedes it.
        let matching: Vec<_> = retained
            .iter()
            .enumerate()
            .filter(|(_, receipt)| {
                receipt.impact_event_id == 0
                    && receipt.source_event_id != 0
                    && receipt.caster_guid == impact.caster_guid
                    && receipt.target_guid == impact.target_guid
                    && receipt.spell_id == impact.spell_id
                    && receipt.resolved_micros <= impact_micros
            })
            .map(|(index, _)| index)
            .take(2)
            .collect();
        if matching.len() != 1 {
            let receipt_id = matching.first().map(|index| retained[*index].id);
            record_impact_failure(
                ctx,
                &mut retained,
                receipt_id,
                if matching.is_empty() {
                    IMPACT_FAILURE_MISSING_RECEIPT
                } else {
                    IMPACT_FAILURE_AMBIGUOUS_RECEIPT
                },
                Some(&impact),
            );
            return;
        }
        let index = matching[0];
        retained[index].damage = impact.damage;
        retained[index].impact_event_id = impact.id;
        retained[index].impact_micros = impact_micros;
        receipts.id().update(retained[index].clone());
        retained_event_ids.insert(impact.id);
    }
}

crate::game_tick_pass!(fn playerbots_companion_acceptance_observe_tick(ctx) {
    let Some(plan) = ctx.db.pkg_playerbots_companion_acceptance().id().find(0) else {
        return;
    };
    if plan.begun_micros == 0 {
        return;
    }
    reconcile_cast_impacts(ctx, &plan);
    let combat_receipts = ctx.db.pkg_playerbots_companion_combat_receipt();
    let retained: Vec<_> = combat_receipts
        .iter()
        .take(COMBAT_RECEIPT_LIMIT + 1)
        .collect();
    if retained.len() <= COMBAT_RECEIPT_LIMIT {
        for mut receipt in retained {
            if receipt.attacker_guid == plan.warrior_guid
                && plan.enemy_guids.contains(&receipt.target_guid)
                && top_threat_target(ctx, receipt.target_guid) == Some(plan.warrior_guid)
                && !receipt.tank_is_top_threat
            {
                receipt.tank_is_top_threat = true;
                receipt.observed_micros = ctx.timestamp.to_micros_since_unix_epoch();
                combat_receipts.id().update(receipt);
            }
        }
    }
    let faults = ctx.db.pkg_playerbots_companion_fault();
    let Some(mut death) = faults.id().find(FAULT_DEATH) else {
        return;
    };
    if death.applied_micros == 0 || death.alive_observed_micros != 0 {
        return;
    }
    let Some(body) = ctx.db.game_world_entity().guid().find(death.subject_guid) else {
        return;
    };
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    let ghost = body.player_flags & lyracore_shared::constants::player_flags::GHOST != 0;
    if death.ghost_observed_micros == 0 && body.dead && ghost {
        death.ghost_observed_micros = now;
        (death.ghost_x, death.ghost_y, death.ghost_z) = (body.x, body.y, body.z);
        faults.id().update(death);
    } else if death.ghost_observed_micros != 0 && !body.dead && !ghost && body.health > 0 {
        death.alive_observed_micros = now;
        (death.alive_x, death.alive_y, death.alive_z) = (body.x, body.y, body.z);
        faults.id().update(death);
    }
});

fn plan(ctx: &ReducerContext) -> Result<CompanionAcceptance, String> {
    ctx.db
        .pkg_playerbots_companion_acceptance()
        .id()
        .find(0)
        .ok_or("companion acceptance fixture is not staged".to_string())
}

fn wound_without_moving(ctx: &ReducerContext, guid: u64) -> Result<(), String> {
    let mut entity = crate::helpers::live_entity(ctx, guid)?;
    if entity.dead {
        return Err(format!(
            "companion acceptance cannot wound dead Character {guid}"
        ));
    }
    let location = (
        entity.map_id,
        entity.instance_id,
        entity.x,
        entity.y,
        entity.z,
        entity.grid_x,
        entity.grid_y,
        entity.cell,
    );
    let health = (entity.max_health.saturating_mul(35) / 100).max(1);
    entity.health = health;
    ctx.db.game_world_entity().guid().update(entity);
    let wounded = crate::helpers::live_entity(ctx, guid)?;
    if wounded.health != health
        || (
            wounded.map_id,
            wounded.instance_id,
            wounded.x,
            wounded.y,
            wounded.z,
            wounded.grid_x,
            wounded.grid_y,
            wounded.cell,
        ) != location
    {
        return Err(format!(
            "companion acceptance wound changed Character {guid} location"
        ));
    }
    Ok(())
}

fn exact_member_guids(ctx: &ReducerContext) -> Result<Vec<u64>, String> {
    let mut members: Vec<_> = ctx
        .db
        .game_group_member()
        .by_group()
        .filter(&GROUP)
        .take(lyracore_shared::group::GROUP_MAX_MEMBERS + 1)
        .map(|member| member.character_guid)
        .collect();
    if members.len() != 5 {
        return Err("companion acceptance requires the exact five-member party".to_string());
    }
    members.sort_unstable();
    Ok(members)
}

fn declare_exit_route(ctx: &ReducerContext) -> Result<(), String> {
    if ctx.db.game_area_trigger().id().find(EXIT_TRIGGER).is_some()
        || ctx
            .db
            .game_areatrigger_teleport()
            .trigger_id()
            .find(EXIT_TRIGGER)
            .is_some()
    {
        return Err("companion acceptance refuses an existing Deadmines exit route".to_string());
    }
    ctx.db.game_area_trigger().insert(crate::GameAreaTrigger {
        id: EXIT_TRIGGER,
        map_id: 36,
        x: EXIT_SOURCE.0,
        y: EXIT_SOURCE.1,
        z: EXIT_SOURCE.2,
        radius: 6.0,
        box_length: 0.0,
        box_width: 0.0,
        box_height: 0.0,
        box_yaw: 0.0,
    });
    ctx.db
        .game_areatrigger_teleport()
        .insert(crate::AreatriggerTeleport {
            trigger_id: EXIT_TRIGGER,
            target_map: 0,
            x: EXIT_LANDING.0,
            y: EXIT_LANDING.1,
            z: EXIT_LANDING.2,
            o: EXIT_LANDING.3,
            name: "Deadmines - Leaving (companion acceptance fixture)".to_string(),
        });
    Ok(())
}

fn realm_partition(character_guid: u64, membership_revision: u64) -> crate::GroupMemberPartition {
    crate::GroupMemberPartition {
        character_guid,
        group_id: GROUP,
        membership_revision,
        member_active: true,
        map_id: 0,
        instance_id: 0,
        locator_revision: 1,
        state: crate::PartyPartitionState::Known,
    }
}

fn realm_locator(character_guid: u64, now: i64) -> crate::CharacterShard {
    crate::CharacterShard {
        character_guid,
        map_id: 0,
        instance_id: 0,
        updated_micros: now,
        revision: 1,
        bot_source_identity: Identity::ZERO,
        bot_transfer_intent_id: 0,
        bot_controller_generation: 0,
        transfer_pending: false,
        pending_destination_map: 0,
        pending_destination_instance: 0,
    }
}

/// Stage the authoritative five-Character party before the Gateway or route clock starts.
#[reducer]
#[allow(clippy::too_many_arguments)]
pub fn playerbots_companion_acceptance_realm_stage(
    ctx: &ReducerContext,
    leader_guid: u64,
    warrior_guid: u64,
    priest_guid: u64,
    mage_one_guid: u64,
    mage_two_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let guids = [
        leader_guid,
        warrior_guid,
        priest_guid,
        mage_one_guid,
        mage_two_guid,
    ];
    if guids.contains(&0) || guids.into_iter().collect::<BTreeSet<_>>().len() != guids.len() {
        return Err(
            "companion acceptance Realm party requires five distinct Characters".to_string(),
        );
    }
    if ctx.db.game_group().group_id().find(GROUP).is_some()
        || ctx
            .db
            .game_group_roster_revision()
            .group_id()
            .find(GROUP)
            .is_some()
        || ctx
            .db
            .game_group_member()
            .by_group()
            .filter(&GROUP)
            .next()
            .is_some()
        || ctx
            .db
            .game_group_member_partition()
            .by_group()
            .filter(&GROUP)
            .next()
            .is_some()
        || guids.iter().any(|guid| {
            ctx.db
                .game_character_shard()
                .character_guid()
                .find(*guid)
                .is_some()
        })
    {
        return Err("companion acceptance Realm party requires fresh authority rows".to_string());
    }
    ctx.db.game_group().insert(crate::Group {
        group_id: GROUP,
        leader_guid,
        loot_method: 0,
        loot_threshold: 2,
        rr_cursor: 0,
        master_looter_guid: 0,
    });
    let members = ctx.db.game_group_member();
    let partitions = ctx.db.game_group_member_partition();
    let locators = ctx.db.game_character_shard();
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    for (guid, membership_revision) in guids.into_iter().zip(MEMBER_IDS) {
        members.insert(crate::GroupMember {
            id: membership_revision,
            group_id: GROUP,
            character_guid: guid,
            owner_identity: Identity::ZERO,
        });
        partitions.insert(realm_partition(guid, membership_revision));
        locators.insert(realm_locator(guid, now));
    }
    ctx.db
        .game_group_roster_revision()
        .insert(crate::GroupRosterRevision {
            group_id: GROUP,
            revision: 2,
            active: true,
        });
    Ok(())
}

/// Stage only the imported exit route on the empty destination Shard.
#[reducer]
pub fn playerbots_companion_acceptance_destination_stage(
    ctx: &ReducerContext,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    if ctx.db.game_group().group_id().find(GROUP).is_some()
        || ctx
            .db
            .game_group_member()
            .by_group()
            .filter(&GROUP)
            .next()
            .is_some()
    {
        return Err("companion acceptance destination party state is occupied".to_string());
    }
    declare_exit_route(ctx)
}

/// Add and accept one unfinished Quest through the normal action owner after the reward fixture.
#[reducer]
pub fn playerbots_companion_acceptance_accept_retained_quest(
    ctx: &ReducerContext,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let plan = plan(ctx)?;
    if plan.begun_micros != 0 {
        return Err("companion acceptance refuses Quest setup after begin".to_string());
    }
    if ctx
        .db
        .game_quest_template()
        .entry()
        .find(RETAINED_QUEST)
        .is_some()
        || ctx
            .db
            .game_quest_objective()
            .id()
            .find(RETAINED_OBJECTIVE)
            .is_some()
    {
        return Err("companion acceptance retained Quest content is occupied".to_string());
    }
    let mut template = ctx
        .db
        .game_quest_template()
        .entry()
        .find(plan.reward_quest)
        .ok_or("companion acceptance reward Quest template is missing")?;
    template.entry = RETAINED_QUEST;
    template.title = "Companion route retained work".to_string();
    template.src_item = 0;
    template.src_item_count = 0;
    ctx.db.game_quest_template().insert(template);
    ctx.db.game_quest_objective().insert(crate::QuestObjective {
        id: RETAINED_OBJECTIVE,
        quest_entry: RETAINED_QUEST,
        obj_index: 0,
        kind: crate::quest::objective_kind::KILL_CREATURE,
        target_entry: 5_098_003,
        required_count: 2,
    });
    for role in [
        crate::quest::quest_role::START,
        crate::quest::quest_role::END,
    ] {
        ctx.db.game_creature_quest().insert(crate::CreatureQuest {
            id: 0,
            creature_entry: INTERACTION_GIVER_ENTRY,
            quest_entry: RETAINED_QUEST,
            role,
        });
    }
    let giver = (0xF130u64 << 48) | (u64::from(INTERACTION_GIVER_ENTRY) << 24) | 1;
    let bot = crate::helpers::live_entity(ctx, plan.warrior_guid)?;
    let mut giver_body = crate::helpers::live_entity(ctx, giver)?;
    giver_body.map_id = bot.map_id;
    giver_body.instance_id = bot.instance_id;
    giver_body.x = bot.x + 1.0;
    giver_body.y = bot.y;
    giver_body.z = bot.z;
    let (grid_x, grid_y) = lyracore_shared::spatial::grid_cell(giver_body.x, giver_body.y);
    giver_body.grid_x = grid_x;
    giver_body.grid_y = grid_y;
    giver_body.cell = lyracore_shared::spatial::grid_cell_id(grid_x, grid_y);
    ctx.db.game_world_entity().guid().update(giver_body);
    super::actions::accept_quest(ctx, plan.warrior_guid, giver, RETAINED_QUEST).map_err(
        |refusal| format!("companion acceptance retained Quest was refused: {refusal:?}"),
    )?;
    let retained = ctx
        .db
        .game_character_quest()
        .by_character_quest()
        .filter((plan.warrior_guid, RETAINED_QUEST))
        .next()
        .filter(|quest| !quest.rewarded && !quest.failed)
        .ok_or("companion acceptance retained Quest did not become active")?;
    let _ = retained;
    Ok(())
}

fn verify_bot(ctx: &ReducerContext, guid: u64, class: u8, role: u8) -> Result<(), String> {
    let bot = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(guid)
        .next()
        .filter(|bot| (bot.class, bot.role) == (class, role))
        .ok_or_else(|| format!("companion acceptance bot {guid} has the wrong class or role"))?;
    if bot.controller != Controller::Cohort || bot.next_think_micros != i64::MAX {
        return Err(format!(
            "companion acceptance bot {guid} must be a parked Cohort before begin"
        ));
    }
    Ok(())
}

fn retain_staged_positions(ctx: &ReducerContext, guids: [u64; 5]) -> Result<(), String> {
    for guid in guids {
        let body = crate::helpers::live_entity(ctx, guid)?;
        let mut character = crate::helpers::character_by_guid(ctx, guid)
            .ok_or("companion acceptance Character missing")?;
        character.map_id = body.map_id;
        character.pending_instance_id = body.instance_id;
        character.x = body.x;
        character.y = body.y;
        character.z = body.z;
        character.orientation = body.orientation;
        ctx.db.game_character().guid().update(character);
    }
    Ok(())
}

fn stage_repair_wall(ctx: &ReducerContext, priest_guid: u64, mage_guid: u64) -> Result<(), String> {
    let priest = crate::helpers::live_entity(ctx, priest_guid)?;
    let mut mage = crate::helpers::live_entity(ctx, mage_guid)?;
    mage.x = priest.x + 100.0;
    mage.y = priest.y;
    mage.z = priest.z;
    let (grid_x, grid_y) = lyracore_shared::spatial::grid_cell(mage.x, mage.y);
    mage.grid_x = grid_x;
    mage.grid_y = grid_y;
    mage.cell = lyracore_shared::spatial::grid_cell_id(grid_x, grid_y);
    ctx.db.game_world_entity().guid().update(mage);
    let wall_x = priest.x + 50.0;
    let cell_x = lyracore_shared::terrain::cell_index(wall_x).ok_or("repair wall off grid")?;
    let cell_y = lyracore_shared::terrain::cell_index(priest.y).ok_or("repair wall off grid")?;
    let walk_sub = lyracore_shared::nav::sub_index(wall_x, cell_x, lyracore_shared::nav::WALK_DIM)
        .ok_or("repair wall off grid")?;
    let walk_y = lyracore_shared::nav::sub_index(priest.y, cell_y, lyracore_shared::nav::WALK_DIM)
        .ok_or("repair wall off grid")?;
    let obstacle_sub =
        lyracore_shared::nav::sub_index(wall_x, cell_x, lyracore_shared::nav::OBS_DIM)
            .ok_or("repair wall off grid")?;
    let obstacle_y =
        lyracore_shared::nav::sub_index(priest.y, cell_y, lyracore_shared::nav::OBS_DIM)
            .ok_or("repair wall off grid")?;
    let key = lyracore_shared::terrain::cell_key(priest.map_id, cell_x, cell_y);
    let mut chunk = ctx
        .db
        .game_nav_chunk()
        .key()
        .find(key)
        .ok_or("repair wall needs the declared flat navigation")?;
    if chunk.walk.len() != lyracore_shared::nav::WALK_BYTES {
        return Err("repair wall found an invalid walkability grid".to_string());
    }
    if chunk.obs.is_empty() {
        chunk.obs = vec![lyracore_shared::nav::OBS_NONE; lyracore_shared::nav::OBS_BYTES];
    } else if chunk.obs.len() != lyracore_shared::nav::OBS_BYTES {
        return Err("repair wall found an invalid obstruction grid".to_string());
    }
    for sub_y in walk_y.saturating_sub(2)..=(walk_y + 2).min(lyracore_shared::nav::WALK_DIM - 1) {
        lyracore_shared::nav::walk_set(&mut chunk.walk, walk_sub, sub_y, false);
    }
    let base_z = chunk.base_z;
    for sub_y in
        obstacle_y.saturating_sub(4)..=(obstacle_y + 4).min(lyracore_shared::nav::OBS_DIM - 1)
    {
        lyracore_shared::nav::obs_raise(
            &mut chunk.obs,
            base_z,
            obstacle_sub,
            sub_y,
            priest.z + 4.0,
        );
    }
    ctx.db.game_nav_chunk().key().update(chunk);
    if crate::nav::has_los(
        ctx,
        priest.map_id,
        priest.instance_id,
        (priest.x, priest.y, priest.z),
        (priest.x + 100.0, priest.y, priest.z),
    ) {
        return Err("companion acceptance repair wall did not block line of sight".to_string());
    }
    Ok(())
}

/// Build the one human and four-bot party and all route/fault inputs before the measured clock.
#[reducer]
#[allow(clippy::too_many_arguments)]
pub fn playerbots_companion_acceptance_stage(
    ctx: &ReducerContext,
    leader_guid: u64,
    warrior_guid: u64,
    priest_guid: u64,
    mage_one_guid: u64,
    mage_two_guid: u64,
    reward_quest: u32,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    if ctx.db.pkg_playerbots_companion_acceptance().count() != 0
        || ctx.db.pkg_playerbots_companion_fault().count() != 0
        || ctx.db.pkg_playerbots_companion_combat_receipt().count() != 0
        || ctx.db.pkg_playerbots_companion_cast_receipt().count() != 0
    {
        return Err("companion acceptance fixture is occupied".to_string());
    }
    let expected = [
        leader_guid,
        warrior_guid,
        priest_guid,
        mage_one_guid,
        mage_two_guid,
    ];
    if expected.into_iter().collect::<BTreeSet<_>>().len() != 5 || expected.contains(&0) {
        return Err("companion acceptance requires five distinct nonzero Characters".to_string());
    }
    let schedules = ctx.db.game_creature_move_schedule();
    let ordinary_tick = schedules.iter().take(2).collect::<Vec<_>>();
    if ordinary_tick.len() != 1 || ordinary_tick[0].instance_id != u64::MAX {
        return Err("companion acceptance requires one recurring global movement tick".to_string());
    }
    let interval_micros = match &ordinary_tick[0].scheduled_at {
        ScheduleAt::Interval(duration) => duration.to_micros(),
        ScheduleAt::Time(_) => {
            return Err(
                "companion acceptance requires one recurring global movement tick".to_string(),
            );
        }
    };
    let ordinary_tick = ordinary_tick.into_iter().next().unwrap();
    let ordinary_tick_id = ordinary_tick.scheduled_id;
    super::transfer_orders_fixture::playerbots_transfer_orders_stage(
        ctx,
        warrior_guid,
        priest_guid,
        mage_one_guid,
        mage_two_guid,
        leader_guid,
    )?;
    let staged_tick = schedules
        .scheduled_id()
        .find(ordinary_tick_id)
        .ok_or("companion acceptance order staging replaced the global movement tick")?;
    if staged_tick.instance_id != ordinary_tick.instance_id {
        return Err(
            "companion acceptance order staging changed the global movement tick".to_string(),
        );
    }
    schedules.scheduled_id().update(ordinary_tick);
    if schedules
        .scheduled_id()
        .find(ordinary_tick_id)
        .is_none_or(|tick| {
            tick.instance_id != u64::MAX
                || !matches!(
                    tick.scheduled_at,
                    ScheduleAt::Interval(duration) if duration.to_micros() == interval_micros
                )
        })
    {
        return Err("companion acceptance did not preserve the movement cadence".to_string());
    }
    declare_exit_route(ctx)?;
    verify_bot(ctx, warrior_guid, super::class::WARRIOR, super::ROLE_TANK)?;
    verify_bot(ctx, priest_guid, super::class::PRIEST, super::ROLE_HEALER)?;
    verify_bot(ctx, mage_one_guid, super::class::MAGE, super::ROLE_DPS)?;
    verify_bot(ctx, mage_two_guid, super::class::MAGE, super::ROLE_DPS)?;
    stage_repair_wall(ctx, priest_guid, mage_one_guid)?;
    retain_staged_positions(ctx, expected)?;
    let actual = exact_member_guids(ctx)?;
    let mut expected = expected.to_vec();
    expected.sort_unstable();
    if actual != expected {
        return Err("companion acceptance party membership changed during staging".to_string());
    }
    let mut enemies = Vec::with_capacity(3);
    for entry in 5_098_001u32..=5_098_003 {
        let guid = (0xF130u64 << 48) | (u64::from(entry) << 24) | 1;
        let enemy = crate::helpers::live_entity(ctx, guid)?;
        if enemy.entry != entry {
            return Err("companion acceptance pull target identity changed".to_string());
        }
        let mut spawn = ctx
            .db
            .game_creature_spawn()
            .guid()
            .find(guid)
            .filter(|spawn| spawn.entry == entry)
            .ok_or("companion acceptance pull target spawn changed")?;
        spawn.map_id = enemy.map_id;
        (spawn.x, spawn.y, spawn.z) = (enemy.x, enemy.y, enemy.z);
        spawn.orientation = enemy.orientation;
        ctx.db.game_creature_spawn().guid().update(spawn);
        enemies.push(guid);
    }
    ctx.db
        .pkg_playerbots_companion_acceptance()
        .insert(CompanionAcceptance {
            id: 0,
            leader_guid,
            warrior_guid,
            priest_guid,
            mage_one_guid,
            mage_two_guid,
            enemy_guids: enemies.clone(),
            reward_quest,
            staged_micros: ctx.timestamp.to_micros_since_unix_epoch(),
            begun_micros: 0,
        });
    let faults = ctx.db.pkg_playerbots_companion_fault();
    for (id, (kind, due_offset_micros)) in EXPECTED_FAULTS.into_iter().enumerate() {
        let (subject_guid, target_guid) = match kind {
            FAULT_WOUND => (priest_guid, mage_one_guid),
            FAULT_CONTROL | FAULT_CLEAR_CONTROL => (leader_guid, enemies[1]),
            FAULT_DEATH => (mage_two_guid, enemies[2]),
            _ => unreachable!(),
        };
        faults.insert(CompanionFault {
            id: id as u8,
            kind,
            subject_guid,
            target_guid,
            due_offset_micros,
            applied_micros: 0,
            death_dead: false,
            death_was_ghost: false,
            death_health: 0,
            death_player_flags: 0,
            death_x: 0.0,
            death_y: 0.0,
            death_z: 0.0,
            ghost_observed_micros: 0,
            ghost_x: 0.0,
            ghost_y: 0.0,
            ghost_z: 0.0,
            alive_observed_micros: 0,
            alive_x: 0.0,
            alive_y: 0.0,
            alive_z: 0.0,
        });
    }
    Ok(())
}

/// Start the route only after ordinary provisioning and one real Quest reward are durable.
#[reducer]
pub fn playerbots_companion_acceptance_begin(ctx: &ReducerContext) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let mut plan = plan(ctx)?;
    if plan.begun_micros != 0 {
        return Err("companion acceptance route already began".to_string());
    }
    let bots = [
        plan.warrior_guid,
        plan.priest_guid,
        plan.mage_one_guid,
        plan.mage_two_guid,
    ];
    for guid in bots {
        let provisioning = ctx
            .db
            .pkg_playerbots_provisioning()
            .character_guid()
            .find(guid)
            .filter(|row| row.action_cursor > 0 && row.free_grants)
            .ok_or_else(|| format!("bot {guid} has no ordinary provisioning receipt"))?;
        let _ = provisioning;
        if !ctx
            .db
            .game_item_instance()
            .by_owner_guid()
            .filter(guid)
            .any(|item| item.slot <= 18)
        {
            return Err(format!("bot {guid} has no equipped item before begin"));
        }
        if ctx
            .db
            .pkg_playerbots_bot()
            .by_character()
            .filter(guid)
            .next()
            .is_none_or(|bot| bot.controller != Controller::Cohort)
        {
            return Err(format!(
                "bot {guid} lost its Cohort controller before begin"
            ));
        }
    }
    if ctx
        .db
        .game_character_quest()
        .by_character_quest()
        .filter((plan.warrior_guid, plan.reward_quest))
        .next()
        .is_none_or(|quest| !quest.rewarded || quest.failed)
    {
        return Err("the declared ordinary Quest reward is not durable before begin".to_string());
    }
    if ctx
        .db
        .game_character_quest()
        .by_character_quest()
        .filter((plan.warrior_guid, RETAINED_QUEST))
        .next()
        .is_none_or(|quest| quest.rewarded || quest.failed)
    {
        return Err("the accepted retained Quest is not active before begin".to_string());
    }
    plan.begun_micros = ctx.timestamp.to_micros_since_unix_epoch();
    ctx.db
        .pkg_playerbots_companion_acceptance()
        .id()
        .update(plan);
    let bots_table = ctx.db.pkg_playerbots_bot();
    for guid in bots {
        let mut bot = bots_table
            .by_character()
            .filter(guid)
            .next()
            .ok_or("companion acceptance bot vanished before begin")?;
        bot.next_think_micros = 0;
        bots_table.id().update(bot);
    }
    Ok(())
}

/// Consume one timed fault exactly once. The plan, subject, and target were fixed before begin.
#[reducer]
pub fn playerbots_companion_acceptance_apply_fault(
    ctx: &ReducerContext,
    fault_id: u8,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let plan = plan(ctx)?;
    if plan.begun_micros == 0 {
        return Err("companion acceptance route has not begun".to_string());
    }
    let faults = ctx.db.pkg_playerbots_companion_fault();
    let mut fault = faults
        .id()
        .find(fault_id)
        .ok_or("companion acceptance fault is not declared")?;
    if fault.applied_micros != 0 {
        return Err("companion acceptance fault already applied".to_string());
    }
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    let due = plan
        .begun_micros
        .checked_add(fault.due_offset_micros)
        .ok_or("companion acceptance fault deadline exhausted")?;
    if now < due {
        return Err(format!("companion acceptance fault is not due until {due}"));
    }
    match fault.kind {
        FAULT_WOUND => {
            wound_without_moving(ctx, plan.leader_guid)?;
            wound_without_moving(ctx, fault.target_guid)?;
            crate::spell::do_cancel_aura(ctx, plan.leader_guid, 1243)?;
            crate::spell::do_cancel_aura(ctx, fault.target_guid, 1243)?;
        }
        FAULT_CONTROL => super::fixture::playerbots_fixture_roles_control(
            ctx,
            fault.subject_guid,
            fault.target_guid,
            CONTROL_SPELL,
        )?,
        FAULT_DEATH => {
            let victim = crate::helpers::live_entity(ctx, fault.subject_guid)?;
            let damage = crate::combat::final_damage(ctx, fault.subject_guid, victim.health);
            let outcome = crate::combat::apply_hit(
                ctx,
                fault.target_guid,
                fault.subject_guid,
                damage,
                crate::combat::Hit::weapon(crate::combat::HitSource::MainHand, false),
            );
            if !outcome.killed {
                return Err("declared companion fault was not lethal".to_string());
            }
            let dead = crate::helpers::live_entity(ctx, fault.subject_guid)?;
            if !dead.dead
                || dead.health != 0
                || dead.player_flags & lyracore_shared::constants::player_flags::GHOST != 0
            {
                return Err("declared companion fault did not retain the lethal state".to_string());
            }
            fault.death_health = dead.health;
            fault.death_dead = dead.dead;
            fault.death_was_ghost =
                dead.player_flags & lyracore_shared::constants::player_flags::GHOST != 0;
            fault.death_player_flags = dead.player_flags;
            (fault.death_x, fault.death_y, fault.death_z) = (dead.x, dead.y, dead.z);
        }
        FAULT_CLEAR_CONTROL => super::fixture::playerbots_fixture_roles_clear_control(
            ctx,
            fault.subject_guid,
            fault.target_guid,
        )?,
        _ => return Err("unknown companion acceptance fault".to_string()),
    }
    fault.applied_micros = now;
    faults.id().update(fault);
    Ok(())
}
