#![cfg(feature = "debug_reducers")]

//! Resolved nuisance inputs for private acceptance journeys.

use super::quest_catalog::{
    pkg_playerbots_catalog_objective, pkg_playerbots_catalog_quest, pkg_playerbots_quest_admission,
    pkg_playerbots_quest_catalog, pkg_playerbots_quest_objective, AdmissionRefusal,
    CatalogDestination, CatalogEntityKind, CatalogObjectiveKind, MissingCapability,
    ObjectiveExecutor, CATALOG_BLUEPRINT_REVISION, CATALOG_NAME, CATALOG_REVISION,
};
use super::quest_catalog_fixture::{
    pkg_playerbots_quest_fixture_ownership, pkg_playerbots_quest_loop_fixture,
    pkg_playerbots_quest_turnin_fixture, pkg_playerbots_seeded_quest_fixture,
};
use super::{
    pkg_playerbots_bot, pkg_playerbots_runner, ActionKind, ActionOutcome, Controller, Failure,
    ObjectiveKind, PlayerbotsBot, RunnerOutcome,
};
use crate::nav::{game_nav_chunk, game_navigation_revision};
use crate::{
    game_character_quest, game_corpse_loot, game_creature_spawn, game_creature_template,
    game_graveyard, game_item_instance, game_quest_template, game_world_entity,
};
use spacetimedb::{reducer, table, ReducerContext, Table, Timestamp};

const PLAN_REVISION: &str = "playerbots-acceptance-seed-plan-v1";
const COMBAT_FAULT_QUEST: u32 = 7;
const INVENTORY_FAULT_QUEST: u32 = 33;
const INVENTORY_FAULT_ITEM: u32 = 750;
const INVENTORY_FAULT_FILLER: u32 = 5_099_400;
const INVENTORY_FAULT_RELEASE_DELAY_MICROS: i64 = 500_000;
const MISSING_TARGET_FAULT_QUEST: u32 = 18;
const MISSING_TARGET_FAULT_ENTRY: u32 = 38;
const MISSING_TARGET_RESPAWN_DELAY_MICROS: i64 = 60_000_000;
const UNREACHABLE_GIVER_QUEST: u32 = 783;
const UNREACHABLE_GIVER_ENTRY: u32 = 823;
const UNREACHABLE_ALTERNATIVE_QUEST: u32 = 40;
const ADMISSION_FAULT_QUEST: u32 = 7;
const ADMISSION_PREREQUISITE: u32 = 783;
const ADMISSION_SUPPORTED_ALTERNATIVE: u32 = 5261;
const ADMISSION_FAULT_RESTORE_DELAY_MICROS: i64 = 500_000;
const LETHAL_FAULT_DELAY_MICROS: i64 = 1_000_000;
const LEVEL_GAP_QUEST: u32 = 40;
const LEVEL_GAP_REQUIRED_LEVEL: u32 = 7;
const LEVEL_GAP_SOURCE_ENTRY: u32 = 5_099_410;
const LEVEL_GAP_SOURCE_COUNT: u64 = 66;
const LEVEL_GAP_LONG_SOURCE_COUNT: u64 = 3;
const JOURNEY_GRAVEYARD_ID: u32 = 105;

const OFFSETS: [(f32, f32); 10] = [
    (-0.75, -0.75),
    (-0.25, -0.75),
    (0.25, -0.75),
    (0.75, -0.75),
    (-0.50, -0.25),
    (0.00, -0.25),
    (0.50, 0.25),
    (-0.75, 0.75),
    (0.25, 0.75),
    (0.75, 0.75),
];

#[table(accessor = pkg_playerbots_acceptance_seed_plan, public)]
pub struct SeedPlan {
    #[primary_key]
    pub seed: u64,
    pub class: u8,
    pub role: u8,
    pub suffix: u8,
    pub start_offset_x: f32,
    pub start_offset_y: f32,
    pub initial_due_phase_micros: i64,
    pub source_insertion_rotation: u8,
    pub plan_revision: String,
    pub catalog_name: String,
    pub catalog_revision: u64,
    pub catalog_blueprint_revision: String,
}

#[table(accessor = pkg_playerbots_acceptance_journey, public)]
pub struct AcceptanceJourney {
    #[primary_key]
    pub character_guid: u64,
    pub seed: u64,
    pub class: u8,
    pub role: u8,
    pub start_map: u32,
    pub start_instance: u64,
    pub start_x: f32,
    pub start_y: f32,
    pub start_z: f32,
    pub initial_due_phase_micros: i64,
    pub journey_started_micros: Option<i64>,
    pub first_due_micros: Option<i64>,
    pub source_insertion_rotation: u8,
    pub source_insertion_guids: Vec<u64>,
    pub plan_revision: String,
    pub catalog_name: String,
    pub catalog_revision: u64,
    pub catalog_blueprint_revision: String,
    pub catalog_content_revision: String,
    pub named_quest_entry: u32,
    pub named_target_entry: u32,
    pub named_target_count: u32,
    pub named_content_revision: String,
    pub simple_quest_entry: u32,
    pub simple_gameobject_entry: u32,
    pub simple_content_revision: String,
    pub fixture_ownership_revision: String,
    pub staged_micros: i64,
    pub graveyard_id: u32,
    pub graveyard_x: f32,
    pub graveyard_y: f32,
    pub graveyard_z: f32,
}

#[table(accessor = pkg_playerbots_acceptance_gameobject_use_receipt, public)]
pub struct AcceptanceGameObjectUseReceipt {
    #[primary_key]
    pub character_guid: u64,
    pub character_quest_id: u64,
    pub runner_objective_identity: u64,
    pub quest_entry: u32,
    pub gameobject_guid: u64,
    pub gameobject_entry: u32,
    pub instance_id: u64,
    pub use_count: u32,
    pub first_observed_micros: i64,
}

#[table(accessor = pkg_playerbots_acceptance_level_gap, public)]
pub struct AcceptanceLevelGap {
    #[primary_key]
    pub character_guid: u64,
    pub quest_entry: u32,
    pub required_level: u32,
    pub initial_level: u32,
    pub initial_xp: u32,
    pub source_entry: u32,
    pub source_guids: Vec<u64>,
    pub long_source_guids: Vec<u64>,
    pub source_requirement_revision: String,
    pub imported_content_revision: Option<String>,
    pub staged_micros: i64,
    pub started_micros: Option<i64>,
    pub observed_level: u32,
    pub observed_xp: u32,
    pub kill_count: u32,
    pub accepted_level: Option<u32>,
    pub accepted_xp: Option<u32>,
    pub accepted_micros: Option<i64>,
}

crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_acceptance_level_gap(ctx, character_guid) {
    ctx.db
        .pkg_playerbots_acceptance_level_gap()
        .character_guid()
        .delete(character_guid);
});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_playerbots_acceptance_level_gap());

#[table(
    accessor = pkg_playerbots_acceptance_level_gap_kill,
    public,
    index(accessor = by_character, btree(columns = [character_guid]))
)]
pub struct AcceptanceLevelGapKill {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub victim_guid: u64,
    pub character_guid: u64,
    pub victim_entry: u32,
    pub ordinal: u32,
    pub level_before: u32,
    pub xp_before: u32,
    pub level_after: u32,
    pub xp_after: u32,
    pub killed_micros: i64,
}

crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_acceptance_level_gap_kill(ctx, character_guid) {
    for row in ctx
        .db
        .pkg_playerbots_acceptance_level_gap_kill()
        .by_character()
        .filter(character_guid)
        .collect::<Vec<_>>()
    {
        ctx.db
            .pkg_playerbots_acceptance_level_gap_kill()
            .id()
            .delete(row.id);
    }
});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_playerbots_acceptance_level_gap_kill());

#[table(accessor = pkg_playerbots_acceptance_combat_fault, public)]
pub struct AcceptanceCombatFault {
    #[primary_key]
    pub character_guid: u64,
    pub quest_entry: u32,
    pub staged_micros: i64,
    pub lethal_delay_micros: i64,
    pub incoming_attempted_micros: Option<i64>,
    pub incoming_applied_micros: Option<i64>,
    pub incoming_attacker_guid: Option<u64>,
    pub incoming_health_before: Option<u32>,
    pub incoming_health_after: Option<u32>,
    pub incoming_credit_before: Option<u32>,
    pub incoming_credit_after: Option<u32>,
    pub incoming_objective_identity_before: Option<u64>,
    pub incoming_objective_identity_after: Option<u64>,
    pub lethal_attempted_micros: Option<i64>,
    pub lethal_applied_micros: Option<i64>,
    pub lethal_attacker_guid: Option<u64>,
    pub lethal_health_before: Option<u32>,
    pub lethal_health_after: Option<u32>,
    pub lethal_credit_before: Option<u32>,
    pub lethal_credit_after: Option<u32>,
    pub lethal_objective_identity_before: Option<u64>,
    pub lethal_objective_identity_after: Option<u64>,
    pub resurrected_micros: Option<i64>,
    pub resurrected_health: Option<u32>,
    pub resurrected_credit: Option<u32>,
    pub resurrected_objective_identity: Option<u64>,
    pub resurrected_map: Option<u32>,
    pub resurrected_instance: Option<u64>,
    pub resurrected_x: Option<f32>,
    pub resurrected_y: Option<f32>,
    pub resurrected_z: Option<f32>,
    pub incoming_purpose: Option<ObjectiveKind>,
    pub incoming_destination: Option<CatalogDestination>,
    pub lethal_purpose: Option<ObjectiveKind>,
    pub lethal_destination: Option<CatalogDestination>,
    pub resurrected_purpose: Option<ObjectiveKind>,
    pub resurrected_destination: Option<CatalogDestination>,
}

#[table(accessor = pkg_playerbots_acceptance_inventory_fault, public)]
pub struct AcceptanceInventoryFault {
    #[primary_key]
    pub character_guid: u64,
    pub quest_entry: u32,
    pub item_entry: u32,
    pub staged_micros: i64,
    pub fill_attempted_micros: Option<i64>,
    pub fill_applied_micros: Option<i64>,
    pub fill_error: Option<String>,
    pub item_count_before_fill: Option<u32>,
    pub credit_before_fill: Option<u32>,
    pub objective_identity_before_fill: Option<u64>,
    pub refused_micros: Option<i64>,
    pub refused_action_observed_micros: Option<i64>,
    pub refused_source_guid: Option<u64>,
    pub refused_loot_slot: Option<u8>,
    pub item_count_at_refusal: Option<u32>,
    pub credit_at_refusal: Option<u32>,
    pub objective_identity_at_refusal: Option<u64>,
    pub retry_count_at_refusal: Option<u8>,
    pub retry_observed_micros: Option<i64>,
    pub retry_after_micros: Option<i64>,
    pub release_delay_micros: i64,
    pub space_release_due_micros: Option<i64>,
    pub space_release_attempted_micros: Option<i64>,
    pub space_released_micros: Option<i64>,
    pub space_release_error: Option<String>,
    pub resumed_micros: Option<i64>,
    pub resumed_action_observed_micros: Option<i64>,
    pub resumed_source_guid: Option<u64>,
    pub resumed_item_count: Option<u32>,
    pub resumed_credit: Option<u32>,
    pub resumed_objective_identity: Option<u64>,
    pub purpose_before_fill: Option<ObjectiveKind>,
    pub destination_before_fill: Option<CatalogDestination>,
    pub purpose_at_refusal: Option<ObjectiveKind>,
    pub destination_at_refusal: Option<CatalogDestination>,
    pub resumed_purpose: Option<ObjectiveKind>,
    pub resumed_destination: Option<CatalogDestination>,
}

#[table(accessor = pkg_playerbots_acceptance_missing_target_fault, public)]
pub struct AcceptanceMissingTargetFault {
    #[primary_key]
    pub character_guid: u64,
    pub quest_entry: u32,
    pub target_entry: u32,
    pub target_guid: u64,
    pub staged_micros: i64,
    pub respawn_delay_micros: i64,
    pub removal_attempted_micros: Option<i64>,
    pub removal_applied_micros: Option<i64>,
    pub removal_error: Option<String>,
    pub removed_life_seq: Option<u64>,
    pub credit_before_removal: Option<u32>,
    pub objective_identity_before_removal: Option<u64>,
    pub respawn_due_micros: Option<i64>,
    pub missing_observed_micros: Option<i64>,
    pub missing_failure: Option<Failure>,
    pub missing_retry_count: Option<u8>,
    pub missing_retry_after_micros: Option<i64>,
    pub attack_target_at_missing: Option<u64>,
    pub attack_observed_micros_at_missing: Option<i64>,
    pub credit_at_missing: Option<u32>,
    pub objective_identity_at_missing: Option<u64>,
    pub respawned_micros: Option<i64>,
    pub respawned_life_seq: Option<u64>,
    pub respawned_health: Option<u32>,
    pub resumed_micros: Option<i64>,
    pub resumed_attack_target: Option<u64>,
    pub resumed_credit: Option<u32>,
    pub resumed_objective_identity: Option<u64>,
    pub removal_character_x: Option<f32>,
    pub removal_character_y: Option<f32>,
    pub removal_character_z: Option<f32>,
    pub removal_target_x: Option<f32>,
    pub removal_target_y: Option<f32>,
    pub removal_target_z: Option<f32>,
    pub removal_target_distance: Option<f32>,
    pub purpose_before_removal: Option<ObjectiveKind>,
    pub destination_before_removal: Option<CatalogDestination>,
    pub purpose_at_missing: Option<ObjectiveKind>,
    pub destination_at_missing: Option<CatalogDestination>,
    pub resumed_purpose: Option<ObjectiveKind>,
    pub resumed_destination: Option<CatalogDestination>,
}

#[table(accessor = pkg_playerbots_acceptance_unreachable_giver_fault, public)]
pub struct AcceptanceUnreachableGiverFault {
    #[primary_key]
    pub character_guid: u64,
    pub quest_entry: u32,
    pub giver_entry: u32,
    pub giver_guid: u64,
    pub alternative_quest_entry: u32,
    pub blocked_navigation_keys: Vec<u64>,
    pub staged_micros: i64,
    pub blocked_navigation_revision: u64,
    pub maximum_approach: u8,
    pub deferred_micros: Option<i64>,
    pub deferred_until_micros: Option<i64>,
    pub quest_id_at_deferral: Option<u64>,
    pub credit_at_deferral: Option<u32>,
    pub alternative_micros: Option<i64>,
    pub alternative_objective_identity: Option<u64>,
    pub alternative_target_guid: Option<u64>,
    pub alternative_action_observed_micros: Option<i64>,
    pub restore_due_micros: Option<i64>,
    pub restore_attempted_micros: Option<i64>,
    pub restored_micros: Option<i64>,
    pub restored_navigation_revision: Option<u64>,
    pub restore_error: Option<String>,
    pub resumed_micros: Option<i64>,
    pub resumed_objective_identity: Option<u64>,
    pub quest_id_at_resume: Option<u64>,
    pub credit_at_resume: Option<u32>,
}

#[table(accessor = pkg_playerbots_acceptance_admission_fault, public)]
pub struct AcceptanceAdmissionFault {
    #[primary_key]
    pub character_guid: u64,
    pub quest_entry: u32,
    pub prerequisite_quest_entry: u32,
    pub supported_alternative_quest_entry: u32,
    pub staged_micros: i64,
    pub prerequisite_refused_micros: Option<i64>,
    pub prerequisite_refusal_detail: Option<String>,
    pub quest_id_at_prerequisite_refusal: Option<u64>,
    pub prerequisite_rewarded_micros: Option<i64>,
    pub prerequisite_turnin_count: Option<u32>,
    pub unsupported_observed_micros: Option<i64>,
    pub unsupported_capability: Option<MissingCapability>,
    pub selected_alternative_quest: Option<u32>,
    pub quest_id_at_unsupported_refusal: Option<u64>,
    pub alternative_accept_observed_micros: Option<i64>,
    pub alternative_accept_target_guid: Option<u64>,
    pub restore_due_micros: Option<i64>,
    pub restore_attempted_micros: Option<i64>,
    pub restored_micros: Option<i64>,
    pub restore_error: Option<String>,
    pub resumed_micros: Option<i64>,
    pub resumed_accept_observed_micros: Option<i64>,
    pub resumed_accept_target_guid: Option<u64>,
    pub resumed_quest_id: Option<u64>,
    pub resumed_credit: Option<u32>,
}

crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_acceptance_journey(ctx, character_guid) {
    ctx.db
        .pkg_playerbots_acceptance_journey()
        .character_guid()
        .delete(character_guid);
});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_playerbots_acceptance_journey());

crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_acceptance_gameobject_use_receipt(ctx, character_guid) {
    ctx.db
        .pkg_playerbots_acceptance_gameobject_use_receipt()
        .character_guid()
        .delete(character_guid);
});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_playerbots_acceptance_gameobject_use_receipt());

crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_acceptance_combat_fault(ctx, character_guid) {
    ctx.db
        .pkg_playerbots_acceptance_combat_fault()
        .character_guid()
        .delete(character_guid);
});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_playerbots_acceptance_combat_fault());

crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_acceptance_inventory_fault(ctx, character_guid) {
    ctx.db
        .pkg_playerbots_acceptance_inventory_fault()
        .character_guid()
        .delete(character_guid);
});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_playerbots_acceptance_inventory_fault());

crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_acceptance_missing_target_fault(ctx, character_guid) {
    ctx.db
        .pkg_playerbots_acceptance_missing_target_fault()
        .character_guid()
        .delete(character_guid);
});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_playerbots_acceptance_missing_target_fault());

crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_acceptance_unreachable_giver_fault(ctx, character_guid) {
    ctx.db
        .pkg_playerbots_acceptance_unreachable_giver_fault()
        .character_guid()
        .delete(character_guid);
});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_playerbots_acceptance_unreachable_giver_fault());

crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_acceptance_admission_fault(ctx, character_guid) {
    ctx.db
        .pkg_playerbots_acceptance_admission_fault()
        .character_guid()
        .delete(character_guid);
});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_playerbots_acceptance_admission_fault());

fn fixture_creature_guid(entry: u32) -> u64 {
    (0xF130u64 << 48) | (u64::from(entry) << 24) | 1
}

fn retained_quest_identity(
    ctx: &ReducerContext,
    character_guid: u64,
    quest_entry: u32,
) -> Option<(u32, u64)> {
    let mut quests = ctx
        .db
        .game_character_quest()
        .by_character_quest()
        .filter((character_guid, quest_entry))
        .take(2);
    let quest = quests.next()?;
    if quests.next().is_some() || quest.rewarded || quest.failed {
        return None;
    }
    let retained = ctx
        .db
        .pkg_playerbots_quest_objective()
        .character_guid()
        .find(character_guid)?;
    if retained.quest_entry != quest_entry {
        return None;
    }
    Some((
        quest.counts.first().copied().unwrap_or(0),
        retained.runner_objective_identity,
    ))
}

struct AcceptanceQuestState {
    credit: u32,
    objective_identity: u64,
    purpose: ObjectiveKind,
    destination: CatalogDestination,
}

fn acceptance_quest_state(
    ctx: &ReducerContext,
    character_guid: u64,
    quest_entry: u32,
) -> Option<AcceptanceQuestState> {
    let (credit, objective_identity) = retained_quest_identity(ctx, character_guid, quest_entry)?;
    let retained = ctx
        .db
        .pkg_playerbots_quest_objective()
        .character_guid()
        .find(character_guid)?;
    let objective = ctx
        .db
        .pkg_playerbots_runner()
        .character_guid()
        .find(character_guid)?
        .objective?;
    (objective.identity == objective_identity).then_some(AcceptanceQuestState {
        credit,
        objective_identity,
        purpose: objective.kind,
        destination: retained.destination,
    })
}

fn live_fault_attacker(ctx: &ReducerContext, character_guid: u64) -> Option<u64> {
    let journey = ctx
        .db
        .pkg_playerbots_acceptance_journey()
        .character_guid()
        .find(character_guid)?;
    journey.source_insertion_guids.into_iter().find(|guid| {
        ctx.db
            .game_world_entity()
            .guid()
            .find(*guid)
            .is_some_and(|entity| !entity.dead)
    })
}

fn apply_incoming_fault(
    ctx: &ReducerContext,
    row: &mut AcceptanceCombatFault,
    now: i64,
    quest: AcceptanceQuestState,
) {
    let Some(attacker_guid) = live_fault_attacker(ctx, row.character_guid) else {
        return;
    };
    let Some(before) = ctx.db.game_world_entity().guid().find(row.character_guid) else {
        return;
    };
    if before.dead || before.health <= 1 {
        return;
    }
    row.incoming_attempted_micros = Some(now);
    row.incoming_attacker_guid = Some(attacker_guid);
    row.incoming_health_before = Some(before.health);
    row.incoming_credit_before = Some(quest.credit);
    row.incoming_objective_identity_before = Some(quest.objective_identity);
    row.incoming_purpose = Some(quest.purpose);
    row.incoming_destination = Some(quest.destination);
    let amount = before.health.saturating_sub(1).min(25);
    let accepted = crate::debug::debug_apply_damage(ctx, row.character_guid, amount, attacker_guid) // package-api: exempt private acceptance fixture applies real Core damage
        .is_ok();
    let Some(after) = ctx.db.game_world_entity().guid().find(row.character_guid) else {
        return;
    };
    row.incoming_health_after = Some(after.health);
    if let Some((after_credit, after_identity)) =
        retained_quest_identity(ctx, row.character_guid, COMBAT_FAULT_QUEST)
    {
        row.incoming_credit_after = Some(after_credit);
        row.incoming_objective_identity_after = Some(after_identity);
    }
    if accepted && !after.dead && after.health < before.health {
        row.incoming_applied_micros = Some(now);
    }
}

fn apply_lethal_fault(
    ctx: &ReducerContext,
    row: &mut AcceptanceCombatFault,
    now: i64,
    quest: AcceptanceQuestState,
) {
    let Some(attacker_guid) = live_fault_attacker(ctx, row.character_guid) else {
        return;
    };
    let Some(before) = ctx.db.game_world_entity().guid().find(row.character_guid) else {
        return;
    };
    if before.dead {
        return;
    }
    row.lethal_attempted_micros = Some(now);
    row.lethal_attacker_guid = Some(attacker_guid);
    row.lethal_health_before = Some(before.health);
    row.lethal_credit_before = Some(quest.credit);
    row.lethal_objective_identity_before = Some(quest.objective_identity);
    row.lethal_purpose = Some(quest.purpose);
    row.lethal_destination = Some(quest.destination);
    let (amount, _) =
        crate::combat::fold_incoming_damage(ctx, attacker_guid, row.character_guid, 10_000);
    let damage = crate::combat::final_damage(ctx, row.character_guid, amount);
    let outcome = crate::combat::apply_hit(
        ctx,
        attacker_guid,
        row.character_guid,
        damage,
        crate::combat::Hit::weapon(crate::combat::HitSource::MainHand, false),
    );
    let Some(after) = ctx.db.game_world_entity().guid().find(row.character_guid) else {
        return;
    };
    row.lethal_health_after = Some(after.health);
    if let Some((after_credit, after_identity)) =
        retained_quest_identity(ctx, row.character_guid, COMBAT_FAULT_QUEST)
    {
        row.lethal_credit_after = Some(after_credit);
        row.lethal_objective_identity_after = Some(after_identity);
    };
    if outcome.killed && after.dead && after.health == 0 {
        row.lethal_applied_micros = Some(now);
    }
}

fn advance_combat_fault(ctx: &ReducerContext, mut row: AcceptanceCombatFault, now: i64) {
    if row.incoming_attempted_micros.is_none() {
        if let Some(quest) = acceptance_quest_state(ctx, row.character_guid, COMBAT_FAULT_QUEST) {
            apply_incoming_fault(ctx, &mut row, now, quest);
            ctx.db
                .pkg_playerbots_acceptance_combat_fault()
                .character_guid()
                .update(row);
        }
        return;
    }
    if row.lethal_attempted_micros.is_none() {
        let due = row
            .incoming_attempted_micros
            .unwrap_or(now)
            .saturating_add(row.lethal_delay_micros);
        if now >= due {
            if let Some(quest) = acceptance_quest_state(ctx, row.character_guid, COMBAT_FAULT_QUEST)
            {
                apply_lethal_fault(ctx, &mut row, now, quest);
                ctx.db
                    .pkg_playerbots_acceptance_combat_fault()
                    .character_guid()
                    .update(row);
            }
        }
        return;
    }
    if row.lethal_applied_micros.is_some() && row.resurrected_micros.is_none() {
        if let Some(entity) = ctx.db.game_world_entity().guid().find(row.character_guid) {
            if !entity.dead && entity.health > 0 {
                if let Some(quest) =
                    acceptance_quest_state(ctx, row.character_guid, COMBAT_FAULT_QUEST)
                {
                    row.resurrected_micros = Some(now);
                    row.resurrected_health = Some(entity.health);
                    row.resurrected_map = Some(entity.map_id);
                    row.resurrected_instance = Some(entity.instance_id);
                    row.resurrected_x = Some(entity.x);
                    row.resurrected_y = Some(entity.y);
                    row.resurrected_z = Some(entity.z);
                    row.resurrected_credit = Some(quest.credit);
                    row.resurrected_objective_identity = Some(quest.objective_identity);
                    row.resurrected_purpose = Some(quest.purpose);
                    row.resurrected_destination = Some(quest.destination);
                    ctx.db
                        .pkg_playerbots_acceptance_combat_fault()
                        .character_guid()
                        .update(row);
                }
            }
        }
    }
}

crate::game_tick_pass!(fn playerbots_acceptance_combat_fault_pass(ctx) {
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    let schedules: Vec<_> = ctx
        .db
        .pkg_playerbots_acceptance_combat_fault()
        .iter()
        .take(101)
        .collect();
    if schedules.len() > 100 {
        spacetimedb::log::error!("playerbots: acceptance combat fault schedule exceeds 100 rows");
        return;
    }
    for row in schedules {
        advance_combat_fault(ctx, row, now);
    }
});

fn fill_inventory_fault(
    ctx: &ReducerContext,
    row: &mut AcceptanceInventoryFault,
    now: i64,
    item_count: u32,
    quest: AcceptanceQuestState,
) {
    row.fill_attempted_micros = Some(now);
    row.item_count_before_fill = Some(item_count);
    row.credit_before_fill = Some(quest.credit);
    row.objective_identity_before_fill = Some(quest.objective_identity);
    row.purpose_before_fill = Some(quest.purpose);
    row.destination_before_fill = Some(quest.destination);
    for _ in 0..64 {
        if !crate::items::has_free_slot(ctx, row.character_guid) {
            row.fill_applied_micros = Some(now);
            return;
        }
        if let Err(error) =
            crate::items::grant_item(ctx, row.character_guid, INVENTORY_FAULT_FILLER, 1)
        {
            row.fill_error = Some(error);
            return;
        }
    }
    row.fill_error = Some("inventory fixture left a free slot".to_string());
}

fn record_inventory_refusal(
    ctx: &ReducerContext,
    row: &mut AcceptanceInventoryFault,
    now: i64,
) -> bool {
    let Some(action) = super::actions::observation(ctx, row.character_guid, ActionKind::TakeLoot)
    else {
        return false;
    };
    let ActionOutcome::Refused(refusal) = action.outcome else {
        return false;
    };
    if action.quest_entry != row.quest_entry
        || refusal.kind != crate::actor::ActionRefusalKind::InventoryFull
    {
        return false;
    }
    let Some(quest) = acceptance_quest_state(ctx, row.character_guid, row.quest_entry) else {
        return false;
    };
    row.refused_micros = Some(now);
    row.refused_action_observed_micros = Some(action.observed_micros);
    row.space_release_due_micros = Some(now.saturating_add(row.release_delay_micros));
    row.refused_source_guid = Some(action.target_guid);
    let loot: Vec<_> = ctx
        .db
        .game_corpse_loot()
        .by_corpse()
        .filter(action.target_guid)
        .filter(|loot| loot.item_entry == row.item_entry && !loot.withheld)
        .take(2)
        .collect();
    if loot.len() == 1 {
        row.refused_loot_slot = Some(loot[0].slot);
    }
    row.item_count_at_refusal = Some(crate::items::item_count(
        ctx,
        row.character_guid,
        row.item_entry,
    ));
    row.credit_at_refusal = Some(quest.credit);
    row.objective_identity_at_refusal = Some(quest.objective_identity);
    row.purpose_at_refusal = Some(quest.purpose);
    row.destination_at_refusal = Some(quest.destination);
    if let Some(runner) = ctx
        .db
        .pkg_playerbots_runner()
        .character_guid()
        .find(row.character_guid)
    {
        row.retry_count_at_refusal = Some(runner.retry_count);
        row.retry_observed_micros = Some(runner.observed_micros);
        row.retry_after_micros = Some(runner.next_eligible_micros);
    }
    true
}

fn release_inventory_space(ctx: &ReducerContext, row: &mut AcceptanceInventoryFault, now: i64) {
    row.space_release_attempted_micros = Some(now);
    let filler = ctx
        .db
        .game_item_instance()
        .by_owner_guid()
        .filter(row.character_guid)
        .filter(|item| item.entry == INVENTORY_FAULT_FILLER)
        .take(1)
        .next();
    let Some(filler) = filler else {
        row.space_release_error = Some("inventory filler missing".to_string());
        return;
    };
    ctx.db.game_item_instance().guid().delete(filler.guid);
    if crate::items::has_free_slot(ctx, row.character_guid) {
        row.space_released_micros = Some(now);
    } else {
        row.space_release_error = Some("one inventory slot did not become available".to_string());
    }
}

fn record_inventory_resume(
    ctx: &ReducerContext,
    row: &mut AcceptanceInventoryFault,
    now: i64,
) -> bool {
    let Some(action) = super::actions::observation(ctx, row.character_guid, ActionKind::TakeLoot)
    else {
        return false;
    };
    let item_count = crate::items::item_count(ctx, row.character_guid, row.item_entry);
    if action.quest_entry != row.quest_entry
        || !matches!(action.outcome, ActionOutcome::Completed)
        || item_count == 0
    {
        return false;
    }
    let Some(quest) = acceptance_quest_state(ctx, row.character_guid, row.quest_entry) else {
        return false;
    };
    row.resumed_micros = Some(now);
    row.resumed_action_observed_micros = Some(action.observed_micros);
    row.resumed_source_guid = Some(action.target_guid);
    row.resumed_item_count = Some(item_count);
    row.resumed_credit = Some(quest.credit);
    row.resumed_objective_identity = Some(quest.objective_identity);
    row.resumed_purpose = Some(quest.purpose);
    row.resumed_destination = Some(quest.destination);
    true
}

fn advance_inventory_fault(ctx: &ReducerContext, mut row: AcceptanceInventoryFault, now: i64) {
    if row.fill_attempted_micros.is_none() {
        let item_count = crate::items::item_count(ctx, row.character_guid, row.item_entry);
        if item_count == 0 {
            if let Some(quest) = acceptance_quest_state(ctx, row.character_guid, row.quest_entry) {
                fill_inventory_fault(ctx, &mut row, now, item_count, quest);
                ctx.db
                    .pkg_playerbots_acceptance_inventory_fault()
                    .character_guid()
                    .update(row);
            }
        }
        return;
    }
    if row.fill_applied_micros.is_none() || row.fill_error.is_some() {
        return;
    }
    if row.refused_micros.is_none() {
        if record_inventory_refusal(ctx, &mut row, now) {
            ctx.db
                .pkg_playerbots_acceptance_inventory_fault()
                .character_guid()
                .update(row);
        }
        return;
    }
    if row.space_release_attempted_micros.is_none()
        && row.space_release_due_micros.is_some_and(|due| now >= due)
    {
        release_inventory_space(ctx, &mut row, now);
        ctx.db
            .pkg_playerbots_acceptance_inventory_fault()
            .character_guid()
            .update(row);
        return;
    }
    if row.space_released_micros.is_some()
        && row.space_release_error.is_none()
        && row.resumed_micros.is_none()
        && record_inventory_resume(ctx, &mut row, now)
    {
        ctx.db
            .pkg_playerbots_acceptance_inventory_fault()
            .character_guid()
            .update(row);
    }
}

crate::game_tick_pass!(fn playerbots_acceptance_inventory_fault_pass(ctx) {
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    let schedules: Vec<_> = ctx
        .db
        .pkg_playerbots_acceptance_inventory_fault()
        .iter()
        .take(101)
        .collect();
    if schedules.len() > 100 {
        spacetimedb::log::error!("playerbots: acceptance inventory fault schedule exceeds 100 rows");
        return;
    }
    for row in schedules {
        advance_inventory_fault(ctx, row, now);
    }
});

fn arm_missing_target_fault(
    ctx: &ReducerContext,
    row: &mut AcceptanceMissingTargetFault,
    now: i64,
    quest: AcceptanceQuestState,
) {
    row.removal_attempted_micros = Some(now);
    row.credit_before_removal = Some(quest.credit);
    row.objective_identity_before_removal = Some(quest.objective_identity);
    row.purpose_before_removal = Some(quest.purpose);
    row.destination_before_removal = Some(quest.destination);
    let mut sources = ctx
        .db
        .game_world_entity()
        .by_entry()
        .filter(row.target_entry)
        .take(2);
    let Some(source) = sources.next() else {
        row.removal_error = Some("missing-target fixture source is absent".to_string());
        return;
    };
    if sources.next().is_some() || source.guid != row.target_guid || source.dead {
        row.removal_error = Some("missing-target fixture source set differs".to_string());
        return;
    }
    let Some(character) = ctx.db.game_world_entity().guid().find(row.character_guid) else {
        row.removal_error = Some("missing-target fixture Character is absent".to_string());
        return;
    };
    if (character.map_id, character.instance_id) != (source.map_id, source.instance_id) {
        row.removal_error =
            Some("missing-target fixture source is on another partition".to_string());
        return;
    }
    row.removal_character_x = Some(character.x);
    row.removal_character_y = Some(character.y);
    row.removal_character_z = Some(character.z);
    row.removal_target_x = Some(source.x);
    row.removal_target_y = Some(source.y);
    row.removal_target_z = Some(source.z);
    row.removal_target_distance = Some(
        ((character.x - source.x).powi(2)
            + (character.y - source.y).powi(2)
            + (character.z - source.z).powi(2))
        .sqrt(),
    );
    let spawns = ctx.db.game_creature_spawn();
    let Some(mut spawn) = spawns.guid().find(row.target_guid) else {
        row.removal_error = Some("missing-target fixture spawn is absent".to_string());
        return;
    };
    row.removed_life_seq = Some(spawn.life_seq);
    let respawn_due = now.saturating_add(row.respawn_delay_micros);
    row.respawn_due_micros = Some(respawn_due);
    spawn.respawn_at = Timestamp::from_micros_since_unix_epoch(respawn_due);
    spawns.guid().update(spawn);
    crate::creatures::despawn_creature_entity(ctx, row.target_guid);
    if ctx
        .db
        .game_world_entity()
        .guid()
        .find(row.target_guid)
        .is_none()
    {
        row.removal_applied_micros = Some(now);
    } else {
        row.removal_error = Some("missing-target fixture source remained live".to_string());
    }
}

fn record_missing_target_wait(
    ctx: &ReducerContext,
    row: &mut AcceptanceMissingTargetFault,
) -> bool {
    let Some(runner) = ctx
        .db
        .pkg_playerbots_runner()
        .character_guid()
        .find(row.character_guid)
    else {
        return false;
    };
    if !matches!(
        runner.last_outcome,
        RunnerOutcome::Refused(Failure::QuestTargetMissing)
    ) || runner.observed_micros < row.removal_applied_micros.unwrap_or(i64::MAX)
    {
        return false;
    }
    let Some(quest) = acceptance_quest_state(ctx, row.character_guid, row.quest_entry) else {
        return false;
    };
    row.missing_observed_micros = Some(runner.observed_micros);
    row.missing_failure = Some(Failure::QuestTargetMissing);
    row.missing_retry_count = Some(runner.retry_count);
    row.missing_retry_after_micros = Some(runner.next_eligible_micros);
    if let Some(action) = super::actions::observation(ctx, row.character_guid, ActionKind::Attack) {
        row.attack_target_at_missing = Some(action.target_guid);
        row.attack_observed_micros_at_missing = Some(action.observed_micros);
    }
    row.credit_at_missing = Some(quest.credit);
    row.objective_identity_at_missing = Some(quest.objective_identity);
    row.purpose_at_missing = Some(quest.purpose);
    row.destination_at_missing = Some(quest.destination);
    true
}

fn record_missing_target_respawn(
    ctx: &ReducerContext,
    row: &mut AcceptanceMissingTargetFault,
    now: i64,
) -> bool {
    if now < row.respawn_due_micros.unwrap_or(i64::MAX) {
        return false;
    }
    let Some(entity) = ctx
        .db
        .game_world_entity()
        .guid()
        .find(row.target_guid)
        .filter(|entity| !entity.dead && entity.health > 0)
    else {
        return false;
    };
    let Some(spawn) = ctx.db.game_creature_spawn().guid().find(row.target_guid) else {
        return false;
    };
    if spawn.life_seq <= row.removed_life_seq.unwrap_or(u64::MAX) {
        return false;
    }
    row.respawned_micros = Some(now);
    row.respawned_life_seq = Some(spawn.life_seq);
    row.respawned_health = Some(entity.health);
    true
}

fn record_missing_target_resume(
    ctx: &ReducerContext,
    row: &mut AcceptanceMissingTargetFault,
    now: i64,
) -> bool {
    let Some(action) = super::actions::observation(ctx, row.character_guid, ActionKind::Attack)
    else {
        return false;
    };
    if action.target_guid != row.target_guid
        || action.observed_micros < row.respawned_micros.unwrap_or(i64::MAX)
        || !matches!(action.outcome, ActionOutcome::AttackAccepted(_))
    {
        return false;
    }
    let Some(quest) = acceptance_quest_state(ctx, row.character_guid, row.quest_entry) else {
        return false;
    };
    row.resumed_micros = Some(now);
    row.resumed_attack_target = Some(action.target_guid);
    row.resumed_credit = Some(quest.credit);
    row.resumed_objective_identity = Some(quest.objective_identity);
    row.resumed_purpose = Some(quest.purpose);
    row.resumed_destination = Some(quest.destination);
    true
}

fn advance_missing_target_fault(
    ctx: &ReducerContext,
    mut row: AcceptanceMissingTargetFault,
    now: i64,
) {
    if row.removal_attempted_micros.is_none() {
        if let Some(quest) = acceptance_quest_state(ctx, row.character_guid, row.quest_entry) {
            arm_missing_target_fault(ctx, &mut row, now, quest);
            ctx.db
                .pkg_playerbots_acceptance_missing_target_fault()
                .character_guid()
                .update(row);
        }
        return;
    }
    if row.removal_applied_micros.is_none() || row.removal_error.is_some() {
        return;
    }
    if row.missing_observed_micros.is_none() {
        if record_missing_target_wait(ctx, &mut row) {
            ctx.db
                .pkg_playerbots_acceptance_missing_target_fault()
                .character_guid()
                .update(row);
        }
        return;
    }
    if row.respawned_micros.is_none() {
        if record_missing_target_respawn(ctx, &mut row, now) {
            ctx.db
                .pkg_playerbots_acceptance_missing_target_fault()
                .character_guid()
                .update(row);
        }
        return;
    }
    if row.resumed_micros.is_none() && record_missing_target_resume(ctx, &mut row, now) {
        ctx.db
            .pkg_playerbots_acceptance_missing_target_fault()
            .character_guid()
            .update(row);
    }
}

crate::game_tick_pass!(fn playerbots_acceptance_missing_target_fault_pass(ctx) {
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    let schedules: Vec<_> = ctx
        .db
        .pkg_playerbots_acceptance_missing_target_fault()
        .iter()
        .take(101)
        .collect();
    if schedules.len() > 100 {
        spacetimedb::log::error!("playerbots: acceptance missing-target schedule exceeds 100 rows");
        return;
    }
    for row in schedules {
        advance_missing_target_fault(ctx, row, now);
    }
});

fn quest_row_state(
    ctx: &ReducerContext,
    character_guid: u64,
    quest_entry: u32,
) -> Option<(u64, u32)> {
    let mut rows = ctx
        .db
        .game_character_quest()
        .by_character_quest()
        .filter((character_guid, quest_entry))
        .take(2);
    let row = rows.next()?;
    if rows.next().is_some() || row.rewarded || row.failed {
        return None;
    }
    Some((row.id, row.counts.first().copied().unwrap_or(0)))
}

fn replace_local_navigation(
    ctx: &ReducerContext,
    character_guid: u64,
    blocked: bool,
) -> Result<Vec<u64>, String> {
    let entity = ctx
        .db
        .game_world_entity()
        .guid()
        .find(character_guid)
        .ok_or("acceptance Character is not in the world")?;
    let cell_x = lyracore_shared::terrain::cell_index(entity.x)
        .ok_or("acceptance Character is outside the navigation grid")?;
    let cell_y = lyracore_shared::terrain::cell_index(entity.y)
        .ok_or("acceptance Character is outside the navigation grid")?;
    let chunks = ctx.db.game_nav_chunk();
    let mut keys = Vec::with_capacity(9);
    for x in cell_x.saturating_sub(1)..=cell_x.saturating_add(1).min(1023) {
        for y in cell_y.saturating_sub(1)..=cell_y.saturating_add(1).min(1023) {
            let key = lyracore_shared::terrain::cell_key(entity.map_id, x, y);
            chunks.key().delete(key);
            chunks.insert(crate::nav::NavChunk {
                key,
                map_id: entity.map_id,
                cell_x: x,
                cell_y: y,
                base_z: entity.z,
                walk: vec![u8::from(!blocked) * u8::MAX; lyracore_shared::nav::WALK_BYTES],
                obs: vec![
                    if blocked {
                        20
                    } else {
                        lyracore_shared::nav::OBS_NONE
                    };
                    lyracore_shared::nav::OBS_BYTES
                ],
            });
            keys.push(key);
        }
    }
    crate::nav::record_change(ctx)?;
    Ok(keys)
}

fn stage_unreachable_giver_layout(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(u64, Vec<u64>, u64), String> {
    let character = ctx
        .db
        .game_world_entity()
        .guid()
        .find(character_guid)
        .ok_or("acceptance Character is not in the world")?;
    let giver_guid = fixture_creature_guid(UNREACHABLE_GIVER_ENTRY);
    super::fixture::playerbots_fixture_position(ctx, giver_guid, character.x + 40.0)?;
    let spawns = ctx.db.game_creature_spawn();
    let mut spawn = spawns
        .guid()
        .find(giver_guid)
        .ok_or("acceptance unreachable giver spawn is absent")?;
    spawn.x = character.x + 40.0;
    spawns.guid().update(spawn);
    super::quest_catalog::refresh_catalog(ctx, "unknown");
    let blocked_navigation_keys = replace_local_navigation(ctx, character_guid, true)?;
    let revision = ctx
        .db
        .game_navigation_revision()
        .id()
        .find(0)
        .ok_or("acceptance navigation revision is absent")?
        .revision;
    Ok((giver_guid, blocked_navigation_keys, revision))
}

fn matching_giver_attempt(
    runner: &super::PlayerbotsRunner,
    giver_guid: u64,
) -> Option<&super::recovery::Attempt> {
    runner.recovery.as_ref()?.attempts.iter().find(|attempt| {
        matches!(
            attempt.work,
            super::recovery::Work::Quest(super::recovery::QuestWork {
                step: super::decision::QuestInteraction { target, quest },
                operation: super::recovery::QuestOperation::Accept,
            }) if target == giver_guid && quest == UNREACHABLE_GIVER_QUEST
        )
    })
}

fn advance_unreachable_giver_fault(
    ctx: &ReducerContext,
    mut row: AcceptanceUnreachableGiverFault,
    now: i64,
) {
    if row.resumed_micros.is_some() || row.restore_error.is_some() {
        return;
    }
    let Some(runner) = ctx
        .db
        .pkg_playerbots_runner()
        .character_guid()
        .find(row.character_guid)
    else {
        return;
    };
    let previous_approach = row.maximum_approach;
    if let Some(number) = matching_giver_attempt(&runner, row.giver_guid)
        .and_then(|attempt| attempt.position.as_ref())
        .map(|position| position.number)
    {
        row.maximum_approach = row.maximum_approach.max(number);
    }
    if row.deferred_micros.is_none() {
        let Some(giver) = ctx.db.game_world_entity().guid().find(row.giver_guid) else {
            return;
        };
        let Some(attempt) = matching_giver_attempt(&runner, row.giver_guid) else {
            return;
        };
        let Some(deferred) = runner.deferred_destinations.iter().find(|deferred| {
            deferred.destination.map_id == giver.map_id
                && deferred.destination.instance_id == giver.instance_id
                && deferred.destination.x == giver.x
                && deferred.destination.y == giver.y
                && deferred.destination.z == giver.z
        }) else {
            if row.maximum_approach != previous_approach {
                ctx.db
                    .pkg_playerbots_acceptance_unreachable_giver_fault()
                    .character_guid()
                    .update(row);
            }
            return;
        };
        if attempt.deferred_until_micros != Some(deferred.until_micros) {
            row.restore_error = Some("deferred destination does not match its attempt".to_string());
            ctx.db
                .pkg_playerbots_acceptance_unreachable_giver_fault()
                .character_guid()
                .update(row);
            return;
        }
        row.deferred_micros = Some(attempt.last_observed_micros);
        row.deferred_until_micros = Some(deferred.until_micros);
        if let Some((id, credit)) = quest_row_state(ctx, row.character_guid, row.quest_entry) {
            row.quest_id_at_deferral = Some(id);
            row.credit_at_deferral = Some(credit);
        }
    }
    if row.alternative_micros.is_none() {
        let alternative = ctx
            .db
            .pkg_playerbots_quest_objective()
            .character_guid()
            .find(row.character_guid)
            .filter(|objective| objective.quest_entry == row.alternative_quest_entry);
        let Some(alternative) = alternative else {
            ctx.db
                .pkg_playerbots_acceptance_unreachable_giver_fault()
                .character_guid()
                .update(row);
            return;
        };
        let Some(action) =
            super::actions::observation(ctx, row.character_guid, ActionKind::AcceptQuest).filter(
                |action| {
                    action.quest_entry == row.alternative_quest_entry
                        && matches!(action.outcome, ActionOutcome::Completed)
                        && action.observed_micros < row.deferred_until_micros.unwrap_or(i64::MIN)
                },
            )
        else {
            ctx.db
                .pkg_playerbots_acceptance_unreachable_giver_fault()
                .character_guid()
                .update(row);
            return;
        };
        row.alternative_micros = Some(now);
        row.alternative_objective_identity = Some(alternative.runner_objective_identity);
        row.alternative_target_guid = Some(action.target_guid);
        row.alternative_action_observed_micros = Some(action.observed_micros);
        row.restore_due_micros = row.deferred_until_micros;
    }
    if row.restore_attempted_micros.is_none()
        && row.restore_due_micros.is_some_and(|due| now >= due)
    {
        row.restore_attempted_micros = Some(now);
        match replace_local_navigation(ctx, row.character_guid, false) {
            Ok(keys) if keys == row.blocked_navigation_keys => {
                row.restored_micros = Some(now);
                row.restored_navigation_revision = ctx
                    .db
                    .game_navigation_revision()
                    .id()
                    .find(0)
                    .map(|revision| revision.revision);
            }
            Ok(_) => row.restore_error = Some("restored navigation key set differs".to_string()),
            Err(error) => row.restore_error = Some(error),
        }
    }
    if row.restored_micros.is_some() && row.resumed_micros.is_none() {
        if let Some(retained) = ctx
            .db
            .pkg_playerbots_quest_objective()
            .character_guid()
            .find(row.character_guid)
            .filter(|objective| objective.quest_entry == row.quest_entry)
        {
            row.resumed_micros = Some(now);
            row.resumed_objective_identity = Some(retained.runner_objective_identity);
            if let Some((id, credit)) = quest_row_state(ctx, row.character_guid, row.quest_entry) {
                row.quest_id_at_resume = Some(id);
                row.credit_at_resume = Some(credit);
            }
        }
    }
    ctx.db
        .pkg_playerbots_acceptance_unreachable_giver_fault()
        .character_guid()
        .update(row);
}

crate::game_tick_pass!(fn playerbots_acceptance_unreachable_giver_fault_pass(ctx) {
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    let schedules: Vec<_> = ctx
        .db
        .pkg_playerbots_acceptance_unreachable_giver_fault()
        .iter()
        .take(101)
        .collect();
    if schedules.len() > 100 {
        spacetimedb::log::error!("playerbots: acceptance unreachable-giver schedule exceeds 100 rows");
        return;
    }
    for row in schedules {
        advance_unreachable_giver_fault(ctx, row, now);
    }
});

fn stage_admission_fault_content(ctx: &ReducerContext) -> Result<(), String> {
    let objectives = ctx.db.pkg_playerbots_catalog_objective();
    let id = u64::from(ADMISSION_FAULT_QUEST) << 8;
    let mut objective = objectives
        .id()
        .find(id)
        .ok_or("acceptance admission fault objective is absent")?;
    if objective.kind != CatalogObjectiveKind::KillCreature {
        return Err("acceptance admission fault objective has changed".to_string());
    }
    objective.kind = CatalogObjectiveKind::Escort;
    objectives.id().update(objective);
    let catalogs = ctx.db.pkg_playerbots_quest_catalog();
    let mut catalog = catalogs
        .revision()
        .find(CATALOG_REVISION)
        .ok_or("acceptance catalog manifest is absent")?;
    catalog.refresh_after_micros = i64::MAX;
    catalogs.revision().update(catalog);
    Ok(())
}

fn prerequisite_turnin_count(
    ctx: &ReducerContext,
    character_guid: u64,
    quest_entry: u32,
) -> Option<u32> {
    let rows: Vec<_> = ctx
        .db
        .pkg_playerbots_quest_turnin_fixture()
        .by_character()
        .filter(character_guid)
        .filter(|row| row.quest_entry == quest_entry)
        .take(2)
        .collect();
    match rows.as_slice() {
        [row] => Some(u32::from(row.turnin_count)),
        _ => None,
    }
}

fn record_prerequisite_refusal(
    ctx: &ReducerContext,
    row: &mut AcceptanceAdmissionFault,
    now: i64,
) -> bool {
    let Err(AdmissionRefusal::Ineligible(detail)) =
        super::quest_catalog::admit_available(ctx, row.character_guid, row.quest_entry)
    else {
        return false;
    };
    row.prerequisite_refused_micros = Some(now);
    row.prerequisite_refusal_detail = Some(detail);
    row.quest_id_at_prerequisite_refusal =
        quest_row_state(ctx, row.character_guid, row.quest_entry).map(|state| state.0);
    true
}

fn record_unsupported_selection(ctx: &ReducerContext, row: &mut AcceptanceAdmissionFault) -> bool {
    let Some(admission) = ctx
        .db
        .pkg_playerbots_quest_admission()
        .character_guid()
        .find(row.character_guid)
        .filter(|admission| {
            admission.considered_quest == row.quest_entry
                && admission.selected_quest == Some(row.supported_alternative_quest_entry)
                && admission.missing_capability == Some(MissingCapability::Escort)
        })
    else {
        return false;
    };
    row.unsupported_observed_micros = Some(admission.observed_micros);
    row.unsupported_capability = admission.missing_capability;
    row.selected_alternative_quest = admission.selected_quest;
    row.quest_id_at_unsupported_refusal =
        quest_row_state(ctx, row.character_guid, row.quest_entry).map(|state| state.0);
    true
}

fn record_alternative_acceptance(
    ctx: &ReducerContext,
    row: &mut AcceptanceAdmissionFault,
    now: i64,
) -> bool {
    let Some(action) =
        super::actions::observation(ctx, row.character_guid, ActionKind::AcceptQuest).filter(
            |action| {
                action.quest_entry == row.supported_alternative_quest_entry
                    && matches!(action.outcome, ActionOutcome::Completed)
            },
        )
    else {
        return false;
    };
    row.alternative_accept_observed_micros = Some(action.observed_micros);
    row.alternative_accept_target_guid = Some(action.target_guid);
    row.restore_due_micros = Some(now.saturating_add(ADMISSION_FAULT_RESTORE_DELAY_MICROS));
    true
}

fn restore_admission_fault(ctx: &ReducerContext, row: &mut AcceptanceAdmissionFault, now: i64) {
    row.restore_attempted_micros = Some(now);
    let objectives = ctx.db.pkg_playerbots_catalog_objective();
    let id = u64::from(row.quest_entry) << 8;
    let Some(mut objective) = objectives.id().find(id) else {
        row.restore_error = Some("acceptance admission fault objective disappeared".to_string());
        return;
    };
    if objective.kind != CatalogObjectiveKind::Escort {
        row.restore_error = Some("acceptance admission fault was replaced".to_string());
        return;
    }
    objective.kind = CatalogObjectiveKind::KillCreature;
    objectives.id().update(objective);
    row.restored_micros = Some(now);
}

fn record_admission_resume(
    ctx: &ReducerContext,
    row: &mut AcceptanceAdmissionFault,
    now: i64,
) -> bool {
    let Some(action) =
        super::actions::observation(ctx, row.character_guid, ActionKind::AcceptQuest).filter(
            |action| {
                action.quest_entry == row.quest_entry
                    && action.observed_micros >= row.restored_micros.unwrap_or(i64::MAX)
                    && matches!(action.outcome, ActionOutcome::Completed)
            },
        )
    else {
        return false;
    };
    let Some((id, credit)) = quest_row_state(ctx, row.character_guid, row.quest_entry) else {
        return false;
    };
    row.resumed_micros = Some(now);
    row.resumed_accept_observed_micros = Some(action.observed_micros);
    row.resumed_accept_target_guid = Some(action.target_guid);
    row.resumed_quest_id = Some(id);
    row.resumed_credit = Some(credit);
    true
}

fn advance_admission_fault(ctx: &ReducerContext, mut row: AcceptanceAdmissionFault, now: i64) {
    if row.resumed_micros.is_some() || row.restore_error.is_some() {
        return;
    }
    if row.prerequisite_refused_micros.is_none() {
        if record_prerequisite_refusal(ctx, &mut row, now) {
            ctx.db
                .pkg_playerbots_acceptance_admission_fault()
                .character_guid()
                .update(row);
        }
        return;
    }
    if row.prerequisite_rewarded_micros.is_none() {
        let rewarded = ctx
            .db
            .game_character_quest()
            .by_character_quest()
            .filter((row.character_guid, row.prerequisite_quest_entry))
            .take(2)
            .collect::<Vec<_>>();
        if rewarded.len() != 1 || !rewarded[0].rewarded {
            return;
        }
        let Some(turnins) =
            prerequisite_turnin_count(ctx, row.character_guid, row.prerequisite_quest_entry)
        else {
            return;
        };
        row.prerequisite_rewarded_micros = Some(now);
        row.prerequisite_turnin_count = Some(turnins);
        ctx.db
            .pkg_playerbots_acceptance_admission_fault()
            .character_guid()
            .update(row);
        return;
    }
    if row.unsupported_observed_micros.is_none() {
        if record_unsupported_selection(ctx, &mut row) {
            ctx.db
                .pkg_playerbots_acceptance_admission_fault()
                .character_guid()
                .update(row);
        }
        return;
    }
    if row.alternative_accept_observed_micros.is_none() {
        if record_alternative_acceptance(ctx, &mut row, now) {
            ctx.db
                .pkg_playerbots_acceptance_admission_fault()
                .character_guid()
                .update(row);
        }
        return;
    }
    if row.restore_attempted_micros.is_none()
        && row.restore_due_micros.is_some_and(|due| now >= due)
    {
        restore_admission_fault(ctx, &mut row, now);
    }
    if row.restored_micros.is_some()
        && row.resumed_micros.is_none()
        && record_admission_resume(ctx, &mut row, now)
    {
        ctx.db
            .pkg_playerbots_acceptance_admission_fault()
            .character_guid()
            .update(row);
        return;
    }
    ctx.db
        .pkg_playerbots_acceptance_admission_fault()
        .character_guid()
        .update(row);
}

crate::game_tick_pass!(fn playerbots_acceptance_admission_fault_pass(ctx) {
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    let schedules: Vec<_> = ctx
        .db
        .pkg_playerbots_acceptance_admission_fault()
        .iter()
        .take(101)
        .collect();
    if schedules.len() > 100 {
        spacetimedb::log::error!("playerbots: acceptance admission fault schedule exceeds 100 rows");
        return;
    }
    for row in schedules {
        advance_admission_fault(ctx, row, now);
    }
});

fn same_plan(left: &SeedPlan, right: &SeedPlan) -> bool {
    left.seed == right.seed
        && left.class == right.class
        && left.role == right.role
        && left.suffix == right.suffix
        && left.start_offset_x == right.start_offset_x
        && left.start_offset_y == right.start_offset_y
        && left.initial_due_phase_micros == right.initial_due_phase_micros
        && left.source_insertion_rotation == right.source_insertion_rotation
        && left.plan_revision == right.plan_revision
        && left.catalog_name == right.catalog_name
        && left.catalog_revision == right.catalog_revision
        && left.catalog_blueprint_revision == right.catalog_blueprint_revision
}

fn seed_plan(seed: u64) -> Result<SeedPlan, String> {
    let (class, role, first) = match seed / 1_000 {
        783_001 => (super::class::WARRIOR, super::ROLE_TANK, 783_001_000),
        783_005 => (super::class::PRIEST, super::ROLE_HEALER, 783_005_000),
        783_008 => (super::class::MAGE, super::ROLE_DPS, 783_008_000),
        _ => return Err(format!("unsupported acceptance seed {seed}")),
    };
    let suffix = seed
        .checked_sub(first)
        .filter(|suffix| *suffix < OFFSETS.len() as u64)
        .ok_or_else(|| format!("unsupported acceptance seed {seed}"))? as u8;
    let (start_offset_x, start_offset_y) = OFFSETS[usize::from(suffix)];
    Ok(SeedPlan {
        seed,
        class,
        role,
        suffix,
        start_offset_x,
        start_offset_y,
        initial_due_phase_micros: i64::from(suffix) * 100_000,
        source_insertion_rotation: suffix,
        plan_revision: PLAN_REVISION.to_string(),
        catalog_name: CATALOG_NAME.to_string(),
        catalog_revision: CATALOG_REVISION,
        catalog_blueprint_revision: CATALOG_BLUEPRINT_REVISION.to_string(),
    })
}

#[reducer]
pub fn playerbots_acceptance_resolve_seed_plan(
    ctx: &ReducerContext,
    seed: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let plan = seed_plan(seed)?;
    let rows = ctx.db.pkg_playerbots_acceptance_seed_plan();
    match rows.seed().find(seed) {
        Some(existing) if same_plan(&existing, &plan) => Ok(()),
        Some(_) => Err(format!("acceptance seed {seed} already has another plan")),
        None => {
            rows.insert(plan);
            Ok(())
        }
    }
}

fn exact_bot(ctx: &ReducerContext, character_guid: u64) -> Result<PlayerbotsBot, String> {
    let mut bots = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(character_guid)
        .take(2);
    let bot = bots.next().ok_or("acceptance Character is not a bot")?;
    if bots.next().is_some() {
        return Err("acceptance Character has more than one bot row".to_string());
    }
    Ok(bot)
}

fn prepare_inventory_fault(ctx: &ReducerContext, character_guid: u64) -> Result<(), String> {
    super::quest_catalog_fixture::playerbots_quest_fixture_fill_inventory(ctx, character_guid)?;
    let items = ctx.db.game_item_instance();
    let fillers: Vec<_> = items
        .by_owner_guid()
        .filter(character_guid)
        .filter(|item| item.entry == INVENTORY_FAULT_FILLER)
        .take(65)
        .collect();
    if fillers.is_empty() || fillers.len() > 64 {
        return Err("acceptance inventory filler count is outside its bounded setup".to_string());
    }
    for filler in fillers {
        items.guid().delete(filler.guid);
    }
    if !crate::items::has_free_slot(ctx, character_guid) {
        return Err("acceptance inventory setup did not restore a free slot".to_string());
    }
    Ok(())
}

fn stage_combat_fault(ctx: &ReducerContext, character_guid: u64, staged_micros: i64) {
    ctx.db
        .pkg_playerbots_acceptance_combat_fault()
        .insert(AcceptanceCombatFault {
            character_guid,
            quest_entry: COMBAT_FAULT_QUEST,
            staged_micros,
            lethal_delay_micros: LETHAL_FAULT_DELAY_MICROS,
            incoming_attempted_micros: None,
            incoming_applied_micros: None,
            incoming_attacker_guid: None,
            incoming_health_before: None,
            incoming_health_after: None,
            incoming_credit_before: None,
            incoming_credit_after: None,
            incoming_objective_identity_before: None,
            incoming_objective_identity_after: None,
            lethal_attempted_micros: None,
            lethal_applied_micros: None,
            lethal_attacker_guid: None,
            lethal_health_before: None,
            lethal_health_after: None,
            lethal_credit_before: None,
            lethal_credit_after: None,
            lethal_objective_identity_before: None,
            lethal_objective_identity_after: None,
            resurrected_micros: None,
            resurrected_health: None,
            resurrected_credit: None,
            resurrected_objective_identity: None,
            resurrected_map: None,
            resurrected_instance: None,
            resurrected_x: None,
            resurrected_y: None,
            resurrected_z: None,
            incoming_purpose: None,
            incoming_destination: None,
            lethal_purpose: None,
            lethal_destination: None,
            resurrected_purpose: None,
            resurrected_destination: None,
        });
}

fn stage_inventory_fault(ctx: &ReducerContext, character_guid: u64, staged_micros: i64) {
    ctx.db
        .pkg_playerbots_acceptance_inventory_fault()
        .insert(AcceptanceInventoryFault {
            character_guid,
            quest_entry: INVENTORY_FAULT_QUEST,
            item_entry: INVENTORY_FAULT_ITEM,
            staged_micros,
            fill_attempted_micros: None,
            fill_applied_micros: None,
            fill_error: None,
            item_count_before_fill: None,
            credit_before_fill: None,
            objective_identity_before_fill: None,
            refused_micros: None,
            refused_action_observed_micros: None,
            refused_source_guid: None,
            refused_loot_slot: None,
            item_count_at_refusal: None,
            credit_at_refusal: None,
            objective_identity_at_refusal: None,
            retry_count_at_refusal: None,
            retry_observed_micros: None,
            retry_after_micros: None,
            release_delay_micros: INVENTORY_FAULT_RELEASE_DELAY_MICROS,
            space_release_due_micros: None,
            space_release_attempted_micros: None,
            space_released_micros: None,
            space_release_error: None,
            resumed_micros: None,
            resumed_action_observed_micros: None,
            resumed_source_guid: None,
            resumed_item_count: None,
            resumed_credit: None,
            resumed_objective_identity: None,
            purpose_before_fill: None,
            destination_before_fill: None,
            purpose_at_refusal: None,
            destination_at_refusal: None,
            resumed_purpose: None,
            resumed_destination: None,
        });
}

fn stage_missing_target_fault(ctx: &ReducerContext, character_guid: u64, staged_micros: i64) {
    ctx.db
        .pkg_playerbots_acceptance_missing_target_fault()
        .insert(AcceptanceMissingTargetFault {
            character_guid,
            quest_entry: MISSING_TARGET_FAULT_QUEST,
            target_entry: MISSING_TARGET_FAULT_ENTRY,
            target_guid: fixture_creature_guid(MISSING_TARGET_FAULT_ENTRY),
            staged_micros,
            respawn_delay_micros: MISSING_TARGET_RESPAWN_DELAY_MICROS,
            removal_attempted_micros: None,
            removal_applied_micros: None,
            removal_error: None,
            removed_life_seq: None,
            credit_before_removal: None,
            objective_identity_before_removal: None,
            respawn_due_micros: None,
            missing_observed_micros: None,
            missing_failure: None,
            missing_retry_count: None,
            missing_retry_after_micros: None,
            attack_target_at_missing: None,
            attack_observed_micros_at_missing: None,
            credit_at_missing: None,
            objective_identity_at_missing: None,
            respawned_micros: None,
            respawned_life_seq: None,
            respawned_health: None,
            resumed_micros: None,
            resumed_attack_target: None,
            resumed_credit: None,
            resumed_objective_identity: None,
            removal_character_x: None,
            removal_character_y: None,
            removal_character_z: None,
            removal_target_x: None,
            removal_target_y: None,
            removal_target_z: None,
            removal_target_distance: None,
            purpose_before_removal: None,
            destination_before_removal: None,
            purpose_at_missing: None,
            destination_at_missing: None,
            resumed_purpose: None,
            resumed_destination: None,
        });
}

fn stage_unreachable_giver_fault(
    ctx: &ReducerContext,
    character_guid: u64,
    staged_micros: i64,
    giver_guid: u64,
    blocked_navigation_keys: Vec<u64>,
    blocked_navigation_revision: u64,
) {
    ctx.db
        .pkg_playerbots_acceptance_unreachable_giver_fault()
        .insert(AcceptanceUnreachableGiverFault {
            character_guid,
            quest_entry: UNREACHABLE_GIVER_QUEST,
            giver_entry: UNREACHABLE_GIVER_ENTRY,
            giver_guid,
            alternative_quest_entry: UNREACHABLE_ALTERNATIVE_QUEST,
            blocked_navigation_keys,
            staged_micros,
            blocked_navigation_revision,
            maximum_approach: 0,
            deferred_micros: None,
            deferred_until_micros: None,
            quest_id_at_deferral: None,
            credit_at_deferral: None,
            alternative_micros: None,
            alternative_objective_identity: None,
            alternative_target_guid: None,
            alternative_action_observed_micros: None,
            restore_due_micros: None,
            restore_attempted_micros: None,
            restored_micros: None,
            restored_navigation_revision: None,
            restore_error: None,
            resumed_micros: None,
            resumed_objective_identity: None,
            quest_id_at_resume: None,
            credit_at_resume: None,
        });
}

fn stage_admission_fault(ctx: &ReducerContext, character_guid: u64, staged_micros: i64) {
    ctx.db
        .pkg_playerbots_acceptance_admission_fault()
        .insert(AcceptanceAdmissionFault {
            character_guid,
            quest_entry: ADMISSION_FAULT_QUEST,
            prerequisite_quest_entry: ADMISSION_PREREQUISITE,
            supported_alternative_quest_entry: ADMISSION_SUPPORTED_ALTERNATIVE,
            staged_micros,
            prerequisite_refused_micros: None,
            prerequisite_refusal_detail: None,
            quest_id_at_prerequisite_refusal: None,
            prerequisite_rewarded_micros: None,
            prerequisite_turnin_count: None,
            unsupported_observed_micros: None,
            unsupported_capability: None,
            selected_alternative_quest: None,
            quest_id_at_unsupported_refusal: None,
            alternative_accept_observed_micros: None,
            alternative_accept_target_guid: None,
            restore_due_micros: None,
            restore_attempted_micros: None,
            restored_micros: None,
            restore_error: None,
            resumed_micros: None,
            resumed_accept_observed_micros: None,
            resumed_accept_target_guid: None,
            resumed_quest_id: None,
            resumed_credit: None,
        });
}

fn stage_journey_for(
    ctx: &ReducerContext,
    character_guid: u64,
    plan: SeedPlan,
) -> Result<(), String> {
    let bot = exact_bot(ctx, character_guid)?;
    if (bot.class, bot.role) != (plan.class, plan.role) {
        return Err(format!(
            "acceptance seed {} requires class {} role {}, got class {} role {}",
            plan.seed, plan.class, plan.role, bot.class, bot.role
        ));
    }
    if bot.controller != Controller::Legacy {
        return Err("acceptance journey setup requires Legacy control".to_string());
    }

    playerbots_acceptance_resolve_seed_plan(ctx, plan.seed)?;
    let journeys = ctx.db.pkg_playerbots_acceptance_journey();
    if let Some(existing) = journeys.character_guid().find(character_guid) {
        return if existing.seed == plan.seed
            && existing.class == plan.class
            && existing.role == plan.role
            && existing.initial_due_phase_micros == plan.initial_due_phase_micros
            && existing.source_insertion_rotation == plan.source_insertion_rotation
            && existing.plan_revision == plan.plan_revision
            && existing.catalog_name == plan.catalog_name
            && existing.catalog_revision == plan.catalog_revision
            && existing.catalog_blueprint_revision == plan.catalog_blueprint_revision
        {
            Ok(())
        } else {
            Err(format!(
                "acceptance Character {character_guid} already has another journey manifest"
            ))
        };
    }
    if journeys.iter().next().is_some() {
        return Err("acceptance journey staging requires a fresh private Shard".to_string());
    }
    let source_insertion_guids = super::quest_catalog_fixture::stage_named_with_rotation(
        ctx,
        character_guid,
        plan.source_insertion_rotation,
    )?;
    super::quest_catalog_fixture::playerbots_quest_loop_fixture_stage_simple_gameobject(
        ctx,
        character_guid,
    )?;
    prepare_inventory_fault(ctx, character_guid)?;

    let mut entity = ctx
        .db
        .game_world_entity()
        .guid()
        .find(character_guid)
        .ok_or("acceptance Character is not in the world")?;
    entity.x += plan.start_offset_x;
    entity.y += plan.start_offset_y;
    let (grid_x, grid_y) = lyracore_shared::spatial::grid_cell(entity.x, entity.y);
    entity.grid_x = grid_x;
    entity.grid_y = grid_y;
    entity.cell = lyracore_shared::spatial::grid_cell_id(grid_x, grid_y);
    ctx.db.game_world_entity().guid().update(entity);
    let entity = ctx
        .db
        .game_world_entity()
        .guid()
        .find(character_guid)
        .ok_or("acceptance Character disappeared during staging")?;
    let graveyards = ctx.db.game_graveyard();
    let mut graveyard = graveyards
        .id()
        .find(JOURNEY_GRAVEYARD_ID)
        .ok_or("acceptance journey graveyard is missing")?;
    graveyard.map_id = entity.map_id;
    graveyard.x = entity.x;
    graveyard.y = entity.y;
    graveyard.z = entity.z;
    graveyard.name = "Acceptance journey graveyard".to_string();
    let graveyard_id = graveyard.id;
    let graveyard_x = graveyard.x;
    let graveyard_y = graveyard.y;
    let graveyard_z = graveyard.z;
    graveyards.id().update(graveyard);
    let (giver_guid, blocked_navigation_keys, blocked_navigation_revision) =
        stage_unreachable_giver_layout(ctx, character_guid)?;
    stage_admission_fault_content(ctx)?;

    let catalog = ctx
        .db
        .pkg_playerbots_quest_catalog()
        .revision()
        .find(CATALOG_REVISION)
        .ok_or("acceptance catalog manifest missing")?;
    let named = ctx
        .db
        .pkg_playerbots_quest_loop_fixture()
        .character_guid()
        .find(character_guid)
        .ok_or("acceptance named journey manifest missing")?;
    let simple = ctx
        .db
        .pkg_playerbots_seeded_quest_fixture()
        .iter()
        .next()
        .ok_or("acceptance simple GameObject manifest missing")?;
    let ownership = ctx
        .db
        .pkg_playerbots_quest_fixture_ownership()
        .iter()
        .next()
        .ok_or("acceptance fixture ownership missing")?;
    let journey = AcceptanceJourney {
        character_guid,
        seed: plan.seed,
        class: plan.class,
        role: plan.role,
        start_map: entity.map_id,
        start_instance: entity.instance_id,
        start_x: entity.x,
        start_y: entity.y,
        start_z: entity.z,
        initial_due_phase_micros: plan.initial_due_phase_micros,
        journey_started_micros: None,
        first_due_micros: None,
        source_insertion_rotation: plan.source_insertion_rotation,
        source_insertion_guids,
        plan_revision: plan.plan_revision,
        catalog_name: catalog.name,
        catalog_revision: catalog.revision,
        catalog_blueprint_revision: catalog.blueprint_revision,
        catalog_content_revision: catalog.content_revision,
        named_quest_entry: named.quest_entry,
        named_target_entry: named.target_entry,
        named_target_count: named.target_count,
        named_content_revision: named.content_revision,
        simple_quest_entry: simple.quest_entry,
        simple_gameobject_entry: simple.gameobject_entry,
        simple_content_revision: simple.content_revision,
        fixture_ownership_revision: ownership.revision,
        staged_micros: ctx.timestamp.to_micros_since_unix_epoch(),
        graveyard_id,
        graveyard_x,
        graveyard_y,
        graveyard_z,
    };
    let staged_micros = journey.staged_micros;
    journeys.insert(journey);
    stage_combat_fault(ctx, character_guid, staged_micros);
    stage_inventory_fault(ctx, character_guid, staged_micros);
    stage_missing_target_fault(ctx, character_guid, staged_micros);
    stage_unreachable_giver_fault(
        ctx,
        character_guid,
        staged_micros,
        giver_guid,
        blocked_navigation_keys,
        blocked_navigation_revision,
    );
    stage_admission_fault(ctx, character_guid, staged_micros);
    Ok(())
}

/// Spawn one bot through the public path and stage its declared resources in the same private-Shard
/// transaction. The caller selects Cohort control only after it reads the saved manifest.
#[reducer]
pub fn playerbots_acceptance_stage_journey(ctx: &ReducerContext, seed: u64) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let plan = seed_plan(seed)?;
    let journeys = ctx.db.pkg_playerbots_acceptance_journey();
    let mut existing = journeys.iter().take(2);
    if let Some(journey) = existing.next() {
        if existing.next().is_some() {
            return Err("acceptance journey staging found multiple manifests".to_string());
        }
        return if journey.seed == seed {
            let bot = exact_bot(ctx, journey.character_guid)?;
            if (bot.class, bot.role) == (journey.class, journey.role)
                && bot.controller == Controller::Legacy
                && journey.journey_started_micros.is_none()
                && journey.first_due_micros.is_none()
            {
                Ok(())
            } else {
                Err("existing acceptance journey is no longer staged".to_string())
            }
        } else {
            Err("acceptance private Shard already has another journey".to_string())
        };
    }
    if ctx.db.pkg_playerbots_bot().iter().take(1).next().is_some() {
        return Err("acceptance journey staging requires a fresh private Shard".to_string());
    }

    super::playerbots_spawn_class_role(ctx, 1, 1_200.0, 1_200.0, 50.0, plan.class, plan.role)?;
    let mut bots = ctx.db.pkg_playerbots_bot().iter().take(2);
    let bot = bots.next().ok_or("acceptance spawn did not create a bot")?;
    if bots.next().is_some() {
        return Err("acceptance spawn created more than one bot".to_string());
    }
    stage_journey_for(ctx, bot.character_guid, plan)
}

/// Select Cohort control and arm the declared first due time in one transaction.
#[reducer]
pub fn playerbots_acceptance_begin_journey(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let journeys = ctx.db.pkg_playerbots_acceptance_journey();
    let mut journey = journeys
        .character_guid()
        .find(character_guid)
        .ok_or("acceptance journey manifest missing")?;
    let bot = exact_bot(ctx, character_guid)?;
    if (bot.class, bot.role) != (journey.class, journey.role) {
        return Err("acceptance journey bot identity changed".to_string());
    }
    if journey.journey_started_micros.is_some() || journey.first_due_micros.is_some() {
        return if journey.journey_started_micros.is_some()
            && journey.first_due_micros.is_some()
            && bot.controller == Controller::Cohort
        {
            Ok(())
        } else {
            Err("acceptance journey start state is inconsistent".to_string())
        };
    }
    if bot.controller != Controller::Legacy {
        return Err("acceptance journey must begin from Legacy control".to_string());
    }

    let started_micros = ctx.timestamp.to_micros_since_unix_epoch();
    let first_due_micros = started_micros.saturating_add(journey.initial_due_phase_micros);
    super::runner::playerbots_select_controller(ctx, character_guid, Controller::Cohort)?;
    let mut bot = exact_bot(ctx, character_guid)?;
    bot.next_think_micros = first_due_micros;
    ctx.db.pkg_playerbots_bot().id().update(bot);
    journey.journey_started_micros = Some(started_micros);
    journey.first_due_micros = Some(first_due_micros);
    journeys.character_guid().update(journey);
    Ok(())
}

fn stage_level_gap_sources(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(Vec<u64>, Vec<u64>), String> {
    let character = crate::helpers::live_entity(ctx, character_guid)?;
    let templates = ctx.db.game_creature_template();
    let mut template = templates
        .entry()
        .find(6)
        .ok_or("level-gap source template missing")?;
    template.entry = LEVEL_GAP_SOURCE_ENTRY;
    template.name = "Acceptance level-gap source".to_string();
    template.level = 8;
    template.max_level = 8;
    template.health = 1;
    template.max_level_health = 1;
    template.rank = 0;
    template.aggro_range = 0;
    template.damage_min = 1;
    template.damage_max = 1;
    template.npc_flags = 0;
    templates.entry().delete(LEVEL_GAP_SOURCE_ENTRY);
    let template = templates.insert(template);

    let spawns = ctx.db.game_creature_spawn();
    let entities = ctx.db.game_world_entity();
    let mut source_guids = [299, 69, 38]
        .into_iter()
        .map(fixture_creature_guid)
        .collect::<Vec<_>>();
    source_guids.reserve(LEVEL_GAP_SOURCE_COUNT as usize);
    let mut long_source_guids = Vec::with_capacity(LEVEL_GAP_LONG_SOURCE_COUNT as usize);
    for offset in 0..LEVEL_GAP_SOURCE_COUNT {
        let guid = fixture_creature_guid(LEVEL_GAP_SOURCE_ENTRY).saturating_add(offset);
        let (x, y) = if offset < LEVEL_GAP_LONG_SOURCE_COUNT {
            (character.x + 0.5 + offset as f32 * 0.2, character.y)
        } else {
            (
                character.x + 3.0 + (offset % 11) as f32 * 5.0,
                character.y - 25.0 + (offset / 11) as f32 * 10.0,
            )
        };
        let spawn = crate::CreatureSpawn {
            guid,
            entry: LEVEL_GAP_SOURCE_ENTRY,
            map_id: character.map_id,
            x,
            y,
            z: character.z,
            orientation: 0.0,
            respawn_at: crate::creatures::timer_never(ctx),
            despawn_at: crate::creatures::timer_never(ctx),
            movement_type: 0,
            respawn_secs: 60,
            life_seq: 1,
        };
        let mut entity = crate::creatures::build_creature_entity(&spawn, &template, 0, 0);
        if offset < LEVEL_GAP_LONG_SOURCE_COUNT {
            entity.health = 20;
            entity.max_health = 20;
            long_source_guids.push(guid);
        }
        spawns.guid().delete(guid);
        spawns.insert(spawn);
        entities.guid().delete(guid);
        crate::creatures::insert_creature_entity(ctx, entity);
        source_guids.push(guid);
    }
    Ok((source_guids, long_source_guids))
}

fn stage_level_gap_history(ctx: &ReducerContext, character_guid: u64) -> Result<(), String> {
    let owner_identity = crate::helpers::live_entity(ctx, character_guid)?.owner_identity;
    let quests = ctx.db.game_character_quest();
    for quest_entry in [783, 7, 5261, 33, 18, 3903, 3904, 3905] {
        for row in quests
            .by_character_quest()
            .filter((character_guid, quest_entry))
            .collect::<Vec<_>>()
        {
            quests.id().delete(row.id);
        }
        quests.insert(crate::CharacterQuest {
            id: 0,
            character_guid,
            owner_identity,
            quest_entry,
            counts: Vec::new(),
            rewarded: true,
            deadline_micros: 0,
            failed: false,
        });
    }
    Ok(())
}

/// Stage the source-derived level-seven boundary without granting XP or changing level after start.
#[reducer]
pub fn playerbots_acceptance_stage_level_gap(ctx: &ReducerContext) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    if ctx.db.pkg_playerbots_bot().iter().next().is_some()
        || ctx
            .db
            .pkg_playerbots_acceptance_level_gap()
            .iter()
            .next()
            .is_some()
    {
        return Err("level-gap fixture needs a fresh private Shard".to_string());
    }
    super::ensure_defaults(ctx);
    let character_guid = super::spawn_one(
        ctx,
        super::class::WARRIOR,
        super::ROLE_TANK,
        super::role_name_stem(super::ROLE_TANK),
        0,
        (1_200.0, 1_200.0, 50.0),
        5,
    )?;
    super::quest_catalog_fixture::stage_named_with_rotation(ctx, character_guid, 0)?;
    stage_level_gap_history(ctx, character_guid)?;
    let templates = ctx.db.game_quest_template();
    let mut quest = templates
        .entry()
        .find(LEVEL_GAP_QUEST)
        .ok_or("level-gap Quest template missing")?;
    quest.min_level = LEVEL_GAP_REQUIRED_LEVEL;
    templates.entry().update(quest);
    let (source_guids, long_source_guids) = stage_level_gap_sources(ctx, character_guid)?;
    super::quest_catalog::refresh_catalog(ctx, "unknown");
    let catalog = ctx
        .db
        .pkg_playerbots_catalog_quest()
        .quest_entry()
        .find(LEVEL_GAP_QUEST)
        .ok_or("level-gap catalog Quest missing")?;
    if catalog.min_level != LEVEL_GAP_REQUIRED_LEVEL {
        return Err("level-gap catalog did not retain the source requirement".to_string());
    }
    let character = crate::helpers::live_entity(ctx, character_guid)?;
    super::runner::playerbots_select_controller(ctx, character_guid, Controller::Frozen)?;
    ctx.db
        .pkg_playerbots_acceptance_level_gap()
        .insert(AcceptanceLevelGap {
            character_guid,
            quest_entry: LEVEL_GAP_QUEST,
            required_level: LEVEL_GAP_REQUIRED_LEVEL,
            initial_level: character.level,
            initial_xp: character.xp,
            source_entry: LEVEL_GAP_SOURCE_ENTRY,
            source_guids,
            long_source_guids,
            source_requirement_revision: CATALOG_BLUEPRINT_REVISION.to_string(),
            imported_content_revision: None,
            staged_micros: ctx.timestamp.to_micros_since_unix_epoch(),
            started_micros: None,
            observed_level: character.level,
            observed_xp: character.xp,
            kill_count: 0,
            accepted_level: None,
            accepted_xp: None,
            accepted_micros: None,
        });
    Ok(())
}

crate::game_hook!(on_kill, fn playerbots_acceptance_record_level_gap_kill(ctx, payload) {
    let fixtures = ctx.db.pkg_playerbots_acceptance_level_gap();
    let Some(mut fixture) = fixtures.character_guid().find(payload.killer_guid) else {
        return;
    };
    let kills = ctx.db.pkg_playerbots_acceptance_level_gap_kill();
    let Some(character) = ctx.db.game_world_entity().guid().find(payload.killer_guid) else {
        return;
    };
    let ordinal = fixture.kill_count.saturating_add(1);
    kills.insert(AcceptanceLevelGapKill {
        id: 0,
        victim_guid: payload.victim_guid,
        character_guid: payload.killer_guid,
        victim_entry: payload.victim_entry,
        ordinal,
        level_before: fixture.observed_level,
        xp_before: fixture.observed_xp,
        level_after: character.level,
        xp_after: character.xp,
        killed_micros: ctx.timestamp.to_micros_since_unix_epoch(),
    });
    fixture.observed_level = character.level;
    fixture.observed_xp = character.xp;
    fixture.kill_count = ordinal;
    fixtures.character_guid().update(fixture);
});

crate::game_hook!(on_quest_accept, fn playerbots_acceptance_record_level_gap_accept(ctx, payload) {
    if payload.quest_entry != LEVEL_GAP_QUEST {
        return;
    }
    let fixtures = ctx.db.pkg_playerbots_acceptance_level_gap();
    let Some(mut fixture) = fixtures.character_guid().find(payload.character_guid) else {
        return;
    };
    let Some(character) = ctx.db.game_world_entity().guid().find(payload.character_guid) else {
        return;
    };
    fixture.accepted_level = Some(character.level);
    fixture.accepted_xp = Some(character.xp);
    fixture.accepted_micros = Some(ctx.timestamp.to_micros_since_unix_epoch());
    fixtures.character_guid().update(fixture);
});

crate::game_hook!(on_go_used, fn playerbots_acceptance_record_simple_gameobject_use(ctx, payload) {
    let Some(journey) = ctx
        .db
        .pkg_playerbots_acceptance_journey()
        .character_guid()
        .find(payload.user_guid)
    else {
        return;
    };
    if payload.go_entry != journey.simple_gameobject_entry {
        return;
    }
    let Some(retained) = ctx
        .db
        .pkg_playerbots_quest_objective()
        .character_guid()
        .find(payload.user_guid)
    else {
        return;
    };
    if retained.quest_entry != journey.simple_quest_entry
        || retained.target.kind != CatalogObjectiveKind::UseGameObject
        || retained.target.executor != ObjectiveExecutor::SimpleGameObject
        || retained.destination.kind != CatalogEntityKind::GameObject
        || retained.destination.entry != payload.go_entry
        || retained.destination.guid != payload.go_guid
        || retained.destination.instance_id != payload.instance_id
    {
        return;
    }
    let mut quests = ctx
        .db
        .game_character_quest()
        .by_character_quest()
        .filter((payload.user_guid, journey.simple_quest_entry))
        .take(2);
    let Some(quest) = quests.next() else {
        return;
    };
    if quests.next().is_some() || quest.failed || quest.rewarded {
        return;
    }

    let receipts = ctx.db.pkg_playerbots_acceptance_gameobject_use_receipt();
    if let Some(mut receipt) = receipts.character_guid().find(payload.user_guid) {
        receipt.use_count = receipt.use_count.saturating_add(1);
        receipts.character_guid().update(receipt);
    } else {
        receipts.insert(AcceptanceGameObjectUseReceipt {
            character_guid: payload.user_guid,
            character_quest_id: quest.id,
            runner_objective_identity: retained.runner_objective_identity,
            quest_entry: retained.quest_entry,
            gameobject_guid: payload.go_guid,
            gameobject_entry: payload.go_entry,
            instance_id: payload.instance_id,
            use_count: 1,
            first_observed_micros: ctx.timestamp.to_micros_since_unix_epoch(),
        });
    }
});

#[reducer]
pub fn playerbots_acceptance_begin_level_gap(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let rows = ctx.db.pkg_playerbots_acceptance_level_gap();
    let mut fixture = rows
        .character_guid()
        .find(character_guid)
        .ok_or("level-gap fixture is absent")?;
    if fixture.started_micros.is_some() {
        return Ok(());
    }
    let bot = exact_bot(ctx, character_guid)?;
    if bot.controller != Controller::Frozen {
        return Err("level-gap fixture is not parked".to_string());
    }
    super::runner::playerbots_select_controller(ctx, character_guid, Controller::Cohort)?;
    fixture.started_micros = Some(ctx.timestamp.to_micros_since_unix_epoch());
    rows.character_guid().update(fixture);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn representative_seed_plans_resolve_exact_nuisance_inputs() {
        let warrior = seed_plan(783_001_000).unwrap();
        assert_eq!((warrior.class, warrior.role), (1, 0));
        assert_eq!(
            (warrior.start_offset_x, warrior.start_offset_y),
            (-0.75, -0.75)
        );
        assert_eq!(warrior.initial_due_phase_micros, 0);
        assert_eq!(warrior.source_insertion_rotation, 0);

        let priest = seed_plan(783_005_005).unwrap();
        assert_eq!((priest.class, priest.role), (5, 1));
        assert_eq!((priest.start_offset_x, priest.start_offset_y), (0.0, -0.25));
        assert_eq!(priest.initial_due_phase_micros, 500_000);
        assert_eq!(priest.source_insertion_rotation, 5);

        let mage = seed_plan(783_008_009).unwrap();
        assert_eq!((mage.class, mage.role), (8, 2));
        assert_eq!((mage.start_offset_x, mage.start_offset_y), (0.75, 0.75));
        assert_eq!(mage.initial_due_phase_micros, 900_000);
        assert_eq!(mage.source_insertion_rotation, 9);
        assert_eq!(mage.plan_revision, PLAN_REVISION);
        assert_eq!(mage.catalog_name, CATALOG_NAME);
        assert_eq!(mage.catalog_revision, CATALOG_REVISION);
        assert_eq!(mage.catalog_blueprint_revision, CATALOG_BLUEPRINT_REVISION);
    }

    #[test]
    fn seeds_outside_the_three_declared_ranges_are_refused() {
        for seed in [
            783_000_999,
            783_001_010,
            783_004_999,
            783_005_010,
            783_007_999,
            783_008_010,
            783_009_000,
        ] {
            assert!(matches!(
                seed_plan(seed),
                Err(message) if message == format!("unsupported acceptance seed {seed}")
            ));
        }
    }
}
