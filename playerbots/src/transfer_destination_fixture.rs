//! Private destination content for retained Quest Transfer cases.

use super::decision::{Action, Reason};
use super::quest_catalog::{
    pkg_playerbots_catalog_objective, pkg_playerbots_catalog_quest, pkg_playerbots_quest_catalog,
    pkg_playerbots_quest_objective, CatalogDestination, CatalogEntityKind, CatalogObjectiveKind,
    CatalogWorkArea, ObjectiveExecutor, PlayerbotsCatalogObjective, PlayerbotsCatalogQuest,
    PlayerbotsQuestCatalog, CATALOG_BLUEPRINT_REVISION, CATALOG_NAME, CATALOG_REVISION,
};
use super::runner::{pkg_playerbots_runner, ObjectiveKind};
use crate::import_meta::game_import_meta; // package-api: exempt private fixture refuses imported content before staging
use crate::nav::game_navigation_revision; // package-api: exempt private fixture requires Navigation Inputs staged by the real import reducer
use crate::{
    game_character, game_character_quest, game_creature_quest, game_creature_spawn,
    game_creature_template, game_quest_objective, game_quest_template, game_world_entity,
};
use spacetimedb::{reducer, ReducerContext, Table};

const REBUILT_CONTENT: &str = "playerbots-transfer-destination-q7-v1";
const REPLACEMENT_CONTENT: &str = "playerbots-transfer-destination-q5261-v1";
const REFERENCE_SOURCE: &str = "playerbots-transfer-destination-private-v1";
const RELATION_BASE: u64 = 5_100_100;
const OBJECTIVE_ID: u64 = 5_100_120;
const DESTINATION_MAP: u32 = 36;
const DESTINATION_INSTANCE: u64 = 5_098_078;
const DESTINATION_LANDING: (f32, f32, f32) = (-14.5732, -385.475, 62.4561);

fn destination_guid(entry: u32, ordinal: u64) -> u64 {
    (0xF130u64 << 48) | (u64::from(entry) << 24) | (10_000 + ordinal)
}

fn require_private_fixture(ctx: &ReducerContext) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    if ctx.db.game_import_meta().iter().next().is_some() {
        return Err("destination catalogue fixture refuses imported content".to_string());
    }
    Ok(())
}

fn prepare_private_fixture(ctx: &ReducerContext) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let imports = ctx.db.game_import_meta();
    let rows: Vec<_> = imports.iter().take(2).collect();
    match rows.as_slice() {
        [] => {}
        [seed]
            if seed.family == "weather_seed"
                && seed.source_sha.is_empty()
                && seed.file_hash.is_empty()
                && seed.row_count == 2 =>
        {
            // A fresh Module stamps this temporary Core seed. Remove only the exact bootstrap
            // row, as the other Quest harnesses do, before the unchanged imported-content Gate.
            imports.family().delete(seed.family.clone());
        }
        _ => return Err("destination catalogue fixture refuses imported content".to_string()),
    }
    require_private_fixture(ctx)
}

fn quest_template(entry: u32, prerequisite: u32) -> crate::QuestTemplate {
    crate::QuestTemplate {
        entry,
        min_level: 1,
        quest_level: 1,
        title: format!("Transfer destination quest {entry}"),
        reward_money: 1,
        reward_xp: 1,
        prev_quest_id: prerequisite,
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
    }
}

fn stage_creature(
    ctx: &ReducerContext,
    entry: u32,
    guid: u64,
    map_id: u32,
    instance_id: u64,
    at: (f32, f32, f32),
    hostile: bool,
) -> Result<CatalogDestination, String> {
    let mut template = ctx
        .db
        .game_creature_template()
        .entry()
        .find(51_000)
        .ok_or("destination catalogue seed creature template missing")?;
    template.entry = entry;
    template.name = format!("Transfer destination creature {entry}");
    template.faction_template = if hostile { 14 } else { 35 };
    template.aggro_range = 0;
    let template = ctx.db.game_creature_template().insert(template);
    let spawn = crate::CreatureSpawn {
        guid,
        entry,
        map_id,
        x: at.0,
        y: at.1,
        z: at.2,
        orientation: 0.0,
        respawn_at: crate::creatures::timer_never(ctx),
        despawn_at: crate::creatures::timer_never(ctx),
        movement_type: 0,
        respawn_secs: 60,
        life_seq: 1,
    };
    let mut entity = crate::creatures::build_creature_entity(&spawn, &template, 0, instance_id);
    ctx.db.game_creature_spawn().insert(spawn);
    entity.map_id = map_id;
    entity.instance_id = instance_id;
    crate::creatures::insert_creature_entity(ctx, entity);
    Ok(CatalogDestination {
        kind: CatalogEntityKind::Creature,
        entry,
        guid,
        map_id,
        instance_id,
        x: at.0,
        y: at.1,
        z: at.2,
    })
}

fn insert_relation(ctx: &ReducerContext, id: u64, creature_entry: u32, quest_entry: u32, role: u8) {
    ctx.db.game_creature_quest().insert(crate::CreatureQuest {
        id,
        creature_entry,
        quest_entry,
        role,
    });
}

fn insert_header(ctx: &ReducerContext, content_revision: &str) {
    ctx.db
        .pkg_playerbots_quest_catalog()
        .insert(PlayerbotsQuestCatalog {
            revision: CATALOG_REVISION,
            name: CATALOG_NAME.to_string(),
            blueprint_revision: CATALOG_BLUEPRINT_REVISION.to_string(),
            reference_source_revision: REFERENCE_SOURCE.to_string(),
            content_revision: content_revision.to_string(),
            quest_count: 1,
            refresh_after_micros: i64::MAX,
        });
}

fn preflight_destination(
    ctx: &ReducerContext,
    quest_entry: u32,
    entries: &[u32],
    guids: &[u64],
) -> Result<(), String> {
    if ctx
        .db
        .pkg_playerbots_quest_catalog()
        .revision()
        .find(CATALOG_REVISION)
        .is_some()
    {
        return Err("destination catalogue fixture state is occupied".to_string());
    }
    if ctx.db.game_navigation_revision().id().find(0).is_none() {
        return Err("destination Navigation Inputs were not imported".to_string());
    }
    if ctx
        .db
        .game_quest_template()
        .entry()
        .find(quest_entry)
        .is_some()
        || ctx
            .db
            .pkg_playerbots_catalog_quest()
            .quest_entry()
            .find(quest_entry)
            .is_some()
        || ctx
            .db
            .game_quest_objective()
            .by_quest()
            .filter(quest_entry)
            .next()
            .is_some()
        || ctx
            .db
            .pkg_playerbots_catalog_objective()
            .by_quest()
            .filter(quest_entry)
            .next()
            .is_some()
    {
        return Err(format!(
            "destination catalogue fixture quest {quest_entry} is occupied"
        ));
    }
    for entry in entries {
        if ctx
            .db
            .game_creature_template()
            .entry()
            .find(*entry)
            .is_some()
        {
            return Err(format!(
                "destination catalogue fixture entry {entry} is occupied"
            ));
        }
    }
    for guid in guids {
        if ctx.db.game_creature_spawn().guid().find(*guid).is_some()
            || ctx.db.game_world_entity().guid().find(*guid).is_some()
        {
            return Err(format!(
                "destination catalogue fixture creature {guid} is occupied"
            ));
        }
    }
    for id in RELATION_BASE..RELATION_BASE + 2 {
        if ctx.db.game_creature_quest().id().find(id).is_some() {
            return Err(format!(
                "destination catalogue fixture relation {id} is occupied"
            ));
        }
    }
    if ctx
        .db
        .game_quest_objective()
        .id()
        .find(OBJECTIVE_ID)
        .is_some()
        || ctx
            .db
            .pkg_playerbots_catalog_objective()
            .id()
            .find(OBJECTIVE_ID)
            .is_some()
    {
        return Err("destination catalogue fixture objective is occupied".to_string());
    }
    Ok(())
}

/// Stage a real active quest and one source-local decision before the Gateway begins Transfer.
#[reducer]
pub fn playerbots_transfer_quest_source_stage(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    prepare_private_fixture(ctx)?;
    if ctx.db.game_navigation_revision().id().find(0).is_none() {
        return Err("source Navigation Inputs were not imported".to_string());
    }
    super::quest_catalog_fixture::playerbots_quest_loop_fixture_stage_named(ctx, character_guid)?;
    super::quest_catalog_fixture::playerbots_quest_fixture_admit_accept(ctx, character_guid, 7)?;
    super::fixture::playerbots_fixture_runner_select_cohort(ctx, character_guid)?;
    super::fixture::playerbots_fixture_provision_steps(ctx, character_guid, 32)?;
    super::fixture::playerbots_fixture_runner_pass_once(ctx, character_guid)?;
    super::quest_catalog_fixture::playerbots_recovery_fixture_exhaust_attempt(ctx, character_guid)
}

/// Execute the real selected portal operation once after the caller rejoins the retained Quest
/// Character to its authenticated party. Case 12 separately covers automatic order selection.
#[reducer]
pub fn playerbots_transfer_quest_execute(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<(), String> {
    require_private_fixture(ctx)?;
    let mut state = ctx
        .db
        .pkg_playerbots_runner()
        .character_guid()
        .find(character_guid)
        .ok_or("source Quest runner is absent")?;
    let objective_identity = state
        .objective
        .as_ref()
        .filter(|objective| objective.kind == ObjectiveKind::Quest)
        .map(|objective| objective.identity)
        .ok_or("source Quest objective is absent")?;
    let retained = ctx
        .db
        .pkg_playerbots_quest_objective()
        .character_guid()
        .find(character_guid)
        .filter(|retained| retained.runner_objective_identity == objective_identity)
        .ok_or("source retained Quest identity changed")?;
    if !ctx
        .db
        .game_character_quest()
        .by_character_quest()
        .filter((character_guid, retained.quest_entry))
        .any(|quest| !quest.rewarded && !quest.failed)
    {
        return Err("source retained Quest identity changed".to_string());
    }
    let me = ctx
        .db
        .game_world_entity()
        .guid()
        .find(character_guid)
        .ok_or("source Quest body is absent")?;
    let party = super::companion::human_led_party(ctx, character_guid)
        .map_err(|error| format!("source party facts unavailable: {error:?}"))?
        .ok_or("source Quest party is absent")?;
    let order = super::orders::active(ctx, character_guid);
    let member_guid = super::transfer::companion_member(order.as_ref(), Some(party.leader_guid))
        .ok_or("source Quest order disables automatic Transfer")?;
    let partition = party
        .members
        .iter()
        .find(|member| member.character_guid == member_guid)
        .and_then(|member| member.partition)
        .ok_or("source Quest destination partition is absent")?;
    let node = super::transfer::candidate(ctx, &me, partition, objective_identity);
    let Action::Transfer(action) = node.candidate.id.action else {
        return Err("source Quest did not resolve a Transfer action".to_string());
    };
    if node.candidate.id.reason != Reason::Transfer
        || !node.prerequisites.is_empty()
        || action.trigger != 78
        || action.destination_map != DESTINATION_MAP
        || action.destination_instance != DESTINATION_INSTANCE
    {
        return Err("source Quest resolved another portal operation".to_string());
    }
    super::transfer::execute(
        ctx,
        &mut state,
        &me,
        action,
        ctx.timestamp.to_micros_since_unix_epoch(),
    )
    .map_err(|failure| format!("source Quest Transfer refused: {failure:?}"))?;
    state.save(ctx);
    Ok(())
}

/// Stage destination-only catalogue after the caller imports Navigation Inputs and before the
/// Gateway begins Transfer. Mode 1 retains quest 7 with new local evidence. Mode 2 offers quest
/// 5261 as a safe replacement.
#[reducer]
pub fn playerbots_transfer_destination_catalogue_stage(
    ctx: &ReducerContext,
    character_guid: u64,
    mode: u8,
) -> Result<(), String> {
    prepare_private_fixture(ctx)?;
    if ctx
        .db
        .game_character()
        .guid()
        .find(character_guid)
        .is_some()
        || ctx
            .db
            .game_world_entity()
            .guid()
            .find(character_guid)
            .is_some()
        || ctx
            .db
            .game_character_quest()
            .by_character()
            .filter(character_guid)
            .next()
            .is_some()
        || ctx
            .db
            .pkg_playerbots_runner()
            .character_guid()
            .find(character_guid)
            .is_some()
    {
        return Err(
            "destination catalogue fixture must run before the Character arrives".to_string(),
        );
    }

    let (quest_entry, creatures, content_revision) = match mode {
        1 => (
            7,
            vec![(197, destination_guid(197, 1)), (6, destination_guid(6, 2))],
            REBUILT_CONTENT,
        ),
        2 => (
            5_261,
            vec![
                (823, destination_guid(823, 3)),
                (196, destination_guid(196, 4)),
            ],
            REPLACEMENT_CONTENT,
        ),
        _ => return Err("unknown destination catalogue fixture mode".to_string()),
    };
    let entries: Vec<_> = creatures.iter().map(|(entry, _)| *entry).collect();
    let guids: Vec<_> = creatures.iter().map(|(_, guid)| *guid).collect();
    preflight_destination(ctx, quest_entry, &entries, &guids)?;

    insert_header(ctx, content_revision);
    ctx.db
        .game_quest_template()
        .insert(quest_template(quest_entry, 783));

    let first = stage_creature(
        ctx,
        creatures[0].0,
        creatures[0].1,
        DESTINATION_MAP,
        DESTINATION_INSTANCE,
        (
            DESTINATION_LANDING.0 + 2.0,
            DESTINATION_LANDING.1,
            DESTINATION_LANDING.2,
        ),
        false,
    )?;
    let second = stage_creature(
        ctx,
        creatures[1].0,
        creatures[1].1,
        DESTINATION_MAP,
        DESTINATION_INSTANCE,
        (
            DESTINATION_LANDING.0 + 18.0,
            DESTINATION_LANDING.1 + 3.0,
            DESTINATION_LANDING.2,
        ),
        mode == 1,
    )?;
    insert_relation(
        ctx,
        RELATION_BASE,
        first.entry,
        quest_entry,
        crate::quest::quest_role::START,
    );
    insert_relation(
        ctx,
        RELATION_BASE + 1,
        if mode == 1 { first.entry } else { second.entry },
        quest_entry,
        crate::quest::quest_role::END,
    );

    let (actual_ender, objective) = if mode == 1 {
        ctx.db.game_quest_objective().insert(crate::QuestObjective {
            id: OBJECTIVE_ID,
            quest_entry,
            obj_index: 0,
            kind: crate::quest::objective_kind::KILL_CREATURE,
            target_entry: 6,
            required_count: 10,
        });
        let objective = PlayerbotsCatalogObjective {
            id: OBJECTIVE_ID,
            quest_entry,
            objective_index: 0,
            kind: CatalogObjectiveKind::KillCreature,
            target_entry: 6,
            required_count: 10,
            executor: ObjectiveExecutor::Attack,
            source_kind: Some(CatalogEntityKind::Creature),
            source_entries: vec![6],
            source_destinations: vec![second.clone()],
            work_area: Some(CatalogWorkArea {
                map_id: second.map_id,
                instance_id: second.instance_id,
                min_x: second.x,
                max_x: second.x,
                min_y: second.y,
                max_y: second.y,
            }),
            destination_evidence_revision: content_revision.to_string(),
            catalog_revision: CATALOG_REVISION,
        };
        (first.clone(), objective)
    } else {
        let objective = PlayerbotsCatalogObjective {
            id: OBJECTIVE_ID,
            quest_entry,
            objective_index: 0,
            kind: CatalogObjectiveKind::TalkOnly,
            target_entry: 0,
            required_count: 0,
            executor: ObjectiveExecutor::Talk,
            source_kind: None,
            source_entries: Vec::new(),
            source_destinations: vec![second.clone()],
            work_area: None,
            destination_evidence_revision: content_revision.to_string(),
            catalog_revision: CATALOG_REVISION,
        };
        (second.clone(), objective)
    };
    ctx.db.pkg_playerbots_catalog_objective().insert(objective);
    ctx.db
        .pkg_playerbots_catalog_quest()
        .insert(PlayerbotsCatalogQuest {
            quest_entry,
            catalog_revision: CATALOG_REVISION,
            catalog_order: 0,
            min_level: 1,
            required_races: 77,
            required_classes: 0,
            prerequisite_quest: 783,
            start_kind: CatalogEntityKind::Creature,
            start_entry: first.entry,
            start_destinations: vec![first],
            actual_ender_kind: CatalogEntityKind::Creature,
            actual_ender_entry: actual_ender.entry,
            actual_ender_destinations: vec![actual_ender],
            content_revision: content_revision.to_string(),
        });
    Ok(())
}
