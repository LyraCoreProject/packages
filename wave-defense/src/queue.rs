use std::collections::BTreeSet;

use spacetimedb::{ReducerContext, Table};

use crate::tables::{
    game_active_taxi_flight, game_group, game_group_member, game_meeting_stone_party,
    game_meeting_stone_seeker,
};

use super::{catalog, matching, progression, state::*, PACKAGE};

fn eligible_party(
    ctx: &ReducerContext,
    guid: u64,
    config: &DefenseConfig,
) -> Result<(u64, Vec<(u64, catalog::Role)>), String> {
    let group_id = crate::actor::group_of(ctx, guid).map_or(0, |member| member.group_id);
    let roster = if group_id == 0 {
        vec![guid]
    } else {
        let group = ctx
            .db
            .game_group()
            .group_id()
            .find(group_id)
            .ok_or("Premade Party is unavailable")?;
        if group.group_type != 0 {
            return Err("A Raid cannot queue for wave defense".into());
        }
        if ctx
            .db
            .game_meeting_stone_party()
            .group_id()
            .find(group_id)
            .is_some()
        {
            return Err("Leave the Meeting Stone Queue first".into());
        }
        let roster: Vec<_> = ctx
            .db
            .game_group_member()
            .by_group()
            .filter(&group_id)
            .take(6)
            .map(|member| member.character_guid)
            .collect();
        let distinct: BTreeSet<_> = roster.iter().copied().collect();
        if !(2..=5).contains(&roster.len())
            || distinct.len() != roster.len()
            || !distinct.contains(&group.leader_guid)
            || !distinct.contains(&guid)
        {
            return Err("Premade Party roster is unavailable".into());
        }
        roster
    };
    let mut roles = Vec::with_capacity(roster.len());
    for member_guid in roster {
        let character = crate::actor::character_by_guid(ctx, member_guid)
            .ok_or("Premade member unavailable")?;
        let body = crate::actor::live_entity(ctx, member_guid)?;
        let profile = ctx
            .db
            .pkg_wave_defense_profile()
            .character_guid()
            .find(member_guid)
            .ok_or("Choose a specialization first")?;
        if !profile.selected
            || profile.lobby_pending
            || profile.class != character.class
            || ctx
                .db
                .pkg_wave_defense_member()
                .character_guid()
                .find(member_guid)
                .is_some()
        {
            return Err("Choose a specialization in the lobby before queueing".into());
        }
        if !character.online
            || !body.is_player()
            || body.dead
            || body.health == 0
            || body.map_id != config.map_id
            || body.instance_id != config.lobby_instance_id
            || body.level != 1
        {
            return Err("Every premade member must be online in the level 1 lobby".into());
        }
        if ctx
            .db
            .game_active_taxi_flight()
            .character_guid()
            .find(member_guid)
            .is_some()
        {
            return Err("Finish the taxi flight before queueing".into());
        }
        if ctx
            .db
            .game_meeting_stone_seeker()
            .character_guid()
            .find(member_guid)
            .is_some()
        {
            return Err("Leave the Meeting Stone Queue first".into());
        }
        let memberships: Vec<_> = ctx
            .db
            .game_group_member()
            .by_character()
            .filter(&member_guid)
            .take(2)
            .collect();
        if memberships.len() != usize::from(group_id != 0)
            || memberships.iter().any(|member| member.group_id != group_id)
        {
            return Err("Premade Party membership is unavailable".into());
        }
        let spec = catalog::spec_for_class(character.class).ok_or("Unsupported class")?;
        roles.push((member_guid, spec.role));
    }
    if !matching::can_queue(&roles) {
        return Err("Your premade must fit one tank, one healer and three damage roles".into());
    }
    Ok((group_id, roles))
}

pub(crate) fn join(ctx: &ReducerContext, guid: u64, config: &DefenseConfig) -> Result<(), String> {
    crate::package::party::require_authority(ctx)?;
    let (group_id, _) = eligible_party(ctx, guid, config)?;
    let queued = ctx.db.pkg_wave_defense_queue();
    if let Some(existing) = queued.character_guid().find(guid) {
        if existing.group_id == group_id {
            return Ok(());
        }
        queued.character_guid().delete(guid);
    }
    queued.insert(DefenseQueue {
        character_guid: guid,
        group_id,
        queued_micros: ctx.timestamp.to_micros_since_unix_epoch(),
    });
    Ok(())
}

fn ready_parties(ctx: &ReducerContext, config: &DefenseConfig) -> Vec<matching::QueueParty> {
    let queue = ctx.db.pkg_wave_defense_queue();
    let mut parties = Vec::new();
    let mut seen = BTreeSet::new();
    let waiting: Vec<_> = queue.iter().take(1024).collect();
    for row in waiting {
        let Ok((group_id, members)) = eligible_party(ctx, row.character_guid, config) else {
            queue.character_guid().delete(row.character_guid);
            continue;
        };
        if group_id != row.group_id {
            queue.character_guid().delete(row.character_guid);
            continue;
        }
        if group_id != 0 && !seen.insert(group_id) {
            continue;
        }
        let mut queued_micros = row.queued_micros;
        let all_consented = members.iter().all(|(guid, _)| {
            let Some(consent) = queue.character_guid().find(guid) else {
                return false;
            };
            queued_micros = queued_micros.max(consent.queued_micros);
            consent.group_id == group_id
        });
        if all_consented {
            parties.push(matching::QueueParty {
                members,
                queued_micros,
            });
        }
    }
    parties
}

pub(crate) fn tick(ctx: &ReducerContext, config: &DefenseConfig) -> Result<(), String> {
    if ctx.db.pkg_wave_defense_match().count() >= 64 {
        return Ok(());
    }
    if ctx.db.pkg_wave_defense_queue().iter().next().is_none() {
        return Ok(());
    }
    crate::package::party::require_authority(ctx)?;
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    let Some(roster) = matching::next_match(&ready_parties(ctx, config), now) else {
        return Ok(());
    };
    let mut members = roster
        .humans
        .iter()
        .map(|&guid| (guid, false))
        .collect::<Vec<_>>();
    for role in roster.bot_roles {
        let role = match role {
            catalog::Role::Tank => 0,
            catalog::Role::Healer => 1,
            catalog::Role::Damage => 2,
        };
        let guid = crate::pkg_playerbots::spawn_wave_defense_bot(
            ctx,
            role,
            1,
            config.map_id,
            config.lobby_instance_id,
            (config.x, config.y, config.z),
        )?;
        members.push((guid, true));
    }
    let guids: Vec<_> = members.iter().map(|&(guid, _)| guid).collect();
    let group_id = crate::package::party::form(ctx, PACKAGE, &guids)?;
    let instance_id =
        crate::package::instance::create_empty(ctx, PACKAGE, config.map_id, group_id)?;
    let run = ctx.db.pkg_wave_defense_match().insert(DefenseMatch {
        id: 0,
        instance_id,
        group_id,
        phase: Phase::Preparation,
        wave: 1,
        level: 1,
        team_xp: 0,
        base_health: 10_000,
        deadline_micros: now + 30_000_000,
        wave_started_micros: 0,
        next_spawn_micros: 0,
        spawned: 0,
        defeated: 0,
        next_base_hit_micros: now,
        empty_human_micros: 0,
    });
    for (guid, bot) in members {
        if !bot {
            crate::package::character::reset_loadout(ctx, guid)?;
        }
        crate::package::instance::enter(
            ctx,
            PACKAGE,
            guid,
            instance_id,
            (config.x, config.y, config.z, config.orientation),
        )?;
        let character =
            crate::actor::character_by_guid(ctx, guid).ok_or("Match Character unavailable")?;
        let mut member = DefenseMember {
            character_guid: guid,
            match_id: run.id,
            account_id: character.account_id,
            class: character.class,
            bot,
            replacement_for: 0,
            disconnected_micros: 0,
            resume_requested: false,
            respawn_micros: 0,
            ready: bot,
            choices_spent: 0,
            offer_revision: 0,
            offer_ids: vec![],
            acquired: vec![],
        };
        progression::start(ctx, &member, instance_id)?;
        progression::prepare_offer(ctx, &mut member, 1)?;
        ctx.db.pkg_wave_defense_member().insert(member);
        ctx.db
            .pkg_wave_defense_queue()
            .character_guid()
            .delete(guid);
        if bot {
            crate::pkg_playerbots::set_wave_defense_objective(
                ctx,
                guid,
                (config.x, config.y, config.z),
                &[],
            )?;
        }
    }
    super::round::spawn_shops(ctx, config, &run)?;
    Ok(())
}
