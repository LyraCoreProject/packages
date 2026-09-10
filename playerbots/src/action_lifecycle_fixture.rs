#![cfg(feature = "debug_reducers")]

//! Declared Core facts for the action-lifecycle acceptance cases.

use crate::{game_creature_spawn, game_gameobject, game_world_entity};
use spacetimedb::{reducer, ReducerContext, Table};

fn place_entity(ctx: &ReducerContext, guid: u64, x: f32) -> Result<(), String> {
    let entities = ctx.db.game_world_entity();
    let mut entity = entities.guid().find(guid).ok_or("quest entity missing")?;
    entity.x = x;
    let (grid_x, grid_y) = lyracore_shared::spatial::grid_cell(entity.x, entity.y);
    entity.grid_x = grid_x;
    entity.grid_y = grid_y;
    entity.cell = lyracore_shared::spatial::grid_cell_id(grid_x, grid_y);
    entities.guid().update(entity);
    if let Some(mut spawn) = ctx.db.game_creature_spawn().guid().find(guid) {
        spawn.x = x;
        ctx.db.game_creature_spawn().guid().update(spawn);
    }
    Ok(())
}

fn place_gameobject(ctx: &ReducerContext, guid: u64, x: f32) -> Result<(), String> {
    let rows = ctx.db.game_gameobject();
    let mut row = rows.guid().find(guid).ok_or("quest GameObject missing")?;
    row.x = x;
    let (grid_x, grid_y) = lyracore_shared::spatial::grid_cell(row.x, row.y);
    row.grid_x = grid_x;
    row.grid_y = grid_y;
    row.cell = lyracore_shared::spatial::grid_cell_id(grid_x, grid_y);
    rows.guid().update(row);
    Ok(())
}

/// Put one Quest into a real planner state and move its exact target out of interaction range.
/// This reducer changes only declared quest and world facts. The ordinary Cohort pass selects the
/// root and starts its movement prerequisite.
#[reducer]
pub fn playerbots_action_lifecycle_stage_quest_plan(
    ctx: &ReducerContext,
    character_guid: u64,
    kind: u8,
    target_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    match kind {
        0 => {}
        1 => super::quest_catalog_fixture::playerbots_quest_fixture_admit_accept(
            ctx,
            character_guid,
            783,
        )?,
        2 => {
            super::quest_catalog_fixture::playerbots_quest_fixture_admit_accept(
                ctx,
                character_guid,
                33,
            )?;
            super::quest_catalog_fixture::playerbots_quest_fixture_kill(ctx, character_guid, 299)?;
        }
        3 => super::quest_catalog_fixture::playerbots_quest_fixture_admit_accept(
            ctx,
            character_guid,
            3904,
        )?,
        4 => {
            super::quest_catalog_fixture::playerbots_quest_fixture_admit_accept(
                ctx,
                character_guid,
                3904,
            )?;
            super::quest_catalog_fixture::playerbots_quest_fixture_use_gameobject(
                ctx,
                character_guid,
                161_557,
                false,
            )?;
        }
        5 | 6 => super::quest_catalog_fixture::playerbots_quest_fixture_admit_accept(
            ctx,
            character_guid,
            7,
        )?,
        _ => return Err("unknown action lifecycle quest plan".to_string()),
    }
    let character = crate::helpers::live_entity(ctx, character_guid)?;
    let x = character.x + 40.0;
    if matches!(kind, 3 | 4) {
        place_gameobject(ctx, target_guid, x)?;
    } else {
        place_entity(ctx, target_guid, x)?;
    }
    super::quest_catalog::refresh_catalog(ctx, "unknown");
    Ok(())
}
