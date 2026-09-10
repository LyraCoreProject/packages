//! Private three-database authority and mirror-failure staging for Gateway Transfer cases.

use super::pkg_playerbots_bot;
use crate::transfer::game_transfer_in; // package-api: exempt private fixture binds the arrival before injecting a mirror fault
use crate::{
    game_character, game_character_shard, game_group, game_group_member,
    game_group_member_partition, game_group_roster_revision, game_world_entity,
};
use spacetimedb::{reducer, table, Identity, ReducerContext, Table};

const GROUP: u64 = 5_098_000;
const LEADER_MEMBER: u64 = 5_098_001;
const COMPANION_MEMBER: u64 = 5_098_002;
const PRIEST_MEMBER: u64 = 5_098_003;
const MAGE_MEMBER: u64 = 5_098_004;
const PARTY_LOOT_METHOD: u8 = 0;
const FAULT_LOOT_METHOD: u8 = 3;
const DESTINATION_POSITION: (f32, f32, f32) = (-14.5732, -385.475, 62.4561);
#[allow(clippy::approx_constant)] // Exact imported AreaTrigger landing orientation.
const DESTINATION_ORIENTATION: f32 = 1.5708;
const EXIT_LEADER_POSITION: (f32, f32, f32) = (-11_198.7, 1_675.9, 24.5733);
#[allow(clippy::approx_constant)] // Exact imported AreaTrigger landing orientation.
const EXIT_ORIENTATION: f32 = 4.71239;

#[table(accessor = pkg_playerbots_transfer_gateway_identity, public)]
pub struct PlayerbotsTransferGatewayIdentity {
    #[primary_key]
    pub id: u8,
    pub identity: Identity,
}

/// Record the actual Module identity used by one private Gateway Transfer database.
#[reducer]
pub fn playerbots_transfer_gateway_identity_stage(ctx: &ReducerContext) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let identities = ctx.db.pkg_playerbots_transfer_gateway_identity();
    if identities.id().find(0).is_some() {
        return Err("Gateway Transfer identity fixture requires a fresh row".to_string());
    }
    identities.insert(PlayerbotsTransferGatewayIdentity {
        id: 0,
        identity: ctx.database_identity(),
    });
    Ok(())
}

fn partition(
    character_guid: u64,
    membership_revision: u64,
    map_id: u32,
    instance_id: u64,
) -> crate::GroupMemberPartition {
    crate::GroupMemberPartition {
        character_guid,
        group_id: GROUP,
        membership_revision,
        member_active: true,
        map_id,
        instance_id,
        locator_revision: 1,
        state: crate::PartyPartitionState::Known,
    }
}

fn locator(character_guid: u64, map_id: u32, instance_id: u64, now: i64) -> crate::CharacterShard {
    crate::CharacterShard {
        character_guid,
        map_id,
        instance_id,
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

struct GatewayParty {
    leader_guid: u64,
    loot_method: u8,
    loot_threshold: u8,
    master_looter_guid: u64,
    member_guids: Vec<u64>,
    partitions: Vec<crate::GroupMemberPartition>,
    roster_revision: u64,
}

fn exact_gateway_party(
    ctx: &ReducerContext,
    companion_guid: u64,
    leader_guid: u64,
    priest_guid: u64,
    mage_guid: u64,
) -> Result<GatewayParty, String> {
    let mut expected = vec![companion_guid, leader_guid, priest_guid, mage_guid];
    expected.sort_unstable();
    if expected[0] == 0 || expected.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err("Gateway exit fixture requires four distinct Characters".to_string());
    }
    let group = ctx
        .db
        .game_group()
        .group_id()
        .find(GROUP)
        .filter(|group| {
            group.leader_guid == leader_guid
                && group.loot_method == PARTY_LOOT_METHOD
                && group.loot_threshold == 2
                && group.master_looter_guid == 0
        })
        .ok_or("Gateway exit fixture party rules changed")?;
    let roster_revision = ctx
        .db
        .game_group_roster_revision()
        .group_id()
        .find(GROUP)
        .filter(|roster| roster.active && roster.revision == 1)
        .ok_or("Gateway exit fixture roster changed")?
        .revision;
    let mut members: Vec<_> = ctx
        .db
        .game_group_member()
        .by_group()
        .filter(&GROUP)
        .take(lyracore_shared::group::GROUP_MAX_MEMBERS + 1)
        .collect();
    members.sort_by_key(|member| member.character_guid);
    if members.len() != expected.len()
        || members
            .iter()
            .map(|member| member.character_guid)
            .ne(expected.iter().copied())
    {
        return Err("Gateway exit fixture party members changed".to_string());
    }
    let mut partitions: Vec<_> = ctx
        .db
        .game_group_member_partition()
        .by_group()
        .filter(&GROUP)
        .take(lyracore_shared::group::GROUP_MAX_MEMBERS * 2 + 1)
        .collect();
    if partitions.len() != expected.len()
        || partitions.iter().any(|partition| {
            let membership_revision = match partition.character_guid {
                guid if guid == leader_guid => LEADER_MEMBER,
                guid if guid == companion_guid => COMPANION_MEMBER,
                guid if guid == priest_guid => PRIEST_MEMBER,
                guid if guid == mage_guid => MAGE_MEMBER,
                _ => 0,
            };
            partition.group_id != GROUP
                || !partition.member_active
                || partition.state != crate::PartyPartitionState::Known
                || partition.membership_revision != membership_revision
        })
    {
        return Err("Gateway exit fixture member partitions changed".to_string());
    }
    partitions.sort_by_key(|partition| (partition.membership_revision, partition.character_guid));
    let member_guids = partitions
        .iter()
        .map(|partition| partition.character_guid)
        .collect();
    Ok(GatewayParty {
        leader_guid: group.leader_guid,
        loot_method: group.loot_method,
        loot_threshold: group.loot_threshold,
        master_looter_guid: group.master_looter_guid,
        member_guids,
        partitions,
        roster_revision,
    })
}

fn exact_known_partition(
    party: &GatewayParty,
    character_guid: u64,
    map_id: u32,
    instance_id: u64,
    locator_revision: u64,
) -> bool {
    party.partitions.iter().any(|partition| {
        partition.character_guid == character_guid
            && (partition.map_id, partition.instance_id) == (map_id, instance_id)
            && partition.locator_revision == locator_revision
    })
}

/// Stage the Realm-owned party roster and settled member partitions used by the private Gateway
/// process. The World Shard fixtures stage their own mirrors separately.
#[reducer]
#[allow(clippy::too_many_arguments)] // The fixture carries four identities and both partitions.
pub fn playerbots_transfer_gateway_realm_stage(
    ctx: &ReducerContext,
    companion_guid: u64,
    leader_guid: u64,
    priest_guid: u64,
    mage_guid: u64,
    source_map: u32,
    source_instance: u64,
    destination_map: u32,
    destination_instance: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let character_guids = [companion_guid, leader_guid, priest_guid, mage_guid];
    if character_guids.contains(&0)
        || character_guids
            .iter()
            .enumerate()
            .any(|(index, guid)| character_guids[index + 1..].contains(guid))
    {
        return Err("Gateway Transfer fixture requires four distinct Characters".to_string());
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
        || character_guids.iter().any(|guid| {
            ctx.db
                .game_character_shard()
                .character_guid()
                .find(*guid)
                .is_some()
        })
    {
        return Err("Gateway Transfer fixture requires fresh Realm rows".to_string());
    }
    let locators = ctx.db.game_character_shard();

    ctx.db.game_group().insert(crate::Group {
        group_id: GROUP,
        leader_guid,
        loot_method: PARTY_LOOT_METHOD,
        loot_threshold: 2,
        rr_cursor: 0,
        master_looter_guid: 0,
    });
    let members = ctx.db.game_group_member();
    members.insert(crate::GroupMember {
        id: LEADER_MEMBER,
        group_id: GROUP,
        character_guid: leader_guid,
        owner_identity: Identity::ZERO,
    });
    members.insert(crate::GroupMember {
        id: PRIEST_MEMBER,
        group_id: GROUP,
        character_guid: priest_guid,
        owner_identity: Identity::ZERO,
    });
    members.insert(crate::GroupMember {
        id: MAGE_MEMBER,
        group_id: GROUP,
        character_guid: mage_guid,
        owner_identity: Identity::ZERO,
    });
    members.insert(crate::GroupMember {
        id: COMPANION_MEMBER,
        group_id: GROUP,
        character_guid: companion_guid,
        owner_identity: Identity::ZERO,
    });
    ctx.db.game_group_member_partition().insert(partition(
        leader_guid,
        LEADER_MEMBER,
        source_map,
        source_instance,
    ));
    ctx.db.game_group_member_partition().insert(partition(
        companion_guid,
        COMPANION_MEMBER,
        source_map,
        source_instance,
    ));
    ctx.db.game_group_member_partition().insert(partition(
        priest_guid,
        PRIEST_MEMBER,
        source_map,
        source_instance,
    ));
    ctx.db.game_group_member_partition().insert(partition(
        mage_guid,
        MAGE_MEMBER,
        source_map,
        source_instance,
    ));
    ctx.db
        .game_group_roster_revision()
        .insert(crate::GroupRosterRevision {
            group_id: GROUP,
            revision: 1,
            active: true,
        });
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    locators.insert(locator(companion_guid, source_map, source_instance, now));
    locators.insert(locator(priest_guid, source_map, source_instance, now));
    locators.insert(locator(mage_guid, source_map, source_instance, now));
    restage_completed_member_crossing(
        ctx,
        leader_guid,
        source_map,
        source_instance,
        destination_map,
        destination_instance,
    )?;
    let leader = locators
        .character_guid()
        .find(leader_guid)
        .filter(|row| {
            (row.map_id, row.instance_id) == (destination_map, destination_instance)
                && row.revision == 2
                && !row.transfer_pending
        })
        .ok_or("Gateway Transfer fixture did not settle its leader locator")?;
    let leader_partition = ctx
        .db
        .game_group_member_partition()
        .character_guid()
        .find(leader_guid)
        .filter(|row| {
            (row.map_id, row.instance_id) == (leader.map_id, leader.instance_id)
                && row.locator_revision == leader.revision
                && row.state == crate::PartyPartitionState::Known
        });
    if leader_partition.is_none() {
        return Err("Gateway Transfer fixture did not project its leader crossing".to_string());
    }
    Ok(())
}

/// Keep the generic Gateway fixture's human leader on the World Shard named by Realm-core.
#[reducer]
#[allow(clippy::too_many_arguments)] // The fixture binds all four identities and one partition.
pub fn playerbots_transfer_gateway_destination_leader_stage(
    ctx: &ReducerContext,
    companion_guid: u64,
    leader_guid: u64,
    priest_guid: u64,
    mage_guid: u64,
    source_map: u32,
    source_instance: u64,
    destination_map: u32,
    destination_instance: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let guids = [companion_guid, leader_guid, priest_guid, mage_guid];
    if guids.contains(&0)
        || guids
            .iter()
            .enumerate()
            .any(|(index, guid)| guids[index + 1..].contains(guid))
    {
        return Err("Gateway Transfer destination requires four distinct Characters".to_string());
    }
    if [companion_guid, priest_guid, mage_guid]
        .iter()
        .any(|guid| crate::helpers::character_by_guid(ctx, *guid).is_some())
        || ctx.db.game_group().group_id().find(GROUP).is_some()
        || ctx
            .db
            .game_group_member()
            .by_group()
            .filter(&GROUP)
            .next()
            .is_some()
    {
        return Err("Gateway Transfer destination rows are not fresh".to_string());
    }
    let leader = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(leader_guid)
        .next()
        .filter(|bot| {
            bot.class == super::class::WARRIOR
                && bot.role == super::ROLE_TANK
                && bot.controller == super::Controller::Legacy
        })
        .ok_or("Gateway Transfer destination leader changed")?;
    ctx.db.pkg_playerbots_bot().id().delete(leader.id);
    crate::actor::set_sessionless_action_consent(ctx, leader_guid, true);

    let mut character = crate::helpers::character_by_guid(ctx, leader_guid)
        .ok_or("Gateway Transfer destination leader Character is absent")?;
    character.map_id = destination_map;
    character.pending_instance_id = destination_instance;
    (character.x, character.y, character.z) = DESTINATION_POSITION;
    character.orientation = DESTINATION_ORIENTATION;
    ctx.db.game_character().guid().update(character);

    let entities = ctx.db.game_world_entity();
    let mut entity = entities
        .guid()
        .find(leader_guid)
        .filter(|entity| entity.is_player())
        .ok_or("Gateway Transfer destination leader body is absent")?;
    entity.map_id = destination_map;
    entity.instance_id = destination_instance;
    (entity.x, entity.y, entity.z) = DESTINATION_POSITION;
    let (grid_x, grid_y) = lyracore_shared::spatial::grid_cell(entity.x, entity.y);
    entity.grid_x = grid_x;
    entity.grid_y = grid_y;
    entity.cell = lyracore_shared::spatial::grid_cell_id(grid_x, grid_y);
    entities.guid().update(entity);
    crate::group::sync_group_mirror(
        ctx,
        GROUP,
        leader_guid,
        PARTY_LOOT_METHOD,
        2,
        0,
        vec![leader_guid, companion_guid, priest_guid, mage_guid],
        crate::SessionActor {
            guid: leader_guid,
            ownership: None,
        },
        vec![
            {
                let mut leader = partition(
                    leader_guid,
                    LEADER_MEMBER,
                    destination_map,
                    destination_instance,
                );
                leader.locator_revision = 2;
                leader
            },
            partition(
                companion_guid,
                COMPANION_MEMBER,
                source_map,
                source_instance,
            ),
            partition(priest_guid, PRIEST_MEMBER, source_map, source_instance),
            partition(mage_guid, MAGE_MEMBER, source_map, source_instance),
        ],
        1,
    )?;
    Ok(())
}

fn restage_completed_member_crossing(
    ctx: &ReducerContext,
    character_guid: u64,
    source_map: u32,
    source_instance: u64,
    destination_map: u32,
    destination_instance: u64,
) -> Result<(), String> {
    let locators = ctx.db.game_character_shard();
    if locators.character_guid().find(character_guid).is_some() {
        locators.character_guid().delete(character_guid);
    }
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    locators.insert(locator(character_guid, source_map, source_instance, now));
    crate::realm_core::record_shard(ctx, character_guid, destination_map, destination_instance); // package-api: exempt private fixture models a completed Realm locator crossing
    let settled = locators
        .character_guid()
        .find(character_guid)
        .filter(|row| {
            row.map_id == destination_map
                && row.instance_id == destination_instance
                && row.revision == 2
                && !row.transfer_pending
        })
        .ok_or("Assist Realm fixture did not settle its member locator")?;
    let partitions = ctx.db.game_group_member_partition();
    let mut partition = partitions
        .character_guid()
        .find(character_guid)
        .filter(|row| row.group_id == GROUP && row.member_active)
        .ok_or("Assist Realm fixture member partition is absent")?;
    partition.map_id = settled.map_id;
    partition.instance_id = settled.instance_id;
    partition.locator_revision = settled.revision;
    partition.state = crate::PartyPartitionState::Known;
    partitions.character_guid().update(partition);
    Ok(())
}

/// Move the already-settled party leader back to the open world before the companion uses the
/// audited Deadmines exit. The companion's locator remains at the revision produced by its real
/// entry Transfer; the Gateway advances it when the exit completes.
#[reducer]
pub fn playerbots_transfer_gateway_exit_realm_stage(
    ctx: &ReducerContext,
    companion_guid: u64,
    leader_guid: u64,
    priest_guid: u64,
    mage_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let party = exact_gateway_party(ctx, companion_guid, leader_guid, priest_guid, mage_guid)?;
    for (guid, map_id, instance_id, revision) in [
        (companion_guid, 36, 5_098_078, 2),
        (leader_guid, 36, 5_098_078, 2),
        (priest_guid, 0, 0, 1),
        (mage_guid, 0, 0, 1),
    ] {
        let locator = ctx
            .db
            .game_character_shard()
            .character_guid()
            .find(guid)
            .filter(|row| {
                (row.map_id, row.instance_id, row.revision) == (map_id, instance_id, revision)
                    && !row.transfer_pending
            });
        // Realm-core retains the companion's membership history here. The settled Character
        // locator and the certified World mirrors own its current partition after Transfer.
        let realm_partition_is_current = guid == companion_guid
            || exact_known_partition(&party, guid, map_id, instance_id, revision);
        if locator.is_none() || !realm_partition_is_current {
            return Err("Gateway exit fixture Realm location changed".to_string());
        }
    }
    crate::realm_core::record_shard(ctx, leader_guid, 0, 0); // package-api: exempt private fixture models the leader's completed return
    let settled = ctx
        .db
        .game_character_shard()
        .character_guid()
        .find(leader_guid)
        .filter(|row| {
            (row.map_id, row.instance_id, row.revision) == (0, 0, 3) && !row.transfer_pending
        })
        .ok_or("Gateway exit fixture did not advance the leader locator")?;
    let partitions = ctx.db.game_group_member_partition();
    let mut partition = partitions
        .character_guid()
        .find(leader_guid)
        .filter(|row| row.group_id == GROUP && row.member_active)
        .ok_or("Gateway exit fixture leader partition is absent")?;
    partition.map_id = settled.map_id;
    partition.instance_id = settled.instance_id;
    partition.locator_revision = settled.revision;
    partition.state = crate::PartyPartitionState::Known;
    partitions.character_guid().update(partition);
    Ok(())
}

/// Prepare the original World Shard as the destination for the companion's Deadmines exit. This
/// restores only the human leader's durable Character and party partition. The caller materializes
/// the leader through the shared body builder; the companion remains absent until Transfer.
#[reducer]
pub fn playerbots_transfer_gateway_exit_destination_stage(
    ctx: &ReducerContext,
    companion_guid: u64,
    leader_guid: u64,
    priest_guid: u64,
    mage_guid: u64,
    request_actor: crate::SessionActor,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let mut party = exact_gateway_party(ctx, companion_guid, leader_guid, priest_guid, mage_guid)?;
    if request_actor.guid != leader_guid
        || !exact_known_partition(&party, companion_guid, 36, 5_098_078, 2)
        || !exact_known_partition(&party, leader_guid, 36, 5_098_078, 2)
        || !exact_known_partition(&party, priest_guid, 0, 0, 1)
        || !exact_known_partition(&party, mage_guid, 0, 0, 1)
        || crate::helpers::character_by_guid(ctx, companion_guid).is_some()
        || ctx
            .db
            .game_world_entity()
            .guid()
            .find(companion_guid)
            .is_some()
        || ctx
            .db
            .pkg_playerbots_bot()
            .by_character()
            .filter(companion_guid)
            .next()
            .is_some()
    {
        return Err("Gateway exit destination is not the exact settled entry source".to_string());
    }
    let mut leader = crate::helpers::character_by_guid(ctx, leader_guid)
        .filter(|row| (row.map_id, row.pending_instance_id) == (36, 5_098_078))
        .ok_or("Gateway exit destination leader Character changed")?;
    if ctx
        .db
        .game_world_entity()
        .guid()
        .find(leader_guid)
        .is_some()
    {
        return Err("Gateway exit destination leader body already exists".to_string());
    }
    crate::account_ownership::require_actor(ctx, request_actor)?; // package-api: exempt private fixture checks exact destination authority before mutation
    leader.map_id = 0;
    leader.pending_instance_id = 0;
    (leader.x, leader.y, leader.z) = EXIT_LEADER_POSITION;
    leader.orientation = EXIT_ORIENTATION;
    ctx.db.game_character().guid().update(leader);

    let leader_partition = party
        .partitions
        .iter_mut()
        .find(|partition| partition.character_guid == leader_guid)
        .ok_or("Gateway exit destination leader partition is absent")?;
    leader_partition.map_id = 0;
    leader_partition.instance_id = 0;
    leader_partition.locator_revision = 3;
    crate::group::sync_group_mirror(
        ctx,
        GROUP,
        party.leader_guid,
        party.loot_method,
        party.loot_threshold,
        party.master_looter_guid,
        party.member_guids,
        request_actor,
        party.partitions,
        party.roster_revision,
    )?;
    Ok(())
}

/// Stage the same Realm roster as the generic Gateway fixture after the leader and selected Priest
/// have completed distinct crossings to the destination partition.
#[reducer]
#[allow(clippy::too_many_arguments)] // The fixture carries four identities and both partitions.
pub fn playerbots_transfer_gateway_assist_realm_stage(
    ctx: &ReducerContext,
    companion_guid: u64,
    leader_guid: u64,
    priest_guid: u64,
    mage_guid: u64,
    source_map: u32,
    source_instance: u64,
    destination_map: u32,
    destination_instance: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let _group = ctx
        .db
        .game_group()
        .group_id()
        .find(GROUP)
        .filter(|group| {
            group.leader_guid == leader_guid
                && group.loot_method == PARTY_LOOT_METHOD
                && group.loot_threshold == 2
                && group.rr_cursor == 0
                && group.master_looter_guid == 0
        })
        .ok_or("Assist Realm fixture requires the staged party")?;
    let _roster = ctx
        .db
        .game_group_roster_revision()
        .group_id()
        .find(GROUP)
        .filter(|roster| roster.revision == 1 && roster.active)
        .ok_or("Assist Realm fixture requires the staged roster")?;
    let mut members: Vec<_> = ctx
        .db
        .game_group_member()
        .by_group()
        .filter(&GROUP)
        .take(lyracore_shared::group::GROUP_MAX_MEMBERS + 1)
        .collect();
    members.sort_by_key(|member| member.character_guid);
    let mut expected = vec![companion_guid, leader_guid, priest_guid, mage_guid];
    expected.sort_unstable();
    if members.len() != expected.len()
        || members
            .iter()
            .map(|member| member.character_guid)
            .ne(expected.iter().copied())
    {
        return Err("Assist Realm fixture party members changed".to_string());
    }
    let partitions = ctx.db.game_group_member_partition();
    let current: Vec<_> = partitions
        .by_group()
        .filter(&GROUP)
        .take(lyracore_shared::group::GROUP_MAX_MEMBERS + 1)
        .collect();
    if current.len() != expected.len()
        || current.iter().any(|partition| {
            let (expected_partition, expected_revision) = if partition.character_guid == leader_guid
            {
                ((destination_map, destination_instance), 2)
            } else {
                ((source_map, source_instance), 1)
            };
            !expected.contains(&partition.character_guid)
                || !partition.member_active
                || partition.state != crate::PartyPartitionState::Known
                || partition.locator_revision != expected_revision
                || (partition.map_id, partition.instance_id) != expected_partition
        })
    {
        return Err("Assist Realm fixture member partitions changed".to_string());
    }
    let leader_locator = ctx
        .db
        .game_character_shard()
        .character_guid()
        .find(leader_guid)
        .filter(|row| {
            (row.map_id, row.instance_id) == (destination_map, destination_instance)
                && row.revision == 2
                && !row.transfer_pending
        });
    let source_locator = |guid| {
        ctx.db
            .game_character_shard()
            .character_guid()
            .find(guid)
            .filter(|row| {
                (row.map_id, row.instance_id) == (source_map, source_instance)
                    && row.revision == 1
                    && !row.transfer_pending
            })
    };
    if leader_locator.is_none()
        || source_locator(priest_guid).is_none()
        || source_locator(mage_guid).is_none()
    {
        return Err("Assist Realm fixture member locators changed".to_string());
    }
    restage_completed_member_crossing(
        ctx,
        priest_guid,
        source_map,
        source_instance,
        destination_map,
        destination_instance,
    )
}

/// Install or remove one fixture-owned equal-revision rules conflict. The first arrival mirror
/// refuses it, and the next real Gateway worker can succeed after the test removes it.
#[reducer]
pub fn playerbots_transfer_gateway_mirror_fault(
    ctx: &ReducerContext,
    enabled: bool,
    companion_guid: u64,
    leader_guid: u64,
    priest_guid: u64,
    mage_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let groups = ctx.db.game_group();
    let mut current = groups
        .group_id()
        .find(GROUP)
        .ok_or("Gateway Transfer mirror fault requires the exact mirrored party")?;
    let revision = ctx.db.game_group_roster_revision().group_id().find(GROUP);
    let members: Vec<_> = ctx
        .db
        .game_group_member()
        .by_group()
        .filter(&GROUP)
        .take(lyracore_shared::group::GROUP_MAX_MEMBERS + 1)
        .collect();
    let partitions: Vec<_> = ctx
        .db
        .game_group_member_partition()
        .by_group()
        .filter(&GROUP)
        .take(lyracore_shared::group::GROUP_MAX_MEMBERS + 1)
        .collect();
    let arrivals: Vec<_> = ctx
        .db
        .game_transfer_in()
        .by_character()
        .filter(&companion_guid)
        .take(2)
        .collect();
    let expected_method = if enabled {
        PARTY_LOOT_METHOD
    } else {
        FAULT_LOOT_METHOD
    };
    let mut member_guids: Vec<_> = members.iter().map(|member| member.character_guid).collect();
    member_guids.sort_unstable();
    let mut partition_guids: Vec<_> = partitions
        .iter()
        .map(|partition| partition.character_guid)
        .collect();
    partition_guids.sort_unstable();
    // Import has detached the transferring Character from this cached party. The real arrival
    // mirror must restore it after the injected equal-revision rules conflict is removed.
    let mut expected_retained_guids = vec![leader_guid, priest_guid, mage_guid];
    expected_retained_guids.sort_unstable();
    if (
        current.leader_guid,
        current.loot_method,
        current.loot_threshold,
        current.rr_cursor,
        current.master_looter_guid,
    ) != (leader_guid, expected_method, 2, 0, 0)
        || revision
            .as_ref()
            .is_none_or(|revision| revision.revision != 1 || !revision.active)
        || arrivals.len() != 1
        || arrivals[0].transfer_id != companion_guid
        || arrivals[0].bot_intent_id == 0
        || arrivals[0].bot_controller_generation == 0
        || arrivals[0].bot_intent_source == Identity::ZERO
        || arrivals[0].source_locator_revision == 0
        || member_guids != expected_retained_guids
        || partition_guids != expected_retained_guids
        || partitions.iter().any(|partition| {
            partition.group_id != GROUP
                || !partition.member_active
                || partition.character_guid == companion_guid
        })
    {
        return Err("Gateway Transfer fixture refuses to alter another party".to_string());
    }
    current.loot_method = if enabled {
        FAULT_LOOT_METHOD
    } else {
        PARTY_LOOT_METHOD
    };
    groups.group_id().update(current);
    Ok(())
}
