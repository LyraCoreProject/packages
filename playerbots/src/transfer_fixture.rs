//! Private AreaTrigger staging for durable Transfer cases.

use crate::{
    game_area_trigger, game_areatrigger_teleport, game_character, game_group, game_group_member,
    game_group_member_partition, game_group_roster_revision, game_world_entity,
};
use spacetimedb::{reducer, ReducerContext, Table};

const GROUP: u64 = 5_098_000;
const TRIGGER: u32 = 78;
const DESTINATION_INSTANCE: u64 = 5_098_078;
const SOURCE: (f32, f32, f32) = (1208.0, 1200.0, 50.0);
const LANDING: (f32, f32, f32, f32) = (-14.5732, -385.475, 62.4561, 1.5708);

/// Put the private role-fixture leader in Deadmines and optionally declare the source-side portal.
/// Mode 0 omits the route, mode 1 leaves the companion outside it, and mode 2 starts inside it.
#[reducer]
pub fn playerbots_transfer_fixture_stage(
    ctx: &ReducerContext,
    companion_guid: u64,
    leader_guid: u64,
    mode: u8,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    if mode > 2 {
        return Err("unknown Transfer fixture mode".to_string());
    }
    let group = ctx
        .db
        .game_group()
        .group_id()
        .find(GROUP)
        .filter(|group| group.leader_guid == leader_guid)
        .ok_or("Transfer fixture party missing")?;
    let members: Vec<_> = ctx
        .db
        .game_group_member()
        .by_group()
        .filter(&GROUP)
        .take(crate::group::GROUP_MAX_MEMBERS + 1)
        .collect();
    if members.len() > crate::group::GROUP_MAX_MEMBERS {
        return Err("Transfer fixture party exceeds the supported member bound".to_string());
    }
    if !members
        .iter()
        .any(|member| member.character_guid == companion_guid)
        || !members
            .iter()
            .any(|member| member.character_guid == leader_guid)
    {
        return Err("Transfer fixture Characters are outside its party".to_string());
    }
    if ctx.db.game_area_trigger().id().find(TRIGGER).is_some()
        || ctx
            .db
            .game_areatrigger_teleport()
            .trigger_id()
            .find(TRIGGER)
            .is_some()
    {
        return Err("Transfer fixture refuses to replace imported AreaTrigger 78".to_string());
    }
    if mode != 0 {
        ctx.db.game_area_trigger().insert(crate::GameAreaTrigger {
            id: TRIGGER,
            map_id: 0,
            x: SOURCE.0,
            y: SOURCE.1,
            z: SOURCE.2,
            radius: 2.0,
            box_length: 0.0,
            box_width: 0.0,
            box_height: 0.0,
            box_yaw: 0.0,
        });
        ctx.db
            .game_areatrigger_teleport()
            .insert(crate::AreatriggerTeleport {
                trigger_id: TRIGGER,
                target_map: 36,
                x: LANDING.0,
                y: LANDING.1,
                z: LANDING.2,
                o: LANDING.3,
                name: "Deadmines - Entering (private Transfer fixture)".to_string(),
            });
    }

    if mode == 2 {
        let entities = ctx.db.game_world_entity();
        let mut companion = entities
            .guid()
            .find(companion_guid)
            .ok_or("Transfer fixture companion body missing")?;
        (companion.x, companion.y, companion.z) = SOURCE;
        let (grid_x, grid_y) = lyracore_shared::spatial::grid_cell(companion.x, companion.y);
        companion.grid_x = grid_x;
        companion.grid_y = grid_y;
        companion.cell = lyracore_shared::spatial::grid_cell_id(grid_x, grid_y);
        entities.guid().update(companion);
    }

    let ensure_instance = crate::instance::ensure_instance; // package-api: exempt private fixture stages the admitted party instance
    ensure_instance(
        ctx,
        DESTINATION_INSTANCE,
        36,
        GROUP,
        crate::SessionActor {
            guid: leader_guid,
            ownership: None,
        },
    )?;
    let leader_instance = crate::instance::resolve_or_create_instance(ctx, leader_guid, 36); // package-api: exempt private fixture stages the leader's ordinary instance binding
    if leader_instance? != DESTINATION_INSTANCE {
        return Err("Transfer fixture leader resolved another instance".to_string());
    }

    let leader = crate::helpers::live_entity(ctx, leader_guid)?;
    crate::world::remove_live_character(ctx, leader); // package-api: exempt private fixture models a completed leader Transfer
    let characters = ctx.db.game_character();
    let mut leader = characters
        .guid()
        .find(leader_guid)
        .ok_or("Transfer fixture leader Character missing")?;
    leader.map_id = 36;
    leader.pending_instance_id = DESTINATION_INSTANCE;
    leader.x = LANDING.0;
    leader.y = LANDING.1;
    leader.z = LANDING.2;
    leader.orientation = LANDING.3;
    characters.guid().update(leader);

    let mut partitions: Vec<_> = members
        .iter()
        .map(|member| {
            ctx.db
                .game_group_member_partition()
                .character_guid()
                .find(member.character_guid)
                .ok_or("Transfer fixture member partition missing")
        })
        .collect::<Result<_, _>>()?;
    let leader_partition = partitions
        .iter_mut()
        .find(|partition| partition.character_guid == leader_guid)
        .ok_or("Transfer fixture leader partition missing")?;
    leader_partition.map_id = 36;
    leader_partition.instance_id = DESTINATION_INSTANCE;
    leader_partition.locator_revision = leader_partition
        .locator_revision
        .checked_add(1)
        .ok_or("Transfer fixture locator revision exhausted")?;
    let roster_revision = ctx
        .db
        .game_group_roster_revision()
        .group_id()
        .find(GROUP)
        .ok_or("Transfer fixture roster revision missing")?
        .revision;
    crate::group::sync_group_mirror(
        ctx,
        GROUP,
        leader_guid,
        group.loot_method,
        group.loot_threshold,
        group.master_looter_guid,
        members.iter().map(|member| member.character_guid).collect(),
        crate::SessionActor {
            guid: leader_guid,
            ownership: None,
        },
        partitions,
        roster_revision,
    )?;
    Ok(())
}
