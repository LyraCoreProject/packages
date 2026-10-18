use spacetimedb::{reducer, table, ReducerContext, ScheduleAt, Table};

use crate::tables::{game_creature_template, game_world_entity};

use super::{catalog, progression, state::*, PACKAGE};

#[table(accessor = pkg_wave_defense_schedule, scheduled(wave_defense_tick))]
pub struct DefenseSchedule {
    #[primary_key]
    pub id: u64,
    pub scheduled_at: ScheduleAt,
}

#[reducer]
pub fn wave_defense_tick(ctx: &ReducerContext, _schedule: DefenseSchedule) -> Result<(), String> {
    if ctx.sender() != ctx.database_identity() {
        return Err("Scheduled operation only".into());
    }
    tick(ctx)
}

pub(crate) fn tick(ctx: &ReducerContext) -> Result<(), String> {
    let Some(config) = ctx
        .db
        .pkg_wave_defense_config()
        .id()
        .find(0)
        .filter(|row| row.enabled)
    else {
        return Ok(());
    };
    admit_lobby(ctx, &config)?;
    super::queue::tick(ctx, &config)?;
    super::reconnect::tick(ctx, &config)?;
    for mut run in ctx.db.pkg_wave_defense_match().iter().take(64) {
        let now = ctx.timestamp.to_micros_since_unix_epoch();
        let humans_present = ctx
            .db
            .pkg_wave_defense_member()
            .by_match()
            .filter(&run.id)
            .any(|member| !member.bot && member.active());
        if humans_present {
            run.empty_human_micros = 0;
        } else if run.empty_human_micros == 0 {
            run.empty_human_micros = now;
        } else if now >= run.empty_human_micros.saturating_add(120_000_000) {
            finish(ctx, &config, &run)?;
            continue;
        }
        for mut member in ctx
            .db
            .pkg_wave_defense_member()
            .by_match()
            .filter(&run.id)
            .filter(DefenseMember::active)
        {
            let Some(body) = ctx
                .db
                .game_world_entity()
                .guid()
                .find(member.character_guid)
            else {
                continue;
            };
            if body.instance_id != run.instance_id || body.map_id != config.map_id {
                continue;
            }
            if body.dead {
                if member.respawn_micros == 0 {
                    member.respawn_micros = now + 15_000_000;
                } else if now >= member.respawn_micros {
                    crate::package::character::respawn(
                        ctx,
                        member.character_guid,
                        (config.x, config.y, config.z, config.orientation),
                    )?;
                    member.respawn_micros = 0;
                }
            } else {
                member.respawn_micros = 0;
            }
            if body.level != u32::from(run.level) {
                if member.bot {
                    crate::pkg_playerbots::update_wave_defense_bot_level(
                        ctx,
                        member.character_guid,
                        run.level,
                    )?;
                }
                progression::refresh_level(ctx, &member, run.level)?;
            }
            if run.phase == Phase::Preparation {
                progression::prepare_offer(ctx, &mut member, run.level)?;
            }
            ctx.db
                .pkg_wave_defense_member()
                .character_guid()
                .update(member);
        }
        match run.phase {
            Phase::Preparation => {
                let members: Vec<_> = ctx
                    .db
                    .pkg_wave_defense_member()
                    .by_match()
                    .filter(&run.id)
                    .filter(DefenseMember::active)
                    .collect();
                let all_ready = members.len() == 5 && members.iter().all(|member| member.ready);
                if now >= run.deadline_micros || all_ready {
                    run.phase = Phase::Combat;
                    run.wave_started_micros = now;
                    run.next_spawn_micros = now;
                    run.spawned = 0;
                    run.defeated = 0;
                }
            }
            Phase::Combat => {
                advance_combat(ctx, &config, &mut run)?;
            }
            Phase::Won | Phase::Lost
                if now >= run.deadline_micros && has_lobby_space(ctx, &config, &run) =>
            {
                finish(ctx, &config, &run)?;
                continue;
            }
            Phase::Won | Phase::Lost => {}
        }
        ctx.db.pkg_wave_defense_match().id().update(run);
    }
    Ok(())
}

fn admit_lobby(ctx: &ReducerContext, config: &DefenseConfig) -> Result<(), String> {
    for mut profile in ctx
        .db
        .pkg_wave_defense_profile()
        .by_lobby_pending()
        .filter(true)
        .take(200)
    {
        let guid = profile.character_guid;
        if ctx
            .db
            .pkg_wave_defense_member()
            .character_guid()
            .find(guid)
            .is_some()
        {
            profile.lobby_pending = false;
            ctx.db
                .pkg_wave_defense_profile()
                .character_guid()
                .update(profile);
            continue;
        }
        let Some(body) = ctx.db.game_world_entity().guid().find(guid) else {
            continue;
        };
        if body.instance_id != config.lobby_instance_id && lobby_occupants(ctx, config) >= 200 {
            break;
        }
        crate::package::character::reset_loadout(ctx, guid)?;
        crate::package::character::return_to_lobby(
            ctx,
            guid,
            config.map_id,
            config.lobby_instance_id,
            (config.x, config.y, config.z, config.orientation),
        )?;
        if body.map_id == config.map_id {
            profile.lobby_pending = false;
            ctx.db
                .pkg_wave_defense_profile()
                .character_guid()
                .update(profile);
        }
    }
    Ok(())
}

fn lobby_occupants(ctx: &ReducerContext, config: &DefenseConfig) -> usize {
    ctx.db
        .game_world_entity()
        .by_cell()
        .filter((config.map_id, config.lobby_instance_id))
        .filter(|body| body.is_player())
        .take(200)
        .count()
}

fn has_lobby_space(ctx: &ReducerContext, config: &DefenseConfig, run: &DefenseMatch) -> bool {
    let returning = ctx
        .db
        .pkg_wave_defense_member()
        .by_match()
        .filter(&run.id)
        .filter(|member| {
            !member.bot
                && ctx
                    .db
                    .game_world_entity()
                    .guid()
                    .find(member.character_guid)
                    .is_some_and(|body| {
                        body.map_id == config.map_id && body.instance_id != config.lobby_instance_id
                    })
        })
        .count();
    returning <= 200usize.saturating_sub(lobby_occupants(ctx, config))
}

pub(crate) fn spawn_shops(
    ctx: &ReducerContext,
    config: &DefenseConfig,
    run: &DefenseMatch,
) -> Result<(), String> {
    if ctx
        .db
        .game_creature_template()
        .entry()
        .find(config.vendor_entry)
        .is_none()
    {
        return Err("Quartermaster is not imported".into());
    }
    let guids = crate::package::encounter::spawn_wave(
        ctx,
        run.instance_id,
        0,
        config.map_id,
        &[config.vendor_entry],
        config.x + 6.0,
        config.y,
        config.z,
        config.orientation,
    );
    if guids.len() != 1 {
        return Err("Quartermaster could not enter the Instance".into());
    }
    Ok(())
}

fn advance_combat(
    ctx: &ReducerContext,
    config: &DefenseConfig,
    run: &mut DefenseMatch,
) -> Result<(), String> {
    let wave = catalog::wave(run.wave).ok_or("Invalid wave")?;
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    if run.spawned < wave.spawn_count && now >= run.next_spawn_micros {
        let boss = wave.boss && run.spawned + 1 == wave.spawn_count;
        let entry = if boss {
            config.boss_entry
        } else {
            config.creature_entry
        };
        let flank = match wave.fronts {
            2 => {
                if run.spawned % 2 == 0 {
                    -6.0
                } else {
                    6.0
                }
            }
            3 => (f32::from(run.spawned % 3) - 1.0) * 6.0,
            _ => 0.0,
        };
        let guids = crate::package::encounter::spawn_wave(
            ctx,
            run.instance_id,
            u32::from(run.wave),
            config.map_id,
            &[entry],
            config.spawn_x,
            config.spawn_y + flank,
            config.spawn_z,
            config.orientation,
        );
        let guid = *guids
            .first()
            .ok_or("Wave creature could not enter the Instance")?;
        let mut body = crate::actor::live_entity(ctx, guid)?;
        body.level = u32::from(run.level);
        let baseline = 40 + u32::from(run.level).pow(2) / 2;
        let health_scale = if boss {
            u32::from(wave.max_health_permille) * 3
        } else {
            1000 + u32::from(run.wave - 1) * 75
        };
        body.max_health = baseline.saturating_mul(health_scale) / 1000;
        body.health = body.max_health;
        body.sheet_dmg_min =
            (2 + u32::from(run.level) * 2) * u32::from(wave.damage_permille) / 1000;
        if boss {
            body.sheet_dmg_min = body.sheet_dmg_min.saturating_mul(3);
        }
        body.sheet_dmg_max = body.sheet_dmg_min.saturating_mul(3) / 2;
        body.armor = u32::from(run.level) * 20;
        body.update(ctx);
        ctx.db.pkg_wave_defense_enemy().insert(DefenseEnemy {
            creature_guid: guid,
            match_id: run.id,
            boss,
            moving_until_micros: 0,
            next_ability_micros: now + 8_000_000,
            ability_count: 0,
        });
        run.spawned += 1;
        let interval =
            i64::from(wave.target_combat_seconds) * 700_000 / i64::from(wave.spawn_count.max(1));
        run.next_spawn_micros = now + interval.max(1_000_000);
    }
    let mut attackers = 0u32;
    let mut targets = Vec::new();
    for mut enemy in ctx.db.pkg_wave_defense_enemy().by_match().filter(&run.id) {
        let Some(body) = ctx.db.game_world_entity().guid().find(enemy.creature_guid) else {
            continue;
        };
        if body.dead {
            continue;
        }
        if enemy.boss && body.target_guid != 0 && now >= enemy.next_ability_micros {
            let spell = if run.wave == 5 || run.wave == 15 && enemy.ability_count % 2 == 0 {
                15496
            } else {
                16727
            };
            let _ = crate::actor::request_cast(ctx, enemy.creature_guid, spell, body.target_guid);
            enemy.ability_count = enemy.ability_count.saturating_add(1);
            enemy.next_ability_micros = now + 8_000_000;
        }
        targets.push(enemy.creature_guid);
        let distance = (body.x - config.x).hypot(body.y - config.y);
        if distance <= 6.0 {
            attackers = attackers.saturating_add(if enemy.boss { 250 } else { 35 });
        } else if body.target_guid == 0 && now >= enemy.moving_until_micros {
            let route = crate::actor::movement::route_path_with_budget(
                ctx,
                config.map_id,
                run.instance_id,
                (body.x, body.y, body.z),
                (config.x, config.y, config.z),
                0.0,
                512,
            );
            if let Some(point) = route.points.first() {
                crate::package::encounter::move_to_point(
                    ctx,
                    enemy.creature_guid,
                    point.0,
                    point.1,
                    point.2,
                    true,
                )?;
                enemy.moving_until_micros = now + 1_000_000;
            }
        }
        ctx.db
            .pkg_wave_defense_enemy()
            .creature_guid()
            .update(enemy);
    }
    if now >= run.next_base_hit_micros {
        run.base_health = run.base_health.saturating_sub(attackers);
        run.next_base_hit_micros = now + 2_000_000;
    }
    for member in ctx
        .db
        .pkg_wave_defense_member()
        .by_match()
        .filter(&run.id)
        .filter(|member| member.bot)
    {
        crate::pkg_playerbots::set_wave_defense_objective(
            ctx,
            member.character_guid,
            (config.x, config.y, config.z),
            &targets,
        )?;
    }
    if run.base_health == 0 {
        run.phase = Phase::Lost;
        run.deadline_micros = now + 20_000_000;
        crate::package::encounter::encounter_reset(ctx, run.instance_id, u32::from(run.wave));
        for enemy in ctx.db.pkg_wave_defense_enemy().by_match().filter(&run.id) {
            ctx.db
                .pkg_wave_defense_enemy()
                .creature_guid()
                .delete(enemy.creature_guid);
        }
    } else if run.spawned == wave.spawn_count && run.defeated == wave.spawn_count {
        let won = run.wave == 15;
        for mut member in ctx.db.pkg_wave_defense_member().by_match().filter(&run.id) {
            progression::pay(
                ctx,
                member.character_guid,
                10_000 + u32::from(run.wave) * 5_000,
            );
            progression::record_wave(ctx, &member, wave.boss, won);
            crate::package::character::refill(ctx, member.character_guid);
            if member.class == 3 && member.active() {
                crate::package::character::provision_hunter_pet(
                    ctx,
                    member.character_guid,
                    config.pet_entry,
                    true,
                )?;
            }
            member.ready = member.bot;
            ctx.db
                .pkg_wave_defense_member()
                .character_guid()
                .update(member);
        }
        if wave.boss {
            run.base_health = run.base_health.saturating_add(2500).min(10_000);
        }
        run.phase = if won { Phase::Won } else { Phase::Preparation };
        if !won {
            run.wave += 1;
        }
        run.deadline_micros = now + i64::from(catalog::PREPARATION_SECONDS) * 1_000_000;
    }
    Ok(())
}

pub(crate) fn enemy_death(ctx: &ReducerContext, guid: u64) {
    let Some(enemy) = ctx.db.pkg_wave_defense_enemy().creature_guid().find(guid) else {
        return;
    };
    let Some(mut run) = ctx.db.pkg_wave_defense_match().id().find(enemy.match_id) else {
        return;
    };
    ctx.db.pkg_wave_defense_enemy().creature_guid().delete(guid);
    if run.phase != Phase::Combat {
        return;
    }
    run.defeated = run.defeated.saturating_add(1);
    if let Some(wave) = catalog::wave(run.wave) {
        let next_level = catalog::wave(run.wave + 1).map_or(60, |next| next.level);
        let start_xp = u32::from(wave.level.saturating_sub(1)) * 100;
        let budget = u32::from(next_level.saturating_sub(wave.level)) * 100;
        run.team_xp =
            start_xp + budget * u32::from(run.defeated) / u32::from(wave.spawn_count.max(1));
        run.level = (1 + run.team_xp / 100).min(60) as u8;
    }
    ctx.db.pkg_wave_defense_match().id().update(run);
}

fn finish(ctx: &ReducerContext, config: &DefenseConfig, run: &DefenseMatch) -> Result<(), String> {
    for member in ctx.db.pkg_wave_defense_member().by_match().filter(&run.id) {
        cleanup_member(ctx, config, &member)?;
    }
    for enemy in ctx.db.pkg_wave_defense_enemy().by_match().filter(&run.id) {
        ctx.db
            .pkg_wave_defense_enemy()
            .creature_guid()
            .delete(enemy.creature_guid);
    }
    crate::package::instance::close(ctx, PACKAGE, run.instance_id)?;
    ctx.db.pkg_wave_defense_match().id().delete(run.id);
    Ok(())
}

fn cleanup_member(
    ctx: &ReducerContext,
    config: &DefenseConfig,
    member: &DefenseMember,
) -> Result<(), String> {
    crate::package::combat::disable(ctx, PACKAGE, member.character_guid)?;
    if member.bot {
        crate::pkg_playerbots::remove_wave_defense_bot(ctx, member.character_guid)?;
    } else {
        crate::package::character::reset_loadout(ctx, member.character_guid)?;
        crate::package::character::return_to_lobby(
            ctx,
            member.character_guid,
            config.map_id,
            config.lobby_instance_id,
            (config.x, config.y, config.z, config.orientation),
        )?;
    }
    ctx.db
        .pkg_wave_defense_member()
        .character_guid()
        .delete(member.character_guid);
    Ok(())
}

pub(crate) fn leave(
    ctx: &ReducerContext,
    guid: u64,
    match_id: u64,
    config: &DefenseConfig,
) -> Result<(), String> {
    if let Some(member) = ctx.db.pkg_wave_defense_member().character_guid().find(guid) {
        if member.match_id != match_id {
            return Err("That match has ended".into());
        }
        super::reconnect::leave(ctx, guid, config)?;
        cleanup_member(ctx, config, &member)?;
    } else if match_id == 0 {
        ctx.db
            .pkg_wave_defense_queue()
            .character_guid()
            .delete(guid);
    } else {
        return Err("That match has ended".into());
    }
    Ok(())
}

pub(crate) fn login(ctx: &ReducerContext, guid: u64) {
    if ctx
        .db
        .pkg_wave_defense_config()
        .id()
        .find(0)
        .is_none_or(|row| !row.enabled)
    {
        return;
    }
    if ctx
        .db
        .pkg_wave_defense_member()
        .character_guid()
        .find(guid)
        .is_some()
    {
        super::reconnect::login(ctx, guid);
        return;
    }
    let Some(character) = crate::actor::character_by_guid(ctx, guid) else {
        return;
    };
    let profiles = ctx.db.pkg_wave_defense_profile();
    if let Some(mut profile) = profiles.character_guid().find(guid) {
        profile.lobby_pending = true;
        profiles.character_guid().update(profile);
    } else {
        profiles.insert(DefenseProfile {
            character_guid: guid,
            class: character.class,
            selected: false,
            last_request: 0,
            lobby_pending: true,
        });
    }
}

pub(crate) fn logout(ctx: &ReducerContext, guid: u64) {
    ctx.db
        .pkg_wave_defense_queue()
        .character_guid()
        .delete(guid);
    if let Some(mut member) = ctx.db.pkg_wave_defense_member().character_guid().find(guid) {
        member.disconnected_micros = ctx.timestamp.to_micros_since_unix_epoch();
        member.resume_requested = false;
        member.ready = false;
        ctx.db
            .pkg_wave_defense_member()
            .character_guid()
            .update(member);
    }
}
