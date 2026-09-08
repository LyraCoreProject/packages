//! Bounded action observations. Acceptance never implies damage or quest credit.

use crate::spell::{CastFinish, CastHandle, CastRefusal, CastStart};
use spacetimedb::{table, ReducerContext, Table};

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    Cast,
    Attack,
    Move,
    AcceptQuest,
    TurnInQuest,
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug)]
pub enum ActionOutcome {
    AttackAccepted(crate::combat::AttackStart),
    Completed,
    Movement(MovementObservation),
    CastResolved,
    Waiting(CastHandle),
    Refused(crate::actor::ActionRefusal),
    CastRefused(CastRefusal),
    Cancelled,
    Expired,
}

/// At most one observation per action kind per Character. Repeated waiting retains its start.
#[table(accessor = pkg_playerbots_action, public,
    index(accessor = by_character, btree(columns = [character_guid])))]
pub struct PlayerbotsAction {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub character_guid: u64,
    pub kind: ActionKind,
    pub target_guid: u64,
    pub spell_id: u32,
    pub quest_entry: u32,
    pub cast_id: u64,
    pub outcome: ActionOutcome,
    pub started_micros: i64,
    pub observed_micros: i64,
}

crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_action(ctx, character_guid) {
    let rows = ctx.db.pkg_playerbots_action();
    for row in rows.by_character().filter(character_guid).collect::<Vec<_>>() {
        rows.id().delete(row.id);
    }
});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_playerbots_action());

pub(super) fn observation(
    ctx: &ReducerContext,
    guid: u64,
    kind: ActionKind,
) -> Option<PlayerbotsAction> {
    ctx.db
        .pkg_playerbots_action()
        .by_character()
        .filter(guid)
        .find(|r| r.kind == kind)
}

fn record(ctx: &ReducerContext, mut row: PlayerbotsAction) {
    let rows = ctx.db.pkg_playerbots_action();
    if let Some(before) = observation(ctx, row.character_guid, row.kind) {
        row.id = before.id;
        if row.cast_id != 0 && row.cast_id == before.cast_id {
            row.started_micros = before.started_micros;
        }
        rows.id().update(row);
    } else {
        rows.insert(row);
    }
}

pub(super) fn attack(
    ctx: &ReducerContext,
    guid: u64,
    target_guid: u64,
) -> Result<crate::combat::AttackStart, crate::actor::ActionRefusal> {
    let result = crate::actor::request_attack(ctx, guid, target_guid);
    let outcome = match &result {
        Ok(accepted) => ActionOutcome::AttackAccepted(*accepted),
        Err(reason) => ActionOutcome::Refused(reason.clone()),
    };
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    record(
        ctx,
        PlayerbotsAction {
            id: 0,
            character_guid: guid,
            kind: ActionKind::Attack,
            target_guid,
            quest_entry: 0,
            spell_id: 0,
            cast_id: 0,
            outcome,
            started_micros: now,
            observed_micros: now,
        },
    );
    result
}

pub(super) fn cast(
    ctx: &ReducerContext,
    guid: u64,
    spell_id: u32,
    target_guid: u64,
) -> Result<CastStart, CastRefusal> {
    let result = crate::actor::request_cast(ctx, guid, spell_id, target_guid);
    let (outcome, cast_id, spell_id, target_guid) = match &result {
        Ok(CastStart::Started(handle) | CastStart::Waiting(handle)) => (
            ActionOutcome::Waiting(handle.clone()),
            handle.scheduled_id,
            handle.spell_id,
            handle.target_guid,
        ),
        Ok(CastStart::Resolved) => (ActionOutcome::CastResolved, 0, spell_id, target_guid),
        Err(reason) => (
            ActionOutcome::CastRefused(reason.clone()),
            0,
            spell_id,
            target_guid,
        ),
    };
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    record(
        ctx,
        PlayerbotsAction {
            id: 0,
            character_guid: guid,
            kind: ActionKind::Cast,
            target_guid,
            quest_entry: 0,
            spell_id,
            cast_id,
            outcome,
            started_micros: now,
            observed_micros: now,
        },
    );
    result
}

crate::game_hook!(on_cast_finished, fn playerbots_cast_finished(ctx, payload) {
    let Some(mut row) = observation(ctx, payload.caster_guid, ActionKind::Cast) else { return; };
    if row.cast_id != payload.scheduled_id { return; }
    row.outcome = match &payload.outcome {
        CastFinish::Resolved => ActionOutcome::CastResolved,
        CastFinish::Refused(reason) => ActionOutcome::CastRefused(reason.clone()),
        CastFinish::Cancelled => ActionOutcome::Cancelled,
        CastFinish::Expired => ActionOutcome::Expired,
    };
    row.observed_micros = ctx.timestamp.to_micros_since_unix_epoch();
    ctx.db.pkg_playerbots_action().id().update(row);
});

fn interaction(
    ctx: &ReducerContext,
    guid: u64,
    giver: u64,
    quest: u32,
    kind: ActionKind,
    result: Result<(), crate::actor::ActionRefusal>,
) -> Result<(), crate::actor::ActionRefusal> {
    let outcome = match &result {
        Ok(()) => ActionOutcome::Completed,
        Err(reason) => ActionOutcome::Refused(reason.clone()),
    };
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    record(
        ctx,
        PlayerbotsAction {
            id: 0,
            character_guid: guid,
            kind,
            target_guid: giver,
            spell_id: 0,
            quest_entry: quest,
            cast_id: 0,
            outcome,
            started_micros: now,
            observed_micros: now,
        },
    );
    result
}

pub(super) fn accept_quest(
    ctx: &ReducerContext,
    guid: u64,
    giver: u64,
    quest: u32,
) -> Result<(), crate::actor::ActionRefusal> {
    interaction(
        ctx,
        guid,
        giver,
        quest,
        ActionKind::AcceptQuest,
        crate::actor::request_accept_quest(ctx, guid, giver, quest),
    )
}

pub(super) fn turn_in_quest(
    ctx: &ReducerContext,
    guid: u64,
    giver: u64,
    quest: u32,
    reward: u32,
) -> Result<(), crate::actor::ActionRefusal> {
    interaction(
        ctx,
        guid,
        giver,
        quest,
        ActionKind::TurnInQuest,
        crate::actor::request_turn_in_quest(ctx, guid, giver, quest, reward),
    )
}

/// Planning evidence plus progress measured at the next request, never at a proposed endpoint.
#[derive(spacetimedb::SpacetimeType, Clone, Debug)]
pub struct MovementObservation {
    pub map_id: u32,
    pub instance_id: u64,
    pub destination: crate::nav::RoutePoint,
    pub route: crate::nav::RouteStep,
    pub arrived: bool,
    pub last_advance_micros: Option<i64>,
}

pub(super) fn movement(
    ctx: &ReducerContext,
    guid: u64,
    map_id: u32,
    instance_id: u64,
    destination: crate::nav::RoutePoint,
    arrived: bool,
    route: crate::nav::RouteStep,
) {
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    let mut last_advance_micros = None;
    if let Some(before) = observation(ctx, guid, ActionKind::Move) {
        if let ActionOutcome::Movement(previous) = before.outcome {
            if previous.map_id == map_id
                && previous.instance_id == instance_id
                && previous.destination == destination
            {
                let dx = route.from.x - previous.route.from.x;
                let dy = route.from.y - previous.route.from.y;
                last_advance_micros = if dx * dx + dy * dy > 0.05 * 0.05 {
                    Some(now)
                } else {
                    previous.last_advance_micros
                };
            }
        }
    }
    let outcome = ActionOutcome::Movement(MovementObservation {
        map_id,
        instance_id,
        destination,
        route,
        arrived,
        last_advance_micros,
    });
    record(
        ctx,
        PlayerbotsAction {
            id: 0,
            character_guid: guid,
            kind: ActionKind::Move,
            target_guid: 0,
            spell_id: 0,
            quest_entry: 0,
            cast_id: 0,
            outcome,
            started_micros: now,
            observed_micros: now,
        },
    );
}
