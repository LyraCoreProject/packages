//! A versioned catalog of quest work this Package can execute with current core operations.

use crate::{
    game_character_quest, game_creature_loot, game_creature_quest, game_gameobject_loot,
    game_gameobject_quest, game_gameobject_template, game_item_template, game_quest_cast_objective,
    game_quest_event_requirement, game_quest_objective, game_quest_template, game_world_entity,
};
use spacetimedb::{table, ReducerContext, Table};

pub const CATALOG_NAME: &str = "northshire-elwynn-supported-v1";
pub const CATALOG_REVISION: u64 = 1;
pub const CATALOG_BLUEPRINT_REVISION: &str =
    "classicdb:d2083bcd2670451279cbf93af138eadae04c6d183a4cd0ff0357047e4a565de6";
const DESTINATION_LIMIT: usize = 128;
const WAIT_MICROS: i64 = 30_000_000;
const REFRESH_INTERVAL_MICROS: i64 = 30_000_000;

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CatalogEntityKind {
    Creature,
    GameObject,
}

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogObjectiveKind {
    TalkOnly,
    KillCreature,
    CollectItem,
    UseGameObject,
    ExploreAreaTrigger,
    Escort,
    ScriptedEvent,
    Transport,
    ComplexGameObject,
}

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectiveExecutor {
    Talk,
    Attack,
    CreatureLoot,
    GameObjectLoot,
    ProvidedItem,
    SimpleGameObject,
}

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingCapability {
    MissingCatalog,
    MissingQuestTemplate,
    ChangedEligibility,
    MissingStartRelation,
    MissingStartDestination,
    MissingActualEndRelation,
    MissingActualEndDestination,
    ObjectiveMismatch,
    MissingSourceDestination,
    MissingItemTemplate,
    MissingLootSource,
    CastObjective,
    Exploration,
    Escort,
    ScriptedEvent,
    Transport,
    ComplexGameObject,
    UnknownObjective,
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug, PartialEq)]
pub struct CatalogDestination {
    pub kind: CatalogEntityKind,
    pub entry: u32,
    pub guid: u64,
    pub map_id: u32,
    pub instance_id: u64,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug, PartialEq)]
pub struct CatalogWorkArea {
    pub map_id: u32,
    pub instance_id: u64,
    pub min_x: f32,
    pub max_x: f32,
    pub min_y: f32,
    pub max_y: f32,
}

#[table(accessor = pkg_playerbots_quest_catalog, public)]
pub struct PlayerbotsQuestCatalog {
    #[primary_key]
    pub revision: u64,
    pub name: String,
    pub blueprint_revision: String,
    pub reference_source_revision: String,
    pub content_revision: String,
    pub quest_count: u32,
    pub refresh_after_micros: i64,
}

#[table(accessor = pkg_playerbots_catalog_seed, public)]
pub struct PlayerbotsCatalogSeed {
    #[primary_key]
    pub class: u8,
    pub fixture_seed: u64,
    pub catalog_revision: u64,
    pub quest_order: Vec<u32>,
}

#[table(
    accessor = pkg_playerbots_catalog_quest,
    public,
    index(accessor = by_order, btree(columns = [catalog_revision, catalog_order]))
)]
pub struct PlayerbotsCatalogQuest {
    #[primary_key]
    pub quest_entry: u32,
    pub catalog_revision: u64,
    pub catalog_order: u16,
    pub min_level: u32,
    pub required_races: u32,
    pub required_classes: u32,
    pub prerequisite_quest: u32,
    pub start_kind: CatalogEntityKind,
    pub start_entry: u32,
    pub start_destinations: Vec<CatalogDestination>,
    pub actual_ender_kind: CatalogEntityKind,
    pub actual_ender_entry: u32,
    pub actual_ender_destinations: Vec<CatalogDestination>,
    pub content_revision: String,
}

#[table(
    accessor = pkg_playerbots_catalog_objective,
    public,
    index(accessor = by_quest, btree(columns = [quest_entry]))
)]
pub struct PlayerbotsCatalogObjective {
    #[primary_key]
    pub id: u64,
    pub quest_entry: u32,
    pub objective_index: u8,
    pub kind: CatalogObjectiveKind,
    pub target_entry: u32,
    pub required_count: u32,
    pub executor: ObjectiveExecutor,
    pub source_kind: Option<CatalogEntityKind>,
    pub source_entries: Vec<u32>,
    pub source_destinations: Vec<CatalogDestination>,
    pub work_area: Option<CatalogWorkArea>,
    pub destination_evidence_revision: String,
    pub catalog_revision: u64,
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug, PartialEq)]
pub struct RetainedQuestTarget {
    pub objective_index: u8,
    pub kind: CatalogObjectiveKind,
    pub target_entry: u32,
    pub required_count: u32,
    pub executor: ObjectiveExecutor,
    pub source: Option<CatalogDestination>,
}

#[table(accessor = pkg_playerbots_quest_objective, public)]
pub struct PlayerbotsQuestObjective {
    #[primary_key]
    pub character_guid: u64,
    pub runner_objective_identity: u64,
    pub quest_entry: u32,
    pub target: RetainedQuestTarget,
    pub actual_ender_kind: CatalogEntityKind,
    pub actual_ender_entry: u32,
    pub actual_ender: CatalogDestination,
    pub destination: CatalogDestination,
    pub destination_evidence_revision: String,
    pub catalog_revision: u64,
    pub reference_source_revision: String,
    pub content_revision: String,
}

crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_quest_objective(ctx, character_guid) {
    ctx.db.pkg_playerbots_quest_objective().character_guid().delete(character_guid);
});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_playerbots_quest_objective());

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuestAdmissionState {
    Admitted,
    Ineligible,
    Unsupported,
    Waiting,
}

#[table(accessor = pkg_playerbots_quest_admission, public)]
pub struct PlayerbotsQuestAdmission {
    #[primary_key]
    pub character_guid: u64,
    pub considered_quest: u32,
    pub selected_quest: Option<u32>,
    pub state: QuestAdmissionState,
    pub missing_capability: Option<MissingCapability>,
    pub detail: String,
    pub catalog_revision: u64,
    pub observed_micros: i64,
    pub wait_until_micros: i64,
}

crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_quest_admission(ctx, character_guid) {
    ctx.db.pkg_playerbots_quest_admission().character_guid().delete(character_guid);
});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_playerbots_quest_admission());

#[derive(Clone, Copy)]
struct EntityDefinition {
    kind: CatalogEntityKind,
    entry: u32,
}

#[derive(Clone, Copy)]
struct ObjectiveDefinition {
    index: u8,
    kind: CatalogObjectiveKind,
    target_entry: u32,
    required_count: u32,
    executor: ObjectiveExecutor,
    source_kind: Option<CatalogEntityKind>,
    source_entries: &'static [u32],
}

#[derive(Clone, Copy)]
struct QuestDefinition {
    entry: u32,
    min_level: u32,
    required_races: u32,
    required_classes: u32,
    prerequisite_quest: u32,
    start: EntityDefinition,
    actual_ender: EntityDefinition,
    objectives: &'static [ObjectiveDefinition],
}

const fn creature(entry: u32) -> EntityDefinition {
    EntityDefinition {
        kind: CatalogEntityKind::Creature,
        entry,
    }
}

const fn gameobject(entry: u32) -> EntityDefinition {
    EntityDefinition {
        kind: CatalogEntityKind::GameObject,
        entry,
    }
}

const TALK: &[ObjectiveDefinition] = &[ObjectiveDefinition {
    index: 0,
    kind: CatalogObjectiveKind::TalkOnly,
    target_entry: 0,
    required_count: 0,
    executor: ObjectiveExecutor::Talk,
    source_kind: None,
    source_entries: &[],
}];
const Q7: &[ObjectiveDefinition] = &[ObjectiveDefinition {
    index: 0,
    kind: CatalogObjectiveKind::KillCreature,
    target_entry: 6,
    required_count: 10,
    executor: ObjectiveExecutor::Attack,
    source_kind: Some(CatalogEntityKind::Creature),
    source_entries: &[6],
}];
const Q33: &[ObjectiveDefinition] = &[ObjectiveDefinition {
    index: 0,
    kind: CatalogObjectiveKind::CollectItem,
    target_entry: 750,
    required_count: 8,
    executor: ObjectiveExecutor::CreatureLoot,
    source_kind: Some(CatalogEntityKind::Creature),
    source_entries: &[299, 69],
}];
const Q18: &[ObjectiveDefinition] = &[ObjectiveDefinition {
    index: 0,
    kind: CatalogObjectiveKind::CollectItem,
    target_entry: 752,
    required_count: 12,
    executor: ObjectiveExecutor::CreatureLoot,
    source_kind: Some(CatalogEntityKind::Creature),
    source_entries: &[38],
}];
const Q3904: &[ObjectiveDefinition] = &[ObjectiveDefinition {
    index: 0,
    kind: CatalogObjectiveKind::CollectItem,
    target_entry: 11119,
    required_count: 8,
    executor: ObjectiveExecutor::GameObjectLoot,
    source_kind: Some(CatalogEntityKind::GameObject),
    source_entries: &[161557],
}];
const Q3905: &[ObjectiveDefinition] = &[ObjectiveDefinition {
    index: 0,
    kind: CatalogObjectiveKind::CollectItem,
    target_entry: 11125,
    required_count: 1,
    executor: ObjectiveExecutor::ProvidedItem,
    source_kind: None,
    source_entries: &[],
}];

const QUESTS: &[QuestDefinition] = &[
    QuestDefinition {
        entry: 783,
        min_level: 1,
        required_races: 77,
        required_classes: 0,
        prerequisite_quest: 0,
        start: creature(823),
        actual_ender: creature(197),
        objectives: TALK,
    },
    QuestDefinition {
        entry: 7,
        min_level: 1,
        required_races: 77,
        required_classes: 0,
        prerequisite_quest: 783,
        start: creature(197),
        actual_ender: creature(197),
        objectives: Q7,
    },
    QuestDefinition {
        entry: 5261,
        min_level: 1,
        required_races: 77,
        required_classes: 0,
        prerequisite_quest: 783,
        start: creature(823),
        actual_ender: creature(196),
        objectives: TALK,
    },
    QuestDefinition {
        entry: 33,
        min_level: 1,
        required_races: 77,
        required_classes: 0,
        prerequisite_quest: 5261,
        start: creature(196),
        actual_ender: creature(196),
        objectives: Q33,
    },
    QuestDefinition {
        entry: 18,
        min_level: 2,
        required_races: 77,
        required_classes: 0,
        prerequisite_quest: 783,
        start: creature(823),
        actual_ender: creature(823),
        objectives: Q18,
    },
    QuestDefinition {
        entry: 3903,
        min_level: 2,
        required_races: 77,
        required_classes: 0,
        prerequisite_quest: 33,
        start: creature(823),
        actual_ender: creature(9296),
        objectives: TALK,
    },
    QuestDefinition {
        entry: 3904,
        min_level: 2,
        required_races: 77,
        required_classes: 0,
        prerequisite_quest: 3903,
        start: creature(9296),
        actual_ender: creature(9296),
        objectives: Q3904,
    },
    QuestDefinition {
        entry: 3905,
        min_level: 2,
        required_races: 77,
        required_classes: 0,
        prerequisite_quest: 3904,
        start: creature(9296),
        actual_ender: creature(952),
        objectives: Q3905,
    },
    QuestDefinition {
        entry: 40,
        min_level: 1,
        required_races: 77,
        required_classes: 0,
        prerequisite_quest: 0,
        start: creature(241),
        actual_ender: creature(240),
        objectives: TALK,
    },
    QuestDefinition {
        entry: 35,
        min_level: 1,
        required_races: 77,
        required_classes: 0,
        prerequisite_quest: 40,
        start: creature(240),
        actual_ender: creature(261),
        objectives: TALK,
    },
    QuestDefinition {
        entry: 37,
        min_level: 1,
        required_races: 77,
        required_classes: 0,
        prerequisite_quest: 35,
        start: creature(261),
        actual_ender: gameobject(55),
        objectives: TALK,
    },
    QuestDefinition {
        entry: 45,
        min_level: 1,
        required_races: 77,
        required_classes: 0,
        prerequisite_quest: 37,
        start: gameobject(55),
        actual_ender: gameobject(56),
        objectives: TALK,
    },
];

fn objective_id(quest_entry: u32, objective_index: u8) -> u64 {
    (u64::from(quest_entry) << 8) | u64::from(objective_index)
}

fn destinations(
    ctx: &ReducerContext,
    entity: EntityDefinition,
    map_id: u32,
) -> Vec<CatalogDestination> {
    let mut found: Vec<_> = match entity.kind {
        CatalogEntityKind::Creature => {
            crate::creatures::creature_spawn_evidence(ctx, entity.entry, map_id, DESTINATION_LIMIT)
                .into_iter()
                .map(|spawn| CatalogDestination {
                    kind: entity.kind,
                    entry: entity.entry,
                    guid: spawn.guid,
                    map_id: spawn.map_id,
                    instance_id: 0,
                    x: spawn.x,
                    y: spawn.y,
                    z: spawn.z,
                })
                .collect()
        }
        CatalogEntityKind::GameObject => crate::gameobject::gameobject_destination_evidence(
            ctx,
            entity.entry,
            map_id,
            0,
            DESTINATION_LIMIT,
        )
        .into_iter()
        .map(|spawn| CatalogDestination {
            kind: entity.kind,
            entry: entity.entry,
            guid: spawn.guid,
            map_id: spawn.map_id,
            instance_id: spawn.instance_id,
            x: spawn.x,
            y: spawn.y,
            z: spawn.z,
        })
        .collect(),
    };
    found.sort_by_key(|destination| destination.guid);
    found
}

fn source_destinations(
    ctx: &ReducerContext,
    source_kind: Option<CatalogEntityKind>,
    entries: &[u32],
) -> Vec<CatalogDestination> {
    let Some(kind) = source_kind else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries {
        found.extend(destinations(
            ctx,
            EntityDefinition {
                kind,
                entry: *entry,
            },
            0,
        ));
    }
    found.sort_by_key(|destination| destination.guid);
    found
}

fn work_area(destinations: &[CatalogDestination]) -> Option<CatalogWorkArea> {
    let first = destinations.first()?;
    let mut area = CatalogWorkArea {
        map_id: first.map_id,
        instance_id: first.instance_id,
        min_x: first.x,
        max_x: first.x,
        min_y: first.y,
        max_y: first.y,
    };
    for destination in destinations.iter().skip(1).filter(|destination| {
        (destination.map_id, destination.instance_id) == (area.map_id, area.instance_id)
    }) {
        area.min_x = area.min_x.min(destination.x);
        area.max_x = area.max_x.max(destination.x);
        area.min_y = area.min_y.min(destination.y);
        area.max_y = area.max_y.max(destination.y);
    }
    Some(area)
}

fn hash_u64(hasher: &mut blake3::Hasher, value: u64) {
    hasher.update(&value.to_le_bytes());
}

fn hash_destination(hasher: &mut blake3::Hasher, destination: &CatalogDestination) {
    hasher.update(&[destination.kind as u8]);
    hash_u64(hasher, u64::from(destination.entry));
    hash_u64(hasher, destination.guid);
    hash_u64(hasher, u64::from(destination.map_id));
    hash_u64(hasher, destination.instance_id);
    hash_u64(hasher, u64::from(destination.x.to_bits()));
    hash_u64(hasher, u64::from(destination.y.to_bits()));
    hash_u64(hasher, u64::from(destination.z.to_bits()));
}

fn observed_content_revision(ctx: &ReducerContext) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"playerbots-observed-catalog-v1");
    for definition in QUESTS.iter().copied() {
        for value in [
            definition.entry,
            definition.min_level,
            definition.required_races,
            definition.required_classes,
            definition.prerequisite_quest,
            definition.start.entry,
            definition.actual_ender.entry,
        ] {
            hash_u64(&mut hasher, u64::from(value));
        }
        hasher.update(&[
            definition.start.kind as u8,
            definition.actual_ender.kind as u8,
        ]);
        if let Some(template) = ctx.db.game_quest_template().entry().find(definition.entry) {
            hasher.update(&[1]);
            for value in [
                template.entry,
                template.min_level,
                template.required_races,
                template.required_classes,
                template.prev_quest_id,
                template.src_item,
                template.src_item_count,
            ] {
                hash_u64(&mut hasher, u64::from(value));
            }
        } else {
            hasher.update(&[0]);
        }
        hasher.update(&[
            relation_exists(
                ctx,
                definition.start.kind,
                definition.start.entry,
                definition.entry,
                crate::quest::quest_role::START,
            ) as u8,
            relation_exists(
                ctx,
                definition.actual_ender.kind,
                definition.actual_ender.entry,
                definition.entry,
                crate::quest::quest_role::END,
            ) as u8,
        ]);
        let mut entity_destinations = destinations(ctx, definition.start, 0);
        entity_destinations.extend(destinations(ctx, definition.actual_ender, 0));
        for destination in &entity_destinations {
            hash_destination(&mut hasher, destination);
        }
        let mut core: Vec<_> = ctx
            .db
            .game_quest_objective()
            .by_quest()
            .filter(definition.entry)
            .collect();
        core.sort_by_key(|objective| objective.obj_index);
        for objective in core {
            hasher.update(&[objective.obj_index, objective.kind]);
            hash_u64(&mut hasher, u64::from(objective.target_entry));
            hash_u64(&mut hasher, u64::from(objective.required_count));
        }
        for objective in definition.objectives.iter().copied() {
            hasher.update(&[
                objective.index,
                objective.kind as u8,
                objective.executor as u8,
                objective.source_kind.map_or(u8::MAX, |kind| kind as u8),
            ]);
            hash_u64(&mut hasher, u64::from(objective.target_entry));
            hash_u64(&mut hasher, u64::from(objective.required_count));
            if let Some(item) = ctx
                .db
                .game_item_template()
                .entry()
                .find(objective.target_entry)
            {
                hasher.update(&[1]);
                hash_u64(&mut hasher, u64::from(item.entry));
                hash_u64(&mut hasher, u64::from(item.max_stack));
                hash_u64(&mut hasher, u64::from(item.max_count));
            } else if objective.kind == CatalogObjectiveKind::CollectItem {
                hasher.update(&[0]);
            }
            for entry in objective.source_entries {
                hash_u64(&mut hasher, u64::from(*entry));
                for destination in destinations(
                    ctx,
                    EntityDefinition {
                        kind: objective.source_kind.unwrap_or(CatalogEntityKind::Creature),
                        entry: *entry,
                    },
                    0,
                ) {
                    hash_destination(&mut hasher, &destination);
                }
                let mut creature_loot: Vec<_> = ctx
                    .db
                    .game_creature_loot()
                    .by_creature()
                    .filter(entry)
                    .collect();
                creature_loot.sort_by_key(|loot| loot.id);
                for loot in creature_loot {
                    for value in [loot.item_entry, loot.chance_bp, loot.count] {
                        hash_u64(&mut hasher, u64::from(value));
                    }
                    hasher.update(&[loot.quest_only as u8]);
                }
                if let Some(template) = ctx.db.game_gameobject_template().entry().find(entry) {
                    hasher.update(&[template.type_id]);
                    for value in [template.data0, template.data1, template.lock_id] {
                        hash_u64(&mut hasher, u64::from(value));
                    }
                    let mut gameobject_loot: Vec<_> = ctx
                        .db
                        .game_gameobject_loot()
                        .by_loot()
                        .filter(template.data1)
                        .collect();
                    gameobject_loot.sort_by_key(|loot| loot.id);
                    for loot in gameobject_loot {
                        for value in [loot.item_entry, loot.chance_bp, loot.count] {
                            hash_u64(&mut hasher, u64::from(value));
                        }
                        hasher.update(&[loot.quest_only as u8]);
                    }
                }
            }
        }
        hasher.update(&[
            ctx.db
                .game_quest_event_requirement()
                .by_quest()
                .filter(definition.entry)
                .next()
                .is_some() as u8,
            ctx.db
                .game_quest_cast_objective()
                .by_quest()
                .filter(definition.entry)
                .next()
                .is_some() as u8,
        ]);
    }
    format!("observed-catalog-v1:{}", hasher.finalize().to_hex())
}

fn runtime_reference_revision(ctx: &ReducerContext) -> String {
    let mut reference = String::from("import-meta-v1");
    let mut found = false;
    for family in ["quests", "creatures", "gameobjects", "loot"] {
        reference.push(';');
        reference.push_str(family);
        reference.push('=');
        if let Some(meta) = crate::actor::import_revision(ctx, family) {
            found = true;
            if meta.source_sha.is_empty() && meta.file_hash.is_empty() {
                reference.push_str("unknown");
            } else {
                reference.push_str(if meta.source_sha.is_empty() {
                    "unknown"
                } else {
                    &meta.source_sha
                });
                reference.push('@');
                reference.push_str(if meta.file_hash.is_empty() {
                    "unknown"
                } else {
                    &meta.file_hash
                });
            }
        } else {
            reference.push_str("unknown");
        }
    }
    if found {
        reference
    } else {
        "unknown".to_string()
    }
}

pub(super) fn refresh_catalog(ctx: &ReducerContext, reference_source_revision: &str) {
    let content_revision = observed_content_revision(ctx);
    let headers = ctx.db.pkg_playerbots_quest_catalog();
    headers.revision().delete(CATALOG_REVISION);
    headers.insert(PlayerbotsQuestCatalog {
        revision: CATALOG_REVISION,
        name: CATALOG_NAME.to_string(),
        blueprint_revision: CATALOG_BLUEPRINT_REVISION.to_string(),
        reference_source_revision: reference_source_revision.to_string(),
        content_revision: content_revision.clone(),
        quest_count: QUESTS.len() as u32,
        refresh_after_micros: ctx
            .timestamp
            .to_micros_since_unix_epoch()
            .saturating_add(REFRESH_INTERVAL_MICROS),
    });
    let seeds = ctx.db.pkg_playerbots_catalog_seed();
    for (class, fixture_seed) in [(1, 783_001), (5, 783_005), (8, 783_008)] {
        seeds.class().delete(class);
        seeds.insert(PlayerbotsCatalogSeed {
            class,
            fixture_seed,
            catalog_revision: CATALOG_REVISION,
            quest_order: QUESTS.iter().map(|quest| quest.entry).collect(),
        });
    }
    let quests = ctx.db.pkg_playerbots_catalog_quest();
    let objectives = ctx.db.pkg_playerbots_catalog_objective();
    for definition in QUESTS.iter().copied() {
        let template = ctx.db.game_quest_template().entry().find(definition.entry);
        let start_destinations = destinations(ctx, definition.start, 0);
        let actual_ender_destinations = destinations(ctx, definition.actual_ender, 0);
        let eligibility = template
            .map(|template| {
                (
                    template.min_level,
                    template.required_races,
                    template.required_classes,
                    template.prev_quest_id,
                )
            })
            .unwrap_or((
                definition.min_level,
                definition.required_races,
                definition.required_classes,
                definition.prerequisite_quest,
            ));
        quests.quest_entry().delete(definition.entry);
        quests.insert(PlayerbotsCatalogQuest {
            quest_entry: definition.entry,
            catalog_revision: CATALOG_REVISION,
            catalog_order: QUESTS
                .iter()
                .position(|entry| entry.entry == definition.entry)
                .unwrap_or_default() as u16,
            min_level: eligibility.0,
            required_races: eligibility.1,
            required_classes: eligibility.2,
            prerequisite_quest: eligibility.3,
            start_kind: definition.start.kind,
            start_entry: definition.start.entry,
            start_destinations,
            actual_ender_kind: definition.actual_ender.kind,
            actual_ender_entry: definition.actual_ender.entry,
            actual_ender_destinations: actual_ender_destinations.clone(),
            content_revision: content_revision.clone(),
        });
        for row in objectives
            .by_quest()
            .filter(definition.entry)
            .collect::<Vec<_>>()
        {
            objectives.id().delete(row.id);
        }
        for objective in definition.objectives.iter().copied() {
            let mut evidence =
                source_destinations(ctx, objective.source_kind, objective.source_entries);
            if objective.kind == CatalogObjectiveKind::TalkOnly
                || objective.executor == ObjectiveExecutor::ProvidedItem
            {
                evidence = actual_ender_destinations.clone();
            }
            let id = objective_id(definition.entry, objective.index);
            objectives.insert(PlayerbotsCatalogObjective {
                id,
                quest_entry: definition.entry,
                objective_index: objective.index,
                kind: objective.kind,
                target_entry: objective.target_entry,
                required_count: objective.required_count,
                executor: objective.executor,
                source_kind: objective.source_kind,
                source_entries: objective.source_entries.to_vec(),
                work_area: work_area(&evidence),
                source_destinations: evidence,
                destination_evidence_revision: content_revision.clone(),
                catalog_revision: CATALOG_REVISION,
            });
        }
    }
}

/// Recheck observed catalog inputs at most once per interval for the whole Shard. Every admission
/// still reads the current quest, relation, objective, loot, and destination rows it depends on.
pub(super) fn ensure_catalog(ctx: &ReducerContext) {
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    if ctx
        .db
        .pkg_playerbots_quest_catalog()
        .revision()
        .find(CATALOG_REVISION)
        .is_some_and(|header| header.refresh_after_micros > now)
    {
        return;
    }
    let observed_revision = observed_content_revision(ctx);
    let reference_revision = runtime_reference_revision(ctx);
    let current = ctx
        .db
        .pkg_playerbots_quest_catalog()
        .revision()
        .find(CATALOG_REVISION);
    match current {
        Some(mut header)
            if header.content_revision == observed_revision
                && header.reference_source_revision == reference_revision =>
        {
            header.refresh_after_micros = now.saturating_add(REFRESH_INTERVAL_MICROS);
            ctx.db
                .pkg_playerbots_quest_catalog()
                .revision()
                .update(header);
        }
        _ => refresh_catalog(ctx, &reference_revision),
    }
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

fn missing(kind: MissingCapability, detail: impl Into<String>) -> AdmissionRefusal {
    AdmissionRefusal::Unsupported {
        capability: kind,
        detail: detail.into(),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum AdmissionRefusal {
    Ineligible(String),
    Unsupported {
        capability: MissingCapability,
        detail: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct QuestAdmission {
    pub quest_entry: u32,
    pub start: CatalogDestination,
    pub target: RetainedQuestTarget,
    pub actual_ender: CatalogDestination,
    pub destination: CatalogDestination,
    pub destination_evidence_revision: String,
    pub content_revision: String,
}

fn objective_rows(ctx: &ReducerContext, quest_entry: u32) -> Vec<PlayerbotsCatalogObjective> {
    let mut rows: Vec<_> = ctx
        .db
        .pkg_playerbots_catalog_objective()
        .by_quest()
        .filter(quest_entry)
        .collect();
    rows.sort_by_key(|row| row.objective_index);
    rows
}

fn usable_source_destinations(
    ctx: &ReducerContext,
    catalog: &PlayerbotsCatalogObjective,
) -> Vec<CatalogDestination> {
    let entries: Vec<_> = catalog
        .source_entries
        .iter()
        .copied()
        .filter(|entry| match catalog.executor {
            ObjectiveExecutor::Attack => true,
            ObjectiveExecutor::CreatureLoot => ctx
                .db
                .game_creature_loot()
                .by_creature()
                .filter(entry)
                .any(|loot| {
                    loot.item_entry == catalog.target_entry && loot.chance_bp > 0 && loot.count > 0
                }),
            ObjectiveExecutor::GameObjectLoot => ctx
                .db
                .game_gameobject_template()
                .entry()
                .find(entry)
                .is_some_and(|template| {
                    template.type_id == crate::gameobject::go_type::CHEST
                        && template.lock_id == 0
                        && ctx
                            .db
                            .game_gameobject_loot()
                            .by_loot()
                            .filter(template.data1)
                            .any(|loot| {
                                loot.item_entry == catalog.target_entry
                                    && loot.chance_bp > 0
                                    && loot.count > 0
                            })
                }),
            ObjectiveExecutor::SimpleGameObject => ctx
                .db
                .game_gameobject_template()
                .entry()
                .find(entry)
                .is_some_and(|template| {
                    template.type_id == crate::gameobject::go_type::GOOBER && template.lock_id == 0
                }),
            ObjectiveExecutor::Talk | ObjectiveExecutor::ProvidedItem => false,
        })
        .collect();
    source_destinations(ctx, catalog.source_kind, &entries)
}

fn validate_executor(
    ctx: &ReducerContext,
    quest: &PlayerbotsCatalogQuest,
    catalog: &PlayerbotsCatalogObjective,
    core: Option<&crate::QuestObjective>,
) -> Result<(), AdmissionRefusal> {
    match catalog.kind {
        CatalogObjectiveKind::ExploreAreaTrigger => {
            return Err(missing(
                MissingCapability::Exploration,
                "area-trigger exploration has no sessionless executor",
            ));
        }
        CatalogObjectiveKind::Escort => {
            return Err(missing(
                MissingCapability::Escort,
                "escort state has no quest executor",
            ));
        }
        CatalogObjectiveKind::ScriptedEvent => {
            return Err(missing(
                MissingCapability::ScriptedEvent,
                "scripted event credit has no quest executor",
            ));
        }
        CatalogObjectiveKind::Transport => {
            return Err(missing(
                MissingCapability::Transport,
                "transport steps have no quest executor",
            ));
        }
        CatalogObjectiveKind::ComplexGameObject => {
            return Err(missing(
                MissingCapability::ComplexGameObject,
                "complex GameObject behavior is outside the supported executor",
            ));
        }
        CatalogObjectiveKind::TalkOnly => {
            if core.is_some() {
                return Err(missing(
                    MissingCapability::ObjectiveMismatch,
                    "talk-only catalog entry has a core objective",
                ));
            }
            if catalog.executor != ObjectiveExecutor::Talk {
                return Err(missing(
                    MissingCapability::ObjectiveMismatch,
                    "talk-only entry does not name the talk executor",
                ));
            }
        }
        CatalogObjectiveKind::KillCreature => {
            let Some(core) = core else {
                return Err(missing(
                    MissingCapability::ObjectiveMismatch,
                    "kill objective is absent from core content",
                ));
            };
            if core.kind != crate::quest::objective_kind::KILL_CREATURE
                || core.target_entry != catalog.target_entry
                || core.required_count != catalog.required_count
                || catalog.executor != ObjectiveExecutor::Attack
            {
                return Err(missing(
                    MissingCapability::ObjectiveMismatch,
                    "kill objective differs from the catalog",
                ));
            }
            if usable_source_destinations(ctx, catalog).is_empty() {
                return Err(missing(
                    MissingCapability::MissingSourceDestination,
                    "kill target has no stored destination",
                ));
            }
        }
        CatalogObjectiveKind::CollectItem => {
            let Some(core) = core else {
                return Err(missing(
                    MissingCapability::ObjectiveMismatch,
                    "collect objective is absent from core content",
                ));
            };
            if core.kind != crate::quest::objective_kind::COLLECT_ITEM
                || core.target_entry != catalog.target_entry
                || core.required_count != catalog.required_count
            {
                return Err(missing(
                    MissingCapability::ObjectiveMismatch,
                    "collect objective differs from the catalog",
                ));
            }
            if ctx
                .db
                .game_item_template()
                .entry()
                .find(catalog.target_entry)
                .is_none()
            {
                return Err(missing(
                    MissingCapability::MissingItemTemplate,
                    "collect objective item is absent",
                ));
            }
            match catalog.executor {
                ObjectiveExecutor::ProvidedItem => {
                    let template = ctx
                        .db
                        .game_quest_template()
                        .entry()
                        .find(quest.quest_entry)
                        .ok_or_else(|| {
                            missing(
                                MissingCapability::MissingQuestTemplate,
                                "quest template disappeared",
                            )
                        })?;
                    if template.src_item != catalog.target_entry
                        || template.src_item_count.max(1) < catalog.required_count
                    {
                        return Err(missing(
                            MissingCapability::MissingLootSource,
                            "acceptance no longer supplies the required item",
                        ));
                    }
                }
                ObjectiveExecutor::CreatureLoot => {
                    let source_ok = catalog.source_entries.iter().any(|entry| {
                        ctx.db
                            .game_creature_loot()
                            .by_creature()
                            .filter(entry)
                            .any(|loot| {
                                loot.item_entry == catalog.target_entry
                                    && loot.chance_bp > 0
                                    && loot.count > 0
                            })
                    });
                    if !source_ok {
                        return Err(missing(
                            MissingCapability::MissingLootSource,
                            "no catalog creature drops the required item",
                        ));
                    }
                    if usable_source_destinations(ctx, catalog).is_empty() {
                        return Err(missing(
                            MissingCapability::MissingSourceDestination,
                            "creature drop source has no stored destination",
                        ));
                    }
                }
                ObjectiveExecutor::GameObjectLoot => {
                    let has_template = catalog.source_entries.iter().any(|entry| {
                        ctx.db
                            .game_gameobject_template()
                            .entry()
                            .find(entry)
                            .is_some()
                    });
                    let has_simple_chest = catalog.source_entries.iter().any(|entry| {
                        ctx.db
                            .game_gameobject_template()
                            .entry()
                            .find(entry)
                            .is_some_and(|template| {
                                template.type_id == crate::gameobject::go_type::CHEST
                                    && template.lock_id == 0
                            })
                    });
                    let source_ok = catalog.source_entries.iter().any(|entry| {
                        ctx.db
                            .game_gameobject_template()
                            .entry()
                            .find(entry)
                            .is_some_and(|template| {
                                template.type_id == crate::gameobject::go_type::CHEST
                                    && template.lock_id == 0
                                    && ctx
                                        .db
                                        .game_gameobject_loot()
                                        .by_loot()
                                        .filter(template.data1)
                                        .any(|loot| {
                                            loot.item_entry == catalog.target_entry
                                                && loot.chance_bp > 0
                                                && loot.count > 0
                                        })
                            })
                    });
                    if !source_ok {
                        if has_template && !has_simple_chest {
                            return Err(missing(
                                MissingCapability::ComplexGameObject,
                                "GameObject loot source requires unsupported behavior",
                            ));
                        }
                        return Err(missing(
                            MissingCapability::MissingLootSource,
                            "no simple chest source drops the required item",
                        ));
                    }
                    if usable_source_destinations(ctx, catalog).is_empty() {
                        return Err(missing(
                            MissingCapability::MissingSourceDestination,
                            "GameObject drop source has no imported spawn",
                        ));
                    }
                }
                _ => {
                    return Err(missing(
                        MissingCapability::ObjectiveMismatch,
                        "collect objective names the wrong executor",
                    ));
                }
            }
        }
        CatalogObjectiveKind::UseGameObject => {
            let Some(core) = core else {
                return Err(missing(
                    MissingCapability::ObjectiveMismatch,
                    "GameObject objective is absent from core content",
                ));
            };
            if core.kind != crate::quest::objective_kind::USE_GAMEOBJECT
                || core.target_entry != catalog.target_entry
                || core.required_count != catalog.required_count
                || catalog.executor != ObjectiveExecutor::SimpleGameObject
            {
                return Err(missing(
                    MissingCapability::ObjectiveMismatch,
                    "GameObject objective differs from the catalog",
                ));
            }
            let simple = catalog.source_entries.iter().any(|entry| {
                ctx.db
                    .game_gameobject_template()
                    .entry()
                    .find(entry)
                    .is_some_and(|template| {
                        template.type_id == crate::gameobject::go_type::GOOBER
                            && template.lock_id == 0
                    })
            });
            if !simple {
                return Err(missing(
                    MissingCapability::ComplexGameObject,
                    "GameObject requires unsupported behavior",
                ));
            }
            if usable_source_destinations(ctx, catalog).is_empty() {
                return Err(missing(
                    MissingCapability::MissingSourceDestination,
                    "GameObject objective has no imported spawn",
                ));
            }
        }
    }
    Ok(())
}

fn inspect(
    ctx: &ReducerContext,
    character_guid: u64,
    quest_entry: u32,
    check_accept_gates: bool,
) -> Result<QuestAdmission, AdmissionRefusal> {
    ensure_catalog(ctx);
    let quest = ctx
        .db
        .pkg_playerbots_catalog_quest()
        .quest_entry()
        .find(quest_entry)
        .ok_or_else(|| {
            missing(
                MissingCapability::MissingCatalog,
                "quest is not in the supported catalog",
            )
        })?;
    let template = ctx
        .db
        .game_quest_template()
        .entry()
        .find(quest_entry)
        .ok_or_else(|| {
            missing(
                MissingCapability::MissingQuestTemplate,
                "quest template is absent",
            )
        })?;
    if (
        template.min_level,
        template.required_races,
        template.required_classes,
        template.prev_quest_id,
    ) != (
        quest.min_level,
        quest.required_races,
        quest.required_classes,
        quest.prerequisite_quest,
    ) {
        return Err(missing(
            MissingCapability::ChangedEligibility,
            "live quest eligibility differs from the catalog",
        ));
    }
    let character = ctx
        .db
        .game_world_entity()
        .guid()
        .find(character_guid)
        .ok_or_else(|| AdmissionRefusal::Ineligible("Character is not in the world".to_string()))?;
    if check_accept_gates {
        crate::quest::accept_gates(ctx, &character, &template)
            .map_err(AdmissionRefusal::Ineligible)?;
    }
    if !relation_exists(
        ctx,
        quest.start_kind,
        quest.start_entry,
        quest_entry,
        crate::quest::quest_role::START,
    ) {
        return Err(missing(
            MissingCapability::MissingStartRelation,
            "catalog start giver does not offer the quest",
        ));
    }
    if !relation_exists(
        ctx,
        quest.actual_ender_kind,
        quest.actual_ender_entry,
        quest_entry,
        crate::quest::quest_role::END,
    ) {
        return Err(missing(
            MissingCapability::MissingActualEndRelation,
            "catalog actual ender does not end the quest",
        ));
    }
    let start = destinations(
        ctx,
        EntityDefinition {
            kind: quest.start_kind,
            entry: quest.start_entry,
        },
        0,
    )
    .into_iter()
    .next()
    .ok_or_else(|| {
        missing(
            MissingCapability::MissingStartDestination,
            "start giver has no stored destination",
        )
    })?;
    let actual_ender = destinations(
        ctx,
        EntityDefinition {
            kind: quest.actual_ender_kind,
            entry: quest.actual_ender_entry,
        },
        0,
    )
    .into_iter()
    .next()
    .ok_or_else(|| {
        missing(
            MissingCapability::MissingActualEndDestination,
            "actual ender has no stored destination",
        )
    })?;
    if ctx
        .db
        .game_quest_event_requirement()
        .by_quest()
        .filter(quest_entry)
        .next()
        .is_some()
    {
        return Err(missing(
            MissingCapability::ScriptedEvent,
            "quest requires scripted event credit",
        ));
    }
    if ctx
        .db
        .game_quest_cast_objective()
        .by_quest()
        .filter(quest_entry)
        .next()
        .is_some()
    {
        return Err(missing(
            MissingCapability::CastObjective,
            "quest requires a cast objective",
        ));
    }
    let mut core: Vec<_> = ctx
        .db
        .game_quest_objective()
        .by_quest()
        .filter(quest_entry)
        .collect();
    core.sort_by_key(|objective| objective.obj_index);
    let catalog = objective_rows(ctx, quest_entry);
    let talk_only = catalog.len() == 1 && catalog[0].kind == CatalogObjectiveKind::TalkOnly;
    if (!talk_only && catalog.len() != core.len()) || (talk_only && !core.is_empty()) {
        return Err(missing(
            MissingCapability::ObjectiveMismatch,
            "core objective count differs from the catalog",
        ));
    }
    for catalog_objective in &catalog {
        let core_objective = core
            .iter()
            .find(|objective| objective.obj_index == catalog_objective.objective_index);
        validate_executor(ctx, &quest, catalog_objective, core_objective)?;
    }
    let selected = catalog
        .iter()
        .find(|objective| {
            core.iter()
                .find(|core| core.obj_index == objective.objective_index)
                .is_none_or(|core| {
                    let count = if core.kind == crate::quest::objective_kind::COLLECT_ITEM {
                        crate::items::item_count(ctx, character_guid, core.target_entry)
                    } else {
                        ctx.db
                            .game_character_quest()
                            .by_character()
                            .filter(character_guid)
                            .find(|row| row.quest_entry == quest_entry)
                            .and_then(|row| row.counts.get(core.obj_index as usize).copied())
                            .unwrap_or(0)
                    };
                    count < core.required_count
                })
        })
        .or_else(|| catalog.first())
        .ok_or_else(|| {
            missing(
                MissingCapability::ObjectiveMismatch,
                "catalog has no objective classification",
            )
        })?;
    let source = usable_source_destinations(ctx, selected).into_iter().next();
    let destination = if talk_only || selected.executor == ObjectiveExecutor::ProvidedItem {
        actual_ender.clone()
    } else {
        source.clone().ok_or_else(|| {
            missing(
                MissingCapability::MissingSourceDestination,
                "selected objective has no destination",
            )
        })?
    };
    Ok(QuestAdmission {
        quest_entry,
        start,
        target: RetainedQuestTarget {
            objective_index: selected.objective_index,
            kind: selected.kind,
            target_entry: selected.target_entry,
            required_count: selected.required_count,
            executor: selected.executor,
            source,
        },
        actual_ender,
        destination,
        destination_evidence_revision: selected.destination_evidence_revision.clone(),
        content_revision: quest.content_revision,
    })
}

#[cfg_attr(
    not(feature = "debug_reducers"),
    allow(
        dead_code,
        reason = "available-quest admission is driven by the durable catalog fixture"
    )
)]
pub(super) fn admit_available(
    ctx: &ReducerContext,
    character_guid: u64,
    quest_entry: u32,
) -> Result<QuestAdmission, AdmissionRefusal> {
    inspect(ctx, character_guid, quest_entry, true)
}

pub(super) fn admit_held(
    ctx: &ReducerContext,
    character_guid: u64,
    quest_entry: u32,
) -> Result<QuestAdmission, AdmissionRefusal> {
    let held = ctx
        .db
        .game_character_quest()
        .by_character()
        .filter(character_guid)
        .any(|row| row.quest_entry == quest_entry && !row.rewarded && !row.failed);
    if !held {
        return Err(AdmissionRefusal::Ineligible(
            "quest is not active".to_string(),
        ));
    }
    inspect(ctx, character_guid, quest_entry, false)
}

pub(super) fn reconcile_active(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Option<QuestAdmission> {
    ensure_catalog(ctx);
    let reconsider = ctx
        .db
        .pkg_playerbots_quest_admission()
        .character_guid()
        .find(character_guid)
        .filter(|row| row.missing_capability.is_some())
        .map(|row| row.considered_quest);
    let retained = ctx
        .db
        .pkg_playerbots_quest_objective()
        .character_guid()
        .find(character_guid)
        .map(|row| row.quest_entry);
    let mut entries: Vec<_> = ctx
        .db
        .game_character_quest()
        .by_character()
        .filter(character_guid)
        .filter(|quest| !quest.rewarded && !quest.failed)
        .map(|quest| quest.quest_entry)
        .collect();
    entries.sort_by_key(|entry| {
        (
            u8::from(Some(*entry) != reconsider),
            u8::from(Some(*entry) != retained),
            QUESTS
                .iter()
                .position(|quest| quest.entry == *entry)
                .unwrap_or(usize::MAX),
        )
    });
    let mut first_refusal = None;
    for entry in entries {
        match admit_held(ctx, character_guid, entry) {
            Ok(admission) => {
                if let Some((considered, refusal)) = first_refusal.as_ref() {
                    record_admission(
                        ctx,
                        character_guid,
                        *considered,
                        Some(admission.quest_entry),
                        Some(refusal),
                    );
                } else {
                    record_admission(
                        ctx,
                        character_guid,
                        admission.quest_entry,
                        Some(admission.quest_entry),
                        None,
                    );
                }
                return Some(admission);
            }
            Err(refusal @ AdmissionRefusal::Unsupported { .. }) => {
                first_refusal.get_or_insert((entry, refusal));
            }
            Err(AdmissionRefusal::Ineligible(_)) => {}
        }
    }
    if let Some((entry, refusal)) = first_refusal.as_ref() {
        record_admission(ctx, character_guid, *entry, None, Some(refusal));
    }
    None
}

pub(super) fn retain(
    ctx: &ReducerContext,
    character_guid: u64,
    runner_objective_identity: u64,
    admission: QuestAdmission,
) {
    let reference_source_revision = ctx
        .db
        .pkg_playerbots_quest_catalog()
        .revision()
        .find(CATALOG_REVISION)
        .map_or_else(
            || "unknown".to_string(),
            |header| header.reference_source_revision,
        );
    let row = PlayerbotsQuestObjective {
        character_guid,
        runner_objective_identity,
        quest_entry: admission.quest_entry,
        target: admission.target,
        actual_ender_kind: admission.actual_ender.kind,
        actual_ender_entry: admission.actual_ender.entry,
        actual_ender: admission.actual_ender,
        destination: admission.destination,
        destination_evidence_revision: admission.destination_evidence_revision,
        catalog_revision: CATALOG_REVISION,
        reference_source_revision,
        content_revision: admission.content_revision,
    };
    let rows = ctx.db.pkg_playerbots_quest_objective();
    if rows.character_guid().find(character_guid).is_some() {
        rows.character_guid().update(row);
    } else {
        rows.insert(row);
    }
}

pub(super) fn retained_matches(
    ctx: &ReducerContext,
    character_guid: u64,
    admission: &QuestAdmission,
) -> bool {
    let reference_source_revision = ctx
        .db
        .pkg_playerbots_quest_catalog()
        .revision()
        .find(CATALOG_REVISION)
        .map_or_else(
            || "unknown".to_string(),
            |header| header.reference_source_revision,
        );
    ctx.db
        .pkg_playerbots_quest_objective()
        .character_guid()
        .find(character_guid)
        .is_some_and(|row| {
            row.quest_entry == admission.quest_entry
                && row.target == admission.target
                && row.actual_ender_kind == admission.actual_ender.kind
                && row.actual_ender_entry == admission.actual_ender.entry
                && row.actual_ender == admission.actual_ender
                && row.destination == admission.destination
                && row.destination_evidence_revision == admission.destination_evidence_revision
                && row.catalog_revision == CATALOG_REVISION
                && row.reference_source_revision == reference_source_revision
                && row.content_revision == admission.content_revision
        })
}

pub(super) fn clear_retained(ctx: &ReducerContext, character_guid: u64) {
    ctx.db
        .pkg_playerbots_quest_objective()
        .character_guid()
        .delete(character_guid);
}

pub(super) fn active_wait_until(ctx: &ReducerContext, character_guid: u64) -> Option<i64> {
    let admission = ctx
        .db
        .pkg_playerbots_quest_admission()
        .character_guid()
        .find(character_guid)
        .filter(|row| row.state == QuestAdmissionState::Waiting)?;
    ctx.db
        .game_character_quest()
        .by_character()
        .filter(character_guid)
        .any(|quest| {
            quest.quest_entry == admission.considered_quest && !quest.rewarded && !quest.failed
        })
        .then_some(admission.wait_until_micros)
}

pub(super) fn record_admission(
    ctx: &ReducerContext,
    character_guid: u64,
    considered_quest: u32,
    selected_quest: Option<u32>,
    refusal: Option<&AdmissionRefusal>,
) {
    let (state, capability, detail, wait_until_micros) = match refusal {
        None => (QuestAdmissionState::Admitted, None, String::new(), 0),
        Some(AdmissionRefusal::Ineligible(detail)) => {
            (QuestAdmissionState::Ineligible, None, detail.clone(), 0)
        }
        Some(AdmissionRefusal::Unsupported { capability, detail }) => (
            if selected_quest.is_some() {
                QuestAdmissionState::Unsupported
            } else {
                QuestAdmissionState::Waiting
            },
            Some(*capability),
            detail.clone(),
            ctx.timestamp
                .to_micros_since_unix_epoch()
                .saturating_add(WAIT_MICROS),
        ),
    };
    let row = PlayerbotsQuestAdmission {
        character_guid,
        considered_quest,
        selected_quest,
        state,
        missing_capability: capability,
        detail,
        catalog_revision: CATALOG_REVISION,
        observed_micros: ctx.timestamp.to_micros_since_unix_epoch(),
        wait_until_micros,
    };
    let rows = ctx.db.pkg_playerbots_quest_admission();
    if rows.character_guid().find(character_guid).is_some() {
        rows.character_guid().update(row);
    } else {
        rows.insert(row);
    }
}

#[cfg_attr(
    not(feature = "debug_reducers"),
    allow(
        dead_code,
        reason = "live-target selection is driven by the durable catalog fixture"
    )
)]
pub(super) fn live_creature_target(
    ctx: &ReducerContext,
    character_guid: u64,
    entry: u32,
) -> Option<crate::WorldEntity> {
    let character = ctx.db.game_world_entity().guid().find(character_guid)?;
    ctx.db
        .game_world_entity()
        .by_entry()
        .filter(entry)
        .filter(|target| {
            !target.dead
                && (target.map_id, target.instance_id) == (character.map_id, character.instance_id)
        })
        .min_by(|left, right| {
            let left_distance = (left.x - character.x).powi(2) + (left.y - character.y).powi(2);
            let right_distance = (right.x - character.x).powi(2) + (right.y - character.y).powi(2);
            left_distance
                .total_cmp(&right_distance)
                .then_with(|| left.guid.cmp(&right.guid))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_catalog_has_one_class_independent_order() {
        let entries: Vec<_> = QUESTS.iter().map(|quest| quest.entry).collect();
        assert_eq!(
            entries,
            [783, 7, 5261, 33, 18, 3903, 3904, 3905, 40, 35, 37, 45]
        );
        assert!(QUESTS.iter().all(|quest| quest.required_classes == 0));
    }

    #[test]
    fn mixed_requirements_need_every_executor() {
        let supported = [
            CatalogObjectiveKind::KillCreature,
            CatalogObjectiveKind::CollectItem,
        ];
        let mixed = [
            CatalogObjectiveKind::KillCreature,
            CatalogObjectiveKind::Escort,
        ];
        let has_executor = |kind| {
            matches!(
                kind,
                CatalogObjectiveKind::TalkOnly
                    | CatalogObjectiveKind::KillCreature
                    | CatalogObjectiveKind::CollectItem
                    | CatalogObjectiveKind::UseGameObject
            )
        };
        assert!(supported.into_iter().all(has_executor));
        assert!(!mixed.into_iter().all(has_executor));
    }

    #[test]
    fn harvest_is_collect_item_backed_by_gameobject_loot() {
        let objective = Q3904[0];
        assert_eq!(objective.kind, CatalogObjectiveKind::CollectItem);
        assert_eq!(objective.executor, ObjectiveExecutor::GameObjectLoot);
        assert_eq!(objective.target_entry, 11119);
        assert_eq!(objective.source_entries, [161557]);
    }
}
