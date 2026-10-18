#![cfg(feature = "debug_reducers")]

use spacetimedb::{reducer, ReducerContext, Table};

use crate::tables::{
    game_character, game_creature_family, game_creature_template, game_spell, game_world_entity,
};

use super::{catalog, state::*, Phase, PACKAGE};

const ENEMY: u32 = 5_097_000;
const BOSS: u32 = 5_097_001;
const VENDOR: u32 = 5_097_002;
const PET: u32 = 5_097_003;

#[reducer]
pub fn wave_defense_fixture_stage(ctx: &ReducerContext) -> Result<(), String> {
    crate::package::require_operator(ctx)?;
    if ctx.db.pkg_wave_defense_config().id().find(0).is_some() {
        return Err("Wave defense fixture requires an unconfigured private Shard".into());
    }
    for spec in catalog::SPECS {
        for &spell in spec.essential_spells {
            ctx.db
                .game_spell()
                .spell_id()
                .find(spell)
                .ok_or_else(|| format!("Import real DBC spells before staging; missing {spell}"))?;
        }
    }
    let templates = ctx.db.game_creature_template();
    for (entry, name) in [
        (ENEMY, "Fixture assault"),
        (BOSS, "Fixture captain"),
        (VENDOR, "Fixture quartermaster"),
        (PET, "Fixture hunter pet"),
    ] {
        if templates.entry().find(entry).is_some() {
            return Err(format!("Reserved fixture creature {entry} already exists"));
        }
        let mut template = templates
            .entry()
            .find(51000)
            .ok_or("Seed creature missing")?;
        template.entry = entry;
        template.name = name.into();
        template.aggro_range = 0;
        template.damage_min = 0;
        template.damage_max = 0;
        if entry == VENDOR {
            template.faction_template = 35;
        }
        if entry == PET {
            template.creature_family = 5;
            template.creature_type = 1;
            template.type_flags |= 1;
        }
        templates.insert(template);
    }
    ctx.db
        .game_creature_family()
        .family_id()
        .find(5)
        .ok_or("Seed pet family missing")?;
    let lobby_instance_id = crate::package::instance::create_lobby(ctx, PACKAGE, 189)?;
    ctx.db.pkg_wave_defense_config().insert(DefenseConfig {
        id: 0,
        enabled: true,
        lobby_instance_id,
        map_id: 189,
        x: 1200.0,
        y: 1200.0,
        z: 50.0,
        orientation: 0.0,
        spawn_x: 1260.0,
        spawn_y: 1200.0,
        spawn_z: 50.0,
        creature_entry: ENEMY,
        boss_entry: BOSS,
        vendor_entry: VENDOR,
        pet_entry: PET,
    });
    let names = [
        "Defensea", "Defenseb", "Defensec", "Defensed", "Defensee", "Defensef", "Defenseg",
        "Defenseh", "Defensei", "Defensej",
    ];
    for (index, name) in names.into_iter().enumerate() {
        let class = [1, 5, 8, 4, 3][index % 5];
        let race = if class == 3 { 3 } else { 1 };
        let guid = crate::package::create_character(
            ctx,
            &format!("{PACKAGE}.fixture.{index}"),
            name,
            race,
            class,
        )?;
        let mut character =
            crate::actor::character_by_guid(ctx, guid).ok_or("Fixture Character missing")?;
        character.online = true;
        character.map_id = 189;
        character.pending_instance_id = lobby_instance_id;
        character.x = 1200.0;
        character.y = 1200.0;
        character.z = 50.0;
        ctx.db.game_character().guid().update(character);
        let character =
            crate::actor::character_by_guid(ctx, guid).ok_or("Fixture Character missing")?;
        crate::package::character::build_player_entity(ctx, &character, character.owner_identity)
            .insert(ctx);
        crate::package::instance::enter(
            ctx,
            PACKAGE,
            guid,
            lobby_instance_id,
            (1200.0, 1200.0, 50.0, 0.0),
        )?;
    }
    Ok(())
}

#[reducer]
pub fn wave_defense_fixture_advance(ctx: &ReducerContext, seconds: u32) -> Result<(), String> {
    crate::package::require_operator(ctx)?;
    if seconds > 3600 {
        return Err("Fixture clock step exceeds one hour".into());
    }
    let elapsed = i64::from(seconds) * 1_000_000;
    for mut run in ctx.db.pkg_wave_defense_match().iter() {
        run.deadline_micros = run.deadline_micros.saturating_sub(elapsed);
        run.next_spawn_micros = run.next_spawn_micros.saturating_sub(elapsed);
        run.next_base_hit_micros = run.next_base_hit_micros.saturating_sub(elapsed);
        if run.empty_human_micros != 0 {
            run.empty_human_micros = run.empty_human_micros.saturating_sub(elapsed);
        }
        ctx.db.pkg_wave_defense_match().id().update(run);
    }
    for mut member in ctx.db.pkg_wave_defense_member().iter() {
        if member.respawn_micros != 0 {
            member.respawn_micros = member.respawn_micros.saturating_sub(elapsed);
        }
        if member.disconnected_micros != 0 {
            member.disconnected_micros = member.disconnected_micros.saturating_sub(elapsed);
        }
        ctx.db
            .pkg_wave_defense_member()
            .character_guid()
            .update(member);
    }
    for mut queued in ctx.db.pkg_wave_defense_queue().iter() {
        queued.queued_micros = queued.queued_micros.saturating_sub(elapsed);
        ctx.db
            .pkg_wave_defense_queue()
            .character_guid()
            .update(queued);
    }
    super::round::tick(ctx)
}

#[reducer]
pub fn wave_defense_fixture_clear(ctx: &ReducerContext, match_id: u64) -> Result<(), String> {
    crate::package::require_operator(ctx)?;
    let initial = ctx
        .db
        .pkg_wave_defense_match()
        .id()
        .find(match_id)
        .ok_or("Fixture match missing")?;
    if initial.phase != Phase::Combat {
        return Err("Fixture clear requires combat".into());
    }
    let killer = ctx
        .db
        .pkg_wave_defense_member()
        .by_match()
        .filter(&match_id)
        .find(|member| {
            ctx.db
                .game_world_entity()
                .guid()
                .find(member.character_guid)
                .is_some_and(|body| !body.dead)
        })
        .ok_or("Fixture clear needs a living Party member")?
        .character_guid;
    for _ in 0..64 {
        let mut run = ctx
            .db
            .pkg_wave_defense_match()
            .id()
            .find(match_id)
            .ok_or("Fixture match ended")?;
        if run.phase != Phase::Combat || run.wave != initial.wave {
            return Ok(());
        }
        run.next_spawn_micros = 0;
        ctx.db.pkg_wave_defense_match().id().update(run);
        super::round::tick(ctx)?;
        let enemies: Vec<_> = ctx
            .db
            .pkg_wave_defense_enemy()
            .by_match()
            .filter(&match_id)
            .collect();
        for enemy in enemies {
            if !crate::package::fixture::kill_creature(ctx, enemy.creature_guid, Some(killer)) {
                return Err("Fixture enemy death failed".into());
            }
        }
    }
    Err("Fixture wave exceeded 64 spawn passes".into())
}

#[reducer]
pub fn wave_defense_fixture_pressure(ctx: &ReducerContext, match_id: u64) -> Result<(), String> {
    crate::package::require_operator(ctx)?;
    let config = ctx
        .db
        .pkg_wave_defense_config()
        .id()
        .find(0)
        .ok_or("Fixture config missing")?;
    for _ in 0..300 {
        let mut run = ctx
            .db
            .pkg_wave_defense_match()
            .id()
            .find(match_id)
            .ok_or("Fixture match missing")?;
        if run.phase == Phase::Lost {
            return Ok(());
        }
        if run.phase != Phase::Combat {
            return Err("Fixture pressure requires combat".into());
        }
        run.next_base_hit_micros = 0;
        ctx.db.pkg_wave_defense_match().id().update(run);
        for enemy in ctx.db.pkg_wave_defense_enemy().by_match().filter(&match_id) {
            if let Some(mut body) = ctx.db.game_world_entity().guid().find(enemy.creature_guid) {
                body.x = config.x;
                body.y = config.y;
                body.update(ctx);
            }
        }
        super::round::tick(ctx)?;
    }
    Err("Fixture pressure did not destroy the base".into())
}

#[reducer]
pub fn wave_defense_fixture_down(ctx: &ReducerContext, guid: u64) -> Result<(), String> {
    crate::package::require_operator(ctx)?;
    ctx.db
        .pkg_wave_defense_member()
        .character_guid()
        .find(guid)
        .ok_or("Fixture member missing")?;
    let body = crate::actor::live_entity(ctx, guid)?;
    let damage = crate::package::fixture::final_damage(ctx, guid, body.health);
    let outcome = crate::package::fixture::apply_hit(
        ctx,
        guid,
        guid,
        damage,
        crate::package::fixture::Hit::weapon(crate::package::fixture::HitSource::MainHand, false),
    );
    if !outcome.killed {
        return Err("Fixture lethal hit did not kill the Character".into());
    }
    Ok(())
}
