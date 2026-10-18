#![cfg(feature = "debug_reducers")]

use spacetimedb::{reducer, ReducerContext, Table};

use crate::tables::{
    game_active_taxi_flight, game_character, game_group, game_group_member,
    game_meeting_stone_party, game_meeting_stone_seeker, game_talent, ActiveTaxiFlight,
    MeetingStoneParty, MeetingStoneSeeker,
};

use super::{catalog, progression, state::*, PACKAGE};

#[reducer]
pub fn wave_defense_fixture_eligibility(
    ctx: &ReducerContext,
    guid: u64,
    condition: String,
) -> Result<(), String> {
    crate::package::require_operator(ctx)?;
    let group = crate::actor::group_of(ctx, guid).ok_or("Fixture requires a premade")?;
    match condition.as_str() {
        "raid" => {
            let mut row = ctx
                .db
                .game_group()
                .group_id()
                .find(group.group_id)
                .ok_or("Party missing")?;
            row.group_type = 1;
            ctx.db.game_group().group_id().update(row);
        }
        "meeting-party" => {
            ctx.db.game_meeting_stone_party().insert(MeetingStoneParty {
                group_id: group.group_id,
                area_id: 0,
                team: 0,
                queued_at: ctx.timestamp,
                next_reminder_at: ctx.timestamp,
            });
        }
        "meeting-seeker" => {
            ctx.db
                .game_meeting_stone_seeker()
                .insert(MeetingStoneSeeker {
                    character_guid: guid,
                    area_id: 0,
                    team: 0,
                    class: 1,
                    group_id: group.group_id,
                    queued_at: ctx.timestamp,
                });
        }
        "taxi" => {
            ctx.db.game_active_taxi_flight().insert(ActiveTaxiFlight {
                character_guid: guid,
                path_id: 0,
                source_node_id: 0,
                destination_node_id: 0,
                mount_display_id: 0,
                fare: 0,
                current_node_index: 0,
                started_micros: ctx.timestamp.to_micros_since_unix_epoch(),
            });
        }
        "offline" => {
            let mut row = crate::actor::character_by_guid(ctx, guid).ok_or("Character missing")?;
            row.online = false;
            ctx.db.game_character().guid().update(row);
        }
        "partition" | "level" | "health" => {
            let mut body = crate::actor::live_entity(ctx, guid)?;
            match condition.as_str() {
                "partition" => body.instance_id = 0,
                "level" => body.level = 2,
                _ => body.health = 0,
            }
            body.update(ctx);
        }
        "pending" | "unselected" => {
            let mut row = ctx
                .db
                .pkg_wave_defense_profile()
                .character_guid()
                .find(guid)
                .ok_or("Profile missing")?;
            if condition == "pending" {
                row.lobby_pending = true;
            } else {
                row.selected = false;
            }
            ctx.db
                .pkg_wave_defense_profile()
                .character_guid()
                .update(row);
        }
        "membership" => {
            ctx.db
                .game_group_member()
                .insert(crate::tables::GroupMember { id: 0, ..group });
        }
        _ => return Err("Unknown eligibility fixture condition".into()),
    }
    Ok(())
}

#[reducer]
pub fn wave_defense_fixture_queue_tick(ctx: &ReducerContext) -> Result<(), String> {
    crate::package::require_operator(ctx)?;
    let config = ctx
        .db
        .pkg_wave_defense_config()
        .id()
        .find(0)
        .ok_or("Configuration missing")?;
    super::queue::tick(ctx, &config)
}

#[reducer]
pub fn wave_defense_fixture_reward_level(
    ctx: &ReducerContext,
    guid: u64,
    level: u8,
    talent_spell: u32,
) -> Result<(), String> {
    crate::package::require_operator(ctx)?;
    if !(1..=60).contains(&level) {
        return Err("Fixture level must be 1 through 60".into());
    }
    let mut member = ctx
        .db
        .pkg_wave_defense_member()
        .character_guid()
        .find(guid)
        .ok_or("Member missing")?;
    let mut run = ctx
        .db
        .pkg_wave_defense_match()
        .id()
        .find(member.match_id)
        .ok_or("Match missing")?;
    run.level = level;
    ctx.db.pkg_wave_defense_match().id().update(run);
    progression::refresh_level(ctx, &member, level)?;
    member.offer_ids.clear();
    progression::prepare_offer(ctx, &mut member, level)?;
    if talent_spell != 0 {
        let spec = catalog::spec_for_class(member.class).ok_or("Class missing")?;
        let talent = ctx
            .db
            .game_talent()
            .by_tab()
            .filter(&spec.talent_tree)
            .find(|talent| talent.spell_id == talent_spell || talent.grant_spell_id == talent_spell)
            .ok_or("Talent missing")?;
        member.offer_ids = vec![1_000_000 + talent.talent_id];
    }
    ctx.db
        .pkg_wave_defense_member()
        .character_guid()
        .update(member);
    Ok(())
}

#[reducer]
pub fn wave_defense_fixture_reward_class(
    ctx: &ReducerContext,
    guid: u64,
    class: u8,
) -> Result<(), String> {
    crate::package::require_operator(ctx)?;
    catalog::spec_for_class(class).ok_or("Unsupported fixture class")?;
    let mut character = crate::actor::character_by_guid(ctx, guid).ok_or("Character missing")?;
    character.class = class;
    ctx.db.game_character().guid().update(character);
    let mut body = crate::actor::live_entity(ctx, guid)?;
    body.unit_bytes_0 = (body.unit_bytes_0 & !0xFF00) | (u32::from(class) << 8);
    let instance_id = body.instance_id;
    body.update(ctx);
    if let Some(mut member) = ctx.db.pkg_wave_defense_member().character_guid().find(guid) {
        member.class = class;
        member.acquired.clear();
        member.choices_spent = 0;
        member.offer_ids.clear();
        crate::package::character::reset_loadout(ctx, guid)?;
        let spec = catalog::spec_for_class(class).ok_or("Unsupported fixture class")?;
        crate::package::combat::enable(ctx, PACKAGE, guid, instance_id, spec.essential_spells)?;
        ctx.db
            .pkg_wave_defense_member()
            .character_guid()
            .update(member);
    }
    Ok(())
}
