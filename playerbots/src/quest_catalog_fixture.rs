#![cfg(feature = "debug_reducers")]

//! Synthetic private-Shard staging for the catalog and its callable executors.

use super::pkg_playerbots_bot;
use super::quest_catalog::{
    self, pkg_playerbots_catalog_objective, pkg_playerbots_catalog_quest,
    pkg_playerbots_catalog_seed, pkg_playerbots_quest_catalog, pkg_playerbots_quest_objective,
    AdmissionRefusal, CatalogDestination, CatalogEntityKind, CatalogObjectiveKind, CatalogWorkArea,
    ObjectiveExecutor, PlayerbotsCatalogObjective, PlayerbotsCatalogQuest, CATALOG_REVISION,
    CATALOG_WALK_LIMIT,
};
use crate::import_meta::game_import_meta; // package-api: exempt operator fixture refuses imported content before staging
use crate::{
    game_character_quest, game_corpse_loot, game_creature_loot, game_creature_quest,
    game_creature_spawn, game_creature_spline, game_creature_template, game_faction_template,
    game_gameobject, game_gameobject_loot, game_gameobject_quest, game_gameobject_template,
    game_item_instance, game_item_template, game_quest_objective, game_quest_template, game_spell,
    game_spell_effect, game_world_entity,
};
use spacetimedb::{reducer, table, ReducerContext, Table};

const FIXTURE_REVISION: &str = "playerbots-synthetic-quest-catalog-v1";
const CHEST_ENTRY: u32 = 161557;
const CHEST_LOOT: u32 = 10119;
const DIRECT_GO_QUEST: u32 = 3904;
const FIXTURE_OWNERSHIP_ID: u8 = 1;
const RELATION_ID_BASE: u64 = 509_9000;
const OBJECTIVE_ID_BASE: u64 = 509_9100;
const CREATURE_LOOT_ID_BASE: u64 = 509_9200;
const GAMEOBJECT_LOOT_ID: u64 = 509_9210;
const INVENTORY_FILLER: u32 = 509_9400;
const MAGE_COLLECT_ITEM: u32 = 750;
const SHARED_FIXTURE_HEAL: u32 = 5_090_100;
const SEEDED_USE_QUEST: u32 = 50_970;
const SEEDED_USE_GAMEOBJECT: u32 = 5_090_970;
const SEEDED_USE_RELATION_START: u64 = 5_099_300;
const SEEDED_USE_RELATION_END: u64 = 5_099_301;
const SEEDED_USE_OBJECTIVE: u64 = 5_099_302;
const SEEDED_USE_CONTENT: &str = "playerbots-loopback-simple-gameobject-v1";
const NAMED_LOOP_CONTENT: &str = "playerbots-loopback-named-q7-ten-targets-v2";
const SEARCH_LIMIT_ENTRY: u32 = 5_090_971;
const SEARCH_LIMIT_ROWS: u64 = 96;
const LOOPBACK_SMITE: u32 = 585;
const LOOPBACK_PRIEST_MANA: u32 = 400;
const FRIENDLY_FIXTURE_FACTION: u32 = 5_090_972;
const HELD_CATALOG_PREFIX_BASE: u32 = 5_098_000;
const ACTIVE_QUEST_OVERFLOW_BASE: u32 = 5_098_100;
const DISPERSION_POPULATION: usize = 25;

const CREATURES: [u32; 12] = [823, 197, 196, 9296, 952, 241, 240, 261, 6, 299, 69, 38];
const GAMEOBJECTS: [u32; 3] = [55, 56, CHEST_ENTRY];
const ITEMS: [u32; 4] = [MAGE_COLLECT_ITEM, 752, 11119, 11125];

const QUESTS: &[(u32, u32, u32, u32, u32)] = &[
    (783, 1, 0, 823, 197),
    (7, 1, 783, 197, 197),
    (5261, 1, 783, 823, 196),
    (33, 1, 5261, 196, 196),
    (18, 2, 783, 823, 823),
    (3903, 2, 33, 823, 9296),
    (3904, 2, 3903, 9296, 9296),
    (3905, 2, 3904, 9296, 952),
    (40, 1, 0, 241, 240),
    (35, 1, 40, 240, 261),
    (37, 1, 35, 261, 55),
    (45, 1, 37, 55, 56),
];

#[table(accessor = pkg_playerbots_quest_fixture_ownership, public)]
pub struct PlayerbotsQuestFixtureOwnership {
    #[primary_key]
    pub id: u8,
    pub revision: String,
}

#[table(accessor = pkg_playerbots_seeded_quest_fixture, public)]
pub struct PlayerbotsSeededQuestFixture {
    #[primary_key]
    pub quest_entry: u32,
    pub gameobject_entry: u32,
    pub content_revision: String,
}

#[table(accessor = pkg_playerbots_quest_loop_fixture, public)]
pub struct PlayerbotsQuestLoopFixture {
    #[primary_key]
    pub character_guid: u64,
    pub quest_entry: u32,
    pub target_entry: u32,
    pub target_count: u32,
    pub content_revision: String,
}

#[table(
    accessor = pkg_playerbots_quest_turnin_fixture,
    public,
    index(accessor = by_character, btree(columns = [character_guid]))
)]
pub struct PlayerbotsQuestTurninFixture {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub character_guid: u64,
    pub quest_entry: u32,
    pub turnin_count: u16,
    pub observed_micros: i64,
}

#[table(accessor = pkg_playerbots_quest_loot_receipt_fixture, public)]
pub struct PlayerbotsQuestLootReceiptFixture {
    #[primary_key]
    pub character_guid: u64,
    pub item_entry: u32,
    pub received_count: u32,
    pub peak_carried_count: u32,
    pub last_source_guid: u64,
    pub observed_micros: i64,
}

crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_quest_loop_fixture(ctx, character_guid) {
    ctx.db
        .pkg_playerbots_quest_loop_fixture()
        .character_guid()
        .delete(character_guid);
});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_playerbots_quest_loop_fixture());

crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_quest_turnin_fixture(ctx, character_guid) {
    let rows = ctx.db.pkg_playerbots_quest_turnin_fixture();
    for row in rows.by_character().filter(character_guid).collect::<Vec<_>>() {
        rows.id().delete(row.id);
    }
});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_playerbots_quest_turnin_fixture());

crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_quest_loot_receipt_fixture(ctx, character_guid) {
    ctx.db
        .pkg_playerbots_quest_loot_receipt_fixture()
        .character_guid()
        .delete(character_guid);
});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_playerbots_quest_loot_receipt_fixture());

crate::game_hook!(on_loot, fn playerbots_quest_fixture_looted(ctx, payload) {
    if payload.item_entry != MAGE_COLLECT_ITEM
        || ctx
            .db
            .pkg_playerbots_quest_loop_fixture()
            .character_guid()
            .find(payload.looter_guid)
            .is_none()
    {
        return;
    }
    let rows = ctx.db.pkg_playerbots_quest_loot_receipt_fixture();
    let carried_count = crate::items::item_count(ctx, payload.looter_guid, payload.item_entry);
    if let Some(mut row) = rows.character_guid().find(payload.looter_guid) {
        row.received_count = row.received_count.saturating_add(payload.count);
        row.peak_carried_count = row.peak_carried_count.max(carried_count);
        row.last_source_guid = payload.corpse_guid;
        row.observed_micros = ctx.timestamp.to_micros_since_unix_epoch();
        rows.character_guid().update(row);
    } else {
        rows.insert(PlayerbotsQuestLootReceiptFixture {
            character_guid: payload.looter_guid,
            item_entry: payload.item_entry,
            received_count: payload.count,
            peak_carried_count: carried_count,
            last_source_guid: payload.corpse_guid,
            observed_micros: ctx.timestamp.to_micros_since_unix_epoch(),
        });
    }
});

crate::game_hook!(on_quest_turnin, fn playerbots_quest_fixture_turned_in(ctx, payload) {
    if ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(payload.character_guid)
        .next()
        .is_none()
    {
        return;
    }
    let rows = ctx.db.pkg_playerbots_quest_turnin_fixture();
    if let Some(mut row) = rows
        .by_character()
        .filter(payload.character_guid)
        .find(|row| row.quest_entry == payload.quest_entry)
    {
        row.turnin_count = row.turnin_count.saturating_add(1);
        row.observed_micros = ctx.timestamp.to_micros_since_unix_epoch();
        rows.id().update(row);
    } else if rows
        .by_character()
        .filter(payload.character_guid)
        .take(20)
        .count()
        < 20
    {
        rows.insert(PlayerbotsQuestTurninFixture {
            id: 0,
            character_guid: payload.character_guid,
            quest_entry: payload.quest_entry,
            turnin_count: 1,
            observed_micros: ctx.timestamp.to_micros_since_unix_epoch(),
        });
    }
});

#[derive(Clone, Copy, PartialEq, Eq)]
enum FixtureStage {
    New,
    Existing,
}

fn reject_imported_content(ctx: &ReducerContext) -> Result<(), String> {
    if let Some(import) = ctx.db.game_import_meta().iter().next() {
        return Err(format!(
            "quest fixture refuses non-empty import catalogue ({})",
            import.family
        ));
    }
    Ok(())
}

fn require_fixture(ctx: &ReducerContext) -> Result<(), String> {
    reject_imported_content(ctx)?;
    let ownership = ctx
        .db
        .pkg_playerbots_quest_fixture_ownership()
        .id()
        .find(FIXTURE_OWNERSHIP_ID)
        .ok_or("quest fixture has not claimed this database")?;
    if ownership.revision != FIXTURE_REVISION {
        return Err("quest fixture ownership revision differs".to_string());
    }
    Ok(())
}

/// Place the Character beside a real quest target inside blocked seeded navigation. Attack
/// admission remains available, but the owning melee line-of-sight Gate prevents damage.
#[reducer]
pub fn playerbots_recovery_fixture_block_quest_target(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let target = ctx
        .db
        .game_world_entity()
        .guid()
        .find(creature_guid(6))
        .ok_or("quest target missing")?;
    super::fixture::playerbots_fixture_position(ctx, character_guid, target.x - 3.0)?;
    let me = crate::helpers::live_entity(ctx, character_guid)?;
    block_navigation(ctx, &me)?;
    if crate::nav::has_los(
        ctx,
        me.map_id,
        me.instance_id,
        (me.x, me.y, me.z),
        (target.x, target.y, target.z),
    ) {
        return Err("fixture ray unexpectedly clear".to_string());
    }
    super::fixture::playerbots_fixture_runner_select_cohort(ctx, character_guid)
}

fn block_navigation(ctx: &ReducerContext, me: &crate::WorldEntity) -> Result<(), String> {
    let cx = lyracore_shared::terrain::cell_index(me.x).ok_or("fixture off grid")?;
    let cy = lyracore_shared::terrain::cell_index(me.y).ok_or("fixture off grid")?;
    use crate::nav::game_nav_chunk;
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
    Ok(())
}

/// The near wall forces a first leg west, while the distant wall exhausts the bounded search east.
#[reducer]
pub fn playerbots_recovery_fixture_partial_route(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    super::fixture::playerbots_fixture_partial_route(ctx, character_guid)?;
    let me = crate::helpers::live_entity(ctx, character_guid)?;
    use crate::nav::game_nav_chunk;
    let mut chunks = std::collections::BTreeMap::new();
    for x in -16..=8 {
        for y in -16..=16 {
            if x != 8 && y != -16 && y != 16 {
                continue;
            }
            let (px, py) = (me.x + x as f32 * 0.25, me.y + y as f32 * 0.25);
            let cx = lyracore_shared::terrain::cell_index(px).ok_or("fixture off grid")?;
            let cy = lyracore_shared::terrain::cell_index(py).ok_or("fixture off grid")?;
            let key = lyracore_shared::terrain::cell_key(me.map_id, cx, cy);
            let chunk = chunks.entry(key).or_insert_with(|| {
                ctx.db
                    .game_nav_chunk()
                    .key()
                    .find(key)
                    .unwrap_or(crate::nav::NavChunk {
                        key,
                        map_id: me.map_id,
                        cell_x: cx,
                        cell_y: cy,
                        base_z: me.z,
                        walk: vec![255; lyracore_shared::nav::WALK_BYTES],
                        obs: vec![lyracore_shared::nav::OBS_NONE; lyracore_shared::nav::OBS_BYTES],
                    })
            });
            let nx = lyracore_shared::nav::sub_index(px, cx, lyracore_shared::nav::WALK_DIM)
                .ok_or("fixture off grid")?;
            let ny = lyracore_shared::nav::sub_index(py, cy, lyracore_shared::nav::WALK_DIM)
                .ok_or("fixture off grid")?;
            lyracore_shared::nav::walk_set(&mut chunk.walk, nx, ny, false);
        }
    }
    for (key, chunk) in chunks {
        ctx.db.game_nav_chunk().key().delete(key);
        ctx.db.game_nav_chunk().insert(chunk);
    }
    super::fixture::playerbots_fixture_runner_select_cohort(ctx, character_guid)
}

#[reducer]
pub fn playerbots_recovery_fixture_block_companion(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let me = crate::helpers::live_entity(ctx, character_guid)?;
    block_navigation(ctx, &me)?;
    super::fixture::playerbots_fixture_runner_select_cohort(ctx, character_guid)
}

#[reducer]
pub fn playerbots_recovery_fixture_exhaust_attempt(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    use super::runner::pkg_playerbots_runner;
    let rows = ctx.db.pkg_playerbots_runner();
    let mut runner = rows
        .character_guid()
        .find(character_guid)
        .ok_or("runner missing")?;
    let recovery = runner.recovery.as_mut().ok_or("recovery missing")?;
    let active = recovery.active.ok_or("no active recovery attempt")?;
    let attempt = recovery
        .attempts
        .iter_mut()
        .find(|attempt| attempt.work == active)
        .ok_or("active recovery attempt missing")?;
    attempt.stalled_micros = 30_000_000;
    rows.character_guid().update(runner);
    Ok(())
}

#[reducer]
pub fn playerbots_recovery_fixture_keep_two_quest_targets(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let fixture = ctx
        .db
        .pkg_playerbots_quest_loop_fixture()
        .character_guid()
        .find(character_guid)
        .ok_or("named quest-loop fixture is absent")?;
    if fixture.quest_entry != 7
        || fixture.target_entry != 6
        || fixture.target_count != 10
        || fixture.content_revision != NAMED_LOOP_CONTENT
    {
        return Err("named quest-loop fixture identity differs".to_string());
    }
    for offset in 2..10u64 {
        let guid = creature_guid(6).saturating_add(offset);
        ctx.db.game_world_entity().guid().delete(guid);
        ctx.db.game_creature_spawn().guid().delete(guid);
    }
    let entities = ctx.db.game_world_entity();
    for offset in 0..2u64 {
        let guid = creature_guid(6).saturating_add(offset);
        let mut target = entities
            .guid()
            .find(guid)
            .ok_or("Quest 7 target is absent")?;
        target.x += 4.0;
        target.y += 4.0;
        entities.guid().update(target);
    }
    Ok(())
}

#[reducer]
pub fn playerbots_recovery_fixture_expire_quest_target(
    ctx: &ReducerContext,
    character_guid: u64,
    target_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    use super::runner::pkg_playerbots_runner;
    let rows = ctx.db.pkg_playerbots_runner();
    let mut runner = rows
        .character_guid()
        .find(character_guid)
        .ok_or("runner missing")?;
    let recovery = runner.recovery.as_mut().ok_or("recovery missing")?;
    let attempt = recovery
        .attempts
        .iter_mut()
        .find(|attempt| {
            attempt.work == super::recovery::Work::Fight(target_guid)
                && attempt.reason == super::decision::Reason::Quest
        })
        .ok_or("Quest target recovery attempt missing")?;
    let previous_until = attempt
        .deferred_until_micros
        .ok_or("Quest target recovery attempt is not deferred")?;
    let expired = ctx.timestamp.to_micros_since_unix_epoch().saturating_sub(1);
    let destination = attempt.destination.clone();
    attempt.deferred_until_micros = Some(expired);
    let deferred = runner
        .deferred_destinations
        .iter_mut()
        .find(|deferred| {
            deferred.destination == destination && deferred.until_micros == previous_until
        })
        .ok_or("paired Quest target deferral missing")?;
    deferred.until_micros = expired;
    rows.character_guid().update(runner);
    Ok(())
}

fn quest_offset(quest_entry: u32) -> u64 {
    QUESTS
        .iter()
        .position(|quest| quest.0 == quest_entry)
        .expect("fixture quest must be listed") as u64
}

fn relation_id(quest_entry: u32, role: u8) -> u64 {
    RELATION_ID_BASE + quest_offset(quest_entry) * 2 + u64::from(role)
}

fn objective_id(quest_entry: u32, index: u8) -> u64 {
    OBJECTIVE_ID_BASE + quest_offset(quest_entry) * 4 + u64::from(index)
}

fn creature_loot_id(source_entry: u32) -> u64 {
    CREATURE_LOOT_ID_BASE
        + match source_entry {
            299 => 0,
            69 => 1,
            38 => 2,
            _ => unreachable!("fixture creature loot source must be listed"),
        }
}

fn creature_guid(entry: u32) -> u64 {
    (0xF130u64 << 48) | (u64::from(entry) << 24) | 1
}

fn gameobject_guid(entry: u32) -> u64 {
    (0xF110u64 << 48) | u64::from(entry)
}

fn alternative_gameobject_guid(entry: u32) -> u64 {
    gameobject_guid(entry).saturating_add(1)
}

fn insert_alternative_gameobject(ctx: &ReducerContext, entry: u32) -> Result<u64, String> {
    let original = ctx
        .db
        .game_gameobject()
        .guid()
        .find(gameobject_guid(entry))
        .ok_or("source GameObject is absent")?;
    let guid = alternative_gameobject_guid(entry);
    let x = original.x + 12.0;
    let y = original.y;
    let (grid_x, grid_y) = lyracore_shared::spatial::grid_cell(x, y);
    let rows = ctx.db.game_gameobject();
    rows.guid().delete(guid);
    rows.insert(crate::GameObject {
        guid,
        template_entry: original.template_entry,
        map_id: original.map_id,
        x,
        y,
        z: original.z,
        orientation: original.orientation,
        state: 0,
        created_at: ctx.timestamp,
        respawn_at_micros: 0,
        instance_id: original.instance_id,
        grid_x,
        grid_y,
        cell: lyracore_shared::spatial::grid_cell_id(grid_x, grid_y),
        rotation_0: original.rotation_0,
        rotation_1: original.rotation_1,
        rotation_2: original.rotation_2,
        rotation_3: original.rotation_3,
    });
    Ok(guid)
}

fn stage_loopback_smite(ctx: &ReducerContext) -> Result<(), String> {
    let existing = ctx.db.game_spell().spell_id().find(LOOPBACK_SMITE);
    let existing_effect = ctx
        .db
        .game_spell_effect()
        .by_spell()
        .filter(LOOPBACK_SMITE)
        .next();
    if existing.as_ref().is_some_and(|spell| {
        spell.name == "Smite"
            && spell.power_type == 0
            && spell.cost == 20
            && spell.cast_time_ms == 1_500
            && spell.gcd_ms == 1_500
            && spell.cooldown_ms == 0
            && spell.range_yd == 30
            && spell.duration_ms == 0
            && spell.school_mask == 2
            && spell.spell_level == 1
            && spell.max_level == 6
            && spell.is_negative
            && spell.family_name == 6
            && spell.family_flags == 128
            && spell.proc_chance == 101
    }) && existing_effect.as_ref().is_some_and(|effect| {
        effect.id == 2_340
            && effect.effect_index == 0
            && effect.kind == 1
            && effect.base_points == 13
            && effect.die_sides == 5
            && effect.per_level == 0.5
            && effect.target == 1
    }) {
        return Ok(());
    }
    if existing.is_some() || existing_effect.is_some() {
        return Err("loopback Smite fixture spell is occupied".to_string());
    }
    // The pinned ClassicDB spell_template row supplies the gameplay fields below. The dump omits
    // auxiliary DBC rows for CastTimeIndex 16, RangeIndex 4, and DurationIndex 0, so this private
    // fixture declares 1500ms, 30yd, and 0ms for those three values.
    ctx.db.game_spell().insert(crate::spell::Spell {
        spell_id: LOOPBACK_SMITE,
        name: "Smite".to_string(),
        power_type: 0,
        cost: 20,
        cast_time_ms: 1_500,
        gcd_ms: 1_500,
        cooldown_ms: 0,
        range_yd: 30,
        duration_ms: 0,
        school_mask: 2,
        dispel_type: 0,
        mechanic: 0,
        max_stacks: 0,
        aura_interrupt: 0,
        attributes: 65_536,
        spell_level: 1,
        max_level: 6,
        is_negative: true,
        cast_flags: 0,
        stances: 0,
        family_name: 6,
        family_flags: 128,
        proc_flags: 0,
        proc_chance: 101,
        proc_charges: 0,
    });
    ctx.db
        .game_spell_effect()
        .insert(crate::spell::SpellEffect {
            id: 2_340,
            spell_id: LOOPBACK_SMITE,
            effect_index: 0,
            kind: 1,
            base_points: 13,
            die_sides: 5,
            per_level: 0.5,
            period_ms: 0,
            target: 1,
            radius_yd: 0.0,
            chain_targets: 0,
            trigger_spell: 0,
            effect_mechanic: 0,
            p0: 0,
            p0_kind: 0,
            p1: 0,
            script_id: 0,
            enters_combat: false,
        });
    Ok(())
}

#[reducer]
pub fn playerbots_quest_loop_fixture_stage_simple_gameobject(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    reject_imported_content(ctx)?;
    if let Some(existing) = ctx
        .db
        .pkg_playerbots_seeded_quest_fixture()
        .quest_entry()
        .find(SEEDED_USE_QUEST)
    {
        return if existing.gameobject_entry == SEEDED_USE_GAMEOBJECT
            && existing.content_revision == SEEDED_USE_CONTENT
        {
            Ok(())
        } else {
            Err("seeded quest-loop fixture identity differs".to_string())
        };
    }
    for occupied in [
        ctx.db
            .game_quest_template()
            .entry()
            .find(SEEDED_USE_QUEST)
            .is_some(),
        ctx.db
            .game_gameobject_template()
            .entry()
            .find(SEEDED_USE_GAMEOBJECT)
            .is_some(),
        ctx.db
            .game_gameobject()
            .guid()
            .find(gameobject_guid(SEEDED_USE_GAMEOBJECT))
            .is_some(),
        ctx.db
            .game_gameobject_quest()
            .id()
            .find(SEEDED_USE_RELATION_START)
            .is_some(),
        ctx.db
            .game_gameobject_quest()
            .id()
            .find(SEEDED_USE_RELATION_END)
            .is_some(),
        ctx.db
            .game_quest_objective()
            .id()
            .find(SEEDED_USE_OBJECTIVE)
            .is_some(),
        ctx.db
            .pkg_playerbots_catalog_quest()
            .quest_entry()
            .find(SEEDED_USE_QUEST)
            .is_some(),
        ctx.db
            .pkg_playerbots_catalog_objective()
            .id()
            .find(u64::from(SEEDED_USE_QUEST) << 8)
            .is_some(),
    ] {
        if occupied {
            return Err("seeded quest-loop fixture reserved content is occupied".to_string());
        }
    }
    super::fixture::playerbots_fixture_prepare(ctx)?;
    let mut character = ctx
        .db
        .game_world_entity()
        .guid()
        .find(character_guid)
        .ok_or("Character missing")?;
    character.health = character.max_health;
    ctx.db.game_world_entity().guid().update(character);
    let character = ctx
        .db
        .game_world_entity()
        .guid()
        .find(character_guid)
        .ok_or("Character missing after health update")?;
    let destination = CatalogDestination {
        kind: CatalogEntityKind::GameObject,
        entry: SEEDED_USE_GAMEOBJECT,
        guid: gameobject_guid(SEEDED_USE_GAMEOBJECT),
        map_id: character.map_id,
        instance_id: character.instance_id,
        x: character.x + 4.0,
        y: character.y,
        z: character.z,
    };
    insert_gameobject(
        ctx,
        SEEDED_USE_GAMEOBJECT,
        crate::gameobject::go_type::GOOBER,
        0,
        destination.map_id,
        destination.instance_id,
        destination.x,
        destination.y,
        destination.z,
    );
    ctx.db.game_quest_template().insert(crate::QuestTemplate {
        entry: SEEDED_USE_QUEST,
        min_level: 1,
        quest_level: 1,
        title: "Loopback simple GameObject".to_string(),
        reward_money: 3,
        reward_xp: 5,
        prev_quest_id: 0,
        required_races: 77,
        required_classes: 0,
        zone_or_sort: 9,
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
    for (id, role) in [
        (SEEDED_USE_RELATION_START, crate::quest::quest_role::START),
        (SEEDED_USE_RELATION_END, crate::quest::quest_role::END),
    ] {
        ctx.db
            .game_gameobject_quest()
            .insert(crate::GameObjectQuest {
                id,
                go_entry: SEEDED_USE_GAMEOBJECT,
                quest_entry: SEEDED_USE_QUEST,
                role,
            });
    }
    ctx.db.game_quest_objective().insert(crate::QuestObjective {
        id: SEEDED_USE_OBJECTIVE,
        quest_entry: SEEDED_USE_QUEST,
        obj_index: 0,
        kind: crate::quest::objective_kind::USE_GAMEOBJECT,
        target_entry: SEEDED_USE_GAMEOBJECT,
        required_count: 1,
    });
    quest_catalog::ensure_catalog(ctx);
    ctx.db
        .pkg_playerbots_catalog_quest()
        .insert(PlayerbotsCatalogQuest {
            quest_entry: SEEDED_USE_QUEST,
            catalog_revision: CATALOG_REVISION,
            catalog_order: 60_000,
            min_level: 1,
            required_races: 77,
            required_classes: 0,
            prerequisite_quest: 0,
            start_kind: CatalogEntityKind::GameObject,
            start_entry: SEEDED_USE_GAMEOBJECT,
            start_destinations: vec![destination.clone()],
            actual_ender_kind: CatalogEntityKind::GameObject,
            actual_ender_entry: SEEDED_USE_GAMEOBJECT,
            actual_ender_destinations: vec![destination.clone()],
            content_revision: SEEDED_USE_CONTENT.to_string(),
        });
    ctx.db
        .pkg_playerbots_catalog_objective()
        .insert(PlayerbotsCatalogObjective {
            id: (u64::from(SEEDED_USE_QUEST) << 8),
            quest_entry: SEEDED_USE_QUEST,
            objective_index: 0,
            kind: CatalogObjectiveKind::UseGameObject,
            target_entry: SEEDED_USE_GAMEOBJECT,
            required_count: 1,
            executor: ObjectiveExecutor::SimpleGameObject,
            source_kind: Some(CatalogEntityKind::GameObject),
            source_entries: vec![SEEDED_USE_GAMEOBJECT],
            source_destinations: vec![destination.clone()],
            work_area: Some(CatalogWorkArea {
                map_id: destination.map_id,
                instance_id: destination.instance_id,
                min_x: destination.x,
                max_x: destination.x,
                min_y: destination.y,
                max_y: destination.y,
            }),
            destination_evidence_revision: SEEDED_USE_CONTENT.to_string(),
            catalog_revision: CATALOG_REVISION,
        });
    ctx.db
        .pkg_playerbots_seeded_quest_fixture()
        .insert(PlayerbotsSeededQuestFixture {
            quest_entry: SEEDED_USE_QUEST,
            gameobject_entry: SEEDED_USE_GAMEOBJECT,
            content_revision: SEEDED_USE_CONTENT.to_string(),
        });
    Ok(())
}

fn relation_exists(
    ctx: &ReducerContext,
    kind: CatalogEntityKind,
    entry: u32,
    quest_entry: u32,
    role: u8,
) -> bool {
    match kind {
        CatalogEntityKind::Creature => ctx
            .db
            .game_creature_quest()
            .by_creature()
            .filter(entry)
            .any(|row| row.quest_entry == quest_entry && row.role == role),
        CatalogEntityKind::GameObject => ctx
            .db
            .game_gameobject_quest()
            .by_gameobject()
            .filter(entry)
            .any(|row| row.quest_entry == quest_entry && row.role == role),
    }
}

fn stage_gate(ctx: &ReducerContext, character_guid: u64) -> Result<FixtureStage, String> {
    reject_imported_content(ctx)?;
    if let Some(ownership) = ctx
        .db
        .pkg_playerbots_quest_fixture_ownership()
        .id()
        .find(FIXTURE_OWNERSHIP_ID)
    {
        return if ownership.revision == FIXTURE_REVISION {
            Ok(FixtureStage::Existing)
        } else {
            Err("quest fixture ownership revision differs".to_string())
        };
    }

    if ctx
        .db
        .game_world_entity()
        .guid()
        .find(character_guid)
        .is_none()
    {
        return Err("Character missing".to_string());
    }
    let source_item = ctx
        .db
        .game_item_instance()
        .by_owner_guid()
        .filter(character_guid)
        .next()
        .ok_or("starter item missing")?;
    if ctx
        .db
        .game_item_template()
        .entry()
        .find(source_item.entry)
        .is_none()
    {
        return Err("starter item template missing".to_string());
    }
    if ctx
        .db
        .game_creature_template()
        .entry()
        .find(51000)
        .is_none()
    {
        return Err("seed creature template missing".to_string());
    }
    if ctx.db.game_spell().spell_id().find(2050).is_none() {
        return Err("seed heal missing".to_string());
    }
    for bot in ctx.db.pkg_playerbots_bot().iter() {
        if ctx
            .db
            .game_world_entity()
            .guid()
            .find(bot.character_guid)
            .is_none()
        {
            return Err("fixture bot is not in the world".to_string());
        }
    }
    let occupied = ITEMS
        .iter()
        .copied()
        .chain([INVENTORY_FILLER])
        .find(|entry| ctx.db.game_item_template().entry().find(*entry).is_some());
    if let Some(entry) = occupied {
        return Err(format!("quest fixture item entry {entry} already exists"));
    }
    for entry in CREATURES {
        if ctx
            .db
            .game_creature_template()
            .entry()
            .find(entry)
            .is_some()
            || ctx
                .db
                .game_creature_spawn()
                .guid()
                .find(creature_guid(entry))
                .is_some()
            || ctx
                .db
                .game_world_entity()
                .guid()
                .find(creature_guid(entry))
                .is_some()
        {
            return Err(format!(
                "quest fixture creature entry {entry} already exists"
            ));
        }
    }
    for entry in GAMEOBJECTS {
        if ctx
            .db
            .game_gameobject_template()
            .entry()
            .find(entry)
            .is_some()
            || ctx
                .db
                .game_gameobject()
                .guid()
                .find(gameobject_guid(entry))
                .is_some()
        {
            return Err(format!(
                "quest fixture GameObject entry {entry} already exists"
            ));
        }
    }
    for (quest_entry, _, _, start, actual_ender) in QUESTS.iter().copied() {
        if ctx
            .db
            .game_quest_template()
            .entry()
            .find(quest_entry)
            .is_some()
            || ctx
                .db
                .game_quest_objective()
                .by_quest()
                .filter(quest_entry)
                .next()
                .is_some()
        {
            return Err(format!("quest fixture quest {quest_entry} already exists"));
        }
        for (entry, role) in [
            (start, crate::quest::quest_role::START),
            (actual_ender, crate::quest::quest_role::END),
        ] {
            let kind = if matches!(entry, 55 | 56) {
                CatalogEntityKind::GameObject
            } else {
                CatalogEntityKind::Creature
            };
            if relation_exists(ctx, kind, entry, quest_entry, role) {
                return Err(format!(
                    "quest fixture relation for quest {quest_entry} already exists"
                ));
            }
            let id = relation_id(quest_entry, role);
            let reserved_occupied = match kind {
                CatalogEntityKind::Creature => ctx.db.game_creature_quest().id().find(id).is_some(),
                CatalogEntityKind::GameObject => {
                    ctx.db.game_gameobject_quest().id().find(id).is_some()
                }
            };
            if reserved_occupied {
                return Err(format!("quest fixture relation id {id} is occupied"));
            }
        }
        for index in 0..4 {
            let id = objective_id(quest_entry, index);
            if ctx.db.game_quest_objective().id().find(id).is_some() {
                return Err(format!("quest fixture objective id {id} is occupied"));
            }
        }
    }
    for source in [299, 69, 38] {
        if ctx
            .db
            .game_creature_loot()
            .by_creature()
            .filter(source)
            .next()
            .is_some()
        {
            return Err(format!(
                "quest fixture creature loot for {source} already exists"
            ));
        }
        let id = creature_loot_id(source);
        if ctx.db.game_creature_loot().id().find(id).is_some() {
            return Err(format!("quest fixture creature loot id {id} is occupied"));
        }
    }
    if ctx
        .db
        .game_gameobject_loot()
        .by_loot()
        .filter(CHEST_LOOT)
        .next()
        .is_some()
        || ctx
            .db
            .game_gameobject_loot()
            .id()
            .find(GAMEOBJECT_LOOT_ID)
            .is_some()
    {
        return Err("quest fixture GameObject loot is occupied".to_string());
    }
    if ctx
        .db
        .game_spell()
        .spell_id()
        .find(SHARED_FIXTURE_HEAL)
        .is_some()
        || ctx
            .db
            .game_spell_effect()
            .by_spell()
            .filter(SHARED_FIXTURE_HEAL)
            .next()
            .is_some()
    {
        return Err("shared playerbots fixture spell is occupied".to_string());
    }
    for effect in ctx.db.game_spell_effect().by_spell().filter(2050u32) {
        let id = (u64::from(SHARED_FIXTURE_HEAL) << 2) | u64::from(effect.effect_index);
        if ctx.db.game_spell_effect().id().find(id).is_some() {
            return Err(format!(
                "shared playerbots fixture spell effect id {id} is occupied"
            ));
        }
    }
    Ok(FixtureStage::New)
}

fn insert_creature(ctx: &ReducerContext, entry: u32, x: f32, y: f32, z: f32) -> Result<(), String> {
    let mut template = ctx
        .db
        .game_creature_template()
        .entry()
        .find(51000)
        .ok_or("seed creature template missing")?;
    template.entry = entry;
    template.name = format!("Catalog creature {entry}");
    template.faction_template = 14;
    let template = ctx.db.game_creature_template().insert(template);
    let guid = creature_guid(entry);
    let spawn = ctx.db.game_creature_spawn().insert(crate::CreatureSpawn {
        guid,
        entry,
        map_id: 0,
        x,
        y,
        z,
        orientation: 0.0,
        respawn_at: crate::creatures::timer_never(ctx),
        despawn_at: crate::creatures::timer_never(ctx),
        movement_type: 0,
        respawn_secs: 60,
        life_seq: 1,
    });
    crate::creatures::insert_creature_entity(
        ctx,
        crate::creatures::build_creature_entity(&spawn, &template, 0, 0),
    );
    Ok(())
}

fn insert_gameobject(
    ctx: &ReducerContext,
    entry: u32,
    type_id: u8,
    data1: u32,
    map_id: u32,
    instance_id: u64,
    x: f32,
    y: f32,
    z: f32,
) {
    ctx.db
        .game_gameobject_template()
        .insert(crate::GameObjectTemplate {
            entry,
            type_id,
            display_id: 259,
            name: format!("Catalog GameObject {entry}"),
            data0: 0,
            data1,
            gather_skill_line: 0,
            respawn_secs: 0,
            gather_gray: 0,
            lock_id: 0,
            size: 1.0,
        });
    let guid = gameobject_guid(entry);
    ctx.db.game_gameobject().insert(crate::GameObject {
        guid,
        template_entry: entry,
        map_id,
        x,
        y,
        z,
        orientation: 0.0,
        state: 0,
        created_at: ctx.timestamp,
        respawn_at_micros: 0,
        instance_id,
        grid_x: lyracore_shared::spatial::grid_cell(x, y).0,
        grid_y: lyracore_shared::spatial::grid_cell(x, y).1,
        cell: lyracore_shared::spatial::cell_id_at(x, y),
        rotation_0: 0.0,
        rotation_1: 0.0,
        rotation_2: 0.0,
        rotation_3: 0.0,
    });
}

fn clone_item(ctx: &ReducerContext, entry: u32, source_entry: u32) -> Result<(), String> {
    let mut item = ctx
        .db
        .game_item_template()
        .entry()
        .find(source_entry)
        .ok_or("starter item template missing")?;
    item.entry = entry;
    item.name = format!("Catalog item {entry}");
    item.max_stack = 20;
    item.max_count = 0;
    ctx.db.game_item_template().insert(item);
    Ok(())
}

fn insert_quest(
    ctx: &ReducerContext,
    entry: u32,
    min_level: u32,
    prerequisite: u32,
    start: u32,
    actual_ender: u32,
) {
    ctx.db.game_quest_template().insert(crate::QuestTemplate {
        entry,
        min_level,
        quest_level: min_level.max(1),
        title: format!("Catalog quest {entry}"),
        reward_money: 1,
        reward_xp: 1,
        prev_quest_id: prerequisite,
        required_races: 77,
        required_classes: 0,
        zone_or_sort: if entry >= 40 && entry < 100 { 12 } else { 9 },
        rew_rep_faction_1: 0,
        rew_rep_value_1: 0,
        rew_rep_faction_2: 0,
        rew_rep_value_2: 0,
        src_item: if entry == 3905 { 11125 } else { 0 },
        src_item_count: if entry == 3905 { 1 } else { 0 },
        repeatable: false,
        next_quest_id: if entry == 783 { 7 } else { 0 },
        limit_time: 0,
        reward_money_max_level: 0,
    });
    let start_kind = if matches!(start, 55 | 56) {
        CatalogEntityKind::GameObject
    } else {
        CatalogEntityKind::Creature
    };
    let end_kind = if matches!(actual_ender, 55 | 56) {
        CatalogEntityKind::GameObject
    } else {
        CatalogEntityKind::Creature
    };
    insert_relation(
        ctx,
        start_kind,
        start,
        entry,
        crate::quest::quest_role::START,
    );
    insert_relation(
        ctx,
        end_kind,
        actual_ender,
        entry,
        crate::quest::quest_role::END,
    );
}

fn insert_relation(
    ctx: &ReducerContext,
    kind: CatalogEntityKind,
    entry: u32,
    quest_entry: u32,
    role: u8,
) {
    match kind {
        CatalogEntityKind::Creature => {
            let table = ctx.db.game_creature_quest();
            let id = relation_id(quest_entry, role);
            table.id().delete(id);
            table.insert(crate::CreatureQuest {
                id,
                creature_entry: entry,
                quest_entry,
                role,
            });
        }
        CatalogEntityKind::GameObject => {
            let table = ctx.db.game_gameobject_quest();
            let id = relation_id(quest_entry, role);
            table.id().delete(id);
            table.insert(crate::GameObjectQuest {
                id,
                go_entry: entry,
                quest_entry,
                role,
            });
        }
    }
}

fn insert_objective(
    ctx: &ReducerContext,
    quest_entry: u32,
    index: u8,
    kind: u8,
    target_entry: u32,
    required_count: u32,
) {
    let id = objective_id(quest_entry, index);
    ctx.db.game_quest_objective().id().delete(id);
    ctx.db.game_quest_objective().insert(crate::QuestObjective {
        id,
        quest_entry,
        obj_index: index,
        kind,
        target_entry,
        required_count,
    });
}

fn reward_prerequisite(ctx: &ReducerContext, guid: u64, quest_entry: u32) -> Result<(), String> {
    if quest_entry == 0
        || ctx
            .db
            .game_character_quest()
            .by_character()
            .filter(guid)
            .any(|row| row.quest_entry == quest_entry && row.rewarded)
    {
        return Ok(());
    }
    let owner_identity = ctx
        .db
        .game_world_entity()
        .guid()
        .find(guid)
        .ok_or("Character missing")?
        .owner_identity;
    ctx.db.game_character_quest().insert(crate::CharacterQuest {
        id: 0, // Core allocates per-Character quest-log ids.
        character_guid: guid,
        owner_identity,
        quest_entry,
        counts: Vec::new(),
        rewarded: true,
        deadline_micros: 0,
        failed: false,
    });
    Ok(())
}

#[reducer]
pub fn playerbots_quest_fixture_stage(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    if stage_gate(ctx, character_guid)? == FixtureStage::Existing {
        return Ok(());
    }
    ctx.db
        .pkg_playerbots_quest_fixture_ownership()
        .insert(PlayerbotsQuestFixtureOwnership {
            id: FIXTURE_OWNERSHIP_ID,
            revision: FIXTURE_REVISION.to_string(),
        });
    super::fixture::playerbots_fixture_prepare(ctx)?;
    let mut character = ctx
        .db
        .game_world_entity()
        .guid()
        .find(character_guid)
        .ok_or("Character missing")?;
    character.level = character.level.max(2);
    ctx.db.game_world_entity().guid().update(character);
    let character = ctx
        .db
        .game_world_entity()
        .guid()
        .find(character_guid)
        .ok_or("Character missing after level update")?;
    let source_item = ctx
        .db
        .game_item_instance()
        .by_owner_guid()
        .filter(character_guid)
        .next()
        .ok_or("starter item missing")?
        .entry;
    for entry in ITEMS {
        clone_item(ctx, entry, source_item)?;
    }
    for (offset, entry) in CREATURES.into_iter().enumerate() {
        insert_creature(
            ctx,
            entry,
            character.x + 2.0,
            character.y + offset as f32 * 0.05,
            character.z,
        )?;
    }
    insert_gameobject(
        ctx,
        55,
        crate::gameobject::go_type::QUESTGIVER,
        0,
        character.map_id,
        character.instance_id,
        character.x + 2.0,
        character.y + 0.7,
        character.z,
    );
    insert_gameobject(
        ctx,
        56,
        crate::gameobject::go_type::QUESTGIVER,
        0,
        character.map_id,
        character.instance_id,
        character.x + 2.0,
        character.y + 0.8,
        character.z,
    );
    insert_gameobject(
        ctx,
        CHEST_ENTRY,
        crate::gameobject::go_type::CHEST,
        CHEST_LOOT,
        character.map_id,
        character.instance_id,
        character.x + 2.0,
        character.y + 0.9,
        character.z,
    );
    for (quest, min_level, prerequisite, start, actual_ender) in QUESTS.iter().copied() {
        insert_quest(ctx, quest, min_level, prerequisite, start, actual_ender);
    }
    for (quest, kind, target, count) in [
        (7, crate::quest::objective_kind::KILL_CREATURE, 6, 10),
        (33, crate::quest::objective_kind::COLLECT_ITEM, 750, 8),
        (18, crate::quest::objective_kind::COLLECT_ITEM, 752, 12),
        (3904, crate::quest::objective_kind::COLLECT_ITEM, 11119, 8),
        (3905, crate::quest::objective_kind::COLLECT_ITEM, 11125, 1),
    ] {
        insert_objective(ctx, quest, 0, kind, target, count);
    }
    let creature_loot = ctx.db.game_creature_loot();
    for (source, item) in [(299, 750), (69, 750), (38, 752)] {
        let id = creature_loot_id(source);
        creature_loot.id().delete(id);
        creature_loot.insert(crate::CreatureLoot {
            id,
            creature_entry: source,
            item_entry: item,
            chance_bp: 10_000,
            count: if item == 750 { 8 } else { 12 },
            group_id: 0,
            quest_only: true,
        });
    }
    let gameobject_loot = ctx.db.game_gameobject_loot();
    gameobject_loot.id().delete(GAMEOBJECT_LOOT_ID);
    gameobject_loot.insert(crate::GameObjectLoot {
        id: GAMEOBJECT_LOOT_ID,
        loot_id: CHEST_LOOT,
        item_entry: 11119,
        chance_bp: 10_000,
        count: 8,
        group_id: 0,
        quest_only: true,
    });
    quest_catalog::refresh_catalog(ctx, "unknown");
    Ok(())
}

/// Add ten deterministic quest targets beyond the Legacy home leash. This only stages world
/// content. The controlled Character must still accept, fight, earn credit, return and turn in.
#[reducer]
pub fn playerbots_quest_loop_fixture_stage_named(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    stage_named_with_rotation(ctx, character_guid, 0).map(|_| ())
}

/// Give the declared population the same Quest 7 facts and position, then park it before one
/// common-time selection pass. The excluded Character owns the synthetic foreign Loot Tag.
#[reducer]
pub fn playerbots_quest_loop_fixture_prepare_dispersion(
    ctx: &ReducerContext,
    excluded_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let primary = ctx
        .db
        .game_world_entity()
        .guid()
        .find(creature_guid(6))
        .ok_or("primary quest target missing")?;
    let giver = ctx
        .db
        .game_world_entity()
        .guid()
        .find(creature_guid(197))
        .ok_or("Quest 7 giver missing")?;
    let mut subjects: Vec<_> = ctx
        .db
        .pkg_playerbots_bot()
        .iter()
        .filter(|bot| bot.character_guid != excluded_guid)
        .map(|bot| bot.character_guid)
        .collect();
    subjects.sort_unstable();
    if subjects.len() != DISPERSION_POPULATION {
        return Err("dispersion fixture requires exactly 25 subjects".to_string());
    }
    if crate::group::group_of(ctx, excluded_guid).is_some()
        || subjects
            .iter()
            .any(|guid| crate::group::group_of(ctx, *guid).is_some())
    {
        return Err("dispersion fixture Characters must be ungrouped".to_string());
    }
    super::runner::transition_controller(ctx, excluded_guid, super::runner::Controller::Frozen)?;
    ctx.db.game_creature_spline().guid().delete(excluded_guid);
    use super::provisioning::pkg_playerbots_provisioning;
    use super::runner::pkg_playerbots_runner;
    let selection_x = primary.x - 40.0;
    for guid in subjects {
        super::runner::transition_controller(ctx, guid, super::runner::Controller::Frozen)?;
        ctx.db.game_creature_spline().guid().delete(guid);
        ctx.db.pkg_playerbots_runner().character_guid().delete(guid);
        let mut provisioning = ctx
            .db
            .pkg_playerbots_provisioning()
            .character_guid()
            .find(guid)
            .ok_or("dispersion fixture provisioning state missing")?;
        provisioning.next_repair_micros = i64::MAX;
        ctx.db
            .pkg_playerbots_provisioning()
            .character_guid()
            .update(provisioning);
        let mut entity = crate::helpers::live_entity(ctx, guid)?;
        entity.x = giver.x;
        entity.y = giver.y;
        entity.z = giver.z;
        let (grid_x, grid_y) = lyracore_shared::spatial::grid_cell(entity.x, entity.y);
        entity.grid_x = grid_x;
        entity.grid_y = grid_y;
        entity.cell = lyracore_shared::spatial::grid_cell_id(grid_x, grid_y);
        ctx.db.game_world_entity().guid().update(entity);
        playerbots_quest_fixture_admit_accept(ctx, guid, 7)?;
        let mut entity = crate::helpers::live_entity(ctx, guid)?;
        entity.x = selection_x;
        entity.y = primary.y;
        entity.z = primary.z;
        let (grid_x, grid_y) = lyracore_shared::spatial::grid_cell(entity.x, entity.y);
        entity.grid_x = grid_x;
        entity.grid_y = grid_y;
        entity.cell = lyracore_shared::spatial::grid_cell_id(grid_x, grid_y);
        ctx.db.game_world_entity().guid().update(entity);
        let mut bot = ctx
            .db
            .pkg_playerbots_bot()
            .by_character()
            .filter(guid)
            .next()
            .ok_or("dispersion fixture bot missing")?;
        bot.home_map = primary.map_id;
        bot.home_x = selection_x;
        bot.home_y = primary.y;
        bot.home_z = primary.z;
        ctx.db.pkg_playerbots_bot().id().update(bot);
        super::fixture::playerbots_fixture_runner_select_cohort(ctx, guid)?;
    }
    let mut excluded = crate::helpers::live_entity(ctx, excluded_guid)?;
    excluded.x = primary.x;
    excluded.y = primary.y;
    excluded.z = primary.z;
    let (grid_x, grid_y) = lyracore_shared::spatial::grid_cell(excluded.x, excluded.y);
    excluded.grid_x = grid_x;
    excluded.grid_y = grid_y;
    excluded.cell = lyracore_shared::spatial::grid_cell_id(grid_x, grid_y);
    ctx.db.game_world_entity().guid().update(excluded);
    Ok(())
}

/// Run one normal parked pass for every prepared subject at this reducer's single timestamp.
#[reducer]
pub fn playerbots_quest_loop_fixture_pass_dispersion(
    ctx: &ReducerContext,
    excluded_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let mut subjects: Vec<_> = ctx
        .db
        .pkg_playerbots_bot()
        .iter()
        .filter(|bot| bot.character_guid != excluded_guid)
        .map(|bot| bot.character_guid)
        .collect();
    subjects.sort_unstable();
    if subjects.len() != DISPERSION_POPULATION {
        return Err("dispersion fixture requires exactly 25 subjects".to_string());
    }
    for guid in subjects {
        super::fixture::playerbots_fixture_runner_pass_once(ctx, guid)?;
    }
    Ok(())
}

pub(super) fn stage_named_with_rotation(
    ctx: &ReducerContext,
    character_guid: u64,
    source_insertion_rotation: u8,
) -> Result<Vec<u64>, String> {
    if source_insertion_rotation >= 10 {
        return Err("quest target insertion rotation must be below 10".to_string());
    }
    playerbots_quest_fixture_stage(ctx, character_guid)?;
    require_fixture(ctx)?;
    stage_loopback_smite(ctx)?;
    let mut character = ctx
        .db
        .game_world_entity()
        .guid()
        .find(character_guid)
        .ok_or("Character missing")?;
    character.health = character.max_health;
    if ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(character_guid)
        .next()
        .is_some_and(|bot| bot.class == 5)
    {
        character.max_power = LOOPBACK_PRIEST_MANA;
        character.power = LOOPBACK_PRIEST_MANA;
    }
    ctx.db.game_world_entity().guid().update(character);
    let character = ctx
        .db
        .game_world_entity()
        .guid()
        .find(character_guid)
        .ok_or("Character missing after health update")?;
    let templates = ctx.db.game_creature_template();
    let mut template = templates
        .entry()
        .find(6)
        .ok_or("quest target template missing")?;
    template.health = 1;
    template.max_level_health = 1;
    template.damage_min = 0;
    template.damage_max = 0;
    templates.entry().update(template);
    let template = templates
        .entry()
        .find(6)
        .ok_or("quest target template missing after update")?;
    for entry in [69, 299] {
        let mut source_template = templates
            .entry()
            .find(entry)
            .ok_or("collect source template missing")?;
        source_template.health = 1;
        source_template.max_level_health = 1;
        source_template.damage_min = 0;
        source_template.damage_max = 0;
        templates.entry().update(source_template);
        let guid = creature_guid(entry);
        let mut source = ctx
            .db
            .game_world_entity()
            .guid()
            .find(guid)
            .ok_or("collect source missing")?;
        source.health = 1;
        source.max_health = 1;
        ctx.db.game_world_entity().guid().update(source);
    }
    let spawns = ctx.db.game_creature_spawn();
    let entities = ctx.db.game_world_entity();
    let mut inserted_sources = Vec::with_capacity(10);
    for insertion in 0..10u64 {
        let offset = (insertion + u64::from(source_insertion_rotation)) % 10;
        let guid = creature_guid(6).saturating_add(offset);
        let x = character.x + 160.0 + offset as f32 * 0.4;
        let y = character.y + (offset % 2) as f32 * 0.4;
        let spawn = crate::CreatureSpawn {
            guid,
            entry: 6,
            map_id: character.map_id,
            x,
            y,
            z: character.z,
            orientation: 0.0,
            respawn_at: crate::creatures::timer_never(ctx),
            despawn_at: crate::creatures::timer_never(ctx),
            movement_type: 0,
            respawn_secs: 60,
            life_seq: 1,
        };
        let entity = crate::creatures::build_creature_entity(&spawn, &template, 0, 0);
        spawns.guid().delete(guid);
        spawns.insert(spawn);
        entities.guid().delete(guid);
        crate::creatures::insert_creature_entity(ctx, entity);
        inserted_sources.push(guid);
    }
    let fixtures = ctx.db.pkg_playerbots_quest_loop_fixture();
    let row = PlayerbotsQuestLoopFixture {
        character_guid,
        quest_entry: 7,
        target_entry: 6,
        target_count: 10,
        content_revision: NAMED_LOOP_CONTENT.to_string(),
    };
    if fixtures.character_guid().find(character_guid).is_some() {
        fixtures.character_guid().update(row);
    } else {
        fixtures.insert(row);
    }
    quest_catalog::refresh_catalog(ctx, "unknown");
    Ok(inserted_sources)
}

/// Fill the first spatial cell visited by the quest target query. The rows are outside the circular
/// search radius, so they exercise the raw-read budget without becoming eligible quest targets.
#[reducer]
pub fn playerbots_quest_loop_fixture_stage_search_limit(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    if ctx
        .db
        .pkg_playerbots_quest_loop_fixture()
        .character_guid()
        .find(character_guid)
        .is_none()
    {
        return Err("named quest-loop fixture is absent".to_string());
    }
    let character = crate::helpers::live_entity(ctx, character_guid)?;
    let templates = ctx.db.game_creature_template();
    let mut template = templates
        .entry()
        .find(6)
        .ok_or("quest target template missing")?;
    template.entry = SEARCH_LIMIT_ENTRY;
    template.aggro_range = 0;
    templates.entry().delete(SEARCH_LIMIT_ENTRY);
    let template = templates.insert(template);
    let spawns = ctx.db.game_creature_spawn();
    let entities = ctx.db.game_world_entity();
    for offset in 0..SEARCH_LIMIT_ROWS {
        let guid =
            (0xF130u64 << 48) | (u64::from(SEARCH_LIMIT_ENTRY) << 24) | offset.saturating_add(1);
        let spawn = crate::CreatureSpawn {
            guid,
            entry: SEARCH_LIMIT_ENTRY,
            map_id: character.map_id,
            x: character.x + 90.0,
            y: character.y + 90.0,
            z: character.z,
            orientation: 0.0,
            respawn_at: crate::creatures::timer_never(ctx),
            despawn_at: crate::creatures::timer_never(ctx),
            movement_type: 0,
            respawn_secs: 0,
            life_seq: 1,
        };
        let entity = crate::creatures::build_creature_entity(&spawn, &template, 0, 0);
        spawns.guid().delete(guid);
        spawns.insert(spawn);
        entities.guid().delete(guid);
        crate::creatures::insert_creature_entity(ctx, entity);
    }
    Ok(())
}

#[reducer]
pub fn playerbots_quest_loop_fixture_set_partition(
    ctx: &ReducerContext,
    character_guid: u64,
    instance_id: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let mut character = crate::helpers::live_entity(ctx, character_guid)?;
    character.instance_id = instance_id;
    ctx.db.game_creature_spline().guid().delete(character_guid);
    ctx.db.game_world_entity().guid().update(character);
    Ok(())
}

#[reducer]
pub fn playerbots_quest_loop_fixture_refresh(ctx: &ReducerContext) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    reject_imported_content(ctx)?;
    quest_catalog::refresh_catalog(ctx, "unknown");
    if ctx
        .db
        .pkg_playerbots_catalog_quest()
        .quest_entry()
        .find(SEEDED_USE_QUEST)
        .is_none()
        || ctx
            .db
            .pkg_playerbots_catalog_objective()
            .id()
            .find(u64::from(SEEDED_USE_QUEST) << 8)
            .is_none()
    {
        return Err("simple GameObject catalog extension was lost on refresh".to_string());
    }
    Ok(())
}

#[reducer]
pub fn playerbots_quest_loop_fixture_set_simple_gameobject_state(
    ctx: &ReducerContext,
    state: u8,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    reject_imported_content(ctx)?;
    if state > 1 {
        return Err("simple GameObject fixture state must be 0 or 1".to_string());
    }
    let rows = ctx.db.game_gameobject();
    let mut row = rows
        .guid()
        .find(gameobject_guid(SEEDED_USE_GAMEOBJECT))
        .ok_or("simple GameObject fixture is absent")?;
    row.state = state;
    rows.guid().update(row);
    Ok(())
}

#[reducer]
pub fn playerbots_quest_loop_fixture_add_gameobject_alternative(
    ctx: &ReducerContext,
    gameobject_entry: u32,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    reject_imported_content(ctx)?;
    match gameobject_entry {
        SEEDED_USE_GAMEOBJECT => {
            if ctx
                .db
                .pkg_playerbots_seeded_quest_fixture()
                .quest_entry()
                .find(SEEDED_USE_QUEST)
                .is_none()
            {
                return Err("simple GameObject fixture is absent".to_string());
            }
        }
        CHEST_ENTRY => require_fixture(ctx)?,
        _ => return Err("unsupported alternative GameObject fixture entry".to_string()),
    }
    insert_alternative_gameobject(ctx, gameobject_entry)?;
    Ok(())
}

/// Add nine inaccessible corpses for the currently retained creature-loot source. The runner must
/// report an inconclusive bounded read instead of attacking another source.
#[reducer]
pub fn playerbots_quest_loop_fixture_stage_corpse_limit(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let retained = ctx
        .db
        .pkg_playerbots_quest_objective()
        .character_guid()
        .find(character_guid)
        .ok_or("retained quest objective is absent")?;
    if retained.target.executor != ObjectiveExecutor::CreatureLoot {
        return Err("retained quest objective is not creature loot".to_string());
    }
    let source = retained
        .target
        .source
        .ok_or("retained creature-loot source is absent")?;
    let character = crate::helpers::live_entity(ctx, character_guid)?;
    let template = ctx
        .db
        .game_creature_template()
        .entry()
        .find(source.entry)
        .ok_or("retained creature-loot template is absent")?;
    let spawns = ctx.db.game_creature_spawn();
    let entities = ctx.db.game_world_entity();
    for offset in 10..19u64 {
        let guid = creature_guid(source.entry).saturating_add(offset);
        let spawn = crate::CreatureSpawn {
            guid,
            entry: source.entry,
            map_id: character.map_id,
            x: character.x + 3.0 + offset as f32 * 0.1,
            y: character.y,
            z: character.z,
            orientation: 0.0,
            respawn_at: crate::creatures::timer_never(ctx),
            despawn_at: crate::creatures::timer_never(ctx),
            movement_type: 0,
            respawn_secs: 60,
            life_seq: 1,
        };
        let mut entity = crate::creatures::build_creature_entity(&spawn, &template, 0, 0);
        entity.health = 0;
        entity.dead = true;
        spawns.guid().delete(guid);
        spawns.insert(spawn);
        entities.guid().delete(guid);
        crate::creatures::insert_creature_entity(ctx, entity);
    }
    Ok(())
}

/// Leave the corpse search inconclusive while one independently valid live source remains.
#[reducer]
pub fn playerbots_quest_loop_fixture_stage_corpse_limit_with_live_alternative(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    playerbots_quest_loop_fixture_stage_corpse_limit(ctx, character_guid)?;
    let retained = ctx
        .db
        .pkg_playerbots_quest_objective()
        .character_guid()
        .find(character_guid)
        .ok_or("retained quest objective is absent")?;
    let source = retained
        .target
        .source
        .ok_or("retained creature-loot source is absent")?;
    let character = crate::helpers::live_entity(ctx, character_guid)?;
    let template = ctx
        .db
        .game_creature_template()
        .entry()
        .find(source.entry)
        .ok_or("retained creature-loot template is absent")?;
    let source_spawn = ctx
        .db
        .game_creature_spawn()
        .guid()
        .find(source.guid)
        .ok_or("retained creature-loot spawn is absent")?;
    let alternative_guid = source.guid.saturating_add(1);
    if ctx
        .db
        .game_creature_spawn()
        .guid()
        .find(alternative_guid)
        .is_some()
        || ctx
            .db
            .game_world_entity()
            .guid()
            .find(alternative_guid)
            .is_some()
    {
        return Err("alternate creature-loot source is occupied".to_string());
    }
    crate::creatures::despawn_creature_entity(ctx, source.guid);
    let spawn = ctx.db.game_creature_spawn().insert(crate::CreatureSpawn {
        guid: alternative_guid,
        entry: source.entry,
        map_id: character.map_id,
        x: character.x + 2.0,
        y: character.y,
        z: character.z,
        orientation: source_spawn.orientation,
        respawn_at: crate::creatures::timer_never(ctx),
        despawn_at: crate::creatures::timer_never(ctx),
        movement_type: source_spawn.movement_type,
        respawn_secs: source_spawn.respawn_secs,
        life_seq: source_spawn.life_seq,
    });
    crate::creatures::insert_creature_entity(
        ctx,
        crate::creatures::build_creature_entity(&spawn, &template, 0, 0),
    );
    Ok(())
}

#[reducer]
pub fn playerbots_quest_fixture_admit_accept(
    ctx: &ReducerContext,
    character_guid: u64,
    quest_entry: u32,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let prerequisite = ctx
        .db
        .game_quest_template()
        .entry()
        .find(quest_entry)
        .ok_or("quest missing")?
        .prev_quest_id;
    reward_prerequisite(ctx, character_guid, prerequisite)?;
    match quest_catalog::admit_available(ctx, character_guid, quest_entry) {
        Ok(admission) => {
            quest_catalog::record_admission(
                ctx,
                character_guid,
                quest_entry,
                Some(quest_entry),
                None,
            );
            let giver = admission
                .start
                .ok_or("available quest has no current start giver")?;
            super::actions::accept_quest(ctx, character_guid, giver.guid, quest_entry)
                .map_err(Into::into)
        }
        Err(refusal) => {
            quest_catalog::record_admission(ctx, character_guid, quest_entry, None, Some(&refusal));
            Err(match refusal {
                AdmissionRefusal::Ineligible(detail)
                | AdmissionRefusal::Unsupported { detail, .. } => detail,
            })
        }
    }
}

#[reducer]
pub fn playerbots_quest_fixture_turn_in(
    ctx: &ReducerContext,
    character_guid: u64,
    quest_entry: u32,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let admission = quest_catalog::admit_held(ctx, character_guid, quest_entry).map_err(
        |refusal| match refusal {
            AdmissionRefusal::Ineligible(detail) | AdmissionRefusal::Unsupported { detail, .. } => {
                detail
            }
        },
    )?;
    super::actions::turn_in_quest(
        ctx,
        character_guid,
        admission.actual_ender.guid,
        quest_entry,
        0,
    )
    .map_err(Into::into)
}

#[reducer]
pub fn playerbots_quest_fixture_kill(
    ctx: &ReducerContext,
    character_guid: u64,
    creature_entry: u32,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let character = crate::helpers::live_entity(ctx, character_guid)?;
    let target =
        match super::quest_loop::live_creature_target(ctx, &character, creature_entry, |_| true) {
            super::quest_loop::LiveCreatureTarget::Found(target) => target,
            super::quest_loop::LiveCreatureTarget::Missing => {
                return Err("no live target".to_string());
            }
            super::quest_loop::LiveCreatureTarget::Deferred
            | super::quest_loop::LiveCreatureTarget::Controlled => {
                return Err("live target is temporarily ineligible".to_string());
            }
            super::quest_loop::LiveCreatureTarget::ReadLimit => {
                return Err("live target read limit".to_string());
            }
        };
    super::actions::attack(ctx, character_guid, target.guid).map_err(String::from)?;
    let (amount, _) = crate::combat::fold_incoming_damage(ctx, character_guid, target.guid, 10_000);
    let damage = crate::combat::final_damage(ctx, target.guid, amount);
    let outcome = crate::combat::apply_hit(
        ctx,
        character_guid,
        target.guid,
        damage,
        crate::combat::Hit::weapon(crate::combat::HitSource::MainHand, false),
    );
    if outcome.killed {
        Ok(())
    } else {
        Err("fixture hit was not lethal".to_string())
    }
}

#[reducer]
pub fn playerbots_quest_fixture_take_creature_loot(
    ctx: &ReducerContext,
    character_guid: u64,
    creature_entry: u32,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let guid = creature_guid(creature_entry);
    super::actions::open_creature_loot(ctx, character_guid, guid, 33).map_err(String::from)?;
    let slots: Vec<_> = ctx
        .db
        .game_corpse_loot()
        .by_corpse()
        .filter(guid)
        .map(|row| row.slot)
        .collect();
    if slots.is_empty() {
        return Err("creature produced no loot".to_string());
    }
    for slot in slots {
        super::actions::take_loot(ctx, character_guid, guid, slot, 33).map_err(String::from)?;
    }
    Ok(())
}

#[reducer]
pub fn playerbots_quest_fixture_use_gameobject(
    ctx: &ReducerContext,
    character_guid: u64,
    gameobject_entry: u32,
    take_loot: bool,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let guid = gameobject_guid(gameobject_entry);
    super::actions::use_gameobject(ctx, character_guid, guid, DIRECT_GO_QUEST)
        .map_err(String::from)?;
    if take_loot {
        let slots: Vec<_> = ctx
            .db
            .game_corpse_loot()
            .by_corpse()
            .filter(guid)
            .map(|row| row.slot)
            .collect();
        if slots.is_empty() {
            return Err("GameObject produced no loot".to_string());
        }
        for slot in slots {
            super::actions::take_loot(ctx, character_guid, guid, slot, DIRECT_GO_QUEST)
                .map_err(String::from)?;
        }
    }
    Ok(())
}

#[reducer]
pub fn playerbots_quest_fixture_fill_inventory(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let source_entry = ctx
        .db
        .game_item_instance()
        .by_owner_guid()
        .filter(character_guid)
        .next()
        .ok_or("starter item missing")?
        .entry;
    clone_item(ctx, INVENTORY_FILLER, source_entry)?;
    let templates = ctx.db.game_item_template();
    let mut filler = templates
        .entry()
        .find(INVENTORY_FILLER)
        .ok_or("inventory filler template missing")?;
    filler.max_stack = 1;
    templates.entry().update(filler);
    for _ in 0..64 {
        if !crate::items::has_free_slot(ctx, character_guid) {
            return Ok(());
        }
        crate::items::grant_item(ctx, character_guid, INVENTORY_FILLER, 1)?;
    }
    Err("fixture did not fill bounded inventory space".to_string())
}

#[reducer]
pub fn playerbots_quest_fixture_try_take_gameobject_loot(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let guid = gameobject_guid(CHEST_ENTRY);
    let slot = ctx
        .db
        .game_corpse_loot()
        .by_corpse()
        .filter(guid)
        .next()
        .ok_or("GameObject produced no loot")?
        .slot;
    let refusal = super::actions::take_loot(ctx, character_guid, guid, slot, DIRECT_GO_QUEST)
        .expect_err("full inventory accepted GameObject loot");
    if refusal.kind != crate::actor::ActionRefusalKind::InventoryFull {
        return Err(format!("expected inventory-full refusal, got {refusal}"));
    }
    Ok(())
}

#[reducer]
pub fn playerbots_quest_fixture_clear_filler_and_take_gameobject_loot(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let items = ctx.db.game_item_instance();
    for item in items
        .by_owner_guid()
        .filter(character_guid)
        .filter(|item| item.entry == INVENTORY_FILLER)
        .collect::<Vec<_>>()
    {
        items.guid().delete(item.guid);
    }
    let guid = gameobject_guid(CHEST_ENTRY);
    let slots: Vec<_> = ctx
        .db
        .game_corpse_loot()
        .by_corpse()
        .filter(guid)
        .map(|row| row.slot)
        .collect();
    if slots.is_empty() {
        return Err("GameObject produced no loot".to_string());
    }
    for slot in slots {
        super::actions::take_loot(ctx, character_guid, guid, slot, DIRECT_GO_QUEST)
            .map_err(String::from)?;
    }
    Ok(())
}

#[reducer]
pub fn playerbots_quest_fixture_try_use_gameobject(
    ctx: &ReducerContext,
    character_guid: u64,
    gameobject_entry: u32,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let _ = super::actions::use_gameobject(
        ctx,
        character_guid,
        gameobject_guid(gameobject_entry),
        DIRECT_GO_QUEST,
    );
    Ok(())
}

#[reducer]
pub fn playerbots_quest_fixture_direct_gameobject(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    reward_prerequisite(ctx, character_guid, 3903)?;
    let templates = ctx.db.game_gameobject_template();
    let mut template = templates
        .entry()
        .find(CHEST_ENTRY)
        .ok_or("GameObject template missing")?;
    template.type_id = crate::gameobject::go_type::GOOBER;
    template.data1 = 0;
    templates.entry().update(template);
    insert_objective(
        ctx,
        DIRECT_GO_QUEST,
        0,
        crate::quest::objective_kind::USE_GAMEOBJECT,
        CHEST_ENTRY,
        1,
    );
    quest_catalog::refresh_catalog(ctx, "unknown");
    let catalog = ctx.db.pkg_playerbots_catalog_objective();
    let id = u64::from(DIRECT_GO_QUEST) << 8;
    let previous = catalog.id().find(id).ok_or("catalog objective missing")?;
    catalog.id().update(PlayerbotsCatalogObjective {
        kind: CatalogObjectiveKind::UseGameObject,
        target_entry: CHEST_ENTRY,
        required_count: 1,
        executor: ObjectiveExecutor::SimpleGameObject,
        source_kind: Some(CatalogEntityKind::GameObject),
        source_entries: vec![CHEST_ENTRY],
        ..previous
    });
    playerbots_quest_fixture_admit_accept(ctx, character_guid, DIRECT_GO_QUEST)
}

#[reducer]
pub fn playerbots_quest_fixture_mixed_unsupported(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    reward_prerequisite(ctx, character_guid, 783)?;
    insert_objective(ctx, 7, 1, crate::quest::objective_kind::KILL_CREATURE, 6, 1);
    quest_catalog::refresh_catalog(ctx, "unknown");
    let catalog = ctx.db.pkg_playerbots_catalog_objective();
    let id = (u64::from(7u32) << 8) | 1;
    catalog.id().delete(id);
    catalog.insert(PlayerbotsCatalogObjective {
        id,
        quest_entry: 7,
        objective_index: 1,
        kind: CatalogObjectiveKind::Escort,
        target_entry: 6,
        required_count: 1,
        executor: ObjectiveExecutor::Attack,
        source_kind: Some(CatalogEntityKind::Creature),
        source_entries: vec![6],
        source_destinations: Vec::new(),
        work_area: None,
        destination_evidence_revision: FIXTURE_REVISION.to_string(),
        catalog_revision: CATALOG_REVISION,
    });
    let refusal = quest_catalog::admit_available(ctx, character_guid, 7)
        .expect_err("mixed unsupported quest must be refused");
    quest_catalog::record_admission(ctx, character_guid, 7, None, Some(&refusal));
    Ok(())
}

#[reducer]
pub fn playerbots_quest_fixture_mixed_progress(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    reward_prerequisite(ctx, character_guid, 783)?;
    insert_objective(
        ctx,
        7,
        0,
        crate::quest::objective_kind::COLLECT_ITEM,
        750,
        8,
    );
    insert_objective(
        ctx,
        7,
        1,
        crate::quest::objective_kind::COLLECT_ITEM,
        752,
        12,
    );
    insert_objective(ctx, 7, 2, crate::quest::objective_kind::KILL_CREATURE, 6, 1);
    crate::items::grant_item(ctx, character_guid, 750, 8)?;
    crate::items::grant_item(ctx, character_guid, 752, 12)?;
    let items = ctx.db.game_item_instance();
    let mut banked = items
        .by_owner_guid()
        .filter(character_guid)
        .find(|item| item.entry == 752)
        .ok_or("bank fixture item missing")?;
    banked.slot = 39;
    items.guid().update(banked);

    quest_catalog::refresh_catalog(ctx, "unknown");
    let objectives = ctx.db.pkg_playerbots_catalog_objective();
    let collect_carried = objectives
        .id()
        .find(u64::from(33u32) << 8)
        .ok_or("quest 33 catalog objective missing")?;
    let collect_banked = objectives
        .id()
        .find(u64::from(18u32) << 8)
        .ok_or("quest 18 catalog objective missing")?;
    let mut kill = objectives
        .id()
        .find(u64::from(7u32) << 8)
        .ok_or("quest 7 catalog objective missing")?;
    for row in objectives.by_quest().filter(7u32).collect::<Vec<_>>() {
        objectives.id().delete(row.id);
    }
    objectives.insert(PlayerbotsCatalogObjective {
        id: u64::from(7u32) << 8,
        quest_entry: 7,
        objective_index: 0,
        ..collect_carried
    });
    objectives.insert(PlayerbotsCatalogObjective {
        id: (u64::from(7u32) << 8) | 1,
        quest_entry: 7,
        objective_index: 1,
        ..collect_banked
    });
    kill.id = (u64::from(7u32) << 8) | 2;
    kill.objective_index = 2;
    kill.required_count = 1;
    objectives.insert(kill);

    let admission =
        quest_catalog::admit_available(ctx, character_guid, 7).map_err(
            |refusal| match refusal {
                AdmissionRefusal::Ineligible(detail)
                | AdmissionRefusal::Unsupported { detail, .. } => detail,
            },
        )?;
    if admission.target.objective_index != 1 || admission.target.target_entry != 752 {
        return Err("banked collect item changed the selected objective".to_string());
    }
    quest_catalog::record_admission(ctx, character_guid, 7, Some(7), None);
    let giver = admission
        .start
        .ok_or("available quest has no current start giver")?;
    super::actions::accept_quest(ctx, character_guid, giver.guid, 7).map_err(Into::into)
}

#[reducer]
pub fn playerbots_quest_fixture_unsupported(
    ctx: &ReducerContext,
    character_guid: u64,
    unsupported_kind: u8,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    if unsupported_kind == 4 {
        reward_prerequisite(ctx, character_guid, 3903)?;
        let templates = ctx.db.game_gameobject_template();
        let mut template = templates
            .entry()
            .find(CHEST_ENTRY)
            .ok_or("GameObject template missing")?;
        template.lock_id = 1;
        templates.entry().update(template);
        quest_catalog::refresh_catalog(ctx, "unknown");
        let refusal = quest_catalog::admit_available(ctx, character_guid, 3904)
            .expect_err("locked GameObject quest must be refused");
        quest_catalog::record_admission(ctx, character_guid, 3904, None, Some(&refusal));
        return Ok(());
    }
    reward_prerequisite(ctx, character_guid, 783)?;
    let kind = match unsupported_kind {
        0 => CatalogObjectiveKind::ExploreAreaTrigger,
        1 => CatalogObjectiveKind::Escort,
        2 => CatalogObjectiveKind::ScriptedEvent,
        3 => CatalogObjectiveKind::Transport,
        _ => return Err("unsupported fixture kind must be 0 through 4".to_string()),
    };
    let catalog = ctx.db.pkg_playerbots_catalog_objective();
    let id = u64::from(7u32) << 8;
    let previous = catalog.id().find(id).ok_or("catalog objective missing")?;
    catalog
        .id()
        .update(PlayerbotsCatalogObjective { kind, ..previous });
    let refusal = quest_catalog::admit_available(ctx, character_guid, 7)
        .expect_err("unsupported quest must be refused");
    quest_catalog::record_admission(ctx, character_guid, 7, None, Some(&refusal));
    Ok(())
}

#[reducer]
pub fn playerbots_quest_fixture_level_admission(
    ctx: &ReducerContext,
    character_guid: u64,
    level: u32,
    quest_entry: u32,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let rows = ctx.db.game_world_entity();
    let mut character = rows
        .guid()
        .find(character_guid)
        .ok_or("Character missing")?;
    character.level = level;
    rows.guid().update(character);
    match quest_catalog::admit_available(ctx, character_guid, quest_entry) {
        Ok(admission) => quest_catalog::record_admission(
            ctx,
            character_guid,
            quest_entry,
            Some(admission.quest_entry),
            None,
        ),
        Err(refusal) => {
            quest_catalog::record_admission(ctx, character_guid, quest_entry, None, Some(&refusal))
        }
    }
    Ok(())
}

#[reducer]
pub fn playerbots_quest_fixture_held_becomes_unsupported(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let alternative = quest_catalog::admit_available(ctx, character_guid, 5261).map_err(
        |refusal| match refusal {
            AdmissionRefusal::Ineligible(detail) | AdmissionRefusal::Unsupported { detail, .. } => {
                detail
            }
        },
    )?;
    let giver = alternative
        .start
        .ok_or("available quest has no current start giver")?;
    super::actions::accept_quest(ctx, character_guid, giver.guid, 5261).map_err(String::from)?;
    let catalog = ctx.db.pkg_playerbots_catalog_objective();
    let id = u64::from(7u32) << 8;
    let previous = catalog.id().find(id).ok_or("catalog objective missing")?;
    catalog.id().update(PlayerbotsCatalogObjective {
        kind: CatalogObjectiveKind::Escort,
        ..previous
    });
    let quests = ctx.db.pkg_playerbots_catalog_quest();
    let mut held = quests
        .quest_entry()
        .find(5261)
        .ok_or("held alternative catalog row missing")?;
    held.catalog_order = u16::MAX;
    for offset in 0..CATALOG_WALK_LIMIT {
        let prefix = PlayerbotsCatalogQuest {
            quest_entry: HELD_CATALOG_PREFIX_BASE + offset as u32,
            catalog_revision: held.catalog_revision,
            catalog_order: 100 + offset as u16,
            min_level: held.min_level,
            required_races: held.required_races,
            required_classes: held.required_classes,
            prerequisite_quest: held.prerequisite_quest,
            start_kind: held.start_kind,
            start_entry: held.start_entry,
            start_destinations: held.start_destinations.clone(),
            actual_ender_kind: held.actual_ender_kind,
            actual_ender_entry: held.actual_ender_entry,
            actual_ender_destinations: held.actual_ender_destinations.clone(),
            content_revision: held.content_revision.clone(),
        };
        quests.quest_entry().delete(prefix.quest_entry);
        quests.insert(prefix);
    }
    quests.quest_entry().update(held);
    let selected = match quest_catalog::reconcile_active(ctx, character_guid, &[]) {
        quest_catalog::ReconcileResult::Found(selected) => selected,
        quest_catalog::ReconcileResult::Missing => {
            return Err("supported held alternative missing".to_string());
        }
        quest_catalog::ReconcileResult::ReadLimit => {
            return Err("held quest read limit reached".to_string());
        }
    };
    if selected.quest_entry != 5261 {
        return Err(format!(
            "expected quest 5261 after quest 7 became unsupported, got {}",
            selected.quest_entry
        ));
    }
    Ok(())
}

#[reducer]
pub fn playerbots_quest_fixture_active_log_overflow(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let quests = ctx.db.game_character_quest();
    let exemplar = quests
        .by_character_active()
        .filter((character_guid, false, false))
        .next()
        .ok_or("active fixture quest missing")?;
    for offset in 0..crate::quest::MAX_QUEST_LOG_SIZE {
        let quest_entry = ACTIVE_QUEST_OVERFLOW_BASE + offset as u32;
        if let Some(previous) = quests
            .by_character_quest()
            .filter((character_guid, quest_entry))
            .next()
        {
            quests.id().delete(previous.id);
        }
        quests.insert(crate::quest::CharacterQuest {
            id: 0,
            character_guid: exemplar.character_guid,
            owner_identity: exemplar.owner_identity,
            quest_entry,
            counts: Vec::new(),
            rewarded: false,
            deadline_micros: exemplar.deadline_micros,
            failed: false,
        });
    }
    Ok(())
}

#[reducer]
pub fn playerbots_quest_fixture_lose_provided_item(
    ctx: &ReducerContext,
    character_guid: u64,
    bank_item: bool,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let items = ctx.db.game_item_instance();
    let mut item = items
        .by_owner_guid()
        .filter(character_guid)
        .find(|item| item.entry == 11125)
        .ok_or("provided delivery item missing")?;
    if bank_item {
        item.slot = 39;
        items.guid().update(item);
    } else {
        items.guid().delete(item.guid);
    }
    match quest_catalog::reconcile_active(ctx, character_guid, &[]) {
        quest_catalog::ReconcileResult::Found(_) => {
            return Err("held quest remained admitted without its provided item".to_string());
        }
        quest_catalog::ReconcileResult::ReadLimit => {
            return Err("held quest read limit reached".to_string());
        }
        quest_catalog::ReconcileResult::Missing => {}
    }
    Ok(())
}

#[reducer]
pub fn playerbots_quest_fixture_hide_live_target(
    ctx: &ReducerContext,
    creature_entry: u32,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    crate::creatures::despawn_creature_entity(ctx, creature_guid(creature_entry));
    Ok(())
}

/// Replace one declared Quest 7 target with a fresh life at its existing spawn point.
#[reducer]
pub fn playerbots_quest_loop_fixture_respawn_target(
    ctx: &ReducerContext,
    character_guid: u64,
    target_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let fixture = ctx
        .db
        .pkg_playerbots_quest_loop_fixture()
        .character_guid()
        .find(character_guid)
        .ok_or("named quest-loop fixture is absent")?;
    let first = creature_guid(6);
    if fixture.quest_entry != 7
        || fixture.target_entry != 6
        || fixture.target_count != 10
        || fixture.content_revision != NAMED_LOOP_CONTENT
        || !(first..first + u64::from(fixture.target_count)).contains(&target_guid)
    {
        return Err("named quest-loop target identity differs".to_string());
    }
    let spawn = ctx
        .db
        .game_creature_spawn()
        .guid()
        .find(target_guid)
        .ok_or("quest target spawn is absent")?;
    let template = ctx
        .db
        .game_creature_template()
        .entry()
        .find(spawn.entry)
        .ok_or("quest target template is absent")?;
    crate::creatures::despawn_creature_entity(ctx, target_guid);
    let entity = crate::creatures::build_creature_entity(&spawn, &template, 0, 0);
    crate::creatures::insert_creature_entity(ctx, entity);
    Ok(())
}

/// Move Quest 7's primary target 40 yards from its spawn so its ordinary idle pass starts a spline.
#[reducer]
pub fn playerbots_quest_loop_fixture_prepare_moving_cast(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let bot = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(character_guid)
        .next()
        .ok_or("moving-cast fixture bot missing")?;
    if bot.class != 8 {
        return Err("moving-cast fixture requires a Mage".to_string());
    }
    ctx.db
        .game_character_quest()
        .by_character_quest()
        .filter((character_guid, 7u32))
        .next()
        .filter(|quest| !quest.rewarded && quest.counts.first() == Some(&0))
        .ok_or("moving-cast fixture requires open Quest 7 with zero credit")?;
    let spells = ctx.db.game_spell();
    let mut spell = spells
        .spell_id()
        .find(133)
        .ok_or("moving-cast fixture requires Fireball")?;
    spell.range_yd = 35;
    spell.cast_time_ms = 1_500;
    spells.spell_id().update(spell);

    let target_guid = creature_guid(6);
    let entities = ctx.db.game_world_entity();
    let mut target = entities
        .guid()
        .find(target_guid)
        .filter(|target| !target.dead && target.health > 0)
        .ok_or("moving-cast fixture target missing")?;
    if (target.map_id, target.instance_id) != (0, 0) {
        return Err("moving-cast fixture target partition differs".to_string());
    }
    let target_x = target.x;
    let target_y = target.y;
    let target_z = target.z;
    target.x = target_x;
    target.y = target_y;
    target.z = target_z;
    target.target_guid = 0;
    let (grid_x, grid_y) = lyracore_shared::spatial::grid_cell(target.x, target.y);
    target.grid_x = grid_x;
    target.grid_y = grid_y;
    target.cell = lyracore_shared::spatial::grid_cell_id(grid_x, grid_y);
    entities.guid().update(target);
    ctx.db.game_creature_spline().guid().delete(target_guid);

    let mut character = crate::helpers::live_entity(ctx, character_guid)?;
    for alternative_guid in target_guid + 1..target_guid + 10 {
        entities
            .guid()
            .find(alternative_guid)
            .filter(|alternative| alternative.entry == 6 && !alternative.dead)
            .ok_or("moving-cast fixture alternative missing")?;
        crate::creatures::despawn_creature_entity(ctx, alternative_guid);
    }
    character.x = target_x - 80.0;
    character.y = target_y;
    character.z = target_z;
    let (grid_x, grid_y) = lyracore_shared::spatial::grid_cell(character.x, character.y);
    character.grid_x = grid_x;
    character.grid_y = grid_y;
    character.cell = lyracore_shared::spatial::grid_cell_id(grid_x, grid_y);
    entities.guid().update(character);
    ctx.db.game_creature_spline().guid().delete(character_guid);

    let spawns = ctx.db.game_creature_spawn();
    let mut spawn = spawns
        .guid()
        .find(target_guid)
        .ok_or("moving-cast fixture target spawn missing")?;
    spawn.x = target_x + 40.0;
    spawn.y = target_y;
    spawn.z = target_z;
    spawn.movement_type = 0;
    spawns.guid().update(spawn);

    Ok(())
}

/// Put the Mage 38.5 yards behind Quest 7's already-moving target, then run one real decision.
#[reducer]
pub fn playerbots_quest_loop_fixture_start_moving_cast(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let bot = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(character_guid)
        .next()
        .filter(|bot| bot.class == 8)
        .ok_or("moving-cast fixture requires a Mage")?;
    ctx.db
        .game_character_quest()
        .by_character_quest()
        .filter((character_guid, 7u32))
        .next()
        .filter(|quest| !quest.rewarded && quest.counts.first() == Some(&0))
        .ok_or("moving-cast fixture requires open Quest 7 with zero credit")?;
    ctx.db
        .game_spell()
        .spell_id()
        .find(133)
        .filter(|spell| spell.range_yd == 35 && spell.cast_time_ms == 1_500)
        .ok_or("moving-cast fixture requires the staged Fireball")?;
    let target_guid = creature_guid(6);
    let target = crate::helpers::live_entity(ctx, target_guid)?;
    if target.dead || (target.map_id, target.instance_id) != (0, 0) {
        return Err("moving-cast fixture target is unavailable".to_string());
    }
    ctx.db
        .game_creature_spline()
        .guid()
        .find(target_guid)
        .filter(|spline| spline.dx > spline.sx)
        .ok_or("moving-cast fixture target is not moving forward")?;

    let caster_x = target.x - 38.5;
    let mut character = crate::helpers::live_entity(ctx, character_guid)?;
    character.x = caster_x;
    character.y = target.y;
    character.z = target.z;
    let (grid_x, grid_y) = lyracore_shared::spatial::grid_cell(character.x, character.y);
    character.grid_x = grid_x;
    character.grid_y = grid_y;
    character.cell = lyracore_shared::spatial::grid_cell_id(grid_x, grid_y);
    ctx.db.game_world_entity().guid().update(character);
    ctx.db.game_creature_spline().guid().delete(character_guid);

    let mut parked = bot;
    parked.home_map = 0;
    parked.home_x = caster_x;
    parked.home_y = target.y;
    parked.home_z = target.z;
    ctx.db.pkg_playerbots_bot().id().update(parked);
    super::fixture::playerbots_fixture_runner_select_cohort(ctx, character_guid)?;
    super::fixture::playerbots_fixture_runner_pass_once(ctx, character_guid)
}

#[reducer]
pub fn playerbots_quest_loop_fixture_make_target_friendly(
    ctx: &ReducerContext,
    character_guid: u64,
    target_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let factions = ctx.db.game_faction_template();
    if factions.id().find(FRIENDLY_FIXTURE_FACTION).is_none() {
        factions.insert(crate::FactionTemplate {
            id: FRIENDLY_FIXTURE_FACTION,
            faction: FRIENDLY_FIXTURE_FACTION,
            faction_group: 1,
            friend_group: 1,
            enemy_group: 0,
            enemy_0: 0,
            enemy_1: 0,
            enemy_2: 0,
            enemy_3: 0,
            friend_0: 0,
            friend_1: 0,
            friend_2: 0,
            friend_3: 0,
        });
    }
    let rows = ctx.db.game_world_entity();
    let mut character = rows
        .guid()
        .find(character_guid)
        .ok_or("Character is absent")?;
    character.faction_template = FRIENDLY_FIXTURE_FACTION;
    rows.guid().update(character);
    let mut target = rows
        .guid()
        .find(target_guid)
        .ok_or("quest target is absent")?;
    target.faction_template = FRIENDLY_FIXTURE_FACTION;
    rows.guid().update(target);
    Ok(())
}

#[reducer]
pub fn playerbots_quest_fixture_deplete_gameobject(
    ctx: &ReducerContext,
    gameobject_entry: u32,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let rows = ctx.db.game_gameobject();
    let mut row = rows
        .guid()
        .find(gameobject_guid(gameobject_entry))
        .ok_or("GameObject missing")?;
    row.state = 1;
    rows.guid().update(row);
    Ok(())
}

#[reducer]
pub fn playerbots_quest_fixture_move_gameobject(
    ctx: &ReducerContext,
    gameobject_entry: u32,
    x: f32,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let rows = ctx.db.game_gameobject();
    let mut row = rows
        .guid()
        .find(gameobject_guid(gameobject_entry))
        .ok_or("GameObject missing")?;
    row.x = x;
    let (grid_x, grid_y) = lyracore_shared::spatial::grid_cell(row.x, row.y);
    row.grid_x = grid_x;
    row.grid_y = grid_y;
    row.cell = lyracore_shared::spatial::grid_cell_id(grid_x, grid_y);
    rows.guid().update(row);
    Ok(())
}

#[reducer]
pub fn playerbots_quest_fixture_move_creature_spawn(
    ctx: &ReducerContext,
    creature_entry: u32,
    x: f32,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let rows = ctx.db.game_creature_spawn();
    let mut row = rows
        .guid()
        .find(creature_guid(creature_entry))
        .ok_or("creature spawn missing")?;
    row.x = x;
    rows.guid().update(row);
    Ok(())
}

#[reducer]
pub fn playerbots_quest_fixture_assert_no_live_target(
    ctx: &ReducerContext,
    character_guid: u64,
    creature_entry: u32,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let character = crate::helpers::live_entity(ctx, character_guid)?;
    match super::quest_loop::live_creature_target(ctx, &character, creature_entry, |_| true) {
        super::quest_loop::LiveCreatureTarget::Found(_) => {
            Err("live target still present".to_string())
        }
        super::quest_loop::LiveCreatureTarget::Missing => Ok(()),
        super::quest_loop::LiveCreatureTarget::Deferred
        | super::quest_loop::LiveCreatureTarget::Controlled => {
            Err("live target still present".to_string())
        }
        super::quest_loop::LiveCreatureTarget::ReadLimit => {
            Err("live target search reached its read limit".to_string())
        }
    }
}

#[reducer]
pub fn playerbots_quest_fixture_refresh(ctx: &ReducerContext) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    quest_catalog::refresh_catalog(ctx, "unknown");
    Ok(())
}

#[reducer]
pub fn playerbots_quest_fixture_recheck_unchanged(ctx: &ReducerContext) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let headers = ctx.db.pkg_playerbots_quest_catalog();
    let mut header = headers
        .revision()
        .find(CATALOG_REVISION)
        .ok_or("catalog header missing")?;
    header.refresh_after_micros = 0;
    headers.revision().update(header);
    quest_catalog::ensure_catalog(ctx);
    Ok(())
}

/// Keep the existing no-prerequisite path to Quest 33, then fill every inventory slot. The runner
/// must complete Quests 783 and 5261 through the Core before it can reach the loot refusal.
#[reducer]
pub fn playerbots_recovery_fixture_stage_full_bag(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    playerbots_quest_loop_fixture_stage_named(ctx, character_guid)?;
    require_fixture(ctx)?;
    let objectives = ctx.db.pkg_playerbots_catalog_objective();
    let removed: Vec<_> = objectives.by_quest().filter(7u32).take(5).collect();
    if removed.len() > 4 {
        return Err("Quest 7 fixture objectives exceed their read limit".to_string());
    }
    ctx.db
        .pkg_playerbots_catalog_quest()
        .quest_entry()
        .delete(7);
    for row in removed {
        objectives.id().delete(row.id);
    }
    let headers = ctx.db.pkg_playerbots_quest_catalog();
    let mut header = headers
        .revision()
        .find(CATALOG_REVISION)
        .ok_or("quest catalog header missing")?;
    header.quest_count = QUESTS.len().saturating_sub(1) as u32;
    header.refresh_after_micros = i64::MAX;
    headers.revision().update(header);
    let seeds = ctx.db.pkg_playerbots_catalog_seed();
    for class in [1, 5, 8] {
        let mut seed = seeds
            .class()
            .find(class)
            .ok_or("quest catalog class seed missing")?;
        seed.quest_order.retain(|quest| *quest != 7);
        seeds.class().update(seed);
    }
    playerbots_quest_fixture_fill_inventory(ctx, character_guid)
}

#[reducer]
pub fn playerbots_recovery_fixture_clear_one_inventory_slot(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    let items = ctx.db.game_item_instance();
    let filler = items
        .by_owner_guid()
        .filter(character_guid)
        .find(|item| item.entry == INVENTORY_FILLER)
        .ok_or("inventory filler missing")?;
    items.guid().delete(filler.guid);
    if !crate::items::has_free_slot(ctx, character_guid) {
        return Err("one inventory slot did not become available".to_string());
    }
    Ok(())
}

/// Exercise three parked runner passes at one observed time so a one-second retry deadline cannot
/// elapse between calls from the test process.
#[reducer]
pub fn playerbots_recovery_fixture_three_parked_passes(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    require_fixture(ctx)?;
    for _ in 0..3 {
        super::fixture::playerbots_fixture_runner_pass_once(ctx, character_guid)?;
    }
    Ok(())
}

#[reducer]
pub fn playerbots_recovery_fixture_arm_gameobject_respawn(
    ctx: &ReducerContext,
    delay_seconds: u32,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    reject_imported_content(ctx)?;
    if !(2..=10).contains(&delay_seconds) {
        return Err("respawn delay must be 2 through 10 seconds".to_string());
    }
    let rows = ctx.db.game_gameobject();
    let mut gameobject = rows
        .guid()
        .find(gameobject_guid(SEEDED_USE_GAMEOBJECT))
        .ok_or("simple GameObject missing")?;
    gameobject.state = 1;
    gameobject.respawn_at_micros = (ctx.timestamp.to_micros_since_unix_epoch() as u64)
        .saturating_add(u64::from(delay_seconds) * 1_000_000);
    rows.guid().update(gameobject);
    Ok(())
}

#[reducer]
pub fn playerbots_recovery_fixture_position_simple_gameobject(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    reject_imported_content(ctx)?;
    let character = crate::helpers::live_entity(ctx, character_guid)?;
    let rows = ctx.db.game_gameobject();
    let mut gameobject = rows
        .guid()
        .find(gameobject_guid(SEEDED_USE_GAMEOBJECT))
        .ok_or("simple GameObject is absent")?;
    gameobject.x = character.x + 30.0;
    let (grid_x, grid_y) = lyracore_shared::spatial::grid_cell(gameobject.x, gameobject.y);
    gameobject.grid_x = grid_x;
    gameobject.grid_y = grid_y;
    gameobject.cell = lyracore_shared::spatial::grid_cell_id(grid_x, grid_y);
    rows.guid().update(gameobject);
    Ok(())
}

#[reducer]
pub fn playerbots_recovery_fixture_remove_simple_gameobject(
    ctx: &ReducerContext,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    reject_imported_content(ctx)?;
    if ctx
        .db
        .pkg_playerbots_seeded_quest_fixture()
        .quest_entry()
        .find(SEEDED_USE_QUEST)
        .is_none()
    {
        return Err("simple GameObject fixture is absent".to_string());
    }
    let guid = gameobject_guid(SEEDED_USE_GAMEOBJECT);
    let gameobjects = ctx.db.game_gameobject();
    if gameobjects.guid().find(guid).is_none() {
        return Err("simple GameObject is absent".to_string());
    }
    gameobjects.guid().delete(guid);
    Ok(())
}

#[reducer]
pub fn playerbots_recovery_fixture_restore_simple_gameobject(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    reject_imported_content(ctx)?;
    let retained = ctx
        .db
        .pkg_playerbots_quest_objective()
        .character_guid()
        .find(character_guid)
        .filter(|retained| retained.quest_entry == SEEDED_USE_QUEST)
        .ok_or("simple GameObject Quest is not retained")?;
    let destination = retained
        .target
        .source
        .filter(|source| {
            source.kind == CatalogEntityKind::GameObject
                && source.entry == SEEDED_USE_GAMEOBJECT
                && source.guid == gameobject_guid(SEEDED_USE_GAMEOBJECT)
        })
        .ok_or("retained simple GameObject destination differs")?;
    let template = ctx
        .db
        .game_gameobject_template()
        .entry()
        .find(SEEDED_USE_GAMEOBJECT)
        .filter(|template| {
            template.type_id == crate::gameobject::go_type::GOOBER && template.lock_id == 0
        })
        .ok_or("simple GameObject template differs")?;
    let rows = ctx.db.game_gameobject();
    if rows.guid().find(destination.guid).is_some() {
        return Err("simple GameObject is already present".to_string());
    }
    rows.insert(crate::GameObject {
        guid: destination.guid,
        template_entry: template.entry,
        map_id: destination.map_id,
        x: destination.x,
        y: destination.y,
        z: destination.z,
        orientation: 0.0,
        state: 0,
        created_at: ctx.timestamp,
        respawn_at_micros: 0,
        instance_id: destination.instance_id,
        grid_x: lyracore_shared::spatial::grid_cell(destination.x, destination.y).0,
        grid_y: lyracore_shared::spatial::grid_cell(destination.x, destination.y).1,
        cell: lyracore_shared::spatial::cell_id_at(destination.x, destination.y),
        rotation_0: 0.0,
        rotation_1: 0.0,
        rotation_2: 0.0,
        rotation_3: 0.0,
    });
    Ok(())
}

#[reducer]
pub fn playerbots_recovery_fixture_remove_simple_gameobject_and_objective(
    ctx: &ReducerContext,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    reject_imported_content(ctx)?;
    if ctx
        .db
        .game_quest_objective()
        .id()
        .find(SEEDED_USE_OBJECTIVE)
        .is_none()
    {
        return Err("simple GameObject objective is absent".to_string());
    }
    playerbots_recovery_fixture_remove_simple_gameobject(ctx)?;
    ctx.db
        .game_quest_objective()
        .id()
        .delete(SEEDED_USE_OBJECTIVE);
    Ok(())
}

/// Move Quest 783's live ender and spawn before rebuilding its catalog destination, then make the
/// route unreachable. Acceptance remains available at the separate nearby start giver.
#[reducer]
pub fn playerbots_recovery_fixture_stage_unreachable_ender(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    playerbots_quest_fixture_stage(ctx, character_guid)?;
    require_fixture(ctx)?;
    let character = crate::helpers::live_entity(ctx, character_guid)?;
    let ender_guid = creature_guid(197);
    super::fixture::playerbots_fixture_position(ctx, ender_guid, character.x + 40.0)?;
    let spawns = ctx.db.game_creature_spawn();
    let mut spawn = spawns
        .guid()
        .find(ender_guid)
        .ok_or("Quest 783 ender spawn missing")?;
    spawn.x = character.x + 40.0;
    spawns.guid().update(spawn);
    quest_catalog::refresh_catalog(ctx, "unknown");
    block_navigation(ctx, &character)
}
