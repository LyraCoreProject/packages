use spacetimedb::{ReducerContext, Table};

use super::{catalog, progression, state::*, PACKAGE};

const REPLACEMENT_GRACE_MICROS: i64 = 15_000_000;

pub(crate) fn login(ctx: &ReducerContext, guid: u64) {
    let table = ctx.db.pkg_wave_defense_member();
    if let Some(mut member) = table
        .character_guid()
        .find(guid)
        .filter(|member| !member.bot)
    {
        member.resume_requested = true;
        table.character_guid().update(member);
    }
}

fn replacement(
    ctx: &ReducerContext,
    member: &DefenseMember,
) -> Result<Option<DefenseMember>, String> {
    let mut replacements = ctx
        .db
        .pkg_wave_defense_member()
        .by_match()
        .filter(member.match_id)
        .filter(|candidate| candidate.bot && candidate.replacement_for == member.character_guid);
    let first = replacements.next();
    if replacements.next().is_some() {
        return Err("A human match slot has more than one replacement bot".into());
    }
    Ok(first)
}

fn create_replacement(
    ctx: &ReducerContext,
    config: &DefenseConfig,
    run: &DefenseMatch,
    human: &DefenseMember,
    permanent: bool,
) -> Result<(), String> {
    let role = match catalog::spec_for_class(human.class)
        .ok_or("Unsupported replacement role")?
        .role
    {
        catalog::Role::Tank => 0,
        catalog::Role::Healer => 1,
        catalog::Role::Damage => 2,
    };
    let guid = crate::pkg_playerbots::spawn_wave_defense_bot(
        ctx,
        role,
        run.level,
        config.map_id,
        config.lobby_instance_id,
        (config.x, config.y, config.z),
    )?;
    crate::package::party::replace(ctx, PACKAGE, run.group_id, human.character_guid, guid)?;
    crate::package::instance::enter(
        ctx,
        PACKAGE,
        guid,
        run.instance_id,
        (config.x, config.y, config.z, config.orientation),
    )?;
    let character =
        crate::actor::character_by_guid(ctx, guid).ok_or("Replacement bot is unavailable")?;
    let member = DefenseMember {
        character_guid: guid,
        match_id: run.id,
        account_id: character.account_id,
        class: character.class,
        bot: true,
        replacement_for: if permanent { 0 } else { human.character_guid },
        disconnected_micros: 0,
        resume_requested: false,
        respawn_micros: 0,
        ready: true,
        choices_spent: 0,
        offer_revision: 0,
        offer_ids: vec![],
        acquired: vec![],
    };
    progression::start(ctx, &member, run.instance_id)?;
    progression::refresh_level(ctx, &member, run.level)?;
    crate::pkg_playerbots::set_wave_defense_objective(
        ctx,
        guid,
        (config.x, config.y, config.z),
        &[],
    )?;
    ctx.db.pkg_wave_defense_member().insert(member);
    Ok(())
}

fn resume(
    ctx: &ReducerContext,
    config: &DefenseConfig,
    run: &DefenseMatch,
    mut member: DefenseMember,
) -> Result<(), String> {
    let Some(character) = crate::actor::character_by_guid(ctx, member.character_guid)
        .filter(|character| character.online)
    else {
        return Ok(());
    };
    let Ok(body) = crate::actor::live_entity(ctx, character.guid) else {
        return Ok(());
    };
    let current_party = crate::actor::group_of(ctx, character.guid).map(|member| member.group_id);
    if body.map_id != config.map_id
        || current_party.is_some_and(|group_id| group_id != run.group_id)
    {
        member.resume_requested = false;
        ctx.db
            .pkg_wave_defense_member()
            .character_guid()
            .update(member);
        return Ok(());
    }
    let bot = replacement(ctx, &member)?;
    if let Some(bot) = &bot {
        crate::package::party::replace(
            ctx,
            PACKAGE,
            run.group_id,
            bot.character_guid,
            character.guid,
        )?;
    } else if current_party != Some(run.group_id) {
        member.resume_requested = false;
        ctx.db
            .pkg_wave_defense_member()
            .character_guid()
            .update(member);
        return Ok(());
    }
    if body.instance_id != run.instance_id {
        if body.dead {
            crate::package::character::respawn(
                ctx,
                character.guid,
                (config.x, config.y, config.z, config.orientation),
            )?;
        }
        crate::package::instance::enter(
            ctx,
            PACKAGE,
            character.guid,
            run.instance_id,
            (config.x, config.y, config.z, config.orientation),
        )?;
    }
    progression::refresh_level(ctx, &member, run.level)?;
    progression::start(ctx, &member, run.instance_id)?;
    if let Some(bot) = bot {
        crate::package::combat::disable(ctx, PACKAGE, bot.character_guid)?;
        crate::pkg_playerbots::remove_wave_defense_bot(ctx, bot.character_guid)?;
        ctx.db
            .pkg_wave_defense_member()
            .character_guid()
            .delete(bot.character_guid);
    }
    member.disconnected_micros = 0;
    member.resume_requested = false;
    member.ready = false;
    if !body.dead {
        member.respawn_micros = 0;
    }
    if run.phase == Phase::Preparation {
        progression::prepare_offer(ctx, &mut member, run.level)?;
    }
    ctx.db
        .pkg_wave_defense_member()
        .character_guid()
        .update(member);
    Ok(())
}

pub(crate) fn tick(ctx: &ReducerContext, config: &DefenseConfig) -> Result<(), String> {
    let now = ctx.timestamp.to_micros_since_unix_epoch();
    for run in ctx.db.pkg_wave_defense_match().iter().take(64) {
        let humans: Vec<_> = ctx
            .db
            .pkg_wave_defense_member()
            .by_match()
            .filter(run.id)
            .filter(|member| !member.bot)
            .collect();
        for mut member in humans {
            if member.resume_requested {
                resume(ctx, config, &run, member)?;
                continue;
            }
            let online = crate::actor::character_by_guid(ctx, member.character_guid)
                .is_some_and(|character| character.online);
            if online {
                continue;
            }
            if member.disconnected_micros == 0 {
                member.disconnected_micros = now;
                member.ready = false;
                ctx.db
                    .pkg_wave_defense_member()
                    .character_guid()
                    .update(member);
                continue;
            }
            if matches!(run.phase, Phase::Preparation | Phase::Combat)
                && now
                    >= member
                        .disconnected_micros
                        .saturating_add(REPLACEMENT_GRACE_MICROS)
                && replacement(ctx, &member)?.is_none()
            {
                create_replacement(ctx, config, &run, &member, false)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn leave(ctx: &ReducerContext, guid: u64, config: &DefenseConfig) -> Result<(), String> {
    let Some(member) = ctx.db.pkg_wave_defense_member().character_guid().find(guid) else {
        return Ok(());
    };
    if member.bot {
        return Err("A bot cannot request a human match departure".into());
    }
    let run = ctx
        .db
        .pkg_wave_defense_match()
        .id()
        .find(member.match_id)
        .ok_or("Match has ended")?;
    if !matches!(run.phase, Phase::Preparation | Phase::Combat) {
        return Ok(());
    }
    if let Some(mut bot) = replacement(ctx, &member)? {
        bot.replacement_for = 0;
        ctx.db
            .pkg_wave_defense_member()
            .character_guid()
            .update(bot);
    } else {
        create_replacement(ctx, config, &run, &member, true)?;
    }
    Ok(())
}
