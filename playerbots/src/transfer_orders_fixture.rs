//! Private party inputs for order decisions around the audited Deadmines entrance.

use super::pkg_playerbots_bot;
use crate::game_creature_move_schedule;
use crate::{
    game_area_trigger, game_areatrigger_teleport, game_character, game_group,
    game_group_member_partition, game_group_roster_revision, game_world_entity,
};
use spacetimedb::{reducer, ReducerContext, ScheduleAt, Table, TimeDuration};

const GROUP: u64 = 5_098_000;
const INSTANCE: u64 = 5_098_078;
const SOURCE: (f32, f32, f32) = (-11208.5, 1685.34, 25.7612);
const LANDING: (f32, f32, f32, f32) = (-14.5732, -385.475, 62.4561, 1.5708);

fn place_body(
    ctx: &ReducerContext,
    guid: u64,
    map: u32,
    instance: u64,
    position: (f32, f32, f32),
) -> Result<(), String> {
    let (x, y, z) = position;
    let mut entity = crate::helpers::live_entity(ctx, guid)?;
    entity.map_id = map;
    entity.instance_id = instance;
    (entity.x, entity.y, entity.z) = (x, y, z);
    let (gx, gy) = lyracore_shared::spatial::grid_cell(x, y);
    entity.grid_x = gx;
    entity.grid_y = gy;
    entity.cell = lyracore_shared::spatial::grid_cell_id(gx, gy);
    ctx.db.game_world_entity().guid().update(entity);
    Ok(())
}

/// Extend the existing three-role fixture to the required human and four-bot party.
#[reducer]
pub fn playerbots_transfer_orders_stage(
    ctx: &ReducerContext,
    warrior: u64,
    priest: u64,
    mage: u64,
    second_mage: u64,
    leader: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let guids = [warrior, priest, mage, second_mage, leader];
    if guids
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        != 5
    {
        return Err("order Transfer fixture requires five distinct Characters".to_string());
    }
    let extra = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(second_mage)
        .next()
        .filter(|bot| bot.class == super::class::MAGE && bot.role == super::ROLE_DPS)
        .ok_or("order Transfer fixture needs a second Mage")?;
    if ctx.db.game_area_trigger().id().find(78).is_some()
        || ctx
            .db
            .game_areatrigger_teleport()
            .trigger_id()
            .find(78)
            .is_some()
    {
        return Err("order Transfer fixture refuses an existing entry route".to_string());
    }
    super::fixture::playerbots_fixture_roles_stage(ctx, warrior, priest, mage, leader)?;
    let mut partitions: Vec<_> = ctx
        .db
        .game_group_member_partition()
        .by_group()
        .filter(&GROUP)
        .take(lyracore_shared::group::GROUP_MAX_MEMBERS + 1)
        .collect();
    if partitions.len() != 4 {
        return Err("order Transfer fixture needs the exact initial role party".to_string());
    }
    let membership_revision = partitions
        .iter()
        .map(|p| p.membership_revision)
        .max()
        .unwrap()
        .checked_add(1)
        .ok_or("fixture membership revision exhausted")?;
    partitions.push(crate::GroupMemberPartition {
        character_guid: second_mage,
        group_id: GROUP,
        membership_revision,
        member_active: true,
        map_id: 0,
        instance_id: 0,
        locator_revision: 1,
        state: crate::PartyPartitionState::Known,
    });
    partitions.sort_by_key(|p| p.membership_revision);
    let group = ctx
        .db
        .game_group()
        .group_id()
        .find(GROUP)
        .ok_or("fixture party missing")?;
    let revision = ctx
        .db
        .game_group_roster_revision()
        .group_id()
        .find(GROUP)
        .ok_or("fixture roster revision missing")?
        .revision
        .checked_add(1)
        .ok_or("fixture roster revision exhausted")?;
    crate::group::sync_group_mirror(
        ctx,
        GROUP,
        leader,
        group.loot_method,
        group.loot_threshold,
        group.master_looter_guid,
        partitions.iter().map(|p| p.character_guid).collect(),
        crate::SessionActor {
            guid: leader,
            ownership: None,
        },
        partitions,
        revision,
    )?;
    super::runner::playerbots_select_controller(
        ctx,
        extra.character_guid,
        super::Controller::Cohort,
    )?;
    for (index, guid) in guids.into_iter().enumerate() {
        super::fixture::playerbots_fixture_companion_health(ctx, guid, 100)?;
        place_body(
            ctx,
            guid,
            0,
            0,
            (SOURCE.0 + index as f32 * 0.25, SOURCE.1, SOURCE.2),
        )?;
        if guid != leader {
            super::fixture::playerbots_fixture_freeze(ctx, guid)?;
        }
    }
    for (index, entry) in [5_098_001u32, 5_098_002, 5_098_003].into_iter().enumerate() {
        let guid = (0xF130u64 << 48) | (u64::from(entry) << 24) | 1;
        place_body(
            ctx,
            guid,
            0,
            0,
            (SOURCE.0 + 100.0, SOURCE.1 + index as f32, SOURCE.2),
        )?;
    }
    ctx.db.game_area_trigger().insert(crate::GameAreaTrigger {
        id: 78,
        map_id: 0,
        x: SOURCE.0,
        y: SOURCE.1,
        z: SOURCE.2,
        radius: 7.0,
        box_length: 0.0,
        box_width: 0.0,
        box_height: 0.0,
        box_yaw: 0.0,
    });
    ctx.db
        .game_areatrigger_teleport()
        .insert(crate::AreatriggerTeleport {
            trigger_id: 78,
            target_map: 36,
            x: LANDING.0,
            y: LANDING.1,
            z: LANDING.2,
            o: LANDING.3,
            name: "Deadmines - Entering".to_string(),
        });
    // Keep the first real Target leg pending while the caller records and changes partition facts.
    let schedules = ctx.db.game_creature_move_schedule(); // package-api: exempt private fixture declares the Core movement tick before orders begin
    let mut ticks: Vec<_> = schedules.iter().take(2).collect();
    if ticks.len() != 1 || ticks[0].instance_id != u64::MAX {
        return Err("order Transfer fixture requires one fresh movement tick".to_string());
    }
    let at = ctx
        .timestamp
        .checked_add(TimeDuration::from_micros(60_000_000))
        .ok_or("fixture movement tick timestamp exhausted")?;
    let mut tick = ticks.remove(0);
    tick.scheduled_at = ScheduleAt::Time(at);
    schedules.scheduled_id().update(tick);
    Ok(())
}

/// Supply a later Known partition through the normal mirror operation. The Gateway process
/// scenario separately proves where these Realm inputs come from.
#[reducer]
pub fn playerbots_transfer_orders_remote(
    ctx: &ReducerContext,
    request_actor: crate::SessionActor,
    priest: u64,
    target: u64,
    mode: u8,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    if mode > 2 {
        return Err("unknown order Transfer fixture mode".to_string());
    }
    let leader = request_actor.guid;
    let group = ctx
        .db
        .game_group()
        .group_id()
        .find(GROUP)
        .filter(|group| group.leader_guid == leader)
        .ok_or("fixture party leader changed")?;
    let mut partitions: Vec<_> = ctx
        .db
        .game_group_member_partition()
        .by_group()
        .filter(&GROUP)
        .take(lyracore_shared::group::GROUP_MAX_MEMBERS + 1)
        .collect();
    if partitions.len() != 5 || !partitions.iter().any(|p| p.character_guid == priest) {
        return Err("order Transfer fixture requires its five-member party".to_string());
    }
    let ensure_instance = crate::instance::ensure_instance; // package-api: exempt private fixture stages admitted instance
    ensure_instance(ctx, INSTANCE, 36, GROUP, request_actor)?;
    let admitted = crate::instance::resolve_or_create_instance(ctx, leader, 36); // package-api: exempt private fixture stages normal leader admission
    if admitted? != INSTANCE {
        return Err("fixture admitted another instance".to_string());
    }
    for guid in [Some(leader), (mode == 1).then_some(priest)]
        .into_iter()
        .flatten()
    {
        let body = crate::helpers::live_entity(ctx, guid)?;
        crate::world::remove_live_character(ctx, body); // package-api: exempt private fixture declares remote party member
        let mut character =
            crate::helpers::character_by_guid(ctx, guid).ok_or("fixture Character missing")?;
        character.map_id = 36;
        character.pending_instance_id = INSTANCE;
        (character.x, character.y, character.z) = (LANDING.0, LANDING.1, LANDING.2);
        character.orientation = LANDING.3;
        ctx.db.game_character().guid().update(character);
        let partition = partitions
            .iter_mut()
            .find(|p| p.character_guid == guid)
            .ok_or("fixture member partition missing")?;
        partition.map_id = 36;
        partition.instance_id = INSTANCE;
        partition.locator_revision = partition
            .locator_revision
            .checked_add(1)
            .ok_or("fixture locator revision exhausted")?;
    }
    if mode == 2 {
        let entity = crate::helpers::live_entity(ctx, target)?;
        if entity.entry != 5_098_001 || entity.is_player() {
            return Err("fixture refuses another designated target".to_string());
        }
        place_body(
            ctx,
            target,
            36,
            INSTANCE,
            (LANDING.0 + 8.0, LANDING.1, LANDING.2),
        )?;
    }
    partitions.sort_by_key(|p| p.membership_revision);
    let revision = ctx
        .db
        .game_group_roster_revision()
        .group_id()
        .find(GROUP)
        .ok_or("fixture roster revision missing")?
        .revision;
    crate::group::sync_group_mirror(
        ctx,
        GROUP,
        leader,
        group.loot_method,
        group.loot_threshold,
        group.master_looter_guid,
        partitions.iter().map(|p| p.character_guid).collect(),
        request_actor,
        partitions,
        revision,
    )
}
