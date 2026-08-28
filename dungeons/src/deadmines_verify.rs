//! Deadmines verification harness (`debug_reducers` only): stage reducers the durable test
//! (`module/tests/deadmines_package.rs`) calls in order over the standalone CLI. Every reducer is
//! argument-free — fixture guids exceed the CLI's safe integer range, so each stage resolves its
//! own subjects server-side, drives the REAL production path (kill, damage, gameobject use), and
//! verifies the durable outcome in the same call.

#[cfg(feature = "debug_reducers")]
use spacetimedb::{reducer, ReducerContext, Table};

#[cfg(feature = "debug_reducers")]
use crate::encounter::{self, DOOR_OPEN_STATE, ENCOUNTER_DONE};
#[cfg(feature = "debug_reducers")]
use super::deadmines::{
    DEFIAS_CANNON, ENCOUNTER_CANNON, ENCOUNTER_GILNID, ENCOUNTER_RHAHKZOR, ENCOUNTER_SMITE,
    ENCOUNTER_SNEED, FACTORY_DOOR, FOUNDRY_DOOR, GILNID, IRON_CLAD_DOOR, MAP_ID, MAST_ROOM_DOOR,
    MR_SMITE, RHAHKZOR, SMITES_CHEST, SMITES_MIGHTY_HAMMER, SMITES_REAVER, SMITE_FIRST_STAND_PCT,
    SMITE_SECOND_STAND_PCT, SNEED, SNEEDS_SHREDDER,
};
#[cfg(feature = "debug_reducers")]
use crate::{
    game_chat_event, game_creature_spline, game_creature_template, game_encounter_equip,
    game_gameobject, game_gameobject_template, game_instance, game_item_template,
    game_world_entity, GameInstance,
};

#[cfg(feature = "debug_reducers")]
const FIXTURE_LOW_BAND: u64 = 0x10_0000;
#[cfg(feature = "debug_reducers")]
const DEADMINES_INSTANCE: u64 = 936;
#[cfg(feature = "debug_reducers")]
const GO_FIXTURE_BAND: u64 = 0xF11D << 48;

/// Seed a Deadmines fixture instance: doors, cannon, chest, Smite's weapons, the bosses, and one
/// player. Positions come from the classic-db z2815 spawn rows.
#[cfg(feature = "debug_reducers")]
#[reducer]
pub fn debug_deadmines_begin(ctx: &ReducerContext) -> Result<(), String> {
    install_instance(ctx, MAP_ID, DEADMINES_INSTANCE)?;
    for (entry, type_id, display_id, x, y, z, orientation) in [
        (FACTORY_DOOR, 0u8, 394u32, -191.414f32, -457.446f32, 54.4391f32, 1.69297f32),
        (MAST_ROOM_DOOR, 0, 394, -290.294, -536.96, 49.4353, 1.55334),
        (FOUNDRY_DOOR, 0, 394, -168.514, -579.861, 19.3159, 3.12412),
        (IRON_CLAD_DOOR, 0, 394, -100.502, -668.771, 7.41049, 1.81514),
        (DEFIAS_CANNON, 10, 245, -107.562, -659.674, 7.21211, -0.890294),
        (SMITES_CHEST, 5, 259, 2.69086, -781.633, 9.76985, 2.33874),
    ] {
        seed_gameobject(ctx, entry, type_id, display_id, x, y, z, orientation)?;
    }
    seed_smite_weapon(ctx, SMITES_REAVER, "Smite's Reaver", 13913)?;
    seed_smite_weapon(ctx, SMITES_MIGHTY_HAMMER, "Smite's Mighty Hammer", 19610)?;
    seed_sneed_template(ctx)?;

    let rhahkzor = spawn_source(ctx, RHAHKZOR, MAP_ID, DEADMINES_INSTANCE, false, 40)?;
    set_fixture_position(ctx, rhahkzor, -192.5, -453.0, 54.4)?;
    let shredder = spawn_source(ctx, SNEEDS_SHREDDER, MAP_ID, DEADMINES_INSTANCE, false, 41)?;
    set_fixture_position(ctx, shredder, -289.0, -531.0, 49.4)?;
    let gilnid = spawn_source(ctx, GILNID, MAP_ID, DEADMINES_INSTANCE, false, 42)?;
    set_fixture_position(ctx, gilnid, -166.0, -576.0, 19.3)?;
    let smite = spawn_source(ctx, MR_SMITE, MAP_ID, DEADMINES_INSTANCE, false, 43)?;
    set_fixture_position(ctx, smite, -2.5, -757.4, 9.8)?;
    let player = spawn_fixture_player(ctx, MAP_ID, DEADMINES_INSTANCE, 44)?;
    set_fixture_position(ctx, player, -5.0, -760.0, 9.8)?;

    // The real fixture registers these through the Package's `on_creature_spawn` hook when Smite
    // spawns via the production path; `spawn_source` writes the row directly, so arm them here.
    encounter::watch_hp_threshold(ctx, MR_SMITE, SMITE_FIRST_STAND_PCT)?;
    encounter::watch_hp_threshold(ctx, MR_SMITE, SMITE_SECOND_STAND_PCT)?;
    Ok(())
}

/// Rhahk'Zor's death opens the Factory Door through the production kill path.
#[cfg(feature = "debug_reducers")]
#[reducer]
pub fn debug_deadmines_rhahkzor_falls(ctx: &ReducerContext) -> Result<(), String> {
    kill_fixture_boss(ctx, RHAHKZOR)?;
    require_door_open(ctx, FACTORY_DOOR, "Rhahk'Zor's death did not open the Factory Door")?;
    require_done(ctx, ENCOUNTER_RHAHKZOR, "Rhahk'Zor encounter state is not Done")
}

/// Destroying Sneed's Shredder ejects Sneed; the Mast Room Door stays shut until Sneed dies.
#[cfg(feature = "debug_reducers")]
#[reducer]
pub fn debug_deadmines_shredder_ejects_sneed(ctx: &ReducerContext) -> Result<(), String> {
    kill_fixture_boss(ctx, SNEEDS_SHREDDER)?;
    let sneed = live_boss(ctx, SNEED).ok_or_else(|| "no Sneed came out of the wreck".to_string())?;
    let wreck = fixture_guid(SNEEDS_SHREDDER, 41);
    let wreck = ctx
        .db
        .game_world_entity()
        .guid()
        .find(wreck)
        .ok_or_else(|| "the shredder wreck disappeared".to_string())?;
    require(
        (sneed.x - wreck.x).abs() < 10.0 && (sneed.y - wreck.y).abs() < 10.0,
        "Sneed spawned away from the shredder wreck",
    )?;
    require(
        door_state(ctx, MAST_ROOM_DOOR)? != DOOR_OPEN_STATE,
        "the Mast Room Door opened before Sneed died",
    )
}

/// Sneed's death opens the Mast Room Door.
#[cfg(feature = "debug_reducers")]
#[reducer]
pub fn debug_deadmines_sneed_falls(ctx: &ReducerContext) -> Result<(), String> {
    let sneed = live_boss(ctx, SNEED).ok_or_else(|| "Sneed is not alive to kill".to_string())?;
    let player = fixture_guid(0, 44);
    require(
        crate::combat::kill_creature(ctx, sneed.guid, Some(player)),
        "Sneed did not die",
    )?;
    require_door_open(ctx, MAST_ROOM_DOOR, "Sneed's death did not open the Mast Room Door")?;
    require_done(ctx, ENCOUNTER_SNEED, "Sneed encounter state is not Done")
}

/// Gilnid's death opens the Foundry Door.
#[cfg(feature = "debug_reducers")]
#[reducer]
pub fn debug_deadmines_gilnid_falls(ctx: &ReducerContext) -> Result<(), String> {
    kill_fixture_boss(ctx, GILNID)?;
    require_door_open(ctx, FOUNDRY_DOOR, "Gilnid's death did not open the Foundry Door")?;
    require_done(ctx, ENCOUNTER_GILNID, "Gilnid encounter state is not Done")
}

/// Damage Smite through his 66% threshold: yell, chest run, dual Reavers.
#[cfg(feature = "debug_reducers")]
#[reducer]
pub fn debug_deadmines_smite_improvises(ctx: &ReducerContext) -> Result<(), String> {
    damage_smite_to_pct(ctx, 60)?;
    require_yell_containing(ctx, "improvise")?;
    require_smite_at_chest(ctx)?;
    require_smite_equip(ctx, SMITES_REAVER, SMITES_REAVER, "66%")
}

/// Damage Smite through his 33% threshold: yell, second chest run, the two-hand hammer.
#[cfg(feature = "debug_reducers")]
#[reducer]
pub fn debug_deadmines_smite_gets_angry(ctx: &ReducerContext) -> Result<(), String> {
    damage_smite_to_pct(ctx, 30)?;
    require_yell_containing(ctx, "angry")?;
    require_smite_equip(ctx, SMITES_MIGHTY_HAMMER, 0, "33%")
}

/// Firing the Defias Cannon breaches the Iron Clad Door through the production use path.
#[cfg(feature = "debug_reducers")]
#[reducer]
pub fn debug_deadmines_cannon_breaches(ctx: &ReducerContext) -> Result<(), String> {
    let player = fixture_guid(0, 44);
    set_fixture_position(ctx, player, -106.5, -659.0, 7.2)?;
    let cannon_guid = GO_FIXTURE_BAND | u64::from(DEFIAS_CANNON);
    crate::gameobject::apply_use_gameobject(ctx, player, cannon_guid)?;
    require_door_open(ctx, IRON_CLAD_DOOR, "the cannon did not breach the Iron Clad Door")?;
    require_done(ctx, ENCOUNTER_CANNON, "cannon encounter state is not Done")
}

/// Smite's death closes his encounter.
#[cfg(feature = "debug_reducers")]
#[reducer]
pub fn debug_deadmines_smite_falls(ctx: &ReducerContext) -> Result<(), String> {
    kill_fixture_boss(ctx, MR_SMITE)?;
    require_done(ctx, ENCOUNTER_SMITE, "Mr. Smite encounter state is not Done")
}

// -------------------------------------------------------------------------------------------
//  Fixture plumbing
// -------------------------------------------------------------------------------------------

#[cfg(feature = "debug_reducers")]
fn seed_gameobject(
    ctx: &ReducerContext,
    entry: u32,
    type_id: u8,
    display_id: u32,
    x: f32,
    y: f32,
    z: f32,
    orientation: f32,
) -> Result<(), String> {
    let templates = ctx.db.game_gameobject_template();
    if templates.entry().find(entry).is_none() {
        templates.insert(crate::gameobject::GameObjectTemplate {
            entry,
            type_id,
            display_id,
            name: format!("Deadmines fixture GO {entry}"),
            data0: 0,
            data1: 0,
            gather_skill_line: 0,
            respawn_secs: 0,
            gather_gray: 0,
            lock_id: 0,
            size: 0.0,
        });
    }
    let guid = GO_FIXTURE_BAND | u64::from(entry);
    let gos = ctx.db.game_gameobject();
    gos.guid().delete(guid);
    let (grid_x, grid_y) = lyracore_shared::spatial::grid_cell(x, y);
    gos.insert(crate::gameobject::GameObject {
        guid,
        template_entry: entry,
        map_id: MAP_ID,
        x,
        y,
        z,
        orientation,
        state: 0,
        created_at: ctx.timestamp,
        respawn_at_micros: 0,
        rotation_0: 0.0,
        rotation_1: 0.0,
        rotation_2: 0.0,
        rotation_3: 0.0,
        instance_id: DEADMINES_INSTANCE,
        grid_x,
        grid_y,
        cell: lyracore_shared::spatial::grid_cell_id(grid_x, grid_y),
    });
    Ok(())
}

/// Clone the seeded starter sword into one of Smite's swap weapons — `equip_swap` only reads the
/// display id, so everything else may stay the starter's.
#[cfg(feature = "debug_reducers")]
fn seed_smite_weapon(
    ctx: &ReducerContext,
    entry: u32,
    name: &str,
    display_id: u32,
) -> Result<(), String> {
    let items = ctx.db.game_item_template();
    if items.entry().find(entry).is_some() {
        return Ok(());
    }
    let mut weapon = items
        .iter()
        .next()
        .ok_or_else(|| "fixture needs one seeded item template".to_string())?;
    weapon.entry = entry;
    weapon.name = name.to_string();
    weapon.display_id = display_id;
    items.insert(weapon);
    Ok(())
}

/// Clone a seeded creature template into Sneed's so `spawn_wave` can eject him.
#[cfg(feature = "debug_reducers")]
fn seed_sneed_template(ctx: &ReducerContext) -> Result<(), String> {
    let templates = ctx.db.game_creature_template();
    if templates.entry().find(SNEED).is_some() {
        return Ok(());
    }
    let mut sneed = templates
        .iter()
        .next()
        .ok_or_else(|| "fixture needs one seeded creature template".to_string())?;
    sneed.entry = SNEED;
    sneed.name = "Sneed".to_string();
    sneed.subname = String::new();
    templates.insert(sneed);
    Ok(())
}

#[cfg(feature = "debug_reducers")]
fn kill_fixture_boss(ctx: &ReducerContext, entry: u32) -> Result<(), String> {
    let boss = live_boss(ctx, entry).ok_or_else(|| format!("boss {entry} is not alive"))?;
    let player = fixture_guid(0, 44);
    require(
        crate::combat::kill_creature(ctx, boss.guid, Some(player)),
        "the killing blow did nothing",
    )
}

#[cfg(feature = "debug_reducers")]
fn live_boss(ctx: &ReducerContext, entry: u32) -> Option<crate::WorldEntity> {
    ctx.db
        .game_world_entity()
        .by_map()
        .filter(&MAP_ID)
        .find(|e| e.instance_id == DEADMINES_INSTANCE && e.entry == entry && !e.dead)
}

#[cfg(feature = "debug_reducers")]
fn damage_smite_to_pct(ctx: &ReducerContext, target_pct: u32) -> Result<(), String> {
    let smite = live_boss(ctx, MR_SMITE).ok_or_else(|| "Mr. Smite is not alive".to_string())?;
    let target_health = smite.max_health * target_pct / 100;
    let amount = smite.health.saturating_sub(target_health);
    require(amount > 0, "Smite is already below the target health")?;
    crate::debug::debug_apply_damage(ctx, smite.guid, amount, fixture_guid(0, 44))
}

#[cfg(feature = "debug_reducers")]
fn door_state(ctx: &ReducerContext, entry: u32) -> Result<u8, String> {
    ctx.db
        .game_gameobject()
        .guid()
        .find(GO_FIXTURE_BAND | u64::from(entry))
        .map(|go| go.state)
        .ok_or_else(|| format!("fixture gameobject {entry} disappeared"))
}

#[cfg(feature = "debug_reducers")]
fn require_door_open(ctx: &ReducerContext, entry: u32, error: &str) -> Result<(), String> {
    require(door_state(ctx, entry)? == DOOR_OPEN_STATE, error)
}

#[cfg(feature = "debug_reducers")]
fn require_done(ctx: &ReducerContext, encounter_id: u32, error: &str) -> Result<(), String> {
    require(
        encounter::get_encounter_state(ctx, DEADMINES_INSTANCE, encounter_id) == ENCOUNTER_DONE,
        error,
    )
}

#[cfg(feature = "debug_reducers")]
fn require_yell_containing(ctx: &ReducerContext, fragment: &str) -> Result<(), String> {
    let smite_guid = fixture_guid(MR_SMITE, 43);
    require(
        ctx.db
            .game_chat_event()
            .iter()
            .any(|event| event.sender_guid == smite_guid && event.message.contains(fragment)),
        &format!("Smite never yelled a line containing {fragment:?}"),
    )
}

#[cfg(feature = "debug_reducers")]
fn require_smite_at_chest(ctx: &ReducerContext) -> Result<(), String> {
    let smite_guid = fixture_guid(MR_SMITE, 43);
    let smite = ctx
        .db
        .game_world_entity()
        .guid()
        .find(smite_guid)
        .ok_or_else(|| "Mr. Smite disappeared".to_string())?;
    let chest = ctx
        .db
        .game_gameobject()
        .guid()
        .find(GO_FIXTURE_BAND | u64::from(SMITES_CHEST))
        .ok_or_else(|| "Smite's Chest disappeared".to_string())?;
    require(
        (smite.x - chest.x).abs() < 0.5 && (smite.y - chest.y).abs() < 0.5,
        "Smite did not run to his chest",
    )?;
    require(
        ctx.db.game_creature_spline().guid().find(smite_guid).is_some(),
        "Smite's chest run emitted no movement spline",
    )
}

#[cfg(feature = "debug_reducers")]
fn require_smite_equip(
    ctx: &ReducerContext,
    main_hand_item: u32,
    off_hand_item: u32,
    stand: &str,
) -> Result<(), String> {
    let display = |item: u32| -> Result<u32, String> {
        if item == 0 {
            return Ok(0);
        }
        ctx.db
            .game_item_template()
            .entry()
            .find(item)
            .map(|row| row.display_id)
            .ok_or_else(|| format!("fixture weapon {item} disappeared"))
    };
    let row = ctx
        .db
        .game_encounter_equip()
        .creature_guid()
        .find(fixture_guid(MR_SMITE, 43))
        .ok_or_else(|| format!("no equip projection after the {stand} stand"))?;
    require(
        row.main_hand == display(main_hand_item)? && row.off_hand == display(off_hand_item)?,
        &format!("wrong weapons after the {stand} stand"),
    )
}

#[cfg(feature = "debug_reducers")]
fn install_instance(ctx: &ReducerContext, map_id: u32, instance_id: u64) -> Result<(), String> {
    let instances = ctx.db.game_instance();
    match instances.instance_id().find(instance_id) {
        Some(instance) if instance.map_id == map_id => Ok(()),
        Some(instance) => Err(format!(
            "fixture instance {instance_id} is already on map {}, not {map_id}",
            instance.map_id
        )),
        None => {
            instances.insert(GameInstance {
                instance_id,
                map_id,
                party_id: 0,
                created_at: ctx.timestamp,
                last_empty_at_micros: 0,
                reset_requested: false,
            });
            Ok(())
        }
    }
}

#[cfg(feature = "debug_reducers")]
fn spawn_source(
    ctx: &ReducerContext,
    entry: u32,
    map_id: u32,
    instance_id: u64,
    dead: bool,
    sequence: u64,
) -> Result<u64, String> {
    if instance_id != 0 {
        install_instance(ctx, map_id, instance_id)?;
    }
    let entities = ctx.db.game_world_entity();
    let mut source = entities
        .by_map()
        .filter(&0u32)
        .find(|entity| !entity.is_player())
        .ok_or_else(|| "fixture needs one seeded creature".to_string())?;
    let guid = fixture_guid(entry, sequence);
    entities.guid().delete(guid);
    source.guid = guid;
    source.entry = entry;
    source.map_id = map_id;
    source.instance_id = instance_id;
    source.dead = dead;
    source.health = if dead { 0 } else { source.max_health.max(1) };
    entities.insert(source);
    Ok(guid)
}

#[cfg(feature = "debug_reducers")]
fn spawn_fixture_player(
    ctx: &ReducerContext,
    map_id: u32,
    instance_id: u64,
    sequence: u64,
) -> Result<u64, String> {
    install_instance(ctx, map_id, instance_id)?;
    let entities = ctx.db.game_world_entity();
    let mut player = entities
        .by_map()
        .filter(&0u32)
        .find(|entity| !entity.is_player())
        .ok_or_else(|| "fixture needs one seeded creature".to_string())?;
    let guid = fixture_guid(0, sequence);
    entities.guid().delete(guid);
    player.guid = guid;
    player.entry = 0;
    player.map_id = map_id;
    player.instance_id = instance_id;
    player.type_mask = lyracore_shared::constants::type_mask::PLAYER;
    player.dead = false;
    player.health = 1_000_000;
    player.max_health = 1_000_000;
    player.faction_template = 1;
    entities.insert(player);
    Ok(guid)
}

#[cfg(feature = "debug_reducers")]
fn set_fixture_position(
    ctx: &ReducerContext,
    guid: u64,
    x: f32,
    y: f32,
    z: f32,
) -> Result<(), String> {
    let entities = ctx.db.game_world_entity();
    let mut entity = entities
        .guid()
        .find(guid)
        .ok_or_else(|| format!("fixture entity {guid} disappeared"))?;
    let (grid_x, grid_y) = lyracore_shared::spatial::grid_cell(x, y);
    entity.x = x;
    entity.y = y;
    entity.z = z;
    entity.grid_x = grid_x;
    entity.grid_y = grid_y;
    entity.cell = lyracore_shared::spatial::grid_cell_id(grid_x, grid_y);
    entities.guid().update(entity);
    Ok(())
}

#[cfg(feature = "debug_reducers")]
fn fixture_guid(entry: u32, sequence: u64) -> u64 {
    encounter::wave_guid(entry, FIXTURE_LOW_BAND | sequence)
}

#[cfg(feature = "debug_reducers")]
fn require(condition: bool, error: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(error.to_string())
    }
}
