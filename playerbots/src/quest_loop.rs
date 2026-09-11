//! Autonomous execution for one retained supported quest.

use super::decision::{
    Action, ActionNode, CastAction, MoveTarget, QuestInteraction, QuestLoot, Readiness, Reason,
};
use super::quest_catalog::{
    pkg_playerbots_quest_objective, CatalogDestination, CatalogEntityKind, ObjectiveExecutor,
    PlayerbotsQuestObjective, RetainedPosition,
};
use super::PlayerbotsBot;
use crate::{game_corpse_loot, game_gameobject, game_world_entity};
use spacetimedb::ReducerContext;

const SEARCH_RADIUS_YD: f32 = 100.0;
const INTERACTION_RANGE_YD: f32 = 9.0;
const MELEE_RANGE_YD: f32 = 4.0;
const RAW_ENTITY_LIMIT: usize = 96;
const CORPSE_LIMIT: usize = 8;
const CONTROL_TARGET_LIMIT: usize = 12;
const CONTROL_AURA_LIMIT: usize = 64;
const LOOT_ROW_LIMIT: usize = 16;
const SAFE_APPROACH_MIN_YD: f32 = 12.0;
const SAFE_APPROACH_MAX_YD: f32 = 35.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WaitReason {
    MissingTarget,
    ReadLimit,
    Respawn,
    Deferred,
    Controlled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum QuestPlan {
    Accept {
        quest: u32,
        giver: u64,
    },
    TurnIn {
        quest: u32,
        giver: u64,
    },
    Attack {
        quest: u32,
        target: u64,
    },
    LootCreature {
        quest: u32,
        corpse: u64,
        slot: u8,
    },
    UseGameObject {
        quest: u32,
        gameobject: u64,
    },
    LootGameObject {
        quest: u32,
        gameobject: u64,
        slot: u8,
    },
    Wait(WaitReason),
}

impl QuestPlan {
    pub(super) fn target(self) -> Option<u64> {
        match self {
            Self::Accept { giver, .. } | Self::TurnIn { giver, .. } => Some(giver),
            Self::Attack { target, .. } => Some(target),
            Self::LootCreature { corpse, .. } => Some(corpse),
            Self::UseGameObject { gameobject, .. } | Self::LootGameObject { gameobject, .. } => {
                Some(gameobject)
            }
            Self::Wait(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StepResult {
    Completed,
    Waiting,
    Refused(crate::actor::ActionRefusalKind),
}

enum Search<T> {
    Found(T),
    Missing,
    Limit,
}

struct EntitySearch {
    rows: Vec<crate::WorldEntity>,
    exhausted: bool,
}

pub(super) enum LiveCreatureTarget {
    Found(crate::WorldEntity),
    Missing,
    ReadLimit,
    Deferred,
    Controlled,
}

enum GameObjectWork {
    Use(u64),
    Loot { guid: u64, slot: u8 },
}

enum GameObjectSearch {
    Found(GameObjectWork),
    Missing,
    Limit,
    Respawn,
}

fn distance_sq(me: &crate::WorldEntity, x: f32, y: f32, z: f32) -> f32 {
    (me.x - x).powi(2) + (me.y - y).powi(2) + (me.z - z).powi(2)
}

fn live_giver(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    destination: &CatalogDestination,
) -> bool {
    match destination.kind {
        CatalogEntityKind::Creature => ctx
            .db
            .game_world_entity()
            .guid()
            .find(destination.guid)
            .is_some_and(|giver| {
                !giver.dead
                    && giver.entry == destination.entry
                    && (giver.map_id, giver.instance_id) == (me.map_id, me.instance_id)
            }),
        CatalogEntityKind::GameObject => ctx
            .db
            .game_gameobject()
            .guid()
            .find(destination.guid)
            .is_some_and(|giver| {
                giver.template_entry == destination.entry
                    && (giver.map_id, giver.instance_id) == (me.map_id, me.instance_id)
            }),
    }
}

fn search_entities(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    dead: bool,
    mut wanted: impl FnMut(&crate::WorldEntity) -> bool,
) -> EntitySearch {
    let (gx0, gx1, gy0, gy1) =
        lyracore_shared::spatial::covering_cell_box(me.x, me.y, SEARCH_RADIUS_YD);
    let entities = ctx.db.game_world_entity();
    let mut scanned = 0usize;
    let mut found = Vec::new();
    for gx in gx0..=gx1 {
        for gy in gy0..=gy1 {
            let cell = lyracore_shared::spatial::grid_cell_id(gx, gy);
            for entity in entities.by_cell().filter((me.map_id, me.instance_id, cell)) {
                if scanned == RAW_ENTITY_LIMIT {
                    return EntitySearch {
                        rows: found,
                        exhausted: true,
                    };
                }
                scanned += 1;
                if entity.dead == dead
                    && wanted(&entity)
                    && distance_sq(me, entity.x, entity.y, entity.z)
                        <= SEARCH_RADIUS_YD * SEARCH_RADIUS_YD
                {
                    found.push(entity);
                }
            }
        }
    }
    EntitySearch {
        rows: found,
        exhausted: false,
    }
}

pub(super) fn live_creature_target(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    entry: u32,
    eligible_work: impl Fn(u64) -> bool,
) -> LiveCreatureTarget {
    let search = search_entities(ctx, me, false, |target| target.entry == entry);
    select_live_target(ctx, me, search, eligible_work, None, None)
}

fn retained_creature(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    source: &CatalogDestination,
) -> Option<crate::WorldEntity> {
    if source.kind == CatalogEntityKind::Creature {
        ctx.db.game_world_entity().guid().find(source.guid)
    } else {
        None
    }
    .filter(|target| {
        target.entry == source.entry
            && (target.map_id, target.instance_id) == (me.map_id, me.instance_id)
            && distance_sq(me, target.x, target.y, target.z)
                <= SEARCH_RADIUS_YD * SEARCH_RADIUS_YD
    })
}

pub(super) fn grind_target(
    ctx: &ReducerContext,
    bot: &PlayerbotsBot,
    me: &crate::WorldEntity,
    preferred: Option<u64>,
    eligible_work: impl Fn(u64) -> bool,
) -> LiveCreatureTarget {
    if (me.map_id, me.instance_id) != (bot.home_map, 0) {
        return LiveCreatureTarget::Missing;
    }
    let leash_sq = super::goals::QUEST_LEASH_YD * super::goals::QUEST_LEASH_YD;
    let search = search_entities(ctx, me, false, |target| {
        (target.x - bot.home_x).powi(2) + (target.y - bot.home_y).powi(2) <= leash_sq
            && super::goals::grind_target_is_worthwhile(ctx, me, target)
    });
    select_live_target(
        ctx,
        me,
        search,
        eligible_work,
        Some(super::goals::pick_salt(ctx, me.guid)),
        preferred,
    )
}

fn select_live_target(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    search: EntitySearch,
    eligible_work: impl Fn(u64) -> bool,
    salt: Option<u64>,
    preferred: Option<u64>,
) -> LiveCreatureTarget {
    let mut deferred = false;
    let mut eligible: Vec<_> = search
        .rows
        .into_iter()
        .filter(|target| crate::combat::validate_attack_target(ctx, me, target.guid).is_ok())
        .filter(|target| {
            let eligible = eligible_work(target.guid);
            deferred |= !eligible;
            eligible
        })
        .collect();
    eligible.sort_by(|left, right| {
        distance_sq(me, left.x, left.y, left.z)
            .total_cmp(&distance_sq(me, right.x, right.y, right.z))
            .then_with(|| left.guid.cmp(&right.guid))
    });
    let mut controlled = false;
    let mut found = Vec::new();
    for (index, target) in eligible.into_iter().enumerate() {
        if index == CONTROL_TARGET_LIMIT {
            if found.is_empty() {
                return LiveCreatureTarget::ReadLimit;
            }
            break;
        }
        match crate::spell::control_status(ctx, target.guid, CONTROL_AURA_LIMIT) {
            Ok(None) => {
                if preferred == Some(target.guid) {
                    return LiveCreatureTarget::Found(target);
                }
                found.push(target);
                if salt.is_none()
                    || (preferred.is_none() && found.len() == super::goals::PICK_AMONG_NEAREST)
                {
                    break;
                }
            }
            Ok(Some(_)) => controlled = true,
            Err(_) => return LiveCreatureTarget::ReadLimit,
        }
    }
    if !found.is_empty() {
        found.truncate(super::goals::PICK_AMONG_NEAREST);
        let index = salt
            .and_then(|salt| super::goals::pick_index(salt, found.len()))
            .unwrap_or(0);
        return LiveCreatureTarget::Found(found.swap_remove(index));
    }
    if search.exhausted {
        LiveCreatureTarget::ReadLimit
    } else if controlled {
        LiveCreatureTarget::Controlled
    } else if deferred {
        LiveCreatureTarget::Deferred
    } else {
        LiveCreatureTarget::Missing
    }
}

fn wanted_loot_slot(ctx: &ReducerContext, source_guid: u64, item_entry: u32) -> Search<u8> {
    let rows = ctx.db.game_corpse_loot();
    let mut scanned = 0usize;
    for row in rows.by_corpse().filter(source_guid) {
        if scanned == LOOT_ROW_LIMIT {
            return Search::Limit;
        }
        scanned += 1;
        if row.item_entry == item_entry && !row.withheld {
            return Search::Found(row.slot);
        }
    }
    Search::Missing
}

fn entitled_corpse(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    source_entry: u32,
    item_entry: u32,
) -> Search<(u64, u8)> {
    let search = search_entities(ctx, me, true, |target| target.entry == source_entry);
    let mut corpses = search.rows;
    if corpses.is_empty() && !search.exhausted {
        return Search::Missing;
    }
    corpses.sort_by(|left, right| {
        distance_sq(me, left.x, left.y, left.z)
            .total_cmp(&distance_sq(me, right.x, right.y, right.z))
            .then_with(|| left.guid.cmp(&right.guid))
    });
    let inconclusive = search.exhausted || corpses.len() > CORPSE_LIMIT;
    for corpse in corpses.into_iter().take(CORPSE_LIMIT) {
        if crate::loot::corpse_access(ctx, me.guid, corpse.guid).is_err() {
            continue;
        }
        match wanted_loot_slot(ctx, corpse.guid, item_entry) {
            Search::Found(slot) => return Search::Found((corpse.guid, slot)),
            Search::Limit => return Search::Limit,
            Search::Missing => {}
        }
    }
    if inconclusive {
        Search::Limit
    } else {
        Search::Missing
    }
}

fn retained_creature_plan(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    source: &CatalogDestination,
    quest: u32,
    loot_item: Option<u32>,
    eligible_work: &impl Fn(u64) -> bool,
) -> Option<QuestPlan> {
    let preferred = retained_creature(ctx, me, source)?;
    if preferred.dead {
        let item = loot_item?;
        if crate::loot::corpse_access(ctx, me.guid, preferred.guid).is_err() {
            return None;
        }
        return match wanted_loot_slot(ctx, preferred.guid, item) {
            Search::Found(slot) => Some(QuestPlan::LootCreature {
                quest,
                corpse: preferred.guid,
                slot,
            }),
            Search::Limit => Some(QuestPlan::Wait(WaitReason::ReadLimit)),
            Search::Missing => None,
        };
    }
    let preferred_guid = preferred.guid;
    let search = EntitySearch {
        rows: vec![preferred],
        exhausted: false,
    };
    match select_live_target(ctx, me, search, eligible_work, None, Some(preferred_guid)) {
        LiveCreatureTarget::Found(target) => Some(QuestPlan::Attack {
            quest,
            target: target.guid,
        }),
        LiveCreatureTarget::ReadLimit => Some(QuestPlan::Wait(WaitReason::ReadLimit)),
        LiveCreatureTarget::Missing
        | LiveCreatureTarget::Deferred
        | LiveCreatureTarget::Controlled => None,
    }
}

fn gameobject_work(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    entry: u32,
    wanted_item: Option<u32>,
) -> GameObjectSearch {
    let (gx0, gx1, gy0, gy1) =
        lyracore_shared::spatial::covering_cell_box(me.x, me.y, SEARCH_RADIUS_YD);
    let rows = ctx.db.game_gameobject();
    let mut scanned = 0usize;
    let mut matching = Vec::new();
    let mut exhausted = false;
    'cells: for gx in gx0..=gx1 {
        for gy in gy0..=gy1 {
            let cell = lyracore_shared::spatial::grid_cell_id(gx, gy);
            for row in rows.by_cell().filter((me.map_id, me.instance_id, cell)) {
                if scanned == RAW_ENTITY_LIMIT {
                    exhausted = true;
                    break 'cells;
                }
                scanned += 1;
                if row.template_entry != entry
                    || distance_sq(me, row.x, row.y, row.z) > SEARCH_RADIUS_YD * SEARCH_RADIUS_YD
                {
                    continue;
                }
                matching.push(row);
            }
        }
    }
    matching.sort_by(|left, right| {
        distance_sq(me, left.x, left.y, left.z)
            .total_cmp(&distance_sq(me, right.x, right.y, right.z))
            .then_with(|| left.guid.cmp(&right.guid))
    });
    let mut loot_inconclusive = false;
    for row in &matching {
        if let Some(item_entry) = wanted_item {
            match wanted_loot_slot(ctx, row.guid, item_entry) {
                Search::Found(slot) => {
                    return GameObjectSearch::Found(GameObjectWork::Loot {
                        guid: row.guid,
                        slot,
                    });
                }
                Search::Limit => {
                    loot_inconclusive = true;
                    continue;
                }
                Search::Missing => {}
            }
        }
        if row.state == 0 {
            return GameObjectSearch::Found(GameObjectWork::Use(row.guid));
        }
    }
    if exhausted || loot_inconclusive {
        GameObjectSearch::Limit
    } else if matching.is_empty() {
        GameObjectSearch::Missing
    } else {
        GameObjectSearch::Respawn
    }
}

pub(super) fn plan(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    retained: &PlayerbotsQuestObjective,
    eligible_fight: impl Fn(u64) -> bool,
) -> QuestPlan {
    let quest = retained.quest_entry;
    let Some(held) = crate::quest::character_quest_row(ctx, me.guid, quest) else {
        return if live_giver(ctx, me, &retained.destination) {
            QuestPlan::Accept {
                quest,
                giver: retained.destination.guid,
            }
        } else {
            QuestPlan::Wait(WaitReason::MissingTarget)
        };
    };
    if held.rewarded || held.failed {
        return QuestPlan::Wait(WaitReason::MissingTarget);
    }
    if crate::quest::quest_is_complete(ctx, &held) {
        return if live_giver(ctx, me, &retained.actual_ender) {
            QuestPlan::TurnIn {
                quest,
                giver: retained.actual_ender.guid,
            }
        } else {
            QuestPlan::Wait(WaitReason::MissingTarget)
        };
    }
    let Some(source) = retained.target.source.as_ref() else {
        return QuestPlan::Wait(WaitReason::MissingTarget);
    };
    match retained.target.executor {
        ObjectiveExecutor::Attack => {
            if let Some(plan) =
                retained_creature_plan(ctx, me, source, quest, None, &eligible_fight)
            {
                return plan;
            }
            match live_creature_target(ctx, me, source.entry, &eligible_fight) {
                LiveCreatureTarget::Found(target) => QuestPlan::Attack {
                    quest,
                    target: target.guid,
                },
                LiveCreatureTarget::ReadLimit => QuestPlan::Wait(WaitReason::ReadLimit),
                LiveCreatureTarget::Missing => QuestPlan::Wait(WaitReason::MissingTarget),
                LiveCreatureTarget::Deferred => QuestPlan::Wait(WaitReason::Deferred),
                LiveCreatureTarget::Controlled => QuestPlan::Wait(WaitReason::Controlled),
            }
        }
        ObjectiveExecutor::CreatureLoot => {
            if let Some(plan) = retained_creature_plan(
                ctx,
                me,
                source,
                quest,
                Some(retained.target.target_entry),
                &eligible_fight,
            ) {
                return plan;
            }
            match entitled_corpse(ctx, me, source.entry, retained.target.target_entry) {
                Search::Found((corpse, slot)) => QuestPlan::LootCreature {
                    quest,
                    corpse,
                    slot,
                },
                Search::Limit => QuestPlan::Wait(WaitReason::ReadLimit),
                Search::Missing => {
                    match live_creature_target(ctx, me, source.entry, &eligible_fight) {
                        LiveCreatureTarget::Found(target) => QuestPlan::Attack {
                            quest,
                            target: target.guid,
                        },
                        LiveCreatureTarget::ReadLimit => QuestPlan::Wait(WaitReason::ReadLimit),
                        LiveCreatureTarget::Missing => QuestPlan::Wait(WaitReason::MissingTarget),
                        LiveCreatureTarget::Deferred => QuestPlan::Wait(WaitReason::Deferred),
                        LiveCreatureTarget::Controlled => QuestPlan::Wait(WaitReason::Controlled),
                    }
                }
            }
        }
        ObjectiveExecutor::GameObjectLoot => {
            match gameobject_work(ctx, me, source.entry, Some(retained.target.target_entry)) {
                GameObjectSearch::Found(GameObjectWork::Loot { guid, slot }) => {
                    QuestPlan::LootGameObject {
                        quest,
                        gameobject: guid,
                        slot,
                    }
                }
                GameObjectSearch::Found(GameObjectWork::Use(guid)) => QuestPlan::UseGameObject {
                    quest,
                    gameobject: guid,
                },
                GameObjectSearch::Limit => QuestPlan::Wait(WaitReason::ReadLimit),
                GameObjectSearch::Missing => QuestPlan::Wait(WaitReason::MissingTarget),
                GameObjectSearch::Respawn => QuestPlan::Wait(WaitReason::Respawn),
            }
        }
        ObjectiveExecutor::SimpleGameObject => match gameobject_work(ctx, me, source.entry, None) {
            GameObjectSearch::Found(GameObjectWork::Use(guid)) => QuestPlan::UseGameObject {
                quest,
                gameobject: guid,
            },
            GameObjectSearch::Found(GameObjectWork::Loot { .. }) => {
                unreachable!("simple GameObject search does not inspect loot")
            }
            GameObjectSearch::Limit => QuestPlan::Wait(WaitReason::ReadLimit),
            GameObjectSearch::Missing => QuestPlan::Wait(WaitReason::MissingTarget),
            GameObjectSearch::Respawn => QuestPlan::Wait(WaitReason::Respawn),
        },
        ObjectiveExecutor::Talk | ObjectiveExecutor::ProvidedItem => {
            QuestPlan::Wait(WaitReason::MissingTarget)
        }
    }
}

fn node(action: Action, objective: u64) -> ActionNode {
    action_node(action, Reason::Quest, 110, objective)
}

fn action_node(action: Action, reason: Reason, priority: i32, objective: u64) -> ActionNode {
    let mut node = ActionNode::ready(action, reason, priority);
    node.candidate.id.objective = objective;
    node
}

pub(super) fn combat_strategy(
    ctx: &ReducerContext,
    bot: &PlayerbotsBot,
    me: &crate::WorldEntity,
    target: u64,
    objective: u64,
    reason: Reason,
    priority: i32,
) -> Result<ActionNode, super::companion::RoleReadError> {
    if let Some(spell) =
        super::companion::combat_spell(ctx, bot, me.guid, target, &[super::cond::ALWAYS])?
    {
        let cast = CastAction { target, spell };
        let mut selected = action_node(Action::Cast(cast), reason, priority, objective);
        if crate::spell::pending_cast(ctx, me.guid)
            .is_none_or(|pending| pending.spell_id != spell || pending.target_guid != target)
        {
            match crate::actor::cast_readiness(ctx, me.guid, spell, target) {
                Ok(()) => {}
                Err(refusal)
                    if matches!(
                        refusal.kind,
                        crate::spell::CastRefusalKind::OutOfRange
                            | crate::spell::CastRefusalKind::NoLineOfSight
                    ) =>
                {
                    selected.prerequisites.push(action_node(
                        Action::Move(MoveTarget::CastingPosition(target)),
                        reason,
                        priority,
                        objective,
                    ));
                }
                Err(_) => selected.readiness = Readiness::Refused,
            }
        }
        return Ok(selected);
    }
    let mut selected = action_node(Action::Attack(target), reason, priority, objective);
    if ctx
        .db
        .game_world_entity()
        .guid()
        .find(target)
        .is_some_and(|target| {
            distance_sq(me, target.x, target.y, target.z) > MELEE_RANGE_YD * MELEE_RANGE_YD
        })
    {
        selected.prerequisites.push(action_node(
            Action::Move(MoveTarget::Entity(target)),
            reason,
            priority,
            objective,
        ));
    }
    Ok(selected)
}

pub(super) fn strategy(
    ctx: &ReducerContext,
    bot: &PlayerbotsBot,
    me: &crate::WorldEntity,
    plan: QuestPlan,
    objective: u64,
    away: bool,
    travel: ActionNode,
) -> Result<ActionNode, super::companion::RoleReadError> {
    let mut selected = match plan {
        QuestPlan::Attack { target, .. } => {
            combat_strategy(ctx, bot, me, target, objective, Reason::Quest, 110)?
        }
        QuestPlan::Accept { quest, giver } => node(
            Action::AcceptQuest(QuestInteraction {
                quest,
                target: giver,
            }),
            objective,
        ),
        QuestPlan::TurnIn { quest, giver } => node(
            Action::TurnInQuest(QuestInteraction {
                quest,
                target: giver,
            }),
            objective,
        ),
        QuestPlan::LootCreature {
            quest,
            corpse,
            slot,
        } => node(
            Action::LootCreature(QuestLoot {
                quest,
                target: corpse,
                slot,
            }),
            objective,
        ),
        QuestPlan::UseGameObject { quest, gameobject } => node(
            Action::UseGameObject(QuestInteraction {
                quest,
                target: gameobject,
            }),
            objective,
        ),
        QuestPlan::LootGameObject {
            quest,
            gameobject,
            slot,
        } => node(
            Action::LootGameObject(QuestLoot {
                quest,
                target: gameobject,
                slot,
            }),
            objective,
        ),
        QuestPlan::Wait(WaitReason::Controlled) => {
            let mut hold = ActionNode::ready(Action::Hold, Reason::CrowdControl, 750);
            hold.candidate.id.objective = objective;
            hold
        }
        QuestPlan::Wait(_) => node(Action::Hold, objective),
    };
    let in_interaction_range = match plan {
        QuestPlan::Attack { .. } | QuestPlan::Wait(_) => true,
        QuestPlan::Accept { giver, .. } | QuestPlan::TurnIn { giver, .. } => {
            if let Some(target) = ctx.db.game_world_entity().guid().find(giver) {
                distance_sq(me, target.x, target.y, target.z)
                    <= INTERACTION_RANGE_YD * INTERACTION_RANGE_YD
            } else if let Some(target) = ctx.db.game_gameobject().guid().find(giver) {
                distance_sq(me, target.x, target.y, target.z)
                    <= INTERACTION_RANGE_YD * INTERACTION_RANGE_YD
            } else {
                false
            }
        }
        QuestPlan::LootCreature { corpse, .. } => ctx
            .db
            .game_world_entity()
            .guid()
            .find(corpse)
            .is_some_and(|target| {
                distance_sq(me, target.x, target.y, target.z)
                    <= INTERACTION_RANGE_YD * INTERACTION_RANGE_YD
            }),
        QuestPlan::UseGameObject { gameobject, .. }
        | QuestPlan::LootGameObject { gameobject, .. } => ctx
            .db
            .game_gameobject()
            .guid()
            .find(gameobject)
            .is_some_and(|target| {
                distance_sq(me, target.x, target.y, target.z)
                    <= INTERACTION_RANGE_YD * INTERACTION_RANGE_YD
            }),
    };
    if !in_interaction_range {
        let approach = match plan {
            QuestPlan::Accept { giver, .. } | QuestPlan::TurnIn { giver, .. } => {
                if ctx.db.game_world_entity().guid().find(giver).is_some() {
                    MoveTarget::Entity(giver)
                } else {
                    MoveTarget::GameObject(giver)
                }
            }
            QuestPlan::LootCreature { corpse, .. } => MoveTarget::Entity(corpse),
            QuestPlan::UseGameObject { gameobject, .. }
            | QuestPlan::LootGameObject { gameobject, .. } => MoveTarget::GameObject(gameobject),
            QuestPlan::Attack { .. } | QuestPlan::Wait(_) => {
                unreachable!("attack and wait plans do not need an interaction approach")
            }
        };
        selected
            .prerequisites
            .insert(0, node(Action::Move(approach), objective));
    }
    if away
        && matches!(
            plan,
            QuestPlan::Wait(
                WaitReason::MissingTarget | WaitReason::ReadLimit | WaitReason::Respawn
            )
        )
    {
        selected.prerequisites.insert(0, travel);
    }
    Ok(selected)
}

pub(super) fn execute(ctx: &ReducerContext, actor: u64, action: Action) -> StepResult {
    let result = match action {
        Action::AcceptQuest(interaction) => {
            super::actions::accept_quest(ctx, actor, interaction.target, interaction.quest)
        }
        Action::TurnInQuest(interaction) => {
            super::actions::turn_in_quest(ctx, actor, interaction.target, interaction.quest, 0)
        }
        Action::LootCreature(loot) => {
            super::actions::open_creature_loot(ctx, actor, loot.target, loot.quest).and_then(|()| {
                super::actions::take_loot(ctx, actor, loot.target, loot.slot, loot.quest)
            })
        }
        Action::UseGameObject(interaction) => {
            super::actions::use_gameobject(ctx, actor, interaction.target, interaction.quest)
        }
        Action::LootGameObject(loot) => {
            super::actions::take_loot(ctx, actor, loot.target, loot.slot, loot.quest)
        }
        _ => return StepResult::Waiting,
    };
    match result {
        Ok(()) => StepResult::Completed,
        Err(refusal) => StepResult::Refused(refusal.kind),
    }
}

pub(super) fn wait_reason(plan: QuestPlan) -> Option<WaitReason> {
    match plan {
        QuestPlan::Wait(reason) => Some(reason),
        _ => None,
    }
}

pub(super) fn observe_safe_position(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    retained: &PlayerbotsQuestObjective,
) -> Option<RetainedPosition> {
    let source = retained.target.source.as_ref()?;
    let area = retained.work_area.as_ref()?;
    if (source.map_id, source.instance_id) != (me.map_id, me.instance_id)
        || (area.map_id, area.instance_id) != (me.map_id, me.instance_id)
    {
        return None;
    }
    let distance = distance_sq(me, source.x, source.y, source.z).sqrt();
    let margin = SAFE_APPROACH_MAX_YD;
    if !(SAFE_APPROACH_MIN_YD..=SAFE_APPROACH_MAX_YD).contains(&distance)
        || me.x < area.min_x - margin
        || me.x > area.max_x + margin
        || me.y < area.min_y - margin
        || me.y > area.max_y + margin
    {
        return None;
    }
    let safe = RetainedPosition {
        map_id: me.map_id,
        instance_id: me.instance_id,
        x: me.x,
        y: me.y,
        z: me.z,
    };
    let rows = ctx.db.pkg_playerbots_quest_objective();
    let mut current = rows.character_guid().find(me.guid)?;
    if current.runner_objective_identity != retained.runner_objective_identity {
        return None;
    }
    current.safe_position = Some(safe.clone());
    rows.character_guid().update(current);
    Some(safe)
}

pub(super) fn invalidate_safe_position(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    retained: &mut PlayerbotsQuestObjective,
) {
    let Some(safe) = retained.safe_position.as_ref() else {
        return;
    };
    if (safe.map_id, safe.instance_id) == (me.map_id, me.instance_id) {
        return;
    }
    let rows = ctx.db.pkg_playerbots_quest_objective();
    let Some(mut current) = rows.character_guid().find(me.guid) else {
        retained.safe_position = None;
        return;
    };
    if current.runner_objective_identity == retained.runner_objective_identity
        && current
            .safe_position
            .as_ref()
            .is_some_and(|safe| (safe.map_id, safe.instance_id) != (me.map_id, me.instance_id))
    {
        current.safe_position = None;
        rows.character_guid().update(current);
    }
    retained.safe_position = None;
}

pub(super) fn safe_position(
    me: &crate::WorldEntity,
    retained: &PlayerbotsQuestObjective,
) -> Option<RetainedPosition> {
    retained
        .safe_position
        .as_ref()
        .filter(|safe| (safe.map_id, safe.instance_id) == (me.map_id, me.instance_id))
        .cloned()
}
