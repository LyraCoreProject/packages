//! Bounded Operator batches at imported race and class start positions.

use crate::{game_start_position, game_world_entity};
use spacetimedb::{reducer, ReducerContext, Table};

use super::{class, pkg_playerbots_bot, Controller, ROLE_DPS, ROLE_HEALER, ROLE_TANK};

struct StartingArea {
    map_id: u32,
    zone_id: u32,
    name_stem: &'static str,
    classes: &'static [(u8, u8, u8)],
}

fn starting_area(name: &str) -> Result<StartingArea, String> {
    let (map_id, zone_id, name_stem, classes): (_, _, _, &[(u8, u8, u8)]) = match name {
        "northshire" => (0, 12, "Ns", &[(1, class::WARRIOR, ROLE_TANK), (1, class::PRIEST, ROLE_HEALER), (1, class::MAGE, ROLE_DPS)]),
        "coldridge" => (0, 1, "Cv", &[(3, class::WARRIOR, ROLE_TANK), (3, class::PRIEST, ROLE_HEALER), (7, class::MAGE, ROLE_DPS)]),
        "shadowglen" => (1, 141, "Sg", &[(4, class::WARRIOR, ROLE_TANK), (4, class::PRIEST, ROLE_HEALER)]),
        "deathknell" => (0, 85, "Dk", &[(5, class::WARRIOR, ROLE_TANK), (5, class::PRIEST, ROLE_HEALER), (5, class::MAGE, ROLE_DPS)]),
        "valley-of-trials" => (1, 14, "Vt", &[(2, class::WARRIOR, ROLE_TANK), (8, class::PRIEST, ROLE_HEALER), (8, class::MAGE, ROLE_DPS)]),
        "red-cloud-mesa" => (1, 215, "Rc", &[(6, class::WARRIOR, ROLE_TANK)]),
        _ => return Err("unknown starting area; use northshire, coldridge, shadowglen, deathknell, valley-of-trials or red-cloud-mesa".to_string()),
    };
    Ok(StartingArea {
        map_id,
        zone_id,
        name_stem,
        classes,
    })
}

/// Spawn at most 50 level-one bots per transaction. The Operator selects the Shard containing
/// the area's imported content. Missing start positions or navigation refuse the whole batch.
#[reducer]
pub fn playerbots_spawn_starting_area(
    ctx: &ReducerContext,
    area: String,
    count: u32,
    controller: Controller,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    if !(1..=50).contains(&count) {
        return Err("starting-area batches must contain 1 through 50 bots".to_string());
    }
    if !matches!(controller, Controller::Cohort | Controller::Frozen) {
        return Err("starting-area bots require Cohort or Frozen control".to_string());
    }
    let area = starting_area(&area)?;
    if !crate::nav::inputs(ctx, area.map_id).navigation_enabled {
        return Err("starting-area bots require enabled navigation".to_string());
    }
    super::ensure_defaults(ctx);
    let roster = ctx.db.pkg_playerbots_bot().count() as usize;
    for index in 0..count as usize {
        let ordinal = roster + index;
        let (race, class, role) = area.classes[ordinal % area.classes.len()];
        if !super::can_fill_role(ctx, class, role) {
            return Err(super::cannot_fill_role_message(class, role));
        }
        let start = ctx
            .db
            .game_start_position()
            .race_class()
            .find((u16::from(race) << 8) | u16::from(class))
            .ok_or("starting area is missing an imported race and class start")?;
        if (start.map_id, start.zone_id) != (area.map_id, area.zone_id) {
            return Err("imported start position belongs to another area".to_string());
        }
        let at = (0..100)
            .find_map(|attempt| {
                let (dx, dy) = super::scatter_offset(ordinal * 100 + attempt);
                let (x, y) = (start.x + dx, start.y + dy);
                let z = crate::terrain::ground_z(ctx, area.map_id, x, y)?;
                (z.is_finite() && crate::nav::walkable(ctx, area.map_id, x, y) == Some(true))
                    .then_some((x, y, z))
            })
            .ok_or("starting area has no walkable imported ground in 100 candidates")?;
        let stem = format!("{}{}", area.name_stem, super::role_name_stem(role));
        let guid = super::spawn_one(ctx, (race, class), role, &stem, area.map_id, at, 1)?;
        if controller == Controller::Frozen {
            super::runner::playerbots_select_controller(ctx, guid, controller)?;
        }
        // Validate the durable result before this batch commits.
        let body = ctx
            .db
            .game_world_entity()
            .guid()
            .find(guid)
            .ok_or("starting-area Character is absent after spawning")?;
        if body.level != 1 || body.xp != 0 {
            return Err("starting-area Character must have level one and zero XP".to_string());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starting_areas_use_supported_class_and_race_pairs() {
        for (name, expected_map) in [
            ("northshire", 0),
            ("coldridge", 0),
            ("deathknell", 0),
            ("shadowglen", 1),
            ("valley-of-trials", 1),
            ("red-cloud-mesa", 1),
        ] {
            let area = starting_area(name).unwrap();
            assert_eq!(area.map_id, expected_map);
            for &(race, class, role) in area.classes {
                assert!(match class {
                    class::WARRIOR => matches!(race, 1..=8),
                    class::PRIEST => matches!(race, 1 | 3 | 4 | 5 | 8),
                    class::MAGE => matches!(race, 1 | 5 | 7 | 8),
                    _ => false,
                });
                assert_eq!(super::super::default_class_for_role(role), Some(class));
            }
        }
        assert_eq!(
            starting_area("red-cloud-mesa").unwrap().classes,
            &[(6, class::WARRIOR, ROLE_TANK)]
        );
        assert!(starting_area("unknown").is_err());
    }
}
