use crate::{game_config, game_start_position, game_world_entity};
use spacetimedb::{reducer, table, ReducerContext, SpacetimeType, Table};

use super::{pkg_playerbots_bot, Controller};

#[derive(SpacetimeType, Clone)]
pub struct ImportedBot {
    pub character_guid: u64,
    pub class: u8,
    pub role: u8,
    pub map_id: u32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub level: u32,
    pub xp: u32,
    pub first_due_offset_micros: i64,
}

#[table(accessor = pkg_playerbots_imported_fixture, public)]
pub struct ImportedFixture {
    #[primary_key]
    pub id: u8,
    pub seed: u32,
    pub bots: Vec<ImportedBot>,
    pub source_revision: String,
    pub dump_sha256: String,
    pub navigation: crate::nav::NavigationInputs,
    pub staged_micros: i64,
    pub started_micros: Option<i64>,
}

fn imported_inputs(
    ctx: &ReducerContext,
    source_revision: &str,
    dump_sha256: &str,
) -> Result<crate::nav::NavigationInputs, String> {
    for (value, length) in [(source_revision, 40), (dump_sha256, 64)] {
        if value.len() != length || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("imported fixture needs exact source and dump identities".to_string());
        }
    }
    for family in [
        "creatures",
        "items",
        "loot",
        "gossip",
        "quests",
        "gameobjects",
        "trainers",
        "casts",
        "creature-ai",
        "globals",
        "spellmeta",
    ] {
        let revision = crate::actor::import_revision(ctx, family)
            .ok_or_else(|| format!("imported fixture is missing the {family} import"))?;
        if revision.source_sha != source_revision || revision.file_hash != dump_sha256 {
            return Err(format!("imported fixture has different {family} inputs"));
        }
    }
    let navigation = crate::nav::inputs(ctx, 0);
    if !navigation.navigation_enabled
        || !navigation.collision_enabled
        || !navigation.coverage_enabled
        || navigation.imported_revision.is_none()
        || navigation.static_generation.is_none()
        || navigation.static_generation != navigation.coverage_generation
    {
        return Err(
            "imported fixture needs active navigation and complete collision coverage".to_string(),
        );
    }
    if ctx.db.game_config().id().find(0).map(|row| row.xp_rate) != Some(1.0) {
        return Err("imported fixture needs the ordinary XP rate".to_string());
    }
    Ok(navigation)
}

/// Create level-one Characters and freeze them in one transaction. Imported content stays intact.
#[reducer]
pub fn playerbots_imported_stage(
    ctx: &ReducerContext,
    seed: u32,
    source_revision: String,
    dump_sha256: String,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    if !(1..=30).contains(&seed) {
        return Err("imported fixture needs seed 1 through 30".to_string());
    }
    if ctx.db.pkg_playerbots_bot().count() != 0
        || ctx
            .db
            .pkg_playerbots_imported_fixture()
            .id()
            .find(0)
            .is_some()
    {
        return Err("imported fixture needs an empty roster and a fresh manifest".to_string());
    }
    let navigation = imported_inputs(ctx, &source_revision, &dump_sha256)?;
    super::ensure_defaults(ctx);
    let mut bots = Vec::with_capacity(25);
    for index in 0..25 {
        let (class, role) = match index % 3 {
            0 => (super::class::WARRIOR, super::ROLE_TANK),
            1 => (super::class::PRIEST, super::ROLE_HEALER),
            _ => (super::class::MAGE, super::ROLE_DPS),
        };
        let start = ctx
            .db
            .game_start_position()
            .race_class()
            .find((u16::from(super::BOT_RACE) << 8) | u16::from(class))
            .ok_or("imported class start is missing")?;
        if start.map_id != 0 || start.zone_id != 12 {
            return Err("imported fixture requires the Human Northshire start".to_string());
        }
        let (x, y, z) = super::spawn_spot(ctx, (start.x, start.y, start.z), index);
        if crate::terrain::ground_z(ctx, start.map_id, x, y).is_none()
            || crate::nav::walkable(ctx, start.map_id, x, y) != Some(true)
            || !z.is_finite()
        {
            return Err(format!("imported bot {index} start lacks a walkable floor"));
        }
        let guid = super::spawn_one(
            ctx,
            class,
            role,
            super::role_name_stem(role),
            start.map_id,
            (x, y, z),
            1,
        )?;
        super::runner::playerbots_select_controller(ctx, guid, Controller::Frozen)?;
        let entity = ctx
            .db
            .game_world_entity()
            .guid()
            .find(guid)
            .ok_or("imported bot disappeared during staging")?;
        if entity.level != 1 || entity.xp != 0 {
            return Err("imported bot must start at level one with zero XP".to_string());
        }
        bots.push(ImportedBot {
            character_guid: guid,
            class,
            role,
            map_id: entity.map_id,
            x: entity.x,
            y: entity.y,
            z: entity.z,
            level: entity.level,
            xp: entity.xp,
            first_due_offset_micros: ((index as i64 + i64::from(seed)) % 10) * 100_000,
        });
    }
    ctx.db
        .pkg_playerbots_imported_fixture()
        .insert(ImportedFixture {
            id: 0,
            seed,
            bots,
            source_revision,
            dump_sha256,
            navigation,
            staged_micros: ctx.timestamp.to_micros_since_unix_epoch(),
            started_micros: None,
        });
    Ok(())
}

/// Start the complete roster once. The ordinary scheduler owns all later progression.
#[reducer]
pub fn playerbots_imported_begin(ctx: &ReducerContext) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let mut fixture = ctx
        .db
        .pkg_playerbots_imported_fixture()
        .id()
        .find(0)
        .ok_or("imported fixture has not been staged")?;
    if fixture.started_micros.is_some() {
        return Ok(());
    }
    if imported_inputs(ctx, &fixture.source_revision, &fixture.dump_sha256)? != fixture.navigation {
        return Err("imported geometry changed after staging".to_string());
    }
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    for staged in &fixture.bots {
        super::runner::playerbots_select_controller(
            ctx,
            staged.character_guid,
            Controller::Cohort,
        )?;
        let mut bot = ctx
            .db
            .pkg_playerbots_bot()
            .by_character()
            .filter(staged.character_guid)
            .next()
            .ok_or("staged imported bot is absent")?;
        bot.next_think_micros = now.saturating_add(staged.first_due_offset_micros);
        ctx.db.pkg_playerbots_bot().id().update(bot);
    }
    fixture.started_micros = Some(now);
    ctx.db
        .pkg_playerbots_imported_fixture()
        .id()
        .update(fixture);
    Ok(())
}
