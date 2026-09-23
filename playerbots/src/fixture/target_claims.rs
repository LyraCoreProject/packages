//! Target selection through the ordinary Runner on a fresh, private Shard.

use super::super::recovery::Work;
use super::super::runner::{Controller, RunnerOutcome};
use super::*;
use crate::import_meta::game_import_meta; // package-api: exempt private fixture refuses imported content

const ENTRY: u32 = 5_098_095;

fn fight(ctx: &ReducerContext, guid: u64) -> Option<u64> {
    let state = ctx.db.pkg_playerbots_runner().character_guid().find(guid)?;
    match state.recovery?.active? {
        Work::Fight(target) => Some(target),
        _ => None,
    }
}

fn available(ctx: &ReducerContext, guid: u64, target: u64) -> Result<bool, String> {
    let me = crate::helpers::live_entity(ctx, guid)?;
    Ok(matches!(
        super::super::quest_loop::live_creature_target(ctx, &me, ENTRY, |_| true),
        super::super::quest_loop::LiveCreatureTarget::Found(found) if found.guid == target
    ))
}

#[reducer]
pub fn playerbots_fixture_solo_target_claim(
    ctx: &ReducerContext,
    case: String,
) -> Result<(), String> {
    crate::helpers::require_operator(ctx)?;
    if ctx
        .db
        .game_import_meta()
        .iter()
        .take(2)
        .any(|row| row.family != "weather_seed")
        || ctx.db.pkg_playerbots_bot().iter().next().is_some()
    {
        return Err("target claim fixture requires a fresh, private Shard".into());
    }
    super::super::playerbots_spawn_class_role(ctx, 2, 1200.0, 1200.0, 50.0, 1, 0)?;
    playerbots_fixture_prepare(ctx)?;
    let bots: Vec<_> = ctx.db.pkg_playerbots_bot().iter().take(2).collect();
    let (owner, other) = (bots[0].character_guid, bots[1].character_guid);
    for bot in &bots {
        playerbots_fixture_runner_stage(ctx, bot.character_guid, false)?;
        playerbots_fixture_runner_select_cohort(ctx, bot.character_guid)?;
        let mut bot = ctx
            .db
            .pkg_playerbots_bot()
            .id()
            .find(bot.id)
            .ok_or("bot missing")?;
        bot.home_x = 1200.0;
        let guid = bot.character_guid;
        ctx.db.pkg_playerbots_bot().id().update(bot);
        let mut me = crate::helpers::live_entity(ctx, guid)?;
        me.level = 1;
        me.power = 0;
        ctx.db.game_world_entity().guid().update(me);
        use super::super::pkg_playerbots_provisioning;
        if let Some(mut profile) = ctx
            .db
            .pkg_playerbots_provisioning()
            .character_guid()
            .find(guid)
        {
            profile.armed_level = 1;
            profile.next_repair_micros = i64::MAX;
            ctx.db
                .pkg_playerbots_provisioning()
                .character_guid()
                .update(profile);
        }
    }
    let target = companion_creature(ctx, ENTRY, 1240.0, 1200.0, 50.0, None)?;
    let mut entity = crate::helpers::live_entity(ctx, target)?;
    entity.level = 1;
    entity.health = 1000;
    entity.max_health = 1000;
    ctx.db.game_world_entity().guid().update(entity);
    playerbots_fixture_runner_pass_once(ctx, owner)?;
    if fight(ctx, owner) != Some(target) {
        let state = ctx.db.pkg_playerbots_runner().character_guid().find(owner);
        return Err(format!(
            "first bot did not retain its approach: {:?}",
            state.map(|s| (s.chosen, s.last_outcome))
        ));
    }
    if available(ctx, other, target)? {
        return Err("another solo bot selected the claimed creature before combat".into());
    }
    if !matches!(
        crate::loot::tag::live_loot_tag_eligibility(ctx, target, other),
        crate::loot::tag::LiveLootTagEligibility::Available
    ) {
        return Err("fixture already had a Loot Tag, so it did not prove an approach claim".into());
    }

    match case.as_str() {
        "backfill" => {
            let rows = ctx.db.pkg_playerbots_runner();
            let mut state = rows.character_guid().find(owner).ok_or("runner missing")?;
            state.solo_target_guid = u64::MAX;
            rows.character_guid().update(state);
            let me = crate::helpers::live_entity(ctx, other)?;
            if !matches!(
                super::super::target_claims::TargetClaims::read(ctx, me.guid)
                    .availability(ctx, target),
                super::super::target_claims::Availability::ReadLimit
            ) {
                return Err("new claims were allowed before backfill".into());
            }
            if !available(ctx, owner, target)? {
                return Err("backfill interrupted an existing approach".into());
            }
            // The claimant remains parked; the other bot is due first after publishing.
            playerbots_fixture_runner_pass_once(ctx, other)?;
            if fight(ctx, other).is_some()
                || rows
                    .character_guid()
                    .find(owner)
                    .ok_or("runner missing")?
                    .solo_target_guid
                    != target
            {
                return Err("another bot acquired the pre-publish owner's creature".into());
            }
            return Ok(());
        }
        "split" | "crowd" => {
            if case == "crowd" {
                super::super::playerbots_spawn_class_role(ctx, 34, 1200.0, 1200.0, 50.0, 1, 0)?;
                let guids: Vec<_> = ctx
                    .db
                    .pkg_playerbots_bot()
                    .iter()
                    .filter(|bot| bot.character_guid != owner && bot.character_guid != other)
                    .map(|bot| bot.character_guid)
                    .collect();
                for guid in guids {
                    super::super::runner::transition_controller(ctx, guid, Controller::Frozen)?;
                    runner_park_for(ctx, guid)?;
                }
            }
            let second = target + 1;
            let mut spawn = ctx
                .db
                .game_creature_spawn()
                .guid()
                .find(target)
                .ok_or("spawn missing")?;
            spawn.guid = second;
            spawn.x = 1245.0;
            let template = ctx
                .db
                .game_creature_template()
                .entry()
                .find(ENTRY)
                .ok_or("template missing")?;
            let spawn = ctx.db.game_creature_spawn().insert(spawn);
            let mut entity = crate::creatures::build_creature_entity(&spawn, &template, 0, 0);
            entity.level = 1;
            entity.health = 1000;
            entity.max_health = 1000;
            crate::creatures::insert_creature_entity(ctx, entity);
            playerbots_fixture_runner_pass_once(ctx, other)?;
            if fight(ctx, other) != Some(second) || fight(ctx, owner) != Some(target) {
                return Err(
                    "solo adventurers did not retain distinct creatures of the same type".into(),
                );
            }
            playerbots_fixture_runner_pass_once(ctx, owner)?;
            if fight(ctx, owner) != Some(target) {
                return Err("owner changed targets while its approach was still valid".into());
            }
            return Ok(());
        }
        "wait" => {
            playerbots_fixture_runner_pass_once(ctx, other)?;
            if fight(ctx, other).is_some() {
                return Err("solo bot piled onto the only claimed creature".into());
            }
            return Ok(());
        }
        "defense" => {
            playerbots_fixture_runner_damage_and_park(ctx, other, target, 1)?;
            playerbots_fixture_runner_pass_once(ctx, other)?;
            if fight(ctx, other) != Some(target) {
                return Err("solo claim prevented self-defense".into());
            }
            if !available(ctx, owner, target)? {
                return Err("self-defense displaced the original claimant".into());
            }
            return Ok(());
        }
        "owner_defense" => {
            playerbots_fixture_runner_damage_and_park(ctx, owner, target, 1)?;
            playerbots_fixture_runner_pass_once(ctx, owner)?;
            if fight(ctx, owner) != Some(target) || available(ctx, other, target)? {
                return Err("the original claim disappeared when its owner defended itself".into());
            }
            return Ok(());
        }
        "party" | "party_owner" => {
            let members = vec![owner, other];
            let partitions = fixture_group_partitions(ctx, ROLES_GROUP, &members)?;
            crate::group::sync_group_mirror(
                ctx,
                ROLES_GROUP,
                owner,
                0,
                2,
                0,
                members,
                crate::SessionActor {
                    guid: owner,
                    ownership: None,
                },
                partitions,
                1,
            )?;
            if case == "party_owner" {
                // The observer leaves; the existing owner's grouped state must release its claim.
                let row = ctx
                    .db
                    .game_group_member()
                    .by_character()
                    .filter(other)
                    .next()
                    .ok_or("member missing")?;
                ctx.db.game_group_member().id().delete(row.id);
            }
        }
        "death" => {
            let mut me = crate::helpers::live_entity(ctx, owner)?;
            me.dead = true;
            me.health = 0;
            ctx.db.game_world_entity().guid().update(me);
        }
        "freeze" => super::super::runner::transition_controller(ctx, owner, Controller::Frozen)?,
        "partition" => {
            let mut me = crate::helpers::live_entity(ctx, owner)?;
            me.instance_id = 1;
            ctx.db.game_world_entity().guid().update(me);
        }
        "expired" | "stalled" | "progress" | "abandoned" | "refused" => {
            let rows = ctx.db.pkg_playerbots_runner();
            let mut state = rows.character_guid().find(owner).ok_or("runner missing")?;
            if case == "refused" {
                state.last_outcome =
                    RunnerOutcome::Refused(super::super::runner::Failure::NoMovement);
            } else {
                let recovery = state.recovery.as_mut().ok_or("recovery missing")?;
                if case == "abandoned" {
                    recovery.active = None;
                }
                let attempt = recovery
                    .attempts
                    .iter_mut()
                    .find(|a| a.work == Work::Fight(target))
                    .ok_or("attempt missing")?;
                if case == "expired" {
                    attempt.last_observed_micros -= 30_000_000;
                }
                if case == "stalled" {
                    attempt.stalled_micros = 30_000_000;
                }
                if case == "progress" {
                    // An old approach remains claimed after observed progress, independent of its start.
                    if let Some(foreground) = state.foreground.as_mut() {
                        foreground.started_micros -= 60_000_000;
                    }
                    attempt.last_observed_micros = ctx.timestamp.to_micros_since_unix_epoch();
                    attempt.stalled_micros = 0;
                }
            }
            rows.character_guid().update(state);
            if case == "progress" {
                if available(ctx, other, target)? {
                    return Err("progressing approach lost its claim".into());
                }
                return Ok(());
            }
        }
        _ => return Err("unknown target claim case".into()),
    }
    if !available(ctx, other, target)? {
        return Err(format!("claim was not released after {case}"));
    }
    Ok(())
}
