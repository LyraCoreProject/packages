//! Deterministic staging for private, per-test durable databases.
//! Staging replaces shared rotation configuration and is not safe in a shared World Shard.

use super::{pkg_playerbots_personality, pkg_playerbots_rotation};
use super::{pkg_playerbots_bot, pkg_playerbots_kit, PlayerbotsBot};
use crate::nav::game_nav_chunk;
use crate::{
    game_creature_spawn, game_creature_template, game_group, game_quest_objective,
    game_quest_template,
};
use crate::{game_creature_spline, game_spell, game_spell_effect, game_world_entity};
use spacetimedb::{reducer, ReducerContext, Table};

const HEAL: u32 = 5_090_100;
const CHANNEL_HEAL: u32 = 5_090_104;
const COMPANION_GROUP: u64 = 5_090_300;

#[reducer]
pub fn playerbots_fixture_prepare(ctx: &ReducerContext) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let bots: Vec<PlayerbotsBot> = ctx.db.pkg_playerbots_bot().iter().collect();
    let bot_guids: Vec<_> = bots.iter().map(|bot| bot.character_guid).collect();
    for mut bot in bots {
        bot.next_think_micros = i64::MAX;
        ctx.db
            .game_creature_spline()
            .guid()
            .delete(bot.character_guid);
        let mut entity = crate::helpers::live_entity(ctx, bot.character_guid)?;
        entity.health = 1;
        entity.x = 1200.0;
        entity.y = 1200.0;
        entity.z = 50.0;
        let (gx, gy) = lyracore_shared::spatial::grid_cell(entity.x, entity.y);
        entity.grid_x = gx;
        entity.grid_y = gy;
        entity.cell = lyracore_shared::spatial::grid_cell_id(gx, gy);
        ctx.db.game_world_entity().guid().update(entity);
        ctx.db.pkg_playerbots_bot().id().update(bot);
    }
    let mut spell = ctx
        .db
        .game_spell()
        .spell_id()
        .find(2050)
        .ok_or("seed heal missing")?;
    spell.spell_id = HEAL;
    spell.cast_time_ms = 5000;
    spell.cost = 0;
    spell.cooldown_ms = 0;
    ctx.db.game_spell().spell_id().delete(HEAL);
    ctx.db.game_spell().insert(spell);
    for mut effect in ctx
        .db
        .game_spell_effect()
        .by_spell()
        .filter(2050u32)
        .collect::<Vec<_>>()
    {
        effect.spell_id = HEAL;
        effect.id = ((HEAL as u64) << 2) | effect.effect_index as u64;
        effect.target = crate::spell::T_TARGET_ALLY;
        ctx.db.game_spell_effect().id().delete(effect.id);
        ctx.db.game_spell_effect().insert(effect);
    }
    for guid in bot_guids {
        crate::spell::learn_spell(ctx, guid, spacetimedb::Identity::ZERO, HEAL);
    }
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_cast(
    ctx: &ReducerContext,
    caster: u64,
    target: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let _ = super::actions::cast(ctx, caster, HEAL, target);
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_resume(ctx: &ReducerContext, guid: u64) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let mut bot = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(guid)
        .next()
        .ok_or("bot missing")?;
    bot.next_think_micros = 0;
    ctx.db.pkg_playerbots_bot().id().update(bot);
    let mut personality = ctx
        .db
        .pkg_playerbots_personality()
        .by_character()
        .filter(guid)
        .next()
        .ok_or("personality missing")?;
    personality.flee_at_pct = 0;
    ctx.db.pkg_playerbots_personality().id().update(personality);
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_cancel(
    ctx: &ReducerContext,
    guid: u64,
    expired: bool,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    if let Some(cast) = crate::spell::pending_cast(ctx, guid) {
        if expired {
            crate::spell::expire_cast_attempt(
                ctx,
                guid,
                cast.scheduled_id,
                ctx.timestamp.to_micros_since_unix_epoch(),
            );
        } else {
            crate::spell::cancel_cast_attempt(ctx, guid, cast.scheduled_id);
        }
    }
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_position(ctx: &ReducerContext, guid: u64, x: f32) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let mut entity = crate::helpers::live_entity(ctx, guid)?;
    entity.x = x;
    let (gx, gy) = lyracore_shared::spatial::grid_cell(entity.x, entity.y);
    entity.grid_x = gx;
    entity.grid_y = gy;
    entity.cell = lyracore_shared::spatial::grid_cell_id(gx, gy);
    ctx.db.game_world_entity().guid().update(entity);
    Ok(())
}

fn companion_unit(
    ctx: &ReducerContext,
    guid: u64,
    x: f32,
    y: f32,
    health_pct: u32,
) -> Result<(), String> {
    let mut entity = crate::helpers::live_entity(ctx, guid)?;
    entity.x = x;
    entity.y = y;
    entity.z = 50.0;
    entity.health = (entity.max_health.saturating_mul(health_pct) / 100).max(1);
    let (gx, gy) = lyracore_shared::spatial::grid_cell(entity.x, entity.y);
    entity.grid_x = gx;
    entity.grid_y = gy;
    entity.cell = lyracore_shared::spatial::grid_cell_id(gx, gy);
    ctx.db.game_world_entity().guid().update(entity);
    Ok(())
}

fn companion_creature(
    ctx: &ReducerContext,
    entry: u32,
    x: f32,
    y: f32,
    z: f32,
    faction_template: Option<u32>,
) -> Result<u64, String> {
    let guid = (0xF130u64 << 48) | ((u64::from(entry)) << 24) | 1;
    let mut template = ctx
        .db
        .game_creature_template()
        .entry()
        .find(51000)
        .ok_or("seed creature missing")?;
    template.entry = entry;
    if let Some(faction_template) = faction_template {
        template.faction_template = faction_template;
    }
    template.aggro_range = 0;
    ctx.db.game_creature_template().entry().delete(entry);
    let template = ctx.db.game_creature_template().insert(template);
    let spawn = crate::CreatureSpawn {
        guid,
        entry,
        map_id: 0,
        x,
        y,
        z,
        orientation: 0.0,
        respawn_at: ctx.timestamp,
        despawn_at: ctx.timestamp,
        movement_type: 0,
        respawn_secs: 60,
        life_seq: 1,
    };
    ctx.db.game_creature_spawn().guid().delete(guid);
    let spawn = ctx.db.game_creature_spawn().insert(spawn);
    crate::creatures::despawn_creature_entity(ctx, guid);
    crate::creatures::insert_creature_entity(
        ctx,
        crate::creatures::build_creature_entity(&spawn, &template, 0, 0),
    );
    Ok(guid)
}

/// Stage one human-led party from otherwise durable spawned Characters.
#[reducer]
pub fn playerbots_fixture_companion_stage(
    ctx: &ReducerContext,
    companion_guid: u64,
    leader_guid: u64,
    ally_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let mut companion = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(companion_guid)
        .next()
        .ok_or("companion bot missing")?;
    let leader_bot = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(leader_guid)
        .next()
        .ok_or("leader bot missing")?;
    ctx.db.pkg_playerbots_bot().id().delete(leader_bot.id);
    companion.next_think_micros = i64::MAX;
    let (class, role) = (companion.class, companion.role);
    ctx.db.pkg_playerbots_bot().id().update(companion);
    companion_unit(ctx, companion_guid, 1200.0, 1200.0, 100)?;
    companion_unit(ctx, leader_guid, 1220.0, 1200.0, 100)?;
    companion_unit(ctx, ally_guid, 1222.0, 1200.0, 100)?;
    companion_creature(ctx, 5_090_302, 1204.0, 1204.0, 50.0, None)?;
    let rotations = ctx.db.pkg_playerbots_rotation();
    for row in rotations
        .by_class_role()
        .filter((class, role))
        .collect::<Vec<_>>()
    {
        rotations.id().delete(row.id);
    }
    rotations.insert(super::PlayerbotsRotation {
        id: 0,
        class,
        role,
        priority: 0,
        spell_id: HEAL,
        condition: super::cond::ALLY_HP_BELOW_PCT,
        threshold_pct: 80,
    });
    let mut personality = ctx
        .db
        .pkg_playerbots_personality()
        .by_character()
        .filter(companion_guid)
        .next()
        .ok_or("companion personality missing")?;
    personality.flee_at_pct = 0;
    personality.heal_at_pct = 80;
    ctx.db.pkg_playerbots_personality().id().update(personality);
    crate::group::sync_group_mirror(
        ctx,
        COMPANION_GROUP,
        leader_guid,
        0,
        2,
        0,
        vec![leader_guid, companion_guid, ally_guid],
        crate::SessionActor {
            guid: leader_guid,
            ownership: None,
        },
    )?;
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_companion_move(
    ctx: &ReducerContext,
    guid: u64,
    x: f32,
    y: f32,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    companion_unit(ctx, guid, x, y, 100)
}

#[reducer]
pub fn playerbots_fixture_companion_health(
    ctx: &ReducerContext,
    guid: u64,
    health_pct: u32,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let entity = crate::helpers::live_entity(ctx, guid)?;
    companion_unit(ctx, guid, entity.x, entity.y, health_pct)
}

#[reducer]
pub fn playerbots_fixture_companion_remove_group(ctx: &ReducerContext) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    ctx.db.game_group().group_id().delete(COMPANION_GROUP);
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_companion_forget_heal(
    ctx: &ReducerContext,
    guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    use crate::game_player_spell;
    if let Some(spell) = ctx
        .db
        .game_player_spell()
        .by_character_spell()
        .filter((guid, HEAL))
        .next()
    {
        ctx.db.game_player_spell().id().delete(spell.id);
    }
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_companion_lesser_heal_target(
    ctx: &ReducerContext,
    target: u8,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let effects = ctx.db.game_spell_effect();
    let mut effect = effects
        .id()
        .find(2050u64 << 2)
        .ok_or("Lesser Heal effect missing")?;
    effect.target = target;
    effects.id().update(effect);
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_companion_extra_lesser_heal_effect(
    ctx: &ReducerContext,
    present: bool,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let effects = ctx.db.game_spell_effect();
    let id = (2050u64 << 2) | 1;
    effects.id().delete(id);
    if present {
        let mut effect = effects
            .id()
            .find(2050u64 << 2)
            .ok_or("Lesser Heal effect missing")?;
        effect.id = id;
        effect.effect_index = 1;
        effects.insert(effect);
    }
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_companion_mixed_heals(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let mut channel = ctx
        .db
        .game_spell()
        .spell_id()
        .find(HEAL)
        .ok_or("fixture heal missing")?;
    channel.spell_id = CHANNEL_HEAL;
    channel.name = "Unsupported channel heal".to_string();
    channel.cast_flags = crate::spell::SPELL_ATTR_CHANNELED;
    ctx.db.game_spell().spell_id().delete(CHANNEL_HEAL);
    ctx.db.game_spell().insert(channel);
    let effects = ctx.db.game_spell_effect();
    for row in effects.by_spell().filter(&CHANNEL_HEAL).collect::<Vec<_>>() {
        effects.id().delete(row.id);
    }
    for mut effect in effects.by_spell().filter(&HEAL).collect::<Vec<_>>() {
        effect.spell_id = CHANNEL_HEAL;
        effect.id = (u64::from(CHANNEL_HEAL) << 2) | u64::from(effect.effect_index);
        effects.insert(effect);
    }
    crate::spell::learn_spell(
        ctx,
        character_guid,
        spacetimedb::Identity::ZERO,
        CHANNEL_HEAL,
    );
    let rotations = ctx.db.pkg_playerbots_rotation();
    let bot = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(character_guid)
        .next()
        .ok_or("companion bot missing")?;
    let mut supported = rotations
        .by_class_role()
        .filter((bot.class, bot.role))
        .find(|row| row.spell_id == HEAL)
        .ok_or("supported heal rotation missing")?;
    supported.priority = 1;
    let (class, role, condition, threshold_pct) = (
        supported.class,
        supported.role,
        supported.condition,
        supported.threshold_pct,
    );
    rotations.id().update(supported);
    rotations.insert(super::PlayerbotsRotation {
        id: 0,
        class,
        role,
        priority: 0,
        spell_id: CHANNEL_HEAL,
        condition,
        threshold_pct,
    });
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_companion_client_cast(
    ctx: &ReducerContext,
    caster_guid: u64,
    target_guid: u64,
) -> Result<(), String> {
    crate::gw::gw_cast_at( // package-api: exempt fixture proves client and bot cast Gate parity
        ctx,
        crate::SessionActor {
            guid: caster_guid,
            ownership: None,
        },
        HEAL,
        target_guid,
    )
}

#[reducer]
pub fn playerbots_fixture_companion_triggered_cast(
    ctx: &ReducerContext,
    caster_guid: u64,
    target_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let caster = crate::helpers::live_entity(ctx, caster_guid)?;
    crate::spell::cast_triggered(ctx, caster_guid, HEAL, caster.level as u8, target_guid)
}

#[reducer]
pub fn playerbots_fixture_companion_creature_cast(
    ctx: &ReducerContext,
    target_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let target = crate::helpers::live_entity(ctx, target_guid)?;
    let guid = companion_creature(
        ctx,
        5_090_301,
        1200.0,
        1201.0,
        target.z,
        Some(target.faction_template),
    )?;
    crate::spell::start_creature_spell(
        ctx,
        crate::spell::CreatureSpellStart {
            caster_guid: guid,
            caster_level: target.level as u8,
            spell_id: HEAL,
            mode: crate::spell::CreatureSpellStartMode::Direct,
            target: crate::spell::CreatureSpellTarget::Unit(target_guid),
            interrupt_previous: false,
            admission: crate::spell::CreatureSpellCasterAdmission::Living,
        },
    )
}

#[reducer]
pub fn playerbots_fixture_companion_wall(
    ctx: &ReducerContext,
    caster_guid: u64,
    target_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    companion_unit(ctx, caster_guid, 1200.0, 1200.0, 100)?;
    companion_unit(ctx, target_guid, 1210.0, 1200.0, 25)?;
    let caster = crate::helpers::live_entity(ctx, caster_guid)?;
    let wall_x = 1205.0;
    let cx = lyracore_shared::terrain::cell_index(wall_x).ok_or("wall off grid")?;
    let cy = lyracore_shared::terrain::cell_index(caster.y).ok_or("wall off grid")?;
    let walk_sub = lyracore_shared::nav::sub_index(wall_x, cx, lyracore_shared::nav::WALK_DIM)
        .ok_or("wall off grid")?;
    let obs_sub = lyracore_shared::nav::sub_index(wall_x, cx, lyracore_shared::nav::OBS_DIM)
        .ok_or("wall off grid")?;
    for cell_y in cy.saturating_sub(1)..=cy.saturating_add(1).min(1023) {
        let key = lyracore_shared::terrain::cell_key(caster.map_id, cx, cell_y);
        let mut walk = vec![255; lyracore_shared::nav::WALK_BYTES];
        let mut obs = vec![lyracore_shared::nav::OBS_NONE; lyracore_shared::nav::OBS_BYTES];
        for sub_y in 0..lyracore_shared::nav::WALK_DIM {
            lyracore_shared::nav::walk_set(&mut walk, walk_sub, sub_y, false);
        }
        for sub_y in 0..lyracore_shared::nav::OBS_DIM {
            lyracore_shared::nav::obs_raise(&mut obs, caster.z, obs_sub, sub_y, caster.z + 4.0);
        }
        ctx.db.game_nav_chunk().key().delete(key);
        ctx.db.game_nav_chunk().insert(crate::nav::NavChunk {
            key,
            map_id: caster.map_id,
            cell_x: cx,
            cell_y,
            base_z: caster.z,
            walk,
            obs,
        });
    }
    if crate::nav::has_los(
        ctx,
        caster.map_id,
        caster.instance_id,
        (caster.x, caster.y, caster.z),
        (1210.0, 1200.0, caster.z),
    ) {
        return Err("synthetic wall did not block line of sight".to_string());
    }
    runner_due_for(ctx, caster_guid)
}

#[reducer]
pub fn playerbots_fixture_blocked_quest(ctx: &ReducerContext, guid: u64) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let mut me = crate::helpers::live_entity(ctx, guid)?;
    me.health = me.max_health;
    ctx.db.game_world_entity().guid().update(me);
    let me = crate::helpers::live_entity(ctx, guid)?;
    let entry = 5_090_101;
    let target_guid = (0xF130u64 << 48) | ((entry as u64) << 24) | 1;
    let mut template = ctx
        .db
        .game_creature_template()
        .entry()
        .find(51000)
        .ok_or("seed wolf missing")?;
    template.entry = entry;
    ctx.db.game_creature_template().entry().delete(entry);
    let template = ctx.db.game_creature_template().insert(template);
    let spawn = crate::CreatureSpawn {
        guid: target_guid,
        entry,
        map_id: me.map_id,
        x: me.x + 3.0,
        y: me.y,
        z: me.z,
        orientation: 0.0,
        respawn_at: ctx.timestamp,
        despawn_at: ctx.timestamp,
        movement_type: 0,
        respawn_secs: 60,
        life_seq: 1,
    };
    ctx.db.game_creature_spawn().guid().delete(target_guid);
    let spawn = ctx.db.game_creature_spawn().insert(spawn);
    crate::creatures::despawn_creature_entity(ctx, target_guid);
    crate::creatures::insert_creature_entity(
        ctx,
        crate::creatures::build_creature_entity(&spawn, &template, 0, 0),
    );
    ctx.db.game_quest_template().entry().delete(50909);
    ctx.db.game_quest_template().insert(crate::QuestTemplate {
        entry: 50909,
        min_level: 1,
        quest_level: 1,
        title: "Blocked fixture hunt".to_string(),
        reward_money: 0,
        reward_xp: 0,
        prev_quest_id: 0,
        required_races: 0,
        required_classes: 0,
        zone_or_sort: 0,
        rew_rep_faction_1: 0,
        rew_rep_value_1: 0,
        rew_rep_faction_2: 0,
        rew_rep_value_2: 0,
        src_item: 0,
        src_item_count: 0,
        repeatable: false,
        next_quest_id: 0,
        limit_time: 0,
        reward_money_max_level: 0,
    });
    ctx.db.game_quest_objective().id().delete(5_090_101);
    ctx.db.game_quest_objective().insert(crate::QuestObjective {
        id: 5_090_101,
        quest_entry: 50909,
        obj_index: 0,
        kind: crate::quest::objective_kind::KILL_CREATURE,
        target_entry: entry,
        required_count: 2,
    });
    crate::actor::stage_quest(ctx, guid, 50909)?;
    let cx = lyracore_shared::terrain::cell_index(me.x).ok_or("fixture off grid")?;
    let cy = lyracore_shared::terrain::cell_index(me.y).ok_or("fixture off grid")?;
    for x in cx.saturating_sub(1)..=cx.saturating_add(1).min(1023) {
        for y in cy.saturating_sub(1)..=cy.saturating_add(1).min(1023) {
            let key = lyracore_shared::terrain::cell_key(me.map_id, x, y);
            ctx.db.game_nav_chunk().key().delete(key);
            ctx.db.game_nav_chunk().insert(crate::nav::NavChunk {
                key,
                map_id: me.map_id,
                cell_x: x,
                cell_y: y,
                base_z: me.z,
                walk: vec![0; lyracore_shared::nav::WALK_BYTES],
                obs: vec![20; lyracore_shared::nav::OBS_BYTES],
            });
        }
    }
    if crate::nav::has_los(
        ctx,
        me.map_id,
        me.instance_id,
        (me.x, me.y, me.z),
        (spawn.x, spawn.y, spawn.z),
    ) {
        return Err("fixture ray unexpectedly clear".to_string());
    }
    playerbots_fixture_resume(ctx, guid)
}

#[reducer]
pub fn playerbots_fixture_move(ctx: &ReducerContext, guid: u64, x: f32) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let me = crate::helpers::live_entity(ctx, guid)?;
    super::goals::walk_toward(ctx, &me, (x, me.y, me.z), 0.0, true);
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_cast_mode(
    ctx: &ReducerContext,
    caster: u64,
    target: u64,
    channel: bool,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let mut spell = ctx
        .db
        .game_spell()
        .spell_id()
        .find(HEAL)
        .ok_or("fixture heal missing")?;
    spell.cast_time_ms = 0;
    spell.cast_flags = if channel {
        crate::spell::SPELL_ATTR_CHANNELED
    } else {
        0
    };
    ctx.db.game_spell().spell_id().update(spell);
    let _ = super::actions::cast(ctx, caster, HEAL, target);
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_cancel_handle(
    ctx: &ReducerContext,
    guid: u64,
    scheduled_id: u64,
    deadline: i64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    if deadline == 0 {
        crate::spell::cancel_cast_attempt(ctx, guid, scheduled_id);
    } else {
        crate::spell::expire_cast_attempt(ctx, guid, scheduled_id, deadline);
    }
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_attack(
    ctx: &ReducerContext,
    guid: u64,
    target: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let _ = super::actions::attack(ctx, guid, target);
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_partial_route(ctx: &ReducerContext, guid: u64) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let me = crate::helpers::live_entity(ctx, guid)?;
    let wall_x = me.x + 10.0;
    let cx = lyracore_shared::terrain::cell_index(wall_x).ok_or("fixture off grid")?;
    let cy = lyracore_shared::terrain::cell_index(me.y).ok_or("fixture off grid")?;
    let nx = lyracore_shared::nav::sub_index(wall_x, cx, lyracore_shared::nav::WALK_DIM)
        .ok_or("fixture wall off grid")?;
    for y in cy - 4..=cy + 4 {
        let key = lyracore_shared::terrain::cell_key(me.map_id, cx, y);
        let mut walk = vec![255; lyracore_shared::nav::WALK_BYTES];
        for ny in 0..lyracore_shared::nav::WALK_DIM {
            lyracore_shared::nav::walk_set(&mut walk, nx, ny, false);
        }
        ctx.db.game_nav_chunk().key().delete(key);
        ctx.db.game_nav_chunk().insert(crate::nav::NavChunk {
            key,
            map_id: me.map_id,
            cell_x: cx,
            cell_y: y,
            base_z: me.z,
            walk,
            obs: vec![lyracore_shared::nav::OBS_NONE; lyracore_shared::nav::OBS_BYTES],
        });
    }
    Ok(())
}

const QUEST: u32 = 50910;
const COLLECT: u32 = 5_090_120;
const GUARANTEED: u32 = 5_090_121;
const CHOICE: u32 = 5_090_122;
const FILLER: u32 = 5_090_123;

fn fixture_giver() -> u64 {
    (0xF130u64 << 48) | (5_090_101u64 << 24) | 1
}

#[reducer]
pub fn playerbots_fixture_interaction_stage(
    ctx: &ReducerContext,
    guid: u64,
    source_item: bool,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    playerbots_fixture_blocked_quest(ctx, guid)?;
    let mut bot = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(guid)
        .next()
        .ok_or("bot missing")?;
    bot.next_think_micros = i64::MAX;
    ctx.db.pkg_playerbots_bot().id().update(bot);
    use crate::{
        game_character_quest, game_creature_quest, game_item_instance, game_item_template,
        game_quest_reward_choice, game_quest_reward_item,
    };
    let original_item = ctx
        .db
        .game_item_instance()
        .by_owner_guid()
        .filter(guid)
        .next()
        .ok_or("starter item missing")?;
    for entry in [COLLECT, GUARANTEED, CHOICE, FILLER] {
        let mut template = ctx
            .db
            .game_item_template()
            .entry()
            .find(original_item.entry)
            .ok_or("starter template missing")?;
        template.entry = entry;
        template.max_stack = 1;
        template.max_count = 0;
        template.random_property = 0;
        ctx.db.game_item_template().entry().delete(entry);
        ctx.db.game_item_template().insert(template);
    }
    for item in ctx
        .db
        .game_item_instance()
        .by_owner_guid()
        .filter(guid)
        .collect::<Vec<_>>()
    {
        ctx.db.game_item_instance().guid().delete(item.guid);
    }
    for quest in ctx
        .db
        .game_character_quest()
        .by_character()
        .filter(guid)
        .collect::<Vec<_>>()
    {
        ctx.db.game_character_quest().id().delete(quest.id);
    }
    let mut template = ctx
        .db
        .game_quest_template()
        .entry()
        .find(50909)
        .ok_or("fixture quest missing")?;
    template.entry = QUEST;
    template.title = "Inventory exchange fixture".to_string();
    template.reward_money = 150;
    template.reward_xp = 90;
    template.src_item = if source_item { COLLECT } else { 0 };
    template.src_item_count = 1;
    ctx.db.game_quest_template().entry().delete(QUEST);
    ctx.db.game_quest_template().insert(template);
    for row in ctx
        .db
        .game_creature_quest()
        .by_creature()
        .filter(5_090_101u32)
        .filter(|row| row.quest_entry == QUEST)
        .collect::<Vec<_>>()
    {
        ctx.db.game_creature_quest().id().delete(row.id);
    }
    ctx.db.game_quest_objective().id().delete(COLLECT as u64);
    ctx.db
        .game_quest_reward_item()
        .id()
        .delete(GUARANTEED as u64);
    ctx.db.game_quest_reward_choice().id().delete(CHOICE as u64);
    ctx.db.game_quest_objective().insert(crate::QuestObjective {
        id: COLLECT as u64,
        quest_entry: QUEST,
        obj_index: 0,
        kind: crate::quest::objective_kind::COLLECT_ITEM,
        target_entry: COLLECT,
        required_count: 1,
    });
    for role in [
        crate::quest::quest_role::START,
        crate::quest::quest_role::END,
    ] {
        ctx.db.game_creature_quest().insert(crate::CreatureQuest {
            id: 0,
            creature_entry: 5_090_101,
            quest_entry: QUEST,
            role,
        });
    }
    ctx.db
        .game_quest_reward_item()
        .insert(crate::QuestRewardItem {
            id: GUARANTEED as u64,
            quest_entry: QUEST,
            item_entry: GUARANTEED,
            count: 1,
        });
    ctx.db
        .game_quest_reward_choice()
        .insert(crate::QuestRewardChoice {
            id: CHOICE as u64,
            quest_entry: QUEST,
            choice_index: 0,
            item_entry: CHOICE,
            count: 1,
        });
    if !source_item {
        crate::actor::stage_quest(ctx, guid, QUEST)?;
        crate::items::grant_item(ctx, guid, COLLECT, 1)?;
    }
    crate::items::grant_item(ctx, guid, FILLER, if source_item { 16 } else { 15 })?;
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_interact(
    ctx: &ReducerContext,
    guid: u64,
    accept: bool,
    choice: u32,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    if accept {
        let _ = super::actions::accept_quest(ctx, guid, fixture_giver(), QUEST);
    } else {
        let _ = super::actions::turn_in_quest(ctx, guid, fixture_giver(), QUEST, choice);
    }
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_free_slot(ctx: &ReducerContext, guid: u64) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    crate::items::remove_items(ctx, guid, FILLER, 1)
}

#[reducer]
pub fn playerbots_fixture_freeze(ctx: &ReducerContext, guid: u64) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let mut bot = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(guid)
        .next()
        .ok_or("bot missing")?;
    bot.next_think_micros = i64::MAX;
    ctx.db.pkg_playerbots_bot().id().update(bot);
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_forget_credit(ctx: &ReducerContext, guid: u64) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    use super::pkg_playerbots_goal;
    let mut goal = ctx
        .db
        .pkg_playerbots_goal()
        .by_character()
        .filter(guid)
        .next()
        .ok_or("goal missing")?;
    goal.quest_credit = None;
    ctx.db.pkg_playerbots_goal().id().update(goal);
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_credit_kill(ctx: &ReducerContext, guid: u64) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let target = fixture_giver();
    let (amount, _) = crate::combat::fold_incoming_damage(ctx, guid, target, 10000);
    let damage = crate::combat::final_damage(ctx, target, amount);
    let outcome = crate::combat::apply_hit(
        ctx,
        guid,
        target,
        damage,
        crate::combat::Hit::weapon(crate::combat::HitSource::MainHand, false),
    );
    if !outcome.killed {
        return Err("fixture hit was not lethal".to_string());
    }
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_runner_stage(
    ctx: &ReducerContext,
    guid: u64,
    healing: bool,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let mut bot = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(guid)
        .next()
        .ok_or("bot missing")?;
    bot.home_x = 1240.0;
    bot.home_y = 1200.0;
    bot.home_z = 50.0;
    bot.home_map = 0;
    bot.next_think_micros = i64::MAX;
    let _ = crate::actor::stop_attack(ctx, guid);
    use crate::game_threat;
    let threats = ctx.db.game_threat();
    for row in threats.by_source().filter(guid).collect::<Vec<_>>() {
        threats.id().delete(row.id);
    }
    let mut me = crate::helpers::live_entity(ctx, guid)?;
    me.health = if healing {
        me.max_health / 4
    } else {
        me.max_health
    };
    ctx.db.game_world_entity().guid().update(me);
    let mut personality = ctx
        .db
        .pkg_playerbots_personality()
        .by_character()
        .filter(guid)
        .next()
        .ok_or("personality missing")?;
    personality.flee_at_pct = 0;
    ctx.db.pkg_playerbots_personality().id().update(personality);
    if healing {
        let rows = ctx.db.pkg_playerbots_rotation();
        for row in rows
            .by_class_role()
            .filter((bot.class, bot.role))
            .collect::<Vec<_>>()
        {
            rows.id().delete(row.id);
        }
        rows.insert(super::PlayerbotsRotation {
            id: 0,
            class: bot.class,
            role: bot.role,
            priority: 0,
            spell_id: HEAL,
            condition: super::cond::ALLY_HP_BELOW_PCT,
            threshold_pct: 50,
        });
    }
    ctx.db.pkg_playerbots_bot().id().update(bot);
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_runner_damage(
    ctx: &ReducerContext,
    guid: u64,
    attacker: u64,
    damage: u32,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let (amount, _) = crate::combat::fold_incoming_damage(ctx, attacker, guid, damage);
    let damage = crate::combat::final_damage(ctx, guid, amount);
    crate::combat::apply_hit(
        ctx,
        attacker,
        guid,
        damage,
        crate::combat::Hit::weapon(crate::combat::HitSource::MainHand, false),
    );
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_runner_due(ctx: &ReducerContext) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    for mut bot in ctx.db.pkg_playerbots_bot().iter().collect::<Vec<_>>() {
        bot.next_think_micros = ctx.timestamp.to_micros_since_unix_epoch() - 2_000_000;
        ctx.db.pkg_playerbots_bot().id().update(bot);
    }
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_runner_pass(ctx: &ReducerContext) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    super::runner::pass(ctx);
    Ok(())
}

/// Make one bot due, run the real Bot Controller pass, then keep background ticks from running its
/// next step before the fixture inspects the result.
#[reducer]
pub fn playerbots_fixture_runner_pass_once(ctx: &ReducerContext, guid: u64) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    runner_due_for(ctx, guid)?;
    super::runner::pass(ctx);
    use super::pkg_playerbots_scheduler;
    let processed = ctx
        .db
        .pkg_playerbots_scheduler()
        .id()
        .find(0)
        .is_some_and(|row| row.processed_guids.contains(&guid));
    if !processed {
        return Err(format!("runner pass did not process bot {guid}"));
    }
    runner_park_for(ctx, guid)
}

/// Select controlled companion behavior without opening a scheduler gap before the fixture's first
/// explicit runner pass.
#[reducer]
pub fn playerbots_fixture_runner_select_cohort(
    ctx: &ReducerContext,
    guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    super::runner::playerbots_select_controller(ctx, guid, super::Controller::Cohort)?;
    runner_park_for(ctx, guid)
}

#[reducer]
pub fn playerbots_fixture_runner_survival(ctx: &ReducerContext, guid: u64) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let mut personality = ctx
        .db
        .pkg_playerbots_personality()
        .by_character()
        .filter(guid)
        .next()
        .ok_or("personality missing")?;
    personality.flee_at_pct = 100;
    ctx.db.pkg_playerbots_personality().id().update(personality);
    runner_due_for(ctx, guid)
}

#[reducer]
pub fn playerbots_fixture_runner_clear_navigation(
    ctx: &ReducerContext,
    guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let me = crate::helpers::live_entity(ctx, guid)?;
    let cx = lyracore_shared::terrain::cell_index(me.x).ok_or("fixture off grid")?;
    let cy = lyracore_shared::terrain::cell_index(me.y).ok_or("fixture off grid")?;
    for x in cx.saturating_sub(1)..=cx.saturating_add(1).min(1023) {
        for y in cy.saturating_sub(1)..=cy.saturating_add(1).min(1023) {
            ctx.db
                .game_nav_chunk()
                .key()
                .delete(lyracore_shared::terrain::cell_key(me.map_id, x, y));
        }
    }
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_runner_expire_objective(
    ctx: &ReducerContext,
    guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    use super::pkg_playerbots_runner;
    let mut state = ctx
        .db
        .pkg_playerbots_runner()
        .character_guid()
        .find(guid)
        .ok_or("runner missing")?;
    let objective = state.objective.as_mut().ok_or("objective missing")?;
    objective.deadline_micros = ctx.timestamp.to_micros_since_unix_epoch();
    ctx.db
        .pkg_playerbots_runner()
        .character_guid()
        .update(state);
    runner_due_for(ctx, guid)
}

fn runner_due_for(ctx: &ReducerContext, guid: u64) -> Result<(), String> {
    let mut bot = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(guid)
        .next()
        .ok_or("bot missing")?;
    bot.next_think_micros = ctx.timestamp.to_micros_since_unix_epoch() - 2_000_000;
    ctx.db.pkg_playerbots_bot().id().update(bot);
    Ok(())
}

fn runner_park_for(ctx: &ReducerContext, guid: u64) -> Result<(), String> {
    let mut bot = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(guid)
        .next()
        .ok_or("bot missing")?;
    bot.next_think_micros = i64::MAX;
    ctx.db.pkg_playerbots_bot().id().update(bot);
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_companion_due(ctx: &ReducerContext, guid: u64) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    runner_due_for(ctx, guid)
}

#[reducer]
pub fn playerbots_fixture_runner_wide_recovery(
    ctx: &ReducerContext,
    guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    playerbots_fixture_runner_stage(ctx, guid, true)?;
    use crate::game_player_spell;
    let spells = ctx.db.game_player_spell();
    let heal = spells
        .by_character_spell()
        .filter((guid, HEAL))
        .next()
        .ok_or("heal missing")?;
    spells.id().delete(heal.id);
    for spell_id in 5_091_000..5_091_300 {
        spells.insert(crate::spell::PlayerSpell {
            id: 0,
            character_guid: guid,
            owner_identity: heal.owner_identity,
            spell_id,
        });
    }
    spells.insert(crate::spell::PlayerSpell { id: 0, ..heal });
    let rotations = ctx.db.pkg_playerbots_rotation();
    let bot = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(guid)
        .next()
        .ok_or("bot missing")?;
    let heal = rotations
        .by_class_role()
        .filter((bot.class, bot.role))
        .next()
        .ok_or("rotation missing")?;
    rotations.id().delete(heal.id);
    for spell_id in 5_092_000..5_092_025 {
        rotations.insert(super::PlayerbotsRotation {
            id: 0,
            spell_id,
            ..heal
        });
    }
    rotations.insert(super::PlayerbotsRotation { id: 0, ..heal });
    Ok(())
}

const PROVISION_QUEST_ITEM: u32 = 5_090_150;
const PROVISION_FILLER: u32 = 5_090_151;
const PROVISION_WARRIOR_TRAINER: u32 = 5_090_200;
const PROVISION_OTHER_TRAINER: u32 = 5_090_201;
const PROVISION_LEARN_WRAPPER: u32 = 5_090_202;
const PROVISION_CHANNEL_WRAPPER: u32 = 5_090_203;
const PROVISION_PROC_WRAPPER: u32 = 5_090_204;
const PROVISION_WRONG_CLASS_SPELL: u32 = 5_090_205;
const PROVISION_LOW_LEVEL_SPELL: u32 = 5_090_206;
const PROVISION_PREVIOUS_RANK_SPELL: u32 = 5_090_207;
const PROVISION_OVERFLOW_SPELL: u32 = 5_090_208;
const PROVISION_PREVIOUS_REQUIRED: u32 = 5_090_209;
const PROVISION_DUPLICATE_SUCCESS_SPELL: u32 = 5_090_210;
const PROVISION_WRONG_WRAPPER: u32 = 5_090_211;
const PROVISION_WRONG_WRAPPER_PAYLOAD: u32 = 5_090_212;
const PROVISION_PARTIAL_TALENT_FIRST: u32 = 5_095_000;
const PROVISION_PARTIAL_TALENT_COUNT: u32 = 65;

/// Fill a missing low-ID item definition for a private seed-only Shard. Imported or otherwise
/// existing rows remain authoritative and are never changed by this fixture.
fn ensure_profile_item(
    ctx: &ReducerContext,
    entry: u32,
    name: &str,
    max_stack: u32,
    inventory_type: u8,
    container_slots: u8,
    spell_id: u32,
    restores_power: bool,
) -> Result<(), String> {
    use crate::game_item_template;
    if ctx.db.game_item_template().entry().find(entry).is_some() {
        return Ok(());
    }
    let mut template = ctx
        .db
        .game_item_template()
        .entry()
        .find(52)
        .ok_or("seed food template missing")?;
    template.entry = entry;
    template.name = name.to_string();
    template.class = if inventory_type == 18 { 1 } else { 0 };
    template.inventory_type = inventory_type;
    template.item_level = 1;
    template.required_level = 1;
    template.max_durability = 0;
    template.max_stack = max_stack;
    template.container_slots = container_slots;
    template.spellid_1 = spell_id;
    template.spelltrigger_1 = 0;
    template.restores_power = restores_power;
    template.bonding = 0;
    template.max_count = 0;
    template.required_skill = 0;
    template.required_skill_rank = 0;
    ctx.db.game_item_template().insert(template);
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_provision_catalog(ctx: &ReducerContext) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    // The seed and World import both own this real low id. Deliberately different values prove that
    // fixture staging takes the import-aware no-op path; item 52 is never part of the profile.
    ensure_profile_item(
        ctx,
        52,
        "Provisioning preservation fixture",
        1,
        18,
        4,
        0,
        true,
    )?;
    ensure_profile_item(ctx, 4496, "Provisioning Bag", 1, 18, 4, 0, false)?;
    ensure_profile_item(ctx, 117, "Provisioning Food", 20, 0, 0, 50115, false)?;
    ensure_profile_item(ctx, 159, "Provisioning Drink", 20, 0, 0, 50114, true)?;
    ensure_profile_item(ctx, 118, "Provisioning Potion", 5, 0, 0, 50110, false)?;
    ensure_profile_item(ctx, 1251, "Provisioning Bandage", 20, 0, 0, 50111, false)?;
    ensure_profile_item(ctx, 2512, "Provisioning Ammo", 200, 0, 0, 0, false)?;
    for (entry, name) in [
        (17033, "Paladin Reagent"),
        (17029, "Priest Reagent"),
        (17056, "Mage Reagent"),
    ] {
        ensure_profile_item(ctx, entry, name, 20, 0, 0, 0, false)?;
    }
    Ok(())
}

/// Give the completion scenario an explicit no-import profile whose every spell has a seeded
/// `game_spell` header. The default Warrior tank profile still names Sunder Armor (7386); its
/// missing-resource behavior is exercised separately.
#[reducer]
pub fn playerbots_fixture_provision_complete_profile(
    ctx: &ReducerContext,
    guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let bot = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(guid)
        .next()
        .ok_or("bot missing")?;
    if bot.class != super::class::WARRIOR || bot.role != super::ROLE_TANK {
        return Err("complete profile fixture requires a Warrior tank".to_string());
    }
    if ctx.db.game_spell().spell_id().find(355).is_none() {
        return Err("seed Taunt spell header missing".to_string());
    }
    let kits = ctx.db.pkg_playerbots_kit();
    for row in kits
        .by_class_role()
        .filter((bot.class, bot.role))
        .filter(|row| row.spell_id == 7386)
        .collect::<Vec<_>>()
    {
        kits.id().delete(row.id);
    }
    if kits
        .by_class_role()
        .filter((bot.class, bot.role))
        .any(|row| ctx.db.game_spell().spell_id().find(row.spell_id).is_none())
    {
        return Err("complete profile fixture contains a spell without a header".to_string());
    }
    Ok(())
}

/// Stage a partial Talent import: tabs are absent and 65 imported-style rows remain at the
/// preferred tree position.
#[reducer]
pub fn playerbots_fixture_provision_partial_talent_catalog(
    ctx: &ReducerContext,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    use crate::{game_talent, game_talent_tab};
    if ctx.db.game_talent_tab().count() != 0 {
        return Err("partial talent catalogue requires no tab rows".to_string());
    }
    let talents = ctx.db.game_talent();
    for ordinal in 0..PROVISION_PARTIAL_TALENT_COUNT {
        let talent_id = PROVISION_PARTIAL_TALENT_FIRST + ordinal;
        let mut talent = talents.talent_id().find(1).ok_or("seed talent missing")?;
        talent.talent_id = talent_id;
        talent.name = format!("Provisioning partial talent {ordinal}");
        talent.tree_id = 2;
        talent.tab_id = talent_id;
        talents.talent_id().delete(talent_id);
        talents.insert(talent);
    }
    Ok(())
}

fn provision_trainer(ctx: &ReducerContext, entry: u32, class: u8) -> Result<(), String> {
    let mut template = ctx
        .db
        .game_creature_template()
        .entry()
        .find(51001)
        .ok_or("seed trainer template missing")?;
    template.entry = entry;
    template.name = format!("PB005 class {class} trainer");
    template.trainer_type = lyracore_shared::trainer::trainer_type::CLASS;
    template.trainer_class = class;
    let templates = ctx.db.game_creature_template();
    templates.entry().delete(entry);
    templates.insert(template);
    Ok(())
}

fn provision_wrapper(
    ctx: &ReducerContext,
    wrapper: u32,
    target: u32,
    kind: u8,
) -> Result<(), String> {
    let mut header = ctx
        .db
        .game_spell()
        .spell_id()
        .find(target)
        .ok_or_else(|| format!("seed spell {target} missing"))?;
    header.spell_id = wrapper;
    header.name = format!("PB005 wrapper for {target}");
    let headers = ctx.db.game_spell();
    headers.spell_id().delete(wrapper);
    headers.insert(header);

    let mut effect = ctx
        .db
        .game_spell_effect()
        .by_spell()
        .filter(target)
        .next()
        .ok_or_else(|| format!("seed spell {target} has no effect"))?;
    effect.id = (u64::from(wrapper) << 2) | u64::from(effect.effect_index);
    effect.spell_id = wrapper;
    effect.kind = kind;
    effect.trigger_spell = target;
    let effects = ctx.db.game_spell_effect();
    for row in effects.by_spell().filter(wrapper).collect::<Vec<_>>() {
        effects.id().delete(row.id);
    }
    effects.insert(effect);
    Ok(())
}

fn provision_spell_clone(
    ctx: &ReducerContext,
    spell_id: u32,
    source_spell: u32,
) -> Result<(), String> {
    let mut header = ctx
        .db
        .game_spell()
        .spell_id()
        .find(source_spell)
        .ok_or_else(|| format!("seed spell {source_spell} missing"))?;
    header.spell_id = spell_id;
    header.name = format!("PB005 fixture spell {spell_id}");
    let headers = ctx.db.game_spell();
    headers.spell_id().delete(spell_id);
    headers.insert(header);

    let source_effects: Vec<_> = ctx
        .db
        .game_spell_effect()
        .by_spell()
        .filter(source_spell)
        .collect();
    if source_effects.is_empty() {
        return Err(format!("seed spell {source_spell} has no effect"));
    }
    let effects = ctx.db.game_spell_effect();
    for row in effects.by_spell().filter(spell_id).collect::<Vec<_>>() {
        effects.id().delete(row.id);
    }
    for mut effect in source_effects {
        effect.id = (u64::from(spell_id) << 2) | u64::from(effect.effect_index);
        effect.spell_id = spell_id;
        effects.insert(effect);
    }
    Ok(())
}

fn provision_offering(
    ctx: &ReducerContext,
    id: u64,
    trainer_entry: u32,
    spell_id: u32,
    required_level: u8,
) {
    use crate::game_trainer_spell;
    let rows = ctx.db.game_trainer_spell();
    rows.id().delete(id);
    rows.insert(crate::TrainerSpell {
        id,
        trainer_entry,
        spell_id,
        cost: 0,
        required_level,
        learn_skill_line: 0,
        learn_skill_cap: 75,
    });
}

fn provision_kit_spell(ctx: &ReducerContext, bot: &PlayerbotsBot, spell_id: u32) {
    let kits = ctx.db.pkg_playerbots_kit();
    if kits
        .by_class_role()
        .filter((bot.class, bot.role))
        .all(|row| row.spell_id != spell_id)
    {
        kits.insert(super::PlayerbotsKit {
            id: 0,
            class: bot.class,
            role: bot.role,
            spell_id,
        });
    }
}

/// Source-derived rows with the same direct and LearnSpell-wrapper shape as imported trainer data.
/// They prove lookup and Gates only; they are not evidence about an imported World catalogue.
#[reducer]
pub fn playerbots_fixture_provision_trainer_catalog(
    ctx: &ReducerContext,
    guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let bot = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(guid)
        .next()
        .ok_or("bot missing")?;
    if bot.class != super::class::WARRIOR {
        return Err("trainer catalogue fixture requires a Warrior".to_string());
    }
    for spell in [355, 2050, 139, 133] {
        if ctx.db.game_spell().spell_id().find(spell).is_none() {
            return Err(format!("seed spell {spell} missing"));
        }
    }
    for (spell, source) in [
        (PROVISION_WRONG_CLASS_SPELL, 133),
        (PROVISION_LOW_LEVEL_SPELL, 133),
        (PROVISION_PREVIOUS_RANK_SPELL, 133),
        (PROVISION_OVERFLOW_SPELL, 133),
        (PROVISION_PREVIOUS_REQUIRED, 133),
        (PROVISION_DUPLICATE_SUCCESS_SPELL, 133),
        (PROVISION_WRONG_WRAPPER_PAYLOAD, 133),
    ] {
        provision_spell_clone(ctx, spell, source)?;
    }
    for spell in [
        355,
        2050,
        139,
        133,
        PROVISION_WRONG_CLASS_SPELL,
        PROVISION_LOW_LEVEL_SPELL,
        PROVISION_PREVIOUS_RANK_SPELL,
        PROVISION_OVERFLOW_SPELL,
        PROVISION_DUPLICATE_SUCCESS_SPELL,
        PROVISION_WRONG_WRAPPER,
    ] {
        provision_kit_spell(ctx, &bot, spell);
    }
    provision_trainer(ctx, PROVISION_WARRIOR_TRAINER, super::class::WARRIOR)?;
    provision_trainer(ctx, PROVISION_OTHER_TRAINER, super::class::PALADIN)?;
    provision_wrapper(ctx, PROVISION_LEARN_WRAPPER, 2050, crate::spell::E_SCRIPTED)?;
    provision_wrapper(
        ctx,
        PROVISION_CHANNEL_WRAPPER,
        139,
        crate::spell::A_PERIODIC_TRIGGER,
    )?;
    provision_wrapper(
        ctx,
        PROVISION_PROC_WRAPPER,
        133,
        crate::spell::A_PROC_TRIGGER,
    )?;
    provision_wrapper(
        ctx,
        PROVISION_WRONG_WRAPPER,
        PROVISION_WRONG_WRAPPER_PAYLOAD,
        crate::spell::E_SCRIPTED,
    )?;

    for ordinal in 0..40u64 {
        provision_offering(
            ctx,
            5_091_000 + ordinal,
            PROVISION_WARRIOR_TRAINER,
            5_099_000 + ordinal as u32,
            1,
        );
    }
    provision_offering(ctx, 5_092_000, PROVISION_WARRIOR_TRAINER, 355, 1);
    provision_offering(
        ctx,
        5_092_001,
        PROVISION_WARRIOR_TRAINER,
        PROVISION_LEARN_WRAPPER,
        1,
    );
    provision_offering(
        ctx,
        5_092_002,
        PROVISION_WARRIOR_TRAINER,
        PROVISION_CHANNEL_WRAPPER,
        1,
    );
    provision_offering(
        ctx,
        5_092_003,
        PROVISION_WARRIOR_TRAINER,
        PROVISION_PROC_WRAPPER,
        1,
    );
    provision_offering(
        ctx,
        5_092_004,
        PROVISION_OTHER_TRAINER,
        PROVISION_WRONG_CLASS_SPELL,
        1,
    );
    provision_offering(
        ctx,
        5_092_005,
        PROVISION_WARRIOR_TRAINER,
        PROVISION_LOW_LEVEL_SPELL,
        60,
    );
    provision_offering(
        ctx,
        5_092_006,
        PROVISION_WARRIOR_TRAINER,
        PROVISION_PREVIOUS_RANK_SPELL,
        1,
    );
    provision_offering(
        ctx,
        5_092_007,
        PROVISION_WARRIOR_TRAINER,
        PROVISION_WRONG_WRAPPER,
        1,
    );
    for ordinal in 0..17u64 {
        provision_offering(
            ctx,
            5_093_000 + ordinal,
            if ordinal < 16 {
                PROVISION_OTHER_TRAINER
            } else {
                PROVISION_WARRIOR_TRAINER
            },
            PROVISION_OVERFLOW_SPELL,
            1,
        );
    }
    for ordinal in 0..17u64 {
        provision_offering(
            ctx,
            5_094_000 + ordinal,
            PROVISION_WARRIOR_TRAINER,
            PROVISION_DUPLICATE_SUCCESS_SPELL,
            1,
        );
    }
    use crate::game_spell_chain;
    ctx.db
        .game_spell_chain()
        .spell_id()
        .delete(PROVISION_PREVIOUS_RANK_SPELL);
    ctx.db.game_spell_chain().insert(crate::spell::SpellChain {
        spell_id: PROVISION_PREVIOUS_RANK_SPELL,
        prev_spell: PROVISION_PREVIOUS_REQUIRED,
        first_spell: PROVISION_PREVIOUS_REQUIRED,
        rank: 2,
        req_spell: 0,
    });
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_provision_spell_action(
    ctx: &ReducerContext,
    guid: u64,
    spell_id: u32,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    if !matches!(
        spell_id,
        355 | 2050
            | 139
            | 133
            | PROVISION_WRONG_CLASS_SPELL
            | PROVISION_LOW_LEVEL_SPELL
            | PROVISION_PREVIOUS_RANK_SPELL
            | PROVISION_OVERFLOW_SPELL
            | PROVISION_DUPLICATE_SUCCESS_SPELL
            | PROVISION_WRONG_WRAPPER
    ) {
        return Err("spell is outside the provisioning trainer fixture".to_string());
    }
    set_provision_action(ctx, guid, |action| {
        action == super::provisioning::ProvisionAction::Spell(spell_id)
    })
}

fn provision_state(
    ctx: &ReducerContext,
    guid: u64,
) -> Result<super::provisioning::PlayerbotsProvisioning, String> {
    use super::provisioning::pkg_playerbots_provisioning;
    ctx.db
        .pkg_playerbots_provisioning()
        .character_guid()
        .find(guid)
        .ok_or_else(|| format!("provisioning state missing for {guid}"))
}

fn set_provision_action(
    ctx: &ReducerContext,
    guid: u64,
    wanted: impl Fn(super::provisioning::ProvisionAction) -> bool,
) -> Result<(), String> {
    use super::provisioning::pkg_playerbots_provisioning;
    let bot = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(guid)
        .next()
        .ok_or("bot missing")?;
    let cursor = super::provisioning::profile_actions(ctx, &bot)
        .map_err(|refusal| refusal.detail)?
        .iter()
        .position(|action| wanted(*action))
        .ok_or("profile action missing")?;
    let mut state = provision_state(ctx, guid)?;
    state.action_cursor = cursor as u16;
    state.next_repair_micros = 0;
    state.history.clear();
    ctx.db
        .pkg_playerbots_provisioning()
        .character_guid()
        .update(state);
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_provision_steps(
    ctx: &ReducerContext,
    guid: u64,
    count: u32,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    if count > 64 {
        return Err("provisioning fixture step count exceeds 64".to_string());
    }
    for _ in 0..count {
        use super::provisioning::pkg_playerbots_provisioning;
        let mut state = provision_state(ctx, guid)?;
        state.next_repair_micros = 0;
        ctx.db
            .pkg_playerbots_provisioning()
            .character_guid()
            .update(state);
        let bot = ctx
            .db
            .pkg_playerbots_bot()
            .by_character()
            .filter(guid)
            .next()
            .ok_or("bot missing")?;
        let _ = super::provisioning::reconcile_due(
            ctx,
            &bot,
            ctx.timestamp.to_micros_since_unix_epoch(),
        );
    }
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_provision_due(ctx: &ReducerContext, guid: u64) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    use super::provisioning::pkg_playerbots_provisioning;
    let mut state = provision_state(ctx, guid)?;
    state.next_repair_micros = 0;
    ctx.db
        .pkg_playerbots_provisioning()
        .character_guid()
        .update(state);
    runner_due_for(ctx, guid)
}

#[reducer]
pub fn playerbots_fixture_provision_reset(ctx: &ReducerContext, guid: u64) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    use super::provisioning::pkg_playerbots_provisioning;
    let mut state = provision_state(ctx, guid)?;
    state.action_cursor = 0;
    state.next_repair_micros = 0;
    state.history.clear();
    ctx.db
        .pkg_playerbots_provisioning()
        .character_guid()
        .update(state);
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_provision_remove_recovery(
    ctx: &ReducerContext,
    guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    use crate::game_player_spell;
    let spells = ctx.db.game_player_spell();
    for row in spells.by_character().filter(guid).collect::<Vec<_>>() {
        spells.id().delete(row.id);
    }
    use crate::game_item_instance;
    let items = ctx.db.game_item_instance();
    let mut banked_food = false;
    for mut row in items
        .by_owner_guid()
        .filter(guid)
        .filter(|row| matches!(row.entry, 117 | 118 | 159))
        .collect::<Vec<_>>()
    {
        if row.entry == 117 && !banked_food {
            row.slot = 39;
            row.stack_count = 9;
            items.guid().update(row);
            banked_food = true;
        } else {
            items.guid().delete(row.guid);
        }
    }
    use super::pkg_playerbots_recovery_scan;
    ctx.db
        .pkg_playerbots_recovery_scan()
        .character_guid()
        .delete(guid);
    let mut state = provision_state(ctx, guid)?;
    state.action_cursor = 0;
    state.next_repair_micros = 0;
    state.history.clear();
    use super::provisioning::pkg_playerbots_provisioning;
    ctx.db
        .pkg_playerbots_provisioning()
        .character_guid()
        .update(state);
    runner_due_for(ctx, guid)
}

#[reducer]
pub fn playerbots_fixture_provision_full_bag(
    ctx: &ReducerContext,
    guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    playerbots_fixture_provision_catalog(ctx)?;
    use crate::{game_item_instance, game_item_template};
    let items = ctx.db.game_item_instance();
    for row in items
        .by_owner_guid()
        .filter(guid)
        .filter(|row| row.slot >= 19)
        .collect::<Vec<_>>()
    {
        items.guid().delete(row.guid);
    }
    ensure_profile_item(
        ctx,
        PROVISION_QUEST_ITEM,
        "Quest Keepsake",
        1,
        0,
        0,
        0,
        false,
    )?;
    ensure_profile_item(ctx, PROVISION_FILLER, "Bag Filler", 1, 0, 0, 0, false)?;
    let mut quest = ctx
        .db
        .game_item_template()
        .entry()
        .find(PROVISION_QUEST_ITEM)
        .ok_or("quest item template missing")?;
    quest.bonding = 4;
    ctx.db.game_item_template().entry().update(quest);
    crate::items::grant_item(ctx, guid, PROVISION_QUEST_ITEM, 1)?;
    crate::items::grant_item(ctx, guid, PROVISION_FILLER, 15)?;
    set_provision_action(ctx, guid, |action| {
        matches!(
            action,
            super::provisioning::ProvisionAction::Item(item)
                if item.kind == super::provisioning::ProvisionItemKind::Food
        )
    })
}

#[reducer]
pub fn playerbots_fixture_provision_missing_resource(
    ctx: &ReducerContext,
    guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    playerbots_fixture_provision_catalog(ctx)?;
    use crate::game_item_template;
    ctx.db.game_item_template().entry().delete(1251);
    set_provision_action(ctx, guid, |action| {
        matches!(
            action,
            super::provisioning::ProvisionAction::Item(item)
                if item.kind == super::provisioning::ProvisionItemKind::Supply
                    && item.entry == 1251
        )
    })
}

#[reducer]
pub fn playerbots_fixture_provision_missing_spell(
    ctx: &ReducerContext,
    guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    use crate::{game_player_spell, game_spell, game_spell_effect};
    let spells = ctx.db.game_player_spell();
    for row in spells
        .by_character_spell()
        .filter((guid, 7386u32))
        .collect::<Vec<_>>()
    {
        spells.id().delete(row.id);
    }
    ctx.db.game_spell().spell_id().delete(7386);
    let effects = ctx.db.game_spell_effect();
    for row in effects.by_spell().filter(7386u32).collect::<Vec<_>>() {
        effects.id().delete(row.id);
    }
    set_provision_action(ctx, guid, |action| {
        action == super::provisioning::ProvisionAction::Spell(7386)
    })
}

#[reducer]
pub fn playerbots_fixture_provision_wrong_class_spell(
    ctx: &ReducerContext,
    guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let bot = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(guid)
        .next()
        .ok_or("bot missing")?;
    if ctx
        .db
        .pkg_playerbots_kit()
        .by_class_role()
        .filter((bot.class, bot.role))
        .all(|row| row.spell_id != 133)
    {
        ctx.db.pkg_playerbots_kit().insert(super::PlayerbotsKit {
            id: 0,
            class: bot.class,
            role: bot.role,
            spell_id: 133,
        });
    }
    set_provision_action(ctx, guid, |action| {
        action == super::provisioning::ProvisionAction::Spell(133)
    })
}

#[reducer]
pub fn playerbots_fixture_provision_profile_overflow(
    ctx: &ReducerContext,
    guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let bot = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(guid)
        .next()
        .ok_or("bot missing")?;
    let kits = ctx.db.pkg_playerbots_kit();
    let count = kits.by_class_role().filter((bot.class, bot.role)).count();
    for ordinal in count..=12 {
        kits.insert(super::PlayerbotsKit {
            id: 0,
            class: bot.class,
            role: bot.role,
            spell_id: 600_000 + ordinal as u32,
        });
    }
    playerbots_fixture_provision_reset(ctx, guid)
}

#[reducer]
pub fn playerbots_fixture_provision_stronger_weapon(
    ctx: &ReducerContext,
    guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    crate::items::grant_item(ctx, guid, 50, 1)?;
    use crate::game_item_instance;
    let slot = ctx
        .db
        .game_item_instance()
        .by_owner_guid()
        .filter(guid)
        .find(|row| row.entry == 50 && row.slot >= 23)
        .map(|row| row.slot)
        .ok_or("stronger weapon was not stored")?;
    crate::actor::equip_profile_upgrade(ctx, guid, slot)
        .map_err(|refusal| refusal.as_tag().to_string())?;
    set_provision_action(ctx, guid, |action| {
        matches!(
            action,
            super::provisioning::ProvisionAction::Equip(item)
                if item.kind == super::provisioning::ProvisionItemKind::Gear
        )
    })
}

#[reducer]
pub fn playerbots_fixture_provision_dead(ctx: &ReducerContext, guid: u64) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let mut entity = crate::helpers::live_entity(ctx, guid)?;
    entity.dead = true;
    entity.health = 0;
    ctx.db.game_world_entity().guid().update(entity);
    let mut state = provision_state(ctx, guid)?;
    state.next_repair_micros = 0;
    state.history.clear();
    use super::provisioning::pkg_playerbots_provisioning;
    ctx.db
        .pkg_playerbots_provisioning()
        .character_guid()
        .update(state);
    Ok(())
}

#[reducer]
pub fn playerbots_fixture_provision_levelup(ctx: &ReducerContext, guid: u64) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    crate::stats::set_character_level(ctx, guid, 9)?;
    let mut entity = crate::helpers::live_entity(ctx, guid)?;
    crate::xp::grant_xp(ctx, &mut entity, 1_000_000);
    ctx.db.game_world_entity().guid().update(entity);
    Ok(())
}
