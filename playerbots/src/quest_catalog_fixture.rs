//! Synthetic private-Shard staging for the catalog and its callable executors.

use super::quest_catalog::{
    self, pkg_playerbots_catalog_objective, pkg_playerbots_quest_catalog, AdmissionRefusal,
    CatalogEntityKind, CatalogObjectiveKind, ObjectiveExecutor, PlayerbotsCatalogObjective,
    CATALOG_REVISION,
};
use crate::{
    game_character_quest, game_corpse_loot, game_creature_loot, game_creature_quest,
    game_creature_spawn, game_creature_template, game_gameobject, game_gameobject_loot,
    game_gameobject_quest, game_gameobject_template, game_item_instance, game_item_template,
    game_quest_objective, game_quest_template, game_world_entity,
};
use spacetimedb::{reducer, ReducerContext, Table};

const FIXTURE_REVISION: &str = "playerbots-synthetic-quest-catalog-v1";
const CHEST_ENTRY: u32 = 161557;
const CHEST_LOOT: u32 = 10119;
const DIRECT_GO_QUEST: u32 = 3904;
const INVENTORY_FILLER: u32 = 51119;

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

fn creature_guid(entry: u32) -> u64 {
    (0xF130u64 << 48) | (u64::from(entry) << 24) | 1
}

fn gameobject_guid(entry: u32) -> u64 {
    (0xF110u64 << 48) | u64::from(entry)
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
    ctx.db.game_creature_template().entry().delete(entry);
    let template = ctx.db.game_creature_template().insert(template);
    let guid = creature_guid(entry);
    crate::creatures::despawn_creature_entity(ctx, guid);
    ctx.db.game_creature_spawn().guid().delete(guid);
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
    x: f32,
    y: f32,
    z: f32,
) {
    ctx.db.game_gameobject_template().entry().delete(entry);
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
    ctx.db.game_gameobject().guid().delete(guid);
    ctx.db.game_gameobject().insert(crate::GameObject {
        guid,
        template_entry: entry,
        map_id: 0,
        x,
        y,
        z,
        orientation: 0.0,
        state: 0,
        created_at: ctx.timestamp,
        respawn_at_micros: 0,
        instance_id: 0,
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
    ctx.db.game_item_template().entry().delete(entry);
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
    ctx.db.game_quest_template().entry().delete(entry);
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
            for row in table
                .by_creature()
                .filter(entry)
                .filter(|row| row.quest_entry == quest_entry && row.role == role)
                .collect::<Vec<_>>()
            {
                table.id().delete(row.id);
            }
            table.insert(crate::CreatureQuest {
                id: 0,
                creature_entry: entry,
                quest_entry,
                role,
            });
        }
        CatalogEntityKind::GameObject => {
            let table = ctx.db.game_gameobject_quest();
            for row in table
                .by_gameobject()
                .filter(entry)
                .filter(|row| row.quest_entry == quest_entry && row.role == role)
                .collect::<Vec<_>>()
            {
                table.id().delete(row.id);
            }
            table.insert(crate::GameObjectQuest {
                id: 0,
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
    let id = (u64::from(quest_entry) << 8) | u64::from(index);
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
        id: 0,
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
    for entry in [750, 752, 11119, 11125] {
        clone_item(ctx, entry, source_item)?;
    }
    for (offset, entry) in [823, 197, 196, 9296, 952, 241, 240, 261, 6, 299, 69, 38]
        .into_iter()
        .enumerate()
    {
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
        character.x + 2.0,
        character.y + 0.7,
        character.z,
    );
    insert_gameobject(
        ctx,
        56,
        crate::gameobject::go_type::QUESTGIVER,
        0,
        character.x + 2.0,
        character.y + 0.8,
        character.z,
    );
    insert_gameobject(
        ctx,
        CHEST_ENTRY,
        crate::gameobject::go_type::CHEST,
        CHEST_LOOT,
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
        creature_loot.insert(crate::CreatureLoot {
            id: 0,
            creature_entry: source,
            item_entry: item,
            chance_bp: 10_000,
            count: if item == 750 { 8 } else { 12 },
            group_id: 0,
            quest_only: true,
        });
    }
    ctx.db.game_gameobject_loot().insert(crate::GameObjectLoot {
        id: 0,
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

#[reducer]
pub fn playerbots_quest_fixture_admit_accept(
    ctx: &ReducerContext,
    character_guid: u64,
    quest_entry: u32,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
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
            super::actions::accept_quest(ctx, character_guid, admission.start.guid, quest_entry)
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
    let target = quest_catalog::live_creature_target(ctx, character_guid, creature_entry)
        .ok_or("no live target")?;
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
    let id = (u64::from(DIRECT_GO_QUEST) << 8) | 0;
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
    super::actions::accept_quest(ctx, character_guid, admission.start.guid, 7).map_err(Into::into)
}

#[reducer]
pub fn playerbots_quest_fixture_unsupported(
    ctx: &ReducerContext,
    character_guid: u64,
    unsupported_kind: u8,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
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
    let alternative = quest_catalog::admit_available(ctx, character_guid, 5261).map_err(
        |refusal| match refusal {
            AdmissionRefusal::Ineligible(detail) | AdmissionRefusal::Unsupported { detail, .. } => {
                detail
            }
        },
    )?;
    super::actions::accept_quest(ctx, character_guid, alternative.start.guid, 5261)
        .map_err(String::from)?;
    let catalog = ctx.db.pkg_playerbots_catalog_objective();
    let id = u64::from(7u32) << 8;
    let previous = catalog.id().find(id).ok_or("catalog objective missing")?;
    catalog.id().update(PlayerbotsCatalogObjective {
        kind: CatalogObjectiveKind::Escort,
        ..previous
    });
    let selected = quest_catalog::reconcile_active(ctx, character_guid)
        .ok_or("supported held alternative missing")?;
    if selected.quest_entry != 5261 {
        return Err(format!(
            "expected quest 5261 after quest 7 became unsupported, got {}",
            selected.quest_entry
        ));
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
    if quest_catalog::reconcile_active(ctx, character_guid).is_some() {
        return Err("held quest remained admitted without its provided item".to_string());
    }
    Ok(())
}

#[reducer]
pub fn playerbots_quest_fixture_hide_live_target(
    ctx: &ReducerContext,
    creature_entry: u32,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    crate::creatures::despawn_creature_entity(ctx, creature_guid(creature_entry));
    Ok(())
}

#[reducer]
pub fn playerbots_quest_fixture_deplete_gameobject(
    ctx: &ReducerContext,
    gameobject_entry: u32,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
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
    if quest_catalog::live_creature_target(ctx, character_guid, creature_entry).is_some() {
        Err("live target still present".to_string())
    } else {
        Ok(())
    }
}

#[reducer]
pub fn playerbots_quest_fixture_refresh(ctx: &ReducerContext) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    quest_catalog::refresh_catalog(ctx, "unknown");
    Ok(())
}

#[reducer]
pub fn playerbots_quest_fixture_recheck_unchanged(ctx: &ReducerContext) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
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
