//! Shared class decisions for autonomous and companion bots.

use super::companion::Party;
use super::decision::{Action, ActionNode, CastAction, MoveTarget, Readiness, Reason};
use super::{
    cond, pkg_playerbots_personality, pkg_playerbots_rotation, PlayerbotsBot, PlayerbotsRotation,
    ROLE_HEALER,
};
use crate::{game_spell, game_spell_effect, game_world_entity};
use spacetimedb::ReducerContext;

const ROTATION_LIMIT: usize = 12;
const MELEE_RANGE_YD: f32 = 4.0;

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoleReadError {
    RotationLimit,
    BuffAuraLimit,
    BuffFamilyUnavailable,
}

fn node(action: Action, reason: Reason, priority: i32, objective: u64) -> ActionNode {
    let mut node = ActionNode::ready(action, reason, priority);
    node.candidate.id.objective = objective;
    node
}

fn wounded_member(
    member: &crate::group::PartyMemberFacts,
    partition: (u32, u64),
    heal_at_pct: u8,
) -> Option<(u32, u32, u64)> {
    let unit = member.unit.as_ref()?;
    ((unit.map_id, unit.instance_id) == partition
        && !unit.dead
        && unit.max_health > 0
        && u64::from(unit.health) * 100 <= u64::from(unit.max_health) * u64::from(heal_at_pct))
    .then_some((unit.health, unit.max_health, member.character_guid))
}

fn wounded_ally(
    party: &Party,
    partition: (u32, u64),
    heal_at_pct: u8,
    retained: Option<u64>,
) -> Option<u64> {
    if let Some(retained) = retained.filter(|guid| {
        party.members.iter().any(|member| {
            member.character_guid == *guid
                && wounded_member(member, partition, heal_at_pct).is_some()
        })
    }) {
        return Some(retained);
    }
    party
        .members
        .iter()
        .filter_map(|member| wounded_member(member, partition, heal_at_pct))
        .min_by(|a, b| {
            (u64::from(a.0) * u64::from(b.1), a.2).cmp(&(u64::from(b.0) * u64::from(a.1), b.2))
        })
        .map(|(_, _, guid)| guid)
}

enum CastPreparation {
    Ready,
    MoveForRange,
    MoveForLineOfSight,
    Refused,
}

/// Keep timed casts inside their nominal range so ordinary target movement can use the Core
/// completion leeway instead of consuming it before the cast starts.
fn cast_preparation(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    spell_id: u32,
    target_guid: u64,
) -> CastPreparation {
    match crate::actor::cast_readiness(ctx, me.guid, spell_id, target_guid) {
        Err(refusal) if refusal.kind == crate::spell::CastRefusalKind::OutOfRange => {
            return CastPreparation::MoveForRange;
        }
        Err(refusal) if refusal.kind == crate::spell::CastRefusalKind::NoLineOfSight => {
            return CastPreparation::MoveForLineOfSight;
        }
        Err(_) => return CastPreparation::Refused,
        Ok(()) => {}
    }
    if target_guid == me.guid {
        return CastPreparation::Ready;
    }
    let Some(spell) = ctx.db.game_spell().spell_id().find(spell_id) else {
        return CastPreparation::Ready;
    };
    if spell.cast_time_ms == 0 || spell.range_yd == 0 {
        return CastPreparation::Ready;
    }
    let Some(target) = ctx.db.game_world_entity().guid().find(target_guid) else {
        return CastPreparation::Ready;
    };
    if (target.map_id, target.instance_id) != (me.map_id, me.instance_id) {
        return CastPreparation::Ready;
    }
    let distance_sq =
        (me.x - target.x).powi(2) + (me.y - target.y).powi(2) + (me.z - target.z).powi(2);
    if distance_sq > (spell.range_yd as f32).powi(2) {
        CastPreparation::MoveForRange
    } else {
        CastPreparation::Ready
    }
}

fn cast_node(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    cast: CastAction,
    reason: Reason,
    position_reason: Reason,
    priority: i32,
    objective: u64,
) -> ActionNode {
    let mut candidate = node(Action::Cast(cast), reason, priority, objective);
    if crate::spell::pending_cast(ctx, me.guid)
        .is_some_and(|pending| pending.spell_id == cast.spell && pending.target_guid == cast.target)
    {
        return candidate;
    }
    if !preserves_queued_swing_resource(ctx, me, cast.spell) {
        candidate.readiness = Readiness::Refused;
        return candidate;
    }
    match cast_preparation(ctx, me, cast.spell, cast.target) {
        CastPreparation::Ready => {}
        preparation @ (CastPreparation::MoveForRange | CastPreparation::MoveForLineOfSight) => {
            let repair_reason = if matches!(preparation, CastPreparation::MoveForLineOfSight)
                && matches!(
                    reason,
                    Reason::Heal | Reason::Buff | Reason::TankFight | Reason::DamageFight
                ) {
                Reason::CastingPosition
            } else {
                position_reason
            };
            let mut position = node(
                Action::Move(MoveTarget::CastingPosition(cast.target)),
                repair_reason,
                priority,
                objective,
            );
            position
                .alternatives
                .push(node(Action::Hold, repair_reason, priority, objective));
            candidate.prerequisites.push(position);
        }
        CastPreparation::Refused => candidate.readiness = Readiness::Refused,
    }
    candidate
}

fn rotation_rows(
    ctx: &ReducerContext,
    bot: &PlayerbotsBot,
    conditions: &[u8],
) -> Result<Vec<PlayerbotsRotation>, RoleReadError> {
    let rows: Vec<_> = ctx
        .db
        .pkg_playerbots_rotation()
        .by_class_role()
        .filter((bot.class, bot.role))
        .take(ROTATION_LIMIT + 1)
        .collect();
    if rows.len() > ROTATION_LIMIT {
        return Err(RoleReadError::RotationLimit);
    }
    let mut rows: Vec<_> = rows
        .into_iter()
        .filter(|row| conditions.contains(&row.condition))
        .collect();
    rows.sort_by_key(|row| (row.priority, row.spell_id, row.id));
    Ok(rows)
}

fn buff_missing(
    ctx: &ReducerContext,
    target_guid: u64,
    spell_id: u32,
    caster_level: u8,
) -> Result<bool, RoleReadError> {
    match crate::spell::buff_status(ctx, target_guid, spell_id, caster_level) {
        crate::spell::BuffStatus::Missing => Ok(true),
        crate::spell::BuffStatus::Satisfied => Ok(false),
        crate::spell::BuffStatus::Unavailable(crate::spell::BuffUnavailableReason::AuraLimit) => {
            Err(RoleReadError::BuffAuraLimit)
        }
        crate::spell::BuffStatus::Unavailable(crate::spell::BuffUnavailableReason::Family) => {
            Err(RoleReadError::BuffFamilyUnavailable)
        }
    }
}

fn buff_target(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    party: Option<&Party>,
    row: &PlayerbotsRotation,
    retained: Option<u64>,
) -> Result<Option<u64>, RoleReadError> {
    if row.condition == cond::SELF_MISSING_AURA {
        return Ok(buff_missing(ctx, me.guid, row.spell_id, me.level as u8)?.then_some(me.guid));
    }
    if row.condition != cond::ALLY_MISSING_AURA {
        return Ok(None);
    }
    let Some(party) = party else {
        return Ok(buff_missing(ctx, me.guid, row.spell_id, me.level as u8)?.then_some(me.guid));
    };
    let eligible = |unit: &crate::group::PartyUnitFacts| {
        !unit.dead && (unit.map_id, unit.instance_id) == (me.map_id, me.instance_id)
    };
    if let Some(retained) = retained {
        if let Some(unit) = party
            .members
            .iter()
            .find(|member| member.character_guid == retained)
            .and_then(|member| member.unit.as_ref())
        {
            if eligible(unit) && buff_missing(ctx, retained, row.spell_id, me.level as u8)? {
                return Ok(Some(retained));
            }
        }
    }
    let mut target = None;
    for member in &party.members {
        let Some(_unit) = member.unit.as_ref().filter(|unit| eligible(unit)) else {
            continue;
        };
        if buff_missing(ctx, member.character_guid, row.spell_id, me.level as u8)? {
            target = Some(target.map_or(member.character_guid, |current: u64| {
                current.min(member.character_guid)
            }));
        }
    }
    Ok(target)
}

fn maintenance(
    ctx: &ReducerContext,
    bot: &PlayerbotsBot,
    me: &crate::WorldEntity,
    party: Option<&Party>,
    in_combat: bool,
    objective: u64,
    retained: Option<u64>,
    fallback: &ActionNode,
) -> Result<Option<(ActionNode, u64)>, RoleReadError> {
    let rows = rotation_rows(
        ctx,
        bot,
        &[cond::SELF_MISSING_AURA, cond::ALLY_MISSING_AURA],
    )?;
    for row in rows {
        let combat_buff =
            row.condition == cond::SELF_MISSING_AURA && combat_buff(ctx, row.spell_id);
        if in_combat && !combat_buff {
            continue;
        }
        let Some(target) = buff_target(ctx, me, party, &row, retained)? else {
            continue;
        };
        let mut cast = cast_node(
            ctx,
            me,
            CastAction {
                target,
                spell: row.spell_id,
            },
            Reason::Buff,
            Reason::BuffPosition,
            if in_combat && combat_buff { 720 } else { 300 },
            objective,
        );
        cast.alternatives.push(fallback.clone());
        if cast.readiness != Readiness::Refused {
            return Ok(Some((cast, target)));
        }
    }
    Ok(None)
}

struct HealingSelection {
    candidate: Option<ActionNode>,
    target: Option<u64>,
}

fn healing(
    ctx: &ReducerContext,
    bot: &PlayerbotsBot,
    me: &crate::WorldEntity,
    party: Option<&Party>,
    personality_heal_at: u8,
    objective: u64,
    retained_target: Option<u64>,
    fallback: &ActionNode,
) -> Result<HealingSelection, RoleReadError> {
    let rows = rotation_rows(ctx, bot, &[cond::ALLY_HP_BELOW_PCT])?;
    let mut selected_target = None;
    for row in rows {
        let heal_at_pct = row.threshold_pct.min(personality_heal_at);
        let target = match party {
            Some(party) => wounded_ally(
                party,
                (me.map_id, me.instance_id),
                heal_at_pct,
                retained_target,
            ),
            None => (me.max_health > 0
                && u64::from(me.health) * 100 < u64::from(me.max_health) * u64::from(heal_at_pct))
            .then_some(me.guid),
        };
        let Some(target) = target else {
            continue;
        };
        if selected_target.is_none() {
            selected_target = Some(target);
        }
        let mut heal = cast_node(
            ctx,
            me,
            CastAction {
                target,
                spell: row.spell_id,
            },
            if party.is_some() {
                Reason::Heal
            } else {
                Reason::Recovery
            },
            if party.is_some() {
                Reason::CastingPosition
            } else {
                Reason::Recovery
            },
            800,
            objective,
        );
        heal.alternatives.push(fallback.clone());
        if heal.readiness != Readiness::Refused {
            return Ok(HealingSelection {
                candidate: Some(heal),
                target: Some(target),
            });
        }
    }
    Ok(HealingSelection {
        candidate: None,
        target: selected_target,
    })
}

pub(super) fn combat_buff(ctx: &ReducerContext, spell: u32) -> bool {
    ctx.db
        .game_spell()
        .spell_id()
        .find(spell)
        .is_some_and(|spell| {
            spell.cast_time_ms == 0
                && spell.power_type == lyracore_shared::packing::power_type::RAGE
                && !spell.is_negative
                && spell.cast_flags & crate::spell::SPELL_ATTR_CHANNELED == 0
        })
}

pub(super) fn queues_swing(ctx: &ReducerContext, spell: u32) -> bool {
    ctx.db
        .game_spell_effect()
        .by_spell()
        .filter(spell)
        .take(3)
        .any(|effect| effect.kind == crate::spell::E_NEXT_SWING)
}

fn preserves_queued_swing_resource(
    ctx: &ReducerContext,
    me: &crate::WorldEntity,
    spell: u32,
) -> bool {
    if me.next_swing_spell == 0 {
        return true;
    }
    if queues_swing(ctx, spell) {
        return false;
    }
    let Some(queued) = ctx.db.game_spell().spell_id().find(me.next_swing_spell) else {
        return false;
    };
    let Some(next) = ctx.db.game_spell().spell_id().find(spell) else {
        return false;
    };
    next.power_type != queued.power_type || me.power >= queued.cost.saturating_add(next.cost)
}

#[derive(Clone, Copy)]
pub(super) enum FightFallback {
    Melee,
    Hold,
}

pub(super) struct Fight {
    pub target: u64,
    pub protecting_ally: bool,
    pub tank_engaged: bool,
    pub fallback: FightFallback,
}

pub(super) fn combat(
    ctx: &ReducerContext,
    bot: &PlayerbotsBot,
    me: &crate::WorldEntity,
    fight: Fight,
    objective: u64,
    reason: Reason,
    priority: i32,
) -> Result<ActionNode, RoleReadError> {
    let mut attack = node(
        match fight.fallback {
            FightFallback::Melee => Action::Attack(fight.target),
            FightFallback::Hold => Action::Hold,
        },
        reason,
        priority,
        objective,
    );
    if matches!(fight.fallback, FightFallback::Melee)
        && ctx
            .db
            .game_world_entity()
            .guid()
            .find(fight.target)
            .is_some_and(|target| {
                (me.x - target.x).powi(2) + (me.y - target.y).powi(2) + (me.z - target.z).powi(2)
                    > MELEE_RANGE_YD * MELEE_RANGE_YD
            })
    {
        attack.prerequisites.push(node(
            Action::Move(MoveTarget::Entity(fight.target)),
            if reason == Reason::TankFight {
                Reason::MeleePosition
            } else {
                reason
            },
            priority,
            objective,
        ));
    }
    for row in rotation_rows(
        ctx,
        bot,
        &[cond::ALWAYS, cond::ENEMY_ON_ALLY, cond::TANK_ENGAGED],
    )? {
        if (row.condition == cond::ENEMY_ON_ALLY && !fight.protecting_ally)
            || (row.condition == cond::TANK_ENGAGED && !fight.tank_engaged)
        {
            continue;
        }
        let position_reason = match reason {
            Reason::TankFight => Reason::MeleePosition,
            Reason::DamageFight => Reason::FightPosition,
            _ => reason,
        };
        let mut cast = cast_node(
            ctx,
            me,
            CastAction {
                target: fight.target,
                spell: row.spell_id,
            },
            reason,
            position_reason,
            priority,
            objective,
        );
        cast.alternatives.push(attack.clone());
        if cast.readiness != Readiness::Refused {
            return Ok(cast);
        }
    }
    Ok(attack)
}

pub(super) struct Support {
    pub candidates: Vec<ActionNode>,
    pub heal_target: Option<u64>,
    pub buff_target: Option<u64>,
    pub read_failure: Option<RoleReadError>,
}

pub(super) struct SupportContext<'a> {
    pub party: Option<&'a Party>,
    pub in_combat: bool,
    pub permits_healing: bool,
    pub heal_target: Option<u64>,
    pub buff_target: Option<u64>,
}

pub(super) fn support(
    ctx: &ReducerContext,
    bot: &PlayerbotsBot,
    me: &crate::WorldEntity,
    context: SupportContext<'_>,
    objective: u64,
    fallback: &ActionNode,
) -> Result<Support, RoleReadError> {
    let heal = if bot.role == ROLE_HEALER && context.permits_healing {
        let threshold = ctx
            .db
            .pkg_playerbots_personality()
            .by_character()
            .filter(me.guid)
            .next()
            .map_or(u8::MAX, |personality| personality.heal_at_pct);
        healing(
            ctx,
            bot,
            me,
            context.party,
            threshold,
            objective,
            context.heal_target,
            fallback,
        )?
    } else {
        HealingSelection {
            candidate: None,
            target: None,
        }
    };
    let (buff, read_failure) = match maintenance(
        ctx,
        bot,
        me,
        context.party,
        context.in_combat,
        objective,
        context.buff_target,
        fallback,
    ) {
        Ok(buff) => (buff, None),
        Err(RoleReadError::RotationLimit) => return Err(RoleReadError::RotationLimit),
        Err(unavailable) => (None, Some(unavailable)),
    };
    let buff_target = buff.as_ref().map(|(_, target)| *target);
    let mut candidates = Vec::with_capacity(3);
    if let Some(heal) = heal.candidate {
        candidates.push(heal);
    }
    if let Some((buff, _)) = buff {
        candidates.push(buff);
    }
    if read_failure.is_some() {
        candidates.push(node(Action::Hold, Reason::RoleUnavailable, 750, objective));
    }
    Ok(Support {
        candidates,
        heal_target: heal.target,
        buff_target,
        read_failure,
    })
}
#[cfg(test)]
mod tests {
    use super::*;

    fn member(guid: u64, health: u32, max_health: u32) -> crate::group::PartyMemberFacts {
        crate::group::PartyMemberFacts {
            character_guid: guid,
            unit: Some(crate::group::PartyUnitFacts {
                map_id: 0,
                instance_id: 0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
                health,
                max_health,
                dead: false,
                target_guid: 0,
            }),
            partition: None,
        }
    }

    #[test]
    fn the_most_injured_ally_wins_with_guid_as_the_stable_tie_break() {
        let party = Party {
            group_id: 1,
            leader_guid: 10,
            leader: None,
            members: vec![member(12, 20, 100), member(11, 10, 50), member(13, 30, 100)],
            enemies: vec![],
            fight_constraint: None,
        };
        assert_eq!(wounded_ally(&party, (0, 0), 50, None), Some(11));
        assert_eq!(wounded_ally(&party, (0, 0), 50, Some(12)), Some(12));
    }
}
