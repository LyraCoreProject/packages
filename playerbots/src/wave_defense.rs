use super::{pkg_playerbots_bot, pkg_playerbots_personality, PlayerbotsBot};
use crate::tables::{game_spell, game_world_entity};
use spacetimedb::{table, ReducerContext, Table};

const MAX_WAVE_TARGETS: usize = 128;
const DEFENSE_RADIUS_YD: f32 = 45.0;

#[table(accessor = pkg_playerbots_wave_defense, public)]
pub struct WaveDefenseObjective {
    #[primary_key]
    pub character_guid: u64,
    pub destination: super::Destination,
    pub target_guids: Vec<u64>,
}

crate::character_owned!(delete, fn sweep_delete_pkg_playerbots_wave_defense(ctx, character_guid) {
    ctx.db.pkg_playerbots_wave_defense().character_guid().delete(character_guid);
});
crate::character_owned!(not_transported, fn sweep_transfer_pkg_playerbots_wave_defense());

fn require_bot(ctx: &ReducerContext, guid: u64) -> Result<PlayerbotsBot, String> {
    if ctx
        .db
        .pkg_playerbots_wave_defense()
        .character_guid()
        .find(guid)
        .is_none()
    {
        return Err("Character is not a wave defense bot".into());
    }
    ctx.db
        .pkg_playerbots_bot()
        .by_character()
        .filter(guid)
        .next()
        .ok_or_else(|| "Bot is missing".into())
}

fn require_position(at: (f32, f32, f32)) -> Result<(), String> {
    if !at.0.is_finite() || !at.1.is_finite() || !at.2.is_finite() {
        return Err("Defense position must be finite".into());
    }
    Ok(())
}

pub(crate) fn spawn_wave_defense_bot(
    ctx: &ReducerContext,
    role: u8,
    level: u8,
    map_id: u32,
    instance_id: u64,
    at: (f32, f32, f32),
) -> Result<u64, String> {
    require_position(at)?;
    if !(1..=60).contains(&level) {
        return Err("Wave defense bot level must be between 1 and 60".into());
    }
    let class = super::default_class_for_role(role).ok_or("Unknown bot role")?;
    super::ensure_defaults(ctx);
    let guid = super::spawn_one(
        ctx,
        (super::BOT_RACE, class),
        role,
        super::role_name_stem(role),
        map_id,
        at,
        level,
    )?;
    if let Some(mut body) = ctx.db.game_world_entity().guid().find(guid) {
        body.instance_id = instance_id;
        body.update(ctx);
    }
    ctx.db
        .pkg_playerbots_wave_defense()
        .insert(WaveDefenseObjective {
            character_guid: guid,
            destination: super::Destination {
                map_id,
                instance_id,
                x: at.0,
                y: at.1,
                z: at.2,
                geometry_revision: None,
            },
            target_guids: vec![],
        });
    for mut personality in ctx
        .db
        .pkg_playerbots_personality()
        .by_character()
        .filter(guid)
    {
        personality.flee_at_pct = 0;
        ctx.db.pkg_playerbots_personality().id().update(personality);
    }
    teach_role(ctx, guid, class, role, level)?;
    Ok(guid)
}

fn teach_role(
    ctx: &ReducerContext,
    guid: u64,
    class: u8,
    role: u8,
    level: u8,
) -> Result<(), String> {
    for (kit_class, kit_role, spell) in super::default_kit() {
        if (kit_class, kit_role) != (class, role) {
            continue;
        }
        for rank in super::spell_ranks::family(ctx, spell)? {
            if ctx
                .db
                .game_spell()
                .spell_id()
                .find(rank)
                .is_some_and(|row| rank == spell || row.spell_level <= level)
            {
                crate::package::character::grant_spell(ctx, guid, rank)?;
            }
        }
    }
    Ok(())
}

/// Set after Instance entry and whenever the active wave changes; targets are bounded to 128.
pub(crate) fn set_wave_defense_objective(
    ctx: &ReducerContext,
    guid: u64,
    defend_at: (f32, f32, f32),
    target_guids: &[u64],
) -> Result<(), String> {
    require_position(defend_at)?;
    if target_guids.len() > MAX_WAVE_TARGETS {
        return Err("Wave defense has too many bot targets".into());
    }
    let mut bot = require_bot(ctx, guid)?;
    let body = ctx
        .db
        .game_world_entity()
        .guid()
        .find(guid)
        .ok_or("Bot body is missing")?;
    let mut targets = target_guids
        .iter()
        .copied()
        .filter(|guid| *guid != 0)
        .collect::<Vec<_>>();
    targets.sort_unstable();
    targets.dedup();
    ctx.db
        .pkg_playerbots_wave_defense()
        .character_guid()
        .update(WaveDefenseObjective {
            character_guid: guid,
            destination: super::Destination {
                map_id: body.map_id,
                instance_id: body.instance_id,
                x: defend_at.0,
                y: defend_at.1,
                z: defend_at.2,
                geometry_revision: None,
            },
            target_guids: targets,
        });
    bot.next_think_micros = ctx.timestamp.to_micros_since_unix_epoch();
    ctx.db.pkg_playerbots_bot().id().update(bot);
    Ok(())
}

pub(crate) fn update_wave_defense_bot_level(
    ctx: &ReducerContext,
    guid: u64,
    level: u8,
) -> Result<(), String> {
    if !(1..=60).contains(&level) {
        return Err("Wave defense bot level must be between 1 and 60".into());
    }
    let bot = require_bot(ctx, guid)?;
    let character = crate::actor::character_by_guid(ctx, guid).ok_or("Bot Character is missing")?;
    if character.level != level {
        crate::package::character::set_character_level(ctx, guid, u32::from(level))?;
        super::provisioning::arm_spawn(
            ctx,
            &bot,
            u32::from(level),
            ctx.timestamp.to_micros_since_unix_epoch(),
        );
    }
    teach_role(ctx, guid, bot.class, bot.role, level)
}

pub(crate) fn remove_wave_defense_bot(ctx: &ReducerContext, guid: u64) -> Result<(), String> {
    require_bot(ctx, guid)?;
    let _ = crate::actor::leave_group_for(ctx, guid);
    crate::package::character::cascade_delete_character(ctx, guid);
    Ok(())
}

impl WaveDefenseObjective {
    pub(super) fn permits(&self, map_id: u32, instance_id: u64, x: f32, y: f32, z: f32) -> bool {
        (map_id, instance_id) == (self.destination.map_id, self.destination.instance_id)
            && (x - self.destination.x).powi(2)
                + (y - self.destination.y).powi(2)
                + (z - self.destination.z).powi(2)
                <= DEFENSE_RADIUS_YD.powi(2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defense_movement_stays_in_its_instance_and_near_the_base() {
        let objective = WaveDefenseObjective {
            character_guid: 1,
            destination: super::super::Destination {
                map_id: 189,
                instance_id: 7,
                x: 100.0,
                y: 200.0,
                z: 10.0,
                geometry_revision: None,
            },
            target_guids: vec![],
        };
        assert!(objective.permits(189, 7, 145.0, 200.0, 10.0));
        assert!(!objective.permits(189, 7, 146.0, 200.0, 10.0));
        assert!(!objective.permits(189, 8, 100.0, 200.0, 10.0));
        assert!(!objective.permits(0, 7, 100.0, 200.0, 10.0));
    }
}
