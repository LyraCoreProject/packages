use crate::nav::game_nav_chunk;
use crate::terrain::game_terrain_chunk;
use spacetimedb::{reducer, table, ReducerContext, SpacetimeType, Table};

use super::{pkg_playerbots_bot, Controller};

const ORIGIN: (f32, f32, f32) = (1200.0, 1200.0, 50.0);
const TRAVEL_YARDS: f32 = 700.0;
const WALL_X: f32 = 1400.0;
const WALL_Y: std::ops::RangeInclusive<f32> = 1190.0..=1225.0;

#[derive(SpacetimeType, Clone)]
pub struct LoadBlockedCell {
    pub key: u64,
    pub sub_x: u8,
    pub sub_y: u8,
}

#[derive(SpacetimeType, Clone)]
pub struct LoadBot {
    pub character_guid: u64,
    pub class: u8,
    pub role: u8,
    pub x: f32,
    pub y: f32,
    pub home_x: f32,
    pub first_due_offset_micros: i64,
}

#[table(accessor = pkg_playerbots_load_fixture, public)]
pub struct LoadFixture {
    #[primary_key]
    pub id: u8,
    pub seed: u32,
    pub bots: Vec<LoadBot>,
    pub geometry_keys: Vec<u64>,
    pub staged_micros: i64,
    pub started_micros: Option<i64>,
    pub blocked_navigation_cells: Vec<LoadBlockedCell>,
}

fn route_geometry(ctx: &ReducerContext) -> Result<(Vec<u64>, Vec<LoadBlockedCell>), String> {
    use lyracore_shared::nav::{sub_center, sub_index, walk_set, WALK_DIM};
    use lyracore_shared::terrain::{cell_index, cell_key};
    let x0 = cell_index(ORIGIN.0 - 40.0).ok_or("load origin is off grid")?;
    let x1 = cell_index(ORIGIN.0 + TRAVEL_YARDS + 40.0).ok_or("load end is off grid")?;
    let y0 = cell_index(ORIGIN.1 - 40.0).ok_or("load origin is off grid")?;
    let y1 = cell_index(ORIGIN.1 + 40.0).ok_or("load end is off grid")?;
    let mut keys = Vec::new();
    let mut blocked = Vec::new();
    for x in x0.min(x1)..=x0.max(x1) {
        for y in y0.min(y1)..=y0.max(y1) {
            let key = cell_key(0, x, y);
            if ctx.db.game_terrain_chunk().key().find(key).is_some()
                || ctx.db.game_nav_chunk().key().find(key).is_some()
            {
                return Err("load geometry must not replace existing input".to_string());
            }
            ctx.db
                .game_terrain_chunk()
                .insert(crate::terrain::TerrainChunk {
                    key,
                    map_id: 0,
                    cell_x: x,
                    cell_y: y,
                    heights: vec![ORIGIN.2; 145],
                    liquid_level: 0.0,
                    has_liquid: false,
                    holes: 0,
                    area_id: 0,
                });
            let mut walk = vec![255; lyracore_shared::nav::WALK_BYTES];
            if let Some(nx) = sub_index(WALL_X, x, WALK_DIM) {
                for ny in 0..WALK_DIM {
                    if WALL_Y.contains(&sub_center(y, ny, WALK_DIM)) {
                        walk_set(&mut walk, nx, ny, false);
                        blocked.push(LoadBlockedCell {
                            key,
                            sub_x: nx as u8,
                            sub_y: ny as u8,
                        });
                    }
                }
            }
            ctx.db.game_nav_chunk().insert(crate::nav::NavChunk {
                key,
                map_id: 0,
                cell_x: x,
                cell_y: y,
                base_z: ORIGIN.2,
                walk,
                obs: vec![lyracore_shared::nav::OBS_NONE; lyracore_shared::nav::OBS_BYTES],
            });
            keys.push(key);
        }
    }
    if blocked.is_empty() {
        return Err("load route has no blocked navigation cells".to_string());
    }
    crate::nav::record_change(ctx)?;
    Ok((keys, blocked))
}

/// Stage a declared route around a wall. The ordinary Core tick owns measured movement.
#[reducer]
pub fn playerbots_load_stage(ctx: &ReducerContext, count: u32, seed: u32) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    if !matches!(count, 10 | 25 | 100) || !(1..=30).contains(&seed) {
        return Err("load fixture needs 10, 25 or 100 bots and seed 1 through 30".to_string());
    }
    if ctx.db.pkg_playerbots_bot().count() != 0
        || ctx.db.pkg_playerbots_load_fixture().id().find(0).is_some()
    {
        return Err("load fixture needs an empty roster and a fresh manifest".to_string());
    }
    super::ensure_defaults(ctx);
    let (geometry_keys, blocked_navigation_cells) = route_geometry(ctx)?;
    let mut bots = Vec::with_capacity(count as usize);
    for index in 0..count {
        let (class, role) = match index % 3 {
            0 => (super::class::WARRIOR, super::ROLE_TANK),
            1 => (super::class::PRIEST, super::ROLE_HEALER),
            _ => (super::class::MAGE, super::ROLE_DPS),
        };
        let x = ORIGIN.0 + (index % 10) as f32 * 1.5;
        let y = ORIGIN.1 + (index / 10) as f32 * 1.5 + seed as f32 * 0.01;
        let guid = super::spawn_one(
            ctx,
            class,
            role,
            super::role_name_stem(role),
            0,
            (x, y, ORIGIN.2),
            super::spawn_level(ctx),
        )?;
        super::runner::playerbots_select_controller(ctx, guid, Controller::Frozen)?;
        let mut bot = ctx
            .db
            .pkg_playerbots_bot()
            .by_character()
            .filter(guid)
            .next()
            .ok_or("load bot disappeared during staging")?;
        bot.home_x = x + TRAVEL_YARDS;
        ctx.db.pkg_playerbots_bot().id().update(bot);
        bots.push(LoadBot {
            character_guid: guid,
            class,
            role,
            x,
            y,
            home_x: x + TRAVEL_YARDS,
            first_due_offset_micros: i64::from((index + seed) % 10) * 100_000,
        });
    }
    ctx.db.pkg_playerbots_load_fixture().insert(LoadFixture {
        id: 0,
        seed,
        bots,
        geometry_keys,
        staged_micros: ctx.timestamp.to_micros_since_unix_epoch(),
        started_micros: None,
        blocked_navigation_cells,
    });
    Ok(())
}

#[reducer]
pub fn playerbots_load_begin(ctx: &ReducerContext) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let mut fixture = ctx
        .db
        .pkg_playerbots_load_fixture()
        .id()
        .find(0)
        .ok_or("load fixture has not been staged")?;
    if fixture.started_micros.is_some() {
        return Ok(());
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
            .ok_or("staged load bot is absent")?;
        bot.next_think_micros = now.saturating_add(staged.first_due_offset_micros);
        ctx.db.pkg_playerbots_bot().id().update(bot);
    }
    fixture.started_micros = Some(now);
    ctx.db.pkg_playerbots_load_fixture().id().update(fixture);
    Ok(())
}
