//! Source inputs for the post-arrival Assist witness.

use super::orders::{CommandRecord, CompanionOrder, CompanionOrderState};
use super::{
    pkg_playerbots_bot, pkg_playerbots_companion_order, pkg_playerbots_runner, Controller,
};
use crate::{
    game_character, game_creature_move_schedule, game_group, game_group_member,
    game_group_member_partition, game_group_roster_revision, game_world_entity,
};
use spacetimedb::{reducer, table, Identity, ReducerContext, ScheduleAt, Table, TimeDuration};

const DESTINATION_MAP: u32 = 36;
const DESTINATION_INSTANCE: u64 = 5_098_078;
#[allow(clippy::approx_constant)] // Exact imported AreaTrigger landing orientation.
const PRIEST_DESTINATION: (f32, f32, f32, f32) = (-10.0, -385.475, 62.4561, 1.5708);
#[allow(clippy::approx_constant)] // Exact imported AreaTrigger landing orientation.
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

// Source staging evidence belongs to the companion it describes. It remains on the source during
// Escrow and expires when that source Character is deleted after Transfer.
crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_transfer_assist_source(ctx, character_guid) {
    let evidence = ctx.db.pkg_playerbots_transfer_assist_source();
    if evidence
        .id()
        .find(0)
        .is_some_and(|row| row.bot_guid == character_guid)
    {
        evidence.id().delete(0);
    }
});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_playerbots_transfer_assist_source());

struct SourceParty {
    order: CompanionOrderState,
    receipt: CommandRecord,
    leader_partition: PartitionSnapshot,
    priest_partition: PartitionSnapshot,
    partitions: Vec<PartitionSnapshot>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct PartitionSnapshot {
    character_guid: u64,
    group_id: u64,
    membership_revision: u64,
    member_active: bool,
    map_id: u32,
    instance_id: u64,
    locator_revision: u64,
    state: crate::PartyPartitionState,
}

impl From<&crate::GroupMemberPartition> for PartitionSnapshot {
    fn from(row: &crate::GroupMemberPartition) -> Self {
        Self {
            character_guid: row.character_guid,
            group_id: row.group_id,
            membership_revision: row.membership_revision,
            member_active: row.member_active,
            map_id: row.map_id,
            instance_id: row.instance_id,
            locator_revision: row.locator_revision,
            state: row.state,
        }
    }
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
    // A normal pass can append a runtime outcome after this command was accepted.
    let receipt = state
        .history
        .iter()
        .rev()
        .find(|receipt| {
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
        .map(|partition| PartitionSnapshot::from(*partition))
        .ok_or("Assist source fixture leader partition is absent")?;
    let priest_partition = active_partitions
        .iter()
        .find(|partition| partition.character_guid == priest_guid)
        .map(|partition| PartitionSnapshot::from(*partition))
        .ok_or("Assist source fixture Priest partition is absent")?;
    Ok(SourceParty {
        order,
        receipt,
        leader_partition,
        priest_partition,
        partitions: partitions.iter().map(PartitionSnapshot::from).collect(),
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
    let mut character = crate::helpers::character_by_guid(ctx, guid)
        .ok_or("Assist source fixture Character disappeared")?;
    character.map_id = DESTINATION_MAP;
    character.pending_instance_id = DESTINATION_INSTANCE;
    (character.x, character.y, character.z, character.orientation) = destination;
    ctx.db.game_character().guid().update(character);
    Ok(())
}

fn relocate_live_character(
    ctx: &ReducerContext,
    guid: u64,
    destination: (f32, f32, f32, f32),
) -> Result<(), String> {
    let entities = ctx.db.game_world_entity();
    let mut entity = entities
        .guid()
        .find(guid)
        .filter(|entity| entity.is_player() && entity.map_id == 0 && entity.instance_id == 0)
        .ok_or("Assist destination fixture requires a local player body")?;
    entity.map_id = DESTINATION_MAP;
    entity.instance_id = DESTINATION_INSTANCE;
    (entity.x, entity.y, entity.z, entity.orientation) = destination;
    let (grid_x, grid_y) = lyracore_shared::spatial::grid_cell(entity.x, entity.y);
    entity.grid_x = grid_x;
    entity.grid_y = grid_y;
    entity.cell = lyracore_shared::spatial::grid_cell_id(grid_x, grid_y);
    entities.guid().update(entity);
    relocate_character(ctx, guid, destination)
}

fn relocate_partition(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<PartitionSnapshot, String> {
    let mut partition = ctx
        .db
        .game_group_member_partition()
        .character_guid()
        .find(character_guid)
        .ok_or("Assist source fixture member partition disappeared")?;
    partition.map_id = DESTINATION_MAP;
    partition.instance_id = DESTINATION_INSTANCE;
    partition.locator_revision = partition
        .locator_revision
        .checked_add(1)
        .ok_or("Assist source fixture partition revision exhausted")?;
    let snapshot = PartitionSnapshot::from(&partition);
    ctx.db
        .game_group_member_partition()
        .character_guid()
        .update(partition);
    Ok(snapshot)
}

fn retained_partitions(
    ctx: &ReducerContext,
    before: &[PartitionSnapshot],
    leader: PartitionSnapshot,
    priest: PartitionSnapshot,
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
    let after: Vec<_> = after.iter().map(PartitionSnapshot::from).collect();
    let mut expected = before.to_vec();
    for changed in [leader, priest] {
        let row = expected
            .iter_mut()
            .find(|row| row.character_guid == changed.character_guid)
            .ok_or("Assist source fixture changed an unknown partition")?;
        *row = changed;
    }
    Ok(after == expected)
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
    let leader_after = relocate_partition(ctx, leader_guid)?;
    let priest_after = relocate_partition(ctx, priest_guid)?;
    let retained_after = retained_work(ctx, bot_guid, party.order.group_id)?;
    if !retained_partitions(ctx, &party.partitions, leader_after, priest_after)?
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

/// Place the already allocated leader and selected Priest on the destination Shard before the
/// companion starts crossing. The Gateway still owns the party mirror and arriving companion.
#[reducer]
pub fn playerbots_transfer_assist_destination_stage(
    ctx: &ReducerContext,
    bot_guid: u64,
    leader_guid: u64,
    priest_guid: u64,
    unused_mage_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let guids = [bot_guid, leader_guid, priest_guid, unused_mage_guid];
    if guids.contains(&0)
        || guids
            .iter()
            .enumerate()
            .any(|(index, guid)| guids[index + 1..].contains(guid))
    {
        return Err("Assist destination fixture requires four distinct Characters".to_string());
    }
    if crate::helpers::character_by_guid(ctx, bot_guid).is_some()
        || crate::helpers::character_by_guid(ctx, unused_mage_guid).is_some()
        || ctx
            .db
            .game_group_member()
            .by_group()
            .filter(&5_098_000u64)
            .next()
            .is_some()
        || ctx.db.game_group().group_id().find(5_098_000).is_some()
    {
        return Err("Assist destination fixture requires fresh arrival and party rows".to_string());
    }
    let leader = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(leader_guid)
        .next()
        .filter(|bot| bot.class == super::class::WARRIOR && bot.role == super::ROLE_TANK)
        .ok_or("Assist destination fixture requires the staged leader")?;
    if leader.controller != Controller::Legacy {
        return Err("Assist destination fixture requires the unclaimed human leader".to_string());
    }
    ctx.db.pkg_playerbots_bot().id().delete(leader.id);
    crate::actor::set_sessionless_action_consent(ctx, leader_guid, true);
    super::runner::playerbots_select_controller(ctx, priest_guid, Controller::Cohort)?;
    let priest = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(priest_guid)
        .next()
        .filter(|bot| bot.class == super::class::PRIEST && bot.role == super::ROLE_HEALER)
        .ok_or("Assist destination fixture requires the selected Priest")?;
    if priest.controller != Controller::Cohort {
        return Err("Assist destination fixture requires the Cohort Priest".to_string());
    }
    super::fixture::playerbots_fixture_freeze(ctx, priest_guid)?;
    relocate_live_character(ctx, leader_guid, LEADER_DESTINATION)?;
    relocate_live_character(ctx, priest_guid, PRIEST_DESTINATION)?;
    let schedules = ctx.db.game_creature_move_schedule(); // package-api: exempt private fixture declares the next Core movement tick before Transfer
    let mut ticks: Vec<_> = schedules.iter().take(2).collect();
    if ticks.len() != 1 || ticks[0].instance_id != u64::MAX {
        return Err("Assist destination fixture requires one fresh movement tick".to_string());
    }
    let at = ctx
        .timestamp
        .checked_add(TimeDuration::from_micros(60_000_000))
        .ok_or("Assist destination movement tick timestamp exhausted")?;
    let mut tick = ticks.remove(0);
    tick.scheduled_at = ScheduleAt::Time(at);
    schedules.scheduled_id().update(tick);
    Ok(())
}
