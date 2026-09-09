//! Authenticated companion orders and their runner-facing policy.

use super::{pkg_playerbots_bot, Controller};
use crate::{game_world_entity, WorldEntity};
use spacetimedb::{table, ReducerContext, Table};

pub(crate) const FOLLOW: u8 = 0;
pub(crate) const STAY: u8 = 1;
pub(crate) const ASSIST: u8 = 2;
pub(crate) const TARGET: u8 = 3;
const HISTORY_LIMIT: usize = 8;

#[derive(spacetimedb::SpacetimeType, Clone, Debug, PartialEq)]
pub struct FollowOrder {
    pub leader_guid: u64,
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug, PartialEq)]
pub struct StayOrder {
    pub map_id: u32,
    pub instance_id: u64,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug, PartialEq)]
pub struct AssistOrder {
    pub member_guid: u64,
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug, PartialEq)]
pub struct TargetOrder {
    pub target_guid: u64,
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug, PartialEq)]
pub enum CompanionOrder {
    Follow(FollowOrder),
    Stay(StayOrder),
    Assist(AssistOrder),
    Target(TargetOrder),
}

#[derive(spacetimedb::SpacetimeType, Clone, Debug)]
pub struct CommandRecord {
    pub at_micros: i64,
    pub source_identity: spacetimedb::Identity,
    pub intent_id: u64,
    pub issuer_guid: u64,
    pub issuer_sequence: u64,
    pub order: CompanionOrder,
    pub outcome: crate::actor::CommandOutcome,
}

#[table(accessor = pkg_playerbots_companion_order, public)]
pub struct CompanionOrderState {
    #[primary_key]
    pub character_guid: u64,
    pub issuer_guid: u64,
    pub issuer_sequence: u64,
    pub group_id: u64,
    pub active: bool,
    pub revision: u64,
    pub order: CompanionOrder,
    pub last_outcome: crate::actor::CommandOutcome,
    pub history: Vec<CommandRecord>,
}

crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_companion_order(ctx, character_guid) {
    ctx.db.pkg_playerbots_companion_order().character_guid().delete(character_guid);
});

crate::character_owned!(transfer, fn sweep_transfer_pkg_playerbots_companion_order(ctx, character_guid, io) {
    table = pkg_playerbots_companion_order,
    primary_key = character_guid,
});

pub(crate) fn parse_command(
    cmd: &str,
    payload: &str,
) -> Option<Result<crate::actor::ParsedClientCommand, crate::actor::CommandOutcome>> {
    if cmd != "playerbots.order" {
        return None;
    }
    Some(parse_payload(payload).ok_or(crate::actor::CommandOutcome::Malformed))
}

fn parse_payload(payload: &str) -> Option<crate::actor::ParsedClientCommand> {
    let mut fields = payload.split('|');
    let operation = fields.next()?;
    let bot_guid = fields.next()?.parse().ok()?;
    let argument = fields.next().map(str::parse).transpose().ok()?.unwrap_or(0);
    if fields.next().is_some() || bot_guid == 0 {
        return None;
    }
    let (kind, authority_member_guid, exact_target_guid) = match operation {
        "follow" if argument == 0 => (FOLLOW, 0, 0),
        "stay" if argument == 0 => (STAY, 0, 0),
        "assist" if argument != 0 => (ASSIST, argument, 0),
        "target" if argument != 0 => (TARGET, 0, argument),
        _ => return None,
    };
    Some(crate::actor::ParsedClientCommand {
        kind,
        bot_guid,
        authority_member_guid,
        exact_target_guid,
    })
}

pub(crate) fn apply_command(
    ctx: &ReducerContext,
    admitted: &crate::actor::AdmittedClientCommand,
) -> crate::actor::CommandOutcome {
    let Some(bot) = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(admitted.command.bot_guid)
        .next()
    else {
        return crate::actor::CommandOutcome::MissingBot;
    };
    if bot.controller != Controller::Cohort {
        return crate::actor::CommandOutcome::Suppressed;
    }
    let order = match admitted.command.kind {
        FOLLOW => CompanionOrder::Follow(FollowOrder {
            leader_guid: admitted.leader_guid,
        }),
        STAY => {
            let Some(me) = ctx
                .db
                .game_world_entity()
                .guid()
                .find(admitted.command.bot_guid)
            else {
                return crate::actor::CommandOutcome::MissingBot;
            };
            stay_at(&me)
        }
        ASSIST => CompanionOrder::Assist(AssistOrder {
            member_guid: admitted.command.authority_member_guid,
        }),
        TARGET => CompanionOrder::Target(TargetOrder {
            target_guid: admitted.command.exact_target_guid,
        }),
        _ => return crate::actor::CommandOutcome::Malformed,
    };
    let states = ctx.db.pkg_playerbots_companion_order();
    let current = states.character_guid().find(admitted.command.bot_guid);
    if current.as_ref().is_some_and(|state| {
        state.issuer_guid == admitted.issuer_guid
            && state.issuer_sequence > admitted.issuer_sequence
    }) {
        return crate::actor::CommandOutcome::Superseded;
    }
    let outcome = if current.as_ref().is_some_and(|state| {
        state.active
            && state.issuer_guid == admitted.issuer_guid
            && state.group_id == admitted.group_id
            && state.order == order
    }) {
        crate::actor::CommandOutcome::Unchanged
    } else {
        crate::actor::CommandOutcome::Applied
    };
    let mut history = current
        .as_ref()
        .map(|state| state.history.clone())
        .unwrap_or_default();
    history.push(CommandRecord {
        at_micros: ctx.timestamp.to_micros_since_unix_epoch(),
        source_identity: admitted.source_identity,
        intent_id: admitted.intent_id,
        issuer_guid: admitted.issuer_guid,
        issuer_sequence: admitted.issuer_sequence,
        order: order.clone(),
        outcome,
    });
    if history.len() > HISTORY_LIMIT {
        history.drain(..history.len() - HISTORY_LIMIT);
    }
    let revision = current.as_ref().map_or(1, |state| {
        state
            .revision
            .saturating_add(u64::from(outcome == crate::actor::CommandOutcome::Applied))
    });
    let row = CompanionOrderState {
        character_guid: admitted.command.bot_guid,
        issuer_guid: admitted.issuer_guid,
        issuer_sequence: admitted.issuer_sequence,
        group_id: admitted.group_id,
        active: true,
        revision,
        order: current
            .as_ref()
            .filter(|_| outcome == crate::actor::CommandOutcome::Unchanged)
            .map_or(order, |state| state.order.clone()),
        last_outcome: outcome,
        history,
    };
    if current.is_some() {
        states.character_guid().update(row);
    } else {
        states.insert(row);
    }
    if outcome == crate::actor::CommandOutcome::Applied {
        let bots = ctx.db.pkg_playerbots_bot();
        let mut due = bot;
        due.next_think_micros = 0;
        bots.id().update(due);
    }
    outcome
}

fn stay_at(me: &WorldEntity) -> CompanionOrder {
    CompanionOrder::Stay(StayOrder {
        map_id: me.map_id,
        instance_id: me.instance_id,
        x: me.x,
        y: me.y,
        z: me.z,
    })
}

pub(crate) fn active(ctx: &ReducerContext, character_guid: u64) -> Option<CompanionOrderState> {
    ctx.db
        .pkg_playerbots_companion_order()
        .character_guid()
        .find(character_guid)
        .filter(|state| state.active)
}

pub(crate) fn clear(ctx: &ReducerContext, character_guid: u64) {
    let states = ctx.db.pkg_playerbots_companion_order();
    if let Some(mut state) = states.character_guid().find(character_guid) {
        if state.active {
            state.active = false;
            states.character_guid().update(state);
        }
    }
}

pub(crate) fn record_runtime_outcome(
    ctx: &ReducerContext,
    character_guid: u64,
    outcome: crate::actor::CommandOutcome,
) {
    let states = ctx.db.pkg_playerbots_companion_order();
    let Some(mut state) = states.character_guid().find(character_guid) else {
        return;
    };
    if !state.active || state.last_outcome == outcome {
        return;
    }
    let (source_identity, intent_id) = state
        .history
        .last()
        .map(|record| (record.source_identity, record.intent_id))
        .unwrap_or((spacetimedb::Identity::ZERO, 0));
    state.last_outcome = outcome;
    state.history.push(CommandRecord {
        at_micros: ctx.timestamp.to_micros_since_unix_epoch(),
        source_identity,
        intent_id,
        issuer_guid: state.issuer_guid,
        issuer_sequence: state.issuer_sequence,
        order: state.order.clone(),
        outcome,
    });
    if state.history.len() > HISTORY_LIMIT {
        state.history.drain(..state.history.len() - HISTORY_LIMIT);
    }
    states.character_guid().update(state);
}

crate::game_client_command!(parse_command, apply_command);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_accepts_only_the_four_complete_order_shapes() {
        assert_eq!(parse_payload("follow|7").unwrap().kind, FOLLOW);
        assert_eq!(parse_payload("stay|7").unwrap().kind, STAY);
        assert_eq!(
            parse_payload("assist|7|8").unwrap().authority_member_guid,
            8
        );
        assert_eq!(parse_payload("target|7|9").unwrap().exact_target_guid, 9);
        assert!(parse_payload("target|7|0").is_none());
        assert!(parse_payload("follow|7|8").is_none());
        assert!(parse_payload("stay|0").is_none());
    }
}
