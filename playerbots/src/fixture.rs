//! Deterministic staging for private, per-test durable databases.
//! Staging replaces shared rotation configuration and is not safe in a shared World Shard.

use super::pkg_playerbots_personality;
use super::{pkg_playerbots_bot, PlayerbotsBot};
use crate::nav::game_nav_chunk;
use crate::{
    game_creature_spawn, game_creature_template, game_quest_objective, game_quest_template,
};
use crate::{game_creature_spline, game_spell, game_spell_effect, game_world_entity};
use spacetimedb::{reducer, ReducerContext, Table};

const HEAL: u32 = 5_090_100;

#[reducer]
pub fn playerbots_fixture_prepare(ctx: &ReducerContext) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let bots: Vec<PlayerbotsBot> = ctx.db.pkg_playerbots_bot().iter().collect();
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
        ctx.db.game_spell_effect().id().delete(effect.id);
        ctx.db.game_spell_effect().insert(effect);
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
        crate::spell::learn_spell(ctx, guid, spacetimedb::Identity::ZERO, HEAL);
        use super::pkg_playerbots_rotation;
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
    use super::pkg_playerbots_rotation;
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
    let mut me = crate::helpers::live_entity(ctx, guid)?;
    me.health = 1_000_000_000;
    me.max_health = 4_000_000_000;
    ctx.db.game_world_entity().guid().update(me);
    Ok(())
}
