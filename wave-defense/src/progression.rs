use lyracore_shared::constants::armor_proficiency::{
    MAIL_PASSIVE_SPELL_ID, PLATE_PASSIVE_SPELL_ID,
};
use lyracore_shared::spell::taxonomy::{A_FLAG, E_SCRIPTED, SPELL_ATTR_PASSIVE};
use spacetimedb::{ReducerContext, Table};

use crate::tables::{
    game_character, game_spell, game_spell_chain, game_spell_effect, game_talent, game_world_entity,
};

use super::{catalog, state::*, PACKAGE};

const TALENT_CHOICE: u32 = 1_000_000;

pub(crate) fn pending(member: &DefenseMember, level: u8) -> u8 {
    let earned = catalog::REWARD_LEVELS
        .iter()
        .filter(|&&milestone| milestone <= level)
        .count() as u8;
    earned.saturating_sub(member.choices_spent)
}

pub(crate) fn start(
    ctx: &ReducerContext,
    member: &DefenseMember,
    instance_id: u64,
) -> Result<(), String> {
    let spec = catalog::spec_for_class(member.class).ok_or("Unsupported class")?;
    let mut essentials = spec.essential_spells.to_vec();
    if member.bot {
        essentials.extend_from_slice(match member.class {
            1 => &[355, 6673],
            5 => &[2050, 1243],
            8 => &[133, 168],
            _ => &[],
        });
        essentials.sort_unstable();
        essentials.dedup();
    }
    crate::package::combat::enable(
        ctx,
        PACKAGE,
        member.character_guid,
        instance_id,
        &essentials,
    )?;
    for spell in essentials {
        crate::package::character::grant_spell(ctx, member.character_guid, spell)?;
    }
    if member.class == 3 {
        let config = ctx
            .db
            .pkg_wave_defense_config()
            .id()
            .find(0)
            .ok_or("Match configuration unavailable")?;
        crate::package::character::provision_hunter_pet(
            ctx,
            member.character_guid,
            config.pet_entry,
            true,
        )?;
    }
    if member.class == 7 {
        for item in [5175, 5176, 5177, 5178] {
            if crate::actor::item_count(ctx, member.character_guid, item) == 0 {
                crate::package::character::grant_item(ctx, member.character_guid, item, 1)?;
            }
        }
    }
    Ok(())
}

fn grant_ranks(ctx: &ReducerContext, guid: u64, base_spell: u32, level: u8) -> Result<(), String> {
    let base = ctx
        .db
        .game_spell()
        .spell_id()
        .find(base_spell)
        .ok_or("Spell is not imported")?;
    crate::package::character::grant_spell(ctx, guid, base_spell)?;
    for chain in ctx.db.game_spell_chain().by_first().filter(&base_spell) {
        if chain.spell_id == base_spell {
            continue;
        }
        if let Some(spell) = ctx.db.game_spell().spell_id().find(chain.spell_id) {
            if spell.spell_level <= level && spell.spell_level >= base.spell_level {
                crate::package::character::grant_spell(ctx, guid, chain.spell_id)?;
            }
        }
    }
    Ok(())
}

fn grant_choice_ranks(
    ctx: &ReducerContext,
    guid: u64,
    choice: u32,
    level: u8,
) -> Result<(), String> {
    if choice < TALENT_CHOICE {
        return grant_ranks(ctx, guid, choice, level);
    }
    let talent = ctx
        .db
        .game_talent()
        .talent_id()
        .find(choice - TALENT_CHOICE)
        .ok_or("Talent is not imported")?;
    let spell_id = if talent.grant_spell_id != 0 {
        talent.grant_spell_id
    } else {
        let spell = ctx
            .db
            .game_spell()
            .spell_id()
            .find(talent.spell_id)
            .ok_or("Talent spell is not imported")?;
        if spell.attributes & SPELL_ATTR_PASSIVE != 0 {
            return Ok(());
        }
        talent.spell_id
    };
    grant_ranks(ctx, guid, spell_id, level)
}

fn grant_class_basics(ctx: &ReducerContext, guid: u64, class: u8, level: u8) -> Result<(), String> {
    // Armor proficiency Spell.dbc levels are zero; class training starts at level 40.
    let armor = match class {
        1 | 2 if level >= 40 => Some(PLATE_PASSIVE_SPELL_ID),
        3 | 7 if level >= 40 => Some(MAIL_PASSIVE_SPELL_ID),
        _ => None,
    };
    if let Some(spell) = armor {
        crate::package::character::grant_spell(ctx, guid, spell)?;
    }
    if class == 4 && level >= 10 || matches!(class, 1 | 3) && level >= 20 {
        crate::package::character::grant_spell(ctx, guid, 674)?;
    }
    let parry_level = match class {
        1 => Some(6),
        2 | 3 => Some(8),
        4 => Some(12),
        _ => None,
    };
    if parry_level.is_some_and(|required| level >= required) {
        crate::package::character::grant_spell(ctx, guid, 3127)?;
    }
    if class == 7 && level >= 30 {
        crate::package::character::grant_spell(ctx, guid, 20608)?;
    }
    if class == 11 && level >= 40 {
        crate::package::character::grant_spell(ctx, guid, 9634)?;
    }
    Ok(())
}

fn spell_has_implemented_effect(ctx: &ReducerContext, spell: u32) -> bool {
    ctx.db
        .game_spell_effect()
        .by_spell()
        .filter(&spell)
        .any(|effect| !matches!(effect.kind, E_SCRIPTED | A_FLAG) || effect.script_id != 0)
}

pub(crate) fn refresh_level(
    ctx: &ReducerContext,
    member: &DefenseMember,
    level: u8,
) -> Result<(), String> {
    let spec = catalog::spec_for_class(member.class).ok_or("Unsupported class")?;
    crate::package::character::advance_match_level(ctx, member.character_guid, u32::from(level))?;
    if member.class == 3 {
        let config = ctx
            .db
            .pkg_wave_defense_config()
            .id()
            .find(0)
            .ok_or("Match configuration unavailable")?;
        crate::package::character::provision_hunter_pet(
            ctx,
            member.character_guid,
            config.pet_entry,
            false,
        )?;
    }
    for &spell in spec.essential_spells {
        grant_ranks(ctx, member.character_guid, spell, level)?;
    }
    grant_class_basics(ctx, member.character_guid, member.class, level)?;
    for &choice in &member.acquired {
        grant_choice_ranks(ctx, member.character_guid, choice, level)?;
    }
    Ok(())
}

pub(crate) fn prepare_offer(
    ctx: &ReducerContext,
    member: &mut DefenseMember,
    level: u8,
) -> Result<(), String> {
    if pending(member, level) == 0 || !member.offer_ids.is_empty() || member.bot {
        return Ok(());
    }
    let spec = catalog::spec_for_class(member.class).ok_or("Unsupported class")?;
    let milestone = catalog::REWARD_LEVELS[usize::from(member.choices_spent)];
    let mut choices: Vec<u32> = spec
        .draft_spells
        .iter()
        .copied()
        .filter(|spell| {
            !member.acquired.contains(spell)
                && spell_has_implemented_effect(ctx, *spell)
                && ctx
                    .db
                    .game_spell()
                    .spell_id()
                    .find(*spell)
                    .is_some_and(|row| row.spell_level <= milestone)
        })
        .collect();
    for talent in ctx.db.game_talent().by_tab().filter(&spec.talent_tree) {
        let choice = TALENT_CHOICE + talent.talent_id;
        let gate_level = talent.tier.saturating_mul(10).max(1);
        if member.acquired.contains(&choice) || gate_level > milestone || talent.max_rank == 0 {
            continue;
        }
        if talent.required_talent_id != 0
            && !member
                .acquired
                .contains(&(TALENT_CHOICE + talent.required_talent_id))
        {
            continue;
        }
        let ranks = [
            talent.spell_id,
            talent.rank_spell_2,
            talent.rank_spell_3,
            talent.rank_spell_4,
            talent.rank_spell_5,
        ];
        if ranks
            .get(usize::from(talent.max_rank - 1))
            .is_none_or(|spell| {
                *spell == 0
                    || ctx.db.game_spell().spell_id().find(*spell).is_none()
                    || !spell_has_implemented_effect(ctx, *spell)
            })
            || talent.grant_spell_id != 0
                && ctx
                    .db
                    .game_spell()
                    .spell_id()
                    .find(talent.grant_spell_id)
                    .is_none()
            || talent.grant_spell_id != 0
                && !spell_has_implemented_effect(ctx, talent.grant_spell_id)
        {
            continue;
        }
        choices.push(choice);
    }
    let salt = ctx.random::<u64>();
    choices.sort_by_key(|choice| {
        salt.wrapping_add(u64::from(*choice))
            .wrapping_mul(0x9e3779b97f4a7c15)
            .rotate_left(23)
    });
    choices.truncate(3);
    if choices.is_empty() {
        return Err(format!(
            "No imported rewards for {} at level {milestone}",
            spec.name
        ));
    }
    member.offer_ids = choices;
    member.offer_revision = member
        .offer_revision
        .checked_add(1)
        .ok_or("Offer revision exhausted")?;
    Ok(())
}

pub(crate) fn choose(
    ctx: &ReducerContext,
    guid: u64,
    match_id: u64,
    revision: u32,
    choice: u32,
) -> Result<(), String> {
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
    if run.id != match_id
        || run.phase != Phase::Preparation
        || pending(&member, run.level) == 0
        || member.offer_revision != revision
        || !member.offer_ids.contains(&choice)
    {
        return Err("That reward is no longer available; refresh the offer".into());
    }
    if choice >= TALENT_CHOICE {
        crate::package::character::grant_talent(ctx, guid, choice - TALENT_CHOICE)?;
    }
    grant_choice_ranks(ctx, guid, choice, run.level)?;
    member.acquired.push(choice);
    member.choices_spent += 1;
    member.offer_ids.clear();
    prepare_offer(ctx, &mut member, run.level)?;
    ctx.db
        .pkg_wave_defense_member()
        .character_guid()
        .update(member);
    Ok(())
}

fn wire_text(text: &str) -> String {
    text.chars()
        .take(80)
        .map(|ch| {
            if ch == '|' || ch.is_control() {
                ' '
            } else {
                ch
            }
        })
        .collect()
}

pub(crate) fn offer(ctx: &ReducerContext, guid: u64) -> Result<String, String> {
    let Some(member) = ctx.db.pkg_wave_defense_member().character_guid().find(guid) else {
        return Ok("v1|0|0|0|0".into());
    };
    let run = ctx
        .db
        .pkg_wave_defense_match()
        .id()
        .find(member.match_id)
        .ok_or("Match ended")?;
    let milestone = catalog::REWARD_LEVELS
        .get(usize::from(member.choices_spent))
        .copied()
        .unwrap_or(60);
    let mut reply = format!(
        "v1|{}|{}|{}|{}",
        run.id,
        member.offer_revision,
        milestone,
        pending(&member, run.level)
    );
    for &choice in &member.offer_ids {
        let (kind, name, description) = if choice >= TALENT_CHOICE {
            let talent = ctx
                .db
                .game_talent()
                .talent_id()
                .find(choice - TALENT_CHOICE)
                .ok_or("Talent unavailable")?;
            (
                "talent",
                talent.name,
                format!("All {} ranks", talent.max_rank),
            )
        } else {
            let spell = ctx
                .db
                .game_spell()
                .spell_id()
                .find(choice)
                .ok_or("Spell unavailable")?;
            (
                "spell",
                spell.name,
                "Ranks grow with team level".to_string(),
            )
        };
        reply.push_str(&format!(
            "|{choice}|{kind}|{}|{}",
            wire_text(&name),
            description
        ));
    }
    Ok(reply)
}

pub(crate) fn pay(ctx: &ReducerContext, guid: u64, copper: u32) {
    if let Some(mut character) = crate::actor::character_by_guid(ctx, guid) {
        character.money = character.money.saturating_add(copper);
        ctx.db.game_character().guid().update(character);
    }
    if let Some(mut body) = ctx.db.game_world_entity().guid().find(guid) {
        body.money = body.money.saturating_add(copper);
        body.update(ctx);
    }
}

pub(crate) fn record_wave(ctx: &ReducerContext, member: &DefenseMember, boss: bool, won: bool) {
    if member.bot {
        return;
    }
    let progress = ctx.db.pkg_wave_defense_progress();
    let existing = progress.account_id().find(member.account_id);
    let present = existing.is_some();
    let mut row = existing.unwrap_or(DefenseProgress {
        account_id: member.account_id,
        waves: 0,
        bosses: 0,
        wins: 0,
    });
    row.waves = row.waves.saturating_add(1);
    row.bosses = row.bosses.saturating_add(u32::from(boss));
    row.wins = row.wins.saturating_add(u32::from(won));
    if present {
        progress.account_id().update(row);
    } else {
        progress.insert(row);
    }
}
