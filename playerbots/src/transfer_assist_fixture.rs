#![cfg(feature = "debug_reducers")]

//! Source inputs for the post-arrival Assist witness.

use super::orders::{CommandRecord, CompanionOrder, CompanionOrderState};
use super::{
    pkg_playerbots_bot, pkg_playerbots_companion_order, pkg_playerbots_runner, Controller,
};
use crate::{
    game_character, game_group, game_group_member, game_group_member_partition,
    game_group_roster_revision,
};
use spacetimedb::{reducer, table, Identity, ReducerContext, Table};

const DESTINATION_MAP: u32 = 36;
const DESTINATION_INSTANCE: u64 = 5_098_078;
const PRIEST_DESTINATION: (f32, f32, f32, f32) = (-10.0, -385.475, 62.4561, 1.5708);
const LEADER_DESTINATION: (f32, f32, f32, f32) = (-14.5732, -410.475, 62.4561, 1.5708);

#[table(accessor = pkg_playerbots_transfer_assist_source, public)]
pub struct PlayerbotsTransferAssistSource {
    #[primary_key]
    pub id: u8,
    pub bot_guid: u64,
    pub leader_guid: u64,
    pub selected_priest_guid: u64,
    pub group_id: u64,
    pub source_identity: Identity,
    pub command_intent_id: u64,
    pub issuer_sequence: u64,
    pub order_revision: u64,
    pub source_map: u32,
    pub source_instance: u64,
    pub priest_source_x: f32,
    pub priest_source_y: f32,
    pub priest_source_z: f32,
    pub leader_source_x: f32,
    pub leader_source_y: f32,
    pub leader_source_z: f32,
    pub destination_map: u32,
    pub destination_instance: u64,
    pub priest_destination_x: f32,
    pub priest_destination_y: f32,
    pub priest_destination_z: f32,
    pub leader_destination_x: f32,
    pub leader_destination_y: f32,
    pub leader_destination_z: f32,
    pub priest_partition_revision_before: u64,
    pub priest_partition_revision_after: u64,
    pub leader_partition_revision_before: u64,
    pub leader_partition_revision_after: u64,
    pub runner_generation: u64,
    pub runner_objective_sequence: u64,
    pub runner_companion_order_revision: u64,
    pub staged_micros: i64,
}

struct SourceParty {
    order: CompanionOrderState,
    receipt: CommandRecord,
    leader_partition: crate::GroupMemberPartition,
    priest_partition: crate::GroupMemberPartition,
    partitions: Vec<crate::GroupMemberPartition>,
}

struct RetainedWork {
    runner: Vec<u8>,
    order: Vec<u8>,
    group: Vec<u8>,
    roster: Vec<u8>,
    members: Vec<u8>,
}

fn authenticated_assist(
    state: &CompanionOrderState,
    leader_guid: u64,
    priest_guid: u64,
) -> Result<CommandRecord, String> {
    if !state.active
        || state.issuer_guid != leader_guid
        || !matches!(
            &state.order,
            CompanionOrder::Assist(assist) if assist.member_guid == priest_guid
        )
    {
        return Err("Assist source fixture requires the selected Priest order".to_string());
    }
    let receipt = state
        .history
        .last()
        .filter(|receipt| {
            receipt.source_identity != Identity::ZERO
                && receipt.intent_id != 0
                && receipt.issuer_guid == leader_guid
                && receipt.issuer_sequence == state.issuer_sequence
                && receipt.order == state.order
                && matches!(
                    receipt.outcome,
                    crate::actor::CommandOutcome::Applied | crate::actor::CommandOutcome::Unchanged
                )
        })
        .cloned()
        .ok_or("Assist source fixture requires an authenticated command receipt")?;
    if !state
        .issuer_fences
        .iter()
        .any(|fence| fence.issuer_guid == leader_guid && fence.sequence == state.issuer_sequence)
    {
        return Err("Assist source fixture requires the command issuer fence".to_string());
    }
    Ok(receipt)
}

fn source_party(
    ctx: &ReducerContext,
    bot_guid: u64,
    leader_guid: u64,
    priest_guid: u64,
) -> Result<SourceParty, String> {
    let order = ctx
        .db
        .pkg_playerbots_companion_order()
        .character_guid()
        .find(bot_guid)
        .ok_or("Assist source fixture order is absent")?;
    let receipt = authenticated_assist(&order, leader_guid, priest_guid)?;
    let _group = ctx
        .db
        .game_group()
        .group_id()
        .find(order.group_id)
        .filter(|group| group.leader_guid == leader_guid)
        .ok_or("Assist source fixture party leader changed")?;
    let _roster = ctx
        .db
        .game_group_roster_revision()
        .group_id()
        .find(order.group_id)
        .filter(|roster| roster.active && roster.revision != 0)
        .ok_or("Assist source fixture party roster is absent")?;
    let mut members: Vec<_> = ctx
        .db
        .game_group_member()
        .by_group()
        .filter(&order.group_id)
        .take(lyracore_shared::group::GROUP_MAX_MEMBERS + 1)
        .collect();
    if members.len() > lyracore_shared::group::GROUP_MAX_MEMBERS {
        return Err("Assist source fixture party exceeds the supported bound".to_string());
    }
    members.sort_by_key(|member| member.id);
    let mut partitions: Vec<_> = ctx
        .db
        .game_group_member_partition()
        .by_group()
        .filter(&order.group_id)
        .take(lyracore_shared::group::GROUP_MAX_MEMBERS * 2 + 1)
        .collect();
    if partitions.len() > lyracore_shared::group::GROUP_MAX_MEMBERS * 2 {
        return Err(
            "Assist source fixture partition history exceeds the supported bound".to_string(),
        );
    }
    partitions.sort_by_key(|partition| (partition.membership_revision, partition.character_guid));
    let member_guids: Vec<_> = members.iter().map(|member| member.character_guid).collect();
    let active_partitions: Vec<_> = partitions
        .iter()
        .filter(|partition| partition.member_active)
        .collect();
    if active_partitions.len() != members.len()
        || [bot_guid, leader_guid, priest_guid]
            .iter()
            .any(|guid| !member_guids.contains(guid))
        || active_partitions.iter().any(|partition| {
            !member_guids.contains(&partition.character_guid)
                || partition.group_id != order.group_id
                || partition.state != crate::PartyPartitionState::Known
        })
    {
        return Err("Assist source fixture party mirror changed".to_string());
    }
    let leader_partition = active_partitions
        .iter()
        .find(|partition| partition.character_guid == leader_guid)
        .map(|partition| (*partition).clone())
        .ok_or("Assist source fixture leader partition is absent")?;
    let priest_partition = active_partitions
        .iter()
        .find(|partition| partition.character_guid == priest_guid)
        .map(|partition| (*partition).clone())
        .ok_or("Assist source fixture Priest partition is absent")?;
    Ok(SourceParty {
        order,
        receipt,
        leader_partition,
        priest_partition,
        partitions,
    })
}

fn serialize<T: spacetimedb::sats::Serialize + ?Sized>(
    value: &T,
    name: &str,
) -> Result<Vec<u8>, String> {
    spacetimedb::sats::bsatn::to_vec(value)
        .map_err(|error| format!("cannot retain Assist {name}: {error}"))
}

fn retained_work(
    ctx: &ReducerContext,
    bot_guid: u64,
    group_id: u64,
) -> Result<RetainedWork, String> {
    let runner = ctx
        .db
        .pkg_playerbots_runner()
        .character_guid()
        .find(bot_guid)
        .ok_or("Assist source fixture Runner is absent")?;
    let order = ctx
        .db
        .pkg_playerbots_companion_order()
        .character_guid()
        .find(bot_guid)
        .ok_or("Assist source fixture order is absent")?;
    let group = ctx
        .db
        .game_group()
        .group_id()
        .find(group_id)
        .ok_or("Assist source fixture party is absent")?;
    let roster = ctx
        .db
        .game_group_roster_revision()
        .group_id()
        .find(group_id)
        .ok_or("Assist source fixture roster is absent")?;
    let mut members: Vec<_> = ctx
        .db
        .game_group_member()
        .by_group()
        .filter(&group_id)
        .take(lyracore_shared::group::GROUP_MAX_MEMBERS + 1)
        .collect();
    if members.len() > lyracore_shared::group::GROUP_MAX_MEMBERS {
        return Err("Assist source fixture party exceeds the supported bound".to_string());
    }
    members.sort_by_key(|member| member.id);
    Ok(RetainedWork {
        runner: serialize(&runner, "Runner")?,
        order: serialize(&order, "order")?,
        group: serialize(&group, "party")?,
        roster: serialize(&roster, "roster")?,
        members: serialize(&members, "party members")?,
    })
}

fn relocate_character(
    ctx: &ReducerContext,
    guid: u64,
    destination: (f32, f32, f32, f32),
) -> Result<(), String> {
    let characters = ctx.db.game_character();
    let mut character = characters
        .guid()
        .find(guid)
        .ok_or("Assist source fixture Character disappeared")?;
    character.map_id = DESTINATION_MAP;
    character.pending_instance_id = DESTINATION_INSTANCE;
    (character.x, character.y, character.z, character.orientation) = destination;
    characters.guid().update(character);
    Ok(())
}

fn relocate_partition(
    ctx: &ReducerContext,
    mut partition: crate::GroupMemberPartition,
) -> Result<crate::GroupMemberPartition, String> {
    partition.map_id = DESTINATION_MAP;
    partition.instance_id = DESTINATION_INSTANCE;
    partition.locator_revision = partition
        .locator_revision
        .checked_add(1)
        .ok_or("Assist source fixture partition revision exhausted")?;
    ctx.db
        .game_group_member_partition()
        .character_guid()
        .update(partition.clone());
    Ok(partition)
}

fn retained_partitions(
    ctx: &ReducerContext,
    before: &[crate::GroupMemberPartition],
    leader: &crate::GroupMemberPartition,
    priest: &crate::GroupMemberPartition,
) -> Result<bool, String> {
    let mut after: Vec<_> = ctx
        .db
        .game_group_member_partition()
        .by_group()
        .filter(&leader.group_id)
        .take(lyracore_shared::group::GROUP_MAX_MEMBERS * 2 + 1)
        .collect();
    if after.len() > lyracore_shared::group::GROUP_MAX_MEMBERS * 2 {
        return Err(
            "Assist source fixture partition history exceeds the supported bound".to_string(),
        );
    }
    after.sort_by_key(|partition| (partition.membership_revision, partition.character_guid));
    let mut expected = before.to_vec();
    for changed in [leader, priest] {
        let row = expected
            .iter_mut()
            .find(|row| row.character_guid == changed.character_guid)
            .ok_or("Assist source fixture changed an unknown partition")?;
        *row = changed.clone();
    }
    Ok(
        serialize(&after, "partition mirror")?
            == serialize(&expected, "expected partition mirror")?,
    )
}

/// Retain a real Assist order while its selected Priest and leader leave the source partition.
#[reducer]
pub fn playerbots_transfer_assist_source_stage(
    ctx: &ReducerContext,
    bot_guid: u64,
    leader_guid: u64,
    priest_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    if bot_guid == 0
        || leader_guid == 0
        || priest_guid == 0
        || bot_guid == leader_guid
        || bot_guid == priest_guid
        || leader_guid == priest_guid
    {
        return Err("Assist source fixture requires three distinct Characters".to_string());
    }
    if ctx
        .db
        .pkg_playerbots_transfer_assist_source()
        .id()
        .find(0)
        .is_some()
    {
        return Err("Assist source fixture requires a fresh receipt".to_string());
    }
    if ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(bot_guid)
        .next()
        .filter(|bot| bot.controller == Controller::Cohort)
        .is_none()
    {
        return Err("Assist source fixture requires an admitted Cohort bot".to_string());
    }
    if ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(priest_guid)
        .next()
        .filter(|bot| bot.class == super::class::PRIEST)
        .is_none()
    {
        return Err("Assist source fixture requires the selected Priest".to_string());
    }
    let party = source_party(ctx, bot_guid, leader_guid, priest_guid)?;
    let bot_body = crate::helpers::live_entity(ctx, bot_guid)?;
    let leader_body = crate::helpers::live_entity(ctx, leader_guid)?;
    let priest_body = crate::helpers::live_entity(ctx, priest_guid)?;
    let source_partition = (bot_body.map_id, bot_body.instance_id);
    if !bot_body.is_player()
        || !leader_body.is_player()
        || !priest_body.is_player()
        || (leader_body.map_id, leader_body.instance_id) != source_partition
        || (priest_body.map_id, priest_body.instance_id) != source_partition
        || party
            .partitions
            .iter()
            .filter(|partition| partition.member_active)
            .any(|partition| (partition.map_id, partition.instance_id) != source_partition)
    {
        return Err("Assist source fixture requires one local party partition".to_string());
    }
    for guid in [bot_guid, leader_guid, priest_guid] {
        let character = crate::helpers::character_by_guid(ctx, guid)
            .ok_or("Assist source fixture Character is absent")?;
        if (character.map_id, character.pending_instance_id) != source_partition {
            return Err("Assist source fixture Character partition changed".to_string());
        }
    }
    let retained_before = retained_work(ctx, bot_guid, party.order.group_id)?;
    let leader_source = (leader_body.x, leader_body.y, leader_body.z);
    let priest_source = (priest_body.x, priest_body.y, priest_body.z);
    crate::world::remove_live_character(ctx, leader_body); // package-api: exempt private fixture declares remote party member
    crate::world::remove_live_character(ctx, priest_body); // package-api: exempt private fixture declares remote party member
    relocate_character(ctx, leader_guid, LEADER_DESTINATION)?;
    relocate_character(ctx, priest_guid, PRIEST_DESTINATION)?;
    let leader_after = relocate_partition(ctx, party.leader_partition.clone())?;
    let priest_after = relocate_partition(ctx, party.priest_partition.clone())?;
    let retained_after = retained_work(ctx, bot_guid, party.order.group_id)?;
    if !retained_partitions(ctx, &party.partitions, &leader_after, &priest_after)?
        || retained_after.runner != retained_before.runner
        || retained_after.order != retained_before.order
        || retained_after.group != retained_before.group
        || retained_after.roster != retained_before.roster
        || retained_after.members != retained_before.members
    {
        return Err("Assist source fixture changed retained party work".to_string());
    }
    let runner = ctx
        .db
        .pkg_playerbots_runner()
        .character_guid()
        .find(bot_guid)
        .ok_or("Assist source fixture Runner disappeared")?;
    ctx.db
        .pkg_playerbots_transfer_assist_source()
        .insert(PlayerbotsTransferAssistSource {
            id: 0,
            bot_guid,
            leader_guid,
            selected_priest_guid: priest_guid,
            group_id: party.order.group_id,
            source_identity: party.receipt.source_identity,
            command_intent_id: party.receipt.intent_id,
            issuer_sequence: party.order.issuer_sequence,
            order_revision: party.order.revision,
            source_map: source_partition.0,
            source_instance: source_partition.1,
            priest_source_x: priest_source.0,
            priest_source_y: priest_source.1,
            priest_source_z: priest_source.2,
            leader_source_x: leader_source.0,
            leader_source_y: leader_source.1,
            leader_source_z: leader_source.2,
            destination_map: DESTINATION_MAP,
            destination_instance: DESTINATION_INSTANCE,
            priest_destination_x: PRIEST_DESTINATION.0,
            priest_destination_y: PRIEST_DESTINATION.1,
            priest_destination_z: PRIEST_DESTINATION.2,
            leader_destination_x: LEADER_DESTINATION.0,
            leader_destination_y: LEADER_DESTINATION.1,
            leader_destination_z: LEADER_DESTINATION.2,
            priest_partition_revision_before: party.priest_partition.locator_revision,
            priest_partition_revision_after: priest_after.locator_revision,
            leader_partition_revision_before: party.leader_partition.locator_revision,
            leader_partition_revision_after: leader_after.locator_revision,
            runner_generation: runner.generation,
            runner_objective_sequence: runner.objective_sequence,
            runner_companion_order_revision: runner.companion_order_revision,
            staged_micros: ctx.timestamp.to_micros_since_unix_epoch(),
        });
    Ok(())
}
