use spacetimedb::{ReducerContext, Table};

use crate::tables::game_world_entity;

use super::{catalog, progression, queue, state::*};

fn number(text: Option<&str>) -> Result<u64, String> {
    let text = text.ok_or("Missing request field")?;
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("Invalid request number".into());
    }
    text.parse().map_err(|_| "Invalid request number".into())
}

pub(crate) fn handle(
    ctx: &ReducerContext,
    guid: u64,
    action: &str,
    payload: &str,
) -> Result<String, String> {
    let config = ctx
        .db
        .pkg_wave_defense_config()
        .id()
        .find(0)
        .filter(|config| config.enabled)
        .ok_or("Wave defense is not enabled")?;
    if matches!(action, "status" | "offer" | "roster") {
        if !payload.is_empty() {
            return Err("Read requests have no payload".into());
        }
        return match action {
            "offer" => progression::offer(ctx, guid),
            "roster" => roster(ctx, guid),
            _ => status(ctx, guid),
        };
    }
    let character = crate::actor::character_by_guid(ctx, guid).ok_or("Missing Character")?;
    let mut profile = ctx
        .db
        .pkg_wave_defense_profile()
        .character_guid()
        .find(guid)
        .unwrap_or(DefenseProfile {
            character_guid: guid,
            class: character.class,
            selected: false,
            last_request: 0,
            lobby_pending: false,
        });
    let mut parts = payload.split('|');
    let request = number(parts.next())?;
    if request == 0 || request < profile.last_request {
        return Err("Stale request; refresh status".into());
    }
    if request == profile.last_request {
        return status(ctx, guid);
    }
    match action {
        "select" => {
            let class = number(parts.next())?;
            if class != u64::from(character.class)
                || catalog::spec_for_class(character.class).is_none()
                || ctx
                    .db
                    .pkg_wave_defense_member()
                    .character_guid()
                    .find(guid)
                    .is_some()
            {
                return Err("Select this Character's specialization in the lobby".into());
            }
            profile.selected = true;
        }
        "queue" => queue::join(ctx, guid, &config)?,
        "choose" => {
            let match_id = number(parts.next())?;
            let revision =
                u32::try_from(number(parts.next())?).map_err(|_| "Invalid offer revision")?;
            let choice = u32::try_from(number(parts.next())?).map_err(|_| "Invalid choice")?;
            progression::choose(ctx, guid, match_id, revision, choice)?;
        }
        "ready" => {
            let match_id = number(parts.next())?;
            let wave = number(parts.next())?;
            let mut member = ctx
                .db
                .pkg_wave_defense_member()
                .character_guid()
                .find(guid)
                .ok_or("No active match")?;
            let run = ctx
                .db
                .pkg_wave_defense_match()
                .id()
                .find(member.match_id)
                .ok_or("Match ended")?;
            if run.id != match_id || u64::from(run.wave) != wave || run.phase != Phase::Preparation
            {
                return Err("This preparation has ended".into());
            }
            member.ready = true;
            ctx.db
                .pkg_wave_defense_member()
                .character_guid()
                .update(member);
        }
        "leave" => super::round::leave(ctx, guid, number(parts.next())?, &config)?,
        _ => return Err("Unknown wave defense action".into()),
    }
    if parts.next().is_some() {
        return Err("Too many request fields".into());
    }
    profile.last_request = request;
    let profiles = ctx.db.pkg_wave_defense_profile();
    if profiles.character_guid().find(guid).is_some() {
        profiles.character_guid().update(profile);
    } else {
        profiles.insert(profile);
    }
    status(ctx, guid)
}

pub(crate) fn status(ctx: &ReducerContext, guid: u64) -> Result<String, String> {
    let character = crate::actor::character_by_guid(ctx, guid).ok_or("Missing Character")?;
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    let last = ctx
        .db
        .pkg_wave_defense_profile()
        .character_guid()
        .find(guid)
        .map_or(0, |row| row.last_request);
    let gold = ctx
        .db
        .game_world_entity()
        .guid()
        .find(guid)
        .map_or(character.money, |body| body.money);
    if let Some(member) = ctx.db.pkg_wave_defense_member().character_guid().find(guid) {
        if let Some(run) = ctx.db.pkg_wave_defense_match().id().find(member.match_id) {
            let enemies = ctx
                .db
                .pkg_wave_defense_enemy()
                .by_match()
                .filter(&run.id)
                .count();
            let ready = ctx
                .db
                .pkg_wave_defense_member()
                .by_match()
                .filter(&run.id)
                .filter(|member| member.active() && member.ready)
                .count();
            let deadline = if member.respawn_micros > now {
                member.respawn_micros
            } else if run.phase != Phase::Combat {
                run.deadline_micros
            } else {
                now
            };
            let seconds = deadline.saturating_sub(now).max(0).saturating_add(999_999) / 1_000_000;
            return Ok(format!(
                "v1|{}|{}|{}|{}|{}|100|{}|10000|{}|{}|{}|{}|{}|{}|{}|0|{}|{}",
                run.phase.name(),
                run.id,
                run.wave,
                run.level,
                run.team_xp % 100,
                run.base_health,
                enemies,
                seconds,
                gold,
                progression::pending(&member, run.level),
                ready,
                u8::from(member.ready),
                character.class,
                last,
                member.offer_revision
            ));
        }
    }
    let queued = ctx.db.pkg_wave_defense_queue().character_guid().find(guid);
    let phase = if queued.is_some() { "queued" } else { "lobby" };
    let seconds = queued.map_or(0, |row| {
        now.saturating_sub(row.queued_micros).max(0) / 1_000_000
    });
    Ok(format!(
        "v1|{phase}|0|0|1|0|100|10000|10000|0|0|{gold}|0|0|0|{}|{seconds}|{last}|0",
        character.class
    ))
}

fn roster(ctx: &ReducerContext, guid: u64) -> Result<String, String> {
    let mut reply = "v1".to_string();
    let members: Vec<_> =
        if let Some(member) = ctx.db.pkg_wave_defense_member().character_guid().find(guid) {
            ctx.db
                .pkg_wave_defense_member()
                .by_match()
                .filter(&member.match_id)
                .filter(DefenseMember::active)
                .map(|row| (row.character_guid, row.bot, row.ready))
                .collect()
        } else if let Some(party) =
            crate::actor::party_facts(ctx, guid).map_err(|_| "Party unavailable")?
        {
            party
                .members
                .iter()
                .map(|member| (member.character_guid, false, false))
                .collect()
        } else {
            vec![(guid, false, false)]
        };
    for (guid, bot, ready) in members.into_iter().take(5) {
        let Some(character) = crate::actor::character_by_guid(ctx, guid) else {
            continue;
        };
        let spec = catalog::spec_for_class(character.class).ok_or("Unsupported class")?;
        let role = match spec.role {
            catalog::Role::Tank => "tank",
            catalog::Role::Healer => "healer",
            catalog::Role::Damage => "damage",
        };
        reply.push_str(&format!(
            "|{guid}|{}|{role}|{}|{}",
            character.name,
            u8::from(bot),
            u8::from(ready)
        ));
    }
    Ok(reply)
}
