//! Human-led party facts and starter-role Strategies.

use super::decision::{
    Action, ActionNode, CastAction, MoveTarget, Readiness, Reason, Strategy, Trigger,
};
use super::{
    cond, pkg_playerbots_bot, pkg_playerbots_personality, pkg_playerbots_rotation, PlayerbotsBot,
    PlayerbotsRotation, ROLE_DPS, ROLE_HEALER, ROLE_TANK,
};
use spacetimedb::ReducerContext;

const MELEE_RANGE_YD: f32 = 4.0;
const ROTATION_LIMIT: usize = 12;

#[derive(spacetimedb::SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoleReadError {
    RotationLimit,
    BuffAuraLimit,
    BuffFamilyUnavailable,
}

pub(super) struct Party {
    pub group_id: u64,
    pub leader_guid: u64,
    pub leader: Option<crate::group::PartyUnitFacts>,
    pub members: Vec<crate::group::PartyMemberFacts>,
    pub enemies: Vec<crate::group::PartyEnemyFacts>,
    pub fight_constraint: Option<u64>,
}

impl Party {
    pub fn destination(&self) -> Option<super::runner::Destination> {
        self.leader
            .as_ref()
            .map(|leader| super::runner::Destination {
                map_id: leader.map_id,
                instance_id: leader.instance_id,
                x: leader.x,
                y: leader.y,
                z: leader.z,
                geometry_revision: None,
            })
    }

    fn designated_target(&self) -> Option<u64> {
        self.leader
            .as_ref()
            .map(|leader| leader.target_guid)
            .filter(|guid| *guid != 0)
    }
}

/// A party is companion-controlled only when its durable leader is not another bot.
pub(super) fn human_led_party(
    ctx: &ReducerContext,
    character_guid: u64,
) -> Result<Option<Party>, crate::group::PartyFactsUnavailable> {
    let Some(facts) = crate::group::party_facts(ctx, character_guid)? else {
        return Ok(None);
    };
    if ctx
        .db
        .pkg_playerbots_bot()
        .by_character()
        .filter(facts.leader_guid)
        .next()
        .is_some()
    {
        return Ok(None);
    }
    let leader = facts
        .members
        .iter()
        .find(|member| member.character_guid == facts.leader_guid)
        .and_then(|member| member.unit.clone());
    Ok(Some(Party {
        group_id: facts.group_id,
        leader_guid: facts.leader_guid,
        leader,
        members: facts.members,
        enemies: facts.enemies,
        fight_constraint: None,
    }))
}

fn distance_sq(me: &crate::WorldEntity, unit: &crate::group::PartyUnitFacts) -> f32 {
    (me.x - unit.x).powi(2) + (me.y - unit.y).powi(2) + (me.z - unit.z).powi(2)
}

fn enemy_distance_sq(me: &crate::WorldEntity, enemy: &crate::group::PartyEnemyFacts) -> f32 {
    (me.x - enemy.x).powi(2) + (me.y - enemy.y).powi(2) + (me.z - enemy.z).powi(2)
}

fn node(action: Action, reason: Reason, priority: i32, objective: u64) -> ActionNode {
    let mut node = ActionNode::ready(action, reason, priority);
    node.candidate.id.objective = objective;
    node
}

fn follow(party: &Party, me: &crate::WorldEntity, objective: u64) -> ActionNode {
    let Some(leader) = party.leader.as_ref() else {
        return node(Action::Hold, Reason::Follow, 100, objective);
    };
    if (leader.map_id, leader.instance_id) != (me.map_id, me.instance_id)
        || distance_sq(me, leader) <= 3.05 * 3.05
    {
        return node(Action::Hold, Reason::Follow, 100, objective);
    }
    node(
        Action::Move(MoveTarget::Entity(party.leader_guid)),
        Reason::Follow,
        100,
        objective,
    )
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
    match crate::actor::cast_readiness(ctx, me.guid, cast.spell, cast.target) {
        Ok(()) => {}
        Err(refusal)
            if matches!(
                refusal.kind,
                crate::spell::CastRefusalKind::OutOfRange
                    | crate::spell::CastRefusalKind::NoLineOfSight
            ) =>
        {
            let repair_reason = if refusal.kind == crate::spell::CastRefusalKind::NoLineOfSight {
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
        Err(_) => candidate.readiness = Readiness::Refused,
    }
    candidate
}

fn fight_target<'a>(
    party: &'a Party,
    me: &crate::WorldEntity,
    role: u8,
    retained: Option<u64>,
) -> Option<&'a crate::group::PartyEnemyFacts> {
    let eligible = |enemy: &&crate::group::PartyEnemyFacts| {
        enemy.control.is_none()
            && (enemy.map_id, enemy.instance_id) == (me.map_id, me.instance_id)
            && enemy.health > 0
    };
    let enemies: Vec<_> = party.enemies.iter().filter(eligible).collect();
    if let Some(exact) = party.fight_constraint {
        return enemies.into_iter().find(|enemy| enemy.guid == exact);
    }
    if let Some(designated) = party.designated_target() {
        if let Some(enemy) = enemies.iter().find(|enemy| enemy.guid == designated) {
            return Some(*enemy);
        }
    }
    if let Some(retained) = retained {
        if let Some(enemy) = enemies.iter().find(|enemy| enemy.guid == retained) {
            return Some(*enemy);
        }
    }
    enemies.into_iter().min_by_key(|enemy| {
        let protecting = role == ROLE_TANK
            && enemy.attacking_party
            && enemy.current_target_guid != Some(me.guid);
        (
            !protecting,
            !enemy.attacking_party,
            !enemy.party_has_threat,
            enemy.guid,
        )
    })
}

fn melee_fight(
    me: &crate::WorldEntity,
    target: &crate::group::PartyEnemyFacts,
    objective: u64,
) -> ActionNode {
    let mut attack = node(
        Action::Attack(target.guid),
        Reason::TankFight,
        650,
        objective,
    );
    if enemy_distance_sq(me, target) > MELEE_RANGE_YD * MELEE_RANGE_YD {
        attack.prerequisites.push(node(
            Action::Move(MoveTarget::Entity(target.guid)),
            Reason::MeleePosition,
            650,
            objective,
        ));
    }
    attack
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

/// Choose the first learned, level-valid combat spell that can cast now or after repairing range or
/// line of sight. Companion and solo loops share this rotation decision.
pub(super) fn combat_spell(
    ctx: &ReducerContext,
    bot: &PlayerbotsBot,
    caster_guid: u64,
    target_guid: u64,
    conditions: &[u8],
) -> Result<Option<u32>, RoleReadError> {
    Ok(rotation_rows(ctx, bot, conditions)?
        .into_iter()
        .find(|row| {
            match crate::actor::cast_readiness(ctx, caster_guid, row.spell_id, target_guid) {
                Ok(()) => true,
                Err(refusal) => matches!(
                    refusal.kind,
                    crate::spell::CastRefusalKind::OutOfRange
                        | crate::spell::CastRefusalKind::NoLineOfSight
                ),
            }
        })
        .map(|row| row.spell_id))
}

fn fight(
    ctx: &ReducerContext,
    bot: &PlayerbotsBot,
    me: &crate::WorldEntity,
    target: &crate::group::PartyEnemyFacts,
    objective: u64,
) -> Result<ActionNode, RoleReadError> {
    if bot.role == ROLE_TANK {
        let fallback = melee_fight(me, target, objective);
        let rows = rotation_rows(ctx, bot, &[cond::ENEMY_ON_ALLY, cond::ALWAYS])?;
        for row in rows {
            if row.condition == cond::ENEMY_ON_ALLY
                && (!target.attacking_party || target.current_target_guid == Some(me.guid))
            {
                continue;
            }
            let mut cast = cast_node(
                ctx,
                me,
                CastAction {
                    target: target.guid,
                    spell: row.spell_id,
                },
                Reason::TankFight,
                Reason::MeleePosition,
                700,
                objective,
            );
            cast.alternatives.push(fallback.clone());
            if cast.readiness != Readiness::Refused {
                return Ok(cast);
            }
        }
        return Ok(fallback);
    }
    if matches!(bot.role, ROLE_HEALER | ROLE_DPS) {
        if let Some(spell) = combat_spell(
            ctx,
            bot,
            me.guid,
            target.guid,
            &[cond::ALWAYS, cond::TANK_ENGAGED],
        )? {
            let mut cast = cast_node(
                ctx,
                me,
                CastAction {
                    target: target.guid,
                    spell,
                },
                Reason::DamageFight,
                Reason::FightPosition,
                650,
                objective,
            );
            cast.alternatives
                .push(node(Action::Hold, Reason::DamageFight, 650, objective));
            return Ok(cast);
        }
    }
    Ok(node(Action::Hold, Reason::DamageFight, 650, objective))
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
    party: &Party,
    row: &PlayerbotsRotation,
    retained: Option<u64>,
) -> Result<Option<u64>, RoleReadError> {
    if row.condition == cond::SELF_MISSING_AURA {
        return Ok(buff_missing(ctx, me.guid, row.spell_id, me.level as u8)?.then_some(me.guid));
    }
    if row.condition != cond::ALLY_MISSING_AURA {
        return Ok(None);
    }
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
    party: &Party,
    objective: u64,
    retained: Option<u64>,
) -> Result<Option<(ActionNode, u64)>, RoleReadError> {
    let rows = rotation_rows(
        ctx,
        bot,
        &[cond::SELF_MISSING_AURA, cond::ALLY_MISSING_AURA],
    )?;
    for row in rows {
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
            300,
            objective,
        );
        cast.alternatives.push(follow(party, me, objective));
        if cast.readiness != Readiness::Refused {
            return Ok(Some((cast, target)));
        }
    }
    Ok(None)
}

pub(super) struct CompanionSelection {
    pub strategy: Strategy,
    pub heal_target: Option<u64>,
    pub fight_target: Option<u64>,
    pub buff_target: Option<u64>,
    pub read_failure: Option<RoleReadError>,
}

struct HealingSelection {
    candidate: Option<ActionNode>,
    target: Option<u64>,
}

fn healing(
    ctx: &ReducerContext,
    bot: &PlayerbotsBot,
    me: &crate::WorldEntity,
    party: &Party,
    personality_heal_at: u8,
    objective: u64,
    retained_target: Option<u64>,
    fallback: &ActionNode,
) -> Result<HealingSelection, RoleReadError> {
    let rows = rotation_rows(ctx, bot, &[cond::ALLY_HP_BELOW_PCT])?;
    let mut selected_target = None;
    for row in rows {
        let heal_at_pct = row.threshold_pct.min(personality_heal_at);
        let Some(target) = wounded_ally(
            party,
            (me.map_id, me.instance_id),
            heal_at_pct,
            retained_target,
        ) else {
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
            Reason::Heal,
            Reason::CastingPosition,
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

#[allow(clippy::too_many_arguments)]
pub(super) fn strategy(
    ctx: &ReducerContext,
    bot: &PlayerbotsBot,
    me: &crate::WorldEntity,
    party: &Party,
    survival_permits_healing: bool,
    objective: u64,
    retained_heal_target: Option<u64>,
    retained_fight_target: Option<u64>,
    retained_buff_target: Option<u64>,
) -> Result<CompanionSelection, RoleReadError> {
    let follow = follow(party, me, objective);
    let personality_heal_at = ctx
        .db
        .pkg_playerbots_personality()
        .by_character()
        .filter(me.guid)
        .next()
        .map_or(u8::MAX, |personality| personality.heal_at_pct);
    let healing = if bot.role == ROLE_HEALER && survival_permits_healing {
        healing(
            ctx,
            bot,
            me,
            party,
            personality_heal_at,
            objective,
            retained_heal_target,
            &follow,
        )?
    } else {
        HealingSelection {
            candidate: None,
            target: None,
        }
    };
    let heal_target = healing.target;
    let fight_target = fight_target(party, me, bot.role, retained_fight_target);
    let mut candidates = Vec::with_capacity(5);
    if let Some(heal) = healing.candidate {
        candidates.push(heal);
    }
    if let Some(target) = fight_target {
        candidates.push(fight(ctx, bot, me, target, objective)?);
    } else if !party.enemies.is_empty() {
        candidates.push(node(Action::Hold, Reason::CrowdControl, 750, objective));
    }
    let (maintenance, read_failure) = if party.enemies.is_empty() {
        match maintenance(ctx, bot, me, party, objective, retained_buff_target) {
            Ok(maintenance) => (maintenance, None),
            Err(RoleReadError::RotationLimit) => return Err(RoleReadError::RotationLimit),
            Err(unavailable) => (None, Some(unavailable)),
        }
    } else {
        (None, None)
    };
    let buff_target = maintenance.as_ref().map(|(_, target)| *target);
    if let Some((buff, _)) = maintenance {
        candidates.push(buff);
    }
    if read_failure.is_some() {
        candidates.push(node(Action::Hold, Reason::RoleUnavailable, 750, objective));
    }
    candidates.push(follow);
    Ok(CompanionSelection {
        strategy: Strategy {
            trigger: Trigger::Always,
            candidates,
            defaults: vec![],
            priority_adjustment: 0,
        },
        heal_target,
        fight_target: fight_target.map(|target| target.guid),
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
