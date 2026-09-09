//! Private three-database authority and mirror-failure staging for Gateway Transfer cases.

use crate::{
    game_character_shard, game_group, game_group_member, game_group_member_partition,
    game_group_roster_revision,
};
use spacetimedb::{reducer, table, Identity, ReducerContext, Table};

const GROUP: u64 = 5_098_000;
const LEADER_MEMBER: u64 = 5_098_001;
const COMPANION_MEMBER: u64 = 5_098_002;
const PRIEST_MEMBER: u64 = 5_098_003;
const MAGE_MEMBER: u64 = 5_098_004;
const FAULT_CURSOR: u32 = u32::MAX;

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

/// Stage the Realm-owned party roster and settled member partitions used by the private Gateway
/// process. The World Shard fixtures stage their own mirrors separately.
#[reducer]
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
    if character_guids.iter().any(|guid| *guid == 0)
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
        || ctx
            .db
            .game_character_shard()
            .character_guid()
            .find(companion_guid)
            .is_some()
        || ctx
            .db
            .game_character_shard()
            .character_guid()
            .find(leader_guid)
            .is_some()
    {
        return Err("Gateway Transfer fixture requires fresh Realm rows".to_string());
    }
    let locators = ctx.db.game_character_shard();

    ctx.db.game_group().insert(crate::Group {
        group_id: GROUP,
        leader_guid,
        loot_method: 3,
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
        destination_map,
        destination_instance,
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
    locators.insert(locator(
        leader_guid,
        destination_map,
        destination_instance,
        now,
    ));
    Ok(())
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
    let expected_cursor = if enabled { 0 } else { FAULT_CURSOR };
    let mut member_guids: Vec<_> = members.iter().map(|member| member.character_guid).collect();
    member_guids.sort_unstable();
    let mut expected_guids = vec![companion_guid, leader_guid, priest_guid, mage_guid];
    expected_guids.sort_unstable();
    if (
        current.leader_guid,
        current.loot_method,
        current.loot_threshold,
        current.rr_cursor,
        current.master_looter_guid,
    ) != (leader_guid, 3, 2, expected_cursor, 0)
        || revision
            .as_ref()
            .is_none_or(|revision| revision.revision != 1 || !revision.active)
        || member_guids != expected_guids
        || partitions.len() != 4
        || partitions.iter().any(|partition| {
            partition.group_id != GROUP
                || !partition.member_active
                || !expected_guids.contains(&partition.character_guid)
        })
    {
        return Err("Gateway Transfer fixture refuses to alter another party".to_string());
    }
    current.rr_cursor = if enabled { FAULT_CURSOR } else { 0 };
    groups.group_id().update(current);
    Ok(())
}
