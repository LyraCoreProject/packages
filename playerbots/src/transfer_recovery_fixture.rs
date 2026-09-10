#![cfg(feature = "debug_reducers")]

use super::actions::pkg_playerbots_action;
use super::runner::{CompanionTransferPurpose, TransferCheckpoint, TransferPurpose};
use super::{pkg_playerbots_bot, pkg_playerbots_runner};
use crate::game_creature_spline;
use spacetimedb::{reducer, ReducerContext};

const STALLED_MICROS: i64 = 30_000_000;
const DEFERRED_MICROS: i64 = 5_000_000;

/// Stage the exact compact Recovery state imported after a successful Companion Transfer.
///
/// The fixture supplies the arrival boundary. Ordinary Runner passes must restore or retain this
/// state from current party facts.
#[reducer]
pub fn playerbots_transfer_recovery_fixture_stage_arrival(
    ctx: &ReducerContext,
    character_guid: u64,
    member_guid: u64,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    let bot = ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(character_guid)
        .next()
        .filter(|bot| {
            matches!(
                bot.controller,
                super::runner::Controller::Legacy | super::runner::Controller::Cohort
            )
        })
        .ok_or("Transfer Recovery fixture requires a Runner owner")?;
    let me = crate::helpers::live_entity(ctx, character_guid)?;
    let mut state = ctx
        .db
        .pkg_playerbots_runner()
        .character_guid()
        .find(character_guid)
        .ok_or("Transfer Recovery fixture Runner is absent")?;
    let objective = state
        .objective
        .as_ref()
        .filter(|objective| {
            objective.kind == super::runner::ObjectiveKind::Companion && objective.identity != 0
        })
        .ok_or("Transfer Recovery fixture Companion objective is absent")?;
    let party = super::companion::party(ctx, character_guid, false)
        .map_err(|failure| format!("Transfer Recovery fixture party is unavailable: {failure:?}"))?
        .ok_or("Transfer Recovery fixture party is absent")?;
    if !party
        .members
        .iter()
        .any(|member| member.character_guid == member_guid)
    {
        return Err("Transfer Recovery fixture member is outside the party".to_string());
    }
    let owned_actions: Vec<_> = ctx
        .db
        .pkg_playerbots_action()
        .by_character()
        .filter(character_guid)
        .take(10)
        .collect();
    if owned_actions.len() == 10 {
        return Err("Transfer Recovery fixture action read exceeds its bound".to_string());
    }
    let objective_identity = objective.identity;
    ctx.db.game_creature_spline().guid().delete(character_guid);
    for action in owned_actions {
        ctx.db.pkg_playerbots_action().id().delete(action.id);
    }
    state.foreground = None;
    state.chosen = None;
    state.candidate_order.clear();
    state.recovery = None;
    state.transfer_checkpoint = Some(TransferCheckpoint {
        intent_id: 509_110_001,
        controller_generation: state.generation,
        source_map: me.map_id,
        source_instance: me.instance_id,
        destination_map: me.map_id,
        destination_instance: me.instance_id,
        objective_identity,
        arrival_started_micros: ctx.timestamp.to_micros_since_unix_epoch(),
        purpose: Some(TransferPurpose::Companion(CompanionTransferPurpose {
            member_guid,
            stalled_micros: STALLED_MICROS,
            approach: 2,
            deferred_micros: DEFERRED_MICROS,
        })),
    });
    state.save(ctx);
    let mut bot = bot;
    bot.next_think_micros = i64::MAX;
    ctx.db.pkg_playerbots_bot().id().update(bot);
    Ok(())
}
