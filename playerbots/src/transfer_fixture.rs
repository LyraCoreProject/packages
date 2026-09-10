//! Private AreaTrigger staging for durable Transfer cases.

use crate::{
    game_area_trigger, game_areatrigger_teleport, game_character, game_group, game_group_member,
    game_group_member_partition, game_group_roster_revision, game_world_entity,
};
use spacetimedb::{reducer, ReducerContext, Table};

const GROUP: u64 = 5_098_000;
const ENTRY_TRIGGER: u32 = 78;
const EXIT_TRIGGER: u32 = 119;
const DESTINATION_INSTANCE: u64 = 5_098_078;
const ENTRY_SOURCE: (f32, f32, f32) = (1208.0, 1200.0, 50.0);
#[allow(clippy::approx_constant)] // Exact imported AreaTrigger landing orientation.
const ENTRY_LANDING: (f32, f32, f32, f32) = (-14.5732, -385.475, 62.4561, 1.5708);
const EXIT_SOURCE: (f32, f32, f32) = (-14.3628, -393.38, 64.5605);
const EXIT_LANDING: (f32, f32, f32, f32) = (-11208.7, 1675.9, 24.5733, 4.71239);

#[derive(Clone, Copy)]
struct FixtureRoute {
    trigger: u32,
    source_map: u32,
    source: (f32, f32, f32),
    radius: f32,
    target_map: u32,
    landing: (f32, f32, f32, f32),
    name: &'static str,
}

fn fixture_route(mode: u8) -> Result<(FixtureRoute, bool), String> {
    let entry = FixtureRoute {
        trigger: ENTRY_TRIGGER,
        source_map: 0,
        source: ENTRY_SOURCE,
        radius: 2.0,
        target_map: 36,
        landing: ENTRY_LANDING,
        name: "Deadmines - Entering (private Transfer fixture)",
    };
    match mode {
        0 => Ok((entry, false)),
        1 | 2 => Ok((entry, true)),
        3 => Ok((
            FixtureRoute {
                trigger: EXIT_TRIGGER,
                source_map: 36,
                source: EXIT_SOURCE,
                radius: 6.0,
                target_map: 0,
                landing: EXIT_LANDING,
                name: "Deadmines - Leaving (private Transfer fixture)",
            },
            true,
        )),
        _ => Err("unknown Transfer fixture mode".to_string()),
    }
}

fn declare_route(ctx: &ReducerContext, route: FixtureRoute) -> Result<(), String> {
    if ctx
        .db
        .game_area_trigger()
        .id()
        .find(route.trigger)
        .is_some()
        || ctx
            .db
            .game_areatrigger_teleport()
            .trigger_id()
            .find(route.trigger)
            .is_some()
    {
        return Err(format!(
            "Transfer fixture refuses to replace imported AreaTrigger {}",
            route.trigger
        ));
    }
    ctx.db.game_area_trigger().insert(crate::GameAreaTrigger {
        id: route.trigger,
        map_id: route.source_map,
        x: route.source.0,
        y: route.source.1,
        z: route.source.2,
        radius: route.radius,
        box_length: 0.0,
        box_width: 0.0,
        box_height: 0.0,
        box_yaw: 0.0,
    });
    ctx.db
        .game_areatrigger_teleport()
        .insert(crate::AreatriggerTeleport {
            trigger_id: route.trigger,
            target_map: route.target_map,
            x: route.landing.0,
            y: route.landing.1,
            z: route.landing.2,
            o: route.landing.3,
            name: route.name.to_string(),
        });
    Ok(())
}

fn place_live(
    ctx: &ReducerContext,
    guid: u64,
    map_id: u32,
    instance_id: u64,
    position: (f32, f32, f32),
) -> Result<(), String> {
    let entities = ctx.db.game_world_entity();
    let mut entity = entities
        .guid()
        .find(guid)
        .ok_or("Transfer fixture body missing")?;
    entity.map_id = map_id;
    entity.instance_id = instance_id;
    (entity.x, entity.y, entity.z) = position;
    let (grid_x, grid_y) = lyracore_shared::spatial::grid_cell(entity.x, entity.y);
    entity.grid_x = grid_x;
    entity.grid_y = grid_y;
    entity.cell = lyracore_shared::spatial::grid_cell_id(grid_x, grid_y);
    entities.guid().update(entity);
    Ok(())
}

fn place_character(
    ctx: &ReducerContext,
    guid: u64,
    map_id: u32,
    instance_id: u64,
    landing: (f32, f32, f32, f32),
) -> Result<(), String> {
    let mut character =
        crate::helpers::character_by_guid(ctx, guid).ok_or("Transfer fixture Character missing")?;
    character.map_id = map_id;
    character.pending_instance_id = instance_id;
    character.x = landing.0;
    character.y = landing.1;
    character.z = landing.2;
    character.orientation = landing.3;
    ctx.db.game_character().guid().update(character);
    Ok(())
}

fn move_partition(
    partitions: &mut [crate::group::GroupMemberPartition],
    guid: u64,
    map_id: u32,
    instance_id: u64,
) -> Result<(), String> {
    let partition = partitions
        .iter_mut()
        .find(|partition| partition.character_guid == guid)
        .ok_or("Transfer fixture member partition missing")?;
    if (partition.map_id, partition.instance_id) != (map_id, instance_id) {
        partition.map_id = map_id;
        partition.instance_id = instance_id;
        partition.locator_revision = partition
            .locator_revision
            .checked_add(1)
            .ok_or("Transfer fixture locator revision exhausted")?;
    }
    Ok(())
}

/// Declare the audited entry and admitted instance while the command issuer and selected member
/// are still local. A later fixture step may then model their completed crossings without a second
/// route writer.
#[reducer]
pub fn playerbots_transfer_fixture_entry_route_stage(
    ctx: &ReducerContext,
    companion_guid: u64,
    leader_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let group = ctx
        .db
        .game_group()
        .group_id()
        .find(GROUP)
        .filter(|group| group.leader_guid == leader_guid)
        .ok_or("Transfer entry fixture party missing")?;
    let members: Vec<_> = ctx
        .db
        .game_group_member()
        .by_group()
        .filter(&GROUP)
        .take(crate::group::GROUP_MAX_MEMBERS + 1)
        .collect();
    if members.len() > crate::group::GROUP_MAX_MEMBERS
        || !members
            .iter()
            .any(|member| member.character_guid == companion_guid)
    {
        return Err("Transfer entry fixture companion is outside its bounded party".to_string());
    }
    let companion = crate::helpers::live_entity(ctx, companion_guid)?;
    let leader = crate::helpers::live_entity(ctx, leader_guid)?;
    if (companion.map_id, companion.instance_id) != (leader.map_id, leader.instance_id) {
        return Err("Transfer entry fixture requires the local command partition".to_string());
    }
    let (route, _) = fixture_route(2)?;
    declare_route(ctx, route)?;
    place_live(ctx, companion_guid, 0, 0, ENTRY_SOURCE)?;
    let actor = crate::SessionActor {
        guid: leader_guid,
        ownership: None,
    };
    let ensure_instance = crate::instance::ensure_instance; // package-api: exempt private fixture stages admitted instance
    ensure_instance(ctx, DESTINATION_INSTANCE, 36, GROUP, actor)?;
    let resolved = crate::instance::resolve_or_create_instance(ctx, group.leader_guid, 36); // package-api: exempt private fixture verifies admitted instance
    if resolved? != DESTINATION_INSTANCE {
        return Err("Transfer entry fixture resolved another instance".to_string());
    }
    Ok(())
}

/// Put the private role-fixture leader in Deadmines and optionally declare the source-side portal.
/// Mode 0 omits the entry route, mode 1 leaves the companion outside it, mode 2 starts inside it,
/// and mode 3 starts inside the imported Deadmines exit sphere.
#[reducer]
pub fn playerbots_transfer_fixture_stage(
    ctx: &ReducerContext,
    companion_guid: u64,
    leader_guid: u64,
    mode: u8,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    stage_transfer_fixture(
        ctx,
        companion_guid,
        leader_guid,
        mode,
        crate::SessionActor {
            guid: leader_guid,
            ownership: None,
        },
    )
}

/// Use the same private route fixture after the caller has claimed the party leader.
#[reducer]
pub fn playerbots_transfer_fixture_stage_authenticated(
    ctx: &ReducerContext,
    companion_guid: u64,
    leader_guid: u64,
    mode: u8,
    request_actor: crate::SessionActor,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    if request_actor.guid != leader_guid {
        return Err("Transfer fixture authority does not own the party leader".to_string());
    }
    crate::account_ownership::require_actor(ctx, request_actor)?; // package-api: exempt private fixture checks exact entry authority before mutation
    stage_transfer_fixture(ctx, companion_guid, leader_guid, mode, request_actor)
}

fn stage_transfer_fixture(
    ctx: &ReducerContext,
    companion_guid: u64,
    leader_guid: u64,
    mode: u8,
    request_actor: crate::SessionActor,
) -> Result<(), String> {
    let (route, declared) = fixture_route(mode)?;
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
    if declared {
        declare_route(ctx, route)?;
    }

    if mode == 2 {
        place_live(ctx, companion_guid, 0, 0, ENTRY_SOURCE)?;
    }

    let ensure_instance = crate::instance::ensure_instance; // package-api: exempt private fixture stages the admitted party instance
    ensure_instance(ctx, DESTINATION_INSTANCE, 36, GROUP, request_actor)?;
    let bound_guid = if mode == 3 {
        companion_guid
    } else {
        leader_guid
    };
    let admitted_instance = crate::instance::resolve_or_create_instance(ctx, bound_guid, 36); // package-api: exempt private fixture stages an ordinary party instance binding
    if admitted_instance? != DESTINATION_INSTANCE {
        return Err("Transfer fixture resolved another instance".to_string());
    }

    if mode == 3 {
        place_live(ctx, companion_guid, 36, DESTINATION_INSTANCE, EXIT_SOURCE)?;
        place_character(
            ctx,
            companion_guid,
            36,
            DESTINATION_INSTANCE,
            (EXIT_SOURCE.0, EXIT_SOURCE.1, EXIT_SOURCE.2, 0.0),
        )?;
    }

    let leader = crate::helpers::live_entity(ctx, leader_guid)?;
    crate::world::remove_live_character(ctx, leader); // package-api: exempt private fixture models a completed leader Transfer
    let leader_instance = if route.target_map == 36 {
        DESTINATION_INSTANCE
    } else {
        0
    };
    place_character(
        ctx,
        leader_guid,
        route.target_map,
        leader_instance,
        route.landing,
    )?;

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
    move_partition(
        &mut partitions,
        leader_guid,
        route.target_map,
        leader_instance,
    )?;
    if mode == 3 {
        move_partition(&mut partitions, companion_guid, 36, DESTINATION_INSTANCE)?;
    }
    let roster_revision = ctx
        .db
        .game_group_roster_revision()
        .group_id()
        .find(GROUP)
        .ok_or("Transfer fixture roster revision missing")?
        .revision;
    let accepted_members: Vec<_> = partitions
        .iter()
        .map(|partition| (partition.membership_revision, partition.character_guid))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|(_, character_guid)| character_guid)
        .collect();
    crate::group::sync_group_mirror(
        ctx,
        GROUP,
        leader_guid,
        group.loot_method,
        group.loot_threshold,
        group.master_looter_guid,
        accepted_members,
        request_actor,
        partitions,
        roster_revision,
    )?;
    Ok(())
}
