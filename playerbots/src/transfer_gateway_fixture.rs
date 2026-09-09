//! Private three-database authority and mirror-failure staging for Gateway Transfer cases.

use crate::{
    game_character_shard, game_group, game_group_member, game_group_member_partition,
    game_group_roster_revision,
};
use spacetimedb::{reducer, Identity, ReducerContext, Table};

const GROUP: u64 = 5_098_000;
const LEADER_MEMBER: u64 = 5_098_001;
const COMPANION_MEMBER: u64 = 5_098_002;
const FAULT_CURSOR: u32 = u32::MAX;

fn remove_fixture_group(ctx: &ReducerContext) -> Result<(), String> {
    let members: Vec<_> = ctx
        .db
        .game_group_member()
        .by_group()
        .filter(&GROUP)
        .take(lyracore_shared::group::GROUP_MAX_MEMBERS + 1)
        .collect();
    if members.len() > lyracore_shared::group::GROUP_MAX_MEMBERS {
        return Err("Gateway Transfer fixture party exceeds its member bound".to_string());
    }
    for member in members {
        ctx.db.game_group_member().id().delete(member.id);
    }
    let partitions: Vec<_> = ctx
        .db
        .game_group_member_partition()
        .by_group()
        .filter(&GROUP)
        .take(lyracore_shared::group::GROUP_MAX_MEMBERS * 2 + 1)
        .collect();
    if partitions.len() > lyracore_shared::group::GROUP_MAX_MEMBERS * 2 {
        return Err("Gateway Transfer fixture partitions exceed their bound".to_string());
    }
    for partition in partitions {
        ctx.db
            .game_group_member_partition()
            .character_guid()
            .delete(partition.character_guid);
    }
    ctx.db.game_group().group_id().delete(GROUP);
    ctx.db.game_group_roster_revision().group_id().delete(GROUP);
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
    source_map: u32,
    source_instance: u64,
    destination_map: u32,
    destination_instance: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    if companion_guid == 0 || leader_guid == 0 || companion_guid == leader_guid {
        return Err("Gateway Transfer fixture requires two Characters".to_string());
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
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    if enabled {
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
        {
            return Err("Gateway Transfer mirror fault requires a fresh party id".to_string());
        }
        ctx.db.game_group().insert(crate::Group {
            group_id: GROUP,
            leader_guid: 1,
            loot_method: 3,
            loot_threshold: 2,
            rr_cursor: FAULT_CURSOR,
            master_looter_guid: 0,
        });
        ctx.db
            .game_group_roster_revision()
            .insert(crate::GroupRosterRevision {
                group_id: GROUP,
                revision: 1,
                active: true,
            });
        return Ok(());
    }
    let current = ctx.db.game_group().group_id().find(GROUP);
    let revision = ctx.db.game_group_roster_revision().group_id().find(GROUP);
    let has_members = ctx
        .db
        .game_group_member()
        .by_group()
        .filter(&GROUP)
        .next()
        .is_some();
    let has_partitions = ctx
        .db
        .game_group_member_partition()
        .by_group()
        .filter(&GROUP)
        .next()
        .is_some();
    if current.as_ref().is_none_or(|group| {
        (
            group.leader_guid,
            group.loot_method,
            group.loot_threshold,
            group.rr_cursor,
            group.master_looter_guid,
        ) != (1, 3, 2, FAULT_CURSOR, 0)
    }) || revision
        .as_ref()
        .is_none_or(|revision| revision.revision != 1 || !revision.active)
        || has_members
        || has_partitions
    {
        return Err("Gateway Transfer fixture refuses to remove another party".to_string());
    }
    remove_fixture_group(ctx)
}
