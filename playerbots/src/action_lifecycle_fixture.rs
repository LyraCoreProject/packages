#![cfg(feature = "debug_reducers")]

//! Declared Core facts for the action-lifecycle acceptance cases.

use crate::{game_creature_spawn, game_creature_spline, game_gameobject, game_world_entity};
use spacetimedb::{reducer, ReducerContext};

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
            let source = ctx
                .db
                .game_world_entity()
                .guid()
                .find(target_guid)
                .ok_or("declared Quest creature source missing")?;
            if source.entry != 69 || source.dead || source.health == 0 {
                return Err("declared Quest creature source changed".to_string());
            }
            super::quest_catalog_fixture::playerbots_quest_fixture_kill(ctx, character_guid, 69)?;
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
    let x = character.x + if kind == 6 { 80.0 } else { 40.0 };
    if matches!(kind, 3 | 4) {
        place_gameobject(ctx, target_guid, x)?;
    } else {
        place_entity(ctx, target_guid, x)?;
    }
    super::quest_catalog::refresh_catalog(ctx, "unknown");
    Ok(())
}

/// Observe one real movement step, then exercise objective expiry before Core can advance again.
#[reducer]
pub fn playerbots_action_lifecycle_expire_movement(
    ctx: &ReducerContext,
    character_guid: u64,
    objective_identity: u64,
    expected_move: u8,
    expected_target: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    super::fixture::playerbots_fixture_runner_pass_once(ctx, character_guid)?;

    use super::decision::{Action, MoveTarget};
    use super::runner::pkg_playerbots_runner;
    use super::runner::{ObjectiveKind, ObjectiveStage, Running};
    let rows = ctx.db.pkg_playerbots_runner();
    let mut state = rows
        .character_guid()
        .find(character_guid)
        .ok_or("runner missing")?;
    let objective = state.objective.as_mut().ok_or("objective missing")?;
    if objective.identity != objective_identity
        || objective.stage != ObjectiveStage::Travelling
        || objective.deadline_micros == i64::MAX
        || objective.deadline_micros <= ctx.timestamp.to_micros_since_unix_epoch()
    {
        return Err("movement expiry objective changed before observation".to_string());
    }
    let foreground = state
        .foreground
        .as_ref()
        .ok_or("movement expiry foreground missing")?;
    let expected_action = match expected_move {
        0 => Action::Move(MoveTarget::Home),
        1 => Action::Move(MoveTarget::Entity(expected_target)),
        2 => Action::Move(MoveTarget::GameObject(expected_target)),
        3 => Action::Move(MoveTarget::CastingPosition(expected_target)),
        4 => Action::Move(MoveTarget::AreaTrigger(
            u32::try_from(expected_target).map_err(|_| "movement expiry trigger exceeds u32")?,
        )),
        _ => return Err("unknown movement expiry root".to_string()),
    };
    let expected_kind = if matches!(expected_move, 0 | 4) {
        ObjectiveKind::ReturnHome
    } else {
        ObjectiveKind::Quest
    };
    let expected_reason = if expected_move == 0 {
        super::decision::Reason::ReturnHome
    } else if expected_move == 4 {
        super::decision::Reason::TransferPosition
    } else {
        super::decision::Reason::Quest
    };
    if objective.kind != expected_kind
        || foreground.candidate.id.objective != objective_identity
        || foreground.candidate.id.action != expected_action
        || foreground.candidate.id.reason != expected_reason
        || !matches!(foreground.running, Running::Movement(_))
    {
        return Err("movement expiry no longer owns the expected movement".to_string());
    }
    let spline = ctx
        .db
        .game_creature_spline()
        .guid()
        .find(character_guid)
        .ok_or("movement expiry spline missing")?;
    if spline.dur_ms == 0 || spline.start_micros != foreground.started_micros as u64 {
        return Err("movement expiry spline is not the current owned leg".to_string());
    }
    let me = ctx
        .db
        .game_world_entity()
        .guid()
        .find(character_guid)
        .ok_or("movement expiry body missing")?;
    if me.dead
        || (me.map_id, me.instance_id) != (foreground.map_id, foreground.instance_id)
        || objective.last_verified_progress_micros
            != Some(ctx.timestamp.to_micros_since_unix_epoch())
    {
        return Err("movement expiry did not observe current living-body progress".to_string());
    }
    let movement =
        super::actions::observation(ctx, character_guid, super::actions::ActionKind::Move)
            .ok_or("movement expiry observation missing")?;
    let super::actions::ActionOutcome::Movement(observed) = movement.outcome else {
        return Err("movement expiry observation is not movement".to_string());
    };
    if movement.observed_micros != foreground.started_micros
        || (observed.map_id, observed.instance_id) != (me.map_id, me.instance_id)
    {
        return Err("movement expiry observation does not match its foreground".to_string());
    }
    if expected_move == 4 {
        let trigger =
            u32::try_from(expected_target).map_err(|_| "movement expiry trigger exceeds u32")?;
        let route = crate::actor::area_trigger_route(ctx, trigger)
            .ok_or("movement expiry trigger route missing")?;
        if me.map_id != route.source_map || route.contains(me.x, me.y, me.z) {
            return Err("movement expiry body reached its trigger before expiry".to_string());
        }
    }

    objective.deadline_micros = ctx.timestamp.to_micros_since_unix_epoch();
    rows.character_guid().update(state);
    super::fixture::playerbots_fixture_runner_pass_once(ctx, character_guid)
}
