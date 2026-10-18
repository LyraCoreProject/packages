use spacetimedb::{reducer, ReducerContext, Table, TimeDuration};

use crate::tables::{
    game_creature_family, game_creature_template, game_item_template, game_npc_vendor, game_spell,
    game_talent, game_talent_tab, game_world_import, NpcVendor,
};

use super::round::{pkg_wave_defense_schedule, DefenseSchedule};
use super::{catalog, pkg_wave_defense_config, pkg_wave_defense_match, DefenseConfig, PACKAGE};

pub(crate) const MAP_ID: u32 = 189;
pub(crate) const CREATURE_ENTRY: u32 = 15_189_001;
pub(crate) const BOSS_ENTRY: u32 = 15_189_002;
pub(crate) const VENDOR_ENTRY: u32 = 15_189_003;
pub(crate) const PET_ENTRY: u32 = 15_189_004;
pub(crate) const FIRST_ITEM: u32 = 7_189_000;
pub(crate) const STOCK_COUNT: u32 = 51;
const VENDOR_STOCK_ID_BASE: u64 = 15_189_003_000;
pub(crate) const BASE: (f32, f32, f32) = (947.376, 1375.67, 18.1066);
pub(crate) const SPAWN: (f32, f32, f32) = (909.251, 1378.57, 18.0794);

fn require_content(ctx: &ReducerContext) -> Result<(), String> {
    for entry in [CREATURE_ENTRY, BOSS_ENTRY] {
        let creature = ctx.db.game_creature_template().entry().find(entry)
            .ok_or_else(|| format!("Missing authored undead template {entry}; build and apply the creatures Package Delta"))?;
        if creature.creature_type != 6 || creature.health == 0 {
            return Err(format!(
                "Authored creature {entry} must be a living undead combatant"
            ));
        }
    }
    let vendor = ctx
        .db
        .game_creature_template()
        .entry()
        .find(VENDOR_ENTRY)
        .ok_or("Missing authored quartermaster; apply the creatures Package Delta")?;
    if vendor.npc_flags & 128 == 0 || vendor.faction_template != 35 || vendor.aggro_range != 0 {
        return Err("The quartermaster must be a neutral vendor without aggression".into());
    }
    for entry in FIRST_ITEM..FIRST_ITEM + STOCK_COUNT {
        let item = ctx
            .db
            .game_item_template()
            .entry()
            .find(entry)
            .ok_or_else(|| {
                format!("Missing authored shop item {entry}; apply the items Package Delta")
            })?;
        if item.sell_price != 0 || item.required_level > 60 || item.buy_price > 80_000 {
            return Err(format!(
                "Authored shop item {entry} has invalid match pricing or level"
            ));
        }
    }
    let pet = ctx
        .db
        .game_creature_template()
        .entry()
        .find(PET_ENTRY)
        .ok_or("Missing authored defense wolf")?;
    let family = ctx
        .db
        .game_creature_family()
        .family_id()
        .find(u32::from(pet.creature_family))
        .ok_or("Import CreatureFamily.dbc before enabling the defense wolf")?;
    if pet.creature_type != 1 || family.pet_talent_type < 0 {
        return Err("The defense wolf must belong to a tameable beast family".into());
    }
    if ctx.db.game_creature_template().entry().find(416).is_none() {
        return Err(
            "Import the Warlock Imp creature template 416 before enabling wave defense".into(),
        );
    }
    for entry in [5175, 5176, 5177, 5178] {
        if ctx.db.game_item_template().entry().find(entry).is_none() {
            return Err(format!("Missing Shaman totem item {entry}"));
        }
    }
    for spec in &catalog::SPECS {
        for spell in spec.essential_spells.iter().chain(spec.draft_spells) {
            if ctx.db.game_spell().spell_id().find(*spell).is_none() {
                return Err(format!("Missing launch spell {spell} for {}", spec.name));
            }
        }
        let tab = ctx
            .db
            .game_talent_tab()
            .tab_id()
            .find(spec.talent_tree)
            .ok_or_else(|| format!("Missing talent tree {} for {}", spec.talent_tree, spec.name))?;
        if tab.class_mask & (1 << (spec.class - 1)) == 0 {
            return Err(format!(
                "Talent tree {} does not belong to {}",
                spec.talent_tree, spec.name
            ));
        }
        let talents = ctx
            .db
            .game_talent()
            .by_tab()
            .filter(spec.talent_tree)
            .collect::<Vec<_>>();
        if talents.is_empty() {
            return Err(format!("No talents imported for {}", spec.name));
        }
        for talent in talents {
            let spells = [
                talent.spell_id,
                talent.rank_spell_2,
                talent.rank_spell_3,
                talent.rank_spell_4,
                talent.rank_spell_5,
            ];
            if talent.max_rank == 0
                || talent.max_rank > 5
                || spells
                    .iter()
                    .take(usize::from(talent.max_rank))
                    .any(|spell| {
                        *spell == 0 || ctx.db.game_spell().spell_id().find(*spell).is_none()
                    })
            {
                return Err(format!(
                    "Missing full-rank spells for talent {}",
                    talent.talent_id
                ));
            }
            if talent.grant_spell_id != 0
                && ctx
                    .db
                    .game_spell()
                    .spell_id()
                    .find(talent.grant_spell_id)
                    .is_none()
            {
                return Err(format!(
                    "Missing granted spell for talent {}",
                    talent.talent_id
                ));
            }
        }
    }
    for spell in [
        355, 6673, 2050, 1243, 133, 168, 15496, 16727, 750, 8737, 674, 3127, 9634, 1178, 9635,
        20608, 21169, 21178,
    ] {
        if ctx.db.game_spell().spell_id().find(spell).is_none() {
            return Err(format!("Missing wave defense support spell {spell}"));
        }
    }
    Ok(())
}

fn require_courtyard(ctx: &ReducerContext) -> Result<(), String> {
    if ctx.db.game_world_import().map_id().find(MAP_ID).is_none() {
        return Err("Import map 189 before enabling wave defense".into());
    }
    let inputs = crate::actor::movement::inputs(ctx, MAP_ID);
    if !inputs.navigation_enabled
        || (inputs.imported_revision.is_none() && inputs.coverage_generation.is_none())
        || !inputs.collision_enabled
        || inputs.static_generation.is_none()
    {
        return Err(
            "Wave defense needs enabled navigation and imported map 189 collision geometry".into(),
        );
    }
    let fronts = [
        (SPAWN.0, SPAWN.1 - 6.0, SPAWN.2),
        SPAWN,
        (SPAWN.0, SPAWN.1 + 6.0, SPAWN.2),
    ];
    let vendor = (BASE.0 + 6.0, BASE.1, BASE.2);
    let routes = [
        ("south front", fronts[0]),
        ("center front", fronts[1]),
        ("north front", fronts[2]),
        ("quartermaster", vendor),
    ];
    for (name, point) in std::iter::once(("base", BASE)).chain(routes) {
        let floor = crate::actor::movement::floor_z(ctx, MAP_ID, 0, point.0, point.1, point.2)
            .ok_or_else(|| format!("The courtyard {name} needs an imported collision floor"))?;
        if (floor - point.2).abs() > 1.0 {
            return Err(format!(
                "The courtyard {name} floor differs from its authored height"
            ));
        }
    }
    for (name, start) in routes {
        let route = crate::actor::movement::route_path(ctx, MAP_ID, 0, start, BASE, 2.0);
        if route.step.status != crate::actor::movement::RouteStatus::Complete {
            return Err(format!(
                "The courtyard {name} needs a checked route to the base"
            ));
        }
        if !crate::actor::movement::has_los(
            ctx,
            MAP_ID,
            0,
            (start.0, start.1, start.2 + 1.5),
            (BASE.0, BASE.1, BASE.2 + 1.5),
        ) {
            return Err(format!("The courtyard {name} is blocked from the base"));
        }
    }
    Ok(())
}

fn require_stock_identifiers(ctx: &ReducerContext) -> Result<(), String> {
    for index in 0..STOCK_COUNT {
        if ctx
            .db
            .game_npc_vendor()
            .id()
            .find(VENDOR_STOCK_ID_BASE + u64::from(index))
            .is_some_and(|row| row.creature_entry != VENDOR_ENTRY)
        {
            return Err("Quartermaster stock identifiers belong to another vendor".into());
        }
    }
    Ok(())
}

pub(crate) fn stock_quartermaster(ctx: &ReducerContext) -> Result<(), String> {
    require_stock_identifiers(ctx)?;
    let stock = ctx.db.game_npc_vendor();
    let existing = stock.by_vendor().filter(VENDOR_ENTRY).collect::<Vec<_>>();
    if existing.len() == STOCK_COUNT as usize
        && (0..STOCK_COUNT).all(|index| {
            existing.iter().any(|row| {
                row.id == VENDOR_STOCK_ID_BASE + u64::from(index)
                    && row.slot == index + 1
                    && row.item_entry == FIRST_ITEM + index
                    && row.max_count == 0
            })
        })
    {
        return Ok(());
    }
    for row in existing {
        stock.id().delete(row.id);
    }
    for index in 0..STOCK_COUNT {
        stock.insert(NpcVendor {
            id: VENDOR_STOCK_ID_BASE + u64::from(index),
            creature_entry: VENDOR_ENTRY,
            item_entry: FIRST_ITEM + index,
            slot: index + 1,
            max_count: 0,
        });
    }
    Ok(())
}

/// Enable only after applying authored Deltas; reconfigure after an item-family import.
#[reducer]
pub fn wave_defense_configure(ctx: &ReducerContext, enabled: bool) -> Result<(), String> {
    crate::package::require_operator(ctx)?;
    if ctx.db.pkg_wave_defense_match().iter().next().is_some() {
        return Err(
            "Return every Party to the lobby before changing wave defense configuration".into(),
        );
    }
    let configs = ctx.db.pkg_wave_defense_config();
    if !enabled {
        if let Some(mut config) = configs.id().find(0) {
            config.enabled = false;
            configs.id().update(config);
        }
        ctx.db.pkg_wave_defense_schedule().id().delete(0);
        return Ok(());
    }
    crate::package::party::require_authority(ctx)?;
    require_courtyard(ctx)?;
    require_content(ctx)?;
    require_stock_identifiers(ctx)?;
    let lobby_instance_id = crate::package::instance::create_lobby(ctx, PACKAGE, MAP_ID)?;
    stock_quartermaster(ctx)?;
    let config = DefenseConfig {
        id: 0,
        enabled: true,
        lobby_instance_id,
        map_id: MAP_ID,
        x: BASE.0,
        y: BASE.1,
        z: BASE.2,
        orientation: std::f32::consts::PI,
        spawn_x: SPAWN.0,
        spawn_y: SPAWN.1,
        spawn_z: SPAWN.2,
        creature_entry: CREATURE_ENTRY,
        boss_entry: BOSS_ENTRY,
        vendor_entry: VENDOR_ENTRY,
        pet_entry: PET_ENTRY,
    };
    if configs.id().find(0).is_some() {
        configs.id().update(config);
    } else {
        configs.insert(config);
    }
    if ctx.db.pkg_wave_defense_schedule().id().find(0).is_none() {
        ctx.db.pkg_wave_defense_schedule().insert(DefenseSchedule {
            id: 0,
            scheduled_at: TimeDuration::from_micros(1_000_000).into(),
        });
    }
    Ok(())
}
